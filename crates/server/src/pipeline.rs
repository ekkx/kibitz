//! The analysis pass — section 10 of `docs/DESIGN.md`.
//!
//! Three rules shape this module:
//!
//! 1. **The opening book runs first.** A move that is still theory reaches
//!    neither the engine nor the LLM.
//! 2. **Every position is searched exactly once.** Position `N`'s MultiPV gives
//!    the alternatives to the move played there; position `N+1`'s evaluation,
//!    negated, gives the value of the move actually played. The first position of
//!    a line therefore needs MultiPV but no evaluation, and the last needs an
//!    evaluation but no MultiPV.
//! 3. **The sign convention of section 3 is absolute.**
//!    `win_prob_after = 1 - win_prob(P_{N+1})`, `delta = after - before`.
//!    Reversing it inverts every classification, so [`opponent_pov`] and
//!    [`classify_input`] are pure functions with tests pinning them down.
//!
//! Everything is cached through [`kibitz_store::Store`] on the normalized FEN,
//! the depth, the MultiPV width **and the engine's identity** — an evaluation is
//! only meaningful for the binary that computed it, and rule 2 above turns any
//! mixing of engines across adjacent positions straight into a wrong `delta`.

use kibitz_book::{Book, BookVerdict};
use kibitz_core::classify::{Classification, ClassifyInput, classify};
use kibitz_core::eval::Score;
use kibitz_core::tree::{GameTree, NodeId};
use kibitz_core::types::{
    AnalysisContext, Candidate, Counterfactual, CounterfactualKind, OpeningInfo, PlayedMove,
    PositionAnalysis, PositionInfo,
};
use kibitz_engine::{Engine, EngineError, SearchResult, Terminal};
use kibitz_store::Store;
use serde::{Deserialize, Serialize};
use shakmaty::{CastlingMode, Chess, Color, Move, Position, uci::UciMove};
use std::sync::Arc;

use crate::gate::{EngineGate, Lane};

/// Score used for a position whose side to move has been checkmated.
///
/// It is expressed as *being* mated so that [`opponent_pov`] turns it into a
/// mating score for the side that delivered mate. `Mate(0)` cannot be used: it
/// has no sign, so flipping it would be a no-op.
pub const MATED: Score = Score::Mate(-1);

/// How many plies of the best line `StrategicOutlook` walks.
pub const OUTLOOK_PLIES: usize = 12;

/// Drives book -> engine -> core for one position or for a whole line.
#[derive(Clone)]
pub struct Pipeline {
    engine: Engine,
    /// Cache. Optional so the server can run without a database.
    store: Option<Store>,
    /// Optional so tests and offline runs skip the Explorer API entirely.
    book: Option<Arc<Book>>,
    depth: u8,
    /// Must match the engine's configured MultiPV — it is part of the cache key.
    multipv: usize,
    /// Arbitration in front of the shared engine. `None` when nothing else can be
    /// using it — the CLI, and tests that drive one pipeline at a time.
    gate: Option<Arc<EngineGate>>,
    /// Which lane this pipeline's searches belong to. See [`crate::gate`].
    lane: Lane,
}

impl Pipeline {
    pub fn new(engine: Engine, depth: u8, multipv: usize) -> Pipeline {
        Pipeline {
            engine,
            store: None,
            book: None,
            depth,
            multipv,
            gate: None,
            // The interactive lane is the safe default: it is the one that may be
            // cancelled, so a caller that forgets to choose never accidentally
            // acquires a sweep's immunity.
            lane: Lane::Interactive,
        }
    }

    pub fn with_store(mut self, store: Option<Store>) -> Pipeline {
        self.store = store;
        self
    }

    pub fn with_book(mut self, book: Option<Arc<Book>>) -> Pipeline {
        self.book = book;
        self
    }

    /// Override the search depth for this run.
    pub fn with_depth(mut self, depth: u8) -> Pipeline {
        self.depth = depth;
        self
    }

    /// Arbitrate this pipeline's engine access against everything else using the
    /// same process.
    pub fn with_gate(mut self, gate: Option<Arc<EngineGate>>) -> Pipeline {
        self.gate = gate;
        self
    }

    /// Mark this pipeline as a whole-game sweep: its searches take the engine
    /// exclusively, one position at a time, and are never cancelled.
    pub fn for_sweep(mut self) -> Pipeline {
        self.lane = Lane::Sweep;
        self
    }

    pub fn depth(&self) -> u8 {
        self.depth
    }

    // ─── one node ───────────────────────────────────────

