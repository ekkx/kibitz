//! `POST /analyze` is SSE, and this file is the contract for what comes out of it.
//!
//! The endpoint exists to answer one complaint: a depth-12 MultiPV-5 search takes
//! a couple of hundred milliseconds and used to show *nothing at all* until it
//! finished, so the board sat empty for the whole search and then filled in at
//! once. Stockfish narrates its search the entire time; the route now forwards
//! that narration.
//!
//! The rule the tests below exist to pin down is **candidates stream, the verdict
//! does not**. A `candidates` event is a ranking, and a ranking is the same kind
//! of claim at every depth. A classification is not — it comes from comparing two
//! searches against thresholds — so it appears once, on the final `analysis`
//! event, and never before.
//!
//! Like `analyze_game_stream.rs`, this runs against a scripted stand-in rather
//! than Stockfish: the assertions are about *when* things arrive, and real
//! timings would make them flaky and the suite dependent on a chess engine.

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
use kibitz_engine::{Engine, EngineConfig, MIN_STREAM_DEPTH};
use kibitz_server::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;

const DEPTH: u8 = 10;
const MULTIPV: usize = 2;

/// How long the stand-in takes over each iteration *above* the streaming floor.
///
/// Below the floor it answers instantly, which is also what Stockfish does —
/// every depth up to about 6 lands inside the first few milliseconds of a real
/// search. The delay is therefore where a real search's delay is, and it makes
/// the arrival times unambiguous without making the suite slow.
const PER_DEPTH: Duration = Duration::from_millis(40);

/// `1. e4 e5`, so node 2 has a parent and the analysis needs **two** searches:
/// the node's own ranking, and the parent's, purely to classify the move played.
/// Only the first is streamed, which is the shape the assertions rely on.
const GAME: &str = "1. e4 e5 *";
const NODE: usize = 2;

// ─── a stand-in that narrates ───────────────────────────

/// Speaks UCI and prints a full MultiPV set for every depth from 1 to [`DEPTH`].
///
/// A shell script cannot know whose turn it is, so each set offers one move for
/// each side; exactly one is legal in any position of [`GAME`], and the pipeline
/// drops the other the way it drops any unplayable PV. So every reported depth
/// carries exactly one candidate — enough to assert on, and it keeps the script
/// readable.
fn script() -> String {
    let mut body = String::new();
    for d in 1..=DEPTH {
        if d > MIN_STREAM_DEPTH {
            body.push_str(&format!("      sleep {}\n", PER_DEPTH.as_secs_f64()));
        }
        body.push_str(&format!(
            "      printf 'info depth {d} multipv 1 score cp {} pv g1f3\\n'\n",
            20 + d as i32
        ));
        body.push_str(&format!(
            "      printf 'info depth {d} multipv 2 score cp {} pv g8f6\\n'\n",
            10 + d as i32
        ));
    }
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id name kibitz-narrator\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    go*)
{body}      printf 'bestmove g1f3\n'
      ;;
    quit) exit 0 ;;
  esac
done
"#
    )
}

