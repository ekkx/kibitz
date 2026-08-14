//! Route handlers. `docs/API.md` is the contract.

use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use kibitz_core::tree::{GameTree, NodeId};
use kibitz_core::types::{AnalysisContext, PositionAnalysis};
use kibitz_llm::{Language, QaSession};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::session::SessionStore;
use crate::{AppState, pgn, pipeline};

/// SSE handlers finish by erasing their stream into a `Response`; `Sse::keep_alive`
/// wraps the stream type, so a shared alias would not hold otherwise.
type EventStream = futures::stream::BoxStream<'static, Result<Event, Infallible>>;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/languages", get(languages))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/play", post(play))
        .route("/api/sessions/{id}/analyze", post(analyze))
        .route("/api/sessions/{id}/analyze-game", post(analyze_game))
        .route("/api/sessions/{id}/explain/{node_id}", get(explain))
        .route("/api/sessions/{id}/ask", post(ask))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not found") })
}

// ─── health and languages ───────────────────────────────

/// Reports whether Stockfish actually started. The frontend calls this on startup
/// and warns the user when the engine is missing, so it is never a constant.
async fn health(State(state): State<AppState>) -> Json<crate::EngineHealth> {
    Json(state.health)
}

#[derive(Serialize)]
struct LanguagesResponse {
    languages: Vec<LanguageEntry>,
    default: &'static str,
}

#[derive(Serialize)]
struct LanguageEntry {
    code: &'static str,
    name: &'static str,
}

async fn languages() -> Json<LanguagesResponse> {
    Json(LanguagesResponse {
        languages: kibitz_llm::SUPPORTED_LANGUAGES
            .iter()
            .map(|&(_, code, name)| LanguageEntry { code, name })
            .collect(),
        default: Language::default().code(),
    })
}

// ─── sessions ───────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct CreateSessionRequest {
    pgn: Option<String>,
    fen: Option<String>,
}

#[derive(Serialize)]
struct SessionResponse {
    session_id: String,
    tree: GameTree,
    headers: HashMap<String, String>,
}

async fn create_session(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<SessionResponse>, ApiError> {
    let req: CreateSessionRequest = optional_body(&body)?;

    let (tree, headers) = match (req.pgn.as_deref(), req.fen.as_deref()) {
        (Some(pgn_text), _) => {
            let game = pgn::import(pgn_text)
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            (game.tree, game.headers)
        }
        // A FEN start still deserves an opening name when the position happens to
        // be a known one, so both of these go through `opening::new_tree`.
        (None, Some(fen)) => {
            let pos = kibitz_core::parse_fen(fen)
                .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))?;
            (crate::opening::new_tree(&pos), HashMap::new())
        }
        (None, None) => (
            crate::opening::new_tree(&shakmaty::Chess::default()),
            HashMap::new(),
        ),
    };

    let session = state.sessions.create(tree, headers);
    Ok(Json(SessionResponse {
        session_id: session.id,
        tree: session.tree,
        headers: session.headers,
    }))
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = state.sessions.get(&id).ok_or_else(session_not_found)?;
    Ok(Json(SessionResponse {
        session_id: session.id,
        tree: session.tree,
        headers: session.headers,
    }))
}

// ─── play ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct PlayRequest {
    node_id: NodeId,
    uci: Option<String>,
    san: Option<String>,
}

#[derive(Serialize)]
struct PlayResponse {
    node_id: NodeId,
    created: bool,
    tree: GameTree,
}

