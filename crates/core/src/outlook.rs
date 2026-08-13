//! StrategicOutlook — strategy and what the position is heading toward.
//!
//! Material for explaining "what is the opponent going for". Built as
//! **a summary of the engine's line, not as inference**.
//!
//! Implementation is mostly `feature` reuse: walk the PV to its end and take a
//! `StaticDiff` against the current position.

use crate::feature::{self, StaticDiff, role_name};
use crate::types::Candidate;
use serde::{Deserialize, Serialize};
use shakmaty::{Chess, EnPassantMode, Move, Position, Square, attacks, fen::Fen, san::San};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicOutlook {
    /// 10-12 ply of best play (SAN).
    pub long_pv: Vec<String>,
    pub terminal_fen: String,
    /// Feature diff from the current position to the end of the PV.
    pub terminal_diff: StaticDiff,
    pub consensus: CandidateConsensus,
}

/// What the top candidate moves agree on — the direction the position demands.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateConsensus {
    /// The piece every candidate move touches, if they all touch the same one.
    pub common_piece: Option<String>,
    /// Squares every candidate move targets.
    pub common_targets: Vec<String>,
}

/// Walk the PV to its end and diff the features against the current position.
///
/// If the PV contains an illegal move, stop there and use the valid prefix —
/// engine PVs are occasionally truncated or corrupt.
pub fn build(pos: &Chess, candidates: &[Candidate], max_plies: usize) -> StrategicOutlook {
    let mut terminal = pos.clone();
    let mut long_pv = Vec::new();

    if let Some(best) = candidates.first() {
        for san in best.pv.iter().take(max_plies) {
            let Ok(parsed) = san.parse::<San>() else {
                break;
            };
            let Ok(mv) = parsed.to_move(&terminal) else {
                break;
            };
            terminal.play_unchecked(mv);
            long_pv.push(san.clone());
        }
    }

    StrategicOutlook {
        long_pv,
        terminal_fen: Fen::from_position(&terminal, EnPassantMode::Legal).to_string(),
        terminal_diff: feature::diff(pos, &terminal),
        consensus: consensus(pos, candidates),
    }
}

/// Extract what the candidate moves have in common. Empty with one or zero candidates.
///
/// A candidate whose SAN does not fit the position is dropped rather than
/// treated as disagreement; if fewer than two usable candidates remain there is
/// nothing to agree on and the result is empty.
pub fn consensus(pos: &Chess, candidates: &[Candidate]) -> CandidateConsensus {
    let moves: Vec<Move> = candidates
        .iter()
        .filter_map(|c| c.san.parse::<San>().ok()?.to_move(pos).ok())
        .collect();
    if moves.len() < 2 {
        return CandidateConsensus::default();
    }

    // `common_piece` is a role, per DESIGN 8.6 — two different knights still
    // agree that the position calls for a knight move.
    let role = moves[0].role();
    let common_piece = moves
        .iter()
        .all(|m| m.role() == role)
        .then(|| role_name(role).to_string());

    let mut common = targets_of(pos, moves[0]);
    for &mv in &moves[1..] {
        let other = targets_of(pos, mv);
        common.retain(|sq| other.contains(sq));
    }
    common.sort();

    CandidateConsensus {
        common_piece,
        common_targets: common.iter().map(|sq| sq.to_string()).collect(),
    }
}

/// The squares a move aims at: where the piece lands, plus the enemy pieces it
/// attacks from there. Anything else would be a guess about intent.
fn targets_of(pos: &Chess, mv: Move) -> Vec<Square> {
    let mut after = pos.clone();
    if !after.is_legal(mv) {
        return Vec::new();
    }
    let mover = after.turn();
    after.play_unchecked(mv);

    let dest = mv.to();
    let mut out = vec![dest];
    let board = after.board();
    if let Some(piece) = board.piece_at(dest) {
        let hits = attacks::attacks(dest, piece, board.occupied()) & board.by_color(mover.other());
        out.extend(hits);
    }
    out.sort();
    out.dedup();
    out
}
