//! Static positional features, and their diff between two positions.
//!
//! This is what lets the LLM say "X improves / worsens". Everything is derived
//! mechanically from `shakmaty::attacks` and `Board::attacks_to()`.
//! **No estimates belong here** — every number must be something that was counted.

use crate::see;
use serde::{Deserialize, Serialize};
use shakmaty::{Bitboard, Chess, Color, File, Position, Rank, Role, Square, attacks};

/// Features of a single position. Per-colour rather than folded into one number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Features {
    /// Material balance in centipawns (white - black).
    pub material: i32,
    /// Squares whose occupant has a negative SEE.
    pub hanging: Vec<HangingPiece>,
    /// Number of pieces attacking d4/e4/d5/e5.
    pub center_control: ColorPair<u32>,
    pub king_safety: ColorPair<KingSafety>,
    pub pawn_structure: ColorPair<PawnStructure>,
    /// Half-open and fully open files.
    pub open_files: Vec<OpenFile>,
    /// Legal move count.
    pub mobility: ColorPair<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorPair<T> {
    pub white: T,
    pub black: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HangingPiece {
    pub square: String,
    pub role: String,
    pub color: String,
    /// SEE value in centipawns (negative).
    pub see: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KingSafety {
    /// Enemy attacks landing on the 8 squares around the king.
    pub attackers: u32,
    /// Missing pawns in front of the king.
    pub missing_shield_pawns: u32,
    pub king_square: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PawnStructure {
    pub isolated: Vec<String>,
    pub doubled: Vec<String>,
    pub passed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenFile {
    /// "a".."h"
    pub file: String,
    /// Fully open (no pawns of either colour) versus half-open.
    pub fully_open: bool,
    /// Squares of rooks and queens sitting on the file.
    pub occupied_by: Vec<String>,
}

/// Feature diff between two positions. This is what reaches the LLM.
///
/// Unchanged items are dropped. Handing over every feature makes the model
/// enumerate them flatly, so **only what actually changed is passed on**.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaticDiff {
    pub before: Features,
    pub after: Features,
    /// Human-readable summary of what moved. Empty means no notable static change.
    pub changes: Vec<FeatureChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureChange {
    /// "material" | "center_control" | "king_safety" | "pawn_structure"
    /// | "open_files" | "mobility" | "hanging"
    pub kind: String,
    /// Which side the change is for: "white" | "black" | "both".
    pub side: String,
    /// Numeric change, `None` where a direction would be meaningless.
    pub delta: Option<f64>,
    /// Squares involved.
    pub squares: Vec<String>,
}

/// The four central squares whose control is counted.
pub const CENTER: [Square; 4] = [Square::D4, Square::E4, Square::D5, Square::E5];

/// Material values for the balance figure, straight out of DESIGN 8.4
/// (P=1, N=B=3, R=5, Q=9), expressed in centipawns. Deliberately *not*
/// `see::PIECE_VALUE`: that table exists to order captures (N=320, B=330), this
/// one to report a balance a human would recognise. Kings weigh nothing — both
/// sides always have exactly one.
const MATERIAL_VALUE: [i32; 6] = [100, 300, 300, 500, 900, 0];

fn material_value(role: Role) -> i32 {
    MATERIAL_VALUE[role as usize - 1]
}

pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::Pawn => "pawn",
        Role::Knight => "knight",
        Role::Bishop => "bishop",
        Role::Rook => "rook",
        Role::Queen => "queen",
        Role::King => "king",
    }
}

pub(crate) fn color_name(color: Color) -> &'static str {
    if color.is_white() { "white" } else { "black" }
}

pub fn extract(pos: &Chess) -> Features {
    Features {
        material: material(pos),
        hanging: hanging(pos),
        center_control: center_control(pos),
        king_safety: king_safety(pos),
        pawn_structure: pawn_structure(pos),
        open_files: open_files(pos),
        mobility: mobility(pos),
    }
}

/// Material balance in centipawns, white minus black.
pub fn material(pos: &Chess) -> i32 {
    let board = pos.board();
    let mut total = 0;
    for role in [
        Role::Pawn,
        Role::Knight,
        Role::Bishop,
        Role::Rook,
        Role::Queen,
    ] {
        let value = material_value(role);
        total += value * board.by_piece(role.of(Color::White)).count() as i32;
        total -= value * board.by_piece(role.of(Color::Black)).count() as i32;
    }
    total
}

/// Every piece whose SEE settlement on its own square is negative.
///
/// Kings are skipped — they cannot be captured, so an exchange sequence on the
/// king square is meaningless.
pub fn hanging(pos: &Chess) -> Vec<HangingPiece> {
    let board = pos.board();
    let mut out = Vec::new();
    for sq in board.occupied() {
        let Some(piece) = board.piece_at(sq) else {
            continue;
        };
        if piece.role == Role::King {
            continue;
        }
        let value = see::see_square(pos, sq);
        if value < 0 {
            out.push(HangingPiece {
                square: sq.to_string(),
                role: role_name(piece.role).to_string(),
                color: color_name(piece.color).to_string(),
                see: value,
            });
        }
    }
    out
}

/// Attacks landing on d4/e4/d5/e5, summed over the four squares.
///
/// This is an *attack* count, not a piece count: a bishop eyeing both d4 and e5
/// contributes two. That is the intent — it measures how contested the centre is.
pub fn center_control(pos: &Chess) -> ColorPair<u32> {
    let board = pos.board();
    let occupied = board.occupied();
    let count = |color: Color| -> u32 {
        CENTER
            .iter()
            .map(|&sq| board.attacks_to(sq, color, occupied).count() as u32)
            .sum()
    };
    ColorPair {
        white: count(Color::White),
        black: count(Color::Black),
    }
}

pub fn king_safety(pos: &Chess) -> ColorPair<KingSafety> {
    ColorPair {
        white: king_safety_of(pos, Color::White),
        black: king_safety_of(pos, Color::Black),
    }
}

fn king_safety_of(pos: &Chess, color: Color) -> KingSafety {
    let board = pos.board();
    let Some(king) = board.king_of(color) else {
        return KingSafety::default();
    };
    let occupied = board.occupied();

    // Again an attack count, not a piece count: two attacks by the same rook on
    // two ring squares are two units of pressure.
    let attackers: u32 = attacks::king_attacks(king)
        .into_iter()
        .map(|sq| board.attacks_to(sq, color.other(), occupied).count() as u32)
        .sum();

    KingSafety {
        attackers,
        missing_shield_pawns: missing_shield_pawns(pos, color, king),
        king_square: Some(king.to_u32() as u8),
    }
}

/// Files of the 3-file block around the king with no friendly pawn on the two
/// ranks in front of it.
///
/// Only counted while the king is still on its own first three ranks. A king
/// that has walked up the board has no pawn shield to speak of, and reporting
/// "3 missing" for every such position would be pure noise.
fn missing_shield_pawns(pos: &Chess, color: Color, king: Square) -> u32 {
    let (king_file, king_rank) = king.coords();
    if color.relative_rank(king_rank).to_u32() > 2 {
        return 0;
    }
    let pawns = pos.board().by_piece(color.pawn());
    let forward: i32 = if color.is_white() { 1 } else { -1 };

    let mut missing = 0;
    for file_delta in -1..=1 {
        let Some(file) = king_file.offset(file_delta) else {
            continue; // the king is on the a- or h-file; there is no fourth file
        };
        let shielded = (1..=2).any(|step| {
            king_rank
                .offset(forward * step)
                .is_some_and(|rank| pawns.contains(Square::from_coords(file, rank)))
        });
        if !shielded {
            missing += 1;
        }
    }
    missing
}

pub fn pawn_structure(pos: &Chess) -> ColorPair<PawnStructure> {
    ColorPair {
        white: pawn_structure_of(pos, Color::White),
        black: pawn_structure_of(pos, Color::Black),
    }
}

fn pawn_structure_of(pos: &Chess, color: Color) -> PawnStructure {
    let board = pos.board();
    let own = board.by_piece(color.pawn());
    let enemy = board.by_piece(color.other().pawn());
    let forward: i32 = if color.is_white() { 1 } else { -1 };

    let mut out = PawnStructure::default();
    for sq in own {
        let (file, rank) = sq.coords();
        let name = sq.to_string();

        let neighbours = adjacent_files(file);
        if (own & neighbours).is_empty() {
            out.isolated.push(name.clone());
        }

        if (own & Bitboard::from_file(file)).count() > 1 {
            out.doubled.push(name.clone());
        }

        // Passed: no enemy pawn anywhere ahead on this file or either neighbour.
        // The textbook mechanical definition — a pawn blocked by a *friendly*
        // pawn in front of it still counts, because only enemy pawns can stop it
        // from queening.
        let span = neighbours | Bitboard::from_file(file);
        if (enemy & span & ahead_of(rank, forward)).is_empty() {
            out.passed.push(name);
        }
    }
    // Board order is file-within-rank, which reads as scrambled. Sort by name
    // so the lists are stable and comparable across positions.
    out.isolated.sort();
    out.doubled.sort();
    out.passed.sort();
    out
}

fn adjacent_files(file: File) -> Bitboard {
    let mut mask = Bitboard::EMPTY;
    for delta in [-1, 1] {
        if let Some(f) = file.offset(delta) {
            mask |= Bitboard::from_file(f);
        }
    }
    mask
}

/// Every square strictly ahead of `rank`, in the direction `forward`.
fn ahead_of(rank: Rank, forward: i32) -> Bitboard {
    let mut mask = Bitboard::EMPTY;
    let mut step = 1;
    while let Some(r) = rank.offset(forward * step) {
        mask |= Bitboard::from_rank(r);
        step += 1;
    }
    mask
}

/// Files with no pawn of at least one colour, plus the rooks and queens on them.
///
/// Half-open for one side is half-open for the other in this representation:
/// `fully_open` distinguishes the two cases, and `occupied_by` names the pieces
/// that actually get to use the file.
pub fn open_files(pos: &Chess) -> Vec<OpenFile> {
    let board = pos.board();
    let white_pawns = board.by_piece(Color::White.pawn());
    let black_pawns = board.by_piece(Color::Black.pawn());
    let heavy = board.by_role(Role::Rook) | board.by_role(Role::Queen);

    let mut out = Vec::new();
    for index in 0..8 {
        let file = File::new(index);
        let mask = Bitboard::from_file(file);
        let has_white = (white_pawns & mask).any();
        let has_black = (black_pawns & mask).any();
        if has_white && has_black {
            continue;
        }
        out.push(OpenFile {
            file: file.to_string(),
            fully_open: !has_white && !has_black,
            occupied_by: (heavy & mask)
                .into_iter()
                .map(|sq| sq.to_string())
                .collect(),
        });
    }
    out
}

/// Legal move count for each colour.
///
/// The side not to move is measured through a null move. When the side to move
/// is in check the null move is illegal (it would leave a king en prise), and
/// there is no meaningful "moves available to the opponent" figure — 0 is
/// reported for that side.
pub fn mobility(pos: &Chess) -> ColorPair<u32> {
    let to_move = pos.legal_moves().len() as u32;
    let waiting = pos
        .clone()
        .swap_turn()
        .map(|p| p.legal_moves().len() as u32)
        .unwrap_or(0);
    if pos.turn().is_white() {
        ColorPair {
            white: to_move,
            black: waiting,
        }
    } else {
        ColorPair {
            white: waiting,
            black: to_move,
        }
    }
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

/// Significance thresholds for `diff`.
///
/// The point of `changes` is to hand the model the handful of things that
/// actually moved. Everything below these thresholds is ordinary move-to-move
/// churn, and reporting it would drown the real change.
///
/// Any material change at all (values are multiples of 100, so this is really
/// "a pawn or more").
pub const MATERIAL_THRESHOLD: i32 = 100;
/// Every developing move shifts a centre attacker by one; two is a real change
/// in the grip on the centre.
pub const CENTER_CONTROL_THRESHOLD: i32 = 2;
/// One attacker appearing next to a king is routine, two is the start of
/// something.
pub const KING_ATTACKER_THRESHOLD: i32 = 2;
/// A single piece move routinely swings the legal move count by a few.
pub const MOBILITY_THRESHOLD: i32 = 5;

pub fn diff(before: &Chess, after: &Chess) -> StaticDiff {
    let a = extract(before);
    let b = extract(after);
    let changes = collect_changes(&a, &b);
    StaticDiff {
        before: a,
        after: b,
        changes,
    }
}

fn collect_changes(before: &Features, after: &Features) -> Vec<FeatureChange> {
    let mut changes = Vec::new();

    // Material. The balance is white-relative, so the sign already names the
    // side that gained; the entry is filed under "both".
    let material_delta = after.material - before.material;
    if material_delta.abs() >= MATERIAL_THRESHOLD {
        changes.push(FeatureChange {
            kind: "material".into(),
            side: "both".into(),
            delta: Some(material_delta as f64),
            squares: Vec::new(),
        });
    }

    // Hanging pieces, as a set of squares that changed status.
    let hanging_change = set_difference(
        &before
            .hanging
            .iter()
            .map(|h| h.square.clone())
            .collect::<Vec<_>>(),
        &after
            .hanging
            .iter()
            .map(|h| h.square.clone())
            .collect::<Vec<_>>(),
    );
    if !hanging_change.is_empty() {
        let side = side_of_hanging(before, after, &hanging_change);
        changes.push(FeatureChange {
            kind: "hanging".into(),
            side,
            delta: Some(after.hanging.len() as f64 - before.hanging.len() as f64),
            squares: hanging_change,
        });
    }

    for color in [Color::White, Color::Black] {
        let name = color_name(color).to_string();

        let center_delta =
            pick(&after.center_control, color) as i32 - pick(&before.center_control, color) as i32;
        if center_delta.abs() >= CENTER_CONTROL_THRESHOLD {
            changes.push(FeatureChange {
                kind: "center_control".into(),
                side: name.clone(),
                delta: Some(center_delta as f64),
                squares: CENTER.iter().map(|sq| sq.to_string()).collect(),
            });
        }

        let ks_before = pick(&before.king_safety, color);
        let ks_after = pick(&after.king_safety, color);
        let attacker_delta = ks_after.attackers as i32 - ks_before.attackers as i32;
        // Losing or regaining a shield pawn is always worth reporting, however
        // small the change in attackers. `delta` carries the attacker count
        // because that is the number with a direction; the shield count is
        // readable from `before` / `after`.
        let shield_moved = ks_after.missing_shield_pawns != ks_before.missing_shield_pawns;
        if attacker_delta.abs() >= KING_ATTACKER_THRESHOLD || shield_moved {
            changes.push(FeatureChange {
                kind: "king_safety".into(),
                side: name.clone(),
                delta: Some(attacker_delta as f64),
                squares: ks_after
                    .king_square
                    .map(|sq| vec![Square::new(sq as u32).to_string()])
                    .unwrap_or_default(),
            });
        }

        let ps_before = pick(&before.pawn_structure, color);
        let ps_after = pick(&after.pawn_structure, color);
        let mut pawn_squares = Vec::new();
        for (b, a) in [
            (&ps_before.isolated, &ps_after.isolated),
            (&ps_before.doubled, &ps_after.doubled),
            (&ps_before.passed, &ps_after.passed),
        ] {
            for sq in set_difference(b, a) {
                if !pawn_squares.contains(&sq) {
                    pawn_squares.push(sq);
                }
            }
        }
        if !pawn_squares.is_empty() {
            pawn_squares.sort();
            changes.push(FeatureChange {
                kind: "pawn_structure".into(),
                side: name.clone(),
                // "isolated became passed" has no direction to report.
                delta: None,
                squares: pawn_squares,
            });
        }

        let mobility_delta =
            pick(&after.mobility, color) as i32 - pick(&before.mobility, color) as i32;
        if mobility_delta.abs() >= MOBILITY_THRESHOLD {
            changes.push(FeatureChange {
                kind: "mobility".into(),
                side: name,
                delta: Some(mobility_delta as f64),
                squares: Vec::new(),
            });
        }
    }

    // Open files: which files changed status, and which heavy pieces changed
    // their claim on them.
    let mut file_squares = Vec::new();
    for index in 0..8u32 {
        let file = File::new(index).to_string();
        let b = before.open_files.iter().find(|f| f.file == file);
        let a = after.open_files.iter().find(|f| f.file == file);
        if b == a {
            continue;
        }
        file_squares.push(file);
        if let Some(a) = a {
            for sq in &a.occupied_by {
                if !file_squares.contains(sq) {
                    file_squares.push(sq.clone());
                }
            }
        }
    }
    if !file_squares.is_empty() {
        changes.push(FeatureChange {
            kind: "open_files".into(),
            side: "both".into(),
            delta: None,
            squares: file_squares,
        });
    }

    changes
}

/// Which colour owns the pieces whose hanging status changed. "both" when the
/// change straddles the two sides.
fn side_of_hanging(before: &Features, after: &Features, squares: &[String]) -> String {
    let mut colors = Vec::new();
    for sq in squares {
        let owner = after
            .hanging
            .iter()
            .chain(before.hanging.iter())
            .find(|h| &h.square == sq)
            .map(|h| h.color.clone());
        if let Some(color) = owner
            && !colors.contains(&color)
        {
            colors.push(color);
        }
    }
    match colors.len() {
        1 => colors.remove(0),
        _ => "both".to_string(),
    }
}

/// Symmetric difference of two square lists, sorted.
fn set_difference(before: &[String], after: &[String]) -> Vec<String> {
    let mut out: Vec<String> = before
        .iter()
        .filter(|s| !after.contains(s))
        .chain(after.iter().filter(|s| !before.contains(s)))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

fn pick<T: Clone>(pair: &ColorPair<T>, color: Color) -> T {
    if color.is_white() {
        pair.white.clone()
    } else {
        pair.black.clone()
    }
}
