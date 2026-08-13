# kibitz HTTP API — contract

The single contract between `crates/server` and `web/`. **Never change one side alone.**

- Base URL: `http://127.0.0.1:7777`
- `Content-Type: application/json` everywhere except SSE
- Errors are an HTTP status plus `{ "error": "..." }`
- No auth — local single-user tool

## Types

The serde representation of `crates/core/src/types.rs` *is* the JSON. The shapes
that matter:

```ts
type Score =
  | { kind: "cp";   value: number }
  | { kind: "mate"; value: number };   // positive: the side to move is mating

type Classification =
  | "book" | "great" | "best" | "excellent"
  | "good" | "inaccuracy" | "mistake" | "blunder" | "miss";

interface Candidate {
  san: string;
  uci: string;
  score: Score;
  win_prob: number;      // 0..1, from the side to move's point of view
  pv: string[];          // SAN
}

interface PlayedMove {
  san: string;
  uci: string;
  win_prob_before: number;
  win_prob_after: number;
  delta: number;          // negative means the move made things worse
  classification: Classification;
  accuracy: number;       // 0..100
}

interface Counterfactual {
  kind: "refutation" | "alternative_collapse";
  // Position the replay starts from. For "refutation" this is the position
  // after the played move; for "alternative_collapse" it is the position
  // *before* it, since the line begins with a different move.
  start_fen: string;
  pv: string[];           // the line to animate on the board (SAN)
  motifs: Motif[];
}

type Motif =
  | { kind: "fork"; from: string; attacker: string; targets: string[] }
  | { kind: "pin"; attacker: string; pinned: string; behind: string }
  | { kind: "skewer"; attacker: string; front: string; behind: string }
  | { kind: "discovered_attack"; moved_from: string; revealed: string; targets: string[] }
  | { kind: "hanging"; square: string; role: string; see: number }
  | { kind: "back_rank"; attacker: string; king: string };

interface PositionAnalysis {
  fen: string;
  depth: number;
  candidates: Candidate[];                // best first; up to 5, fewer near mate
  context: AnalysisContext | null;        // null at the root
  explanations: Record<string, string>;   // keyed by language code
}

interface OpeningInfo {
  eco: string;            // "C65"
  name: string;           // "Ruy Lopez: Berlin Defense"
  matched_plies: number;  // plies from the root to this node
}

interface Node {
  id: number;
  parent: number | null;
  children: number[];     // [0] is the mainline
  san: string | null;     // null at the root
  uci: string | null;
  fen: string;
  analysis: PositionAnalysis | null;
  // The named opening this position belongs to, from an ECO table embedded in
  // the binary. Offline, always present, and **display only** — it never makes
  // a move `Classification::Book`. Deciding whether a move is still theory needs
  // the Opening Explorer's game counts, which an ECO table does not carry.
  opening: OpeningInfo | null;
}

interface GameTree { nodes: Node[]; root: number }
```

`Features`, `StaticDiff`, `StrategicOutlook` and `AnalysisContext` follow their
definitions in `crates/core/src/types.rs`, `feature.rs` and `outlook.rs`.

## Explanation language

The user picks the explanation language in the UI. It is a request parameter, not
a server setting — two clients may ask for different languages against the same
session, and both results are cached side by side under
`PositionAnalysis.explanations`.

### `GET /api/languages`

```jsonc
{
  "languages": [
    { "code": "en", "name": "English" },
    { "code": "ja", "name": "日本語" }
  ],
  "default": "en"
}
```

The frontend populates its picker from this. An unknown `lang` on any endpoint is
`400 { "error": "unsupported language: xx" }`.

## Endpoints

### `POST /api/sessions`

Create a session from a game or a position.

```jsonc
// body — one of
{ "pgn": "1. e4 e5 ..." }
{ "fen": "rnbq..." }   // omitted means the starting position
```

```jsonc
// 200
{
  "session_id": "01J...",
  "tree": { "nodes": [...], "root": 0 },
  "headers": { "White": "...", "Black": "...", "Result": "1-0" }  // from PGN, {} otherwise
}
```

### `GET /api/sessions/{id}`

```jsonc
{ "session_id": "...", "tree": {...}, "headers": {...} }
```

### `POST /api/sessions/{id}/play`

Play a move on the board. An existing child with the same move is returned as-is
(automatic merge).

```jsonc
// body — san or uci
{ "node_id": 12, "uci": "g1f3" }
{ "node_id": 12, "san": "Nf3" }
```

