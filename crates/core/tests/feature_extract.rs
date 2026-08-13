//! `feature::extract` on positions where every expected number can be counted
//! by hand, and `feature::diff` on a quiet move versus a capture.

use kibitz_core::feature;
use kibitz_core::parse_fen;
use shakmaty::Chess;

fn pos(fen: &str) -> Chess {
    parse_fen(fen).expect("test FEN must be legal")
}

/// After 1.e4 e5 2.Nf3 Nc6.
///
/// White: Nf3 hits d4 and e5, the e4 pawn hits d5 — and nothing at all defends
/// e4. Black: the e5 pawn and Nc6 both hit d4, Nc6 also hits e5.
#[test]
fn center_control_counts_attacks_on_the_four_central_squares() {
    let p = pos("r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 4 3");
    let cc = feature::center_control(&p);
    assert_eq!(cc.white, 3, "Nf3->d4, Nf3->e5, e4->d5");
    assert_eq!(cc.black, 3, "e5->d4, Nc6->d4, Nc6->e5");
}

/// Giuoco Piano, where the two sides are not level in the centre.
///
/// White: Nf3->d4, e4->d5, Bc4->d5, Nf3->e5 = 4. Nothing defends e4.
/// Black: e5->d4, Nc6->d4, Bc5->d4, Nf6->e4, Nf6->d5, Nc6->e5 = 6.
#[test]
fn center_control_separates_the_two_sides() {
    let p = pos("r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 w kq - 6 5");
    let cc = feature::center_control(&p);
    assert_eq!(cc.white, 4);
    assert_eq!(cc.black, 6);
}

/// The starting position: the centre is completely uncontested, every line is
/// still blocked by the pawns.
#[test]
fn center_control_is_zero_in_the_starting_position() {
    let p = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    let cc = feature::center_control(&p);
    assert_eq!(cc.white, 0);
    assert_eq!(cc.black, 0);
}

/// Giuoco Piano, white castled, black not.
///
/// White king g1: the ring is f1/f2/g2/h1/h2 and only Bc5 reaches it (c5-d4-e3-f2).
/// The f2/g2/h2 pawns are all home, so nothing is missing from the shield.
/// Black king e8: the ring is d7/e7/f7/d8/f8 and only Bc4 reaches it (c4-d5-e6-f7).
/// The e-pawn has gone to e5, so the e-file in front of the king is bare.
#[test]
fn king_safety_counts_ring_attacks_and_missing_shield_pawns() {
    let p = pos("r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 w kq - 6 5");
    let ks = feature::king_safety(&p);

    assert_eq!(ks.white.attackers, 1, "Bc5 hits f2");
    assert_eq!(
        ks.white.missing_shield_pawns, 0,
        "f2, g2 and h2 are all home"
    );
    assert_eq!(
        ks.white.king_square,
        Some(shakmaty::Square::G1.to_u32() as u8)
    );

    assert_eq!(ks.black.attackers, 1, "Bc4 hits f7");
    assert_eq!(ks.black.missing_shield_pawns, 1, "the e-pawn left e7/e6");
    assert_eq!(
        ks.black.king_square,
        Some(shakmaty::Square::E8.to_u32() as u8)
    );
}

/// White pawns d4, d5, f2; black pawns b7, f7.
///
/// White: d4/d5 are doubled, all three are isolated (no white pawn on c, e or g).
/// d4 and d5 are passed — no black pawn on c/d/e at all — while f2 is stopped by f7.
/// Black: b7 and f7 are isolated, neither is doubled, and only b7 is passed
/// (f2 stands in the f-pawn's way).
#[test]
fn pawn_structure_lists_isolated_doubled_and_passed_pawns() {
    let p = pos("k7/1p3p2/8/3P4/3P4/8/5P2/K7 w - - 0 1");
    let ps = feature::pawn_structure(&p);

    assert_eq!(ps.white.doubled, vec!["d4", "d5"]);
    assert_eq!(ps.white.isolated, vec!["d4", "d5", "f2"]);
    assert_eq!(ps.white.passed, vec!["d4", "d5"]);

    assert!(ps.black.doubled.is_empty());
    assert_eq!(ps.black.isolated, vec!["b7", "f7"]);
    assert_eq!(ps.black.passed, vec!["b7"]);
}

