//! Tactical motif detection.
//!
//! The substance of "why was that bad" is not the evaluation delta but **how the
//! opponent punishes it**, so this is the core of the explanation. Motifs are
//! detected per move along `Counterfactual.pv`.
//!
//! # Precision over recall
//!
//! A motif that is reported becomes a confident sentence in the explanation. A
//! false "fork" therefore costs far more than a missed one, so wherever DESIGN
//! 8.5 leaves room, the **stricter** reading is implemented. The individual
//! decisions are marked "strict:" in the comments below.
//!
//! The same argument applies to the *number* of motifs. [`detect`] is a per-move
//! rule set and says everything a move creates; [`detect_in_line`] is what the
//! explanation actually reads, and it reports the one or two things a coach
//! would name when walking the line. See its documentation for the cut.

use crate::feature::role_name;
use crate::see;
use serde::{Deserialize, Serialize};
use shakmaty::{Bitboard, Chess, Color, Move, Piece, Position, Role, Square, attacks, san::San};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Motif {
    /// The moved piece attacks two or more higher-valued pieces, or a king, at once.
    Fork {
        from: String,
        attacker: String,
        targets: Vec<String>,
    },
    /// An enemy piece stands on the attacker's line in front of a more valuable one.
    Pin {
        attacker: String,
        pinned: String,
        behind: String,
    },
    /// Same line, reversed order — the valuable piece is in front.
    Skewer {
        attacker: String,
        front: String,
        behind: String,
    },
    /// Moving a piece opens a line for a friendly piece behind it.
    DiscoveredAttack {
        moved_from: String,
        revealed: String,
        targets: Vec<String>,
    },
    /// A piece with negative SEE is actually captured in the line.
    Hanging {
        square: String,
        role: String,
        see: i32,
    },
    /// A rook or queen reaches the back rank while the king has no escape square.
    BackRank { attacker: String, king: String },
}

impl Motif {
    /// Stable identifier, also used as the glossary key when the explanation is
    /// rendered in a language other than English.
    pub fn key(&self) -> &'static str {
        match self {
            Motif::Fork { .. } => "fork",
            Motif::Pin { .. } => "pin",
            Motif::Skewer { .. } => "skewer",
            Motif::DiscoveredAttack { .. } => "discovered_attack",
            Motif::Hanging { .. } => "hanging",
            Motif::BackRank { .. } => "back_rank",
        }
    }
}

/// Motifs created by playing `mv` in `pos`.
pub fn detect(pos: &Chess, mv: Move) -> Vec<Motif> {
    let mut out = Vec::new();
    if !pos.is_legal(mv) {
        // Callers replay engine lines, which are occasionally corrupt. Silence
        // beats a motif read off a position that never existed.
        return out;
    }

    let mover = pos.turn();
    let mut after = pos.clone();
    after.play_unchecked(mv);

    // `hanging` is about the piece that was captured, so it is read from the
    // position *before* the move.
    if let Some(motif) = hanging(pos, mv) {
        out.push(motif);
    }

    let (from, dest) = match mv {
        Move::Normal { from, to, .. } | Move::EnPassant { from, to } => (from, to),
        // strict: castling moves two pieces at once, which makes "the moved
        // piece" ambiguous for every geometric rule below. Drops cannot occur
        // in standard chess.
        Move::Castle { .. } | Move::Put { .. } => return out,
    };

    // Reading the piece off the board after the move gets promotions right for
    // free — a pawn promoting to a knight forks as a knight.
    let Some(piece) = after.board().piece_at(dest) else {
        return out;
    };

    if let Some(motif) = fork(&after, mover, from, dest, piece) {
        out.push(motif);
    }
    out.extend(line_motifs(&after, mover, dest, piece));
    if let Some(motif) = discovered_attack(pos, &after, mover, from, dest) {
        out.push(motif);
    }
    if let Some(motif) = back_rank(&after, mover, dest, piece) {
        out.push(motif);
    }
    out
}

// ---------------------------------------------------------------------------
// Aggregation over a line
// ---------------------------------------------------------------------------

/// How deep into the line a motif still says something about the move being
/// explained. The punishment of a move starts immediately; by ply seven the
/// line has moved on to a different position, and a fork found there is not why
/// the move was bad.
const MOTIF_PLY_HORIZON: usize = 6;

/// Charged against a motif's [`severity`] for every ply it is buried under —
/// half a pawn per ply. A pin on the first ply outranks a bigger tactic four
/// plies later, because the first ply is the one the reader is being shown.
const PLY_DISCOUNT: i32 = 50;

/// A motif has to clear this, after the ply discount, to be worth a sentence.
/// Together with [`PLY_DISCOUNT`] this is what cuts the noise: a pin is
/// nameable anywhere inside the horizon, a hanging knight only in the first
/// four plies, and a pawn snatched in passing never at all.
const MOTIF_SCORE_FLOOR: i32 = 150;

