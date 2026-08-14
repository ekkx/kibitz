//! Tests driven by a scripted stand-in for Stockfish.
//!
//! These need no chess engine at all, which lets them assert two things the real
//! binary cannot: that a terminal position is answered **without the engine being
//! consulted** (the stand-in never replies to `go`, so consulting it would hang),
//! and that a process dying mid-search turns into
//! [`EngineError::Died`](kibitz_engine::EngineError::Died) rather than a hang.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use kibitz_engine::{DepthUpdate, Engine, EngineConfig, EngineError, MIN_STREAM_DEPTH, Terminal};
use shakmaty::fen::Fen;
use shakmaty::{CastlingMode, Chess};

/// Speaks just enough UCI to finish the handshake, then goes quiet. It never
/// answers `go`, so any search sent to it never completes.
const DEAF_TO_GO: &str = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id name kibitz-fake\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    quit) exit 0 ;;
  esac
done
"#;

/// Completes the handshake, then exits the moment a search starts.
const DIES_ON_GO: &str = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id name kibitz-dying\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    go*) exit 1 ;;
    quit) exit 0 ;;
  esac
done
"#;

/// Finishes the handshake but never says who it is.
const ANONYMOUS: &str = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id author nobody\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    quit) exit 0 ;;
  esac
done
"#;

/// Not a UCI engine at all — exits before saying anything.
const EXITS_IMMEDIATELY: &str = "#!/bin/sh\nexit 0\n";

fn write_script(body: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "kibitz-fake-engine-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let path = std::env::temp_dir().join(name);

    let mut file = std::fs::File::create(&path).expect("create script");
    file.write_all(body.as_bytes()).expect("write script");
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("make executable");

    path
}

/// Serializes "write the script, then exec it".
///
/// Without this the suite is flaky on Linux with `ETXTBSY`. Cargo runs these
/// tests on parallel threads; if one thread forks while another still holds a
/// write handle to its freshly-created script, the forked child inherits that
/// descriptor and holds it until `exec` clears it — and an `exec` of a file
/// somebody has open for writing fails. Holding this lock from `File::create`
/// through the fork inside `Engine::spawn` closes the window. macOS does not
/// enforce ETXTBSY, which is why this only ever shows up in the container.
static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Write `body` to a temporary script and start an `Engine` on it.
async fn spawn_fake(body: &str) -> Result<(Engine, PathBuf), EngineError> {
    let _guard = SPAWN_LOCK.lock().await;
    let script = write_script(body);
    let engine = Engine::spawn(config(&script)).await?;
    Ok((engine, script))
}

fn config(path: &Path) -> EngineConfig {
    EngineConfig {
        path: path.display().to_string(),
        threads: 1,
        hash: 16,
        multipv: 3,
        depth: 10,
    }
}

/// Write `body` to a temporary script and start an `Engine` at a chosen MultiPV
/// width — the width decides how many `info` lines make up one iteration, which
/// is the thing the depth buffering exists to group.
async fn spawn_fake_multipv(body: &str, multipv: usize) -> (Engine, PathBuf) {
    let _guard = SPAWN_LOCK.lock().await;
    let script = write_script(body);
    let mut config = config(&script);
    config.multipv = multipv;
    let engine = Engine::spawn(config).await.expect("spawn");
    (engine, script)
}

fn chess(fen: &str) -> Chess {
    fen.parse::<Fen>()
        .expect("valid fen")
        .into_position(CastlingMode::Standard)
        .expect("legal position")
}

#[tokio::test]
async fn the_id_name_line_becomes_the_engine_name() {
    let (engine, script) = spawn_fake(DEAF_TO_GO).await.expect("spawn");
    assert_eq!(engine.name(), "kibitz-fake");

    let (other, other_script) = spawn_fake(DIES_ON_GO).await.expect("spawn");
    // Two binaries, two identities — which is what keeps their cache entries apart.
    assert_eq!(other.name(), "kibitz-dying");
    assert_ne!(engine.name(), other.name());

    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_file(&other_script);
}

/// A missing `id name` must not fail the spawn: the engine works, it is merely
/// anonymous, and every anonymous binary shares one cache namespace.
#[tokio::test]
async fn a_nameless_engine_still_starts() {
    let (engine, script) = spawn_fake(ANONYMOUS).await.expect("spawn");
    assert_eq!(engine.name(), kibitz_engine::UNKNOWN_ENGINE_NAME);

    let _ = std::fs::remove_file(&script);
}

