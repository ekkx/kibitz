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

use kibitz_engine::{Engine, EngineConfig, EngineError, Terminal};
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