/// Playing a move that already exists returns the existing node — the tree merges
/// automatically, so retrying a line never duplicates it.
async fn play(
    State(state): State<AppState>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<PlayRequest>,
) -> Result<Json<PlayResponse>, ApiError> {
    let result = state
        .sessions
        .update(&id, |session| {
            if session.tree.get(req.node_id).is_none() {
                return Err(node_not_found());
            }
            let before = session.tree.nodes.len();
            let node_id = match (&req.uci, &req.san) {
                (Some(uci), _) => {
                    let pos = session
                        .tree
                        .position(req.node_id)
                        .map_err(|_| illegal_move())?;
                    let mv = uci
                        .parse::<shakmaty::uci::UciMove>()
                        .map_err(|_| illegal_move())?
                        .to_move(&pos)
                        .map_err(|_| illegal_move())?;
                    session.tree.play(req.node_id, mv)
                }
                (None, Some(san)) => session.tree.play_san(req.node_id, san),
                (None, None) => return Err(ApiError::new(StatusCode::BAD_REQUEST, "no move given")),
            }
            .map_err(|_| illegal_move())?;

            // The new node — or the existing one a merge returned, which already
            // carries the same name.
            crate::opening::annotate(&mut session.tree, node_id);

            Ok(PlayResponse {
                node_id,
                created: session.tree.nodes.len() > before,
                tree: session.tree.clone(),
            })
        })
        .ok_or_else(session_not_found)?;

    result.map(Json)
}

// ─── analyze one node ───────────────────────────────────

#[derive(Debug, Deserialize)]
struct AnalyzeRequest {
    node_id: NodeId,
    depth: Option<u8>,
}

/// Analyse one node, streaming the ranking as the search deepens.
///
/// Two events, and the names are load-bearing:
///
/// - `candidates` — the ranked moves at one completed depth. A **ranking**, and
///   nothing else. It carries no classification, no accuracy and no
///   counterfactual, and `pipeline::analyze_node_streaming` explains at length
///   why it must not grow one.
/// - `analysis` — the finished `PositionAnalysis`, byte for byte what this route
///   used to return as its JSON body.
///
/// A client that has never heard of `candidates` still works: it waits for
/// `analysis` and gets exactly the old behaviour, one event later.
///
/// **Where the errors went.** Everything checkable before the search — unknown
/// session, unknown node, no engine — is still an HTTP status with the usual
/// `{"error": ...}` body, because it is decided before the response begins. But
/// once the stream is open the status line has already been sent as `200`, so a
/// failure *during* the search can only be an `error` event. The one that matters
/// is `409 cancelled`: a newer `analyze` superseding this one now arrives as
/// `event: error {"error":"cancelled"}`, possibly after several `candidates`
/// events have already been drawn. Clients must treat the two shapes as the same
/// condition — `web/src/api/client.ts` does, by turning the event back into the
/// same `ApiError` the status used to produce.
///
/// The two-lane gate is untouched: this is still `Lane::Interactive`, so a
/// running sweep is waited for rather than killed, and the cache is still
/// consulted before the gate — a node the sweep already did answers with a single
/// `analysis` event and no `candidates` at all, because there was no search to
/// narrate.
async fn analyze(
    State(state): State<AppState>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<AnalyzeRequest>,
) -> Result<Response, ApiError> {
    let session = state.sessions.get(&id).ok_or_else(session_not_found)?;
    if session.tree.get(req.node_id).is_none() {
        return Err(node_not_found());
    }
    let pipeline = state.pipeline().ok_or_else(engine_unavailable)?;

    let (tx, rx) = unbounded_channel();
    let sessions = state.sessions.clone();
    let tree = session.tree;

    tokio::spawn(async move {
        let outcome = {
            let tx = tx.clone();
            pipeline
                .analyze_node_streaming(&tree, req.node_id, req.depth, move |update| {
                    send(
                        &tx,
                        "candidates",
                        json!({ "depth": update.depth, "candidates": update.candidates }),
                    );
                })
                .await
        };

        match outcome {
            Ok(analysis) => {
                // The store sees the finished analysis and only the finished
                // analysis; nothing above this line has touched it.
                store_analysis(&sessions, &id, req.node_id, analysis.clone());
                match serde_json::to_value(&analysis) {
                    Ok(value) => send(&tx, "analysis", value),
                    Err(e) => send(&tx, "error", json!({ "error": e.to_string() })),
                }
            }
            // Reuse `ApiError`'s mapping so the message a client reads is the
            // same string it used to read off the status body — "cancelled" for
            // a superseded search, and so on.
            Err(e) => send(&tx, "error", json!({ "error": ApiError::from(e).message })),
        }
    });

    Ok(sse(rx))
}

// ─── analyze the whole game (SSE) ───────────────────────

