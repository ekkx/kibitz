//! Stockfish UCI driver.
//!
//! Drives a native Stockfish binary over `tokio::process`. No UCI crate.
//!
//! Design points:
//! - **Everything funnels through a single task.** Interactive use means the user
//!   keeps trying moves, so the previous search has to be abandoned. Policy is
//!   "latest only": drop anything pending, send `stop` to whatever is running.
//! - **Stop on depth, not time** (`go depth N`). `movetime` is not reproducible,
//!   so results would disagree with what the SQLite cache holds.
//! - Terminal positions never reach the engine; `shakmaty` decides them.

use kibitz_core::types::Candidate;
use shakmaty::Chess;

pub mod uci;

mod driver;

use driver::{Request, RequestKind, SearchSpec};

pub use driver::UNKNOWN_ENGINE_NAME;

/// Default search depth, shared by [`EngineConfig::default`], the server and the
/// CLI's `--depth` so the three can never drift apart.
///
/// Chosen by measuring a fresh-cache sweep of `testdata/opera_game.pgn` (33 plies,
/// 9 threads, no opening book) on an idle 10-core machine, at the fixed
/// [`DEFAULT_MULTIPV`] width:
///
/// | depth | 10 | 12 | 14 |
/// |---|---|---|---|
/// | sweep | 4.0s | **9.6s** | 45.4s |
///
/// Analysis runs automatically when a game is loaded, so the budget is a few
/// seconds, not a coffee break. Depth 14 is already past that on this short game
/// and a typical 40-move club game is roughly 2.5x longer, so 12 is the last depth
/// that stays comfortable.
///
/// Below 12 the saving stops being worth it: depth 10 buys 5.6s but gives up two
/// full plies of lookahead, and 12 plies is what makes a three-to-four move tactic
/// reliably visible — which is exactly the audience's failure mode, since a
/// 1200-rated player's mistakes are hanging pieces and short tactics rather than
/// deep positional errors.
///
/// `--depth` and the API's `depth` parameter override this per request.
pub const DEFAULT_DEPTH: u8 = 12;

/// How many candidate moves every search produces.
///
/// This is a **fixed** width, and deliberately not a per-request parameter. Two
/// separate reasons, either of which would be enough:
///
/// 1. `multipv` is part of the analysis cache key, so a negotiable width would
///    fragment the cache — changing how many arrows the board draws would
///    silently re-analyse a game the user had already analysed.
/// 2. **The width is not classification-neutral.** MultiPV changes how the search
///    spends its effort, so the scores at rank 0 and rank 1 shift slightly with
///    it, and `classify`'s "only move" test compares exactly those two. Re-running
///    `testdata/opera_game.pgn` at depth 12 moved 9 of 33 verdicts between width 3
///    and width 5 (`Qb3` best -> great, `O-O-O` great -> best, `Bxd7` mistake ->
///    inaccuracy, and so on). The landmark verdicts are stable — `Nxb5` and
///    `Bxb5+` stay great, `Nxd7` a blunder — but the neighbourhood of a
///    threshold is not. A client-chosen width would therefore mean two users
///    seeing different judgements of the same game, and a settings toggle quietly
///    rewriting the move list. The server picks one width for everyone.
///
/// The number of arrows to draw is a view concern: the server always searches
/// five, and a client shows the first *n*.
///
/// Five is affordable because MultiPV's cost is badly non-linear in the width.
/// Over the 34 positions of that game at depth 12 on 9 threads — raw Stockfish
/// (median of three), and the whole pipeline fresh-cache (median of two):
///
/// | MultiPV | 1 | 3 | 5 |
/// |---|---|---|---|
/// | raw engine | 0.85s | 5.44s | 8.10s |
/// | full sweep | — | 6.9s | 10.0s |
///
/// Nearly all of the cost is the step away from a single PV (6.4x): one PV gets
/// aspiration windows and root pruning that any MultiPV search gives up. Widening
/// 3 -> 5 shares that work and adds only ~45%, which is the asymmetry that makes a
/// fixed width of 5 worth paying for. Re-measure before changing it again.
pub const DEFAULT_MULTIPV: usize = 5;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Path to the binary. Defaults to `stockfish`, resolved through PATH.
    pub path: String,
    pub threads: usize,
    /// Megabytes.
    pub hash: usize,
    pub multipv: usize,
    pub depth: u8,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            path: std::env::var("KIBITZ_STOCKFISH").unwrap_or_else(|_| "stockfish".into()),
            threads: std::thread::available_parallelism()
                .map(|n| (n.get().saturating_sub(1)).max(1))
                .unwrap_or(1),
            hash: 1024,
            multipv: DEFAULT_MULTIPV,
            depth: DEFAULT_DEPTH,
        }
    }
}

