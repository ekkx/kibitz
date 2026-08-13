//! The engine's identity is part of the analysis cache key, and of what
//! `/api/health` reports.
//!
//! Both properties used to be assumed rather than tested: the cache keyed only
//! `(fen, depth, multipv)`, so upgrading Stockfish silently served the old
//! binary's evaluations, and health came from a second, throwaway process that
//! could disagree with the engine actually doing the work.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use kibitz_core::eval::Score;
use kibitz_engine::{Engine, EngineConfig};
use kibitz_server::pipeline::Pipeline;
use kibitz_store::Store;
use shakmaty::{Chess, EnPassantMode, fen::Fen};

const DEPTH: u8 = 10;
const MULTIPV: usize = 3;

/// The cache key the pipeline computes for a position.
fn cache_key(pos: &Chess) -> String {
    kibitz_core::normalize_fen(&Fen::from_position(pos, EnPassantMode::Legal).to_string())
}

// ─── scripted engines ───────────────────────────────────

/// A stand-in that announces `name` and answers every `go` with `cp` — enough for
/// the pipeline to produce a cacheable result, and different enough between the
/// two engines that a cache hit from the wrong one is unmistakable.
#[cfg(unix)]
fn script(name: &str, cp: i32) -> String {
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id name {name}\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    go*) printf 'info depth 10 multipv 1 score cp {cp} pv e2e4 e7e5\ninfo depth 10 multipv 2 score cp {second} pv d2d4 d7d5\ninfo depth 10 multipv 3 score cp {third} pv g1f3 g8f6\nbestmove e2e4\n' ;;
    quit) exit 0 ;;
  esac
done
"#,
        second = cp - 5,
        third = cp - 10,
    )
}

/// Serializes "write the script, then exec it" — see the same guard in
/// `kibitz-engine`'s `fake_engine.rs` for why (`ETXTBSY` on Linux).
#[cfg(unix)]
static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(unix)]
async fn spawn_fake(name: &str, cp: i32) -> (Engine, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let _guard = SPAWN_LOCK.lock().await;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "kibitz-server-fake-engine-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::File::create(&path).expect("create script");
    file.write_all(script(name, cp).as_bytes())
        .expect("write script");
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("make executable");

    let engine = Engine::spawn(EngineConfig {
        path: path.display().to_string(),
        threads: 1,
        hash: 16,
        multipv: MULTIPV,
        depth: DEPTH,
    })
    .await
    .expect("spawn");

    (engine, path)
}

/// The bug this file exists for: analyse a position with one engine, then point
/// kibitz at another, and the second must search for itself instead of being
/// handed the first engine's numbers.
#[cfg(unix)]
#[tokio::test]
async fn two_engines_do_not_share_cache_entries() {
    let store = Store::in_memory().expect("store");
    let pos = Chess::default();
    let key = cache_key(&pos);

    let (alpha, alpha_script) = spawn_fake("kibitz-alpha", 25).await;
    let (beta, beta_script) = spawn_fake("kibitz-beta", -70).await;
    assert_eq!(alpha.name(), "kibitz-alpha");
    assert_eq!(beta.name(), "kibitz-beta");

    let alpha_pipeline = Pipeline::new(alpha.clone(), DEPTH, MULTIPV).with_store(Some(store.clone()));
    let beta_pipeline = Pipeline::new(beta.clone(), DEPTH, MULTIPV).with_store(Some(store.clone()));

    let first = alpha_pipeline
        .search(&pos, DEPTH, MULTIPV)
        .await
        .expect("alpha search");
    assert_eq!(first.candidates[0].score, Score::Cp(25));

    // Cached under alpha's name, and only under alpha's name.
    assert!(
        store
            .get_analysis(&key, DEPTH, MULTIPV, "kibitz-alpha")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_analysis(&key, DEPTH, MULTIPV, "kibitz-beta")
            .unwrap()
            .is_none(),
        "alpha's entry must not be visible to beta"
    );

    // Beta gets its own evaluation, not alpha's.
    let second = beta_pipeline
        .search(&pos, DEPTH, MULTIPV)
        .await
        .expect("beta search");
    assert_eq!(
        second.candidates[0].score,
        Score::Cp(-70),
        "beta was served alpha's cached evaluation"
    );

    // Two independent entries now coexist for the same (fen, depth, multipv).
    let alpha_json = store
        .get_analysis(&key, DEPTH, MULTIPV, "kibitz-alpha")
        .unwrap()
        .expect("alpha entry");
    let beta_json = store
        .get_analysis(&key, DEPTH, MULTIPV, "kibitz-beta")
        .unwrap()
        .expect("beta entry");
    assert_ne!(alpha_json, beta_json);

    // And each pipeline keeps hitting its own entry on a repeat search.
    let again = alpha_pipeline
        .search(&pos, DEPTH, MULTIPV)
        .await
        .expect("alpha search");
    assert_eq!(again.candidates[0].score, Score::Cp(25));

    alpha.shutdown().await.ok();
    beta.shutdown().await.ok();
    let _ = std::fs::remove_file(&alpha_script);
    let _ = std::fs::remove_file(&beta_script);
}

// ─── health, against the real binary ────────────────────

/// Where the test looks for Stockfish, or `None` when it is not installed.
fn stockfish_path() -> Option<PathBuf> {
    let configured = std::env::var("KIBITZ_STOCKFISH").unwrap_or_else(|_| "stockfish".into());

    if configured.contains(std::path::MAIN_SEPARATOR) {
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path);
    }

    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| dir.join(&configured))
        .find(|candidate| candidate.is_file())
}

fn real_config(path: &Path) -> EngineConfig {
    EngineConfig {
        path: path.display().to_string(),
        threads: 1,
        hash: 32,
        multipv: MULTIPV,
        depth: DEPTH,
    }
}

/// `/api/health` must name the engine that is actually running — the same string
/// the cache is keyed on, so the two can never drift apart.
#[tokio::test]
async fn health_reports_the_running_engine() {
    let Some(path) = stockfish_path() else {
        println!("skipping: no stockfish binary on PATH or in KIBITZ_STOCKFISH");
        return;
    };

    let (engine, health) = kibitz_server::start_engine(real_config(&path)).await;
    let engine = engine.expect("stockfish starts");

    assert!(health.ok, "{health:?}");
    assert_eq!(health.engine.as_deref(), Some(engine.name()));
    assert!(
        engine.name().starts_with("Stockfish"),
        "got {:?}",
        engine.name()
    );
    assert_eq!(health.stockfish_path, path.display().to_string());
    assert!(health.error.is_none());

    engine.shutdown().await.ok();
}

/// A binary that cannot start is reported as unhealthy, with the reason — the
/// property the separate `probe_engine` process used to provide.
#[tokio::test]
async fn health_explains_an_engine_that_cannot_start() {
    let config = EngineConfig {
        path: "/nonexistent/definitely-not-stockfish".into(),
        ..EngineConfig::default()
    };
    let (engine, health) = kibitz_server::start_engine(config).await;

    assert!(engine.is_none());
    assert!(!health.ok);
    assert_eq!(health.engine, None);
    assert_eq!(health.stockfish_path, "/nonexistent/definitely-not-stockfish");
    let error = health.error.expect("a reason");
    assert!(
        error.contains("/nonexistent/definitely-not-stockfish"),
        "the reason should name the binary: {error}"
    );
}
