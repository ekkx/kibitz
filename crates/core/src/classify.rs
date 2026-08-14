//! Move classification.
//!
//! `Blunder` and `Mistake` follow Lichess
//! (`lila/modules/analyse/src/main/Advice.scala`). `Inaccuracy` and `Excellent`
//! started there and have since been recalibrated against club-player games —
//! the reasoning, and the measurements behind it, are in the threshold block
//! below. Great / Miss do not exist in Lichess — they are Chess.com and
//! WintrChess inventions — so they are defined here. **These thresholds are
//! calibration, not law**, which is why every one of them is a named constant.

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
// Loss in winning chances. Blunder and Mistake are Lichess's numbers.
// Inaccuracy and Excellent are **not** — see the note below.
pub const BLUNDER_DELTA: f64 = -0.30;
pub const MISTAKE_DELTA: f64 = -0.20;

// ─── Where this departs from Lichess ────────────────────
//
// Lichess uses -0.10 and -0.02, which leaves `Good` a band 0.08 wide. Measured
// on 40 rated Lichess games between players in the 1200-1600 band (mean Elo
// 1440, 2639 classified moves, depth 12, no opening book), that band was the
// single largest bucket in the table: 29.3% of every move played. It ran from
// "indistinguishable from the best move" to "clearly an error", under one label
// that reads as praise. Both edges moved, for two different reasons.
//
// **`EXCELLENT_DELTA`: -0.02 -> -0.03. This one is forced by measurement.**
// Re-running 16 of those games at depth 18 and diffing per move, the depth-12
// delta agrees with the depth-18 delta to a median of 0.007, with a 90th
// percentile of 0.028 in this region. A boundary at -0.02 therefore sits
// *inside* the error bar of the number being compared against it: the tool
// cannot reliably tell -0.015 from -0.025, so splitting Excellent from Good
// there asserts precision the search does not have. -0.03 puts the boundary just
// outside the noise. It is free: depth-12-vs-18 verdict agreement is identical
// at either value (776/1039 moves), so this buys the distinction at no cost in
// stability. 183 moves — 6.9% of the sample — move from `Good` to `Excellent`.
//
// The noise is not a depth artefact and cannot be searched away. Running the
// *same* 40 games twice at depth 12 with the *same* configuration reproduces
// only 3% of deltas exactly, with a median difference of 0.0070 and a p90 of
// 0.0218 — statistically indistinguishable from the depth-12-vs-18 numbers
// above. This is the multi-threaded search non-determinism already documented on
// `DEFAULT_MULTIPV` in `crates/engine/src/lib.rs`, and it puts a hard floor
// under how fine any delta-based boundary can usefully be.
//
// **`INACCURACY_DELTA`: -0.10 -> -0.07. This one is a judgement call, and is
// recorded as one.** The delta distribution decays smoothly across this whole
// region with no knee anywhere in it, so the data does not contain a "correct"
// number and no amount of further measurement will produce one. What the data
// does establish is that the moves being relabelled are real and not artefacts:
// of the 79 sampled moves that lost 6-10 points of win probability at depth 12,
// the median delta at depth 18 is -0.068 — unmoved — and 71 of the 79 still lose
// at least 4 points. Depth is not manufacturing them. It also establishes that
// they do not behave like good moves: 66% of them are not in the engine's top
// five candidates at all, which is the profile of an `Inaccuracy` (72% outside
// the top five) and not of an `Excellent` (15%). Calling a move that loses 8
// points of win probability, and that the engine never considered, "Good" is the
// defect the user reported. -0.07 splits the old band roughly in half and costs
// about two points of depth-12-vs-18 verdict agreement (74.7% -> 72.6%), which
// is the price of drawing a finer distinction in a denser part of the
// distribution rather than a sign that the line is in the wrong place. Judge
// that 72.6% against the 77.4% two identical depth-12 runs score against each
// other, not against 100%: run-to-run noise, not the thresholds, is what sets
// the ceiling. Under the new pair that run-to-run figure is 78.5%, slightly
// better than the 77.4% the old pair scored, so this does not make verdicts
// jitterier.
//
// Net effect on the sample: `Good` 29.3% -> 17.1%, `Excellent` 22.6% -> 29.5%,
// `Inaccuracy` 7.2% -> 12.5%. `Mistake`, `Blunder` and `Miss` are untouched —
// nothing here changes what counts as a serious error, only what stops counting
// as a good move. Both numbers stay named constants because both are still
// calibration, not law.
pub const INACCURACY_DELTA: f64 = -0.07;
pub const EXCELLENT_DELTA: f64 = -0.03;

