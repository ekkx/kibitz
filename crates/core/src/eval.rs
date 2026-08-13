//! Evaluation score to win probability.
//!
//! Classifying moves by centipawn delta misjudges "+900 -> +700" as a blunder.
//! Always convert to win probability first, then take the difference. Same
//! logistic transform Lichess uses.

use serde::{Deserialize, Serialize};

/// A Stockfish evaluation. Always from the point of view of **the side to move**
/// in that position.
///
/// Mate is not collapsed into a win probability. Collapsing it would make
/// `win_prob(500) = 0.863`, so missing a forced mate and landing on +5 yields a
/// delta of only 0.137 — never a blunder, at any threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Score {
    Cp(i32),
    /// Positive means the side to move is delivering mate. The value is in moves.
    Mate(i32),
}

/// Lichess win-probability coefficient.
pub const WIN_PROB_K: f64 = -0.00368208;

/// Centipawns to win probability (0..1), from the side to move's point of view.
pub fn win_prob(cp: i32) -> f64 {
    1.0 / (1.0 + (WIN_PROB_K * cp as f64).exp())
}

impl Score {
    /// Win probability for display and comparison. Mate saturates to 0.0 / 1.0.
    /// **Do not use this for classification** — `classify` handles mate separately.
    pub fn win_prob(self) -> f64 {
        match self {
            Score::Cp(cp) => win_prob(cp),
            Score::Mate(_) => {
                if self.mating_side() == Some(true) {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    pub fn is_mate(self) -> bool {
        matches!(self, Score::Mate(_))
    }

    /// Whether the side to move is the one delivering mate. `None` for `Cp`.
    ///
    /// `Mate(0)` means the side to move is *already* mated, so it counts as
    /// getting mated, not as mating.
    pub fn mating_side(self) -> Option<bool> {
        match self {
            Score::Cp(_) => None,
            Score::Mate(n) => Some(n > 0),
        }
    }

    /// Flip the point of view — used to read position N+1's evaluation from the
    /// perspective of the side that moved in position N.
    pub fn negate(self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            // `Mate(0)` = the side to move is already mated. Flipped around, that
            // is a mate the *other* side has just delivered. `i32` has no signed
            // zero, so "mate in 0 for the winning side" cannot be encoded; the
            // shortest representable winning mate is used instead. Losing the
            // exact distance is harmless (the game is over), whereas mapping
            // `Mate(0)` to itself would silently turn a delivered checkmate into
            // a loss.
            Score::Mate(0) => Score::Mate(1),
            Score::Mate(n) => Score::Mate(-n),
        }
    }
}

/// Lichess display accuracy.
/// `Accuracy% = 103.1668 * exp(-0.04354 * (WinPct_before - WinPct_after)) - 3.1669`
/// Arguments are win probabilities in 0..1; the percentage conversion happens inside.
pub fn accuracy(win_prob_before: f64, win_prob_after: f64) -> f64 {
    let drop = (win_prob_before - win_prob_after) * 100.0;
    let acc = 103.1668 * (-0.04354 * drop).exp() - 3.1669;
    acc.clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn win_prob_of_zero_is_even() {
        assert!((win_prob(0) - 0.5).abs() < EPS);
    }

    #[test]
    fn win_prob_of_one_pawn() {
        // 1 / (1 + exp(-0.368208))
        assert!((win_prob(100) - 0.591_025_9).abs() < 1e-6);
        assert!(win_prob(100) > 0.5);
        assert!(win_prob(-100) < 0.5);
    }

    #[test]
    fn win_prob_is_symmetric() {
        for cp in [0, 1, 37, 100, 250, 500, 1500, 9000] {
            let sum = win_prob(cp) + win_prob(-cp);
            assert!((sum - 1.0).abs() < 1e-12, "cp={cp} sum={sum}");
        }
    }

    #[test]
    fn win_prob_is_monotonic_and_bounded() {
        let mut prev = win_prob(-10_000);
        for cp in (-9_000..=10_000).step_by(250) {
            let p = win_prob(cp);
            assert!(p >= prev, "not monotonic at {cp}");
            assert!((0.0..=1.0).contains(&p));
            prev = p;
        }
    }

    #[test]
    fn mate_saturates() {
        assert_eq!(Score::Mate(1).win_prob(), 1.0);
        assert_eq!(Score::Mate(7).win_prob(), 1.0);
        assert_eq!(Score::Mate(-1).win_prob(), 0.0);
        // Mate(0): the side to move has already been mated.
        assert_eq!(Score::Mate(0).win_prob(), 0.0);
    }

    #[test]
    fn mating_side_reports_who_delivers() {
        assert_eq!(Score::Cp(500).mating_side(), None);
        assert_eq!(Score::Mate(3).mating_side(), Some(true));
        assert_eq!(Score::Mate(-3).mating_side(), Some(false));
        assert_eq!(Score::Mate(0).mating_side(), Some(false));
    }

    #[test]
    fn negate_flips_point_of_view() {
        assert_eq!(Score::Cp(120).negate(), Score::Cp(-120));
        assert_eq!(Score::Cp(0).negate(), Score::Cp(0));
        assert_eq!(Score::Mate(3).negate(), Score::Mate(-3));
        assert_eq!(Score::Mate(-3).negate(), Score::Mate(3));
    }

    #[test]
    fn negate_of_delivered_mate_is_a_win() {
        // Position N+1 says "I am mated" -> from the mover's view, "I mated".
        let played = Score::Mate(0).negate();
        assert_eq!(played.mating_side(), Some(true));
        assert_eq!(played.win_prob(), 1.0);
    }

    #[test]
    fn negate_is_an_involution_except_for_mate_zero() {
        for s in [
            Score::Cp(0),
            Score::Cp(-42),
            Score::Mate(2),
            Score::Mate(-2),
        ] {
            assert_eq!(s.negate().negate(), s);
        }
    }

    /// Section 3: a score is always from the point of view of the side to move
    /// in that position, and `win_prob_after` is read through `negate()`.
    #[test]
    fn sign_convention_of_section_3() {
        // Position N: white to move, white is a pawn up.
        let before = Score::Cp(100);
        assert!(before.win_prob() > 0.5);

        // White hangs a queen. Position N+1 is black to move and black is winning,
        // so Stockfish reports a large *positive* score there.
        let after_opponent_view = Score::Cp(800);
        let after = after_opponent_view.negate();
        assert_eq!(after, Score::Cp(-800));

        let delta = after.win_prob() - before.win_prob();
        assert!(delta < -0.30, "delta={delta} should read as a blunder");

        // And 1 - p is the same thing as negating, for Cp.
        assert!((after.win_prob() - (1.0 - after_opponent_view.win_prob())).abs() < 1e-12);
    }

    #[test]
    fn accuracy_is_full_when_nothing_is_lost() {
        assert!((accuracy(0.5, 0.5) - 100.0).abs() < 0.01);
        assert!((accuracy(0.83, 0.83) - 100.0).abs() < 0.01);
    }

    #[test]
    fn accuracy_is_clamped() {
        // Improving the win probability cannot exceed 100.
        assert_eq!(accuracy(0.2, 0.9), 100.0);
        // A total collapse cannot go below 0.
        assert_eq!(accuracy(1.0, 0.0), 0.0);
        assert!((0.0..=100.0).contains(&accuracy(0.9, 0.4)));
    }

    #[test]
    fn accuracy_decreases_with_the_loss() {
        let small = accuracy(0.6, 0.55);
        let big = accuracy(0.6, 0.25);
        assert!(small > big);
        // 10 percentage points lost -> ~63.58%.
        assert!((accuracy(0.6, 0.5) - 63.5826).abs() < 0.01);
    }
}
