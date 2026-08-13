//! End-to-end checks over real positions: SEE on an actual board, the point-of-
//! view flip of §3, and the classification that comes out of them.

use kibitz_core::classify::{Classification, ClassifyInput, classify};
use kibitz_core::eval::Score;
use kibitz_core::parse_fen;
use kibitz_core::see::{see, see_square};
use shakmaty::{Chess, Position, Square, uci::UciMove};

fn play(fen: &str, uci: &str) -> (Chess, shakmaty::Move) {
    let pos = parse_fen(fen).expect("valid fen");
    let m = uci
        .parse::<UciMove>()
        .expect("valid uci")
        .to_move(&pos)
        .expect("legal move");
    (pos, m)
}

fn sq(name: &str) -> Square {
    name.parse().expect("valid square")
}

/// The Greek gift: Bxh7+ hands over a bishop for a pawn, and the engine still
/// likes white. It is the only move that holds, so it is Great.
///
/// The sacrifice itself is *not* part of that verdict: classification does not
/// look at SEE at all (DESIGN §8.3). The material offer is still visible in
/// `AnalysisContext`, which is where an explanation reads it from.
#[test]
fn greek_gift_is_great() {
    let fen = "r1bq1rk1/ppp1bppp/2n1pn2/3p4/3P4/2NBPN2/PPP2PPP/R1BQ1RK1 w - - 0 1";
    let (pos, bxh7) = play(fen, "d3h7");
    let after = pos.clone().play(bxh7).expect("legal");

    // The move loses material on h7, and the bishop left standing there hangs.
    assert!(see(&pos, bxh7) < 0, "see = {}", see(&pos, bxh7));
    assert_eq!(after.turn(), shakmaty::Color::Black);
    assert!(see_square(&after, sq("h7")) < 0);

    // Engine numbers: white keeps a small edge, everything else is worse.
    let input = ClassifyInput {
        best: Score::Cp(60),
        second: Some(Score::Cp(-60)),
        played: Score::Cp(-55).negate(), // position N+1 is black to move
        played_rank: Some(0),
    };
    let out = classify(&input);
    assert_eq!(out.classification, Classification::Great);
    assert!(out.delta > -0.02, "delta = {}", out.delta);
}

/// Section 3, on a real board: the score of position N+1 belongs to the
/// opponent, so it has to be negated before it can be compared with position N.
#[test]
fn point_of_view_flip_on_a_real_position() {
    // 1.e4 e5 2.Nf3 Nc6 3.Nxe5?? — the knight is simply recaptured.
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 0 1";
    let (pos, nxe5) = play(fen, "f3e5");
    assert_eq!(see(&pos, nxe5), -220); // +100 pawn, -320 knight
    let after = pos.clone().play(nxe5).expect("legal");
    assert_eq!(after.turn(), shakmaty::Color::Black);

    // Stockfish would report position N+1 from black's point of view. Say black
    // is doing well there: +250 for black.
    let raw = Score::Cp(250);
    let played = raw.negate();
    assert_eq!(played, Score::Cp(-250));

    let out = classify(&ClassifyInput {
        best: Score::Cp(30),
        second: None,
        played,
        played_rank: None,
    });
    // Negative delta = the move made things worse for the mover.
    assert!(out.delta < 0.0);
    assert!(out.win_prob_after < out.win_prob_before);
    assert!(out.classification.is_mistake(), "{:?}", out.classification);

    // Had the raw score been used as-is, the move would have looked like a gain.
    let wrong = classify(&ClassifyInput {
        best: Score::Cp(30),
        second: None,
        played: raw,
        played_rank: None,
    });
    assert!(wrong.delta > 0.0, "the sign error flips the verdict");
}