#[tokio::test]
async fn terminal_positions_never_reach_the_engine() {
    let (engine, script) = spawn_fake(DEAF_TO_GO).await.expect("spawn");

    // Fool's mate, stalemate, bare kings, and a spent fifty-move counter. If any
    // of these were forwarded to the process, the stand-in would never reply and
    // the timeout below would fire.
    let cases = [
        (
            "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
            Terminal::Checkmate,
        ),
        ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", Terminal::Stalemate),
        ("8/8/4k3/8/8/3K4/8/8 w - - 0 1", Terminal::Draw),
        ("8/8/4k3/8/8/3K4/R7/8 w - - 100 80", Terminal::Draw),
    ];

    for (fen, expected) in cases {
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            engine.analyze(&chess(fen), Some(10)),
        )
        .await
        .unwrap_or_else(|_| panic!("{fen} was sent to the engine"))
        .expect("analysis");

        assert_eq!(result.terminal, Some(expected), "{fen}");
        assert!(result.candidates.is_empty(), "{fen}");
        assert_eq!(result.fen, fen, "{fen}");
    }

    let _ = std::fs::remove_file(&script);
}

#[tokio::test]
async fn a_dying_process_fails_the_in_flight_request() {
    let (engine, script) = spawn_fake(DIES_ON_GO).await.expect("spawn");

    let err = tokio::time::timeout(
        Duration::from_secs(5),
        engine.analyze(&Chess::default(), Some(10)),
    )
    .await
    .expect("a dead process must not hang the caller")
    .expect_err("the process died");
    assert!(matches!(err, EngineError::Died), "got {err:?}");

    // And every later request, rather than queueing up behind a corpse.
    for _ in 0..3 {
        let err = tokio::time::timeout(
            Duration::from_secs(5),
            engine.analyze(&Chess::default(), Some(10)),
        )
        .await
        .expect("must not hang")
        .expect_err("the process is gone");
        assert!(matches!(err, EngineError::Died), "got {err:?}");
    }

    // `shutdown` on an already-dead engine is not an error.
    engine.shutdown().await.expect("shutdown");

    let _ = std::fs::remove_file(&script);
}

#[tokio::test]
async fn queued_requests_fail_when_the_process_dies() {
    let (engine, script) = spawn_fake(DIES_ON_GO).await.expect("spawn");

    // Fire several at once. One reaches the process and kills it; the rest must
    // resolve too, either superseded or dead — never left hanging.
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let engine = engine.clone();
        tasks.push(tokio::spawn(async move {
            engine.analyze(&Chess::default(), Some(10)).await
        }));
    }

    for task in tasks {
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("must not hang")
            .expect("task did not panic");
        assert!(
            matches!(result, Err(EngineError::Died | EngineError::Cancelled)),
            "got {result:?}"
        );
    }

    let _ = std::fs::remove_file(&script);
}

#[tokio::test]
async fn a_wedged_search_does_not_freeze_later_requests() {
    // The stand-in answers neither `go` nor `stop`. Nothing may hang: the stuck
    // search has to give up on its own and take the process down with it.
    let (engine, script) = spawn_fake(DEAF_TO_GO).await.expect("spawn");

    let stuck = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.analyze(&Chess::default(), Some(10)).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The newer request supersedes the stuck one, which triggers `stop`.
    let second = tokio::time::timeout(
        Duration::from_secs(15),
        engine.analyze(&Chess::default(), Some(10)),
    )
    .await
    .expect("must not hang")
    .expect_err("the engine never answers");
    assert!(matches!(second, EngineError::Died), "got {second:?}");

    let stuck = tokio::time::timeout(Duration::from_secs(15), stuck)
        .await
        .expect("must not hang")
        .expect("task did not panic");
    assert!(
        matches!(stuck, Err(EngineError::Died | EngineError::Cancelled)),
        "got {stuck:?}"
    );

    let _ = std::fs::remove_file(&script);
}

// ─── streaming: completed depths ────────────────────────

/// The five moves the scripts below rank, all legal in the starting position.
const RANKED: [&str; 5] = ["e2e4", "d2d4", "g1f3", "c2c4", "b1c3"];

