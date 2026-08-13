//! Move classification.
//!
//! Base thresholds follow Lichess (`lila/modules/analyse/src/main/Advice.scala`).
//! Great / Miss do not exist in Lichess — they are Chess.com and WintrChess
//! inventions — so they are defined here. **These thresholds need tuning against
//! real games**, which is why every one of them is a named constant.

use crate::eval::Score;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    /// Decided by the opening-book stage before the engine. `classify()` never returns it.
    Book,
    /// `!`
    Great,
    Best,
    Excellent,
    Good,
    /// `?!`
    Inaccuracy,
    /// `?`
    Mistake,
    /// `??`
    Blunder,
    /// The best move was mate, and the move played gave the mate up.
    Miss,
}

impl Classification {
    /// Whether this move gets an LLM explanation (see the filter in the design doc).
    pub fn deserves_explanation(self) -> bool {
        matches!(
            self,
            Classification::Blunder
                | Classification::Mistake
                | Classification::Great
                | Classification::Miss
        )
    }

    /// Whether this is a bad move — decides which line the UI replays on the board.
    pub fn is_mistake(self) -> bool {
        matches!(
            self,
            Classification::Inaccuracy
                | Classification::Mistake
                | Classification::Blunder
                | Classification::Miss
        )
    }

    /// NAG-style glyph.
    pub fn glyph(self) -> &'static str {
        match self {
            Classification::Great => "!",
            Classification::Inaccuracy => "?!",
            Classification::Mistake => "?",
            Classification::Blunder => "??",
            _ => "",
        }
    }
}

// ─── Thresholds ─────────────────────────────────────────
// From Lichess: loss in winning chances.
pub const BLUNDER_DELTA: f64 = -0.30;
pub const MISTAKE_DELTA: f64 = -0.20;
pub const INACCURACY_DELTA: f64 = -0.10;
pub const EXCELLENT_DELTA: f64 = -0.02;

// Defined here. Start conservative; loosen against real data.
/// Great: the gap between the best and second-best move's win probability that
/// makes a move "the only move".
pub const GREAT_GAP: f64 = 0.10;
/// Bounds outside which the position counts as already decided. No Great is
/// awarded there.
pub const DECIDED_HIGH: f64 = 0.90;
pub const DECIDED_LOW: f64 = 0.10;

/// Input to `classify()`. The caller is responsible for honoring the sign and
/// point-of-view convention from the design doc.
#[derive(Debug, Clone)]
pub struct ClassifyInput {
    /// Evaluation of the best move in position N, side-to-move's view.
    pub best: Score,
    /// Evaluation of the second-best move in position N, side-to-move's view.
    /// `None` when MultiPV returned only one move.
    pub second: Option<Score>,
    /// Evaluation of the move actually played: position N+1's score, already
    /// negated into the mover's point of view.
    pub played: Score,
    /// Rank of the played move within the MultiPV candidates, `None` if absent.
    pub played_rank: Option<usize>,
}

/// Classification result. The intermediate values are returned too, so
/// thresholds can be tuned against logged data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassifyOutput {
    pub classification: Classification,
    pub win_prob_before: f64,
    pub win_prob_after: f64,
    pub delta: f64,
}

/// Evaluated top to bottom; the first matching rule wins.
///
/// Order: Miss -> Great -> Blunder -> Mistake -> Inaccuracy
///        -> Best -> Excellent -> Good
pub fn classify(input: &ClassifyInput) -> ClassifyOutput {
    let win_prob_before = input.best.win_prob();
    let win_prob_after = input.played.win_prob();
    let delta = win_prob_after - win_prob_before;

    let classification = decide(input, win_prob_before, delta);

    ClassifyOutput {
        classification,
        win_prob_before,
        win_prob_after,
        delta,
    }
}

/// `true` when the position was already decided, so no Great may be awarded.
fn undecided(win_prob_before: f64) -> bool {
    win_prob_before > DECIDED_LOW && win_prob_before < DECIDED_HIGH
}

