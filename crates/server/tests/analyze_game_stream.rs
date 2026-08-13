//! `POST /analyze-game` is SSE because the client annotates the move list as the
//! sweep walks the game. Two defects made that impossible, and both are pinned
//! down here:
//!
//! 1. **Nothing streamed.** `Pipeline::analyze_path` ran every search first and
//!    only then assembled the results, firing the progress callback from the
//!    assembly loop — so all thirty-four events of a five-hundred-second run were
//!    produced in the same instant. The response body was never the problem, which
//!    is why `curl -sN`, an unbuffered reader and a browser all agreed.
//! 2. **A sweep was killable.** The engine driver is "latest only", so a single
//!    `POST /analyze` — which the frontend issues whenever the user selects a
//!    move — cancelled the sweep and ended it with
//!    `event: error {"error":"engine: search was cancelled by a newer request"}`.
//!
//! Both need an engine that is *slow on purpose*, so these run against a scripted
//! stand-in rather than Stockfish: real timings would make the assertions flaky
//! and the suite would need a chess engine installed.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use kibitz_engine::{Engine, EngineConfig};
use kibitz_server::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;

const DEPTH: u8 = 10;
const MULTIPV: usize = 3;

/// How long the stand-in takes to answer one `go`. Long enough that the arrival
/// times of the SSE events are unambiguous, short enough to keep the suite quick.
const SEARCH_TIME: Duration = Duration::from_millis(150);

/// Scholar's mate: seven plies, so eight positions.
///
/// It ends in checkmate on purpose, exactly like `testdata/opera_game.pgn`. The
/// final position of a line is the one the pipeline evaluates rather than
/// searches, and `shakmaty` settles a terminal position without consulting the
/// engine at all — so the sweep is seven searches, and the stand-in never has to
/// produce a principal variation for a position with no legal moves.
const GAME: &str = "1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7# 1-0";
const POSITIONS: usize = 8;
/// Positions that actually reach the engine: every one but the mate.
const SEARCHES: usize = POSITIONS - 1;

// ─── a deliberately slow engine ─────────────────────────

/// Speaks UCI, and takes [`SEARCH_TIME`] over every search.
///
/// A shell script cannot know whose turn it is, so it offers one knight move for
/// each side. Exactly one of the two is legal in any position of [`GAME`] — the
/// pipeline drops the other, the way it drops any unplayable PV — which leaves
/// every position with a real candidate instead of an empty list. Nothing here
/// asserts on evaluations; the point is that the sweep does the same work it
/// would with Stockfish, only predictably slowly.
fn script() -> String {
    let seconds = SEARCH_TIME.as_secs_f64();
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id name kibitz-slow\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    go*)
      sleep {seconds}
      printf 'info depth 10 multipv 1 score cp 30 pv g1h3\n'
      printf 'info depth 10 multipv 2 score cp 20 pv g8h6\n'
      printf 'bestmove g1h3\n'
      ;;
    quit) exit 0 ;;
  esac
done
"#
    )
}

