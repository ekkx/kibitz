//! The single task that owns the Stockfish process.
//!
//! Nothing else ever touches stdin or stdout. That is what makes the "latest
//! only" cancellation policy and the MultiPV switch race-free: both are just
//! ordinary sequential steps inside one task.
//!
//! Failure modes all collapse to the same shape. When the task returns, the
//! request receiver and every unanswered reply channel are dropped, so callers
//! waiting on them wake up with [`EngineError::Died`] instead of hanging, and
//! later requests fail their `send` for the same reason. An interactive tool must
//! never wedge.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};

use crate::progress::{CompletedDepth, DepthTracker};
use crate::uci::{self, InfoLine};
use crate::{EngineConfig, EngineError};

/// The handshake talks to a process that may not be a UCI engine at all, so it
/// gets a deadline. Everything after it is driven by the process's own output.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// How long `quit` gets to bring the process down before it is killed.
const QUIT_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a stopped search gets to produce its `bestmove`.
///
/// Stockfish answers `stop` in milliseconds. Waiting forever would mean one wedged
/// process freezes every later request, which for an interactive tool is worse
/// than declaring the engine dead and restarting it.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct Request {
    pub kind: RequestKind,
    pub reply: oneshot::Sender<Result<SearchOutcome, EngineError>>,
}

pub(crate) enum RequestKind {
    Search(SearchSpec),
    Shutdown,
}

pub(crate) struct SearchSpec {
    pub fen: String,
    pub depth: u8,
    pub multipv: usize,
    /// Space-separated UCI moves for `go ... searchmoves`.
    pub searchmoves: Option<String>,
    /// Where finished iterations are reported while the search is still running.
    ///
    /// `None` for every caller that only wants the answer, which is all of them
    /// except [`crate::Engine::analyze_streaming`]. Raw [`InfoLine`]s rather than
    /// candidates: turning a PV into SAN needs a board, and the board is the
    /// caller's, not this task's.
    pub progress: Option<mpsc::UnboundedSender<CompletedDepth>>,
}

/// Raw engine output for one search. Turning PVs into SAN needs a board, which
/// is the caller's business, not the task's.
#[derive(Debug, Default)]
pub(crate) struct SearchOutcome {
    /// Deepest completed iteration actually observed.
    pub depth: u8,
    /// One entry per MultiPV rank, ascending.
    pub infos: Vec<InfoLine>,
}

/// Stand-in for the `id name` line when a binary finishes the handshake without
/// ever sending one.
///
/// Failing the spawn over a missing name would be worse than useless — the engine
/// works, it is just anonymous. The consequence is confined to the cache key: every
/// nameless binary shares this one namespace, so two *different* nameless engines
/// would still be cached against each other. That is the pre-existing bug, now
/// fenced off to binaries that break the UCI spec, and never to Stockfish, which
/// always identifies itself.
pub const UNKNOWN_ENGINE_NAME: &str = "unknown-engine";

/// Spawn the process, complete the handshake, and hand back the engine's `id name`
/// along with the request channel.
pub(crate) async fn start(
    config: &EngineConfig,
) -> Result<(String, mpsc::UnboundedSender<Request>), EngineError> {
    let mut child = Command::new(&config.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // If the driver task is ever dropped, the process goes with it.
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| EngineError::Spawn(config.path.clone(), err))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| EngineError::Protocol("stdin was not piped".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EngineError::Protocol("stdout was not piped".into()))?;
    let mut lines = BufReader::new(stdout).lines();

    let handshake = handshake(&mut stdin, &mut lines, config);
    let name = match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await {
        Ok(result) => result?,
        Err(_) => {
            return Err(EngineError::Protocol(format!(
                "{} did not complete the UCI handshake within {}s",
                config.path,
                HANDSHAKE_TIMEOUT.as_secs()
            )));
        }
    };

    // A dedicated reader keeps stdout draining, which means `select!` in the
    // driver only ever waits on channels — both of which are cancellation safe.
    let (line_tx, line_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            if line_tx.send(line).is_err() {
                break;
            }
        }
        // Dropping `line_tx` here is the death signal for the driver.
    });

    let (request_tx, request_rx) = mpsc::unbounded_channel();
    tokio::spawn(
        Driver {
            child,
            stdin,
            lines: line_rx,
            requests: request_rx,
            requests_closed: false,
            multipv: config.multipv.max(1),
        }
        .run(),
    );

    Ok((name, request_tx))
}