    /// Analyse a single node: MultiPV for the node's own position plus, when the
    /// node has a parent, the context for the move that led into it.
    ///
    /// This is the interactive path — the user asked for this position
    /// explicitly, so the strategic outlook is always built and the book is only
    /// consulted to label the move, never to skip work the user requested.
    pub async fn analyze_node(
        &self,
        tree: &GameTree,
        node: NodeId,
        depth: Option<u8>,
    ) -> Result<PositionAnalysis, PipelineError> {
        let depth = depth.unwrap_or(self.depth);
        let node_ref = tree.get(node).ok_or(PipelineError::NodeNotFound(node))?;
        let pos = tree
            .position(node)
            .map_err(|e| PipelineError::Position(e.to_string()))?;

        let here = self.search(&pos, depth, self.multipv).await?;

        let Some(parent) = node_ref.parent else {
            return Ok(PositionAnalysis {
                fen: node_ref.fen.clone(),
                depth,
                candidates: here.candidates,
                context: None,
                explanations: Default::default(),
            });
        };

        let before = tree
            .position(parent)
            .map_err(|e| PipelineError::Position(e.to_string()))?;
        let mv = move_into(tree, node)?;

        let verdict = match &self.book {
            Some(book) => {
                let uci = UciMove::from_move(mv, CastlingMode::Standard).to_string();
                // The book is an optimisation, not a dependency. Losing the
                // Explorer API (offline, rate limited, down) must not take the
                // whole analysis with it — the move is simply treated as out of
                // book and goes through the engine like any other.
                match book.judge(&before, &uci).await {
                    Ok(verdict) => verdict,
                    Err(e) => {
                        tracing::warn!("opening book unavailable, continuing without it: {e}");
                        BookVerdict::OutOfBook
                    }
                }
            }
            None => BookVerdict::OutOfBook,
        };

        let context = match verdict {
            BookVerdict::InBook(opening) => book_context(&before, mv, &pos, Some(opening), depth),
            other => {
                let parent_search = self.search(&before, depth, self.multipv).await?;
                build_context(
                    &before,
                    mv,
                    &pos,
                    &parent_search,
                    &here,
                    depth,
                    true,
                    left_book(&other),
                )
            }
        };

        Ok(PositionAnalysis {
            fen: node_ref.fen.clone(),
            depth,
            candidates: here.candidates,
            context: Some(context),
            explanations: Default::default(),
        })
    }

    // ─── a whole line ───────────────────────────────────

    /// Analyse a path through the tree (usually `tree.mainline()`), one search
    /// per position.
    ///
    /// `progress` is invoked after every node with `(node_id, done, total)`.
    pub async fn analyze_path<F>(
        &self,
        tree: &GameTree,
        path: &[NodeId],
        mut progress: F,
    ) -> Result<Vec<(NodeId, PositionAnalysis)>, PipelineError>
    where
        F: FnMut(NodeId, usize, usize, &PositionAnalysis),
    {
        if path.is_empty() {
            return Ok(Vec::new());
        }

        let mut positions = Vec::with_capacity(path.len());
        for &id in path {
            positions.push(
                tree.position(id)
                    .map_err(|e| PipelineError::Position(e.to_string()))?,
            );
        }
        let moves: Vec<Move> = path[1..]
            .iter()
            .map(|&id| move_into(tree, id))
            .collect::<Result<_, _>>()?;

        // 1. Book first. Everything it accepts skips both the engine and the LLM.
        let verdicts = self.judge_line(&positions[0], &moves).await?;

        // 2. Which positions actually have to be searched. A move that is book
        //    needs neither its own MultiPV nor the evaluation after it.
        let last = positions.len() - 1;
        let mut needed = vec![false; positions.len()];
        for (i, verdict) in verdicts.iter().enumerate() {
            if matches!(verdict, BookVerdict::InBook(_)) {
                continue;
            }
            needed[i] = true; // MultiPV at position N
            needed[i + 1] = true; // evaluation at position N+1
        }
        if last == 0 {
            // A bare position with no moves: the user still wants the alternatives.
            needed[0] = true;
        }

        // 3. Search and assemble in one pass, **one node at a time**.
        //
        //    Node `i` needs nothing beyond the searches at `i - 1` and `i`, so it
        //    is finished the moment search `i` is — and `progress` fires there.
        //    Running every search first and only then assembling would be
        //    equivalent in output and useless over SSE: the callback exists so the
        //    client can annotate the move list as the sweep walks the game, and a
        //    batch of thirty-four events delivered at the end is not progress.
        //
        //    The final position of the line gets an evaluation only — no move is
        //    played from it.
        let total = path.len();
        let mut searches: Vec<Option<SearchResult>> = Vec::with_capacity(positions.len());
        let mut out = Vec::with_capacity(path.len());

        for (i, &id) in path.iter().enumerate() {
            let search = if !needed[i] {
                None
            } else if i == last && last > 0 {
                Some(self.evaluate(&positions[i], self.depth).await?)
            } else {
                Some(self.search(&positions[i], self.depth, self.multipv).await?)
            };
            searches.push(search);

            let candidates = searches[i]
                .as_ref()
                .map(|s| s.candidates.clone())
                .unwrap_or_default();

            let context = if i == 0 {
                None
            } else {
                let verdict = &verdicts[i - 1];
                match verdict {
                    BookVerdict::InBook(opening) => Some(book_context(
                        &positions[i - 1],
                        moves[i - 1],
                        &positions[i],
                        Some(opening.clone()),
                        self.depth,
                    )),
                    other => match (&searches[i - 1], &searches[i]) {
                        (Some(before), Some(after)) => {
                            let ctx = build_context(
                                &positions[i - 1],
                                moves[i - 1],
                                &positions[i],
                                before,
                                after,
                                self.depth,
                                // The outlook doubles the payload, so a full-game
                                // sweep only builds it where an explanation follows.
                                false,
                                left_book(other),
                            );
                            Some(ctx)
                        }
                        _ => None,
                    },
                }
            };

            let analysis = PositionAnalysis {
                fen: tree.nodes[id].fen.clone(),
                depth: self.depth,
                candidates,
                context,
                explanations: Default::default(),
            };
            progress(id, i + 1, total, &analysis);
            out.push((id, analysis));
        }
        Ok(out)
    }

