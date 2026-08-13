//! Opening book detection. Runs **before** the engine so book moves are skipped
//! entirely — calling a theoretical move "inaccurate" is worthless.
//!
//! Uses the Lichess Opening Explorer API, which has no depth limit as long as the
//! moves were actually played, so long book lines (the Berlin, for instance) are
//! followed correctly.
//!
//! Rate limiting is the real constraint. <https://lichess.org/page/api-tips> asks
//! for one request at a time, and for a **full minute** of silence after a 429.
//! So:
//! - Serialize requests (never in parallel).
//! - On a 429 — or a 401/403, which is what the Explorer has been returning to
//!   everyone since early 2026, see [`DEFAULT_BASE_URL`] — enter a shared
//!   cooldown and report `OutOfBook` instead of blocking. The book runs first in
//!   the analysis pipeline, so sleeping a minute inside a request would freeze a
//!   whole game sweep; the caller degrades gracefully to "no book" instead.
//! - Cache in SQLite (`kibitz_store::Store`). The cache keeps answering during
//!   a cooldown.
//! - **Stop querying once the game leaves the book** — it never re-enters.
//!   That last one is what keeps a game down to 10-20 requests.
//!
//! Opening *names* do not depend on any of this: [`eco`] answers them from a
//! table embedded in the binary. It is a strictly separate concern — an ECO
//! table names a position, it cannot say whether a move is still theory, so it
//! never produces [`kibitz_core::Classification::Book`].

pub mod eco;

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kibitz_core::types::OpeningInfo;
use shakmaty::{Chess, Color, EnPassantMode, Position, fen::Fen, uci::UciMove};

/// Root of the Explorer API. Endpoints are `{base}/masters` and `{base}/lichess`.
///
/// Neither host could be verified working. As of 2026-08 **both**
/// `explorer.lichess.ovh` and `explorer.lichess.org` answer every Explorer
/// request with 401 from Lichess's own nginx, while `lichess.org/api/...` and
/// `tablebase.lichess.ovh` are fine — an outage tracked by lichess-org/lila#19610
/// (429 for everything from 2026-02-23, later 401), still open, and breaking
/// other clients (En Croissant, openingtree.com) the same way.
///
/// So this stays on `.ovh`: it is the host documented by the upstream
/// `lila-openingexplorer` README and the one the service used before the outage.
/// The official OpenAPI spec (`lichess-org/api`,
/// `doc/specs/tags/openingexplorer/*.yaml`) publishes
/// `https://explorer.lichess.org` instead; reach it through [`BASE_URL_ENV`]
/// without a rebuild once there is evidence either way.
pub const DEFAULT_BASE_URL: &str = "https://explorer.lichess.ovh";

/// Environment override for [`DEFAULT_BASE_URL`], e.g.
/// `KIBITZ_BOOK_URL=https://explorer.lichess.org` (the host named by the
/// official OpenAPI spec).
pub const BASE_URL_ENV: &str = "KIBITZ_BOOK_URL";

/// Explorer root to use: `$KIBITZ_BOOK_URL` if set and non-empty, else
/// [`DEFAULT_BASE_URL`].
pub fn base_url_from_env() -> String {
    match std::env::var(BASE_URL_ENV) {
        Ok(url) if !url.trim().is_empty() => url.trim().to_string(),
        _ => DEFAULT_BASE_URL.to_string(),
    }
}

/// Sent on every request. The Explorer is a free service run by volunteers, so
/// identify the client and point at the project.
pub const USER_AGENT: &str = concat!(
    "kibitz/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ekkx/kibitz)"
);

/// Which database to query. `masters` is stricter but may flag amateur games as
/// leaving book too early, so this stays switchable (see the open questions in
/// the design doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Database {
    /// `{base}/masters`
    Masters,
    /// `{base}/lichess`
    Lichess,
}

impl Database {
    pub fn from_env() -> Database {
        match std::env::var("KIBITZ_BOOK_DB").as_deref() {
            Ok("lichess") => Database::Lichess,
            _ => Database::Masters,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Database::Masters => "masters",
            Database::Lichess => "lichess",
        }
    }
}

/// Default number of continuations to request. The API's own default is 12,
/// which is a correctness problem here: we ask "was the move played?", and a
/// perfectly ordinary sideline can sit below the twelfth most common move in a
/// busy opening position. Truncated at 12 the move would be reported as
/// `LeftBook` even though it is theory.
///
/// 30 is comfortably past the point where a continuation could still clear
/// [`BookConfig::min_games`] in a position we would call book, while staying far
/// below the number of legal moves in a typical opening position (~30-40), so
/// the response does not grow much: each entry is a handful of small fields.
pub const DEFAULT_MOVES: u32 = 30;

/// Default silence after a 429, per <https://lichess.org/page/api-tips>:
/// "please wait a full minute before resuming API usage".
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(60);

/// Default silence after a 401/403. An auth failure is not a rate limit and will
/// not clear in a minute — the Explorer has been answering 401 to everyone since
/// early 2026 (lichess-org/lila#19610) — so back off for what is effectively the
/// rest of the process's life rather than firing ~20 doomed requests per game.
pub const DEFAULT_UNAVAILABLE_COOLDOWN: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
pub struct BookConfig {
    pub database: Database,
    /// Minimum game count; moves below it do not count as book.
    pub min_games: u64,
    /// How long to stop issuing Explorer requests after a 429. A longer
    /// `Retry-After` wins over this. See [`DEFAULT_COOLDOWN`].
    pub cooldown: Duration,
    /// How long to stop issuing Explorer requests after a 401/403. See
    /// [`DEFAULT_UNAVAILABLE_COOLDOWN`].
    pub unavailable_cooldown: Duration,
    /// How many of the most common continuations to ask for. See
    /// [`DEFAULT_MOVES`] for why the API default of 12 is not enough.
    pub moves: u32,
}

