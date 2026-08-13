//! Motif detection, each variant on a position that plainly contains it and on
//! a near-miss where it must stay silent.
//!
//! The near-miss cases are the point of this file: a confidently wrong "fork"
//! reads as authoritative in the generated explanation, so every rule is tested
//! from both sides.

use kibitz_core::motif::{self, Motif};
use kibitz_core::parse_fen;
use shakmaty::{Chess, san::San};

fn pos(fen: &str) -> Chess {
    parse_fen(fen).expect("test FEN must be legal")
}

/// Detect the motifs created by playing `san` in `fen`.
fn motifs(fen: &str, san: &str) -> Vec<Motif> {
    motifs_of(&pos(fen), san)
}

/// Detect the motifs created by playing `san` in an already-built position.
fn motifs_of(p: &Chess, san: &str) -> Vec<Motif> {
    let mv = san
        .parse::<San>()
        .expect("test SAN must parse")
        .to_move(p)
        .expect("test SAN must be legal");
    motif::detect(p, mv)
}

/// A PV in the form `detect_in_line` takes.
fn line(san: &[&str]) -> Vec<String> {
    san.iter().map(|s| s.to_string()).collect()
}

fn keys(motifs: &[Motif]) -> Vec<&'static str> {
    motifs.iter().map(|m| m.key()).collect()
}

// ---------------------------------------------------------------------------
// Fork
// ---------------------------------------------------------------------------

/// Nd5-c7+ hits the king on e8 and the rook on a8, and nothing can touch c7.
#[test]
fn fork_on_king_and_rook() {
    let found = motifs("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1", "Nc7+");
    assert_eq!(keys(&found), vec!["fork"]);
    assert_eq!(
        found[0],
        Motif::Fork {
            from: "d5".into(),
            attacker: "c7".into(),
            targets: vec!["a8".into(), "e8".into()],
        }
    );
}

/// The same fork, except Bb8 covers c7 and nothing defends the knight. A piece
/// that simply drops has not forked anything.
#[test]
fn fork_does_not_fire_when_the_forking_piece_just_hangs() {
    let found = motifs("rb2k3/8/8/3N4/8/8/8/4K3 w - - 0 1", "Nc7+");
    assert!(found.is_empty(), "knight is en prise on c7, got {found:?}");
}

/// Qd1-d4 attacks both knights, but a queen does not win anything by attacking
/// two pieces worth a third of it.
#[test]
fn fork_does_not_fire_on_targets_worth_less_than_the_attacker() {
    let found = motifs("4k3/8/8/2n1n3/8/8/8/3QK3 w - - 0 1", "Qd4");
    assert!(
        found.is_empty(),
        "a queen does not fork two knights, got {found:?}"
    );
}

// ---------------------------------------------------------------------------
// Pin
// ---------------------------------------------------------------------------

/// Ra1-d1 puts the knight on d5 in front of the king on d8: an absolute pin.
#[test]
fn pin_of_a_knight_against_the_king() {
    let found = motifs("3k4/8/8/3n4/8/8/8/R3K3 w - - 0 1", "Rd1");
    assert_eq!(keys(&found), vec!["pin"]);
    assert_eq!(
        found[0],
        Motif::Pin {
            attacker: "d1".into(),
            pinned: "d5".into(),
            behind: "d8".into(),
        }
    );
}

/// Rook in front of rook. Forcing the front one to move wins nothing, so this
/// is neither a pin nor a skewer.
#[test]
fn pin_does_not_fire_when_both_pieces_are_worth_the_same() {
    let found = motifs("3rk3/8/8/3r4/8/8/8/R3K3 w - - 0 1", "Rd1");
    assert!(
        found.is_empty(),
        "rook behind rook is not a pin, got {found:?}"
    );
}

/// Qa1-d1 lines up the d7 pawn in front of the d8 bishop, and the geometry is
/// real — but the bishop is defended and worth a third of the queen, so
/// breaking through wins nothing.
#[test]
fn pin_does_not_fire_when_there_is_nothing_to_win_behind() {
    let found = motifs("3bk3/3p4/8/8/8/8/8/Q3K3 w - - 0 1", "Qd1");
    assert!(
        found.is_empty(),
        "a queen wins nothing by breaking through to a defended bishop, got {found:?}"
    );
}

