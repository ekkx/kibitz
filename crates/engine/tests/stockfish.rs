//! Integration tests that drive a real Stockfish binary.
//!
//! Every test here skips cleanly when the binary is missing, because CI is not
//! guaranteed to have it. Depths are deliberately shallow so the suite stays fast.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kibitz_core::eval::Score;
use kibitz_engine::{Engine, EngineConfig, EngineError, Terminal};
use shakmaty::fen::Fen;
use shakmaty::{CastlingMode, Chess};

/// Where the tests look for Stockfish, or `None` when it is not installed.
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

/// Skip marker. Returns `None` and prints a note when Stockfish is absent.
macro_rules! stockfish_or_skip {
    () => {
        match stockfish_path() {
            Some(path) => path,
            None => {
                println!("skipping: no stockfish binary on PATH or in KIBITZ_STOCKFISH");
                return;
            }
        }
    };
}

fn config(path: &Path) -> EngineConfig {
    EngineConfig {
        path: path.display().to_string(),
        // Single-threaded keeps the results reproducible and the machine quiet.
        threads: 1,
        hash: 32,
        multipv: 3,
        depth: 10,
    }
}

fn chess(fen: &str) -> Chess {
    fen.parse::<Fen>()
        .expect("valid fen")
        .into_position(CastlingMode::Standard)
        .expect("legal position")
}

/// Ask the binary who it is, without going through `Engine` at all, so the test
/// has an independent answer to compare `Engine::name` against.
async fn id_name_of(path: &Path) -> String {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut child = tokio::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn stockfish");

    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
    stdin.write_all(b"uci\n").await.expect("write uci");
    stdin.flush().await.expect("flush");

    let mut name = None;
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(rest) = line.strip_prefix("id name ") {
            name = Some(rest.trim().to_string());
        }
        if line.trim() == "uciok" {
            break;
        }
    }
    let _ = stdin.write_all(b"quit\n").await;
    let _ = child.kill().await;
    name.expect("stockfish announces an id name")
}

