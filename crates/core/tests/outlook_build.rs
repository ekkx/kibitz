//! `outlook::build` walking a PV, and `outlook::consensus` over candidate moves.

use kibitz_core::eval::Score;
use kibitz_core::outlook;
use kibitz_core::parse_fen;
use kibitz_core::types::Candidate;
use shakmaty::Chess;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn pos(fen: &str) -> Chess {
    parse_fen(fen).expect("test FEN must be legal")
}

/// A candidate with only the fields the outlook actually reads filled in.
fn candidate(san: &str, pv: &[&str]) -> Candidate {
    Candidate {
        san: san.to_string(),
        uci: String::new(),
        score: Score::Cp(20),
        win_prob: 0.5,
        pv: pv.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn build_walks_the_pv_up_to_max_plies() {
    let start = pos(START);
    let candidates = [candidate("e4", &["e4", "e5", "Nf3", "Nc6"])];

    let out = outlook::build(&start, &candidates, 3);
    assert_eq!(out.long_pv, vec!["e4", "e5", "Nf3"]);
    assert_eq!(
        out.terminal_fen,
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
    );
    // Three plies of development have to register as *something*.
    assert!(!out.terminal_diff.changes.is_empty());
    assert_eq!(
        out.terminal_diff.before,
        kibitz_core::feature::extract(&start)
    );
}

/// Engine PVs are occasionally corrupt. The valid prefix is kept, the rest is
/// dropped, and nothing panics.
#[test]
fn build_truncates_at_an_illegal_move() {
    let start = pos(START);
    // "Nf6" is a black move; no white knight can reach f6 on move two.
    let candidates = [candidate("e4", &["e4", "e5", "Nf6", "Nc3"])];

    let out = outlook::build(&start, &candidates, 12);
    assert_eq!(out.long_pv, vec!["e4", "e5"]);
    assert_eq!(
        out.terminal_fen,
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
    );
}

#[test]
fn build_without_candidates_stays_where_it_is() {
    let start = pos(START);
    let out = outlook::build(&start, &[], 12);
    assert!(out.long_pv.is_empty());
    assert_eq!(out.terminal_fen, START);
    assert!(out.terminal_diff.changes.is_empty());
    assert!(out.consensus.common_piece.is_none());
    assert!(out.consensus.common_targets.is_empty());
}

#[test]
fn build_with_zero_plies_stays_where_it_is() {
    let start = pos(START);
    let candidates = [candidate("e4", &["e4", "e5"])];
    let out = outlook::build(&start, &candidates, 0);
    assert!(out.long_pv.is_empty());
    assert_eq!(out.terminal_fen, START);
}

/// Both candidates are knight moves and both hit the queen on d5.
#[test]
fn consensus_reports_the_shared_piece_and_the_shared_target() {
    let p = pos("4k3/8/8/3q4/8/8/8/1N2KN2 w - - 0 1");
    let candidates = [candidate("Nc3", &["Nc3"]), candidate("Ne3", &["Ne3"])];

    let c = outlook::consensus(&p, &candidates);
    assert_eq!(c.common_piece.as_deref(), Some("knight"));
    assert_eq!(c.common_targets, vec!["d5"]);
}

/// A pawn move and a knight move that land nowhere near each other.
#[test]
fn consensus_is_empty_when_the_candidates_share_nothing() {
    let p = pos(START);
    let candidates = [candidate("e4", &["e4"]), candidate("Nf3", &["Nf3"])];

    let c = outlook::consensus(&p, &candidates);
    assert_eq!(c.common_piece, None);
    assert!(c.common_targets.is_empty());
}

/// Same piece, different squares, nothing in common to aim at: the piece is
/// still reported, the targets are not.
#[test]
fn consensus_reports_a_shared_piece_without_a_shared_target() {
    let p = pos(START);
    let candidates = [candidate("Nf3", &["Nf3"]), candidate("Nc3", &["Nc3"])];

    let c = outlook::consensus(&p, &candidates);
    assert_eq!(c.common_piece.as_deref(), Some("knight"));
    assert!(c.common_targets.is_empty());
}

#[test]
fn consensus_needs_at_least_two_candidates() {
    let p = pos(START);
    let one = [candidate("e4", &["e4"])];
    let c = outlook::consensus(&p, &one);
    assert_eq!(c.common_piece, None);
    assert!(c.common_targets.is_empty());

    let none: [Candidate; 0] = [];
    let c = outlook::consensus(&p, &none);
    assert_eq!(c.common_piece, None);
}

/// A candidate that does not fit the position is dropped, not counted as
/// disagreement — but dropping it can leave too few to agree on anything.
#[test]
fn consensus_drops_unplayable_candidates() {
    let p = pos("4k3/8/8/3q4/8/8/8/1N2KN2 w - - 0 1");
    let candidates = [
        candidate("Nc3", &["Nc3"]),
        candidate("Ne3", &["Ne3"]),
        candidate("Qxh7", &["Qxh7"]),
    ];

    let c = outlook::consensus(&p, &candidates);
    assert_eq!(c.common_piece.as_deref(), Some("knight"));
    assert_eq!(c.common_targets, vec!["d5"]);
}