/// How much deeper than `EngineConfig::depth` [`Engine::long_pv`] searches when
/// no depth is given. `StrategicOutlook` wants 10-12 ply of principal variation,
/// which a MultiPV=3 search at the normal depth rarely produces.
const LONG_PV_EXTRA_DEPTH: u8 = 6;

/// Result of analysing one position.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub fen: String,
    pub depth: u8,
    /// MultiPV results, `[0]` best. Empty for a terminal position.
    pub candidates: Vec<Candidate>,
    /// Set when the position was already over.
    pub terminal: Option<Terminal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// The side to move has lost. win_prob = 0.0
    Checkmate,
    /// win_prob = 0.5
    Stalemate,
    /// Draw by insufficient material, the fifty-move rule, and so on. win_prob = 0.5
    Draw,
}

impl Terminal {
    pub fn win_prob(self) -> f64 {
        match self {
            Terminal::Checkmate => 0.0,
            Terminal::Stalemate | Terminal::Draw => 0.5,
        }
    }
}

/// Handle to the engine. Cloneable and shareable across tasks.
#[derive(Clone)]
pub struct Engine {
    inner: std::sync::Arc<EngineInner>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("name", &self.inner.name)
            .field("config", &self.inner.config)
            .field("alive", &!self.inner.requests.is_closed())
            .finish()
    }
}

struct EngineInner {
    config: EngineConfig,
    /// The `id name` line from the handshake, e.g. `"Stockfish 18"`.
    name: String,
    /// Requests to the single task that owns the process. When the task is gone
    /// this send fails, which is how callers learn the engine died.
    requests: tokio::sync::mpsc::UnboundedSender<Request>,
}

impl Engine {
    /// Start Stockfish and complete the UCI handshake and option setup.
    pub async fn spawn(config: EngineConfig) -> Result<Engine, EngineError> {
        let (name, requests) = driver::start(&config).await?;
        Ok(Engine {
            inner: std::sync::Arc::new(EngineInner {
                config,
                name,
                requests,
            }),
        })
    }

    /// The configuration this engine was started with.
    pub fn config(&self) -> &EngineConfig {
        &self.inner.config
    }

    /// How the engine identified itself during the handshake, e.g.
    /// `"Stockfish 18"`, or [`UNKNOWN_ENGINE_NAME`] when it sent no `id name`.
    ///
    /// This is the engine's identity in the analysis cache key: two binaries that
    /// answer differently must never read each other's cached evaluations, because
    /// `delta` compares adjacent positions and mixing engines corrupts it.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Analyse one position.
    ///
    /// **Cancellation**: when a newer `analyze` arrives on the same `Engine`, the
    /// running search is stopped and the older call returns `Err(Cancelled)`.
    pub async fn analyze(
        &self,
        pos: &Chess,
        depth: Option<u8>,
    ) -> Result<SearchResult, EngineError> {
        let depth = depth.unwrap_or(self.inner.config.depth);
        let fen = position::fen(pos);

        // Terminal positions never reach the engine: `bestmove (none)` carries no
        // score and no PV, so there would be nothing to parse anyway.
        if let Some(terminal) = position::terminal(pos) {
            return Ok(SearchResult {
                fen,
                depth,
                candidates: Vec::new(),
                terminal: Some(terminal),
            });
        }

        let outcome = self
            .search(SearchSpec {
                fen: fen.clone(),
                depth,
                multipv: self.inner.config.multipv.max(1),
                searchmoves: None,
            })
            .await?;

        Ok(SearchResult {
            fen,
            depth: if outcome.depth == 0 {
                depth
            } else {
                outcome.depth
            },
            candidates: position::candidates(pos, &outcome.infos),
            terminal: None,
        })
    }

    /// Search a single specific move (`go depth N searchmoves <uci>`), used when
    /// the user tries an arbitrary move on the board.
    ///
    /// Returns [`EngineError::Protocol`] when `uci` is not a legal move in `pos`,
    /// which includes every terminal position (they have no legal moves at all).
    pub async fn analyze_move(
        &self,
        pos: &Chess,
        uci: &str,
        depth: Option<u8>,
    ) -> Result<Candidate, EngineError> {
        let depth = depth.unwrap_or(self.inner.config.depth);
        let normalized = position::normalize_uci(pos, uci)?;

        // MultiPV above 1 buys nothing here: `searchmoves` restricts the root to
        // the single move, so Stockfish emits one PV either way.
        let outcome = self
            .search(SearchSpec {
                fen: position::fen(pos),
                depth,
                multipv: 1,
                searchmoves: Some(normalized),
            })
            .await?;

        position::candidates(pos, &outcome.infos)
            .into_iter()
            .next()
            .ok_or_else(|| EngineError::Protocol(format!("no PV returned for searchmoves {uci}")))
    }

