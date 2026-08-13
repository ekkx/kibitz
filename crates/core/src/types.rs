//! The single data structure handed to the LLM.
//!
//! **Anything the LLM says that is not in here is a hallucination.**

use crate::classify::Classification;
use crate::eval::Score;
use crate::feature::StaticDiff;
use crate::motif::Motif;
use crate::outlook::StrategicOutlook;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisContext {
    pub position: PositionInfo,
    /// MultiPV results verbatim. `[0]` is the best move.
    pub candidates: Vec<Candidate>,
    /// Rank of `played` within `candidates`, `None` if absent.
    pub played_rank: Option<usize>,
    pub played: PlayedMove,
    pub counterfactual: Option<Counterfactual>,
    pub static_diff: StaticDiff,
    pub outlook: Option<StrategicOutlook>,
    /// If this move left the opening book, the book line that matched up to it.
    pub opening: Option<OpeningInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub fen: String,
    /// "white" | "black"
    pub side_to_move: String,
    pub fullmove_number: u32,
    /// Search depth, recorded for reproducibility.
    pub depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub san: String,
    pub uci: String,
    pub score: Score,
    /// 0..1, from the point of view of the side to move in this position.
    pub win_prob: f64,
    /// SAN move list.
    pub pv: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayedMove {
    pub san: String,
    pub uci: String,
    pub win_prob_before: f64,
    pub win_prob_after: f64,
    /// `win_prob_after - win_prob_before`. **Negative means the move made things worse.**
    pub delta: f64,
    pub classification: Classification,
    /// Display value, 0..100.
    pub accuracy: f64,
}

/// "What would have happened otherwise" — contents depend on the classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Counterfactual {
    pub kind: CounterfactualKind,
    /// Position the replay starts from: after the move for `Refutation`,
    /// before it for `AlternativeCollapse`.
    pub start_fen: String,
    /// The line replayed on the board (SAN).
    pub pv: Vec<String>,
    pub motifs: Vec<Motif>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterfactualKind {
    /// Bad move: how the opponent punishes what was actually played.
    Refutation,
    /// Good move: how the position collapses if the second-best move is played instead.
    AlternativeCollapse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningInfo {
    pub eco: String,
    pub name: String,
    /// How many plies matched the book.
    pub matched_plies: u32,
}

/// Analysis of one position, attached to a node of the variation tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionAnalysis {
    pub fen: String,
    pub depth: u8,
    pub candidates: Vec<Candidate>,
    /// Classification of the move leading into this position. `None` at the root.
    pub context: Option<AnalysisContext>,
    /// Generated explanations, keyed by language code ("en", "ja", ...).
    #[serde(default)]
    pub explanations: std::collections::HashMap<String, String>,
}