// ─── Great ──────────────────────────────────────────────
//
// **`GREAT_GAP`: 0.10 -> 0.30, measured.** The corpus is a fresh draw by the
// same recipe as the one above — the Lichess open database for 2015-01, rated
// classical, both players 1200-1600, at least 40 plies, first 40 games in file
// order; mean Elo 1458, 3,286 classified moves, depth 12, `DEFAULT_MULTIPV`, no
// opening book. It is not the identical 40 games (those were not kept), which is
// why the rates below differ in the second decimal from the ones recorded
// there. At 0.10, `Great` fired on **6.27% of all moves** — 5.15 a game, a
// median of 4, and only 2 of 40 games without one. "The only move that held the
// position" cannot be something that happens four times a game; at that rate the
// mark is wallpaper and stops carrying information.
//
// The split first, because the two routes needed separate answers. Route A (the
// win-probability gap) accounted for 193 of the 206, or 5.87% of all moves.
// Route B (the only move that mates) accounted for 13 — 0.40%, about one game in
// three, and every one of them a move route A could not reach. **Route B is left
// exactly as it was**: it is already rare, and it is the only thing that marks
// the move a human annotator would star first.
//
// Where route A's qualifying moves sat: the gap distribution has its mode
// against the old threshold. 28.5% of them fell in the single bucket 0.10-0.15
// — p25 = 0.138, median 0.248 — so the boundary was drawn through the densest
// part of the distribution. The gap is as noisy as everything else here: two
// identical depth-12 runs of this corpus put the same move's gap a median of
// 0.0045 apart, with a p90 of 0.018, which is the same order as the delta noise
// recorded above and the same reason nothing finer than about 0.02 means
// anything. At 0.10 that put **70 of the 193 awarded moves (36%) within one
// noise unit of the line**; at 0.30 it is 17 of 70 (24%).
//
// The rate against the gap, route A only:
//
// | gap | 0.10 | 0.15 | 0.20 | 0.25 | 0.30 | 0.35 | 0.40 |
// |---|---|---|---|---|---|---|---|
// | % of moves | 5.87 | 4.20 | 3.62 | 2.80 | 2.13 | 1.55 | 1.10 |
// | per game | 4.83 | 3.45 | 2.98 | 2.30 | 1.75 | 1.27 | 0.90 |
//
// A smooth curve with no knee, so the data does not choose for us. **0.30 is
// chosen because it is `-BLUNDER_DELTA`**, which turns the rule into a statement
// the rest of this table already makes: the second-best move, had it been
// played, would itself have been a `Blunder` (its delta is exactly `-gap`). That
// is what "any other move would have collapsed the position" has to mean if the
// two labels are to be read together, and it introduces no number of its own.
// The old 0.10 was below even `INACCURACY_DELTA`'s 0.07-to-0.20 band — it asked
// for less than a mistake's worth of collapse.
//
// Result on the corpus: 6.27% -> **2.53%**, 5.15 -> 2.08 a game, median 4 -> 1,
// and games with no `Great` at all 2 of 40 -> 7 of 40. For scale, the same
// corpus produced `Blunder` at 3.41% and `Mistake` at 1.98%: the mirror claim is
// now about as rare as the thing it mirrors, which it was not before. The mean
// is dragged above the median by a single 137-ply game — a pawn race and a
// queen-versus-pawn ending, where "the only move that holds" is the literal
// truth 15 times over. Half the games get one or none.
//
// **Stability is not the argument, and is not damaged either.** Running the
// same 40 games twice at depth 12 agrees on 79.9% of verdicts overall — the
// ceiling described above. Of the moves marked `Great` in the first run, 94.7%
// are `Great` again in the second at the old gap and 90.4% at the new one, so
// this mark is markedly more reproducible than verdicts in general at either
// setting, and raising the threshold does not buy reproducibility: the handful
// of moves sitting on the line is roughly constant in absolute terms (11 vs 8)
// while the population it is measured against shrinks by two thirds. The
// argument for moving the line is the rate, not the noise.
//
// **What this cannot fix, and does not pretend to.** 69% of the moves that
// qualify at 0.30 are captures, against 36.5% of all rank-0 moves — and the
// proportion is the same at 0.10 and at 0.40. Most of what the rule finds is
// forced recaptures, which are "the only move" in exactly the arithmetic sense
// and were never hard to see. Nothing in win probability separates a recapture
// from a move that had to be found; `see.rs` could, but classification stopped
// reading SEE when `Brilliant` was removed and this is not a good enough reason
// to bring it back. The threshold therefore controls how often the mark appears,
// not how deserved it is, and that limit is the honest reason not to chase an
// even lower rate by tightening this number further.
//
// **`DECIDED_LOW` and `DECIDED_HIGH` stay at 0.10 / 0.90, having been measured
// too.** `DECIDED_LOW` never fires and provably cannot: the gap is at most
// `best.win_prob()`, which is `win_prob_before`, so any move clearing `GREAT_GAP`
// is already in a position worth at least 0.30. It is kept as documentation of
// the intent, not as a live check. `DECIDED_HIGH` does real work — it blocked 22
// of the 92 moves that otherwise cleared the new gap, a quarter of them. Tighter
// bands were measured and rejected: (0.20, 0.80) gives 2.25% and (0.30, 0.70)
// gives 1.86%, but a 0.30 collapse from 0.80 leaves the mover at 0.50, which is
// a won game thrown away rather than a case of nothing being at stake. 0.90 is
// where "already decided" actually starts.
//
// **The cost, stated plainly: `testdata/opera_game.pgn` goes from 6 `Great`s to
// 1** on the run this was measured on, and from 4 to 2 on a second depth-12 run
// of the same game — the move that moves is 11.Bxb5+, whose gap comes out 0.296
// once and 0.337 the next time, which is what sitting on a boundary looks like.
// 16.Qb8+ survives either way, by route B. 10.Nxb5 — Morphy's knight sacrifice,
// and a move the design doc has cited as a landmark — does not: its gap is
// 0.137-0.149, because win probability saturates (§8.1), so from 0.76 the
// alternatives still lead to 0.62 and in this tool's own currency missing it
// costs 14 points, an inaccuracy rather than a collapse. That is a real loss of
// a satisfying mark, and it is accepted rather than worked
// around: what made those moves special is that they were sacrifices, and
// §8.3's `Brilliant` note already settled that the sacrifice belongs in the
// explanation text and not in the glyph. A gap low enough to keep them is a gap
// that also keeps 4.83 marks a game.
/// Great: the gap between the best and second-best move's win probability that
/// makes a move "the only move" — i.e. how badly the runner-up would have gone
/// wrong. Equal to `-BLUNDER_DELTA` by construction; see the block above.
pub const GREAT_GAP: f64 = -BLUNDER_DELTA;
/// Bounds outside which the position counts as already decided. No Great is
/// awarded there. `DECIDED_LOW` cannot fire while `GREAT_GAP` exceeds it — see
/// the block above.
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

    /// An "only move": played, ranked first, with the runner-up comfortably
    /// more than `GREAT_GAP` worse. Derived from the constant rather than
    /// written out, for the same reason the delta boundaries are — `GREAT_GAP`
    /// is calibration (see the threshold block), and a test that spelled the
    /// number out would quietly become an assertion about the old one.
    fn only_move_at(p: f64) -> ClassifyInput {
        ClassifyInput {
            best: at(p),
            second: Some(at(p - GREAT_GAP - 0.02)),
            played: at(p),
            played_rank: Some(0),
        }
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
        assert_eq!(class(&only_move_at(0.55)), Classification::Great);
    }

    #[test]
    fn great_needs_the_gap() {
        // The runner-up is only half the required gap behind: a real difference,
        // but not the collapse `Great` claims.
        let i = ClassifyInput {
            second: Some(at(0.55 - GREAT_GAP / 2.0)),
            ..only_move_at(0.55)
        };
        assert_eq!(class(&i), Classification::Best);
    }

    /// The gap is what a `Blunder` costs, and deliberately so: the rule says the
    /// runner-up would itself have been one. If `BLUNDER_DELTA` ever moves, this
    /// moves with it.
    #[test]
    fn the_great_gap_is_a_blunder_sized_collapse() {
        assert_eq!(GREAT_GAP, -BLUNDER_DELTA);

        let i = only_move_at(0.60);
        let second = i.second.unwrap();
        // Playing the runner-up instead is what the gap describes, and the table
        // has to agree it is a blunder.
        let alternative = ClassifyInput {
            played: second,
            played_rank: Some(1),
            ..i.clone()
        };
        assert_eq!(class(&i), Classification::Great);
        assert_eq!(class(&alternative), Classification::Blunder);
    }

    #[test]
    fn great_needs_a_second_candidate() {
        let i = ClassifyInput {
            second: None,
            ..only_move_at(0.55)
        };
        assert_eq!(class(&i), Classification::Best);
    }

    #[test]
    fn great_is_not_awarded_in_a_decided_position() {
        // Winning by a mile already: nothing here was held together by one move.
        let i = only_move_at(0.95);
        assert!(i.best.win_prob() > DECIDED_HIGH);
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

    /// `win_prob(0)` is exactly 0.5, so `delta` here is exactly `p - 0.5` — which
    /// is why the probability to test at is derived from the threshold rather
    /// than written down beside it. These constants are calibration (see the
    /// threshold block), so a test that hardcoded both would have to be edited
    /// every time one is retuned, and would be asserting the old number.
    #[test]
    fn thresholds_are_inclusive_on_the_worse_side() {
        // Each pair straddles a threshold by less than a thousandth.
        let cases = [
            (
                BLUNDER_DELTA,
                Classification::Blunder,
                Classification::Mistake,
            ),
            (
                MISTAKE_DELTA,
                Classification::Mistake,
                Classification::Inaccuracy,
            ),
            (
                INACCURACY_DELTA,
                Classification::Inaccuracy,
                Classification::Good,
            ),
        ];
        for (threshold, worse, better) in cases {
            let p = 0.5 + threshold;
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
        // delta > EXCELLENT_DELTA is Excellent; delta == EXCELLENT_DELTA is not.
        // Same reason as above for deriving the probability from the constant.
        let p = 0.5 + EXCELLENT_DELTA;
        let out = classify(&input(Score::Cp(0), at_most(p)));
        assert!(out.delta <= EXCELLENT_DELTA);
        assert_eq!(out.classification, Classification::Good);

        let out = classify(&input(Score::Cp(0), at_least(p)));
        assert!(out.delta >= EXCELLENT_DELTA);
        assert_eq!(out.classification, Classification::Excellent);
    }

    #[test]
    fn great_gap_boundary() {
        // Gap exactly at GREAT_GAP counts (the rule is `>=`). Both sides are
        // derived from the constant; `p` only has to leave room for the gap
        // inside the undecided band.
        let p = 0.60;
        let exactly = ClassifyInput {
            best: at_least(p),
            second: Some(at_most(p - GREAT_GAP)),
            played: at_least(p),
            played_rank: Some(0),
        };
        assert!(exactly.best.win_prob() - exactly.second.unwrap().win_prob() >= GREAT_GAP);
        assert_eq!(class(&exactly), Classification::Great);

        let just_under = ClassifyInput {
            best: at_most(p),
            second: Some(at_least(p - GREAT_GAP)),
            played: at_most(p),
            ..exactly.clone()
        };
        assert!(just_under.best.win_prob() - just_under.second.unwrap().win_prob() < GREAT_GAP);
        assert_eq!(class(&just_under), Classification::Best);
    }

    #[test]
    fn decided_bounds_are_strict() {
        // win_prob_before exactly at DECIDED_HIGH is already "decided". The
        // runner-up sits a full gap below it, so only the band decides.
        let mut i = ClassifyInput {
            best: at_least(DECIDED_HIGH),
            second: Some(at_most(DECIDED_HIGH - GREAT_GAP - 0.02)),
            played: at_least(DECIDED_HIGH),
            played_rank: Some(0),
        };
        assert_eq!(class(&i), Classification::Best);

        // A hair below, the same move is great.
        i.best = at_most(DECIDED_HIGH);
        i.played = i.best;
        assert_eq!(class(&i), Classification::Great);
    }

    /// `DECIDED_LOW` cannot fire, and this is where that is recorded: the gap is
    /// at most `win_prob_before`, so a move clearing `GREAT_GAP` is already in a
    /// position worth at least that much. The constant stays as documentation of
    /// the intent; if `GREAT_GAP` ever drops below it, this test starts failing
    /// and the check becomes live again.
    #[test]
    fn the_lower_decided_bound_is_unreachable_while_the_gap_exceeds_it() {
        // A compile-time check: both sides are constants, so this is the shape
        // clippy asks for and it fails the build rather than a test run.
        const { assert!(GREAT_GAP >= DECIDED_LOW) };

        // The most one-sided input the rule allows: everything the mover has is
        // the gap itself.
        let i = ClassifyInput {
            best: at(GREAT_GAP + 0.01),
            second: Some(Score::Mate(-1)),
            played: at(GREAT_GAP + 0.01),
            played_rank: Some(0),
        };
        assert!(i.best.win_prob() > DECIDED_LOW);
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

        let only_move = only_move_at(0.60);

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