#[derive(Debug, Default, Deserialize)]
struct AnalyzeGameRequest {
    depth: Option<u8>,
}

async fn analyze_game(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let req: AnalyzeGameRequest = optional_body(&body)?;
    let session = state.sessions.get(&id).ok_or_else(session_not_found)?;
    // The sweep lane: a `POST /analyze` arriving mid-sweep now waits for the
    // current position instead of cancelling the whole run.
    let mut pipeline = state.sweep_pipeline().ok_or_else(engine_unavailable)?;
    if let Some(depth) = req.depth {
        pipeline = pipeline.with_depth(depth);
    }

    let (tx, rx) = unbounded_channel();
    let sessions = state.sessions.clone();
    let tree = session.tree;

    tokio::spawn(async move {
        let path = tree.mainline();
        let total = path.len();
        let outcome = {
            let tx = tx.clone();
            let sessions = sessions.clone();
            let id = id.clone();
            pipeline
                .analyze_path(&tree, &path, move |node_id, done, total, analysis| {
                    send(
                        &tx,
                        "progress",
                        json!({ "node_id": node_id, "done": done, "total": total }),
                    );
                    // The payload carries its own `node_id`. Pairing it with the
                    // preceding `progress` event would be unsound: the same
                    // position can occur twice in a game, so `fen` does not
                    // identify a node either.
                    match serde_json::to_value(analysis) {
                        Ok(mut value) => {
                            if let Some(obj) = value.as_object_mut() {
                                obj.insert("node_id".into(), json!(node_id));
                            }
                            send(&tx, "node", value);
                        }
                        Err(e) => send(&tx, "error", json!({ "error": e.to_string() })),
                    }
                    store_analysis(&sessions, &id, node_id, analysis.clone());
                })
                .await
        };

        match outcome {
            Ok(_) => send(&tx, "done", json!({ "total": total })),
            Err(e) => send(&tx, "error", json!({ "error": e.to_string() })),
        }
    });

    Ok(sse(rx))
}

// ─── explain (SSE) ──────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

async fn explain(
    State(state): State<AppState>,
    Path((id, node_id)): Path<(String, NodeId)>,
    Query(q): Query<LangQuery>,
) -> Result<Response, ApiError> {
    let lang = parse_lang(q.lang.as_deref())?;
    let session = state.sessions.get(&id).ok_or_else(session_not_found)?;
    let node = session.tree.get(node_id).ok_or_else(node_not_found)?;
    let analysis = node.analysis.as_ref().ok_or_else(not_analyzed)?;
    let context = analysis.context.clone().ok_or_else(not_analyzed)?;
    let model = state.models.narrate.clone();

    // Already generated for this language in this session.
    if let Some(text) = analysis.explanations.get(lang.code()) {
        return Ok(cached_stream(text.clone(), lang, model));
    }

    // Persisted from an earlier run. The key covers the context, the model **and**
    // the language, or a Japanese explanation would answer an English request.
    let hash = context_hash(&context, &model, lang);
    if let Some(Ok(Some(text))) = state.store.as_ref().map(|s| s.get_explanation(&hash)) {
        store_explanation(&state.sessions, &id, node_id, lang, &text);
        return Ok(cached_stream(text, lang, model));
    }

    let provider = state.llm.clone().ok_or_else(llm_unavailable)?;
    let (tx, rx) = unbounded_channel();
    let sessions = state.sessions.clone();
    let store = state.store.clone();

    tokio::spawn(async move {
        let stream = match provider.explain(&model, lang, &context).await {
            Ok(stream) => stream,
            Err(e) => {
                send(&tx, "error", json!({ "error": e.to_string() }));
                return;
            }
        };
        let Some(text) = drain(stream, &tx).await else {
            return;
        };

        store_explanation(&sessions, &id, node_id, lang, &text);
        if let Some(Err(e)) =
            store.map(|s| s.put_explanation(&hash, &text, &model, lang.code()))
        {
            tracing::warn!("failed to cache explanation: {e}");
        }
        send(
            &tx,
            "done",
            json!({ "text": text, "lang": lang.code(), "model": model, "cached": false }),
        );
    });

    Ok(sse(rx))
}