    /// Fetch a long PV for `StrategicOutlook` (MultiPV=1, searched deeper).
    ///
    /// MultiPV is a global UCI option, so it is switched inside the task that owns
    /// the process — a concurrent [`Engine::analyze`] can never observe the
    /// narrowed value.
    pub async fn long_pv(&self, pos: &Chess, depth: Option<u8>) -> Result<Candidate, EngineError> {
        let depth = depth.unwrap_or_else(|| {
            self.inner
                .config
                .depth
                .saturating_add(LONG_PV_EXTRA_DEPTH)
        });

        if position::terminal(pos).is_some() {
            return Err(EngineError::Protocol(
                "cannot build a long PV for a terminal position".into(),
            ));
        }

        let outcome = self
            .search(SearchSpec {
                fen: position::fen(pos),
                depth,
                multipv: 1,
                searchmoves: None,
            })
            .await?;

        position::candidates(pos, &outcome.infos)
            .into_iter()
            .next()
            .ok_or_else(|| EngineError::Protocol("no PV returned for the long search".into()))
    }

    pub async fn shutdown(&self) -> Result<(), EngineError> {
        match self.dispatch(RequestKind::Shutdown).await {
            // The task exiting before it could answer means the process is
            // already gone, which is exactly what was asked for.
            Ok(_) | Err(EngineError::Died) => Ok(()),
            Err(err) => Err(err),
        }
    }

    async fn search(&self, spec: SearchSpec) -> Result<driver::SearchOutcome, EngineError> {
        self.dispatch(RequestKind::Search(spec)).await
    }

