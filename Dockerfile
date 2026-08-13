# kibitz — reproducible container build.
#
# Every version that can drift is pinned here or in a lockfile that lives in the
# repo:
#
#   Rust      RUST_VERSION, kept equal to rust-toolchain.toml
#   Node      NODE_VERSION
#   Stockfish STOCKFISH_COMMIT (a commit, not a tag — tags can be moved)
#   crates    Cargo.lock, enforced with --locked
#   npm       package-lock.json, enforced with `npm ci`
#   base imgs tag + digest, so the same bytes are pulled next year
#
# Stages:
#   web-deps       npm ci
#   web-build      web/dist
#   stockfish      Stockfish built from a pinned source commit
#   rust-deps      third-party crates compiled against stub sources
#   rust-build     the kibitz binary
#   test           everything above plus cargo and npm, for the test suite
#   runtime        binary + web/dist + stockfish, no toolchain — the last
#                  stage, so a plain `docker build .` produces this one
#
# Build a specific stage with `--target`, e.g.
#   docker build --target test -t kibitz-test .

# ─── pinned versions ────────────────────────────────────────────────────────
#
# Digests are the multi-platform index digests, so the same reference works on
# amd64 and arm64. Refresh with:
#   docker buildx imagetools inspect <image>:<tag>

# The version literal appears once; the tag is built from it, and the digest is
# what actually fixes the bytes. Change one, change the other.
ARG RUST_VERSION=1.95.0
ARG NODE_VERSION=24.10.0

ARG RUST_IMAGE=rust:${RUST_VERSION}-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1
ARG NODE_IMAGE=node:${NODE_VERSION}-bookworm-slim@sha256:b8d2197aff9129d16c801a3e3e1b2a873c4946480f5a310f38056df2268c38d9
ARG DEBIAN_IMAGE=debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241

# Stockfish 18 (tag sf_18). The NNUE file names are `#define`d in this commit's
# src/evaluate.h and scripts/net.sh checks each download against the sha256 in
# its own name, so the weights are content-addressed: this commit can only ever
# produce one engine.
ARG STOCKFISH_VERSION=18
ARG STOCKFISH_COMMIT=cb3d4ee9b47d0c5aae855b12379378ea1439675c


# ─── web-deps: node_modules from the lockfile ───────────────────────────────
#
# Its own stage so that editing a .tsx file does not reinstall npm packages, and
# so the test stage can reuse the same node_modules.

FROM ${NODE_IMAGE} AS web-deps
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci


# ─── web-build: web/dist ────────────────────────────────────────────────────

FROM web-deps AS web-build
COPY web/ ./
# `npm run build` is `tsc --noEmit && vite build`, so a type error fails the
# image build rather than shipping.
RUN npm run build


# ─── stockfish: built from a pinned commit ──────────────────────────────────
#
# Why not `apt-get install stockfish`: the version would be whatever the distro
# happens to ship on the day the image is built (bookworm has 15.1, trixie has
# 16.1, and neither is what this project is developed against). The engine
# version changes analysis output, so it has to be pinned like any other
# dependency.
#
# ARCH is chosen for portability, not speed: `armv8` and `x86-64-sse41-popcnt`
# run on any 64-bit ARM / x86 CPU. Stockfish's search is integer-only and gives
# identical results on every ARCH, so this costs nodes per second and nothing
# else — and kibitz searches to a fixed depth (`go depth N`), not a fixed time.

FROM ${DEBIAN_IMAGE} AS stockfish
ARG STOCKFISH_COMMIT
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
RUN git init -q . \
    && git remote add origin https://github.com/official-stockfish/Stockfish.git \
    && git fetch -q --depth 1 origin "${STOCKFISH_COMMIT}" \
    && git checkout -q FETCH_HEAD
RUN case "${TARGETARCH}" in \
        arm64) SF_ARCH=armv8 ;; \
        amd64) SF_ARCH=x86-64-sse41-popcnt ;; \
        *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && make -C src -j"$(nproc)" build ARCH="${SF_ARCH}" \
    && strip src/stockfish \
    && install -m 0755 src/stockfish /usr/local/bin/stockfish
# Record what was built, so the image can be asked which engine it carries.
# `uci` prints the same `id name` line that kibitz reports through /api/health.
RUN printf 'uci\nquit\n' | /usr/local/bin/stockfish | grep '^id name' > /usr/local/share/stockfish-version \
    && cat /usr/local/share/stockfish-version


# ─── rust-deps: third-party crates, compiled once ───────────────────────────
#
# Only the manifests and the lockfile are copied here, and every workspace crate
# gets an empty stub source. The layer therefore depends on Cargo.toml and
# Cargo.lock alone: editing Rust code reuses this layer, and the ~250 crates in
# Cargo.lock are neither re-downloaded nor recompiled.

FROM ${RUST_IMAGE} AS rust-deps
ARG RUST_VERSION
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
# "host and container agree on the compiler" is worth failing the build over
# rather than trusting a comment.
RUN set -eu; \
    if ! grep -qx "channel = \"${RUST_VERSION}\"" rust-toolchain.toml; then \
        echo "rust-toolchain.toml does not pin ${RUST_VERSION}" >&2; exit 1; \
    fi; \
    if ! rustc --version | grep -q " ${RUST_VERSION} "; then \
        echo "image rustc is not ${RUST_VERSION}: $(rustc --version)" >&2; exit 1; \
    fi; \
    rustc --version
