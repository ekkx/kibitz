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
| Where it runs | Cloud, paid | Local |

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

## Status

Early. See [`docs/DESIGN.md`](docs/DESIGN.md) for the full design and
[`docs/API.md`](docs/API.md) for the HTTP contract.

| Phase | | |
|---|---|---|
| 1 | `core` + `engine` + `book` — classification, features, motifs, opening book | in progress |
| 2 | `llm` — explanations, selectable model and language | in progress |
| 3 | `server` + `web` — board, variation tree, line replay, follow-up questions | in progress |
| 4 | Bulk import from Chess.com / Lichess | not started |

## Requirements

- Rust (edition 2024)
- Node 20+
- Stockfish — `brew install stockfish`, or set `KIBITZ_STOCKFISH=/path/to/stockfish`
- For explanations, one of:
  - the `claude` CLI, which runs inside an existing subscription at no extra cost (default)
  - `ANTHROPIC_API_KEY` with `KIBITZ_LLM=anthropic`

## Running

```sh
cargo build --release

# analyse a game on the command line
./target/release/kibitz analyze game.pgn --depth 20 --explain --lang en

# or start the server and open the board
./target/release/kibitz serve --port 7777

# it binds loopback by default — there is no auth, so opening it up is opt-in
./target/release/kibitz serve --bind 0.0.0.0 --port 7777
```

## Docker

If you would rather not install Rust, Node and Stockfish, everything is built
inside the image — with the Stockfish version, the Rust version and both
lockfiles pinned in the repo, so the engine and its analysis are the same on
every machine.

```sh
docker compose up -d          # http://127.0.0.1:7777
curl http://127.0.0.1:7777/api/health

docker compose --profile test run --rm test   # cargo test --workspace + npm test
```

The container defaults to `KIBITZ_LLM=anthropic`, because the `claude` CLI is
not in the image. Without `ANTHROPIC_API_KEY` you get everything except the
written explanations. See [`docs/DOCKER.md`](docs/DOCKER.md).

Frontend development:

```sh
cd web
npm install
npm run dev            # proxies /api to 127.0.0.1:7777
VITE_MOCK=1 npm run dev  # fixture data, no backend needed
```

Board sounds work out of the box — the five samples live in
`web/public/sounds/`. To use your own, overwrite them and reload; see
[`web/public/sounds/README.md`](web/public/sounds/README.md).

## Configuration

| Variable | Default | |
|---|---|---|
| `KIBITZ_STOCKFISH` | `stockfish` | Path to the engine binary |
| `KIBITZ_LLM` | `claude-code` | `claude-code` or `anthropic` |
| `KIBITZ_MODEL_NARRATE` | `claude-haiku-4-5` | Model used to write explanations |
| `KIBITZ_MODEL_REASON` | `claude-opus-5` | Model used for strategy and follow-up questions |
| `KIBITZ_BOOK_DB` | `masters` | Opening Explorer database: `masters` or `lichess` |
| `KIBITZ_BOOK_URL` | `https://explorer.lichess.ovh` | Opening Explorer host |
| `KIBITZ_BIND` | `127.0.0.1` | Bind address (the Docker image sets `0.0.0.0`) |
| `KIBITZ_LOG` | `kibitz=info` | Log filter |

## Built on

[shakmaty](https://github.com/niklasf/shakmaty) and
[chessground](https://github.com/lichess-org/chessground) by the Lichess authors,
[Stockfish](https://stockfishchess.org/), and the
[Lichess Opening Explorer](https://explorer.lichess.ovh) — a free public API run
by volunteers. kibitz caches aggressively, sends one request at a time, and stops
querying the moment a game leaves book; please keep it that way.

**The Opening Explorer is currently unavailable.** Every endpoint has returned
`401` since an outage that began on 2026-02-23
([lichess-org/lila#19610](https://github.com/lichess-org/lila/issues/19610), still
open). kibitz stops asking for an hour and falls back to the ECO table embedded in
the binary, which recognises a move as theory while the line it belongs to is still
being followed — typically the first four to ten plies of a club game. The Explorer
stays authoritative whenever it answers: its game counts say how often a move was
actually played, which an ECO table cannot. See §9.3 of
[docs/DESIGN.md](docs/DESIGN.md) for what the offline mark can and cannot claim.

Move classification thresholds started from Lichess (`lila`'s `Advice.scala`).
Blunder and Mistake still match it. Inaccuracy and Excellent no longer do:
they were recalibrated to -0.07 and -0.03 against 40 rated games between
1200-1600 players, because Lichess's -0.10 and -0.02 left `Good` covering 29% of
a club player's moves — everything from imperceptible to clearly bad under one
label. Great and Miss do not exist in Lichess at all; Great's "only move" gap was
recalibrated against the same games, from 0.10 to 0.30, after it turned out to
fire on 6.3% of all moves — five a game. All of it lives in
[`crates/core/src/classify.rs`](crates/core/src/classify.rs), with the
measurements behind each number; see §8.3 of [docs/DESIGN.md](docs/DESIGN.md).

## License

GPL-3.0-or-later. The full text is in [LICENSE](LICENSE).

    kibitz — interactive chess analysis that explains why
    Copyright (C) 2026 ekkx

    This program is free software: you can redistribute it and/or modify it
    under the terms of the GNU General Public License as published by the Free
    Software Foundation, either version 3 of the License, or (at your option)
    any later version.

    This program is distributed in the hope that it will be useful, but WITHOUT
    ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
    FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
    more details.

    You should have received a copy of the GNU General Public License along
    with this program. If not, see <https://www.gnu.org/licenses/>.

### Why GPL and not something permissive

The board is [chessground](https://github.com/lichess-org/chessground), which is
GPL-3.0-or-later, and it is linked into the frontend bundle that this project
ships. A distributed work that includes it has to be GPL-3.0 as well, so that is
what this is. The project was briefly labelled MIT; that was a mistake, and this
is the correction rather than a change of intent.

The alternative would have been to drop chessground for a permissively licensed
board. It was not worth it: chessground is what lichess itself uses, and copyleft
is a reasonable home for a tool whose whole argument is that you should be able to
see why it says what it says.

### Third-party licenses

| Component | License |
| --- | --- |
| [chessground](https://github.com/lichess-org/chessground) — board rendering, and the cburnett piece set embedded in it | GPL-3.0-or-later |
| [chess.js](https://github.com/jhlywa/chess.js) — client-side move legality | BSD-2-Clause |
| React, and the frontend toolchain | MIT |
| [Stockfish](https://stockfishchess.org/) — run as a separate process, never linked | GPL-3.0-or-later |
| ECO opening names in [`crates/book/data`](crates/book/data) | see [`crates/book/data/README.md`](crates/book/data/README.md) |