/// Backstop on how many motifs one line may report. A coach explaining a line
/// names one or two things; three is already generous. The horizon and the
/// score floor are what normally do the cutting — this only catches a line that
/// really is full of tactics from the first move.
const MAX_MOTIFS_PER_LINE: usize = 3;

/// A motif with the position in the line it was found at.
struct Ranked {
    motif: Motif,
    ply: usize,
    score: i32,
}

/// Replay a PV (SAN) and report the handful of motifs that explain it.
///
/// [`detect`] answers "what does this move create", and a principal variation
/// is twenty of those moves, so their union is not an explanation — it is a
/// list. Three things make it one:
///
/// * **Exchanges are not hanging pieces.** Every recapture leaves the piece on
///   the contested square looking en prise, which is what made `hanging` fire
///   on nearly every ply. See [`loses_material_outright`].
/// * **One idea is reported once.** A pin that persists, or is renewed by a
///   second piece, is one pin. Motifs are deduplicated by [`subject`], which
///   keys on what the motif is *about* rather than on which piece does it.
/// * **Depth is a discount.** Motifs are scored by [`severity`] less
///   [`PLY_DISCOUNT`] per ply, and only those above [`MOTIF_SCORE_FLOOR`]
///   survive — so early, heavy tactics are kept and late, small ones are not.
///
/// Motifs from both sides are collected — who punished whom is evident from the
/// position within the line. The survivors are returned in the order they occur
/// in the line, so that they can be read off against the PV.
pub fn detect_in_line(start: &Chess, pv_san: &[String]) -> Vec<Motif> {
    let mut pos = start.clone();
    let mut ranked: Vec<Ranked> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    // Destination of the previous capture: the square an exchange is running on.
    let mut contested: Option<Square> = None;

    for (ply, san) in pv_san.iter().take(MOTIF_PLY_HORIZON).enumerate() {
        // Engine PVs are occasionally truncated or corrupt. Stop at the first
        // move that does not fit the position and keep what was collected.
        let Ok(parsed) = san.parse::<San>() else {
            break;
        };
        let Ok(mv) = parsed.to_move(&pos) else {
            break;
        };

        for motif in detect(&pos, mv) {
            if matches!(motif, Motif::Hanging { .. })
                && !loses_material_outright(&pos, mv, contested)
            {
                continue;
            }
            let score = severity(&motif) - (ply as i32) * PLY_DISCOUNT;
            if score < MOTIF_SCORE_FLOOR {
                continue;
            }
            let key = subject(&motif);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            ranked.push(Ranked { motif, ply, score });
        }

        contested = if mv.is_capture() { Some(mv.to()) } else { None };
        pos.play_unchecked(mv);
    }

    // Rank, cut, then put the survivors back into the order of the line.
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then(a.ply.cmp(&b.ply)));
    ranked.truncate(MAX_MOTIFS_PER_LINE);
    ranked.sort_by_key(|r| r.ply);
    ranked.into_iter().map(|r| r.motif).collect()
}

/// Whether a capture inside a line really wins the piece it takes, as opposed
/// to being one link of an exchange.
///
/// `contested` is the square the previous ply captured on, if any. Within a
/// principal variation this is what separates "that piece was left hanging"
/// from "the pieces came off on d5": a piece that is only standing on the
/// square because it captured there a moment ago has not been left anywhere.
///
/// strict: taking the piece that is *giving check* is forced housekeeping. A
/// piece that arrives with check has been given, not left standing, and the
/// reply that removes it is the only move rather than a punishment. This
/// deliberately costs recall — a piece dropped with check is not reported as
/// hanging.
fn loses_material_outright(pos: &Chess, mv: Move, contested: Option<Square>) -> bool {
    if contested == Some(mv.to()) {
        return false;
    }
    if pos.checkers().contains(mv.to()) {
        return false;
    }
    // `detect` has already established that the square settles badly for its
    // owner; this asks the complementary question — does the capture actually
    // come out ahead, or is it an even trade someone has to start?
    see::see(pos, mv) > 0
}

/// How much of the explanation a motif could carry, in centipawn-ish units so
/// that a pattern and the material a hanging piece costs are comparable.
fn severity(motif: &Motif) -> i32 {
    match motif {
        // Mate, or one move from it.
        Motif::BackRank { .. } => 900,
        // Every target beyond the second makes the fork harder to answer.
        Motif::Fork { targets, .. } => 500 + 100 * (targets.len() as i32 - 2),
        Motif::Skewer { .. } => 450,
        Motif::Pin { .. } => 400,
        Motif::DiscoveredAttack { .. } => 400,
        // What it actually costs, capped so that losing a queen does not
        // outrank being mated.
        Motif::Hanging { see, .. } => see.abs().min(800),
    }
}