impl Default for BookConfig {
    fn default() -> Self {
        BookConfig {
            database: Database::from_env(),
            min_games: 10,
            cooldown: DEFAULT_COOLDOWN,
            unavailable_cooldown: DEFAULT_UNAVAILABLE_COOLDOWN,
            moves: DEFAULT_MOVES,
        }
    }
}

/// The parts of the Explorer API response we need.
///
/// Everything the crate does not use (`topGames`, and `recentGames` / `history`
/// on `/lichess`) is simply not declared: serde ignores unknown fields, so the
/// response can grow without breaking us. `opening` is nullable in the schema
/// and null for the first few plies of a game.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExplorerResponse {
    pub opening: Option<ExplorerOpening>,
    pub moves: Vec<ExplorerMove>,
    pub white: u64,
    pub draws: u64,
    pub black: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExplorerOpening {
    pub eco: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExplorerMove {
    pub uci: String,
    pub san: String,
    pub white: u64,
    pub draws: u64,
    pub black: u64,
    /// Mean rating of the players who chose this move. Required by the schema,
    /// but declared optional here so that a body written by an older version of
    /// this crate (the SQLite cache stores raw response bodies) still parses.
    /// Not used by the verdict: "is this move theory?" is a question about how
    /// often it was played, not about how strong the players were. It is kept so
    /// callers can report it, and as the natural input if the `/lichess`
    /// database ever needs a strength filter to go with `min_games`.
    #[serde(default, rename = "averageRating")]
    pub average_rating: Option<u32>,
}

impl ExplorerMove {
    pub fn total(&self) -> u64 {
        self.white + self.draws + self.black
    }
}

impl ExplorerResponse {
    /// Total number of games that reached this position.
    pub fn total(&self) -> u64 {
        self.white + self.draws + self.black
    }

    /// Whether the position is known to the database at all. A position with no
    /// games *and* no continuations is simply not in there.
    pub fn is_known(&self) -> bool {
        self.total() > 0 || !self.moves.is_empty()
    }
}

/// Verdict for a single move.
#[derive(Debug, Clone, PartialEq)]
pub enum BookVerdict {
    /// Theory. Skip both analysis and explanation.
    InBook(OpeningInfo),
    /// The move that left the book. Nothing after this is queried.
    LeftBook(Option<OpeningInfo>),
    /// The position is not in the database at all — already out of book.
    OutOfBook,
}

/// One HTTP reply, reduced to what the retry logic needs.
///
/// The transport is abstracted so tests can run without a network: see
/// [`ExplorerHttp`].
#[derive(Debug, Clone)]
pub struct HttpReply {
    pub status: u16,
    /// Parsed `Retry-After` header, when present. Only the delay-seconds form is
    /// understood; the HTTP-date form is treated as absent, which falls back to
    /// [`BookConfig::cooldown`].
    pub retry_after: Option<Duration>,
    pub body: String,
}

/// The HTTP seam. The production implementation is [`ReqwestHttp`]; tests inject
/// a fake so no request ever leaves the machine.
#[async_trait::async_trait]
pub trait ExplorerHttp: Send + Sync {
    /// Perform a GET. Transport failures map to [`BookError::Http`]; HTTP status
    /// codes are reported through [`HttpReply::status`], not as errors.
    async fn get(&self, url: &str) -> Result<HttpReply, BookError>;
}

/// `reqwest`-backed transport with a descriptive `User-Agent`.
pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    pub fn new() -> ReqwestHttp {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        ReqwestHttp { client }
    }
}

impl Default for ReqwestHttp {
    fn default() -> Self {
        ReqwestHttp::new()
    }
}

#[async_trait::async_trait]
impl ExplorerHttp for ReqwestHttp {
    async fn get(&self, url: &str) -> Result<HttpReply, BookError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| BookError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = resp
            .text()
            .await
            .map_err(|e| BookError::Http(e.to_string()))?;
        Ok(HttpReply {
            status,
            retry_after,
            body,
        })
    }
}

pub struct Book {
    config: BookConfig,
    store: kibitz_store::Store,
    http: Arc<dyn ExplorerHttp>,
    base_url: String,
    /// Semaphore of one: every Explorer request goes through this, so requests
    /// are never issued in parallel no matter how many tasks call in.
    gate: tokio::sync::Mutex<()>,
    /// Set on a 429: no request may be issued before this instant. Shared by
    /// every caller of this `Book`, and never held across an `.await`.
    cooldown_until: Mutex<Option<Instant>>,
}

impl Book {
    pub fn new(config: BookConfig, store: kibitz_store::Store) -> Book {
        Book::with_http(
            config,
            store,
            base_url_from_env(),
            Arc::new(ReqwestHttp::new()),
        )
    }