// ─── ask (SSE) ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AskRequest {
    node_id: NodeId,
    question: String,
    lang: Option<String>,
}

async fn ask(
    State(state): State<AppState>,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<AskRequest>,
) -> Result<Response, ApiError> {
    let lang = parse_lang(req.lang.as_deref())?;
    let session = state.sessions.get(&id).ok_or_else(session_not_found)?;
    let node = session.tree.get(req.node_id).ok_or_else(node_not_found)?;
    let context = node
        .analysis
        .as_ref()
        .and_then(|a| a.context.clone());

    let provider = state.llm.clone().ok_or_else(llm_unavailable)?;
    let model = state.models.reason.clone();
    let (tx, rx) = unbounded_channel();

    tokio::spawn(async move {
        let mut qa = QaSession {
            context,
            history: Vec::new(),
        };
        let stream = match provider.ask(&model, lang, &mut qa, &req.question).await {
            Ok(stream) => stream,
            Err(e) => {
                send(&tx, "error", json!({ "error": e.to_string() }));
                return;
            }
        };
        let Some(text) = drain(stream, &tx).await else {
            return;
        };
        send(
            &tx,
            "done",
            json!({ "text": text, "lang": lang.code(), "model": model, "cached": false }),
        );
    });

    Ok(sse(rx))
}

// ─── shared plumbing ────────────────────────────────────

fn send(tx: &UnboundedSender<Event>, name: &str, value: Value) {
    let _ = tx.send(Event::default().event(name).data(value.to_string()));
}

/// Forward an LLM text stream as `delta` events and return the full text.
/// `None` means the stream errored and an `error` event has already been sent.
async fn drain(mut stream: kibitz_llm::TextStream, tx: &UnboundedSender<Event>) -> Option<String> {
    let mut full = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(text) => {
                send(tx, "delta", json!({ "text": text }));
                full.push_str(&text);
            }
            Err(e) => {
                send(tx, "error", json!({ "error": e.to_string() }));
                return None;
            }
        }
    }
    Some(full)
}

fn sse(rx: UnboundedReceiver<Event>) -> Response {
    let stream: EventStream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (Ok(event), rx))
    })
    .boxed();
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// A cached explanation arrives as a single `done` event, as the contract says.
fn cached_stream(text: String, lang: Language, model: String) -> Response {
    let (tx, rx) = unbounded_channel();
    send(
        &tx,
        "done",
        json!({ "text": text, "lang": lang.code(), "model": model, "cached": true }),
    );
    sse(rx)
}

/// An absent or empty body means "all defaults", which is how `POST /api/sessions`
/// with no arguments starts a game from the initial position.
fn optional_body<T: serde::de::DeserializeOwned + Default>(body: &Bytes) -> Result<T, ApiError> {
    if body.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(body).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))
}

fn parse_lang(code: Option<&str>) -> Result<Language, ApiError> {
    match code {
        None | Some("") => Ok(Language::default()),
        Some(code) => Language::parse(code).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                format!("unsupported language: {code}"),
            )
        }),
    }
}

/// Cache key for an explanation: the context, the model and the language.
///
/// `DefaultHasher` is seeded with fixed keys, so this is stable across runs. It
/// is a cache key, not a checksum — collisions cost a stale explanation, nothing
/// worse.
fn context_hash(context: &AnalysisContext, model: &str, lang: Language) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(context)
        .unwrap_or_default()
        .hash(&mut hasher);
    model.hash(&mut hasher);
    lang.code().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Attach an analysis to a node, keeping any explanations already generated for it.
fn store_analysis(
    sessions: &SessionStore,
    id: &str,
    node_id: NodeId,
    mut analysis: PositionAnalysis,
) {
    sessions.update(id, |session| {
        if let Some(node) = session.tree.nodes.get_mut(node_id) {
            if let Some(previous) = node.analysis.take() {
                for (lang, text) in previous.explanations {
                    analysis.explanations.entry(lang).or_insert(text);
                }
            }
            node.analysis = Some(analysis);
        }
    });
}