    async fn judge_line(
        &self,
        start: &Chess,
        moves: &[Move],
    ) -> Result<Vec<BookVerdict>, PipelineError> {
        let Some(book) = &self.book else {
            return Ok(vec![BookVerdict::OutOfBook; moves.len()]);
        };
        let ucis: Vec<String> = moves
            .iter()
            .map(|&mv| UciMove::from_move(mv, CastlingMode::Standard).to_string())
            .collect();
        // Same as `analyze_node`: a book failure degrades to "no book", it does
        // not abort the sweep.
        match book.judge_line(start, &ucis).await {
            Ok(verdicts) => Ok(verdicts),
            Err(e) => {
                tracing::warn!("opening book unavailable, continuing without it: {e}");
                Ok(vec![BookVerdict::OutOfBook; moves.len()])
            }
        }
    }

    // ─── engine + cache ─────────────────────────────────

    /// MultiPV search of one position, through the cache.
    pub async fn search(
        &self,
        pos: &Chess,
        depth: u8,
        multipv: usize,
    ) -> Result<SearchResult, PipelineError> {
        if let Some(terminal) = terminal_of(pos) {
            return Ok(SearchResult {
                fen: fen_of(pos),
                depth,
                candidates: Vec::new(),
                terminal: Some(terminal),
            });
        }
        let key = kibitz_core::normalize_fen(&fen_of(pos));
        if let Some(hit) = self.cache_get(&key, depth, multipv) {
            return Ok(hit);
        }
        // The gate is taken *after* the cache lookup: a position a running sweep
        // has already computed costs an interactive request no wait at all.
        let permit = self.permit().await;
        // The wait may have been long enough for a sweep to compute exactly this
        // position, so look again before paying for a duplicate search.
        if permit.is_some()
            && let Some(hit) = self.cache_get(&key, depth, multipv)
        {
            return Ok(hit);
        }
        let result = self.engine.analyze(pos, Some(depth)).await?;
        drop(permit);
        self.cache_put(&key, depth, multipv, &result);
        Ok(result)
    }

    /// Evaluation only (MultiPV 1), for the final position of a line.
    pub async fn evaluate(&self, pos: &Chess, depth: u8) -> Result<SearchResult, PipelineError> {
        if let Some(terminal) = terminal_of(pos) {
            return Ok(SearchResult {
                fen: fen_of(pos),
                depth,
                candidates: Vec::new(),
                terminal: Some(terminal),
            });
        }
        let key = kibitz_core::normalize_fen(&fen_of(pos));
        if let Some(hit) = self.cache_get(&key, depth, 1) {
            return Ok(hit);
        }
        let permit = self.permit().await;
        if permit.is_some()
            && let Some(hit) = self.cache_get(&key, depth, 1)
        {
            return Ok(hit);
        }
        let candidate = self.engine.long_pv(pos, Some(depth)).await?;
        drop(permit);
        let result = SearchResult {
            fen: fen_of(pos),
            depth,
            candidates: vec![candidate],
            terminal: None,
        };
        self.cache_put(&key, depth, 1, &result);
        Ok(result)
    }

