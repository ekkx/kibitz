//! kibitz-core — pure logic layer.
//!
//! Depends on neither the engine nor the network. Explanation quality is decided
//! here, so every public function is pinned down by unit tests.

pub mod classify;
pub mod eval;
pub mod feature;
pub mod motif;
pub mod outlook;
pub mod see;
pub mod tree;
pub mod types;

pub use classify::Classification;
pub use eval::{Score, win_prob};
pub use types::{AnalysisContext, Candidate, Counterfactual, PlayedMove};

/// Shared helper for restoring a position from FEN.
pub fn parse_fen(fen: &str) -> Result<shakmaty::Chess, FenError> {
    use shakmaty::{CastlingMode, fen::Fen};
    let f: Fen = fen.parse().map_err(|_| FenError(fen.to_string()))?;
    f.into_position(CastlingMode::Standard)
        .map_err(|_| FenError(fen.to_string()))
}

/// Normalized FEN for cache keys: drops the halfmove clock and fullmove number.
pub fn normalize_fen(fen: &str) -> String {
    fen.split_whitespace().take(4).collect::<Vec<_>>().join(" ")
}

#[derive(Debug, thiserror::Error)]
#[error("invalid FEN: {0}")]
pub struct FenError(pub String);