/// White pawns a2, b2, f2, g2, h2; black pawns f7, g7, h7.
///
/// c, d and e carry no pawn at all (fully open); a and b carry only white pawns
/// (half-open). f, g and h are contested by both sides and are not listed.
/// The heavy pieces on those files are Ra8 and Rd1.
#[test]
fn open_files_separates_fully_open_from_half_open() {
    let p = pos("r5k1/5ppp/8/8/8/8/PP3PPP/3R2K1 w - - 0 1");
    let files = feature::open_files(&p);

    let names: Vec<&str> = files.iter().map(|f| f.file.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c", "d", "e"]);

    let fully: Vec<&str> = files
        .iter()
        .filter(|f| f.fully_open)
        .map(|f| f.file.as_str())
        .collect();
    assert_eq!(fully, vec!["c", "d", "e"]);

    let a = files.iter().find(|f| f.file == "a").unwrap();
    assert_eq!(a.occupied_by, vec!["a8"]);
    let d = files.iter().find(|f| f.file == "d").unwrap();
    assert_eq!(d.occupied_by, vec!["d1"]);
    let b = files.iter().find(|f| f.file == "b").unwrap();
    assert!(b.occupied_by.is_empty());
}

#[test]
fn mobility_is_twenty_each_in_the_starting_position() {
    let p = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    let m = feature::mobility(&p);
    assert_eq!(m.white, 20);
    assert_eq!(m.black, 20);
}

#[test]
fn material_is_a_white_relative_balance() {
    let start = pos("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
    assert_eq!(feature::material(&start), 0);

    // Black is a queen down.
    let p = pos("4k3/8/8/8/8/8/8/3QK3 w - - 0 1");
    assert_eq!(feature::material(&p), 900);
}

/// Kg1-h1 changes nothing that matters: the same pawns, the same files, the
/// same 24 white and 19 black legal moves, and a2 stays hanging on both sides
/// of the move.
#[test]
fn diff_of_a_quiet_move_reports_no_changes() {
    let before = pos("r5k1/5ppp/8/8/8/8/PP3PPP/3R2K1 w - - 0 1");
    let after = pos("r5k1/5ppp/8/8/8/8/PP3PPP/3R3K b - - 1 1");
    let d = feature::diff(&before, &after);
    assert!(
        d.changes.is_empty(),
        "a king step with nothing behind it should be silent, got {:?}",
        d.changes
    );
}

/// Qxd5 wins a whole queen, which has to show up.
#[test]
fn diff_of_a_capture_reports_the_material_swing() {
    let before = pos("4k3/8/8/3q4/3Q4/8/8/4K3 w - - 0 1");
    let after = pos("4k3/8/8/3Q4/8/8/8/4K3 b - - 0 1");
    let d = feature::diff(&before, &after);

    assert!(!d.changes.is_empty());
    let material = d
        .changes
        .iter()
        .find(|c| c.kind == "material")
        .expect("a queen capture is a material change");
    assert_eq!(material.delta, Some(900.0));
    assert_eq!(material.side, "both");
}

/// Diffing a position against itself is always silent.
#[test]
fn diff_against_itself_is_empty() {
    let p = pos("r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1 w kq - 6 5");
    let d = feature::diff(&p, &p);
    assert!(d.changes.is_empty());
    assert_eq!(d.before, d.after);
}

// ---------------------------------------------------------------------------
// Hanging — depends on see.rs
// ---------------------------------------------------------------------------
// These are the only assertions that read `see`. Drop the `#[ignore]` once
// see.rs has landed and its sign convention is pinned down by its own tests.

/// The black knight on d5 is attacked by Rd1 and defended by nothing.
#[test]
#[ignore = "depends on see.rs, implemented in parallel"]
fn hanging_lists_an_undefended_attacked_piece() {
    let p = pos("4k3/8/8/3n4/8/8/8/3RK3 w - - 0 1");
    let h = feature::hanging(&p);
    let knight = h
        .iter()
        .find(|h| h.square == "d5")
        .expect("an undefended knight in front of a rook is hanging");
    assert_eq!(knight.role, "knight");
    assert_eq!(knight.color, "black");
    assert!(knight.see < 0);
}

/// The same knight, now defended by the e6 pawn: RxN loses the exchange, so
/// nothing is hanging.
#[test]
#[ignore = "depends on see.rs, implemented in parallel"]
fn hanging_ignores_a_defended_piece() {
    let p = pos("4k3/8/4p3/3n4/8/8/3R4/4K3 w - - 0 1");
    let h = feature::hanging(&p);
    assert!(
        h.iter().all(|h| h.square != "d5"),
        "a knight defended by a pawn is not hanging, got {h:?}"
    );
}