    /// Same as [`Book::new`], but with an injectable transport and base URL.
    /// Used by the tests to avoid the network entirely.
    pub fn with_http(
        config: BookConfig,
        store: kibitz_store::Store,
        base_url: impl Into<String>,
        http: Arc<dyn ExplorerHttp>,
    ) -> Book {
        Book {
            config,
            store,
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            gate: tokio::sync::Mutex::new(()),
            cooldown_until: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &BookConfig {
        &self.config
    }

    /// How much of the cooldown is left, if any. `None` means the Explorer may
    /// be queried.
    pub fn cooldown_remaining(&self) -> Option<Duration> {
        let until = *self
            .cooldown_until
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        until
            .and_then(|t| t.checked_duration_since(Instant::now()))
            .filter(|left| !left.is_zero())
    }

    /// Start (or extend) the cooldown: no request may be issued for `wait`.
    ///
    /// Logged once per cooldown window — a second failure of the same kind
    /// cannot happen until this one expires, because the cooldown suppresses
    /// the requests that would produce it.
    fn enter_cooldown(&self, wait: Duration, status: u16, reason: &str) {
        let until = Instant::now() + wait;
        {
            let mut guard = self
                .cooldown_until
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // Never shorten a cooldown an earlier failure already set.
            if guard.is_none_or(|current| until > current) {
                *guard = Some(until);
            }
        }
        tracing::warn!(
            "opening explorer {reason} (status {status}); \
             pausing book lookups for {wait:?}, analysis continues without the book"
        );
    }

    /// Verdict for playing `uci` in `pos`.
    ///
    /// Consults the SQLite cache first and hits the API only on a miss.
    /// Requests are serialized. While the book is in a cooldown (see
    /// [`Book::cooldown_remaining`]) this answers [`BookVerdict::OutOfBook`] for
    /// anything not already cached, without issuing a request.
    pub async fn judge(&self, pos: &Chess, uci: &str) -> Result<BookVerdict, BookError> {
        let fen = position_fen(pos);
        match self.lookup(&fen).await? {
            Some(resp) => Ok(verdict(&resp, uci, ply_of(pos), self.config.min_games)),
            None => Ok(BookVerdict::OutOfBook),
        }
    }

    /// Judge a whole game from the start, stopping all queries once a move
    /// returns `LeftBook`.
    ///
    /// The returned vector has the same length as `ucis`; everything after the
    /// cutoff is filled with `OutOfBook`. During a cooldown the first uncached
    /// position already yields `OutOfBook`, so the whole line comes back as
    /// `OutOfBook` without a single request.
    pub async fn judge_line(
        &self,
        start: &Chess,
        ucis: &[String],
    ) -> Result<Vec<BookVerdict>, BookError> {
        let mut verdicts = Vec::with_capacity(ucis.len());
        let mut pos = start.clone();
        let mut in_book = true;

        for uci in ucis {
            if !in_book {
                // A game never re-enters the book, so nothing past the cutoff is
                // worth a request.
                verdicts.push(BookVerdict::OutOfBook);
                continue;
            }

            let verdict = self.judge(&pos, uci).await?;
            if !matches!(verdict, BookVerdict::InBook(_)) {
                in_book = false;
            }
            verdicts.push(verdict);

            if in_book {
                pos = play_uci(pos, uci)?;
            }
        }

        Ok(verdicts)
    }

    /// Cache lookup, falling back to the API. The response is cached as the raw
    /// JSON body, keyed by `(normalized fen, database)`.
    ///
    /// `Ok(None)` means "the Explorer is off limits right now" — a cooldown was
    /// already running, or this call is the one that hit the 429. The caller
    /// treats that as out of book.
    async fn lookup(&self, fen: &str) -> Result<Option<ExplorerResponse>, BookError> {
        let key = kibitz_core::normalize_fen(fen);
        let db = self.config.database.as_str();

        // The cache keeps working through a cooldown, so check it first.
        if let Some(resp) = self.cached(&key, db) {
            return Ok(Some(resp));
        }
        if let Some(left) = self.cooldown_remaining() {
            tracing::debug!("skipping explorer lookup, {left:?} of rate-limit cooldown left");
            return Ok(None);
        }

        // Serialize: only one request is in flight at any time.
        let _permit = self.gate.lock().await;

        // Another task may have filled the cache — or tripped the rate limit —
        // while we waited for the gate.
        if let Some(resp) = self.cached(&key, db) {
            return Ok(Some(resp));
        }
        if self.cooldown_remaining().is_some() {
            return Ok(None);
        }

        let url = self.request_url(fen);
        let reply = self.http.get(&url).await?;
        let body = match reply.status {
            200..=299 => reply.body,
            429 => {
                // `Retry-After` wins when it asks for longer than the minimum.
                let wait = reply
                    .retry_after
                    .unwrap_or_default()
                    .max(self.config.cooldown);
                self.enter_cooldown(wait, 429, "rate limited");
                return Ok(None);
            }
            // The whole Explorer has been answering 401 since early 2026
            // (lichess-org/lila#19610). Retrying cannot help and no credential
            // exists to supply, so treat it like a rate limit with a much
            // longer cooldown instead of failing ~20 times per game.
            status @ (401 | 403) => {
                self.enter_cooldown(self.config.unavailable_cooldown, status, "unavailable");
                return Ok(None);
            }
            status => {
                return Err(BookError::Http(format!(
                    "status {status}: {}",
                    reply.body.chars().take(200).collect::<String>()
                )));
            }
        };

        let resp: ExplorerResponse = serde_json::from_str(&body)
            .map_err(|e| BookError::Http(format!("malformed explorer response: {e}")))?;
        self.store
            .put_book(&key, db, &body)
            .map_err(|e| BookError::Store(e.to_string()))?;
        Ok(Some(resp))
    }

    /// Build the request URL.
    ///
    /// `topGames` (and `recentGames` on `/lichess`) are pinned to 0: the crate
    /// never looks at game lists, and the Explorer is a free service run by
    /// volunteers, so there is no reason to make it serialize 15 games per
    /// position. `moves` is sent explicitly; see [`DEFAULT_MOVES`].
    fn request_url(&self, fen: &str) -> String {
        let mut url = format!(
            "{}/{}?fen={}&moves={}&topGames=0",
            self.base_url,
            self.config.database.as_str(),
            percent_encode(fen),
            self.config.moves,
        );
        if self.config.database == Database::Lichess {
            url.push_str("&recentGames=0");
        }
        url
    }

    fn cached(&self, key: &str, db: &str) -> Option<ExplorerResponse> {
        match self.store.get_book(key, db) {
            Ok(Some(json)) => match serde_json::from_str(&json) {
                Ok(resp) => Some(resp),
                Err(e) => {
                    // A row we cannot parse is worse than no row; refetch.
                    tracing::warn!("discarding unparsable cached explorer row: {e}");
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("book cache read failed: {e}");
                None
            }
        }
    }
}

/// Turn one Explorer response into a verdict.
///
/// `ply` is the number of plies already played when reaching this position, so
/// a book move matches through `ply + 1`.
fn verdict(resp: &ExplorerResponse, uci: &str, ply: u32, min_games: u64) -> BookVerdict {
    if !resp.is_known() {
        return BookVerdict::OutOfBook;
    }

    let played = resp
        .moves
        .iter()
        .find(|m| m.uci.eq_ignore_ascii_case(uci))
        .filter(|m| m.total() >= min_games);

    match played {
        Some(_) => BookVerdict::InBook(opening_info(resp, ply + 1)),
        // The position is theory, but this continuation is not: this is the move
        // that left the book.
        None => BookVerdict::LeftBook(resp.opening.as_ref().map(|_| opening_info(resp, ply))),
    }
}

/// `eco` / `name` fall back to empty strings: the very first plies are in the
/// database but have no named opening yet, and they are still book.
fn opening_info(resp: &ExplorerResponse, matched_plies: u32) -> OpeningInfo {
    let (eco, name) = match &resp.opening {
        Some(o) => (o.eco.clone(), o.name.clone()),
        None => (String::new(), String::new()),
    };
    OpeningInfo {
        eco,
        name,
        matched_plies,
    }
}

fn position_fen(pos: &Chess) -> String {
    Fen::from_position(pos, EnPassantMode::Legal).to_string()
}

/// Plies played to reach `pos`, derived from the move counters. For a game
/// starting from the initial position this is the absolute ply number, which is
/// what `OpeningInfo::matched_plies` reports.
fn ply_of(pos: &Chess) -> u32 {
    let fullmoves = pos.fullmoves().get();
    (fullmoves - 1) * 2 + u32::from(pos.turn() == Color::Black)
}

fn play_uci(pos: Chess, uci: &str) -> Result<Chess, BookError> {
    let parsed =
        UciMove::from_str(uci).map_err(|_| BookError::Http(format!("invalid uci move: {uci}")))?;
    let mv = parsed
        .to_move(&pos)
        .map_err(|_| BookError::Http(format!("illegal move: {uci}")))?;
    pos.play(mv)
        .map_err(|_| BookError::Http(format!("illegal move: {uci}")))
}

/// Percent-encode a FEN for use in a query string (spaces and slashes at least).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[derive(Debug, thiserror::Error)]
pub enum BookError {
    #[error("explorer api error: {0}")]
    Http(String),
    #[error("store error: {0}")]
    Store(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Scripted transport: replies are handed out in order (requests are
    /// serialized, so the order is deterministic), and every call is counted.
    struct FakeHttp {
        script: Mutex<VecDeque<HttpReply>>,
        fallback: HttpReply,
        calls: AtomicUsize,
        urls: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        fn new(script: Vec<HttpReply>) -> Arc<FakeHttp> {
            Arc::new(FakeHttp {
                script: Mutex::new(script.into()),
                fallback: reply(200, &empty_body()),
                calls: AtomicUsize::new(0),
                urls: Mutex::new(Vec::new()),
            })
        }

        fn always(reply: HttpReply) -> Arc<FakeHttp> {
            Arc::new(FakeHttp {
                script: Mutex::new(VecDeque::new()),
                fallback: reply,
                calls: AtomicUsize::new(0),
                urls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl ExplorerHttp for FakeHttp {
        async fn get(&self, url: &str) -> Result<HttpReply, BookError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.urls.lock().unwrap().push(url.to_string());
            let next = self.script.lock().unwrap().pop_front();
            Ok(next.unwrap_or_else(|| self.fallback.clone()))
        }
    }

    fn reply(status: u16, body: &str) -> HttpReply {
        HttpReply {
            status,
            retry_after: None,
            body: body.to_string(),
        }
    }

    /// A position the Explorer has never seen.
    fn empty_body() -> String {
        serde_json::json!({
            "opening": null,
            "moves": [],
            "white": 0,
            "draws": 0,
            "black": 0
        })
        .to_string()
    }

    /// A position with the given continuations, as `(uci, games)`. Shaped like
    /// the documented schema, `game` / `opening` nulls included.
    fn body_with(moves: &[(&str, u64)]) -> String {
        let moves: Vec<_> = moves
            .iter()
            .map(|(uci, games)| {
                serde_json::json!({
                    "uci": uci,
                    "san": uci,
                    "averageRating": 2412,
                    "white": games,
                    "draws": 0,
                    "black": 0,
                    "game": null,
                    "opening": null
                })
            })
            .collect();
        serde_json::json!({
            "opening": { "eco": "C65", "name": "Ruy Lopez: Berlin Defense" },
            "moves": moves,
            "white": 1000,
            "draws": 500,
            "black": 400
        })
        .to_string()
    }

    fn make_book(http: Arc<FakeHttp>, config: BookConfig) -> Book {
        Book::with_http(
            config,
            kibitz_store::Store::in_memory().unwrap(),
            "http://test.invalid",
            http,
        )
    }

    /// The real cooldown is a minute; tests use a short one so the "requests
    /// resume afterwards" case can actually be observed.
    const TEST_COOLDOWN: Duration = Duration::from_millis(60);

    fn fast_config() -> BookConfig {
        BookConfig {
            database: Database::Masters,
            min_games: 10,
            cooldown: TEST_COOLDOWN,
            unavailable_cooldown: TEST_COOLDOWN * 10,
            moves: DEFAULT_MOVES,
        }
    }

    fn throttled(retry_after: Option<Duration>) -> HttpReply {
        HttpReply {
            status: 429,
            retry_after,
            body: String::new(),
        }
    }

    #[tokio::test]
    async fn in_book_move_is_recognized() {
        let http = FakeHttp::new(vec![reply(200, &body_with(&[("e2e4", 5000)]))]);
        let book = make_book(http.clone(), fast_config());

        let verdict = book.judge(&Chess::default(), "e2e4").await.unwrap();
        match verdict {
            BookVerdict::InBook(info) => {
                assert_eq!(info.eco, "C65");
                assert_eq!(info.matched_plies, 1);
            }
            other => panic!("expected InBook, got {other:?}"),
        }
        assert_eq!(http.calls(), 1);
    }

    #[tokio::test]
    async fn unknown_position_is_out_of_book() {
        let http = FakeHttp::new(vec![reply(200, &empty_body())]);
        let book = make_book(http.clone(), fast_config());

        let verdict = book.judge(&Chess::default(), "e2e4").await.unwrap();
        assert_eq!(verdict, BookVerdict::OutOfBook);
    }

    #[tokio::test]
    async fn move_not_in_the_list_leaves_book() {
        let http = FakeHttp::new(vec![reply(200, &body_with(&[("e2e4", 5000)]))]);
        let book = make_book(http.clone(), fast_config());

        let verdict = book.judge(&Chess::default(), "a2a3").await.unwrap();
        match verdict {
            BookVerdict::LeftBook(Some(info)) => {
                assert_eq!(info.name, "Ruy Lopez: Berlin Defense");
                // Nothing matched yet: this is ply 1 and it left the book.
                assert_eq!(info.matched_plies, 0);
            }
            other => panic!("expected LeftBook, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn min_games_filters_rare_moves() {
        // 9 games is below the default threshold of 10.
        let http = FakeHttp::new(vec![reply(200, &body_with(&[("e2e4", 9)]))]);
        let book = make_book(http.clone(), fast_config());
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::LeftBook(_)
        ));

        // The very same response counts as book once the threshold drops.
        let http = FakeHttp::new(vec![reply(200, &body_with(&[("e2e4", 9)]))]);
        let book = make_book(
            http.clone(),
            BookConfig {
                min_games: 5,
                ..fast_config()
            },
        );
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::InBook(_)
        ));
    }

    #[tokio::test]
    async fn cache_hit_avoids_the_fetch() {
        let http = FakeHttp::new(vec![reply(200, &body_with(&[("e2e4", 5000)]))]);
        let store = kibitz_store::Store::in_memory().unwrap();
        let book = Book::with_http(
            fast_config(),
            store.clone(),
            "http://test.invalid",
            http.clone(),
        );

        book.judge(&Chess::default(), "e2e4").await.unwrap();
        assert_eq!(http.calls(), 1);

        // Same position again: served from SQLite.
        book.judge(&Chess::default(), "e2e4").await.unwrap();
        assert_eq!(http.calls(), 1);

        // And a freshly constructed Book sharing the store is a hit too.
        let book = Book::with_http(fast_config(), store, "http://test.invalid", http.clone());
        book.judge(&Chess::default(), "e2e4").await.unwrap();
        assert_eq!(http.calls(), 1);
    }

    #[tokio::test]
    async fn cache_is_keyed_by_database() {
        let store = kibitz_store::Store::in_memory().unwrap();
        let http = FakeHttp::always(reply(200, &body_with(&[("e2e4", 5000)])));

        let masters = Book::with_http(
            BookConfig {
                database: Database::Masters,
                ..fast_config()
            },
            store.clone(),
            "http://test.invalid",
            http.clone(),
        );
        masters.judge(&Chess::default(), "e2e4").await.unwrap();

        let lichess = Book::with_http(
            BookConfig {
                database: Database::Lichess,
                ..fast_config()
            },
            store,
            "http://test.invalid",
            http.clone(),
        );
        lichess.judge(&Chess::default(), "e2e4").await.unwrap();

        // The masters row must not answer a lichess lookup.
        assert_eq!(http.calls(), 2);
        let urls = http.urls.lock().unwrap();
        assert!(urls[0].contains("/masters?"), "{}", urls[0]);
        assert!(urls[1].contains("/lichess?"), "{}", urls[1]);
    }

    #[tokio::test]
    async fn judge_line_stops_querying_after_left_book() {
        // 1. e4 is book; 1... a5 is not. The two positions after that are never
        // looked up.
        let http = FakeHttp::new(vec![
            reply(200, &body_with(&[("e2e4", 5000)])),
            reply(200, &body_with(&[("e7e5", 3000), ("c7c5", 2000)])),
        ]);
        let book = make_book(http.clone(), fast_config());

        let ucis: Vec<String> = ["e2e4", "a7a5", "d2d4", "d7d5"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();

        assert_eq!(verdicts.len(), ucis.len());
        assert!(matches!(verdicts[0], BookVerdict::InBook(_)));
        assert!(matches!(verdicts[1], BookVerdict::LeftBook(_)));
        assert_eq!(verdicts[2], BookVerdict::OutOfBook);
        assert_eq!(verdicts[3], BookVerdict::OutOfBook);
        assert_eq!(http.calls(), 2, "no request may follow the cutoff");
    }

    #[tokio::test]
    async fn judge_line_counts_matched_plies() {
        let http = FakeHttp::always(reply(
            200,
            &body_with(&[("e2e4", 5000), ("e7e5", 3000), ("g1f3", 2000)]),
        ));
        let book = make_book(http.clone(), fast_config());

        let ucis: Vec<String> = ["e2e4", "e7e5", "g1f3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();

        for (i, verdict) in verdicts.iter().enumerate() {
            match verdict {
                BookVerdict::InBook(info) => assert_eq!(info.matched_plies, i as u32 + 1),
                other => panic!("expected InBook at {i}, got {other:?}"),
            }
        }
        assert_eq!(http.calls(), 3);
    }

    #[tokio::test]
    async fn judge_line_out_of_book_also_stops_querying() {
        let http = FakeHttp::always(reply(200, &empty_body()));
        let book = make_book(http.clone(), fast_config());

        let ucis: Vec<String> = ["e2e4", "e7e5"].iter().map(|s| s.to_string()).collect();
        let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();

        assert_eq!(
            verdicts,
            vec![BookVerdict::OutOfBook, BookVerdict::OutOfBook]
        );
        assert_eq!(http.calls(), 1);
    }

    #[tokio::test]
    async fn rate_limit_starts_a_cooldown_instead_of_blocking() {
        let http = FakeHttp::new(vec![
            throttled(None),
            reply(200, &body_with(&[("e2e4", 5000)])),
        ]);
        let book = make_book(http.clone(), fast_config());

        // The 429 itself is not an error and does not block: it degrades to
        // "no book".
        let started = std::time::Instant::now();
        assert_eq!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::OutOfBook
        );
        assert_eq!(http.calls(), 1);
        assert!(
            started.elapsed() < TEST_COOLDOWN,
            "judge must return immediately, not wait out the cooldown"
        );
        assert!(book.cooldown_remaining().is_some());

        // Same uncached position again: no request at all while cooling down.
        assert_eq!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::OutOfBook
        );
        assert_eq!(http.calls(), 1, "no request may be issued during cooldown");

        // Once it expires, requests resume and the scripted 200 is used.
        tokio::time::sleep(TEST_COOLDOWN + Duration::from_millis(40)).await;
        assert!(book.cooldown_remaining().is_none());
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::InBook(_)
        ));
        assert_eq!(http.calls(), 2);
    }

    #[tokio::test]
    async fn judge_line_issues_no_requests_during_cooldown() {
        let http = FakeHttp::always(throttled(None));
        let book = make_book(http.clone(), fast_config());

        let ucis: Vec<String> = ["e2e4", "e7e5", "g1f3", "b8c6"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();

        assert!(verdicts.iter().all(|v| *v == BookVerdict::OutOfBook));
        // Only the very first position was ever requested.
        assert_eq!(http.calls(), 1);

        // A second sweep touches the network zero times.
        let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();
        assert!(verdicts.iter().all(|v| *v == BookVerdict::OutOfBook));
        assert_eq!(http.calls(), 1);
    }

    #[tokio::test]
    async fn retry_after_longer_than_the_default_extends_the_cooldown() {
        let http = FakeHttp::always(throttled(Some(Duration::from_secs(120))));
        let book = make_book(http.clone(), fast_config());

        book.judge(&Chess::default(), "e2e4").await.unwrap();
        let left = book.cooldown_remaining().expect("cooldown must be running");
        assert!(
            left > Duration::from_secs(60),
            "Retry-After must win over the {TEST_COOLDOWN:?} default, got {left:?}"
        );

        // A shorter Retry-After never shortens the configured minimum.
        let http = FakeHttp::always(throttled(Some(Duration::from_millis(1))));
        let book = make_book(http.clone(), fast_config());
        book.judge(&Chess::default(), "e2e4").await.unwrap();
        assert!(book.cooldown_remaining().unwrap() > Duration::from_millis(1));
    }

    #[tokio::test]
    async fn cached_position_still_answers_during_cooldown() {
        let http = FakeHttp::new(vec![
            reply(200, &body_with(&[("e2e4", 5000)])),
            throttled(None),
        ]);
        let book = make_book(http.clone(), fast_config());

        // Cache the starting position, then trip the rate limit on another one.
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::InBook(_)
        ));
        let after_e4 = play_uci(Chess::default(), "e2e4").unwrap();
        assert_eq!(
            book.judge(&after_e4, "e7e5").await.unwrap(),
            BookVerdict::OutOfBook
        );
        assert!(book.cooldown_remaining().is_some());
        assert_eq!(http.calls(), 2);

        // SQLite still answers, no request needed.
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::InBook(_)
        ));
        assert_eq!(http.calls(), 2);
    }

    #[tokio::test]
    async fn rate_limited_response_is_not_cached() {
        let http = FakeHttp::new(vec![
            throttled(None),
            reply(200, &body_with(&[("e2e4", 5000)])),
        ]);
        let book = make_book(
            http.clone(),
            BookConfig {
                cooldown: Duration::from_millis(1),
                ..fast_config()
            },
        );

        assert_eq!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::OutOfBook
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        // The empty 429 body left no row behind: the position is refetched.
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::InBook(_)
        ));
        assert_eq!(http.calls(), 2);
    }

    #[tokio::test]
    async fn auth_failure_stops_querying_instead_of_erroring() {
        // What the live Explorer actually does today (lichess-org/lila#19610).
        for status in [401, 403] {
            let http = FakeHttp::always(reply(status, "Unauthorized"));
            let book = make_book(http.clone(), fast_config());

            let ucis: Vec<String> = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();

            // Not an error: the pipeline analyses on without a book.
            assert!(verdicts.iter().all(|v| *v == BookVerdict::OutOfBook));
            assert_eq!(http.calls(), 1, "status {status} must not be retried");

            // And the cooldown is much longer than the rate-limit one, because
            // an auth failure will not clear in a minute.
            let left = book.cooldown_remaining().expect("cooldown must be running");
            assert!(left > TEST_COOLDOWN, "status {status}: {left:?}");

            // Later sweeps in the same session stay quiet too.
            book.judge_line(&Chess::default(), &ucis).await.unwrap();
            assert_eq!(http.calls(), 1);
        }
    }

    #[tokio::test]
    async fn request_carries_the_documented_parameters() {
        // Masters: moves + topGames, no recentGames (the parameter does not
        // exist on /masters).
        let http = FakeHttp::always(reply(200, &empty_body()));
        let book = make_book(http.clone(), fast_config());
        book.judge(&Chess::default(), "e2e4").await.unwrap();
        let url = http.urls.lock().unwrap()[0].clone();
        assert!(url.starts_with("http://test.invalid/masters?fen="), "{url}");
        assert!(url.contains(&format!("&moves={DEFAULT_MOVES}")), "{url}");
        assert!(url.contains("&topGames=0"), "{url}");
        assert!(!url.contains("recentGames"), "{url}");

        // Lichess: also recentGames=0.
        let http = FakeHttp::always(reply(200, &empty_body()));
        let book = make_book(
            http.clone(),
            BookConfig {
                database: Database::Lichess,
                moves: 25,
                ..fast_config()
            },
        );
        book.judge(&Chess::default(), "e2e4").await.unwrap();
        let url = http.urls.lock().unwrap()[0].clone();
        assert!(url.starts_with("http://test.invalid/lichess?fen="), "{url}");
        assert!(url.contains("&moves=25"), "{url}");
        assert!(url.contains("&topGames=0"), "{url}");
        assert!(url.contains("&recentGames=0"), "{url}");
    }

    #[tokio::test]
    async fn move_below_the_api_default_move_limit_is_in_book() {
        // Twenty continuations: with the API's default of 12 the response would
        // stop before `a2a3` and the move would be misreported as LeftBook.
        let ucis = [
            "e2e4", "d2d4", "g1f3", "c2c4", "g2g3", "b2b3", "f2f4", "b1c3", "b2b4", "e2e3", "d2d3",
            "c2c3", "a2a4", "h2h3", "a2a3", "h2h4", "g2g4", "f2f3", "b1a3", "g1h3",
        ];
        let moves: Vec<(&str, u64)> = ucis
            .iter()
            .enumerate()
            .map(|(i, uci)| (*uci, 100_000 / (i as u64 + 1)))
            .collect();
        let http = FakeHttp::always(reply(200, &body_with(&moves)));
        let book = make_book(http.clone(), fast_config());

        // 15th most common, well past the twelve the API would return by default.
        assert!(matches!(
            book.judge(&Chess::default(), "a2a3").await.unwrap(),
            BookVerdict::InBook(_)
        ));
        assert!(
            http.urls.lock().unwrap()[0].contains(&format!("&moves={DEFAULT_MOVES}")),
            "the fix is the request parameter, not just the parsing"
        );
    }

    #[tokio::test]
    async fn null_opening_and_null_game_parse() {
        // Shaped exactly like a documented /lichess reply: nullable `opening`
        // and `game`, plus fields this crate does not declare.
        let body = serde_json::json!({
            "white": 12,
            "draws": 3,
            "black": 9,
            "opening": null,
            "moves": [{
                "uci": "e2e4",
                "san": "e4",
                "averageRating": 1834,
                "white": 5000,
                "draws": 1000,
                "black": 4000,
                "game": null,
                "opening": null
            }],
            "topGames": [],
            "recentGames": [],
            "history": [{ "month": "2024-01", "white": 1, "draws": 0, "black": 2 }],
            "queuePosition": 0
        })
        .to_string();

        let parsed: ExplorerResponse = serde_json::from_str(&body).expect("must parse");
        assert_eq!(parsed.moves[0].average_rating, Some(1834));
        assert!(parsed.opening.is_none());

        let http = FakeHttp::always(reply(200, &body));
        let book = make_book(http.clone(), fast_config());
        match book.judge(&Chess::default(), "e2e4").await.unwrap() {
            // No named opening yet, but still book.
            BookVerdict::InBook(info) => {
                assert!(info.eco.is_empty());
                assert_eq!(info.matched_plies, 1);
            }
            other => panic!("expected InBook, got {other:?}"),
        }
    }

    #[test]
    fn move_without_average_rating_still_parses() {
        // Rows cached by an older build of this crate, and the hand-written
        // fixtures above, omit the field. It must stay optional.
        let body = serde_json::json!({
            "white": 1, "draws": 0, "black": 0, "opening": null,
            "moves": [{ "uci": "e2e4", "san": "e4", "white": 1, "draws": 0, "black": 0 }]
        })
        .to_string();
        let parsed: ExplorerResponse = serde_json::from_str(&body).expect("must parse");
        assert_eq!(parsed.moves[0].average_rating, None);
    }

    #[test]
    fn default_host_is_the_upstream_one() {
        // Deliberately *not* the host published by the OpenAPI spec: both
        // answer 401 today, so there is no evidence to switch the default on,
        // and `.ovh` is what the service used before the outage. See
        // `DEFAULT_BASE_URL`. The env override itself is not exercised here:
        // tests share one process and `std::env::set_var` is unsafe in
        // edition 2024.
        assert_eq!(DEFAULT_BASE_URL, "https://explorer.lichess.ovh");
        assert!(!base_url_from_env().is_empty());
    }

    #[tokio::test]
    async fn trailing_slash_in_the_base_url_is_trimmed() {
        let http = FakeHttp::always(reply(200, &empty_body()));
        let book = Book::with_http(
            fast_config(),
            kibitz_store::Store::in_memory().unwrap(),
            "http://test.invalid/",
            http.clone(),
        );
        book.judge(&Chess::default(), "e2e4").await.unwrap();
        let url = http.urls.lock().unwrap()[0].clone();
        assert!(url.starts_with("http://test.invalid/masters?"), "{url}");
    }

    #[tokio::test]
    async fn server_error_is_reported_not_retried() {
        let http = FakeHttp::always(reply(500, "boom"));
        let book = make_book(http.clone(), fast_config());

        let err = book.judge(&Chess::default(), "e2e4").await.unwrap_err();
        assert!(matches!(err, BookError::Http(_)), "{err:?}");
        assert_eq!(http.calls(), 1);
    }

    #[tokio::test]
    async fn malformed_response_is_not_cached() {
        let http = FakeHttp::new(vec![
            reply(200, "not json"),
            reply(200, &body_with(&[("e2e4", 5000)])),
        ]);
        let book = make_book(http.clone(), fast_config());

        assert!(book.judge(&Chess::default(), "e2e4").await.is_err());
        // The bad body left no row behind, so the next call retries the API.
        assert!(matches!(
            book.judge(&Chess::default(), "e2e4").await.unwrap(),
            BookVerdict::InBook(_)
        ));
        assert_eq!(http.calls(), 2);
    }

    #[tokio::test]
    async fn concurrent_lookups_are_serialized() {
        // A fake that panics if two requests ever overlap.
        struct Exclusive {
            in_flight: AtomicUsize,
            calls: AtomicUsize,
        }

        #[async_trait::async_trait]
        impl ExplorerHttp for Exclusive {
            async fn get(&self, _url: &str) -> Result<HttpReply, BookError> {
                assert_eq!(
                    self.in_flight.fetch_add(1, Ordering::SeqCst),
                    0,
                    "requests must not overlap"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(HttpReply {
                    status: 200,
                    retry_after: None,
                    body: empty_body(),
                })
            }
        }

        let http = Arc::new(Exclusive {
            in_flight: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        });
        let book = Arc::new(Book::with_http(
            fast_config(),
            kibitz_store::Store::in_memory().unwrap(),
            "http://test.invalid",
            http.clone(),
        ));

        // Four distinct positions, so no request is served from the cache.
        let mut tasks = Vec::new();
        for uci in ["e2e4", "d2d4", "c2c4", "g1f3"] {
            let book = book.clone();
            tasks.push(tokio::spawn(async move {
                let pos = play_uci(Chess::default(), uci).unwrap();
                book.judge(&pos, "e7e5").await.unwrap()
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(http.calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn fen_is_percent_encoded() {
        let encoded = percent_encode("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert!(!encoded.contains(' '));
        assert!(!encoded.contains('/'));
        assert!(encoded.contains("%20"));
        assert!(encoded.contains("%2F"));
    }

    #[test]
    fn ply_counts_from_the_move_counters() {
        let pos = Chess::default();
        assert_eq!(ply_of(&pos), 0);
        let pos = play_uci(pos, "e2e4").unwrap();
        assert_eq!(ply_of(&pos), 1);
        let pos = play_uci(pos, "e7e5").unwrap();
        assert_eq!(ply_of(&pos), 2);
    }

    /// Hits the real Lichess Explorer, to confirm the response shape still
    /// parses and that the host and query parameters are accepted. Ignored by
    /// default so the test suite stays offline.
    ///
    /// **This test currently fails, and that is not a bug in this crate.** As of
    /// 2026-08 every Explorer endpoint answers 401 on both
    /// `explorer.lichess.ovh` and `explorer.lichess.org`, for every client
    /// (lichess-org/lila#19610); the rest of `lichess.org/api` and
    /// `tablebase.lichess.ovh` are fine. `judge_line` will come back all
    /// `OutOfBook` with a warn log rather than an error. Try the other host with
    /// `KIBITZ_BOOK_URL=https://explorer.lichess.org` before concluding anything
    /// about the code.
    ///
    /// Run it with:
    ///   cargo test -p kibitz-book -- --ignored --nocapture live_explorer_response_still_parses
    #[tokio::test]
    #[ignore = "hits the live Lichess Opening Explorer API"]
    async fn live_explorer_response_still_parses() {
        // `Book::new` picks up the documented host (or $KIBITZ_BOOK_URL) and
        // the default `moves` / `topGames=0` parameters.
        let book = Book::new(
            BookConfig {
                database: Database::Masters,
                ..BookConfig::default()
            },
            kibitz_store::Store::in_memory().unwrap(),
        );
        println!("base url: {}", base_url_from_env());

        let ucis: Vec<String> = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "g8f6"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let verdicts = book.judge_line(&Chess::default(), &ucis).await.unwrap();

        println!("{verdicts:#?}");
        assert_eq!(verdicts.len(), ucis.len());
        // The Berlin Defense is theory all the way through.
        assert!(
            verdicts.iter().all(|v| matches!(v, BookVerdict::InBook(_))),
            "{verdicts:#?}"
        );
    }
}
