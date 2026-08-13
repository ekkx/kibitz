# Running kibitz in Docker

Everything kibitz needs — Rust, Node, Stockfish — is built inside the image, so
Docker is the only thing you have to install. Every version is pinned in the
repo, so a fresh clone produces the same engine and the same analysis on any
machine.

## Run it

```sh
docker compose up -d
open http://127.0.0.1:7777
```

Check the engine came up:

```sh
curl http://127.0.0.1:7777/api/health
# {"ok":true,"engine":"Stockfish 18","stockfish_path":"/usr/local/bin/stockfish"}
```

`engine` is the `id name` line from a real UCI handshake, so `"ok":true` means
Stockfish actually started. The compose healthcheck asserts exactly that, which
is why `docker compose ps` shows `(healthy)` rather than merely `Up`.

Stop with `docker compose down`. The cache survives (see [Data](#data));
`docker compose down -v` deletes it.

If port 7777 is taken on your machine:

```sh
KIBITZ_HOST_PORT=7788 docker compose up -d
```

The published port is bound to `127.0.0.1` only. kibitz has no authentication.

### The CLI

The runtime image has no PGNs in it, so mount the directory your games are in.
The service is called `kibitz` and so is the binary, hence the repetition:

```sh
docker compose run --rm -v "$PWD/testdata:/games:ro" \
    --entrypoint kibitz kibitz analyze /games/opera_game.pgn --depth 20
```

`--explain` additionally needs an `ANTHROPIC_API_KEY`; see below. `run` attaches
the same cache volume the server uses, so the two share their analyses.

## Run the tests

```sh
docker compose --profile test run --rm test
```

The test image sits behind a compose profile, so `docker compose up` and
`docker compose build` ignore it; the command above builds it on first use.

That runs `cargo test --workspace --locked` and then the frontend's `npm test`
(vitest). The test image carries the Rust toolchain, Node, npm and Stockfish, so
the engine integration tests in `crates/engine/tests/stockfish.rs` actually run
instead of skipping themselves.

One half at a time:

```sh
docker compose --profile test run --rm test cargo test --workspace --locked
docker compose --profile test run --rm test sh -c 'cd web && npm test'
```

## Explanations (the LLM)

kibitz's default provider shells out to the `claude` CLI, which runs on your
existing subscription. That CLI is not in the image and would have no
credentials there, so **the container defaults to `KIBITZ_LLM=anthropic`.**

What that means in practice:

| | You get | You do not get |
|---|---|---|
| No `ANTHROPIC_API_KEY` (default) | Engine analysis, move classification, features, motifs, counterfactual lines, the board, the variation tree, the SQLite cache | Written explanations. `GET /api/…/explain/…` and `POST /api/…/ask` answer `503 {"error":"llm provider unavailable"}` and the log says `no llm provider configured: missing ANTHROPIC_API_KEY` |
| `ANTHROPIC_API_KEY` set | All of the above plus written explanations, billed to your API account | — |

To turn explanations on, put the key in a `.env` file next to
`docker-compose.yml`:

```sh
echo 'ANTHROPIC_API_KEY=sk-ant-...' >> .env
docker compose up -d
```

### Why the free path cannot be containerised

This is worth stating precisely, because "mount `~/.claude` into the container"
is the obvious idea and it does not work. Two independent reasons, both checked
rather than assumed:

- **The `claude` binary is native.** On this machine `file $(which claude)` says
  `Mach-O 64-bit executable arm64` — a macOS executable. It cannot run in a Linux
  container no matter what you mount.
- **The credentials are not in `~/.claude`.** On macOS they live in the Keychain;
  there is no `~/.claude/.credentials.json` to bind-mount. Mounting the directory
  carries settings and history, but not authentication.

So the subscription-backed, zero-cost path is a host-only path:

```sh
cargo build --release
./target/release/kibitz serve --port 7777    # explanations work, nothing to pay
docker compose up -d                          # everything except explanations
```

The container's job is getting a working engine and toolchain without installing
Rust, Node or Stockfish. Explanations are the one thing it cannot give you for
free.

Nothing else degrades. Analysis and classification never touch the LLM — see
`docs/DESIGN.md` §2.

### The opening book

The log line

```
opening explorer unavailable (status 401); pausing book lookups for 3600s,
analysis continues without the book
```

is expected and is **not** caused by the container. The Lichess Opening Explorer
has answered `401` to everyone since 2026-02-23
([lichess-org/lila#19610](https://github.com/lichess-org/lila/issues/19610)).
kibitz treats it as "no book" and analyses the game normally. You would see the
same thing running on the host.

## Data

The SQLite cache — analyses, explanations and book responses — lives at
`~/.kibitz/kibitz.db` inside the container, which compose backs with the named
volume `kibitz_kibitz-cache`. It survives `restart`, `down` and `up`, so a game
you have analysed once is answered from cache in milliseconds instead of being
re-searched.

```sh
docker volume inspect kibitz_kibitz-cache   # where it is
docker compose down -v                      # throw it away
```

The container runs as uid 10001, which owns the volume.

## Configuration

Every variable from `README.md` is passed through from your shell or from
`.env`. The defaults the container ships with:

| Variable | Container default |
|---|---|
| `KIBITZ_STOCKFISH` | `/usr/local/bin/stockfish` (the pinned build) |
| `KIBITZ_LLM` | `anthropic` (`claude-code` cannot work in the image) |
| `ANTHROPIC_API_KEY` | unset |
| `KIBITZ_MODEL_NARRATE` | `claude-haiku-4-5` |
| `KIBITZ_MODEL_REASON` | `claude-opus-5` |
| `KIBITZ_BOOK_DB` | `masters` |
| `KIBITZ_BOOK_URL` | `https://explorer.lichess.ovh` |
| `KIBITZ_LOG` | `kibitz=info` |
| `KIBITZ_HOST_PORT` | `7777` (host side of the published port) |

## What is pinned, and where

| | Version | Pinned in |
|---|---|---|
| Rust | 1.95.0 | `rust-toolchain.toml` and the `RUST_VERSION` / `RUST_IMAGE` args in `Dockerfile` |
| Node | 24.10.0 | `NODE_VERSION` / `NODE_IMAGE` in `Dockerfile` |
| Stockfish | 18, commit `cb3d4ee9` | `STOCKFISH_COMMIT` in `Dockerfile` |
| Rust crates | — | `Cargo.lock`, enforced by `--locked` |
| npm packages | — | `web/package-lock.json`, enforced by `npm ci` |
| Base images | — | tag **and** digest in `Dockerfile` |

`rust-toolchain.toml` also applies on the host, so a host `cargo build` and a
container build use the same compiler.

### Updating Stockfish

Stockfish is built from source rather than installed from a distro package. The
reason is **engine strength, not reproducibility of output**: a package gives you
whatever version that distro shipped — Debian bookworm has 15.1, trixie 16.1 —
and this tool's whole value is the quality of the analysis, so shipping a
three-year-old engine because it happens to be in `apt` is a real downgrade.

To be clear about what pinning does and does not buy: a newer Stockfish returning
different evaluations is **not a problem**. The UCI protocol is what kibitz
depends on, and it is stable. Nobody needs bit-identical centipawn numbers across
machines. What *would* be a problem is one installation mixing results from two
engines — and that is handled where it belongs, by including the engine's `id
name` in the analysis cache key, not by freezing the engine.

Given that, a commit pin is simply how you say "this exact engine" for a
dependency built from source. The NNUE weight files are `#define`d by name in
that commit's `src/evaluate.h`, and `scripts/net.sh` checks each download against
the sha256 that forms its own filename, so a commit can only ever produce one
engine. Bumping it is expected, not exceptional.

To move to a new release:

1. Find the commit the tag points at:

   ```sh
   git ls-remote https://github.com/official-stockfish/Stockfish refs/tags/sf_19
   ```

   Use the commit, not the tag — a tag can be repointed.

2. Edit `STOCKFISH_VERSION` and `STOCKFISH_COMMIT` in `Dockerfile`.

3. Rebuild and confirm:

   ```sh
   docker compose build
   docker compose up -d
   curl -s http://127.0.0.1:7777/api/health
   ```

   The image also records it:

   ```sh
   docker compose exec kibitz cat /usr/local/share/stockfish-version
   # id name Stockfish 18
   ```

`ARCH` is chosen for portability, not speed (`armv8` on arm64,
`x86-64-sse41-popcnt` on amd64) so the binary runs on any CPU of that family.
Stockfish's search is integer-only and returns identical results on every
`ARCH`, and kibitz searches to a fixed depth rather than a fixed time, so this
affects how long a search takes and nothing about what it finds.

### Updating a base image

```sh
docker buildx imagetools inspect rust:1.95.0-bookworm   # prints the index digest
```

Put the tag and the digest back into `Dockerfile` together. The digest is what
makes the pin real; the tag is there so a human can read it.

## What is not reproducible

- **Engine thread count.** kibitz sets Stockfish's `Threads` from
  `available_parallelism()`, and Stockfish's search is not deterministic with
  more than one thread — two machines with different core counts can return
  different principal variations at the same depth. Rust reads the cgroup quota,
  so pinning it pins the search:

  ```yaml
  # docker-compose.yml, under services.kibitz
  cpus: 1.0
  ```

  Left alone, the container uses the host's cores; the classification of a move
  is stable but the exact centipawn numbers may move by a point or two. This is
  usually fine — see the note under "Updating Stockfish" on why identical numbers
  are not a goal.

- **The two `apt-get install` lines** (`ca-certificates`, `curl` at
  runtime; `build-essential`, `git` at build time). Debian's archive is mutable,
  so these track bookworm point releases. None of them touch analysis output.
  The digest-pinned base image fixes everything else about the filesystem.

- **LLM output.** Explanations are model output and are not deterministic. They
  are cached per position and language, so a given explanation is stable once
  written.

## How the image is put together

Seven stages, in `Dockerfile`:

```
web-deps ──> web-build ──┐
stockfish ───────────────┼──> runtime   (371 MB on disk, 115 MB pulled)
rust-deps ─> rust-build ─┴──> test      (3.8 GB on disk — toolchain and caches)
```

The runtime image is a Debian slim base (108 MB), the Stockfish binary
(113 MB — Stockfish 18 embeds its NNUE weights, which is also why no `.nnue`
file has to be shipped alongside it), the kibitz binary (16 MB), `ca-certificates`
/ `curl` (18 MB) and `web/dist` (0.3 MB). No compiler, no Node, no
`apt` build tooling.

That `web/dist` figure includes the board's five sound samples, which are part of
the image: `web/public/sounds/` is copied into the build verbatim, so the
container has working board sounds with nothing further to mount or configure.
Replacing them means replacing the files on the host and rebuilding — see
[`web/public/sounds/README.md`](../web/public/sounds/README.md).

On a 10-core arm64 machine with nothing cached — no build cache, no base images
pulled — `docker compose build` takes about 70 s, and `--profile test` adds
about 20 s on top. A change to a Rust source file rebuilds in about 7 s: only
the seven workspace crates recompile, no third-party crate is downloaded or
rebuilt, and the Stockfish and web stages are not re-entered.

`rust-deps` copies only `Cargo.toml`, `Cargo.lock` and the per-crate manifests,
gives every workspace crate an empty stub source, and compiles the third-party
dependencies there. That layer's cache key is the manifests alone, so editing
Rust code reuses it and neither re-downloads nor recompiles the dependency tree.
`web-deps` does the same for `npm ci`.

### A note on the bind address

`kibitz serve` defaults to `127.0.0.1`. That is deliberate: it is a local
single-user tool with no authentication, so it should not be reachable from the
network unless someone asks for it. A published container port cannot reach
loopback inside the container, so the image passes `--bind 0.0.0.0` and lets the
container's port publishing be the boundary instead. Override with
`KIBITZ_BIND`.