    /// Wait for this pipeline's turn at the engine. `None` when no gate is
    /// attached, which means nothing else can be competing for the process.
    async fn permit(&self) -> Option<crate::gate::Permit> {
        match &self.gate {
            Some(gate) => Some(gate.acquire(self.lane).await),
            None => None,
        }
    }

    /// The cached evaluations belong to the engine that produced them, so the
    /// engine's `id name` is part of every cache key. Upgrading Stockfish, or
    /// pointing `KIBITZ_STOCKFISH` somewhere else, therefore starts a fresh
    /// namespace instead of mixing two engines' numbers inside one game — which
    /// would corrupt `delta`, a difference between adjacent positions.
    fn engine_name(&self) -> &str {
        self.engine.name()
    }

    fn cache_get(&self, key: &str, depth: u8, multipv: usize) -> Option<SearchResult> {
        let store = self.store.as_ref()?;
        let json = store
            .get_analysis(key, depth, multipv, self.engine_name())
            .ok()??;
        let cached: CachedSearch = serde_json::from_str(&json).ok()?;
        Some(cached.into())
    }

    fn cache_put(&self, key: &str, depth: u8, multipv: usize, result: &SearchResult) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let cached = CachedSearch::from(result);
        match serde_json::to_string(&cached) {
            Ok(json) => {
                if let Err(e) =
                    store.put_analysis(key, depth, multipv, self.engine_name(), &json)
                {
                    tracing::warn!("failed to cache analysis for {key}: {e}");
                }
            }
            Err(e) => tracing::warn!("failed to serialize analysis for {key}: {e}"),
        }
    }
}

// ─── pure helpers ───────────────────────────────────────

/// Section 3's point-of-view flip. Position `N+1` belongs to the opponent, so its
/// evaluation has to be negated before it can be read as the value of the move
/// played at position `N`.
pub fn opponent_pov(score: Score) -> Score {
    match score {
        Score::Cp(cp) => Score::Cp(-cp),
        Score::Mate(mate) => Score::Mate(-mate),
    }
}

/// A position's evaluation, from the side to move's point of view.
///
/// Terminal positions never reach the engine, so their value is fixed here:
/// checkmate is [`MATED`], every draw is a dead-even `Cp(0)`.
pub fn position_score(result: &SearchResult) -> Score {
    match result.terminal {
        Some(Terminal::Checkmate) => MATED,
        Some(Terminal::Stalemate) | Some(Terminal::Draw) => Score::Cp(0),
        None => result
            .candidates
            .first()
            .map(|c| c.score)
            .unwrap_or(Score::Cp(0)),
    }
}

/// Assemble the input to [`kibitz_core::classify::classify`].
///
/// This is where the sign convention lives: `best` and `second` come from the
/// search **before** the move, `played` from the search **after** it, negated.
pub fn classify_input(
    played_uci: &str,
    before: &SearchResult,
    after: &SearchResult,
) -> ClassifyInput {
    ClassifyInput {
        best: before
            .candidates
            .first()
            .map(|c| c.score)
            .unwrap_or_else(|| position_score(before)),
        second: before.candidates.get(1).map(|c| c.score),
        played: opponent_pov(position_score(after)),
        played_rank: before.candidates.iter().position(|c| c.uci == played_uci),
    }
}

/// The alternative whose collapse gets shown for a good move: the second-best
/// candidate, or — when the move played *is* the second-best — the next distinct
/// one, falling back to the best move.
pub fn alternative<'a>(candidates: &'a [Candidate], played_uci: &str) -> Option<&'a Candidate> {
    candidates
        .iter()
        .skip(1)
        .find(|c| c.uci != played_uci)
        .or_else(|| candidates.first().filter(|c| c.uci != played_uci))
}

