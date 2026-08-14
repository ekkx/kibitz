//! Variation tree.
//!
//! PGN is only used at import and export; the internal representation is an arena
//! (`Vec<Node>` + index). This avoids `Rc<RefCell<>>` borrow juggling, keeps
//! `NodeId` `Copy`, and serializes straight to JSON for the frontend.

use crate::types::PositionAnalysis;
use serde::{Deserialize, Serialize};
use shakmaty::{
    Chess, Move, Position,
    san::{San, SanPlus},
};

pub type NodeId = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameTree {
    pub nodes: Vec<Node>,
    pub root: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    /// `[0]` is the mainline, `[1..]` are variations.
    pub children: Vec<NodeId>,
    /// The move leading into this node. `None` at the root.
    pub san: Option<String>,
    pub uci: Option<String>,
    pub fen: String,
    /// Lazily computed.
    pub analysis: Option<PositionAnalysis>,
    /// The named opening this position belongs to, from the embedded ECO table.
    ///
    /// Display only. A name is not a verdict: `Classification::Book` is decided
    /// separately by `kibitz_book::Book::judge`, which asks the same table a
    /// different question (every position a line passes through, not just the
    /// one it is named after) and guards the answer. The two do not line up move
    /// for move — a book move usually has no name of its own.
    ///
    /// Filled in by whoever creates the node (`kibitz-server`): the ECO table
    /// lives in `kibitz-book`, which depends on this crate, so the tree only
    /// carries the field.
    pub opening: Option<crate::types::OpeningInfo>,
}

impl GameTree {
    pub fn new(start: &Chess) -> Self {
        let root = Node {
            id: 0,
            parent: None,
            children: Vec::new(),
            san: None,
            uci: None,
            fen: fen_of(start),
            analysis: None,
            opening: None,
        };
        GameTree {
            nodes: vec![root],
            root: 0,
        }
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Restore the position at the given node.
    pub fn position(&self, id: NodeId) -> Result<Chess, crate::FenError> {
        crate::parse_fen(&self.nodes[id].fen)
    }

    /// Add a move. If a child with the same move already exists, return it
    /// (**automatic merge**), so retrying a variation never grows the tree.
    pub fn play(&mut self, at: NodeId, mv: Move) -> Result<NodeId, TreeError> {
        let pos = self.position(at).map_err(|_| TreeError::BadPosition)?;
        if !pos.is_legal(mv) {
            return Err(TreeError::IllegalMove);
        }
        let uci = mv.to_uci(shakmaty::CastlingMode::Standard).to_string();

        if let Some(&existing) = self.nodes[at]
            .children
            .iter()
            .find(|&&c| self.nodes[c].uci.as_deref() == Some(uci.as_str()))
        {
            return Ok(existing);
        }

        // `SanPlus`, not `San`: the stored move keeps its `+` / `#` suffix, which
        // the move list renders directly. `San::from_ascii` skips the suffix when
        // parsing, so anything reading these strings back still works.
        let san = SanPlus::from_move(pos.clone(), mv).to_string();
        let mut next = pos.clone();
        next.play_unchecked(mv);

        let id = self.nodes.len();
        self.nodes.push(Node {
            id,
            parent: Some(at),
            children: Vec::new(),
            san: Some(san),
            uci: Some(uci),
            fen: fen_of(&next),
            analysis: None,
            opening: None,
        });
        self.nodes[at].children.push(id);
        Ok(id)
    }

    /// Add a move given in SAN.
    pub fn play_san(&mut self, at: NodeId, san: &str) -> Result<NodeId, TreeError> {
        let pos = self.position(at).map_err(|_| TreeError::BadPosition)?;
        let parsed: San = san.parse().map_err(|_| TreeError::IllegalMove)?;
        let mv = parsed.to_move(&pos).map_err(|_| TreeError::IllegalMove)?;
        self.play(at, mv)
    }

    /// The mainline, following `children[0]`. Includes the root.
    pub fn mainline(&self) -> Vec<NodeId> {
        let mut out = vec![self.root];
        let mut cur = self.root;
        while let Some(&next) = self.nodes[cur].children.first() {
            out.push(next);
            cur = next;
        }
        out
    }

    /// Path from the root to `id`, inclusive.
    pub fn path_to(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = Some(id);
        while let Some(n) = cur {
            out.push(n);
            cur = self.nodes[n].parent;
        }
        out.reverse();
        out
    }

    /// Whether `id` sits on the mainline.
    pub fn is_mainline(&self, id: NodeId) -> bool {
        self.path_to(id)
            .windows(2)
            .all(|w| self.nodes[w[0]].children.first() == Some(&w[1]))
    }
}

fn fen_of(pos: &Chess) -> String {
    shakmaty::fen::Fen::from_position(pos, shakmaty::EnPassantMode::Legal).to_string()
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TreeError {
    #[error("illegal move")]
    IllegalMove,
    #[error("node holds an unparsable position")]
    BadPosition,
}