/// Run the UCI handshake and return the engine's `id name`.
async fn handshake(
    stdin: &mut ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    config: &EngineConfig,
) -> Result<String, EngineError> {
    write_line(stdin, "uci").await?;
    // `id name` arrives between `uci` and `uciok`, so this is the only chance to
    // read it: from here on the reader task owns stdout.
    let name = wait_for(lines, "uciok").await?;

    let options = [
        ("Threads", config.threads.max(1)),
        ("Hash", config.hash.max(1)),
        ("MultiPV", config.multipv.max(1)),
    ];
    for (option, value) in options {
        write_line(stdin, &format!("setoption name {option} value {value}")).await?;
    }

    write_line(stdin, "isready").await?;
    wait_for(lines, "readyok").await?;

    Ok(name.unwrap_or_else(|| UNKNOWN_ENGINE_NAME.to_string()))
}

/// Read lines until `token` arrives, picking up the `id name` line if it goes past
/// on the way. `None` means the engine never announced a name.
async fn wait_for(
    lines: &mut Lines<BufReader<ChildStdout>>,
    token: &str,
) -> Result<Option<String>, EngineError> {
    let mut name = None;
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if line.trim() == token => return Ok(name),
            Ok(Some(line)) => {
                if let Some(rest) = line.trim().strip_prefix("id name ") {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        name = Some(rest.to_string());
                    }
                }
            }
            Ok(None) => {
                return Err(EngineError::Protocol(format!(
                    "process exited before answering `{token}`"
                )));
            }
            Err(err) => return Err(EngineError::Io(err)),
        }
    }
}

/// Sleep until `deadline`, or forever when there is none. Lets `select!` carry an
/// optional timeout without a second loop.
async fn wait_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Park `incoming` as the request to run next, cancelling whatever it displaced.
///
/// A parked shutdown is never displaced: it is the one request that has to
/// survive, or the process would outlive the caller that asked to end it.
fn park(next: &mut Option<Request>, incoming: Request) {
    match next.take() {
        Some(shutdown) if matches!(shutdown.kind, RequestKind::Shutdown) => {
            let _ = incoming.reply.send(Err(EngineError::Cancelled));
            *next = Some(shutdown);
        }
        Some(stale) => {
            let _ = stale.reply.send(Err(EngineError::Cancelled));
            *next = Some(incoming);
        }
        None => *next = Some(incoming),
    }
}