COPY crates/core/Cargo.toml   crates/core/
COPY crates/engine/Cargo.toml crates/engine/
COPY crates/book/Cargo.toml   crates/book/
COPY crates/llm/Cargo.toml    crates/llm/
COPY crates/store/Cargo.toml  crates/store/
COPY crates/server/Cargo.toml crates/server/
COPY cmd/Cargo.toml           cmd/
# `cargo fetch` is separate from `cargo build` on purpose: it pulls
# dev-dependencies as well, which `cargo build` does not, so the test stage
# never has to reach the network.
RUN set -eux; \
    for crate in core engine book llm store server; do \
        mkdir -p "crates/${crate}/src"; \
        : > "crates/${crate}/src/lib.rs"; \
    done; \
    mkdir -p cmd/src; \
    echo 'fn main() {}' > cmd/src/main.rs; \
    cargo fetch --locked; \
    cargo build --release --locked; \
    rm -rf crates/*/src cmd/src


# ─── rust-build: the kibitz binary ──────────────────────────────────────────

FROM rust-deps AS rust-build
COPY crates/ crates/
COPY cmd/ cmd/
# Cargo decides what to rebuild from mtimes, and COPY preserves the mtimes of
# the files in the build context — which are older than the stub build above.
# Without this, cargo would consider the stub artefacts up to date.
RUN find crates cmd -name '*.rs' -exec touch {} + \
    && cargo build --release --locked \
    && install -m 0755 target/release/kibitz /usr/local/bin/kibitz


# ─── test: the full suite ───────────────────────────────────────────────────
#
#   docker compose --profile test run --rm test
#
# Built on rust-build so the crate registry and the compiled dependencies are
# already there. Node and npm are copied out of the pinned Node image rather
# than installed from a distro or a tarball — same Debian release, so the
# binaries just work, and the version stays tied to one digest.

FROM rust-build AS test
COPY --from=stockfish /usr/local/bin/stockfish /usr/local/bin/stockfish
COPY --from=web-deps /usr/local/bin/node /usr/local/bin/node
COPY --from=web-deps /usr/local/lib/node_modules /usr/local/lib/node_modules
COPY --from=web-deps /app/web/node_modules /app/web/node_modules
# Recreated rather than copied: COPY dereferences a symlink, and npm-cli.js
# resolves `../lib/cli.js` relative to wherever it is loaded from.
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && node --version && npm --version
COPY web/ /app/web/
COPY testdata/ /app/testdata/

ENV KIBITZ_STOCKFISH=/usr/local/bin/stockfish \
    CI=1

WORKDIR /app
# `cargo test` needs the dev-dependencies, which the rust-deps stage fetched but
# did not build. --locked keeps Cargo.lock authoritative here too.
CMD ["sh", "-c", "cargo test --workspace --locked && cd web && npm test"]


# ─── runtime ────────────────────────────────────────────────────────────────
#
# No Rust, no Node, no build tools: the binary, the built frontend, Stockfish.
#
# ca-certificates  rustls reads the system trust store (Opening Explorer, the
#                  Anthropic API)
# curl             the healthcheck, which has to run inside the container

FROM ${DEBIAN_IMAGE} AS runtime
ARG STOCKFISH_VERSION
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=stockfish /usr/local/bin/stockfish /usr/local/bin/stockfish
COPY --from=stockfish /usr/local/share/stockfish-version /usr/local/share/stockfish-version
COPY --from=rust-build /usr/local/bin/kibitz /usr/local/bin/kibitz
COPY --from=web-build /app/web/dist /app/web/dist

# `kibitz serve` defaults to 127.0.0.1 — it is a local single-user tool with no
# auth, so being unreachable from the network is the right default outside a
# container. Here the container's published port is the boundary instead, so we
# ask for 0.0.0.0 explicitly.
RUN cat > /usr/local/bin/kibitz-serve <<'SH' && chmod 0755 /usr/local/bin/kibitz-serve
#!/bin/sh
set -eu
exec kibitz serve \
    --bind "${KIBITZ_BIND:-0.0.0.0}" \
    --port "${KIBITZ_PORT:-7777}" \
    --web /app/web/dist
SH

# The engine is at a known path, so `stockfish` never has to be resolved through
# PATH and a wrong binary cannot be picked up.
ENV KIBITZ_STOCKFISH=/usr/local/bin/stockfish \
    KIBITZ_BIND=0.0.0.0 \
    KIBITZ_PORT=7777 \
    KIBITZ_LOG=kibitz=info \
    HOME=/home/kibitz

# A fixed uid/gid keeps the named volume's ownership stable across rebuilds.
RUN groupadd --gid 10001 kibitz \
    && useradd --uid 10001 --gid 10001 --home-dir /home/kibitz --create-home kibitz \
    && mkdir -p /home/kibitz/.kibitz \
    && chown -R kibitz:kibitz /home/kibitz
USER kibitz
WORKDIR /app

# ~/.kibitz/kibitz.db. Declared so `docker run` without compose still keeps the
# cache; compose gives it a named volume.
VOLUME ["/home/kibitz/.kibitz"]
EXPOSE 7777

HEALTHCHECK --interval=15s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${KIBITZ_PORT}/api/health" | grep -q '"ok":true'

LABEL org.opencontainers.image.title="kibitz" \
      org.opencontainers.image.description="Interactive chess analysis that explains why" \
      org.opencontainers.image.source="https://github.com/ekkx/kibitz" \
      org.opencontainers.image.licenses="GPL-3.0-or-later" \
      dev.kibitz.stockfish.version="${STOCKFISH_VERSION}"

ENTRYPOINT ["/usr/local/bin/kibitz-serve"]