/// Which line the board replays, and the position it starts from.
///
/// Bad move -> how the opponent punishes what was played, starting **after** the
/// move. Good move -> how things collapse if the second-best move is played
/// instead, starting **before** it. Motif detection is layered on top by
/// [`counterfactual`]; keeping the choice separate makes it testable on its own.
pub fn counterfactual_line(
    classification: Classification,
    before: &Chess,
    after: &Chess,
    played_uci: &str,
    before_search: &SearchResult,
    after_search: &SearchResult,
) -> Option<(CounterfactualKind, String, Vec<String>)> {
    if classification.is_mistake() {
        let pv = after_search.candidates.first()?.pv.clone();
        if pv.is_empty() {
            return None;
        }
        Some((CounterfactualKind::Refutation, fen_of(after), pv))
    } else {
        let alt = alternative(&before_search.candidates, played_uci)?;
        if alt.pv.is_empty() {
            return None;
        }
        Some((
            CounterfactualKind::AlternativeCollapse,
            fen_of(before),
            alt.pv.clone(),
        ))
    }
}

/// [`counterfactual_line`] plus the tactical motifs found along it.
pub fn counterfactual(
    classification: Classification,
    before: &Chess,
    after: &Chess,
    played_uci: &str,
    before_search: &SearchResult,
    after_search: &SearchResult,
) -> Option<Counterfactual> {
    let (kind, start_fen, pv) = counterfactual_line(
        classification,
        before,
        after,
        played_uci,
        before_search,
        after_search,
    )?;
    let start = match kind {
        CounterfactualKind::Refutation => after,
        CounterfactualKind::AlternativeCollapse => before,
    };
    let motifs = kibitz_core::motif::detect_in_line(start, &pv);
    Some(Counterfactual {
        kind,
        start_fen,
        pv,
        motifs,
    })
}

/// Build the full context for one played move.
///
/// `with_outlook` is off during a whole-game sweep — `StrategicOutlook` carries a
/// second `StaticDiff` and would double the size of every node.
#[allow(clippy::too_many_arguments)]
pub fn build_context(
    before: &Chess,
    mv: Move,
    after: &Chess,
    before_search: &SearchResult,
    after_search: &SearchResult,
    depth: u8,
    with_outlook: bool,
    opening: Option<OpeningInfo>,
) -> AnalysisContext {
    let san = shakmaty::san::San::from_move(before, mv).to_string();
    let uci = UciMove::from_move(mv, CastlingMode::Standard).to_string();

    let input = classify_input(&uci, before_search, after_search);
    let verdict = classify(&input);

    let counterfactual = counterfactual(
        verdict.classification,
        before,
        after,
        &uci,
        before_search,
        after_search,
    );

    let outlook = if with_outlook && !before_search.candidates.is_empty() {
        Some(kibitz_core::outlook::build(
            before,
            &before_search.candidates,
            OUTLOOK_PLIES,
        ))
    } else {
        None
    };

    AnalysisContext {
        position: position_info(before, depth),
        candidates: before_search.candidates.clone(),
        played_rank: input.played_rank,
        played: PlayedMove {
            san,
            uci,
            win_prob_before: verdict.win_prob_before,
            win_prob_after: verdict.win_prob_after,
            delta: verdict.delta,
            classification: verdict.classification,
            accuracy: kibitz_core::eval::accuracy(verdict.win_prob_before, verdict.win_prob_after),
        },
        counterfactual,
        static_diff: kibitz_core::feature::diff(before, after),
        outlook,
        opening,
    }
}

/// Context for a move the book accepted. The engine and the LLM are both skipped,
/// so there are no candidates and no counterfactual; the win probabilities are
/// placeholders and must not be plotted — `classification` is the only meaningful
/// judgement here.
pub fn book_context(
    before: &Chess,
    mv: Move,
    after: &Chess,
    opening: Option<OpeningInfo>,
    depth: u8,
) -> AnalysisContext {
    let san = shakmaty::san::San::from_move(before, mv).to_string();
    let uci = UciMove::from_move(mv, CastlingMode::Standard).to_string();
    AnalysisContext {
        position: position_info(before, depth),
        candidates: Vec::new(),
        played_rank: None,
        played: PlayedMove {
            san,
            uci,
            win_prob_before: 0.5,
            win_prob_after: 0.5,
            delta: 0.0,
            classification: Classification::Book,
            accuracy: 100.0,
        },
        counterfactual: None,
        static_diff: kibitz_core::feature::diff(before, after),
        outlook: None,
        opening,
    }
}

pub fn position_info(pos: &Chess, depth: u8) -> PositionInfo {
    PositionInfo {
        fen: fen_of(pos),
        side_to_move: match pos.turn() {
            Color::White => "white".into(),
            Color::Black => "black".into(),
        },
        fullmove_number: pos.fullmoves().get(),
        depth,
    }
}