async fn write_line(stdin: &mut ChildStdin, command: &str) -> Result<(), EngineError> {
    stdin.write_all(command.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

struct Driver {
    child: Child,
    stdin: ChildStdin,
    /// Lines from the reader task. Closing means the process is gone.
    lines: mpsc::UnboundedReceiver<String>,
    requests: mpsc::UnboundedReceiver<Request>,
    /// Set once every `Engine` handle has been dropped.
    requests_closed: bool,
    /// The MultiPV value currently set on the process.
    multipv: usize,
}

impl Driver {
    async fn run(mut self) {
        // The request superseding whatever is running, waiting its turn.
        let mut next: Option<Request> = None;

        loop {
            let request = match next.take() {
                Some(request) => request,
                None => {
                    if self.requests_closed {
                        break;
                    }
                    match self.requests.recv().await {
                        Some(request) => request,
                        None => break,
                    }
                }
            };

            // Anything already queued makes this request stale before it even
            // starts, so resolve it now instead of paying for a `go`/`stop`.
            let request = self.take_latest(request);

            match request.kind {
                RequestKind::Shutdown => {
                    self.quit().await;
                    let _ = request.reply.send(Ok(SearchOutcome::default()));
                    // Returning drops the receiver, so anything still queued —
                    // including `next` — resolves to `Died`.
                    return;
                }
                RequestKind::Search(ref spec) => {
                    let outcome = self.search(spec, &mut next).await;
                    let died = matches!(outcome, Err(EngineError::Died));
                    let _ = request.reply.send(outcome);
                    if died {
                        return;
                    }
                }
            }
        }

        self.quit().await;
    }

    /// Skip past requests that a later one has already superseded, answering each
    /// with [`EngineError::Cancelled`]. Only the newest is worth running.
    fn take_latest(&mut self, mut request: Request) -> Request {
        // Shutdown is the one request that is never superseded — dropping it
        // would leave a process running that the caller asked to be rid of.
        if matches!(request.kind, RequestKind::Shutdown) {
            return request;
        }
        loop {
            match self.requests.try_recv() {
                Ok(newer) => {
                    let _ = request.reply.send(Err(EngineError::Cancelled));
                    request = newer;
                }
                Err(mpsc::error::TryRecvError::Empty) => return request,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.requests_closed = true;
                    return request;
                }
            }
        }
    }

    /// Run one search to `bestmove`.
    ///
    /// While it runs, a newly arrived request parks itself in `next`, the running
    /// search is told to `stop`, and this call resolves to
    /// [`EngineError::Cancelled`]. The `bestmove` still has to be read off stdout
    /// before anything else is written, or the next `setoption` would land in the
    /// middle of a search.
    async fn search(
        &mut self,
        spec: &SearchSpec,
        next: &mut Option<Request>,
    ) -> Result<SearchOutcome, EngineError> {
        self.set_multipv(spec.multipv).await?;
        self.send(&format!("position fen {}", spec.fen)).await?;

        let mut go = format!("go depth {}", spec.depth);
        if let Some(searchmoves) = &spec.searchmoves {
            go.push_str(" searchmoves ");
            go.push_str(searchmoves);
        }
        self.send(&go).await?;

        // Best info line seen per MultiPV rank. BTreeMap so ranks come out in
        // order, with rank 1 first.
        let mut best: BTreeMap<usize, InfoLine> = BTreeMap::new();
        // The same lines again, grouped by iteration, for a caller that wants to
        // watch the search rather than only its answer. Kept separate from `best`
        // on purpose: `best` accumulates the deepest line per rank across the
        // whole search, which is the *result*, while this is a running commentary
        // that has to forget each depth as the next one opens.
        let mut tracker = DepthTracker::new();
        let mut superseded = false;
        // Deadline for the `bestmove` that must follow a `stop`.
        let mut stop_deadline: Option<tokio::time::Instant> = None;

        loop {
            tokio::select! {
                () = wait_until(stop_deadline) => {
                    tracing::warn!("engine did not answer `stop`; treating it as dead");
                    return Err(EngineError::Died);
                }
                line = self.lines.recv() => {
                    let Some(line) = line else {
                        return Err(EngineError::Died);
                    };
                    if uci::is_bestmove(&line) {
                        break;
                    }
                    if let Some(info) = uci::parse_info(&line) {
                        if info.multipv > spec.multipv {
                            continue;
                        }
                        if let Some(progress) = &spec.progress
                            && let Some(closed) = tracker.push(info.clone())
                        {
                            // A closed receiver means the caller gave up on the
                            // commentary; the search itself carries on.
                            let _ = progress.send(closed);
                        }
                        match best.get(&info.multipv) {
                            Some(current) if current.depth > info.depth => {}
                            _ => {
                                best.insert(info.multipv, info);
                            }
                        }
                    }
                }
                request = self.requests.recv(), if !self.requests_closed => {
                    match request {
                        None => self.requests_closed = true,
                        Some(request) => {
                            superseded = true;
                            // Latest only: whatever was queued behind us is now
                            // stale too.
                            park(next, request);
                            if stop_deadline.is_none() {
                                stop_deadline =
                                    Some(tokio::time::Instant::now() + STOP_TIMEOUT);
                                self.send("stop").await?;
                            }
                        }
                    }
                }
            }
        }

        if superseded {
            // No flush. A stopped search was cut off part-way through an
            // iteration, so what is buffered is a set that was never true of any
            // depth — see `progress::DepthTracker::flush`.
            return Err(EngineError::Cancelled);
        }

        // The last iteration is finished, so report it now rather than leaving
        // the caller to infer it from the result. For an interactive analysis
        // that is the difference between the arrows reaching full depth when the
        // engine gets there and reaching it when everything else is also done.
        if let Some(progress) = &spec.progress
            && let Some(closed) = tracker.flush()
        {
            let _ = progress.send(closed);
        }

        let depth = best.values().map(|info| info.depth).max().unwrap_or(0);
        Ok(SearchOutcome {
            depth,
            infos: best.into_values().collect(),
        })
    }

    /// MultiPV is a global UCI option. Setting it here — between searches, on the
    /// one task that owns the process — is what keeps [`crate::Engine::long_pv`]
    /// from stepping on a concurrent [`crate::Engine::analyze`]. There is no
    /// separate "restore": every search declares the value it needs and gets it.
    async fn set_multipv(&mut self, multipv: usize) -> Result<(), EngineError> {
        let multipv = multipv.max(1);
        if self.multipv == multipv {
            return Ok(());
        }
        self.send(&format!("setoption name MultiPV value {multipv}"))
            .await?;
        self.multipv = multipv;
        Ok(())
    }

    async fn send(&mut self, command: &str) -> Result<(), EngineError> {
        tracing::trace!(command, "uci >");
        write_line(&mut self.stdin, command)
            .await
            // A broken pipe means the process is gone, not a transient IO fault.
            .map_err(|_| EngineError::Died)
    }

    async fn quit(&mut self) {
        let _ = self.send("quit").await;
        match tokio::time::timeout(QUIT_TIMEOUT, self.child.wait()).await {
            Ok(Ok(_)) => {}
            _ => {
                let _ = self.child.start_kill();
            }
        }
    }
}
