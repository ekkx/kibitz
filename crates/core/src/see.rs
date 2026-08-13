//! Static Exchange Evaluation.
//!
//! Resolves a capture sequence on a square statically and returns the material
//! swing in centipawns for the side to move. Required for `hanging_pieces` and
//! for motif detection ("is this capture actually winning material?").
//!
//! `Board::attacks_to()` makes this straightforward: take with the least valuable
//! attacker each time and settle the sequence with negamax.

use shakmaty::{Bitboard, Board, Chess, Color, Move, Position, Role, Square};

/// Piece values for exchange arithmetic (centipawns). Deliberately separate from
/// any evaluation-function values — this table only has to order captures.
pub const PIECE_VALUE: [i32; 6] = [
    100,   // Pawn
    320,   // Knight
    330,   // Bishop
    500,   // Rook
    900,   // Queen
    20000, // King
];

pub fn piece_value(role: Role) -> i32 {
    PIECE_VALUE[role as usize - 1]
}

/// Roles ordered by exchange value, cheapest first. Matches `PIECE_VALUE`.
const ROLES_BY_VALUE: [Role; 6] = [
    Role::Pawn,
    Role::Knight,
    Role::Bishop,
    Role::Rook,
    Role::Queen,
    Role::King,
];

/// Cheapest piece of `attackers` (a set of squares already known to attack the
/// exchange square).
fn least_valuable_attacker(board: &Board, attackers: Bitboard) -> Option<(Square, Role)> {
    for role in ROLES_BY_VALUE {
        if let Some(sq) = (attackers & board.by_role(role)).first() {
            return Some((sq, role));
        }
    }
    None
}

/// Value of continuing the exchange on `sq` for `side`, who captures next.
///
/// `victim_value` is what the piece currently standing on `sq` is worth.
/// `occupied` is the live occupancy: pieces already spent in the exchange have
/// been cleared from it, which is what lets `attacks_to` pick up X-rays — a
/// slider behind a departed attacker joins the exchange on the next round.
///
/// The result is never negative: the side to move may always decline to capture,
/// which is exactly the negamax "stand pat" bound.
fn exchange_gain(
    board: &Board,
    sq: Square,
    side: Color,
    occupied: Bitboard,
    victim_value: i32,
) -> i32 {
    // `attacks_to` looks at the whole board, so mask with the live occupancy to
    // drop pieces that have already been captured or have already moved in.
    let attackers = board.attacks_to(sq, side, occupied) & occupied;
    let Some((from, role)) = least_valuable_attacker(board, attackers) else {
        return 0;
    };

    let next_occupied = occupied.without(from);

    // A king may only capture when nothing of the opponent's is left defending
    // the square; otherwise the capture would be illegal.
    if role == Role::King {
        let defenders = board.attacks_to(sq, side.other(), next_occupied) & next_occupied;
        if defenders.any() {
            return 0;
        }
    }

    let gain =
        victim_value - exchange_gain(board, sq, side.other(), next_occupied, piece_value(role));
    gain.max(0)
}

/// Material swing, in centipawns, of the exchange sequence on `mv`'s destination
/// square after `mv` is played. Negative means losing material.
pub fn see(pos: &Chess, mv: Move) -> i32 {
    let board = pos.board();
    let us = pos.turn();
    let to = mv.to();

    // Castling moves no piece onto a contested square in the exchange sense.
    if mv.is_castle() {
        return 0;
    }
    // Crazyhouse drops are out of scope.
    let Some(from) = mv.from() else {
        return 0;
    };

    let mut occupied = board.occupied();
    // The moving piece leaves `from` — this is what opens X-ray lines through it.
    occupied.discard(from);

    let mut captured = match mv.capture() {
        Some(role) => piece_value(role),
        None => 0,
    };
    if mv.is_en_passant() {
        // The captured pawn is not on `to`; clear its own square too.
        occupied.discard(Square::from_coords(to.file(), from.rank()));
    }

    // A promotion swaps the pawn for the new piece: the extra material is won
    // immediately, and it is the promoted piece that stands on the square.
    let mut standing = piece_value(mv.role());
    if let Some(promoted) = mv.promotion() {
        captured += piece_value(promoted) - piece_value(Role::Pawn);
        standing = piece_value(promoted);
    }
    occupied.add(to);

    captured - exchange_gain(board, to, us.other(), occupied, standing)
}