/// Identity of the *idea* behind a motif, so that the same idea occurring on
/// several plies is reported once.
///
/// Deliberately coarser than the motif itself: which piece does the pinning is
/// detail, and a pin renewed by a second piece is still "that knight cannot
/// move". Keying on the pieces the motif is about collapses a pin that persists
/// across the line into the one pin it is.
fn subject(motif: &Motif) -> String {
    match motif {
        Motif::Fork {
            attacker, targets, ..
        } => format!("fork:{attacker}:{}", targets.join(",")),
        Motif::Pin { pinned, behind, .. } => format!("pin:{pinned}:{behind}"),
        Motif::Skewer { front, behind, .. } => format!("skewer:{front}:{behind}"),
        Motif::DiscoveredAttack {
            revealed, targets, ..
        } => format!("discovered_attack:{revealed}:{}", targets.join(",")),
        Motif::Hanging { square, role, .. } => format!("hanging:{square}:{role}"),
        Motif::BackRank { king, .. } => format!("back_rank:{king}"),
    }
}

// ---------------------------------------------------------------------------
// Individual rules
// ---------------------------------------------------------------------------

/// The moved piece now attacks two or more pieces it would profit from taking.
///
/// strict: a target counts only if it is the enemy king or **strictly** more
/// valuable than the forking piece. A queen "forking" two pawns is not a fork.
///
/// strict: the forking piece must be safe where it stands (see `stands_safely`).
/// A piece that can simply be taken has not forked anything.
fn fork(after: &Chess, mover: Color, from: Square, dest: Square, piece: Piece) -> Option<Motif> {
    let board = after.board();
    let attacker_value = see::piece_value(piece.role);
    let enemies = board.by_color(mover.other());

    let targets: Vec<Square> = (attacks::attacks(dest, piece, board.occupied()) & enemies)
        .into_iter()
        .filter(|&sq| {
            board
                .role_at(sq)
                .is_some_and(|role| role == Role::King || see::piece_value(role) > attacker_value)
        })
        .collect();

    if targets.len() < 2 || !stands_safely(after, mover, dest, piece) {
        return None;
    }

    Some(Motif::Fork {
        from: from.to_string(),
        attacker: dest.to_string(),
        targets: targets.iter().map(|sq| sq.to_string()).collect(),
    })
}

/// Whether a piece on `dest` is not simply losable.
///
/// Unattacked is safe. Attacked by something cheaper is never safe. Attacked by
/// an equal or dearer piece is safe only if the square is defended.
fn stands_safely(after: &Chess, mover: Color, dest: Square, piece: Piece) -> bool {
    let board = after.board();
    let occupied = board.occupied();
    let attackers = board.attacks_to(dest, mover.other(), occupied);
    if attackers.is_empty() {
        return true;
    }
    let value = see::piece_value(piece.role);
    let cheaper = attackers.into_iter().any(|sq| {
        board
            .role_at(sq)
            .is_some_and(|r| see::piece_value(r) < value)
    });
    if cheaper {
        return false;
    }
    board.attacks_to(dest, mover, occupied).any()
}

/// Pin and skewer share their geometry: a slider, an enemy piece on its line,
/// and a second enemy piece directly behind it. Which of the two it is depends
/// only on which of the pair is worth more.
///
/// strict: both pieces on the line must be enemies, and their values must
/// differ. Rook-behind-rook is neither a pin nor a skewer — nothing is won by
/// forcing the front piece to move.
///
/// strict: the slider must stand safely, exactly as for a fork. A "pin" by a
/// bishop the pinned pawn can simply take is not a pin.
///
/// strict: there must be something to win at the far end — the piece behind has
/// to outvalue the slider, or be undefended. Without that the geometry is real
/// but the motif is empty: a queen "pinning" a pawn against a king-defended
/// bishop wins nothing, and reporting it drowns the pins that matter. This
/// deliberately costs recall.
fn line_motifs(after: &Chess, mover: Color, dest: Square, piece: Piece) -> Vec<Motif> {
    let mut out = Vec::new();
    if !matches!(piece.role, Role::Bishop | Role::Rook | Role::Queen) {
        return out;
    }
    if !stands_safely(after, mover, dest, piece) {
        return out;
    }
    let board = after.board();
    let occupied = board.occupied();
    let enemies = board.by_color(mover.other());
    let slider_value = see::piece_value(piece.role);

    for front in attacks::attacks(dest, piece, occupied) & enemies {
        let Some(behind) = first_behind(dest, front, occupied) else {
            continue;
        };
        if !enemies.contains(behind) {
            continue;
        }
        let (Some(front_role), Some(behind_role)) = (board.role_at(front), board.role_at(behind))
        else {
            continue;
        };
        let front_value = see::piece_value(front_role);
        let behind_value = see::piece_value(behind_role);

        let worth_breaking_through = behind_value > slider_value
            || board.attacks_to(behind, mover.other(), occupied).is_empty();
        if !worth_breaking_through {
            continue;
        }

        if behind_value > front_value {
            out.push(Motif::Pin {
                attacker: dest.to_string(),
                pinned: front.to_string(),
                behind: behind.to_string(),
            });
        } else if front_value > behind_value {
            out.push(Motif::Skewer {
                attacker: dest.to_string(),
                front: front.to_string(),
                behind: behind.to_string(),
            });
        }
    }
    out
}