/// A stand-in that narrates a search: `width` MultiPV lines for every depth from
/// 1 to `depth`, then `bestmove`.
///
/// The ranking is rotated by the depth, so each iteration says something
/// different from the one before it. That is what makes the assertions able to
/// tell "the update for depth 8" apart from "the final answer, delivered eight
/// times" — which a script that printed the same order at every depth could not.
fn narrating_engine(depth: u8, width: usize) -> String {
    let mut lines = String::new();
    for d in 1..=depth {
        for rank in 0..width {
            let mv = RANKED[(rank + d as usize) % RANKED.len()];
            let cp = 100 - (rank as i32) * 10 + d as i32;
            lines.push_str(&format!(
                "  printf 'info depth {d} seldepth {d} multipv {} score cp {cp} nodes 100 nps 1000 time 1 pv {mv}\\n'\n",
                rank + 1
            ));
        }
    }
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    uci) printf 'id name kibitz-narrator\nuciok\n' ;;
    isready) printf 'readyok\n' ;;
    go*)
{lines}      printf 'bestmove e2e4\n'
      ;;
    quit) exit 0 ;;
  esac
done
"#
    )
}

/// Collect every update a streaming analysis reports, with the search's answer.
async fn stream(engine: &Engine, pos: &Chess, depth: u8) -> (Vec<DepthUpdate>, Vec<String>) {
    let mut updates = Vec::new();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        engine.analyze_streaming(pos, Some(depth), |update| updates.push(update)),
    )
    .await
    .expect("must not hang")
    .expect("analysis");
    let final_ranking = result.candidates.iter().map(|c| c.uci.clone()).collect();
    (updates, final_ranking)
}

/// The rotation `narrating_engine` applies, read back so the assertions can say
/// what depth 8's ranking *should* be rather than repeating the arithmetic.
fn ranking_at(depth: u8, width: usize) -> Vec<String> {
    (0..width)
        .map(|rank| RANKED[(rank + depth as usize) % RANKED.len()].to_string())
        .collect()
}

/// The headline case: five `info` lines per iteration become **one** update per
/// iteration, not five.
#[tokio::test]
async fn a_full_multipv_set_is_reported_once_per_depth() {
    let (engine, script) = spawn_fake_multipv(&narrating_engine(12, 5), 5).await;
    let (updates, final_ranking) = stream(&engine, &Chess::default(), 12).await;

    let depths: Vec<u8> = updates.iter().map(|u| u.depth).collect();
    assert_eq!(
        depths,
        (MIN_STREAM_DEPTH..=12).collect::<Vec<u8>>(),
        "one update per completed depth from the floor up, and no repeats"
    );

    for update in &updates {
        let ranking: Vec<String> = update.candidates.iter().map(|c| c.uci.clone()).collect();
        assert_eq!(update.candidates.len(), 5, "depth {}", update.depth);
        assert_eq!(
            ranking,
            ranking_at(update.depth, 5),
            "depth {} must carry its own ranking, not the final one",
            update.depth
        );
    }

    // The last update is the deepest iteration, and it agrees with the answer —
    // which is what lets a client draw full-depth arrows before whatever else the
    // caller still has to compute.
    assert_eq!(updates.last().expect("an update").depth, 12);
    assert_eq!(final_ranking, ranking_at(12, 5));

    let _ = std::fs::remove_file(&script);
    let _ = engine.shutdown().await;
}

/// MultiPV 1: one line per iteration, so the buffering has nothing to group —
/// and must still report exactly once per depth rather than falling back to
/// per-line.
#[tokio::test]
async fn multipv_1_reports_one_update_per_depth_too() {
    let (engine, script) = spawn_fake_multipv(&narrating_engine(12, 1), 1).await;
    let (updates, _) = stream(&engine, &Chess::default(), 12).await;

    assert_eq!(
        updates.iter().map(|u| u.depth).collect::<Vec<u8>>(),
        (MIN_STREAM_DEPTH..=12).collect::<Vec<u8>>()
    );
    assert!(updates.iter().all(|u| u.candidates.len() == 1));

    let _ = std::fs::remove_file(&script);
    let _ = engine.shutdown().await;
}

/// Everything below the floor is dropped. At depth 5 the search never reaches an
/// iteration worth drawing, so the caller hears nothing at all until the answer.
#[tokio::test]
async fn a_search_that_ends_before_the_floor_reports_nothing() {
    let shallow = MIN_STREAM_DEPTH - 1;
    let (engine, script) = spawn_fake_multipv(&narrating_engine(shallow, 5), 5).await;

    let (updates, final_ranking) = stream(&engine, &Chess::default(), shallow).await;
    assert!(
        updates.is_empty(),
        "depths below {MIN_STREAM_DEPTH} are noise; got {:?}",
        updates.iter().map(|u| u.depth).collect::<Vec<u8>>()
    );
    // The answer still arrives in full — suppressing the commentary is not
    // suppressing the result.
    assert_eq!(final_ranking, ranking_at(shallow, 5));

    let _ = std::fs::remove_file(&script);
    let _ = engine.shutdown().await;
}