```jsonc
// 200
{ "node_id": 13, "created": true, "tree": {...} }
```

An illegal move is `400 { "error": "illegal move" }`.

### `POST /api/sessions/{id}/analyze`

Analyse one node. A newer `analyze` cancels the running one; the older request
then returns `409 { "error": "cancelled" }`. This is "latest only", and it is
what makes rapidly trying moves on the board feel immediate.

`depth` is optional and defaults to 12.

`candidates` is ordered best first and holds **up to 5** moves (fewer only when
the position has fewer legal moves). The width is fixed server-side and is not a
request parameter: it is part of the analysis cache key, so making it negotiable
would silently re-analyse an already-analysed game whenever a client changed it.
A client that draws fewer arrows should take the first *n* candidates.

**A running `analyze-game` is not affected.** The server arbitrates the one
engine process, so an `analyze` issued during a sweep waits for the sweep's
current position rather than cancelling the run — and a node the sweep has
already reached is served from cache without waiting at all. Cancellation now
only ever happens between two `analyze` requests; clients do not need to
serialise engine requests themselves.

```jsonc
// body
{ "node_id": 13, "depth": 12 }   // depth optional, defaults to 12
```

```jsonc
// 200 — PositionAnalysis
{ "fen": "...", "depth": 12, "candidates": [...], "context": {...} | null, "explanations": {} }
```

### `POST /api/sessions/{id}/analyze-game` (SSE)

Analyse the mainline from the start. `text/event-stream`. The `node` payload is a `PositionAnalysis` with an extra
`node_id` — pairing it with the preceding `progress` event would break on a
repeated position, and `fen` does not identify a node either.

Events are emitted **as each position finishes**: one `progress` and one `node`
per position, then a single `done`. Positions the opening book accepts, and
terminal positions, are answered without the engine and so arrive immediately.

The sweep is never cancelled by an `analyze` request; an `error` event means the
run genuinely failed. `depth` is optional and defaults to 12.

```jsonc
// body
{ "depth": 12 }
```

```
event: progress
data: {"node_id":5,"done":5,"total":42}

event: node
data: { ...PositionAnalysis..., "node_id": 5 }

event: done
data: {"total":42}

event: error
data: {"error":"..."}
```

### `GET /api/sessions/{id}/explain/{node_id}?lang=en` (SSE)

Stream the LLM explanation. A cached explanation for that language arrives as a
single event.

```
event: delta
data: {"text":"This move "}

event: done
data: {"text":"(full text)","lang":"en","model":"claude-haiku-4-5","cached":false}

event: error
data: {"error":"..."}
```

`lang` defaults to `en`. A node that has not been analysed yet is
`409 { "error": "not analyzed" }`.

### `POST /api/sessions/{id}/ask` (SSE, Phase 3)

Follow-up question, answered by an agent with `AnalysisTools` exposed as tools.

```jsonc
// body
{ "node_id": 13, "question": "What happens after Bxf7+?", "lang": "en" }
```

Same events as `explain`, plus:

```
event: tool
data: {"name":"analyze_move","input":{"fen":"...","san":"Bxf7+"}}
```

**Not implemented yet.** The endpoint answers on the plain conversational path
(`delta` / `done` / `error`); `tool` events arrive once `AnalysisTools` is exposed
as an MCP server in Phase 3. Until then the model is instructed to say when a
question cannot be answered from the JSON it was given, rather than guessing.

### `GET /api/health`

```jsonc
{ "ok": true, "engine": "Stockfish 18", "stockfish_path": "/opt/homebrew/bin/stockfish" }
```

`"engine": null` when the engine failed to start. **The frontend calls this on
startup and tells the user if the engine is missing.**

When it did fail, an extra `error` field carries the reason, so the message shown
to the user can be specific. The field is absent when `ok` is `true`.

```jsonc
{
  "ok": false,
  "engine": null,
  "stockfish_path": "stockfish",
  "error": "cannot start stockfish: No such file or directory (os error 2)"
}
```

`engine` is the `id name` line from a real UCI handshake, never a constant.

## Flow the frontend is expected to use

1. `GET /api/health`, `GET /api/languages`
2. Paste a PGN → `POST /api/sessions`
3. `POST /api/sessions/{id}/analyze-game` (SSE) to sweep the mainline, showing progress
4. User moves a piece → `POST .../play` → `POST .../analyze`
5. If the classification warrants it → `GET .../explain/{node_id}?lang=<selected>`
6. Animate `counterfactual.pv` on the board