/// Settlement value for the piece standing on `sq` if the opponent starts an
/// exchange there. Negative means hanging — either free to take, or losing even
/// after the recapture.
pub fn see_square(pos: &Chess, sq: Square) -> i32 {
    let board = pos.board();
    let Some(piece) = board.piece_at(sq) else {
        return 0;
    };
    // A king is never actually captured, so an exchange on its square is
    // meaningless; check is not "hanging".
    if piece.role == Role::King {
        return 0;
    }
    // The opponent starts the exchange; the result is reported from the point of
    // view of the piece's owner, hence the negation.
    -exchange_gain(
        board,
        sq,
        piece.color.other(),
        board.occupied(),
        piece_value(piece.role),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_fen;
    use shakmaty::uci::UciMove;

    /// Build the move from a UCI string in the given position.
    fn mv(pos: &Chess, uci: &str) -> Move {
        let uci: UciMove = uci.parse().expect("valid uci");
        uci.to_move(pos).expect("legal move")
    }

    fn sq(name: &str) -> Square {
        name.parse().expect("valid square")
    }

    #[test]
    fn piece_values_are_ordered() {
        assert!(piece_value(Role::Pawn) < piece_value(Role::Knight));
        assert!(piece_value(Role::Knight) < piece_value(Role::Bishop));
        assert!(piece_value(Role::Bishop) < piece_value(Role::Rook));
        assert!(piece_value(Role::Rook) < piece_value(Role::Queen));
        assert!(piece_value(Role::Queen) < piece_value(Role::King));
    }

    #[test]
    fn quiet_move_to_a_safe_square_is_zero() {
        let pos = parse_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, mv(&pos, "e2e4")), 0);
    }

    /// (a) An undefended piece is taken for free.
    #[test]
    fn free_capture_of_an_undefended_rook() {
        // Black rook on d5 with nothing defending it; white bishop on g2 takes.
        let pos = parse_fen("4k3/8/8/3r4/8/8/6B1/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, mv(&pos, "g2d5")), piece_value(Role::Rook));
        // From black's point of view the rook is simply hanging.
        assert_eq!(see_square(&pos, sq("d5")), -piece_value(Role::Rook));
    }

    /// (b) A defended piece: the capture loses material.
    #[test]
    fn capturing_a_defended_pawn_loses_material() {
        // White knight on f3 takes the e5 pawn, which the d6 pawn defends.
        let pos = parse_fen("4k3/8/3p4/4p3/8/5N2/8/4K3 w - - 0 1").unwrap();
        // +100 for the pawn, -320 for the knight.
        assert_eq!(
            see(&pos, mv(&pos, "f3e5")),
            piece_value(Role::Pawn) - piece_value(Role::Knight)
        );
        // The pawn itself is not hanging: taking it costs the knight, so the
        // exchange settles at zero for its owner.
        assert_eq!(see_square(&pos, sq("e5")), 0);
    }

    /// (c) An X-ray: a slider behind the first attacker joins the exchange.
    #[test]
    fn xray_changes_the_outcome() {
        // Black queen d8 stands behind the black rook d6; both bear on d4.
        // The white pawn on d4 is defended by the c3 pawn only.
        let pos = parse_fen("3qk3/8/3r4/8/3P4/2P5/8/4K3 b - - 0 1").unwrap();
        // Rxd4 cxd4 Qxd4: black wins a pawn and a pawn, loses a rook = 100 - 500 + 100.
        assert_eq!(see(&pos, mv(&pos, "d6d4")), -300);

        // Same position without the queen behind: the exchange stops after cxd4.
        let no_xray = parse_fen("4k3/8/3r4/8/3P4/2P5/8/4K3 b - - 0 1").unwrap();
        assert_eq!(see(&no_xray, mv(&no_xray, "d6d4")), -400);
    }

    /// (c') An X-ray that opens because the *first* attacker vacated its square.
    #[test]
    fn xray_through_a_departed_attacker() {
        // White rooks doubled on e1/e2 aiming at the black pawn on e5, which is
        // defended by the f6 pawn. Re2xe5 fxe5 Rxe5 — the second rook only ever
        // reaches e5 because the first one left e2.
        let pos = parse_fen("4k3/8/5p2/4p3/8/8/4R3/4R1K1 w - - 0 1").unwrap();
        // +100 (pawn) -500 (rook) +100 (pawn back) = -300.
        assert_eq!(see(&pos, mv(&pos, "e2e5")), -300);
    }

    /// (d) A longer sequence that has to be settled rather than played out.
    #[test]
    fn long_exchange_is_settled_not_played_out() {
        // The classic SEE test positions (chessprogramming.org).
        //
        // 1. Rxe5 wins a pawn outright: nothing recaptures.
        let pos = parse_fen("1k1r4/1pp4p/p7/4p3/8/P5P1/1PP4P/2K1R3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, mv(&pos, "e1e5")), piece_value(Role::Pawn));

        // 2. Nxe5 on a square defended four deep. Played out to the end it would
        //    be Nxe5 Nxe5 Rxe5 Bxe5 Qxe5 Qxe5, but the settlement stops after
        //    Nxe5 Nxe5: white does not want to continue, so the swing is
        //    +100 (pawn) - 320 (knight).
        let pos = parse_fen("1k1r3q/1ppn3p/p4b2/4p3/8/P2N2P1/1PP1R1BP/2K1Q3 w - - 0 1").unwrap();
        assert_eq!(
            see(&pos, mv(&pos, "d3e5")),
            piece_value(Role::Pawn) - piece_value(Role::Knight)
        );
    }

    #[test]
    fn recapture_is_declined_when_it_loses_material() {
        // White: pawn c4, knight c3. Black: pawns d5/e6.
        // cxd5 exd5 and white stops — Nxd5 would just drop the knight later.
        let pos = parse_fen("4k3/8/4p3/3p4/2P5/2N5/8/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, mv(&pos, "c4d5")), piece_value(Role::Pawn));

        // With a black knight on f6 guarding d5 a second time, even the first
        // recapture is refused and the exchange settles at zero.
        let pos = parse_fen("4k3/8/4pn2/3p4/2P5/2N5/8/4K3 w - - 0 1").unwrap();
        assert_eq!(see(&pos, mv(&pos, "c4d5")), 0);
    }

    #[test]
    fn king_may_not_capture_a_defended_piece() {
        // Black pawn d5 defended by the e6 pawn; only the white king attacks it.
        let pos = parse_fen("4k3/8/4p3/3p4/3K4/8/8/8 w - - 0 1").unwrap();
        // The white king cannot take, so d5 is not hanging.
        assert_eq!(see_square(&pos, sq("d5")), 0);

        // Undefended, the king takes it for free.
        let pos = parse_fen("4k3/8/8/3p4/3K4/8/8/8 w - - 0 1").unwrap();
        assert_eq!(see_square(&pos, sq("d5")), -piece_value(Role::Pawn));
    }

    #[test]
    fn king_recapture_ends_the_sequence() {
        // White queen takes a defended pawn next to the black king; the king
        // recaptures because nothing else defends the square.
        let pos = parse_fen("4k3/3p4/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        assert_eq!(
            see(&pos, mv(&pos, "d1d7")),
            piece_value(Role::Pawn) - piece_value(Role::Queen)
        );
    }

    #[test]
    fn en_passant_is_handled() {
        let pos = parse_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
        assert_eq!(see(&pos, mv(&pos, "e5d6")), piece_value(Role::Pawn));
    }

    #[test]
    fn promotion_counts_the_extra_material() {
        // Pawn promotes on an empty, unattacked square.
        let pos = parse_fen("7k/3P4/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(
            see(&pos, mv(&pos, "d7d8q")),
            piece_value(Role::Queen) - piece_value(Role::Pawn)
        );
    }

    #[test]
    fn sacrifice_onto_an_attacked_square_is_negative() {
        // Bxh7+ — the black king simply takes the bishop back.
        let pos = parse_fen("6k1/7p/8/8/8/3B4/8/6K1 w - - 0 1").unwrap();
        assert_eq!(
            see(&pos, mv(&pos, "d3h7")),
            piece_value(Role::Pawn) - piece_value(Role::Bishop)
        );
    }

    #[test]
    fn see_square_of_an_empty_square_is_zero() {
        let pos = parse_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(see_square(&pos, sq("d4")), 0);
    }

    #[test]
    fn see_square_never_reports_a_gain() {
        // Whatever the position, being attacked cannot make a piece gain material.
        let pos =
            parse_fen("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 0 1").unwrap();
        for square in pos.board().occupied() {
            assert!(see_square(&pos, square) <= 0, "{square} gained material");
        }
    }

    #[test]
    fn starting_position_has_no_hanging_pieces() {
        let pos = Chess::default();
        for square in pos.board().occupied() {
            assert_eq!(see_square(&pos, square), 0, "{square} looks hanging");
        }
        // ... and no capture is available at all.
        for m in pos.legal_moves() {
            assert_eq!(see(&pos, m), 0);
        }
    }

    #[test]
    fn castling_is_neutral() {
        let pos = parse_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let castle = pos
            .legal_moves()
            .into_iter()
            .find(|m| m.is_castle())
            .expect("castling available");
        assert_eq!(see(&pos, castle), 0);
    }
}