/// Serialises "write the script, then exec it" — see `fake_engine.rs` for why
/// (`ETXTBSY` on Linux).
static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn narrating_engine() -> (Engine, PathBuf) {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let _guard = SPAWN_LOCK.lock().await;

    let path = std::env::temp_dir().join(format!(
        "kibitz-narrator-{}-{}",
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

/// A router with no store, so every search is real work and nothing is served
/// from cache — a cache hit answers with a single `analysis` event and would
/// make every assertion below vacuous.
async fn app() -> (Router, PathBuf) {
    let (engine, script_path) = narrating_engine().await;
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
    serde_json::from_slice::<Value>(&bytes).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn start_analyze(app: &Router, id: &str, node: usize) -> Body {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/analyze"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "node_id": node, "depth": DEPTH }).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    // The stream is open, so the status is 200 even for failures that used to be
    // a 4xx. Everything checkable before the search still answers with a status.
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body()
}

// ─── reading SSE as it arrives ──────────────────────────

#[derive(Debug)]
struct Arrival {
    name: String,
    data: Value,
    at: Duration,
}

/// Pulls one event at a time off a live body, so a test can act *between* two
/// events rather than only after the last one.
struct Events {
    body: Body,
    pending: String,
    start: Instant,
}

impl Events {
    fn new(body: Body, start: Instant) -> Events {
        Events {
            body,
            pending: String::new(),
            start,
        }
    }

    async fn next(&mut self) -> Option<Arrival> {
        loop {
            if let Some(event) = self.take() {
                return Some(event);
            }
            let frame = self.body.frame().await?.expect("body frame");
            if let Some(chunk) = frame.data_ref() {
                self.pending.push_str(&String::from_utf8_lossy(chunk));
            }
        }
    }

    fn take(&mut self) -> Option<Arrival> {
        let end = self.pending.find("\n\n")?;
        let block: String = self.pending.drain(..end + 2).collect();
        let mut name = None;
        let mut data = None;
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event: ") {
                name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data: ") {
                data = Some(rest.trim().to_string());
            }
        }
        // Keep-alive comments carry no event name; skip them and look again.
        let (name, data) = (name?, data?);
        Some(Arrival {
            name,
            data: serde_json::from_str(&data).expect("event data is json"),
            at: self.start.elapsed(),
        })
    }

    async fn drain(mut self) -> Vec<Arrival> {
        let mut all = Vec::new();
        while let Some(event) = self.next().await {
            all.push(event);
        }
        all
    }
}

async fn stored_analysis(app: &Router, id: &str, node: usize) -> Value {
    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/sessions/{id}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    value["tree"]["nodes"][node]["analysis"].clone()
}

// ─── the shape of the stream ────────────────────────────

/// The headline: the ranking arrives repeatedly while the search runs, and the
/// judgement arrives exactly once, at the end.
#[tokio::test]
async fn candidates_stream_and_the_verdict_arrives_once_at_the_end() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let events = Events::new(start_analyze(&app, &id, NODE).await, start)
        .drain()
        .await;

    let names: Vec<&str> = events.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.iter().filter(|n| **n == "candidates").count() > 1,
        "the search should have been narrated, got {names:?}"
    );
    assert_eq!(
        names.last(),
        Some(&"analysis"),
        "the finished analysis is the last thing to arrive, got {names:?}"
    );
    assert_eq!(
        names.iter().filter(|n| **n == "analysis").count(),
        1,
        "exactly one verdict"
    );

    // Every partial is a ranking and nothing but a ranking. This is the assertion
    // that fails if someone ever "helpfully" adds a provisional classification.
    for event in events.iter().filter(|e| e.name == "candidates") {
        let object = event.data.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["candidates", "depth"],
            "a partial carries a depth and a ranking, and nothing that is a judgement"
        );
        assert!(event.data["candidates"].as_array().is_some_and(|c| !c.is_empty()));
        assert!(event.data["candidates"][0]["san"].is_string());
        assert!(event.data["candidates"][0]["win_prob"].is_number());
    }

    // …and the final event is the whole `PositionAnalysis`, judgement included.
    let last = events.last().unwrap();
    assert_eq!(last.data["depth"], json!(DEPTH));
    assert!(last.data["candidates"].as_array().is_some_and(|c| !c.is_empty()));
    let played = &last.data["context"]["played"];
    assert!(played["classification"].is_string(), "{last:?}");
    assert!(played["accuracy"].is_number());

    let _ = std::fs::remove_file(&script_path);
}

/// The point of the whole exercise: the first ranking is on the wire long before
/// the verdict is.
///
/// The gap is not small. `analyze_node` runs *two* searches — this node's, and
/// its parent's, the second purely to classify the move played — so the verdict
/// cannot exist until both are done, while the first ranking exists after one
/// iteration of the first.
#[tokio::test]
async fn the_first_ranking_arrives_far_ahead_of_the_verdict() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let events = Events::new(start_analyze(&app, &id, NODE).await, start)
        .drain()
        .await;

    let first = events
        .iter()
        .find(|e| e.name == "candidates")
        .expect("a candidates event");
    let verdict = events.last().expect("an analysis event");

    assert!(
        first.at * 4 < verdict.at,
        "the first ranking arrived at {:?} of a {:?} analysis — that is not streaming",
        first.at,
        verdict.at
    );

    let _ = std::fs::remove_file(&script_path);
}