/// First occupied square on the far side of `through`, looking out from `from`.
fn first_behind(from: Square, through: Square, occupied: Bitboard) -> Option<Square> {
    (attacks::ray(from, through) & occupied)
        .into_iter()
        .find(|&sq| {
            // Beyond `through`, and nothing in between: the very next piece on the line.
            attacks::between(from, sq).contains(through)
                && (attacks::between(through, sq) & occupied).is_empty()
        })
}

/// Vacating a square opens a line for a piece that was standing behind it.
///
/// strict: only newly attacked enemy pieces count, and only those worth taking
/// — the enemy king, a piece worth more than the revealed attacker, or an
/// undefended one. A rook that now "attacks" a defended pawn has revealed
/// nothing worth saying out loud.
fn discovered_attack(
    before: &Chess,
    after: &Chess,
    mover: Color,
    from: Square,
    dest: Square,
) -> Option<Motif> {
    let board = after.board();
    let occupied = board.occupied();
    let enemies = board.by_color(mover.other());
    let sliders = board.by_color(mover)
        & (board.by_role(Role::Bishop) | board.by_role(Role::Rook) | board.by_role(Role::Queen));

    for slider_sq in sliders {
        if slider_sq == dest || slider_sq == from {
            continue;
        }
        let Some(slider) = board.piece_at(slider_sq) else {
            continue;
        };
        let was = attacks::attacks(slider_sq, slider, before.board().occupied());
        let now = attacks::attacks(slider_sq, slider, occupied);
        let slider_value = see::piece_value(slider.role);

        let targets: Vec<Square> = (now & !was & enemies)
            .into_iter()
            // The vacated square has to be the blocker that was in the way,
            // otherwise the new line has nothing to do with this move.
            .filter(|&target| attacks::between(slider_sq, target).contains(from))
            .filter(|&target| {
                board.role_at(target).is_some_and(|role| {
                    role == Role::King
                        || see::piece_value(role) > slider_value
                        || board.attacks_to(target, mover.other(), occupied).is_empty()
                })
            })
            .collect();

        if !targets.is_empty() {
            return Some(Motif::DiscoveredAttack {
                moved_from: from.to_string(),
                revealed: slider_sq.to_string(),
                targets: targets.iter().map(|sq| sq.to_string()).collect(),
            });
        }
    }
    None
}

/// A rook or queen lands on the enemy king's own back rank and the king cannot
/// step off it.
///
/// strict: the arriving piece must actually give check along that rank, and the
/// king must have no legal move at all — including capturing the intruder.
/// "The back rank looks weak" is a judgement, not a motif; this rule only fires
/// on the concrete pattern of a king walled in by its own pieces.
fn back_rank(after: &Chess, mover: Color, dest: Square, piece: Piece) -> Option<Motif> {
    if !matches!(piece.role, Role::Rook | Role::Queen) {
        return None;
    }
    let defender = mover.other();
    let king = after.board().king_of(defender)?;
    if king.rank() != defender.backrank() || dest.rank() != defender.backrank() {
        return None;
    }
    if !after.is_check() {
        return None;
    }
    // The check has to come from the piece that just arrived.
    if !after
        .board()
        .attacks_to(king, mover, after.board().occupied())
        .contains(dest)
    {
        return None;
    }
    if after
        .legal_moves()
        .iter()
        .any(|m| m.role() == Role::King || m.castling_side().is_some())
    {
        return None;
    }
    Some(Motif::BackRank {
        attacker: dest.to_string(),
        king: king.to_string(),
    })
}

/// A capture of a piece that was hanging — negative SEE on its own square
/// before it was taken.
///
/// strict: en passant is excluded. The captured pawn is not on the destination
/// square, so a SEE settlement there says nothing about it.
fn hanging(pos: &Chess, mv: Move) -> Option<Motif> {
    if !mv.is_capture() || mv.is_en_passant() {
        return None;
    }
    let square = mv.to();
    let role = pos.board().role_at(square)?;
    let value = see::see_square(pos, square);
    if value >= 0 {
        return None;
    }
    Some(Motif::Hanging {
        square: square.to_string(),
        role: role_name(role).to_string(),
        see: value,
    })
}