// ---------------------------------------------------------------------------
// Skewer
// ---------------------------------------------------------------------------

/// Bd1-b3 hits the queen on d5 with the rook on f7 right behind it. The bishop
/// is worth less than the rook it will collect, and the a2 pawn covers b3 so
/// the queen cannot simply take it.
#[test]
fn skewer_of_a_queen_in_front_of_a_rook() {
    let found = motifs("4k3/5r2/8/3q4/8/8/P7/3BK3 w - - 0 1", "Bb3");
    assert_eq!(keys(&found), vec!["skewer"]);
    assert_eq!(
        found[0],
        Motif::Skewer {
            attacker: "b3".into(),
            front: "d5".into(),
            behind: "f7".into(),
        }
    );
}

/// Same geometry, but the piece behind the queen is white's own rook. Nothing
/// is skewered.
#[test]
fn skewer_does_not_fire_when_the_piece_behind_is_friendly() {
    let found = motifs("4k3/3R4/8/3q4/8/8/8/R3K3 w - - 0 1", "Rad1");
    assert!(
        found.is_empty(),
        "the piece behind the queen belongs to the mover, got {found:?}"
    );
}

/// Rook attacking a queen with a defended rook of its own value behind it.
/// The queen steps aside, the rooks trade, and nothing has been won.
#[test]
fn skewer_does_not_fire_when_nothing_is_won_behind() {
    let found = motifs("3rk3/8/8/3q4/8/8/8/R3K3 w - - 0 1", "Rd1");
    assert!(
        found.is_empty(),
        "trading rooks at the far end is not a skewer, got {found:?}"
    );
}

// ---------------------------------------------------------------------------
// Discovered attack
// ---------------------------------------------------------------------------

/// The knight steps off the long diagonal and Bb2 hits the rook on g7.
#[test]
fn discovered_attack_when_a_knight_clears_the_long_diagonal() {
    let found = motifs("4k3/6r1/8/8/3N4/8/1B6/4K3 w - - 0 1", "Nb5");
    assert_eq!(keys(&found), vec!["discovered_attack"]);
    assert_eq!(
        found[0],
        Motif::DiscoveredAttack {
            moved_from: "d4".into(),
            revealed: "b2".into(),
            targets: vec!["g7".into()],
        }
    );
}

/// The same clearance, but the bishop only ends up looking at a pawn that the
/// king defends. Nothing has been discovered that is worth saying.
#[test]
fn discovered_attack_does_not_fire_on_a_defended_pawn() {
    let found = motifs("7k/6p1/8/8/3N4/8/1B6/4K3 w - - 0 1", "Nb5");
    assert!(
        found.is_empty(),
        "a defended pawn worth less than the bishop is not a discovery, got {found:?}"
    );
}

// ---------------------------------------------------------------------------
// Back rank
// ---------------------------------------------------------------------------

/// Ra1-a8+ with the king walled in by its own f7/g7/h7 pawns.
#[test]
fn back_rank_when_the_king_is_shut_in_by_its_own_pawns() {
    let found = motifs("6k1/5ppp/8/8/8/8/8/R3K3 w - - 0 1", "Ra8+");
    assert_eq!(keys(&found), vec!["back_rank"]);
    assert_eq!(
        found[0],
        Motif::BackRank {
            attacker: "a8".into(),
            king: "g8".into(),
        }
    );
}

/// The same rook check, but the h-pawn has made luft and the king walks to h7.
#[test]
fn back_rank_does_not_fire_when_the_king_has_luft() {
    let found = motifs("6k1/5pp1/7p/8/8/8/8/R3K3 w - - 0 1", "Ra8+");
    assert!(found.is_empty(), "the king escapes to h7, got {found:?}");
}

// ---------------------------------------------------------------------------
// Line replay
// ---------------------------------------------------------------------------