    async fn dispatch(&self, kind: RequestKind) -> Result<driver::SearchOutcome, EngineError> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.inner
            .requests
            .send(Request { kind, reply })
            .map_err(|_| EngineError::Died)?;
        // A dropped reply channel means the task died mid-flight. Never a hang.
        answer.await.map_err(|_| EngineError::Died)?
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("failed to spawn stockfish at {0}: {1}")]
    Spawn(String, std::io::Error),
    #[error("engine process died")]
    Died,
    #[error("search was cancelled by a newer request")]
    Cancelled,
    #[error("unexpected UCI output: {0}")]
    Protocol(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Everything that needs a chess board rather than the process.
mod position {
    use super::{EngineError, Terminal};
    use crate::uci::InfoLine;
    use kibitz_core::types::Candidate;
    use shakmaty::fen::Fen;
    use shakmaty::san::SanPlus;
    use shakmaty::uci::UciMove;
    use shakmaty::{CastlingMode, Chess, EnPassantMode, Position};

    /// Halfmove clock at which the fifty-move rule applies.
    const FIFTY_MOVE_PLIES: u32 = 100;

    pub fn fen(pos: &Chess) -> String {
        Fen::from_position(pos, EnPassantMode::Legal).to_string()
    }

    /// Decide terminality with shakmaty so the engine is never asked about a
    /// position it cannot search.
    ///
    /// Checkmate is tested first: a position can be both mate and past the
    /// fifty-move count, and mate wins.
    pub fn terminal(pos: &Chess) -> Option<Terminal> {
        if pos.is_checkmate() {
            Some(Terminal::Checkmate)
        } else if pos.is_stalemate() {
            Some(Terminal::Stalemate)
        } else if pos.is_insufficient_material() || pos.halfmoves() >= FIFTY_MOVE_PLIES {
            Some(Terminal::Draw)
        } else {
            None
        }
    }

    /// Parse a UCI move against `pos` and print it back in canonical form.
    pub fn normalize_uci(pos: &Chess, uci: &str) -> Result<String, EngineError> {
        let parsed: UciMove = uci
            .parse()
            .map_err(|_| EngineError::Protocol(format!("not a UCI move: {uci}")))?;
        let played = parsed
            .to_move(pos)
            .map_err(|_| EngineError::Protocol(format!("illegal move {uci} in this position")))?;
        Ok(UciMove::from_move(played, CastlingMode::Standard).to_string())
    }

    /// Convert info lines into candidates, in MultiPV order.
    ///
    /// Lines whose very first PV move is unplayable are dropped; a line that goes
    /// bad partway through is truncated at that point. Engines do occasionally
    /// emit a stale PV tail, especially right after `stop`.
    pub fn candidates(pos: &Chess, infos: &[InfoLine]) -> Vec<Candidate> {
        infos.iter().filter_map(|info| candidate(pos, info)).collect()
    }

    fn candidate(pos: &Chess, info: &InfoLine) -> Option<Candidate> {
        let mut walk = pos.clone();
        let mut san: Vec<String> = Vec::with_capacity(info.pv.len());
        let mut first_uci: Option<String> = None;

        for token in &info.pv {
            let Ok(parsed) = token.parse::<UciMove>() else {
                break;
            };
            let Ok(played) = parsed.to_move(&walk) else {
                break;
            };
            if first_uci.is_none() {
                first_uci = Some(UciMove::from_move(played, CastlingMode::Standard).to_string());
            }
            san.push(SanPlus::from_move_and_play_unchecked(&mut walk, played).to_string());
        }

        Some(Candidate {
            san: san.first()?.clone(),
            uci: first_uci?,
            score: info.score,
            // Mate is saturated by `Score::win_prob`; centipawns go through the
            // shared logistic transform in `kibitz_core::eval::win_prob`.
            win_prob: info.score.win_prob(),
            pv: san,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::fen::Fen;
    use shakmaty::{CastlingMode, Chess};

    fn chess(fen: &str) -> Chess {
        fen.parse::<Fen>()
            .expect("valid fen")
            .into_position(CastlingMode::Standard)
            .expect("legal position")
    }

    #[test]
    fn detects_checkmate() {
        // Fool's mate.
        let pos = chess("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
        assert_eq!(position::terminal(&pos), Some(Terminal::Checkmate));
        assert_eq!(Terminal::Checkmate.win_prob(), 0.0);
    }

    #[test]
    fn detects_stalemate() {
        let pos = chess("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
        assert_eq!(position::terminal(&pos), Some(Terminal::Stalemate));
    }

    #[test]
    fn detects_insufficient_material() {
        let pos = chess("8/8/4k3/8/8/3K4/8/7B w - - 0 1");
        assert_eq!(position::terminal(&pos), Some(Terminal::Draw));
    }

    #[test]
    fn detects_the_fifty_move_rule() {
        let pos = chess("8/8/4k3/8/8/3K4/R7/8 w - - 100 80");
        assert_eq!(position::terminal(&pos), Some(Terminal::Draw));
    }

    #[test]
    fn ordinary_positions_are_not_terminal() {
        assert_eq!(position::terminal(&Chess::default()), None);
    }

    #[test]
    fn fen_round_trips_the_start_position() {
        assert_eq!(
            position::fen(&Chess::default()),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        );
    }

    #[test]
    fn normalizes_castling_to_king_target_squares() {
        let pos = chess("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        assert_eq!(position::normalize_uci(&pos, "e1g1").unwrap(), "e1g1");
        assert!(position::normalize_uci(&pos, "e1e8").is_err());
        assert!(position::normalize_uci(&pos, "not-a-move").is_err());
    }

    #[test]
    fn converts_a_pv_to_san() {
        let info = crate::uci::parse_info(
            "info depth 12 multipv 1 score cp 46 pv e2e4 c7c5 g1f3 b8c6 f1b5",
        )
        .unwrap();
        let candidates = position::candidates(&Chess::default(), std::slice::from_ref(&info));
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].san, "e4");
        assert_eq!(candidates[0].uci, "e2e4");
        assert_eq!(candidates[0].pv, vec!["e4", "c5", "Nf3", "Nc6", "Bb5"]);
    }

    #[test]
    fn truncates_a_pv_at_the_first_unplayable_move() {
        let info =
            crate::uci::parse_info("info depth 5 multipv 1 score cp 10 pv e2e4 e7e5 h8h1").unwrap();
        let candidates = position::candidates(&Chess::default(), std::slice::from_ref(&info));
        assert_eq!(candidates[0].pv, vec!["e4", "e5"]);
    }

    #[test]
    fn drops_a_pv_whose_first_move_is_unplayable() {
        let info = crate::uci::parse_info("info depth 5 multipv 1 score cp 10 pv a1a8").unwrap();
        assert!(position::candidates(&Chess::default(), std::slice::from_ref(&info)).is_empty());
    }

    #[test]
    fn adds_check_and_mate_suffixes() {
        // Scholar's mate finish.
        let pos = chess("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 4 4");
        let info = crate::uci::parse_info("info depth 5 multipv 1 score mate 1 pv f3f7").unwrap();
        let candidates = position::candidates(&pos, std::slice::from_ref(&info));
        assert_eq!(candidates[0].san, "Qxf7#");
    }
}