/// The cache is keyed on this string, so it has to be the engine's own words —
/// not a guess, and not a constant baked into kibitz.
#[tokio::test]
async fn the_engine_reports_the_name_stockfish_announces() {
    let path = stockfish_or_skip!();
    let expected = id_name_of(&path).await;

    let engine = Engine::spawn(config(&path)).await.expect("spawn");
    assert_eq!(engine.name(), expected);
    assert!(
        engine.name().starts_with("Stockfish"),
        "expected a Stockfish id name, got {:?}",
        engine.name()
    );
    // The fallback is for binaries that send no `id name`; Stockfish always does.
    assert_ne!(engine.name(), kibitz_engine::UNKNOWN_ENGINE_NAME);

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn startpos_returns_three_candidates() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    let result = engine
        .analyze(&Chess::default(), Some(12))
        .await
        .expect("analysis");

    assert_eq!(result.terminal, None);
    assert_eq!(result.candidates.len(), 3, "MultiPV=3 should give 3 lines");
    assert_eq!(
        result.fen,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    );
    assert_eq!(result.depth, 12);

    for candidate in &result.candidates {
        assert!(!candidate.san.is_empty());
        assert_eq!(candidate.pv.first(), Some(&candidate.san));
        assert!(matches!(candidate.score, Score::Cp(_)));
        assert!((0.0..=1.0).contains(&candidate.win_prob));
    }

    // Candidates come back best-first.
    let scores: Vec<i32> = result
        .candidates
        .iter()
        .map(|candidate| match candidate.score {
            Score::Cp(cp) => cp,
            Score::Mate(_) => unreachable!("no mate from the start position"),
        })
        .collect();
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "MultiPV order should be descending: {scores:?}"
    );

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn mate_in_one_is_reported_as_mate() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    // White has Ra1-a8#.
    let pos = chess("6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1");
    let result = engine.analyze(&pos, Some(12)).await.expect("analysis");

    let best = result.candidates.first().expect("a best move");
    assert_eq!(best.score, Score::Mate(1));
    assert_eq!(best.uci, "a1a8");
    assert_eq!(best.san, "Ra8#");

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn analyze_move_searches_only_the_given_move() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    // 1. a3 is playable but not the engine's choice.
    let candidate = engine
        .analyze_move(&Chess::default(), "a2a3", Some(10))
        .await
        .expect("analysis");

    assert_eq!(candidate.uci, "a2a3");
    assert_eq!(candidate.san, "a3");
    assert_eq!(candidate.pv.first().map(String::as_str), Some("a3"));

    // An illegal move is rejected by shakmaty, without troubling the engine.
    let err = engine
        .analyze_move(&Chess::default(), "e2e5", Some(10))
        .await
        .expect_err("illegal move");
    assert!(matches!(err, EngineError::Protocol(_)));

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn long_pv_restores_multipv_for_the_next_analysis() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    let long = engine
        .long_pv(&Chess::default(), Some(14))
        .await
        .expect("long pv");
    assert!(
        long.pv.len() >= 6,
        "a deep MultiPV=1 search should give a long line, got {:?}",
        long.pv
    );

    // MultiPV was narrowed to 1 for the call above; the next analysis must still
    // see all three lines.
    let result = engine
        .analyze(&Chess::default(), Some(10))
        .await
        .expect("analysis");
    assert_eq!(result.candidates.len(), 3);

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn checkmate_is_terminal_and_the_engine_stays_usable() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    // Fool's mate. Stockfish would answer `bestmove (none)` here, so the driver
    // must not ask. See `fake_engine.rs` for the test that proves it does not.
    let pos = chess("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    let result = engine.analyze(&pos, Some(12)).await.expect("analysis");

    assert_eq!(result.terminal, Some(Terminal::Checkmate));
    assert!(result.candidates.is_empty());
    assert_eq!(result.terminal.unwrap().win_prob(), 0.0);

    // The process was never disturbed, so ordinary analysis still works.
    let after = engine
        .analyze(&Chess::default(), Some(10))
        .await
        .expect("analysis");
    assert_eq!(after.candidates.len(), 3);

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn stalemate_is_terminal() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    let pos = chess("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    let result = engine.analyze(&pos, Some(12)).await.expect("analysis");

    assert_eq!(result.terminal, Some(Terminal::Stalemate));
    assert!(result.candidates.is_empty());

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_second_analysis_cancels_the_first() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    // A tangled middlegame at a depth that takes a good while.
    let slow = chess("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1");

    let first = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.analyze(&slow, Some(40)).await })
    };

    // Give the first search time to actually start.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let second = engine
        .analyze(&Chess::default(), Some(10))
        .await
        .expect("the newer analysis wins");
    assert_eq!(second.candidates.len(), 3);

    let first = tokio::time::timeout(Duration::from_secs(10), first)
        .await
        .expect("the cancelled call must not hang")
        .expect("task did not panic");

    assert!(
        matches!(first, Err(EngineError::Cancelled)),
        "expected Cancelled, got {first:?}"
    );

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn requests_after_shutdown_report_death_instead_of_hanging() {
    let path = stockfish_or_skip!();
    let engine = Engine::spawn(config(&path)).await.expect("spawn");

    engine.shutdown().await.expect("shutdown");

    let err = tokio::time::timeout(
        Duration::from_secs(5),
        engine.analyze(&Chess::default(), Some(10)),
    )
    .await
    .expect("must not hang")
    .expect_err("the process is gone");

    assert!(matches!(err, EngineError::Died), "got {err:?}");
}

#[tokio::test]
async fn spawning_a_missing_binary_fails_fast() {
    let config = EngineConfig {
        path: "/nonexistent/definitely-not-stockfish".into(),
        ..EngineConfig::default()
    };
    let err = Engine::spawn(config).await.expect_err("should fail");
    assert!(matches!(err, EngineError::Spawn(_, _)), "got {err:?}");
}
