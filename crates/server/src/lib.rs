//! HTTP API. `docs/API.md` is the contract — **the frontend is written against it**,
//! so any change here must be mirrored there.

pub mod api;
pub mod gate;
pub mod opening;
pub mod pgn;
pub mod pipeline;
pub mod session;

use serde::Serialize;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct AppState {
    /// `None` when Stockfish could not be started. Every analysis endpoint then
    /// answers `503`, and `/api/health` says so — see [`EngineHealth`].
    pub engine: Option<kibitz_engine::Engine>,
    /// The SQLite cache. `None` disables caching; the server still works.
    pub store: Option<kibitz_store::Store>,
    pub sessions: session::SessionStore,
    pub models: kibitz_llm::ModelConfig,
    /// `None` when no LLM provider could be configured. `explain` and `ask` then
    /// answer `503`.
    pub llm: Option<Arc<dyn kibitz_llm::Provider>>,
    /// `None` disables the opening-book lookup (offline runs, tests).
    pub book: Option<Arc<kibitz_book::Book>>,
    pub health: EngineHealth,
    /// Default search depth when a request does not give one.
    pub depth: u8,
    /// MultiPV width the engine is configured with. Part of the cache key.
    pub multipv: usize,
    /// Arbitrates the one engine process between interactive analysis and
    /// whole-game sweeps. See [`gate`].
    pub gate: std::sync::Arc<gate::EngineGate>,
}

impl AppState {
    /// A state with nothing attached: no engine, no store, no book, no LLM.
    /// Session, PGN and tree endpoints work fully; everything else answers `503`.
    pub fn bare() -> AppState {
        AppState {
            engine: None,
            store: None,
            sessions: session::SessionStore::new(),
            models: kibitz_llm::ModelConfig::default(),
            llm: None,
            book: None,
            health: EngineHealth::unavailable(
                std::env::var("KIBITZ_STOCKFISH").unwrap_or_else(|_| "stockfish".into()),
                "engine not started",
            ),
            depth: kibitz_engine::DEFAULT_DEPTH,
            multipv: kibitz_engine::DEFAULT_MULTIPV,
            gate: gate::EngineGate::new(),
        }
    }

    /// A pipeline for the interactive lane — one node, at the user's request.
    pub fn pipeline(&self) -> Option<pipeline::Pipeline> {
        let engine = self.engine.clone()?;
        Some(
            pipeline::Pipeline::new(engine, self.depth, self.multipv)
                .with_store(self.store.clone())
                .with_book(self.book.clone())
                .with_gate(Some(self.gate.clone())),
        )
    }

    /// A pipeline for a whole-game sweep: same engine, but its searches take the
    /// process exclusively one position at a time and are never cancelled.
    pub fn sweep_pipeline(&self) -> Option<pipeline::Pipeline> {
        Some(self.pipeline()?.for_sweep())
    }
}

/// What `GET /api/health` reports. The frontend calls it on startup and tells the
/// user when the engine is missing, so **`ok` must be the truth** — it is read off
/// the running engine's own handshake ([`start_engine`]), never hard-coded.
#[derive(Clone, Debug, Serialize)]
pub struct EngineHealth {
    pub ok: bool,
    /// The `id name` line from the UCI handshake, e.g. `"Stockfish 18"`.
    /// `null` when the engine failed to start.
    pub engine: Option<String>,
    pub stockfish_path: String,
    /// Why it failed. Absent when everything is fine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl EngineHealth {
    pub fn ok(path: impl Into<String>, name: impl Into<String>) -> EngineHealth {
        EngineHealth {
            ok: true,
            engine: Some(name.into()),
            stockfish_path: path.into(),
            error: None,
        }
    }

    pub fn unavailable(path: impl Into<String>, error: impl Into<String>) -> EngineHealth {
        EngineHealth {
            ok: false,
            engine: None,
            stockfish_path: path.into(),
            error: Some(error.into()),
        }
    }
}

/// Start the engine and report on it in one step.
///
/// This replaces the old `probe_engine`, which spawned a throwaway second process
/// purely to read the `id name` line. Now that [`kibitz_engine::Engine::name`]
/// exposes it, the engine kibitz actually analyses with *is* the probe: one process,
/// one handshake, and no second code path that could report a healthy engine while
/// the real one failed to start. `EngineError` already distinguishes a binary that
/// will not start from one that will not speak UCI, so nothing is lost by dropping
/// the separate probe.
pub async fn start_engine(
    config: kibitz_engine::EngineConfig,
) -> (Option<kibitz_engine::Engine>, EngineHealth) {
    let path = config.path.clone();
    match kibitz_engine::Engine::spawn(config).await {
        Ok(engine) => {
            let health = EngineHealth::ok(&path, engine.name());
            (Some(engine), health)
        }
        Err(e) => (None, EngineHealth::unavailable(&path, e.to_string())),
    }
}

pub fn router(state: AppState) -> Router {
    api::routes().layer(cors_layer()).with_state(state)
}

/// Permissive CORS, but only for loopback origins — the Vite dev server runs on
/// `http://localhost:5173` while the API is on `127.0.0.1:7777`, and a local tool
/// has no business accepting anything else.
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            let Ok(origin) = origin.to_str() else {
                return false;
            };
            let host = origin
                .split("://")
                .nth(1)
                .unwrap_or(origin)
                .split(':')
                .next()
                .unwrap_or("");
            matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
        }))
        .allow_methods(Any)
        .allow_headers(Any)
}

/// Serve a built frontend from `/`, falling back to `index.html` so the SPA
/// router owns any path the API does not.
pub fn with_static_files(router: Router, dir: impl AsRef<Path>) -> Router {
    let dir = dir.as_ref();
    let index = dir.join("index.html");
    router.fallback_service(ServeDir::new(dir).not_found_service(ServeFile::new(index)))
}

pub async fn serve(state: AppState, addr: SocketAddr) -> anyhow::Result<()> {
    serve_router(router(state), addr).await
}

pub async fn serve_router(router: Router, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("kibitz listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router).await?;
    Ok(())
}