/// The move played walks into a forced mate that the best move avoided.
///
/// This has to be checked explicitly. `Score::win_prob()` saturates mate to 0.0,
/// so in a position that was already grim (say `win_prob_before` = 0.15) the
/// delta of walking into mate is only -0.15 — an inaccuracy at best. Exactly the
/// structural problem §8.1 describes, mirrored onto the losing side.
///
/// If the best move was also getting mated there is nothing to blame the move
/// for, so the delta decides as usual.
fn walks_into_mate(input: &ClassifyInput) -> bool {
    input.played.mating_side() == Some(false) && input.best.mating_side() != Some(false)
}

/// The played move is the only move that forces a mate still a move away.
///
/// The mirror image of `Miss`, and defined the same way: by the presence of
/// mate, never by a win-probability gap. `Score::win_prob()` saturates mate to
/// 1.0, so against a second-best candidate of +9.00 the gap is only about 0.035
/// and would fail `GREAT_GAP` — the structural problem of §8.1, pointed at the
/// winning side this time. Seeing a mate in two where every other move merely
/// wins is precisely the case worth marking, so `GREAT_GAP` is not consulted on
/// this path.
///
/// A second candidate has to exist: with a single legal move the move is
/// forced, and calling a forced move "!" is wrong. And if the second candidate
/// mates too (mate in 2 against mate in 3), this was not the only move that
/// mates.
///
/// The mate has to be **mate in two or more**. A move scoring `Mate(1)` *is*
/// the checkmate; playing it is not an insight, and by definition it was not
/// hard to see. Three reasons, so this is not undone later as an off-by-one:
///
/// 1. Annotation convention puts `!` on the move that sets the mate up, not on
///    the mate itself. `16.Qb8+` in the Opera Game scores `Mate(2)` — the
///    player had to see the queen sacrifice and read two moves ahead, which is
///    the "only move that does the job" `Great` exists for. `17.Rd8#` scores
///    `Mate(1)` and is just the combination finishing.
/// 2. `Great` implies `deserves_explanation()`, so without this we spend an LLM
///    call producing "Rd8 is checkmate", which tells the reader nothing they
///    cannot see on the board.
/// 3. It is not a threshold that needs tuning. Mate in 1 is the move itself;
///    mate in 2 is the first position where something had to be found.
fn only_mate(input: &ClassifyInput) -> bool {
    // `Mate(n)` with `n >= 2` rather than `is_mate()`: it has to be the side to
    // move delivering the mate, not being mated, and not on this very move.
    matches!(input.best, Score::Mate(n) if n >= 2)
        && match input.second {
            Some(second) => second.mating_side() != Some(true),
            None => false,
        }
}