/// Serialises "write the script, then exec it" — see the same guard in
/// `kibitz-engine`'s `fake_engine.rs` for why (`ETXTBSY` on Linux).
static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn slow_engine() -> (Engine, PathBuf) {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let _guard = SPAWN_LOCK.lock().await;

    let path = std::env::temp_dir().join(format!(
        "kibitz-slow-engine-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::File::create(&path).expect("create script");
    file.write_all(script().as_bytes()).expect("write script");
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
    .expect("spawn the stand-in engine");

    (engine, path)
}

/// A router wired to the slow engine, with no store — every search is real work,
/// nothing is served from cache.
async fn app() -> (Router, PathBuf) {
    let (engine, script_path) = slow_engine().await;
    let state = AppState {
        engine: Some(engine),
        depth: DEPTH,
        multipv: MULTIPV,
        ..AppState::bare()
    };
    (kibitz_server::router(state), script_path)
}

async fn new_session(app: &Router, pgn: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/api/sessions")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "pgn": pgn }).to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    value["session_id"].as_str().unwrap().to_string()
}

// ─── reading SSE as it arrives ──────────────────────────

/// One event, and how long after the request it turned up.
#[derive(Debug)]
struct Arrival {
    name: String,
    data: String,
    at: Duration,
}

/// Consume an SSE body frame by frame, timestamping each event.
///
/// Collecting the body first — as the other tests do — would destroy exactly the
/// property under test, so this reads incrementally and never buffers.
async fn read_events(body: Body, start: Instant) -> Vec<Arrival> {
    let mut body = body;
    let mut pending = String::new();
    let mut events = Vec::new();

    while let Some(frame) = body.frame().await {
        let frame = frame.expect("body frame");
        let Some(chunk) = frame.data_ref() else {
            continue;
        };
        let at = start.elapsed();
        pending.push_str(&String::from_utf8_lossy(chunk));

        // Events are separated by a blank line. Keep any partial tail for the
        // next frame.
        while let Some(end) = pending.find("\n\n") {
            let block: String = pending.drain(..end + 2).collect();
            let mut name = None;
            let mut data = None;
            for line in block.lines() {
                if let Some(rest) = line.strip_prefix("event: ") {
                    name = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data: ") {
                    data = Some(rest.trim().to_string());
                }
            }
            if let (Some(name), Some(data)) = (name, data) {
                events.push(Arrival { name, data, at });
            }
        }
    }

    events
}

async fn start_sweep(app: &Router, id: &str) -> Body {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/analyze-game"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "depth": DEPTH }).to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body()
}

// ─── defect 1: it must actually stream ──────────────────

/// The regression test for the buffering: the first `progress` has to be
/// observable long before the run finishes.
///
/// Under the old `analyze_path` the whole sweep completed before the first
/// callback fired, so `first.at` and `last.at` were the same instant and this
/// fails by a factor of nine.
#[tokio::test]
async fn progress_arrives_while_the_sweep_is_still_running() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let events = read_events(start_sweep(&app, &id).await, start).await;

    let first = events
        .iter()
        .find(|e| e.name == "progress")
        .expect("a progress event");
    let total = events.last().expect("at least one event").at;

    // Seven searches at 150 ms: the first node is done after roughly one of them,
    // so it lands in the first third of the run with a lot of room to spare.
    assert!(
        first.at < total / 3,
        "the first progress event must arrive while the sweep is still running, \
         but it arrived at {:?} of a {:?} run — every event was buffered to the end",
        first.at,
        total
    );

    // And the events really are spread across the run rather than emitted in one
    // burst: consecutive positions are separated by roughly a search each.
    let progress: Vec<&Arrival> = events.iter().filter(|e| e.name == "progress").collect();
    assert_eq!(progress.len(), POSITIONS);
    let spread = progress[progress.len() - 1].at - progress[0].at;
    assert!(
        spread > SEARCH_TIME,
        "progress events were emitted in a single burst spanning only {spread:?}"
    );

    let _ = std::fs::remove_file(&script_path);
}

/// The contract in `docs/API.md`: one `progress` and one `node` per position,
/// then a single `done`. The `node` payload carries its own `node_id`.
#[tokio::test]
async fn every_position_produces_a_progress_and_a_node_event() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let events = read_events(start_sweep(&app, &id).await, start).await;

    let names: Vec<&str> = events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names.iter().filter(|n| **n == "progress").count(),
        POSITIONS
    );
    assert_eq!(names.iter().filter(|n| **n == "node").count(), POSITIONS);
    assert_eq!(names.last(), Some(&"done"));
    assert!(!names.contains(&"error"), "{names:?}");

    // Each position reports its own ordinal, in order, and the paired `node`
    // event names the same node.
    let mut expected_done = 0;
    let mut pairs = events.chunks(2);
    while let Some([progress, node]) = pairs.next() {
        if progress.name != "progress" {
            break;
        }
        expected_done += 1;
        let p: Value = serde_json::from_str(&progress.data).unwrap();
        let n: Value = serde_json::from_str(&node.data).unwrap();
        assert_eq!(p["done"], json!(expected_done));
        assert_eq!(p["total"], json!(POSITIONS));
        assert_eq!(n["node_id"], p["node_id"]);
        assert!(n["fen"].is_string());
    }
    assert_eq!(expected_done, POSITIONS);

    let _ = std::fs::remove_file(&script_path);
}

// ─── defect 2: a sweep must survive interactive use ─────

/// Selecting a move mid-sweep used to abort the whole run. It must now cost the
/// sweep nothing but the wait for one position.
#[tokio::test]
async fn an_interactive_analyze_does_not_kill_a_running_sweep() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let body = start_sweep(&app, &id).await;

    // Fire single-node analyses while the sweep is in flight — this is what the
    // frontend does on every move the user selects.
    let interactive = tokio::spawn({
        let app = app.clone();
        let id = id.clone();
        async move {
            let mut statuses = Vec::new();
            for node_id in [1, 2, 3] {
                tokio::time::sleep(SEARCH_TIME).await;
                let request = Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{id}/analyze"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "node_id": node_id, "depth": DEPTH }).to_string(),
                    ))
                    .unwrap();
                let response = app.clone().oneshot(request).await.unwrap();
                statuses.push(response.status());
            }
            statuses
        }
    });

    let events = read_events(body, start).await;
    let statuses = interactive.await.expect("interactive task");

    let errors: Vec<&str> = events
        .iter()
        .filter(|e| e.name == "error")
        .map(|e| e.data.as_str())
        .collect();
    assert!(
        errors.is_empty(),
        "the sweep was killed by interactive analysis: {errors:?}"
    );
    assert_eq!(
        events.last().map(|e| e.name.as_str()),
        Some("done"),
        "the sweep must run to completion"
    );
    assert_eq!(
        events.iter().filter(|e| e.name == "node").count(),
        POSITIONS,
        "every position must still be reported"
    );

    // And the interactive requests were served rather than rejected: they wait
    // for the sweep's current position, they do not fight it.
    assert!(
        statuses.iter().all(|s| *s == StatusCode::OK),
        "interactive analysis should succeed alongside a sweep, got {statuses:?}"
    );

    let _ = std::fs::remove_file(&script_path);
}