fn store_explanation(
    sessions: &SessionStore,
    id: &str,
    node_id: NodeId,
    lang: Language,
    text: &str,
) {
    sessions.update(id, |session| {
        if let Some(analysis) = session
            .tree
            .nodes
            .get_mut(node_id)
            .and_then(|node| node.analysis.as_mut())
        {
            analysis
                .explanations
                .insert(lang.code().to_string(), text.to_string());
        }
    });
}

// ─── errors ─────────────────────────────────────────────

/// Every failure is an HTTP status plus `{"error": "..."}`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<pipeline::PipelineError> for ApiError {
    fn from(e: pipeline::PipelineError) -> ApiError {
        use pipeline::PipelineError;
        match e {
            // A newer analyze on the same engine superseded this search.
            e if e.is_cancelled() => ApiError::new(StatusCode::CONFLICT, "cancelled"),
            PipelineError::NodeNotFound(_) => node_not_found(),
            other => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
        }
    }
}

fn session_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "session not found")
}

fn node_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "node not found")
}

fn illegal_move() -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "illegal move")
}

fn not_analyzed() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "not analyzed")
}

fn engine_unavailable() -> ApiError {
    ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "engine unavailable")
}

fn llm_unavailable() -> ApiError {
    ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "llm provider unavailable")
}