/// The rule table itself. Evaluated top to bottom; the first match wins.
fn decide(input: &ClassifyInput, win_prob_before: f64, delta: f64) -> Classification {
    // 1. Miss — the best move was mate and the played move is no longer a mate
    //    at all. Deliberately defined by the presence of mate rather than by win
    //    probability, which would double-fire with Blunder / Mistake.
    if input.best.mating_side() == Some(true) && !input.played.is_mate() {
        return Classification::Miss;
    }

    let is_best = input.played_rank == Some(0);
    // Gap between the best and the second-best candidate: "the only move".
    let only_move = match input.second {
        Some(second) => input.best.win_prob() - second.win_prob() >= GREAT_GAP,
        // MultiPV returned a single move: there is no gap to measure, so no
        // claim of "only move" can be made.
        None => false,
    };
    // `delta >= EXCELLENT_DELTA` is what makes this "the only move that *held*".
    // Ranking first is not enough: position N and position N+1 are separate
    // searches, so a move can rank first and still show a large negative delta —
    // either because the search disagreed with itself, or because every move
    // loses and this one merely loses least. Awarding "!" alongside an accuracy
    // of 35 is a contradiction the user would be right to distrust.
    // 2. Great — the only move that held the position.
    if is_best && only_move && undecided(win_prob_before) && delta >= EXCELLENT_DELTA {
        return Classification::Great;
    }

    // 2b. Great — the only move that mates. A second route, not a loosening of
    //     the one above: `undecided` still guards that one, and it has to, or
    //     any flashy move in a won position would look special. A forced mate
    //     is the one thing that stays worth pointing at once the position is
    //     decided, because the mate is *why* it is decided. `delta` is still
    //     checked for the reason above — position N and position N+1 are
    //     separate searches, and a first-ranked move whose own search does not
    //     agree it mates should not be given "!".
    if is_best && only_mate(input) && delta >= EXCELLENT_DELTA {
        return Classification::Great;
    }

    // 3. Blunder.
    if delta <= BLUNDER_DELTA || walks_into_mate(input) {
        return Classification::Blunder;
    }

    // 4. Mistake.
    if delta <= MISTAKE_DELTA {
        return Classification::Mistake;
    }

    // 5. Inaccuracy.
    if delta <= INACCURACY_DELTA {
        return Classification::Inaccuracy;
    }

    // 6. Best.
    if is_best {
        return Classification::Best;
    }

    // 7. Excellent.
    if delta > EXCELLENT_DELTA {
        return Classification::Excellent;
    }

    // 8. Good.
    Classification::Good
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::win_prob;

    /// Centipawns whose win probability is `p`. Not an integer in general.
    fn inverse(p: f64) -> f64 {
        (p / (1.0 - p)).ln() / -crate::eval::WIN_PROB_K
    }

    /// Score whose win probability is `p`, to within integer-centipawn rounding.
    fn at(p: f64) -> Score {
        Score::Cp(inverse(p).round() as i32)
    }

    /// Score whose win probability is `p` or a hair below. Integer centipawns
    /// cannot land exactly on a threshold, so boundary tests approach it from a
    /// chosen side.
    fn at_most(p: f64) -> Score {
        Score::Cp(inverse(p).floor() as i32)
    }

    /// Score whose win probability is `p` or a hair above.
    fn at_least(p: f64) -> Score {
        Score::Cp(inverse(p).ceil() as i32)
    }

    fn input(best: Score, played: Score) -> ClassifyInput {
        ClassifyInput {
            best,
            second: None,
            played,
            played_rank: None,
        }
    }

    fn class(input: &ClassifyInput) -> Classification {
        classify(input).classification
    }

    #[test]
    fn delta_follows_the_sign_convention() {
        // A move that makes things worse has a negative delta.
        let out = classify(&input(Score::Cp(50), Score::Cp(-300)));
        assert!(out.delta < 0.0);
        assert!((out.win_prob_before - win_prob(50)).abs() < 1e-12);
        assert!((out.win_prob_after - win_prob(-300)).abs() < 1e-12);
        assert!((out.delta - (out.win_prob_after - out.win_prob_before)).abs() < 1e-12);
    }

    #[test]
    fn miss_when_the_mate_is_given_up() {
        let mut i = input(Score::Mate(3), Score::Cp(500));
        // Even as the top-ranked move by the caller's bookkeeping.
        i.played_rank = Some(0);
        assert_eq!(class(&i), Classification::Miss);
    }

    #[test]
    fn slower_mate_is_not_a_miss() {
        // Mate in 5 instead of mate in 2 is still mate: no Miss, no penalty.
        let mut i = input(Score::Mate(2), Score::Mate(5));
        i.played_rank = Some(3);
        assert_eq!(class(&i), Classification::Excellent);
    }

    #[test]
    fn delivering_the_mate_is_best() {
        // Position N+1 reports Mate(0) — the opponent is mated. Negated, that is
        // the mover's win.
        let mut i = input(Score::Mate(1), Score::Mate(0).negate());
        i.played_rank = Some(0);
        assert_eq!(class(&i), Classification::Best);
        assert_eq!(classify(&i).delta, 0.0);
    }

    #[test]
    fn miss_does_not_fire_when_the_best_move_was_only_defending_a_mate() {
        // best = Mate(-4): the side to move is the one getting mated, so there
        // was no mate to miss.
        let i = input(Score::Mate(-4), Score::Cp(-800));
        assert_ne!(class(&i), Classification::Miss);
    }

    #[test]
    fn walking_into_mate_is_a_blunder() {
        // The saturated win probability alone would call this an inaccuracy:
        // before = 0.15, after = 0.0, delta = -0.15.
        let i = input(at(0.15), Score::Mate(-2));
        let out = classify(&i);
        assert!(out.delta > BLUNDER_DELTA, "delta={} ", out.delta);
        assert_eq!(out.classification, Classification::Blunder);
    }

    #[test]
    fn walking_into_mate_from_an_equal_position_is_a_blunder() {
        let i = input(Score::Cp(0), Score::Mate(-1));
        assert_eq!(class(&i), Classification::Blunder);
    }

    #[test]
    fn already_lost_to_mate_is_not_blamed_on_the_move() {
        // Mate was unavoidable: the best move was mated too.
        let mut i = input(Score::Mate(-5), Score::Mate(-3));
        i.played_rank = Some(0);
        assert_eq!(class(&i), Classification::Best);

        i.played_rank = Some(2);
        assert_eq!(class(&i), Classification::Excellent);
    }

    #[test]
    fn giving_up_a_mate_and_getting_mated_is_a_blunder() {
        // `Miss` is defined as "no longer a mate at all", so this heavier case
        // falls through to the mate-aware Blunder rule rather than to Miss.
        let i = input(Score::Mate(2), Score::Mate(-3));
        assert_eq!(class(&i), Classification::Blunder);
    }

    #[test]
    fn great_is_the_only_move() {
        let i = ClassifyInput {
            best: at(0.55),
            second: Some(at(0.40)),
            played: at(0.55),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Great);
    }

    #[test]
    fn great_needs_the_gap() {
        let i = ClassifyInput {
            best: at(0.55),
            second: Some(at(0.50)),
            played: at(0.55),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn great_needs_a_second_candidate() {
        let i = ClassifyInput {
            best: at(0.55),
            second: None,
            played: at(0.55),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn great_is_not_awarded_in_a_decided_position() {
        // Winning by a mile already: nothing here was held together by one move.
        let i = ClassifyInput {
            best: at(0.95),
            second: Some(at(0.80)),
            played: at(0.95),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn blunder_mistake_inaccuracy_by_delta() {
        assert_eq!(
            class(&input(at(0.80), at(0.40))),
            Classification::Blunder,
            "-0.40"
        );
        assert_eq!(
            class(&input(at(0.80), at(0.55))),
            Classification::Mistake,
            "-0.25"
        );
        assert_eq!(
            class(&input(at(0.80), at(0.65))),
            Classification::Inaccuracy,
            "-0.15"
        );
    }

    /// `win_prob(0)` is exactly 0.5, so `delta` here is exactly `p - 0.5`.
    #[test]
    fn thresholds_are_inclusive_on_the_worse_side() {
        // Each pair straddles a threshold by less than a thousandth.
        let cases = [
            (
                0.20,
                BLUNDER_DELTA,
                Classification::Blunder,
                Classification::Mistake,
            ),
            (
                0.30,
                MISTAKE_DELTA,
                Classification::Mistake,
                Classification::Inaccuracy,
            ),
            (
                0.40,
                INACCURACY_DELTA,
                Classification::Inaccuracy,
                Classification::Good,
            ),
        ];
        for (p, threshold, worse, better) in cases {
            let out = classify(&input(Score::Cp(0), at_most(p)));
            assert!(out.delta <= threshold, "delta={} <= {threshold}", out.delta);
            assert!((out.delta - threshold).abs() < 1e-3);
            assert_eq!(out.classification, worse, "at or below {p}");

            let out = classify(&input(Score::Cp(0), at_least(p)));
            assert!(out.delta >= threshold, "delta={} >= {threshold}", out.delta);
            assert!((out.delta - threshold).abs() < 1e-3);
            assert_eq!(out.classification, better, "at or above {p}");
        }
    }

    #[test]
    fn excellent_boundary_is_strict() {
        // delta > -0.02 is Excellent; delta == -0.02 is not.
        let out = classify(&input(Score::Cp(0), at_most(0.48)));
        assert!(out.delta <= EXCELLENT_DELTA);
        assert_eq!(out.classification, Classification::Good);

        let out = classify(&input(Score::Cp(0), at_least(0.48)));
        assert!(out.delta >= EXCELLENT_DELTA);
        assert_eq!(out.classification, Classification::Excellent);
    }

    #[test]
    fn great_gap_boundary() {
        // Gap exactly at GREAT_GAP counts (the rule is `>=`).
        let exactly = ClassifyInput {
            best: at_least(0.60),
            second: Some(at_most(0.50)),
            played: at(0.60),
            played_rank: Some(0),
        };
        assert!(exactly.best.win_prob() - exactly.second.unwrap().win_prob() >= GREAT_GAP);
        assert_eq!(class(&exactly), Classification::Great);

        let just_under = ClassifyInput {
            second: Some(at_least(0.50)),
            best: at_most(0.60),
            ..exactly.clone()
        };
        assert!(just_under.best.win_prob() - just_under.second.unwrap().win_prob() < GREAT_GAP);
        assert_eq!(class(&just_under), Classification::Best);
    }

    #[test]
    fn decided_bounds_are_strict() {
        // win_prob_before exactly at DECIDED_HIGH is already "decided".
        let mut i = ClassifyInput {
            best: at_least(DECIDED_HIGH),
            second: Some(at(0.70)),
            played: at_least(DECIDED_HIGH),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);

        // A hair below, the same move is great.
        i.best = at_most(DECIDED_HIGH);
        i.played = i.best;
        assert_eq!(class(&i), Classification::Great);
    }

    #[test]
    fn great_for_the_only_move_that_mates() {
        // Decided position — win_prob_before is 1.0 — so the ordinary Great path
        // is closed by `undecided`. The mate route opens it anyway.
        let i = ClassifyInput {
            best: Score::Mate(2),
            second: Some(Score::Cp(405)),
            played: Score::Mate(2),
            played_rank: Some(0),
        };
        let out = classify(&i);
        assert!(!undecided(out.win_prob_before));
        assert_eq!(out.classification, Classification::Great);
    }

    #[test]
    fn the_mate_route_starts_at_mate_in_two() {
        // Identical inputs either side of the boundary.
        let mate_in_one = ClassifyInput {
            best: Score::Mate(1),
            second: Some(Score::Cp(900)),
            played: Score::Mate(0).negate(),
            played_rank: Some(0),
        };
        // The move *is* the checkmate: nothing was found, so no "!".
        assert_eq!(class(&mate_in_one), Classification::Best);

        let mate_in_two = ClassifyInput {
            best: Score::Mate(2),
            played: Score::Mate(2),
            ..mate_in_one
        };
        // One move further out, something had to be seen.
        assert_eq!(class(&mate_in_two), Classification::Great);
    }

    #[test]
    fn the_mate_route_does_not_go_through_the_gap() {
        // The §8.1 case: the alternative is +9.00, so the saturated gap is far
        // below GREAT_GAP. Finding mate where the alternative merely wins is
        // exactly what should be marked, so the gap must not be consulted.
        let i = ClassifyInput {
            best: Score::Mate(2),
            second: Some(Score::Cp(900)),
            played: Score::Mate(2),
            played_rank: Some(0),
        };
        assert!(i.best.win_prob() - i.second.unwrap().win_prob() < GREAT_GAP);
        assert_eq!(class(&i), Classification::Great);
    }

    #[test]
    fn a_slower_second_mate_is_not_the_only_move_that_mates() {
        // Mate in 2 against mate in 3: another move mates too, so nothing was
        // found that only this move finds.
        let i = ClassifyInput {
            best: Score::Mate(2),
            second: Some(Score::Mate(3)),
            played: Score::Mate(2),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn the_only_legal_move_that_mates_is_not_great() {
        // MultiPV returned one move: it was forced, not found.
        let i = ClassifyInput {
            best: Score::Mate(2),
            second: None,
            played: Score::Mate(2),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn being_mated_is_not_great() {
        // The mate on the board belongs to the opponent. Holding out longest is
        // not "!".
        let i = ClassifyInput {
            best: Score::Mate(-2),
            second: Some(Score::Cp(-900)),
            played: Score::Mate(-2),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn the_mate_route_still_wants_the_delta() {
        // Position N ranked this first and called it mate; position N+1 says the
        // mover is the one getting mated. The two searches disagree, so no "!".
        // (Rule 1 does not catch this: Miss needs the played move to be no mate
        // at all, and this one is a mate — so the delta guard is what stops it.)
        let i = ClassifyInput {
            best: Score::Mate(2),
            second: Some(Score::Cp(405)),
            played: Score::Mate(-1),
            played_rank: Some(0),
        };
        assert!(only_mate(&i));
        assert!(classify(&i).delta < EXCELLENT_DELTA);
        assert_eq!(class(&i), Classification::Blunder);
    }

    #[test]
    fn the_undecided_guard_on_the_ordinary_great_path_is_untouched() {
        // No mate anywhere: a decided position still gets no Great.
        let i = ClassifyInput {
            best: at(0.95),
            second: Some(at(0.60)),
            played: at(0.95),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn best_wins_over_excellent() {
        let mut i = input(at(0.50), at(0.50));
        i.played_rank = Some(0);
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn excellent_for_a_near_free_alternative() {
        let mut i = input(at(0.50), at(0.495));
        i.played_rank = Some(2);
        assert_eq!(class(&i), Classification::Excellent);
    }

    #[test]
    fn good_for_a_small_but_real_loss() {
        let mut i = input(at(0.50), at(0.45));
        i.played_rank = Some(4);
        assert_eq!(class(&i), Classification::Good);
    }

    /// One concrete input per classification, checked in one place so the whole
    /// rule table is exercised together.
    #[test]
    fn every_classification_is_produced() {
        use Classification::*;

        let only_move = ClassifyInput {
            best: at(0.60),
            second: Some(at(0.45)),
            played: at(0.60),
            played_rank: Some(0),
        };

        let cases: [(Classification, ClassifyInput); 8] = [
            (Miss, input(Score::Mate(3), Score::Cp(500))),
            (Great, only_move),
            (Blunder, input(at(0.80), at(0.40))),
            (Mistake, input(at(0.80), at(0.55))),
            (Inaccuracy, input(at(0.80), at(0.65))),
            (
                Best,
                ClassifyInput {
                    played_rank: Some(0),
                    ..input(at(0.50), at(0.50))
                },
            ),
            (
                Excellent,
                ClassifyInput {
                    played_rank: Some(1),
                    ..input(at(0.50), at(0.495))
                },
            ),
            (
                Good,
                ClassifyInput {
                    played_rank: Some(1),
                    ..input(at(0.50), at(0.45))
                },
            ),
        ];

        for (expected, i) in cases {
            let out = classify(&i);
            assert_eq!(out.classification, expected, "input: {i:?} -> {out:?}");
            // Book is decided before classify() runs and is never returned.
            assert_ne!(out.classification, Book);
        }
    }

    #[test]
    fn centipawn_deltas_high_up_are_not_blunders() {
        // The motivating case from §8.1: +900 -> +700 is not a blunder.
        let out = classify(&input(Score::Cp(900), Score::Cp(700)));
        assert!(out.delta > INACCURACY_DELTA, "delta={}", out.delta);
        assert_eq!(out.classification, Classification::Good);
    }
}