/// Missing a mate in one must not be softened by the win-probability collapse.
#[test]
fn missing_mate_in_one_is_a_miss() {
    // Back-rank mate is available (Ra8#); pushing a pawn instead throws it away.
    let fen = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
    let (pos, mate) = play(fen, "a1a8");
    let mated = pos.clone().play(mate).expect("legal");
    assert!(mated.is_checkmate());

    // The move actually played keeps a completely winning rook endgame,
    // yet it is still a Miss.
    let out = classify(&ClassifyInput {
        best: Score::Mate(1),
        second: Some(Score::Cp(900)),
        played: Score::Cp(900),
        played_rank: Some(1),
    });
    assert_eq!(out.classification, Classification::Miss);

    // And playing the mate itself is Best: position N+1 says "I am mated".
    let out = classify(&ClassifyInput {
        best: Score::Mate(1),
        second: Some(Score::Cp(900)),
        played: Score::Mate(0).negate(),
        played_rank: Some(0),
    });
    assert_eq!(out.classification, Classification::Best);
}

/// The Opera Game, before 16.Qb8+ — the queen sacrifice that forces mate.
///
/// White's win probability is already 1.0, so the `undecided` guard closes the
/// ordinary Great path. The move is still the only one that mates, and every
/// annotator marks it, so the mate route of §8.3 has to reach it.
#[test]
fn the_opera_game_queen_sacrifice_is_great() {
    let fen = "4kb1r/p2n1ppp/4q3/4p1B1/4P3/1Q6/PPP2PPP/2KR4 w k - 0 16";
    let (pos, qb8) = play(fen, "b3b8");
    let after = pos.clone().play(qb8).expect("legal");
    // The queen is offered: black recaptures with the knight and nothing of
    // white's answers back on b8.
    assert!(see(&pos, qb8) < 0, "see = {}", see(&pos, qb8));
    assert_eq!(after.turn(), shakmaty::Color::Black);

    // Engine numbers at depth 12: Qb8+ mates, the runners-up merely win.
    let input = ClassifyInput {
        best: Score::Mate(2),
        second: Some(Score::Cp(343)),
        played: Score::Mate(-1).negate(), // position N+1 is black to move
        played_rank: Some(0),
    };
    let out = classify(&input);
    assert_eq!(out.win_prob_before, 1.0, "the position is already decided");
    assert_eq!(out.classification, Classification::Great);
}

/// The same position, with the second-best move changed into a slower mate.
/// Two moves mate, so this one was not found — no Great.
#[test]
fn the_opera_game_sacrifice_is_not_great_when_another_move_mates_too() {
    let out = classify(&ClassifyInput {
        best: Score::Mate(2),
        second: Some(Score::Mate(4)),
        played: Score::Mate(-1).negate(),
        played_rank: Some(0),
    });
    assert_eq!(out.classification, Classification::Best);
}

/// A forced move is not "!", even when it mates. `second` is `None` when
/// MultiPV returns a single line, which is what one legal move looks like.
#[test]
fn a_forced_mate_with_one_legal_move_is_not_great() {
    // Black is in check on the eighth rank. h7 is its own pawn, g8 stays on the
    // rank, and the pawn cannot interpose: Kg7 is the whole move list.
    let pos = parse_fen("R6k/7p/8/8/8/8/8/6K1 b - - 0 1").unwrap();
    assert_eq!(pos.legal_moves().len(), 1);

    let out = classify(&ClassifyInput {
        best: Score::Mate(3),
        second: None,
        played: Score::Mate(3),
        played_rank: Some(0),
    });
    assert_eq!(out.classification, Classification::Best);
}

/// Being mated is not the same as mating: `Score::mating_side()` decides which.
#[test]
fn holding_out_longest_against_mate_is_not_great() {
    let out = classify(&ClassifyInput {
        best: Score::Mate(-3),
        second: Some(Score::Mate(-1)),
        played: Score::Mate(-3),
        played_rank: Some(0),
    });
    assert_eq!(out.classification, Classification::Best);
}

/// Hanging-piece detection, which is what `see_square` exists for.
#[test]
fn hanging_pieces_of_a_real_position() {
    // A white knight on d4, attacked by the e5 pawn and defended by nothing.
    let pos = parse_fen("rnbqkbnr/pppp1ppp/8/4p3/3N4/8/PPPPPPPP/RNBQKB1R b KQkq - 0 1").unwrap();
    let hanging: Vec<Square> = pos
        .board()
        .occupied()
        .into_iter()
        .filter(|&s| see_square(&pos, s) < 0)
        .collect();
    assert_eq!(hanging, vec![sq("d4")]);
}
