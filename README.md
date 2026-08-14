# kibitz

**Interactive chess analysis that explains why — Stockfish finds the refutation, an LLM narrates it.**

Load one of your own games, move a piece to try something different, and get an
explanation of what would have happened: how the opponent punishes it, which line
follows, what changes on the board. The refutation is replayed on the board while
the explanation streams in.

Runs entirely on your machine.

## Why another one

| | Existing tools | kibitz |
|---|---|---|
| Shape | Feed in a PGN, get a batch report | Branch from any position, analyse on the spot |
| Commentary | A classification, or canned template text | Actual reasoning, grounded in engine output |
| Counterfactual lines | Listed as text | **Replayed on the board** |

## The one design rule

**The LLM never reasons about the board.**

Language models cannot track piece positions. They will describe pieces that are
not there. So every fact comes from Stockfish and from deterministic code — the
classification, the refutation line, the tactical motifs, even a claim like
"this takes the centre" (that one is a count of attackers on d4/e4/d5/e5).

The model's only freedom is *which* of those facts to mention, in what order, and
in what words.

The useful side effect: when an explanation is wrong, you can tell where the bug
is. Wrong fact means the feature-extraction layer; correct facts described badly
means the prompt.

## Requirements

- Rust (edition 2024), Node 20+
- Stockfish — `brew install stockfish`, or set `KIBITZ_STOCKFISH`
- For explanations, either the `claude` CLI (default, runs inside an existing
  subscription at no extra cost) or `ANTHROPIC_API_KEY` with `KIBITZ_LLM=anthropic`

## Running

```sh
cargo build --release

./target/release/kibitz analyze game.pgn --depth 12 --explain --lang en
./target/release/kibitz serve --port 7777

# loopback by default — there is no auth, so opening it up is opt-in
./target/release/kibitz serve --bind 0.0.0.0 --port 7777
```

### Docker

Everything is built inside the image, with the Stockfish version, the Rust
version and both lockfiles pinned, so the engine and its analysis are the same on
every machine. The container defaults to `KIBITZ_LLM=anthropic` because the
`claude` CLI is not in the image; without a key you get everything except the
written explanations. See [`docs/DOCKER.md`](docs/DOCKER.md).

```sh
docker compose up -d          # http://127.0.0.1:7777
docker compose --profile test run --rm test
```

### Frontend

```sh
cd web
npm install
npm run dev              # proxies /api to 127.0.0.1:7777
VITE_MOCK=1 npm run dev  # fixture data, no backend needed
```

Board sounds ship in `web/public/sounds/`; overwrite them to use your own.

## Configuration

| Variable | Default | |
|---|---|---|
| `KIBITZ_STOCKFISH` | `stockfish` | Path to the engine binary |
| `KIBITZ_LLM` | `claude-code` | `claude-code` or `anthropic` |
| `KIBITZ_MODEL_NARRATE` | `claude-haiku-4-5` | Writes explanations |
| `KIBITZ_MODEL_REASON` | `claude-opus-5` | Strategy and follow-up questions |
| `KIBITZ_BOOK_DB` | `masters` | Opening Explorer database: `masters` or `lichess` |
| `KIBITZ_BOOK_URL` | `https://explorer.lichess.ovh` | Opening Explorer host |
| `KIBITZ_BIND` | `127.0.0.1` | Bind address (the Docker image sets `0.0.0.0`) |
| `KIBITZ_LOG` | `kibitz=info` | Log filter |

## Notes

**Move classification** started from Lichess (`lila`'s `Advice.scala`). Blunder
and Mistake still match it; Inaccuracy, Excellent and Great's "only move" gap were
recalibrated against 40 rated games between 1200–1600 players, because the
inherited numbers left `Good` covering 29% of a club player's moves. The
measurement behind each threshold is in
[`crates/core/src/classify.rs`](crates/core/src/classify.rs) and §8.3 of
[`docs/DESIGN.md`](docs/DESIGN.md).

**The Lichess Opening Explorer has returned `401` since 2026-02-23**
([lila#19610](https://github.com/lichess-org/lila/issues/19610)). kibitz stops
asking for an hour and falls back to the ECO table embedded in the binary, which
recognises theory only while a line is still being followed — typically the first
four to ten plies. The Explorer stays authoritative whenever it answers: its game
counts say how often a move was actually played, which an ECO table cannot.

**The Explorer is a free public API run by volunteers.** kibitz caches
aggressively, sends one request at a time, and stops querying the moment a game
leaves book. Please keep it that way.

Design in [`docs/DESIGN.md`](docs/DESIGN.md), HTTP contract in
[`docs/API.md`](docs/API.md).

## License

GPL-3.0-or-later — see [LICENSE](LICENSE). Copyright (C) 2026 ekkx.

Copyleft is not incidental: the board is
[chessground](https://github.com/lichess-org/chessground), which is GPL-3.0 and is
linked into the bundle this ships.
[chess.js](https://github.com/jhlywa/chess.js) is BSD-2-Clause, and the ECO data
in [`crates/book/data`](crates/book/data) carries its own notice.