/// A PV that runs out of legality mid-way keeps the motifs of the prefix and
/// stops there instead of panicking.
#[test]
fn detect_in_line_truncates_at_an_illegal_move() {
    let start = pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");

    let good: Vec<String> = ["Nc7+", "Kf7"].iter().map(|s| s.to_string()).collect();
    assert_eq!(keys(&motif::detect_in_line(&start, &good)), vec!["fork"]);

    // "Qh8" is syntactically fine and completely illegal: black has no queen.
    let corrupt: Vec<String> = ["Nc7+", "Qh8", "Kf7"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(keys(&motif::detect_in_line(&start, &corrupt)), vec!["fork"]);

    // Not even parsable as SAN.
    let garbage: Vec<String> = ["Nc7+", "!!??"].iter().map(|s| s.to_string()).collect();
    assert_eq!(keys(&motif::detect_in_line(&start, &garbage)), vec!["fork"]);

    // An illegal first move collects nothing at all.
    let doomed: Vec<String> = ["Qh8", "Nc7+"].iter().map(|s| s.to_string()).collect();
    assert!(motif::detect_in_line(&start, &doomed).is_empty());
}

/// An empty PV is not an error.
#[test]
fn detect_in_line_accepts_an_empty_line() {
    let start = pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
    assert!(motif::detect_in_line(&start, &[]).is_empty());
}

// ---------------------------------------------------------------------------
// Line aggregation
// ---------------------------------------------------------------------------
//
// `detect` is generous per move; a PV is twenty moves of it. These tests are
// about the cut from that union down to the one or two things a coach would
// actually name.

/// A piece left en prise and taken is exactly what `hanging` is for, so the
/// filters below must not swallow it.
#[test]
fn detect_in_line_reports_a_piece_that_is_simply_taken() {
    let start = pos("4k3/8/8/3n4/8/8/8/3RK3 w - - 0 1");
    let line = line(&["Rxd5"]);
    assert_eq!(keys(&motif::detect_in_line(&start, &line)), vec!["hanging"]);
}

/// A chain of recaptures on d5. Every link of it looks like a hanging piece to
/// `detect` — the pieces really are taken, and really do settle badly on the
/// contested square — but the sequence is an exchange, not a piece dropped.
#[test]
fn detect_in_line_does_not_report_an_exchange_as_hanging() {
    let start = pos("3rk3/8/2p5/3p4/2B5/8/8/3RK3 w - - 0 1");

    // Per move, the recaptures do read as hanging pieces.
    let after_first = pos("3rk3/8/2p5/3B4/8/8/8/3RK3 b - - 0 1");
    assert!(
        motifs_of(&after_first, "cxd5")
            .iter()
            .any(|m| m.key() == "hanging"),
        "the bishop on d5 does look hanging on its own ply"
    );

    // Over the line they are one exchange and nothing is named.
    let found = motif::detect_in_line(&start, &line(&["Bxd5", "cxd5", "Rxd5", "Rxd5"]));
    assert!(
        found.iter().all(|m| m.key() != "hanging"),
        "an exchange on one square is not a hanging piece, got {found:?}"
    );
}

/// Taking the piece that is giving check is forced housekeeping. The bishop on
/// h7 was given, not left.
#[test]
fn detect_in_line_does_not_report_a_captured_checking_piece_as_hanging() {
    let start = pos("6k1/7B/8/8/8/8/8/6K1 b - - 0 1");
    let found = motif::detect_in_line(&start, &line(&["Kxh7"]));
    assert!(
        found.iter().all(|m| m.key() != "hanging"),
        "a checking piece that is captured is not hanging, got {found:?}"
    );
}

/// A pawn picked up along the way is not what the line is about.
#[test]
fn detect_in_line_does_not_report_a_pawn_grab() {
    let start = pos("4k3/1p6/8/8/8/8/8/1Q2K3 w - - 0 1");
    assert!(
        motifs_of(&start, "Qxb7")
            .iter()
            .any(|m| m.key() == "hanging"),
        "the pawn is hanging on its own ply"
    );
    assert!(
        motif::detect_in_line(&start, &line(&["Qxb7"])).is_empty(),
        "a pawn is not worth a sentence"
    );
}

/// The rook shuffles down the d-file with the knight pinned against the king
/// the whole way. Three plies re-create the same pin; it is one pin.
#[test]
fn detect_in_line_reports_a_persisting_pin_once() {
    let start = pos("3k4/7p/8/3n4/8/8/8/R3K3 w - - 0 1");
    let found = motif::detect_in_line(&start, &line(&["Rd1", "h6", "Rd2", "h5", "Rd3", "h4"]));
    assert_eq!(keys(&found), vec!["pin"], "got {found:?}");
    // The first sighting is the one kept: it is the ply the reader is shown.
    assert_eq!(
        found[0],
        Motif::Pin {
            attacker: "d1".into(),
            pinned: "d5".into(),
            behind: "d8".into(),
        }
    );
}

/// A second piece taking over the same pin is still that one pin, not a new
/// motif: the rook steps aside and the queen takes over the d-file.
#[test]
fn detect_in_line_reports_a_pin_renewed_by_another_piece_once() {
    let start = pos("3k4/7p/8/3n4/8/8/Q7/R3K3 w - - 0 1");
    let found = motif::detect_in_line(&start, &line(&["Rd1", "h6", "Rc1", "h5", "Qd2", "h4"]));
    assert_eq!(keys(&found), vec!["pin"], "got {found:?}");
}

/// The knight forks on ply eight. By then the line is about a different
/// position, and the move being explained is not why it happens.
#[test]
fn detect_in_line_ignores_motifs_beyond_the_horizon() {
    let start = pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
    let far = line(&[
        "Nb4", "Kf8", "Nd5", "Ke8", "Nb4", "Kf8", "Nd5", "Ke8", "Nc7+",
    ]);
    assert!(
        motif::detect_in_line(&start, &far).is_empty(),
        "a fork nine plies in is not what the move is about"
    );

    // The same fork on the first ply is reported.
    assert_eq!(
        keys(&motif::detect_in_line(&start, &line(&["Nc7+"]))),
        vec!["fork"]
    );
}

/// Four loose pieces, one collected on each of the first four plies — every one
/// of them a genuine hanging piece, none of them an exchange. The explanation
/// still gets a handful, and it gets the first ones.
#[test]
fn detect_in_line_keeps_only_the_first_few_of_a_line_full_of_tactics() {
    let start = pos("4k2r/n7/8/8/1b6/2B5/2B4N/R3K3 w - - 0 1");
    let found = motif::detect_in_line(&start, &line(&["Rxa7", "Rxh2", "Bxb4", "Rxc2"]));

    assert_eq!(
        keys(&found),
        vec!["hanging", "hanging", "hanging"],
        "got {found:?}"
    );
    assert!(
        found
            .iter()
            .all(|m| !matches!(m, Motif::Hanging { square, .. } if square == "c2")),
        "the fourth piece is one too many to name, got {found:?}"
    );
}

/// An illegal move handed straight to `detect` produces nothing rather than a
/// motif read off a position that never existed.
#[test]
fn detect_ignores_an_illegal_move() {
    let start = pos("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
    let other = pos("r3k3/8/8/8/3N4/8/8/4K3 w - - 0 1");
    let mv = "Nc6"
        .parse::<San>()
        .unwrap()
        .to_move(&other)
        .expect("legal in the other position");
    assert!(motif::detect(&start, mv).is_empty());
}

// ---------------------------------------------------------------------------
// Hanging — depends on see.rs
// ---------------------------------------------------------------------------

/// Rd1xd5 takes a knight that nothing defends.
#[test]
fn hanging_when_an_undefended_piece_is_taken() {
    let found = motifs("4k3/8/8/3n4/8/8/8/3RK3 w - - 0 1", "Rxd5");
    let hanging = found
        .iter()
        .find(|m| m.key() == "hanging")
        .expect("the knight was hanging");
    match hanging {
        Motif::Hanging { square, role, see } => {
            assert_eq!(square, "d5");
            assert_eq!(role, "knight");
            assert!(*see < 0);
        }
        other => panic!("expected hanging, got {other:?}"),
    }
}

/// The same capture, but the e6 pawn recaptures and white loses the exchange.
/// The knight was never hanging.
#[test]
fn hanging_does_not_fire_on_a_defended_piece() {
    let found = motifs("4k3/8/4p3/3n4/8/8/3R4/4K3 w - - 0 1", "Rxd5");
    assert!(
        found.iter().all(|m| m.key() != "hanging"),
        "a defended knight is not hanging, got {found:?}"
    );
}