/// Terminal state decided by `shakmaty`, so the engine is never asked about a
/// position with no legal moves.
pub fn terminal_of(pos: &Chess) -> Option<Terminal> {
    if pos.is_checkmate() {
        Some(Terminal::Checkmate)
    } else if pos.is_stalemate() {
        Some(Terminal::Stalemate)
    } else if pos.is_insufficient_material() || pos.halfmoves() >= 100 {
        Some(Terminal::Draw)
    } else {
        None
    }
}

fn left_book(verdict: &BookVerdict) -> Option<OpeningInfo> {
    match verdict {
        BookVerdict::LeftBook(opening) => opening.clone(),
        _ => None,
    }
}

fn move_into(tree: &GameTree, node: NodeId) -> Result<Move, PipelineError> {
    let node_ref = tree.get(node).ok_or(PipelineError::NodeNotFound(node))?;
    let parent = node_ref.parent.ok_or(PipelineError::NodeNotFound(node))?;
    let pos = tree
        .position(parent)
        .map_err(|e| PipelineError::Position(e.to_string()))?;
    let uci = node_ref
        .uci
        .as_deref()
        .ok_or_else(|| PipelineError::Position(format!("node {node} has no move")))?;
    uci.parse::<UciMove>()
        .map_err(|_| PipelineError::Position(format!("unparsable uci: {uci}")))?
        .to_move(&pos)
        .map_err(|_| PipelineError::Position(format!("illegal uci: {uci}")))
}

pub(crate) fn fen_of(pos: &Chess) -> String {
    crate::pgn::fen_of(pos)
}

// ─── cache representation ───────────────────────────────

/// `SearchResult` is not `Serialize` (`Terminal` is an engine-side enum), so the
/// cache round-trips through this.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSearch {
    fen: String,
    depth: u8,
    candidates: Vec<Candidate>,
    terminal: Option<String>,
}

impl From<&SearchResult> for CachedSearch {
    fn from(r: &SearchResult) -> CachedSearch {
        CachedSearch {
            fen: r.fen.clone(),
            depth: r.depth,
            candidates: r.candidates.clone(),
            terminal: r.terminal.map(|t| {
                match t {
                    Terminal::Checkmate => "checkmate",
                    Terminal::Stalemate => "stalemate",
                    Terminal::Draw => "draw",
                }
                .to_string()
            }),
        }
    }
}