/// Depths below the floor are dropped, and the ones above it arrive once each,
/// in order. A client reads `depth` straight onto the screen, so it must never
/// go backwards or repeat.
#[tokio::test]
async fn reported_depths_start_at_the_floor_and_only_increase() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let events = Events::new(start_analyze(&app, &id, NODE).await, start)
        .drain()
        .await;

    let depths: Vec<u64> = events
        .iter()
        .filter(|e| e.name == "candidates")
        .map(|e| e.data["depth"].as_u64().expect("a depth"))
        .collect();

    assert_eq!(
        depths,
        (MIN_STREAM_DEPTH as u64..=DEPTH as u64).collect::<Vec<u64>>(),
        "every iteration from the floor to the requested depth, once each"
    );

    let _ = std::fs::remove_file(&script_path);
}

/// A partial must never be mistaken for a result — including by the server
/// itself. The session's tree stays empty at this node until the analysis is
/// finished, so a reload or a `GET /sessions/{id}` mid-search cannot hand a
/// half-done search back as a whole one.
#[tokio::test]
async fn a_partial_never_reaches_the_store() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let mut events = Events::new(start_analyze(&app, &id, NODE).await, start);

    let mut checked = 0;
    while let Some(event) = events.next().await {
        match event.name.as_str() {
            "candidates" => {
                assert_eq!(
                    stored_analysis(&app, &id, NODE).await,
                    Value::Null,
                    "a partial was written to the session tree"
                );
                checked += 1;
            }
            "analysis" => break,
            other => panic!("unexpected event {other}"),
        }
    }
    assert!(checked > 1, "the stream was never actually partial");

    // And once it is finished, it *is* stored — the streaming did not lose it.
    let stored = stored_analysis(&app, &id, NODE).await;
    assert_eq!(stored["depth"], json!(DEPTH));
    assert!(stored["context"]["played"]["classification"].is_string());

    let _ = std::fs::remove_file(&script_path);
}

/// Cancellation survives, in the only shape a stream can express it.
///
/// "Latest analyze wins" is unchanged — the newer request still stops the older
/// search — but the older request has already sent its `200` and some of its
/// `candidates` events, so it can no longer answer `409`. It ends with an `error`
/// event carrying the same `"cancelled"` message the status body used to carry,
/// and a client has to recognise both shapes as the same condition.
#[tokio::test]
async fn a_superseded_analysis_ends_with_a_cancelled_error_event() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    let start = Instant::now();
    let first = Events::new(start_analyze(&app, &id, NODE).await, start);
    let reader = tokio::spawn(first.drain());

    // Let the first search get past the floor and into the slow iterations, then
    // supersede it from the same session.
    tokio::time::sleep(PER_DEPTH * 2).await;
    let newer = start_analyze(&app, &id, 1).await;
    let newer = tokio::spawn(Events::new(newer, Instant::now()).drain());

    let events = tokio::time::timeout(Duration::from_secs(20), reader)
        .await
        .expect("the superseded stream must end, not hang")
        .expect("reader task");

    let last = events.last().expect("at least one event");
    assert_eq!(last.name, "error", "got {events:?}");
    assert_eq!(
        last.data["error"], json!("cancelled"),
        "the message a client matches on must not drift"
    );
    assert!(
        events.iter().all(|e| e.name != "analysis"),
        "a cancelled analysis must not also deliver a verdict"
    );
    // The rankings it managed to send before being stopped are real and stay
    // sent; only the judgement is missing.
    assert!(events.iter().any(|e| e.name == "candidates"));

    // Nothing was stored for a node whose analysis never completed.
    assert_eq!(stored_analysis(&app, &id, NODE).await, Value::Null);

    let _ = tokio::time::timeout(Duration::from_secs(20), newer).await;
    let _ = std::fs::remove_file(&script_path);
}

/// Failures the server can see *before* it starts searching are still ordinary
/// HTTP errors: the response has not begun, so there is nothing forcing a 200.
#[tokio::test]
async fn errors_before_the_stream_are_still_http_statuses() {
    let (app, script_path) = app().await;
    let id = new_session(&app, GAME).await;

    for (uri, node, status, message) in [
        (format!("/api/sessions/{id}/analyze"), 99, StatusCode::NOT_FOUND, "node not found"),
        ("/api/sessions/nope/analyze".to_string(), 0, StatusCode::NOT_FOUND, "session not found"),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("content-type", "application/json")
            .body(Body::from(json!({ "node_id": node }).to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), status, "{uri}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"], json!(message), "{uri}");
    }

    let _ = std::fs::remove_file(&script_path);
}
