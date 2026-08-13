# kibitz — web

The frontend: a board you can branch from, a variation tree, and an explanation
that arrives while the engine's line replays itself on the board.

## Running it

Requires Node 24.

```sh
npm install
```

### Against fixture data (no backend needed)

```sh
npm run dev:mock          # or: VITE_MOCK=1 npm run dev
```

Open http://localhost:5273 and press **Use the sample game → Open**.

Mock mode answers every request in-process from `src/mock/`, so the whole app
works with no server and no engine: the Opera Game with a scripted inaccuracy,
a blunder, two piece sacrifices and a forced mate, candidate moves for
every position, counterfactual lines, and explanations (English and Japanese)
streamed in pieces.

The mock is a **fetch-level** stand-in: it returns real `Response` objects, and
its SSE responses are byte streams chopped at sizes that deliberately fall in
the middle of events. Mock mode therefore exercises the same client, the same
stream reader and the same rendering path as the real server — only the origin
of the bytes differs.

### Against the real server

```sh
cargo run          # in the repo root: the API on 127.0.0.1:7777
npm run dev        # here
```

The dev server proxies `/api` to `http://127.0.0.1:7777` (see `vite.config.ts`).
If the server is not up, or it is up but Stockfish failed to start, the app says
so in words instead of failing silently — that is what `GET /api/health` is for.

### Checks

```sh
npm run typecheck   # tsc --noEmit
npm test            # vitest: SSE framing, fixture legality, mock endpoints
npm run build       # typecheck + production build
```

## How it is put together

```
src/
  api/         the contract with the server: types.ts mirrors docs/API.md,
               sse.ts is the stream reader, client.ts the typed calls
  mock/        the in-process server for VITE_MOCK=1 (fixtures + a toy engine)
  chess/       the only place chess rules run in the browser: legal dests,
               SAN → from/to for the replay, check and turn detection
  state/       one hook per concern: session, explanation, sweep, health,
               language, persisted preferences
  hooks/       useReplay — the clock behind the counterfactual animation
  components/  board, eval bar, move tree, analysis panel, import, sweep
  i18n/        UI strings (English and Japanese)
  ui/format.ts score / glyph / move-number formatting shared by components
```

### Language: one setting, two mechanisms

The user picks one language (`state/useLanguage.ts`, stored as
`kibitz.language`). English and Japanese, being the languages that are both in
`GET /api/languages` and in `src/i18n`. It does two different things:

- **The interface.** `t()` reads the catalogue for that language. No component
  contains a literal string, so a locale is a catalogue and nothing else — see
  `src/i18n/ja.ts`, whose terminology follows the Japanese glossary in
  `crates/llm/src/prompt.rs` so the panel and the explanation call a fork a
  フォーク and a blunder a 大悪手.
- **The explanations.** The same value is sent as the `lang` parameter on
  `/explain` and `/ask`. Per API.md it is a request parameter, so switching it
  re-requests the current explanation and leaves the session and the analysis
  alone.

The two stay separate in the code — one is a catalogue lookup, the other a
request parameter — but they are never set apart from each other. Text the
server owns (opening names, SAN, ECO codes, engine names, the explanation body)
is rendered as it arrives and never routed through `t()`.

### SSE

`EventSource` only does GET, and `/analyze-game` and `/ask` are POSTs, so
`src/api/sse.ts` reads the body itself. Network chunks have nothing to do with
event boundaries — a `data:` line can arrive in three pieces, the blank line
that ends an event can land in the next chunk, and a multi-byte character can
be split down the middle. `SseParser` buffers across pushes and decodes with
`TextDecoder({stream:true})`; `src/api/sse.test.ts` replays a stream split at
**every** character position and again as 3-byte chunks, and asserts identical
output each time.

### The replay (DESIGN §13)

When a move is classified, its `Counterfactual.pv` is expanded into per-move
positions and played on the board one move at a time (`src/hooks/useReplay.ts`),
while the explanation streams into the panel beside it. A refutation starts from
the position the move created; an alternative collapse starts from the position
before it, since the point is to play a different move instead. When the line
ends, any tactical motifs are drawn on the squares where they happen, and after
a beat the board returns to the game position. The replay is always started by
hand, with **Replay line**: selecting a move draws the engine's arrows and
streams the explanation, and leaves the board where the user put it.

## Known limitations

- **Promotion is always to a queen.** A promotion picker is not implemented yet;
  a pawn dragged to the last rank becomes a queen (`chess/rules.ts`).
- The sweep summary is computed in the browser from the `PositionAnalysis`
  events, since the API has no summary endpoint.
- Follow-up questions (`/ask`) are wired up but the backend endpoint is Phase 3.

## Notes on the API contract

Points where `docs/API.md` left the frontend to make a decision. None of these
are divergences — they are readings that the server should confirm.

1. **`analyze-game`'s `node` event carries no node id.** Its payload is a bare
   `PositionAnalysis`, which has a `fen` but no id, while `progress` has the id.
   The client pairs each `node` with the `progress` immediately before it. A
   `node_id` field on the `node` event would remove the guess — the same
   position can repeat in a game, so matching on `fen` is not sound.
2. **`candidates` appears at two levels.** `PositionAnalysis.candidates` is read
   as the MultiPV of *this* position (what to play now), and
   `AnalysisContext.candidates` as the MultiPV of the *parent* position, which
   is what `played_rank` must index into. That is the only reading in which
   `played_rank` means anything, but API.md does not say so.
3. **`AnalysisContext` is not specified in API.md**, which points at
   `crates/core/src/types.rs`. It is typed here from DESIGN §8 with everything
   the UI does not strictly need marked optional, so extra or renamed fields
   cannot break rendering. The fields the UI does rely on are `played`,
   `played_rank` and `counterfactual`.
4. **Terminal positions are undefined.** What `/analyze` returns for a mated
   position — presumably `candidates: []` — is not stated, so the eval bar
   detects mate locally and shows `#` rather than an empty bar.
5. **`Counterfactual.start_fen` is not always the node's own position.** For
   `alternative_collapse` the line has to start from the position *before* the
   played move. The field carries this correctly; it is worth stating that
   `start_fen` may be the parent's FEN so both sides agree.
6. Minor: `Classification` includes `miss`, but the glyph list in the UI spec
   does not cover it. It is rendered as `×`.