impl From<CachedSearch> for SearchResult {
    fn from(c: CachedSearch) -> SearchResult {
        SearchResult {
            fen: c.fen,
            depth: c.depth,
            candidates: c.candidates,
            terminal: match c.terminal.as_deref() {
                Some("checkmate") => Some(Terminal::Checkmate),
                Some("stalemate") => Some(Terminal::Stalemate),
                Some("draw") => Some(Terminal::Draw),
                _ => None,
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("engine: {0}")]
    Engine(#[from] EngineError),
    #[error("book: {0}")]
    Book(String),
    #[error("invalid position: {0}")]
    Position(String),
    #[error("node {0} not found")]
    NodeNotFound(NodeId),
}

impl PipelineError {
    /// A newer request superseded this search — the API turns this into a 409.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, PipelineError::Engine(EngineError::Cancelled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(pos: &Chess, san: &str) -> Chess {
        let mv = san
            .parse::<shakmaty::san::San>()
            .unwrap()
            .to_move(pos)
            .unwrap();
        let mut next = pos.clone();
        next.play_unchecked(mv);
        next
    }

    fn candidate(uci: &str, san: &str, score: Score, pv: &[&str]) -> Candidate {
        Candidate {
            san: san.to_string(),
            uci: uci.to_string(),
            score,
            // The engine fills this in; the pure helpers here never read it.
            win_prob: 0.5,
            pv: pv.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn search(candidates: Vec<Candidate>) -> SearchResult {
        SearchResult {
            fen: "startpos".into(),
            depth: 20,
            candidates,
            terminal: None,
        }
    }

    fn terminal(t: Terminal) -> SearchResult {
        SearchResult {
            fen: "terminal".into(),
            depth: 20,
            candidates: Vec::new(),
            terminal: Some(t),
        }
    }

    #[test]
    fn opponent_pov_flips_both_kinds_of_score() {
        assert_eq!(opponent_pov(Score::Cp(120)), Score::Cp(-120));
        assert_eq!(opponent_pov(Score::Cp(-120)), Score::Cp(120));
        assert_eq!(opponent_pov(Score::Mate(3)), Score::Mate(-3));
        assert_eq!(opponent_pov(Score::Mate(-3)), Score::Mate(3));
        // Involution: flipping twice is the identity.
        for s in [Score::Cp(0), Score::Cp(77), Score::Mate(1), MATED] {
            assert_eq!(opponent_pov(opponent_pov(s)), s);
        }
    }

    #[test]
    fn a_mated_position_flips_into_a_mating_score() {
        // The move that delivered mate must read as winning for the mover, never
        // as a no-op. This is why MATED is Mate(-1) and not Mate(0).
        assert_eq!(position_score(&terminal(Terminal::Checkmate)), MATED);
        let played = opponent_pov(position_score(&terminal(Terminal::Checkmate)));
        assert_eq!(played, Score::Mate(1));
        assert!(matches!(played, Score::Mate(m) if m > 0));
    }

    #[test]
    fn draws_are_dead_even_from_either_side() {
        for t in [Terminal::Stalemate, Terminal::Draw] {
            assert_eq!(position_score(&terminal(t)), Score::Cp(0));
            assert_eq!(opponent_pov(position_score(&terminal(t))), Score::Cp(0));
        }
    }

    /// **The sign convention.** Position N is good for the side to move (+50);
    /// after the move played, position N+1 is very good for the *opponent*
    /// (+300 from their side). Negated, the move played is worth -300 to the
    /// mover — far below the +50 the best move held. Reverse this and every
    /// blunder in the tool reads as a brilliancy.
    #[test]
    fn played_value_comes_from_the_next_position_negated() {
        let before = search(vec![
            candidate("e2e4", "e4", Score::Cp(50), &["e4", "e5"]),
            candidate("d2d4", "d4", Score::Cp(20), &["d4", "d5"]),
            candidate("g1f3", "Nf3", Score::Cp(10), &["Nf3", "Nf6"]),
        ]);
        let after = search(vec![candidate("d8h4", "Qh4#", Score::Cp(300), &["Qh4"])]);

        let input = classify_input("g1f3", &before, &after);

        assert_eq!(input.best, Score::Cp(50), "best comes from position N");
        assert_eq!(input.second, Some(Score::Cp(20)));
        assert_eq!(
            input.played,
            Score::Cp(-300),
            "played comes from position N+1, negated"
        );
        assert_eq!(input.played_rank, Some(2));
    }

    #[test]
    fn a_move_outside_the_multipv_has_no_rank() {
        let before = search(vec![candidate("e2e4", "e4", Score::Cp(50), &["e4"])]);
        let after = search(vec![candidate("e7e5", "e5", Score::Cp(-40), &["e5"])]);
        let input = classify_input("a2a3", &before, &after);
        assert_eq!(input.played_rank, None);
        assert_eq!(input.second, None);
        // A position the opponent stands worse in is a gain for the mover.
        assert_eq!(input.played, Score::Cp(40));
    }

    #[test]
    fn a_move_delivering_mate_is_read_as_mating() {
        let before = search(vec![candidate("d1h5", "Qh5#", Score::Mate(1), &["Qh5#"])]);
        let input = classify_input("d1h5", &before, &terminal(Terminal::Checkmate));
        assert_eq!(input.best, Score::Mate(1));
        assert_eq!(input.played, Score::Mate(1));
    }

    #[test]
    fn giving_up_a_mate_reads_as_no_longer_mating() {
        // Best move was mate in 2; the move played leaves the opponent at -200,
        // so the mover is at +200 — good, but no mate. That is a Miss.
        let before = search(vec![
            candidate("f3f7", "Qxf7#", Score::Mate(2), &["Qxf7"]),
            candidate("f3f4", "Qf4", Score::Cp(200), &["Qf4"]),
        ]);
        let after = search(vec![candidate("e8e7", "Ke7", Score::Cp(-200), &["Ke7"])]);
        let input = classify_input("f3f4", &before, &after);
        assert!(input.best.is_mate());
        assert!(!input.played.is_mate());
        assert_eq!(input.played, Score::Cp(200));
    }

    #[test]
    fn alternative_skips_the_move_actually_played() {
        let cands = vec![
            candidate("e2e4", "e4", Score::Cp(50), &["e4"]),
            candidate("d2d4", "d4", Score::Cp(40), &["d4"]),
            candidate("g1f3", "Nf3", Score::Cp(30), &["Nf3"]),
        ];
        // Played the best move: the collapse to show is the second best.
        assert_eq!(alternative(&cands, "e2e4").unwrap().uci, "d2d4");
        // Played the second best: skip to the next distinct one.
        assert_eq!(alternative(&cands, "d2d4").unwrap().uci, "g1f3");
        // Played something outside the list: still the second best.
        assert_eq!(alternative(&cands, "a2a3").unwrap().uci, "d2d4");
        // Nothing to compare against.
        assert!(alternative(&cands[..1], "e2e4").is_none());
        assert!(alternative(&[], "e2e4").is_none());
    }

    #[test]
    fn bad_moves_replay_the_refutation_from_after_the_move() {
        let before = Chess::default();
        let after = play(&before, "e4");
        let before_search = search(vec![
            candidate("e2e4", "e4", Score::Cp(50), &["e4"]),
            candidate("d2d4", "d4", Score::Cp(40), &["d4", "d5", "c4"]),
        ]);
        let after_search = search(vec![candidate("e7e5", "e5", Score::Cp(20), &["e5", "Nf3"])]);

        let (kind, start_fen, pv) = counterfactual_line(
            Classification::Blunder,
            &before,
            &after,
            "e2e4",
            &before_search,
            &after_search,
        )
        .unwrap();
        assert_eq!(kind, CounterfactualKind::Refutation);
        assert_eq!(start_fen, fen_of(&after), "the punishment starts after the move");
        assert_eq!(pv, ["e5", "Nf3"]);
    }

    #[test]
    fn good_moves_replay_the_alternative_from_before_the_move() {
        let before = Chess::default();
        let after = play(&before, "e4");
        let before_search = search(vec![
            candidate("e2e4", "e4", Score::Cp(50), &["e4"]),
            candidate("d2d4", "d4", Score::Cp(40), &["d4", "d5", "c4"]),
        ]);
        let after_search = search(vec![candidate("e7e5", "e5", Score::Cp(20), &["e5"])]);

        for classification in [
            Classification::Great,
            Classification::Best,
            Classification::Excellent,
            Classification::Good,
        ] {
            let (kind, start_fen, pv) = counterfactual_line(
                classification,
                &before,
                &after,
                "e2e4",
                &before_search,
                &after_search,
            )
            .unwrap();
            assert_eq!(kind, CounterfactualKind::AlternativeCollapse);
            assert_eq!(start_fen, fen_of(&before), "{classification:?}");
            assert_eq!(pv, ["d4", "d5", "c4"], "{classification:?}");
        }
    }

    #[test]
    fn no_counterfactual_without_a_line_to_show() {
        let before = Chess::default();
        let after = before.clone();
        let one = search(vec![candidate("e2e4", "e4", Score::Cp(50), &["e4"])]);
        // Good move, single candidate: nothing to collapse.
        assert!(
            counterfactual_line(Classification::Best, &before, &after, "e2e4", &one, &one).is_none()
        );
        // Bad move, but the position after it is terminal: no punishment to replay.
        assert!(
            counterfactual_line(
                Classification::Blunder,
                &before,
                &after,
                "e2e4",
                &one,
                &terminal(Terminal::Checkmate),
            )
            .is_none()
        );
    }

    #[test]
    fn terminal_positions_never_reach_the_engine() {
        // Fool's mate.
        let mated = kibitz_core::parse_fen(
            "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
        )
        .unwrap();
        assert_eq!(terminal_of(&mated), Some(Terminal::Checkmate));

        let stalemate =
            kibitz_core::parse_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(terminal_of(&stalemate), Some(Terminal::Stalemate));

        let bare_kings = kibitz_core::parse_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(terminal_of(&bare_kings), Some(Terminal::Draw));

        assert_eq!(terminal_of(&Chess::default()), None);
    }

    #[test]
    fn cache_representation_round_trips() {
        let original = SearchResult {
            fen: "8/8/8/8/8/8/8/8 w - - 0 1".into(),
            depth: 18,
            candidates: vec![candidate("e2e4", "e4", Score::Mate(3), &["e4", "e5"])],
            terminal: Some(Terminal::Stalemate),
        };
        let json = serde_json::to_string(&CachedSearch::from(&original)).unwrap();
        let back: SearchResult = serde_json::from_str::<CachedSearch>(&json).unwrap().into();
        assert_eq!(back.fen, original.fen);
        assert_eq!(back.depth, original.depth);
        assert_eq!(back.terminal, original.terminal);
        assert_eq!(back.candidates[0].uci, "e2e4");
        assert_eq!(back.candidates[0].score, Score::Mate(3));
    }
}
