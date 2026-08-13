//! kibitz CLI.

mod report;

use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use kibitz_book::{Book, BookConfig};
use kibitz_engine::{Engine, EngineConfig};
use kibitz_llm::Language;
use kibitz_server::AppState;
use kibitz_store::Store;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "kibitz", about = "Interactive chess analysis that explains why")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyse a PGN and print the AnalysisContext as JSON.
    Analyze {
        pgn: std::path::PathBuf,
        /// Search depth. Defaults to the engine's own default so the CLI and
        /// `EngineConfig` can never disagree about what "no depth given" means.
        #[arg(long, default_value_t = kibitz_engine::DEFAULT_DEPTH)]
        depth: u8,
        /// Also generate LLM explanations and print a Markdown report.
        #[arg(long)]
        explain: bool,
        /// Explanation language ("en", "ja").
        #[arg(long, default_value = "en")]
        lang: String,
    },
    /// Start the HTTP server.
    Serve {
        #[arg(long, default_value_t = 7777)]
        port: u16,
        /// Address to bind. Defaults to loopback: this is a local single-user
        /// tool with no auth, so it must not be reachable from the network
        /// unless that is asked for explicitly. Containers need `0.0.0.0`,
        /// where the container's own port publishing is the boundary.
        #[arg(long, default_value = "127.0.0.1")]
        bind: std::net::IpAddr,
        /// Directory holding a built frontend. Defaults to `web/dist` when present.
        #[arg(long)]
        web: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logs go to stderr: `kibitz analyze game.pgn > out.json` has to produce a
    // file that is only JSON.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("KIBITZ_LOG")
                .unwrap_or_else(|_| "kibitz=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Analyze {
            pgn,
            depth,
            explain,
            lang,
        } => analyze(pgn, depth, explain, &lang).await,
        Command::Serve { port, bind, web } => serve(port, bind, web).await,
    }
}

// ─── analyze ────────────────────────────────────────────

async fn analyze(pgn_path: PathBuf, depth: u8, explain: bool, lang: &str) -> anyhow::Result<()> {
    let lang = Language::parse(lang).ok_or_else(|| anyhow!("unsupported language: {lang}"))?;
    let source = std::fs::read_to_string(&pgn_path)
        .with_context(|| format!("reading {}", pgn_path.display()))?;
    let mut game = kibitz_server::pgn::import(&source)
        .with_context(|| format!("parsing {}", pgn_path.display()))?;

    let store = open_store();
    let config = EngineConfig {
        depth,
        ..EngineConfig::default()
    };
    let multipv = config.multipv;
    let path_for_error = config.path.clone();
    let engine = Engine::spawn(config)
        .await
        .with_context(|| format!("starting stockfish at {path_for_error}"))?;

    let book = store
        .clone()
        .map(|store| Arc::new(Book::new(BookConfig::default(), store)));

    let pipeline = kibitz_server::pipeline::Pipeline::new(engine.clone(), depth, multipv)
        .with_store(store)
        .with_book(book);

    let mainline = game.tree.mainline();
    let total = mainline.len();
    let results = pipeline
        .analyze_path(&game.tree, &mainline, |_, done, _, _| {
            tracing::info!("analysed {done}/{total}");
        })
        .await?;

    for (node_id, analysis) in &results {
        game.tree.nodes[*node_id].analysis = Some(analysis.clone());
    }

    if explain {
        let provider = kibitz_llm::from_env()?;
        let models = kibitz_llm::ModelConfig::default();
        let mut explanations: Vec<(usize, String)> = Vec::new();

        // Section 12.1: only the moves worth spending an LLM call on.
        for (index, (_, analysis)) in results.iter().enumerate() {
            let Some(context) = analysis.context.as_ref() else {
                continue;
            };
            if !context.played.classification.deserves_explanation() {
                continue;
            }
            let mut stream = provider.explain(&models.narrate, lang, context).await?;
            let mut text = String::new();
            while let Some(chunk) = stream.next().await {
                text.push_str(&chunk?);
            }
            explanations.push((index, text));
        }

        let entries: Vec<report::MoveEntry<'_>> = results
            .iter()
            .enumerate()
            .filter(|(_, (_, analysis))| analysis.context.is_some())
            .map(|(index, (_, analysis))| report::MoveEntry {
                analysis,
                explanation: explanations
                    .iter()
                    .find(|(i, _)| *i == index)
                    .map(|(_, text)| text.as_str()),
            })
            .collect();

        print!("{}", report::render(&game.headers, &entries));
    } else {
        let output = serde_json::json!({
            "headers": game.headers,
            "tree": game.tree,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    engine.shutdown().await.ok();
    Ok(())
}

// ─── serve ──────────────────────────────────────────────

async fn serve(port: u16, bind: std::net::IpAddr, web: Option<PathBuf>) -> anyhow::Result<()> {
    let config = EngineConfig::default();
    let stockfish_path = config.path.clone();
    let depth = config.depth;
    let multipv = config.multipv;

    // Health has to be the truth, so it comes from the handshake of the very engine
    // that will serve the analysis; the frontend shows the result on startup.
    let (engine, health) = kibitz_server::start_engine(config).await;
    if engine.is_none() {
        tracing::warn!(
            "stockfish is unavailable at {stockfish_path} ({}); analysis endpoints will answer 503",
            health.error.as_deref().unwrap_or("unknown error")
        );
    }

    let store = open_store();
    let book = store
        .clone()
        .map(|store| Arc::new(Book::new(BookConfig::default(), store)));

    let llm = match kibitz_llm::from_env() {
        Ok(provider) => Some(Arc::from(provider)),
        Err(e) => {
            tracing::warn!("no llm provider configured: {e}");
            None
        }
    };

    let state = AppState {
        engine,
        store,
        sessions: kibitz_server::session::SessionStore::new(),
        models: kibitz_llm::ModelConfig::default(),
        llm,
        book,
        health,
        depth,
        multipv,
        gate: kibitz_server::gate::EngineGate::new(),
    };

    let mut router = kibitz_server::router(state);
    if let Some(dir) = web.or_else(default_web_dir) {
        tracing::info!("serving the frontend from {}", dir.display());
        router = kibitz_server::with_static_files(router, dir);
    } else {
        tracing::info!("no built frontend found; serving the API only");
    }

    let addr = SocketAddr::new(bind, port);
    kibitz_server::serve_router(router, addr).await
}

/// `web/dist` next to the working directory, or next to the binary for an
/// installed build.
fn default_web_dir() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("web/dist")];
    if let Ok(exe) = std::env::current_exe() {
        let mut dir: Option<&Path> = exe.parent();
        for _ in 0..4 {
            let Some(current) = dir else { break };
            candidates.push(current.join("web/dist"));
            dir = current.parent();
        }
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
}

/// The cache is a convenience, not a requirement: a database that will not open
/// is a warning, and everything keeps working without it.
fn open_store() -> Option<Store> {
    let path = kibitz_store::default_path();
    match Store::open(&path) {
        Ok(store) => Some(store),
        Err(e) => {
            tracing::warn!("cache unavailable at {}: {e}", path.display());
            None
        }
    }
}