/// `Json`, but rejections come back in our error shape instead of axum's.
#[derive(Debug)]
pub struct JsonBody<T>(pub T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(rejection) => Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                rejection.body_text(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// A router with no engine, no store, no book and no LLM. Everything that
    /// only touches sessions and the tree works; the rest answers 503.
    fn app() -> Router {
        crate::router(AppState::bare())
    }

    async fn request(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let builder = HttpRequest::builder().method(method).uri(uri);
        let request = match body {
            Some(value) => builder
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn new_session(app: &Router, body: Value) -> Value {
        let (status, value) = request(app, "POST", "/api/sessions", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }

    #[tokio::test]
    async fn health_is_truthful_about_a_missing_engine() {
        let (status, value) = request(&app(), "GET", "/api/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["engine"], Value::Null);
        assert!(value["stockfish_path"].is_string());
    }

    #[tokio::test]
    async fn languages_come_from_the_llm_crate() {
        let (status, value) = request(&app(), "GET", "/api/languages", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["default"], json!("en"));
        let codes: Vec<&str> = value["languages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["code"].as_str().unwrap())
            .collect();
        assert_eq!(codes, ["en", "ja"]);
        assert_eq!(value["languages"][1]["name"], json!("日本語"));
        assert_eq!(codes.len(), kibitz_llm::SUPPORTED_LANGUAGES.len());
    }

    #[tokio::test]
    async fn creates_a_session_from_a_pgn() {
        let app = app();
        let value = new_session(
            &app,
            json!({ "pgn": "[White \"A\"]\n[Black \"B\"]\n\n1. e4 e5 2. Nf3 1-0" }),
        )
        .await;
        assert!(value["session_id"].as_str().unwrap().len() == 26);
        assert_eq!(value["headers"]["White"], json!("A"));
        assert_eq!(value["tree"]["root"], json!(0));
        assert_eq!(value["tree"]["nodes"].as_array().unwrap().len(), 4);
        assert_eq!(value["tree"]["nodes"][1]["san"], json!("e4"));
        assert_eq!(value["tree"]["nodes"][0]["san"], Value::Null);
    }

    /// The wire shape from `docs/API.md`: every node carries `opening`, either an
    /// `{eco, name, matched_plies}` object or `null`.
    #[tokio::test]
    async fn nodes_carry_their_opening_name() {
        let app = app();
        let value = new_session(&app, json!({ "pgn": "1. e4 e5 2. Nf3 Nc6 3. Bb5 *" })).await;
        let nodes = value["tree"]["nodes"].as_array().unwrap();
        // The root has no move played, so it has no name.
        assert_eq!(nodes[0]["opening"], Value::Null);
        assert_eq!(nodes[1]["opening"]["eco"], json!("B00"));
        assert_eq!(nodes[1]["opening"]["matched_plies"], json!(1));
        assert_eq!(
            nodes[5]["opening"],
            json!({
                "eco": "C60",
                "name": "Ruy Lopez",
                "matched_plies": 5
            })
        );

        // And a node created by `play` is named the same way.
        let id = value["session_id"].as_str().unwrap().to_string();
        let (status, value) = request(
            &app,
            "POST",
            &format!("/api/sessions/{id}/play"),
            Some(json!({ "node_id": 5, "san": "Nf6" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let node_id = value["node_id"].as_u64().unwrap() as usize;
        assert_eq!(
            value["tree"]["nodes"][node_id]["opening"]["name"],
            json!("Ruy Lopez: Berlin Defense")
        );
        assert_eq!(
            value["tree"]["nodes"][node_id]["opening"]["matched_plies"],
            json!(6)
        );
    }

    #[tokio::test]
    async fn creates_a_session_from_a_fen_and_from_nothing() {
        let app = app();
        let fen = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";
        let value = new_session(&app, json!({ "fen": fen })).await;
        assert_eq!(value["tree"]["nodes"][0]["fen"], json!(fen));
        assert_eq!(value["headers"], json!({}));

        let (status, value) = request(&app, "POST", "/api/sessions", None).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert!(
            value["tree"]["nodes"][0]["fen"]
                .as_str()
                .unwrap()
                .starts_with("rnbqkbnr/pppppppp")
        );
    }

    #[tokio::test]
    async fn a_broken_pgn_is_a_400_in_our_error_shape() {
        let (status, value) = request(&app(), "POST", "/api/sessions", Some(json!({ "pgn": "1. e4 Qxd8" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(value["error"].as_str().unwrap().contains("invalid move"), "{value}");
    }

    #[tokio::test]
    async fn a_malformed_body_is_a_400_in_our_error_shape() {
        let app = app();
        let session = new_session(&app, json!({})).await;
        let id = session["session_id"].as_str().unwrap();
        let (status, value) = request(&app, "POST", &format!("/api/sessions/{id}/play"), Some(json!({ "nope": 1 }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(value["error"].is_string(), "{value}");
    }

    #[tokio::test]
    async fn fetches_a_session_and_404s_on_an_unknown_one() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 *" })).await;
        let id = session["session_id"].as_str().unwrap();

        let (status, value) = request(&app, "GET", &format!("/api/sessions/{id}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["session_id"], json!(id));

        let (status, value) = request(&app, "GET", "/api/sessions/nope", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], json!("session not found"));
    }

    #[tokio::test]
    async fn play_merges_an_existing_move_instead_of_duplicating_it() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 e5 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();
        let url = format!("/api/sessions/{id}/play");

        // e4 is already the first move of the game, so replaying it merges.
        let (status, value) = request(&app, "POST", &url, Some(json!({ "node_id": 0, "uci": "e2e4" }))).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["node_id"], json!(1));
        assert_eq!(value["created"], json!(false));
        assert_eq!(value["tree"]["nodes"].as_array().unwrap().len(), 3);

        // The same move in SAN merges the same way.
        let (_, value) = request(&app, "POST", &url, Some(json!({ "node_id": 0, "san": "e4" }))).await;
        assert_eq!(value["node_id"], json!(1));
        assert_eq!(value["created"], json!(false));

        // A different move branches.
        let (_, value) = request(&app, "POST", &url, Some(json!({ "node_id": 0, "san": "d4" }))).await;
        assert_eq!(value["created"], json!(true));
        assert_eq!(value["node_id"], json!(3));
        assert_eq!(value["tree"]["nodes"][0]["children"], json!([1, 3]));
        // The variation is a sibling, not a new mainline.
        assert_eq!(value["tree"]["nodes"][0]["children"][0], json!(1));
    }

    #[tokio::test]
    async fn play_rejects_illegal_and_unknown_input() {
        let app = app();
        let session = new_session(&app, json!({})).await;
        let id = session["session_id"].as_str().unwrap().to_string();
        let url = format!("/api/sessions/{id}/play");

        for body in [
            json!({ "node_id": 0, "uci": "e2e5" }),
            json!({ "node_id": 0, "san": "Qxd8" }),
            json!({ "node_id": 0, "uci": "not-a-move" }),
        ] {
            let (status, value) = request(&app, "POST", &url, Some(body.clone())).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body} -> {value}");
            assert_eq!(value["error"], json!("illegal move"), "{body}");
        }

        let (status, value) = request(&app, "POST", &url, Some(json!({ "node_id": 999, "san": "e4" }))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], json!("node not found"));

        let (status, _) = request(&app, "POST", "/api/sessions/nope/play", Some(json!({ "node_id": 0, "san": "e4" }))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn analysis_endpoints_are_503_without_an_engine() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 e5 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();

        for (uri, body) in [
            (format!("/api/sessions/{id}/analyze"), json!({ "node_id": 1 })),
            (format!("/api/sessions/{id}/analyze-game"), json!({ "depth": 12 })),
        ] {
            let (status, value) = request(&app, "POST", &uri, Some(body)).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
            assert_eq!(value["error"], json!("engine unavailable"));
        }
    }

    #[tokio::test]
    async fn analyze_checks_the_session_and_node_before_the_engine() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();

        let (status, value) = request(&app, "POST", &format!("/api/sessions/{id}/analyze"), Some(json!({ "node_id": 42 }))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], json!("node not found"));

        let (status, _) = request(&app, "POST", "/api/sessions/nope/analyze", Some(json!({ "node_id": 0 }))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn explain_rejects_an_unsupported_language_before_anything_else() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();

        let (status, value) = request(&app, "GET", &format!("/api/sessions/{id}/explain/1?lang=xx"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"], json!("unsupported language: xx"));

        // Even for a session that does not exist — the parameter is checked first.
        let (status, value) = request(&app, "GET", "/api/sessions/nope/explain/1?lang=klingon", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"], json!("unsupported language: klingon"));
    }

    #[tokio::test]
    async fn explain_accepts_every_supported_language_and_regional_tags() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();

        // Node 1 exists but has never been analysed: past the language check, the
        // next failure is 409.
        for lang in ["en", "ja", "ja-JP", "en_US", ""] {
            let (status, value) = request(&app, "GET", &format!("/api/sessions/{id}/explain/1?lang={lang}"), None).await;
            assert_eq!(status, StatusCode::CONFLICT, "lang={lang} -> {value}");
            assert_eq!(value["error"], json!("not analyzed"), "lang={lang}");
        }

        // No lang at all defaults to English.
        let (status, _) = request(&app, "GET", &format!("/api/sessions/{id}/explain/1"), None).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn explain_404s_on_an_unknown_node() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();
        let (status, value) = request(&app, "GET", &format!("/api/sessions/{id}/explain/99"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], json!("node not found"));
    }

    #[tokio::test]
    async fn ask_validates_its_language_too() {
        let app = app();
        let session = new_session(&app, json!({ "pgn": "1. e4 *" })).await;
        let id = session["session_id"].as_str().unwrap().to_string();
        let (status, value) = request(
            &app,
            "POST",
            &format!("/api/sessions/{id}/ask"),
            Some(json!({ "node_id": 1, "question": "why?", "lang": "xx" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"], json!("unsupported language: xx"));

        // With a valid language it gets as far as needing a provider.
        let (status, value) = request(
            &app,
            "POST",
            &format!("/api/sessions/{id}/ask"),
            Some(json!({ "node_id": 1, "question": "why?", "lang": "ja" })),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"], json!("llm provider unavailable"));
    }

    #[tokio::test]
    async fn unknown_routes_use_the_error_shape() {
        let (status, value) = request(&app(), "GET", "/api/nope", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"], json!("not found"));
    }

    #[tokio::test]
    async fn cors_allows_localhost_and_refuses_the_rest() {
        for (origin, allowed) in [
            ("http://localhost:5173", true),
            ("http://127.0.0.1:5173", true),
            ("http://evil.example.com", false),
        ] {
            let request = HttpRequest::builder()
                .method("GET")
                .uri("/api/health")
                .header("origin", origin)
                .body(Body::empty())
                .unwrap();
            let response = app().oneshot(request).await.unwrap();
            assert_eq!(
                response
                    .headers()
                    .contains_key("access-control-allow-origin"),
                allowed,
                "{origin}"
            );
        }
    }
}