/// The floor is a floor, not an offset: a search that runs past it reports every
/// depth from the floor up and none below it.
#[tokio::test]
async fn the_floor_suppresses_only_the_depths_beneath_it() {
    let (engine, script) = spawn_fake_multipv(&narrating_engine(MIN_STREAM_DEPTH, 5), 5).await;
    let (updates, _) = stream(&engine, &Chess::default(), MIN_STREAM_DEPTH).await;

    assert_eq!(
        updates.iter().map(|u| u.depth).collect::<Vec<u8>>(),
        vec![MIN_STREAM_DEPTH],
        "the floor depth itself is reported"
    );

    let _ = std::fs::remove_file(&script);
    let _ = engine.shutdown().await;
}

/// A terminal position is decided by `shakmaty` without the engine, so there is
/// no search to narrate. The stand-in never answers `go`, so a single update here
/// would mean the position had been sent to it — and the test would hang instead.
#[tokio::test]
async fn a_terminal_position_reports_no_depths() {
    let (engine, script) = spawn_fake(DEAF_TO_GO).await.expect("spawn");

    for (fen, expected) in [
        (
            "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
            Terminal::Checkmate,
        ),
        ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", Terminal::Stalemate),
        ("8/8/4k3/8/8/3K4/8/8 w - - 0 1", Terminal::Draw),
    ] {
        let mut updates: Vec<DepthUpdate> = Vec::new();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            engine.analyze_streaming(&chess(fen), Some(10), |u| updates.push(u)),
        )
        .await
        .unwrap_or_else(|_| panic!("{fen} was sent to the engine"))
        .expect("analysis");

        assert!(updates.is_empty(), "{fen} narrated a search that never ran");
        assert_eq!(result.terminal, Some(expected), "{fen}");
        assert!(result.candidates.is_empty(), "{fen}");
    }

    let _ = std::fs::remove_file(&script);
}

/// A superseded search must not report the iteration it was cut off in the
/// middle of: what is buffered there is however many ranks happened to have been
/// printed, which was never a ranking of anything.
#[tokio::test]
async fn a_cancelled_search_does_not_report_a_torn_iteration() {
    let (engine, script) = spawn_fake(DEAF_TO_GO).await.expect("spawn");

    let pos = Chess::default();
    let seen: std::sync::Arc<std::sync::Mutex<Vec<u8>>> = Default::default();

    let outcome = {
        let recorder = std::sync::Arc::clone(&seen);
        let stuck = engine.analyze_streaming(&pos, Some(10), move |update| {
            recorder.lock().expect("not poisoned").push(update.depth)
        });
        tokio::pin!(stuck);

        // Let the search reach the engine — the stand-in will never answer it —
        // and then supersede it with a newer one.
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut stuck)
                .await
                .is_err(),
            "the stand-in never answers `go`"
        );
        let second = engine.clone();
        let newer = tokio::spawn(async move { second.analyze(&Chess::default(), Some(10)).await });

        let outcome = tokio::time::timeout(Duration::from_secs(15), stuck)
            .await
            .expect("must not hang");
        let _ = tokio::time::timeout(Duration::from_secs(15), newer).await;
        outcome
    };

    assert!(
        matches!(outcome, Err(EngineError::Cancelled | EngineError::Died)),
        "got {outcome:?}"
    );
    assert!(
        seen.lock().expect("not poisoned").is_empty(),
        "a search with no finished iteration reported one"
    );

    let _ = std::fs::remove_file(&script);
}

#[tokio::test]
async fn a_binary_that_is_not_an_engine_fails_the_handshake() {
    let _guard = SPAWN_LOCK.lock().await;
    let script = write_script(EXITS_IMMEDIATELY);
    let err = tokio::time::timeout(Duration::from_secs(5), Engine::spawn(config(&script)))
        .await
        .expect("must not hang")
        .expect_err("not a UCI engine");
    assert!(matches!(err, EngineError::Protocol(_)), "got {err:?}");

    let _ = std::fs::remove_file(&script);
}
