# kibitz — Design

Interactive chess analysis that explains why — Stockfish finds the refutation, an LLM narrates it.

## 1. Purpose

A local tool for loading your own games (Chess.com / Lichess) and understanding, in natural language, **why a move is bad or why the best move is good — while trying arbitrary moves on the board**.

How it differs from existing tools:

| | Existing | kibitz |
|---|---|---|
| Analysis model | Feed a PGN → one bulk report (batch) | Branch from any position and analyze on demand (interactive) |
| Explanation | Move classification only, or canned template text | Concrete reasoning from an LLM |
| Counterfactual lines | Listed as text | **Auto-played on the board while narrated** |
| Runtime | Cloud, paid | Fully local |

### Non-goals

- Playing games (use Lichess for that)
- Building an opening database, or game management
- Multi-user / auth / deployment

## 2. Design principle

**Never let the LLM reason about the board.**

An LLM cannot track piece placement and will happily talk about pieces that are not on the board. Therefore:

1. All facts are produced by Stockfish and deterministic code
2. The LLM's only job is to verbalize the structured data it is given
3. Even a claim like "this takes the center" is handed over as the result of actually counting attackers

The LLM keeps exactly three degrees of freedom:

- **Which facts to pick** — of the many features available, which ones matter in this position
- **In what order to say them** — conclusion → how the opponent punishes → positional reasons
- **In what words** — vocabulary suited to the reader's level

It makes no judgments. Good/bad is decided by `Classification`, why-it-fails by `Counterfactual`, and whether you take the center by the `center_control` numbers.

This split has a side benefit. When an explanation is wrong, you can localize the cause: **if the facts are wrong it's a bug in the feature-extraction layer; if the facts are right but the wording is off it's the LLM layer**.

### Reconciling this with strategic explanation and follow-up questions

Requests like "what is the opponent aiming at here?" or "what if I had played something else?" appear to violate the principle. The resolution is **not to give the LLM freedom, but to give it more facts to reason from**.

| Request | What it gets |
|---|---|
| Strategy and future outlook | A long PV plus the feature diff at its terminal position (`StrategicOutlook`, §8.6) |
| Arbitrary "what if" | **The engine handed over as a tool** (`AnalysisTools`, §12.4) |

Neither has the LLM guessing; both have the LLM going out and fetching facts. The principle is unchanged.

## 3. Conventions — sign and point of view

Sign errors are the single most likely bug here, so the conventions are pinned down first.

| | Convention |
|---|---|
| Stockfish `score` | Always from the side to move **in that position** |
| `win_prob` | Same. 0..1 |
| `win_prob_before` | Eval of position N = win probability for the side about to move |
| `win_prob_after` | Eval of position N+1 **subtracted from 1** = win probability for the side that just moved |
| `delta` | `win_prob_after - win_prob_before`. **Negative means it got worse** |

Position N+1 is the opponent's turn, so `win_prob_after = 1.0 - win_prob(P_{N+1})`. Get this wrong and every classification flips.

## 4. What to build, what to reuse

| | Owner |
|---|---|
| Board logic, legal moves, attack maps, SAN, FEN | `shakmaty` |
| PGN parsing | `pgn-reader` |
| Position eval, best-move search, PV generation | **Stockfish itself** |
| Opening-book lookup | Lichess Opening Explorer API |
| **Win-probability conversion, move classification** | ours |
| **SEE** (static exchange evaluation) | ours (~80 lines, thanks to `attacks_to`) |
| **Feature extraction** (center control, king safety, pawn structure) | ours |
| **Tactical motif detection** (fork, pin, discovered attack) | ours |
| Variation tree, prompts, GUI | ours |

We only build the middle layer. This is not a chess engine project; it is **a layer that translates the numbers and lines an engine produces into human vocabulary**.

## 5. Architecture

```
┌─ web (browser / TypeScript) ───────────────┐
│  chessground board + variation tree        │
│  user moves a piece = a node is added      │
└───────────────┬────────────────────────────┘
                │ HTTP
┌───────────────▼────────────────────────────┐
│ kibitz (Rust)                              │
│                                            │
│  1. book:   opening lookup (before engine) │
│       └ if in book, skip the rest          │
│  2. engine: Stockfish UCI (MultiPV=3)      │
│       └ one task, latest request only      │
│  3. core:   classify + features (det.)     │
│       └ AnalysisContext                    │
│  4. filter: pick what gets explained       │
│  5. llm:    Provider                       │
│       ├ ClaudeCode   (claude -p)           │
│       └ AnthropicApi (raw HTTP)            │
│                                            │
│  store: SQLite (analysis / explanation /   │
│         book cache)                        │
└────────────────────────────────────────────┘
```

The crux is that **the book lookup sits in front of the engine**. Book moves reach neither the engine nor the LLM.

### Language choice

**Rust on the backend, TypeScript on the frontend.**

The board UI uses chessground (Lichess's board), so the frontend is TS by necessity. What settles Rust for the backend is `shakmaty`:

- Written by niklasf (a Lichess developer), running in Lichess production (tablebase / opening explorer servers)
- **It has `Board::attacks_to(sq, color, occupied)`** — the equivalent of python-chess's `attackers()`. This removes nearly all hand-rolled code in the feature-extraction layer
- `attacks::{rook_attacks, bishop_attacks, knight_attacks, king_attacks, ray, between}` are all there, so pin / skewer / discovered-attack detection can be written directly
- SAN / FEN / UCI conversion, legal move generation, and game-end detection are included

Two languages, but the boundary is a single HTTP API.

### Crates used

| Purpose | crate | Notes |
|---|---|---|
| Board logic | `shakmaty` | |
| PGN | `pgn-reader` | Only at import/export |
| Async, process management | `tokio` | Drives Stockfish stdin/stdout |
| HTTP server | `axum` | |
| HTTP client | `reqwest` | Explorer API, game fetching |
| SQLite | `rusqlite` (`bundled`) | Keeps a single binary |
| Serialization | `serde` / `serde_json` | |

**There is no official Anthropic Rust SDK.** `AnthropicApiProvider` calls the Messages API directly via `reqwest` (~150 lines). The default `claude -p` path is unaffected.

The UCI driver is likewise hand-rolled on `tokio::process` rather than pulling in a dedicated crate.

## 6. Directory layout

```
kibitz/
├── crates/
│   ├── core/            # pure logic. no engine, no network
│   │   ├── eval.rs      # win-probability conversion
│   │   ├── classify.rs  # move classification
│   │   ├── see.rs       # Static Exchange Evaluation
│   │   ├── feature.rs   # StaticDiff
│   │   ├── motif.rs     # tactical motif detection
│   │   └── tree.rs      # variation tree
│   ├── engine/          # Stockfish UCI driver
│   ├── book/            # opening-book lookup
│   ├── llm/             # Provider abstraction
│   ├── store/           # SQLite
│   └── server/          # HTTP API
├── cmd/                 # CLI entry point
├── web/                 # Vite + React + chessground
├── docs/DESIGN.md
└── Cargo.toml           # workspace
```

`core` optimizes for testability above all else. Explanation quality is decided in this layer.

## 7. Variation tree

PGN is used **only at import and export**. The internal representation is our own.

We use an arena: `Vec<Node>` plus indices. Building bidirectional parent/child links in Rust with `Rc<RefCell<>>` makes borrow handling tedious. `NodeId` is `Copy` and easy to pass around, and serde can serialize the whole tree to JSON and ship it to the frontend as-is.

```rust
pub type NodeId = usize;

#[derive(Serialize)]
pub struct GameTree {
    nodes: Vec<Node>,
    root:  NodeId,
}

#[derive(Serialize)]
pub struct Node {
    parent:   Option<NodeId>,
    children: Vec<NodeId>,          // [0] is the mainline, [1..] are variations
    san:      Option<String>,       // the move leading here (None at root)
    fen:      String,               // used as cache key and for API calls
    analysis: Option<PositionAnalysis>,  // lazily evaluated
}
```

| Operation | Behavior |
|---|---|
| `play(node, mv)` | If a child with the same move exists, return it (**auto-merge**). Otherwise create one |
| `mainline()` | Follow `children[0]` |
| `path_to(node)` | Move sequence from the root |

Auto-merge means the tree does not balloon no matter how many times you retry the same line, and analysis results get reused.

One game plus variations is only a few hundred nodes, so sending the entire tree as JSON to the frontend every time is fine.

PGN export writes `children[0]` as the mainline and `children[1..]` as `( ... )` variations.

## 8. core — AnalysisContext

The only data structure handed to the LLM. If the LLM says something not contained here, it is a hallucination.

```rust
#[derive(Serialize)]
pub struct AnalysisContext {
    pub position:       Position,
    pub candidates:     Vec<Candidate>,   // MultiPV results verbatim. [0] is best
    pub played_rank:    Option<usize>,    // where `played` ranks among candidates
    pub played:         PlayedMove,
    pub counterfactual: Counterfactual,
    pub static_diff:    StaticDiff,
}

#[derive(Serialize)]
pub struct Candidate {
    pub san:      String,
    pub win_prob: f64,
    pub pv:       Vec<String>,
}

#[derive(Serialize)]
pub struct PlayedMove {
    pub san:             String,
    pub win_prob_before: f64,
    pub win_prob_after:  f64,
    pub delta:           f64,
    pub classification:  Classification,
}

/// The "what would have happened otherwise" line. Its content depends on the classification.
/// Bad move → how the opponent punishes the move actually played
/// Good move → how things collapse if the second-best move is played
#[derive(Serialize)]
pub struct Counterfactual {
    pub kind:   CounterfactualKind,  // Refutation | AlternativeCollapse
    pub pv:     Vec<String>,         // ★ this is what gets replayed on the board
    pub motifs: Vec<Motif>,
}
```

`best` / `alternative` are folded into `candidates` rather than kept as separate fields, to avoid duplication when the played move is the best move, and so the Great check (gap between #1 and #2) can be written directly.

### 8.1 Eval → win probability

Classifying moves by centipawn difference misjudges "+900 → +700" as a huge blunder. Always convert to win probability first, then take the difference. We use the same logistic transform as Lichess.

```rust
pub fn win_prob(cp: i32) -> f64 {
    1.0 / (1.0 + (-0.00368208 * cp as f64).exp())
}
```

**Mate is not collapsed into a win probability.** The presence of mate is handled as a separate rule ahead of classification (see §8.3).

```rust
pub enum Score {
    Cp(i32),
    Mate(i32),   // positive = side to move wins
}
```

Reason: if mate saturates to 1.0, then since `win_prob(500) = 0.863`, **missing a forced mate and ending up at +5 yields a delta of only 0.137, which lands as an Inaccuracy**. No amount of threshold tuning makes it a Blunder — the problem is structural.

Lichess also uses the following formula for its displayed Accuracy%. Add it if we need it.

```
Accuracy% = 103.1668 * exp(-0.04354 * (WinPct_before - WinPct_after)) - 3.1669
```

### 8.2 SEE — Static Exchange Evaluation

Given a square where an exchange happens, statically settle it and return the material result. Used by `hanging_pieces` (§8.4) and by motif detection (§8.5), and it is what lets an explanation say "this move offers a piece" — see the note at the end of §8.3.

`Board::attacks_to()` makes it straightforward (capture with the least valuable attacker first, settle with negamax). About 80 lines.

### 8.3 Move classification

`Blunder` and `Mistake` follow Lichess's `lila/modules/analyse/src/main/Advice.scala`. `Inaccuracy` and `Excellent` started there and have since been recalibrated against club-player games — see "Calibration" below. Great / Miss do not exist in Lichess (they are Chess.com and WintrChess inventions), so we define them ourselves.

```rust
pub enum Classification {
    Book,        //
    Great,       // !
    Best,
    Excellent,
    Good,
    Inaccuracy,  // ?!
    Mistake,     // ?
    Blunder,     // ??
    Miss,
}
```

Checks are evaluated top-down; the first match wins.

| Classification | Condition |
|---|---|
| `Book` | **Decided by the book lookup earlier in the pipeline** (not handled inside `classify()`) |
| `Miss` | **The best move was mate, and the played move is no longer mate** |
| `Great` | See below |
| `Blunder` | delta ≤ -0.30 (e.g. 80% → 50%) |
| `Mistake` | delta ≤ -0.20 |
| `Inaccuracy` | delta ≤ -0.07 (Lichess: -0.10) |
| `Best` | Matches the best move |
| `Excellent` | delta > -0.03 (Lichess: -0.02) |
| `Good` | Everything else, i.e. -0.07 < delta ≤ -0.03 |

`Miss` is defined by the presence of mate rather than by win probability because a win-probability-based drop would double-fire with Blunder / Mistake.

#### Calibration — why `Inaccuracy` and `Excellent` left Lichess

Lichess's -0.10 and -0.02 leave `Good` a band 0.08 wide. Measured on 40 rated Lichess games between players in the 1200-1600 band (mean Elo 1440, 2639 classified moves, depth 12, no opening book), that band was the largest bucket in the table — **29.3% of every move played** — running from "indistinguishable from the best move" to "clearly an error" under a single label that reads as praise. Each edge moved for a different reason, and the distinction matters:

**`Excellent` -0.02 → -0.03 is forced by measurement.** Re-running 16 of those games at depth 18 and diffing per move, the depth-12 delta agrees with the depth-18 delta to a median of 0.007, but the 90th percentile of the disagreement is 0.028 for moves in this region. A boundary at -0.02 sits *inside* the error bar of the number tested against it. -0.03 puts it just outside, and costs nothing: depth-12-vs-18 verdict agreement is identical at either value (776/1039 moves).

**`Inaccuracy` -0.10 → -0.07 is a judgement call, recorded as one.** The delta distribution decays smoothly here with no knee, so no "correct" number exists in the data. What the data does establish is that the relabelled moves are real — of the 79 sampled moves losing 6-10 points of win probability at depth 12, the depth-18 median is -0.068, unmoved, and 71 still lose at least 4 points — and that they do not behave like good moves: 66% are outside the engine's top five candidates, the profile of an `Inaccuracy` (72%) rather than an `Excellent` (15%). The cost is about two points of depth agreement (74.7% → 72.6%), which is what drawing a finer line in a denser part of the distribution costs.

Net effect on the sample: `Good` 29.3% → 17.1%, `Excellent` 22.6% → 29.5%, `Inaccuracy` 7.2% → 12.5%. `Mistake`, `Blunder` and `Miss` are untouched — this changes what stops counting as a *good* move, not what counts as a serious error.

**Search depth was ruled out first.** The obvious competing explanation is that depth 12 (the default) invents these deltas out of fixed-depth noise. It does not: across the 1039 moves analysed at both depths the aggregate distribution barely moves (`Good` 27.8% → 25.8%, `Mistake` 2.5% → 2.6%), and 78.2% of depth-12 `Good` moves are still `Good` at depth 18. The bulk of the depth disagreement that does exist is `Best` ↔ `Excellent` churn (118 of 263 disagreements), which is MultiPV rank instability between two praise labels and is symmetric, so it biases nothing.

**Most of that disagreement is not depth at all.** Two runs of the *same* 40 games at the *same* depth 12 with the *same* configuration agree on only 77.4% of verdicts, and reproduce just 3% of deltas exactly (median difference 0.0070, p90 0.0218) — essentially the same spread as depth 12 against depth 18. That is the multi-threaded search non-determinism documented on `DEFAULT_MULTIPV` in `crates/engine/src/lib.rs`. Two consequences: searching deeper buys far less verdict stability than it appears to, and no delta-based boundary can be drawn finer than about 0.02 no matter how much CPU is spent. Any future threshold work should be judged against the 77.4% run-to-run baseline rather than against 100%.

#### Great (!) — "the only move"

Two routes reach `Great`. Either one is enough.

**Route A — the only move that held.**

```
1. played move is best (played_rank == 0)
2. candidates[0].win_prob - candidates[1].win_prob ≥ 0.30  (= -`BLUNDER_DELTA`)
3. the position is undecided (0.10 < win_prob_before < 0.90)
4. win probability holds anyway (delta ≥ `EXCELLENT_DELTA`, currently -0.03)
```

"Any other move would have collapsed the position; only this one held it." The gap between #1 and #2 is the core of the check, and MultiPV gives it to us for free. **Condition 2 is deliberately `-BLUNDER_DELTA`**: the runner-up's delta, had it been played, is exactly `-gap`, so the rule says the second-best move would itself have been a `Blunder`. See "Calibration — how often `Great` should fire" below for the measurements that set it, and for what it costs.

**Condition 3 is the important one.** Without it, "the only move in a +15 position" fires on positions where nothing was ever at stake. Only its upper bound is live — the gap is at most `win_prob_before`, so `DECIDED_LOW` cannot fire while `GREAT_GAP` exceeds it. Condition 4 matters for a subtler reason: position N and position N+1 are separate searches, so a move can rank first and still show a large negative delta — awarding "!" alongside an accuracy of 35 is a contradiction the user would be right to distrust.

**Route B — the only move that mates.**

```
1. played move is best (played_rank == 0)
2. candidates[0] is Mate(n) for the side to move, n ≥ 2
3. candidates[1] exists and is not mate for the side to move
4. win probability holds anyway (delta ≥ `EXCELLENT_DELTA`, currently -0.03)
```

The mirror image of `Miss`, and defined the same way — by the presence of mate, never by a win-probability gap. **This is why route B does not use the 0.10 gap of route A**: mate saturates to 1.0 (§8.1), so against a second-best of +9.00 the gap is about 0.035 and route A's condition 2 fails, even though finding a forced mate in two where every other move merely wins is exactly the case worth marking. It is the §8.1 problem pointed at the winning side, so probability arithmetic is not used here at all.

There is no `undecided` condition on route B, and route A keeps its own: a forced mate is the one thing still worth pointing at once the position is decided, because the mate is *why* it is decided.

**`n ≥ 2` in condition 2 is deliberate.** A move scoring `Mate(1)` *is* the checkmate; playing it is not an insight and by definition was not hard to see. Annotation convention agrees — `!` goes on the move that sets the mate up, not on the mate itself — and `Great` implies `deserves_explanation()` (§12.1), so including mate in one would spend an LLM call producing "Rd8 is checkmate". Condition 3 carries the rest of the weight: with a single legal move (`candidates[1]` absent) the move was forced, not found, and if the runner-up mates too then this move was not the only one that mates. Condition 4 is kept for the same reason as on route A: the two searches have to agree that this move mates.

In `testdata/opera_game.pgn` at depth 12, route B moves exactly one verdict: `16.Qb8+` from `Best` to `Great`. White's win probability there is already 1.0 and the runners-up are +3.43 and +3.34, so route A cannot reach it. `17.Rd8#` scores `Mate(1)` and stays `Best`.

#### Calibration — how often `Great` should fire

This document predicted the Great thresholds would need tuning. They did.

The corpus is a fresh draw by the recipe above, since the earlier games were not kept: **the Lichess open database for 2015-01, rated classical, both players 1200–1600, at least 40 plies, first 40 games in file order** — mean Elo 1458, 3,286 classified moves, depth 12, MultiPV 5, no opening book. Rates therefore differ in the second decimal from the delta calibration's, and the harness is again a throwaway (`cargo run --example`) rather than shipped code: it analyses a directory of PGNs and dumps one JSON record per move — the two top candidates, the played move's rank, the delta and the verdict — so thresholds can be re-swept offline without touching the engine.

On that corpus `GREAT_GAP = 0.10` marked **6.27% of all moves — 5.15 a game, a median of 4, and only 2 of 40 games without one.** "The only move that held the position" cannot happen four times a game.

The two routes were measured separately. Route A produced 193 of the 206 marks (5.87% of all moves); **route B produced 13 — 0.40%, about one game in three — and is left exactly as it was.** It is already rare, every one of its marks is a move route A cannot reach, and it is what puts the "!" on the queen sacrifice.

Route A's gap distribution has its mode against the old threshold: 28.5% of qualifying moves sat in the single bucket 0.10–0.15 (p25 = 0.138, median 0.248), so the line was drawn through the densest part of the distribution. The rate falls smoothly with no knee — 5.87% at 0.10, 3.62% at 0.20, 2.13% at 0.30, 1.10% at 0.40 — so the data does not choose a number. **0.30 is chosen because it is `-BLUNDER_DELTA`**, which makes the rule say something the table already says: the runner-up would itself have been a blunder. The old 0.10 asked for less collapse than an `Inaccuracy`.

| | old (0.10) | new (0.30) | for scale |
|---|---|---|---|
| % of all moves | 6.27% | **2.53%** | `Blunder` 3.41%, `Mistake` 1.98% |
| per game | 5.15 | **2.08** | |
| median per game | 4 | **1** | |
| games with none | 2 of 40 | **7 of 40** | |

The mean sits above the median because of one 137-ply game — a pawn race and a queen-versus-pawn ending, where "the only move that holds" is literally true 15 times.

**`undecided` was measured too, and stays at 0.10 / 0.90.** Its lower bound never fires and cannot: the gap is at most `win_prob_before`. Its upper bound does real work — it blocks 22 of the 92 moves that otherwise clear the new gap. Tighter bands were tried and rejected: (0.20, 0.80) gives 2.25% and (0.30, 0.70) gives 1.86%, but a 0.30 collapse from 0.80 leaves the mover at 0.50, which is a won game thrown away, not a case of nothing being at stake.

**What the gap cannot do.** 69% of the moves qualifying at 0.30 are captures, against 36.5% of all rank-0 moves — and that proportion is the same at 0.10 and at 0.40. Most of what the rule finds is forced recaptures: "the only move" in the arithmetic sense, and never hard to see. Nothing in win probability separates them from a move that had to be found (`see.rs` could, but classification stopped reading SEE when `Brilliant` was removed). The threshold sets how often the mark appears, not how deserved it is.

**Stability is not the argument either way.** Two identical depth-12 runs of the corpus agree on 79.9% of verdicts. Of the moves marked `Great` in run 1, 94.7% are `Great` in run 2 at the old gap and 90.4% at the new one — better than verdicts in general at either setting. Raising the line does not buy reproducibility; the moves sitting on it stay roughly constant in number (11 vs 8) while the population shrinks by two thirds.

**The cost: `testdata/opera_game.pgn` drops from 6 `Great`s to 1** in the run this was measured on, and from 4 to 2 in a second depth-12 run of the same game — the difference is `11.Bxb5+`, whose gap comes out 0.296 once and 0.337 the next time, i.e. exactly on the line. `16.Qb8+` survives either way, by route B. `10.Nxb5` — the knight sacrifice this document has cited as a landmark verdict — does not: its gap is 0.137–0.149, because win probability saturates (§8.1), so from 0.76 the alternatives still lead to 0.62 and in this tool's currency missing it costs 14 points, an inaccuracy rather than a collapse. That is a real loss, accepted rather than worked around: what made those moves special is that they were sacrifices, and the `Brilliant` note below already settled that a sacrifice belongs in the explanation text rather than in the glyph. A gap low enough to keep them is a gap that also keeps 4.83 marks a game.

#### Note: `Brilliant` (!!) was removed

Earlier versions had a tenth classification, `Brilliant` (`!!`), between `Miss` and `Great`. It was the Great conditions **plus** one more: the SEE of the played piece is negative, i.e. the move gives material away. It has been removed, and it is not coming back. The reasons, so this is not re-litigated as an oversight:

1. **It carried no information of its own.** The difficulty of finding the move and its value to the position are identical to `Great` — both mean "the only move that held". Only the visual flourish differed. The SEE value is in `AnalysisContext` regardless, so an explanation can still say "and this move offers a piece" inside a `Great` write-up, which is where a fact about the move belongs rather than in the label above it.
2. **It was the most fragile rule in the table.** It stacked two thresholds (the win-probability gap *and* SEE), so borderline moves flipped there most easily, and it was the category that actually flipped under the engine's known non-determinism (see the `DEFAULT_MULTIPV` note in `crates/engine/src/lib.rs`).
3. **It is the classification everyone gets wrong.** This document already called it the most prone to false positives and budgeted a tuning campaign against real games for it. For a tool aimed at club players, that tuning cost does not pay for itself.

`Great` deliberately did **not** absorb anything in exchange, and no replacement category was added. A sacrifice that is *not* the only move is classified purely on its delta, like any other move — usually `Best` or `Excellent` — but that was equally true before, since `Brilliant` required the Great conditions too. Nothing lost coverage: every move that was `Brilliant` is now `Great`, and the set of moves that get an LLM explanation is unchanged. In `testdata/opera_game.pgn` at depth 12 exactly one verdict moved, `10.Nxb5` from `Brilliant` to `Great`; the other 32 are identical.

The one thing that genuinely disappears is the label's ability to say "and it was a sacrifice". That fact now lives where the rest of the evidence lives — `StaticDiff.hanging_pieces` and the motifs in `AnalysisContext` — and the LLM writes it into the explanation instead of the glyph implying it.

`see.rs` stays. It was never only for this rule — `hanging_pieces` (§8.4) and motif detection (§8.5) both use it. What did go with `Brilliant` is `ClassifyInput::played_see`: classification no longer reads SEE at all, so the pipeline stopped computing it per move.

### 8.4 StaticDiff

Computed before and after the move and kept as a **diff**. This is the evidence behind the LLM saying "X improves / degrades".

| Item | How it's computed |
|---|---|
| `material` | Material balance (P=1, N=B=3, R=5, Q=9) |
| `hanging_pieces` | Squares where SEE is negative |
| `center_control` | Attacker count on d4/e4/d5/e5 per color (`attacks_to`) |
| `king_safety` | Enemy attackers on the 8 squares around the king, missing pawns in front |
| `pawn_structure` | Squares with isolated / doubled / passed pawns |
| `open_files` | Half-open and open files, and the rooks sitting on them |
| `mobility` | Legal move count per color |

All of it derives mechanically from `shakmaty::attacks` and `Board::attacks_to()`.

### 8.5 Tactical motif detection

Run on each move of `Counterfactual.pv`. The substance of "why it's bad" is not the eval difference but **how the opponent punishes it**, so this is the core of the explanation.

| Motif | Check | API used |
|---|---|---|
| `fork` | The moved piece attacks two or more higher-value pieces or the king at once | `attacks()` |
| `pin` | On the attacker's line: enemy piece, then a higher-value enemy piece behind it | `ray()` / `between()` |
| `skewer` | Same, order reversed | Same |
| `discovered_attack` | A friendly piece behind the moved piece gains a new line | `ray()` |
| `hanging` | A piece with negative SEE is actually captured in the PV | `see()` |
| `back_rank` | A rook/queen arrives while the king on the 1st/8th rank has no escape square | `king_attacks()` |

### 8.6 StrategicOutlook — strategy and future outlook

Material for explaining "what the opponent is aiming at". Constructed **as a summary of the engine's line, not as inference**.

```rust
#[derive(Serialize)]
pub struct StrategicOutlook {
    pub long_pv:       Vec<String>,   // 10-12 ply of best play
    pub terminal_fen:  String,
    pub terminal_diff: StaticDiff,    // feature diff: current position → PV terminal
    pub consensus:     CandidateConsensus,
}

/// What the top candidate moves have in common. Shows the direction the position demands.
#[derive(Serialize)]
pub struct CandidateConsensus {
    pub common_piece:   Option<Role>,      // all three move the same piece
    pub common_targets: Vec<String>,       // all three aim at the same squares
}
```

`terminal_diff` is what earns its keep. Statements like "ten moves later the d-file opens and a rook lands on it" or "the enemy king gets less safe" can be written as measured values at the PV terminal position rather than as guesses.

**State the limits explicitly.** A PV assumes best play from both sides, so the opponent will not necessarily play it — the lower their rating, the sooner they leave the PV. So the prompt's prohibitions gain:

```
✗ "Your opponent is going for a kingside attack"   ← asserting intent
○ "In the engine's line, the d-file opens next and …"
```

Getting closer to "what the opponent will actually do" requires pairing this with Maia (open question, §15).

## 9. book — opening lookup

Two different questions live in this chapter, and keeping them apart matters:

| Question | Answered by | Depends on the network |
|---|---|---|
| **Is this move still theory?** | Lichess Opening Explorer game counts, or the ECO table when the Explorer is unreachable (§9.3) | preferably |
| **What is this opening called?** | An ECO table embedded in the binary | no |

Naming a position is still not the same event as leaving the book, and the two indexes
are built for the two different questions — names from the position each ECO row *ends*
on, theory from every position a row *passes through*. `eco.rs` hands out both as facts
and never a verdict; `Book::judge` decides what they are worth (there is a test
asserting `eco.rs` never references `Classification` or `BookVerdict`).

### 9.1 Is it theory? — Lichess Opening Explorer

Being told a book move is "inaccurate" is useless, so the lookup happens **before the engine** and matching moves are skipped.

We use the **Lichess Opening Explorer API**. It has no depth limit as long as the moves were actually played, so books running past move 20 (e.g. the Berlin Defense) are tracked correctly.

```
GET https://explorer.lichess.ovh/masters?fen={FEN}
→ { opening: {eco, name}, moves: [{uci, san, white, draws, black}], ... }
```

Lookup logic:

```
position is in the DB and the played move is among the candidates
  → Classification::Book. Skip both analysis and explanation
first move not among the candidates
  → mark as "left the book" and send it down the normal analysis pipeline
```

As a by-product we get **"you left the book here (Ruy Lopez, Berlin Defense — matched through move 12)"** for free.

### Rate limiting

The unauthenticated Explorer API is strict about back-to-back calls. Firing 40 positions from one game at once returns 429.

- Serialize the calls (never in parallel) + exponential backoff
- Cache in SQLite; never look up the same position twice
- Stop querying once the game leaves the book (you never re-enter it)

The third one does the work. Actual lookups settle at 10-20 positions per game.

If we ever want offline operation, the structure should allow a fallback to a Polyglot book (.bin).

### 9.2 What is it called? — the embedded ECO table

`crates/book/data/{a..e}.tsv` is vendored from
[lichess-org/chess-openings](https://github.com/lichess-org/chess-openings) (3,810
named lines, CC0, 388 KB). `eco.rs` replays each row's PGN with shakmaty and indexes
the resulting EPD, so transpositions resolve to the same opening. Building the whole
table costs **4.7 ms in release**, which is why it is a `LazyLock` and not a build
script.

Names attach only to the **terminal position of each row**, so they are not
continuous — in the Chigorin test game ply 21 has no name while plies 22 and 25 do.
Consumers walk back to the nearest named ancestor rather than assuming every position
in a line carries a name.

This became load-bearing rather than decorative: **the Explorer has returned `401` for
every request since an outage beginning 2026-02-23**
([lichess-org/lila#19610](https://github.com/lichess-org/lila/issues/19610), still
open, and affecting other clients too). While that lasts, 9.1 yields nothing. The book
layer treats an auth failure as "unavailable" and stops asking for an hour — and then
falls back to the table below.

### 9.3 The ECO fallback — reversing "names only"

**This section reverses a decision recorded above, on purpose.** The original rule was
that ECO supplies names and never a verdict, because "this position has a name" is a
far weaker claim than "N thousand games reached it": a named line can still be a bad
move order, and the table carries no frequency information at all. *That reasoning is
still correct.* What changed is availability. Six months into the outage,
`Classification::Book` has never been produced in practice, and theoretical opening
moves are being handed to the engine and returned as `Inaccuracy`. A weak mark that
appears is more useful than a strong one that never does, so the weaker claim is now
allowed to speak — **only where the stronger one cannot**.

The Explorer stays authoritative. Whenever a response exists, live or from the SQLite
cache, its game counts decide and the ECO table is not consulted; the two are never
merged. The fallback runs only on the "Explorer off limits" path (401/403/429 cooldown).
A genuine fault — a 500, a malformed body — still surfaces as an error and still
degrades to `OutOfBook`, which is the property `pipeline.rs` depends on.

**"In book" in ECO terms.** Three conditions, all required:

1. The position *after* the move occurs somewhere in the vendored lines. This uses a
   second index — every position each row passes through, 7,855 of them, against 3,810
   named line ends. Indexing whole lines rather than their endpoints is what makes the
   mark continuous: "the position after the move is *named*" would stop the book at ply
   6 of `testdata/ruy_lopez_chigorin.pgn` and resume at 22, when the theory in fact runs
   unbroken to ply 25.
2. The position *before* the move is in that index too. One position matching is a
   coincidence; two consecutive ones joined by the move actually played is a line. This
   is the guard against the mark reappearing on a transposition thirty moves later, and
   it is needed because `judge` is called per move on the interactive path, where there
   is no "we already left the book" state. `judge_line` enforces the same thing across a
   sweep by never re-entering.
3. The move is within `eco_max_ply` = **30 plies** (move 15). A backstop for what
   continuity cannot catch — a game that wanders back into a theory position long after
   leaving its own opening. Set from both ends: 99.9% of vendored rows end by ply 28 and
   the deepest ends at 36, while over 3,286 plies of club games the longest continuous
   run through the index was 10 plies and the Chigorin test game runs 25. Nothing
   measured is cut; past move 15 the claim "you were still following preparation" has
   stopped being plausible anyway.

Anything else is `LeftBook` when the position before the move was theory and `OutOfBook`
when it was not — the same split the Explorer path makes between "known position,
unknown continuation" and "unknown position".

**What is knowingly given up.** `min_games` has no counterpart offline, so any sideline
one vendored row happens to contain counts as book: after 1. e4 e5 2. Nf3 Nc6 3. Bb5,
the table calls 3... a5 theory (it is the Bulgarian Variation) and 3... Nh6 not. The
Explorer would have weighed those by how often they were actually played. This is the
weakness the original decision was protecting against, accepted deliberately in exchange
for a mark that exists.

Measured on club games, the fallback marks the first **2 to 10 plies** of a game as book
(median 4). `testdata/opera_game.pgn` gets **7**: through 4. dxe5, leaving the book on
4... Bxf3. The last *named* position there is 3. d4 at ply 5, which is the gap condition
1 exists to close.

## 10. engine — Stockfish

- Drive **native Stockfish** (`brew install stockfish`) over UCI. 2-3x faster than WASM, and NNUE runs at full strength
- Hold stdin/stdout via `tokio::process` and parse `info ... multipv N ... pv ...`
- **`MultiPV = 5`**, one fixed width for every search, never a request parameter. Classification reads only ranks 0 and 1 (the best move, and the second for the Great check); ranks 3 to 5 exist so the board can draw up to five arrows. Two reasons the width is fixed rather than client-chosen: `multipv` is part of the analysis cache key, so a negotiable width would silently re-analyse an already-analysed game whenever the setting changed; and the width is **not classification-neutral** — re-running `testdata/opera_game.pgn` at depth 12 moved 9 of 33 verdicts between width 3 and 5, because MultiPV shifts how effort is spent and the "only move" gap compares exactly the two top scores. A client-chosen width would mean two users disagreeing about the same game. (That measurement predates the Great calibration in §8.3, which removed five of that game's six `Great`s outright. `Nxd7??` is unaffected, and the one surviving mark, `16.Qb8+!`, comes from route B, which does not read the gap the width perturbs)

  The cost is badly non-linear in the width, not linear as this line previously claimed. Over the 34 positions of that game at depth 12: raw Stockfish MultiPV 1 → 0.85s, 3 → 5.44s, 5 → 8.10s; the whole sweep fresh-cache → 6.9s at width 3, 10.0s at width 5. Almost all of the cost is the step away from a single PV (6.4x), which gives up aspiration windows and root pruning; widening 3 → 5 shares that work and adds only ~45%. That asymmetry is what makes a fixed width of 5 affordable — re-measure before changing it again
- `Threads = CPU count - 1`, `Hash = 1024`

### One task, and cancellation

In interactive use, each new move the user tries must stop the previous search.

Access to Stockfish is **funneled into a single tokio task**, fed by an mpsc channel. The policy is "latest only":

```
request arrives
  → discard anything pending in the queue
  → if a search is running, send UCI `stop` to abort it
  → start the new analysis
```

This is a personal tool; there is no point waiting on a stale result. It also fits the one-resident-process approach.

### Analysis pass — one search per position

Classification needs the eval of the move actually played, but if that move is not in the MultiPV top ranks there is no value for it. One option is a separate `go depth N searchmoves <move>` search, but there is a simpler way.

**Analyzing every position of the game exactly once is enough.**

```
MultiPV at position N   → best move, second best, counterfactual line
eval at position N+1    → the value of the move actually played at N (flipped per §3)
```

For positions P0..P_last, what we need:

| | MultiPV | eval |
|---|---|---|
| `P0` (start) | yes (alternatives to move 1) | no (no move leads to P0) |
| `P1..P_{last-1}` | yes | yes |
| `P_last` | **no** (no move is played from it) | yes |

Lichess does the same.

### Terminal positions

When `P_last` is checkmate or stalemate there are no legal moves, so Stockfish cannot search (it returns `bestmove (none)`, with no score or PV in the usual form).

Detect the terminal state with `shakmaty` before going to the engine and return fixed values:

```
checkmate  → win_prob = 0.0 (side to move has lost)
stalemate  → win_prob = 0.5
otherwise  → to the engine as usual
```

The final position of a game ended by resignation or timeout is an ordinary position, so it is analyzed normally.

### Search cutoff

**Cut off by depth, not by time** (`go depth 20`). The depth value itself is configurable.

The reason to avoid `movetime` is reproducibility. Cutting by time makes the same position yield different results under different machine load, so analysis stored in SQLite disagrees with the next run.

Strictly speaking, though, Stockfish with `Threads > 1` orders its search according to thread scheduling and is therefore **not exactly deterministic**. Top candidates rarely swap, so this is acceptable in practice; set `Threads = 1` if full reproducibility is required.

The trade-off is that wall time varies a lot by position (a simple endgame is instant, a complex middlegame takes tens of seconds). The right depth gets decided by measuring on my own games.

## 11. store — SQLite

Uses `rusqlite` (`bundled` feature) to keep single-binary distribution.

```sql
CREATE TABLE analysis (
  fen      TEXT NOT NULL,          -- normalized FEN (move counters stripped)
  depth    INTEGER NOT NULL,
  multipv  INTEGER NOT NULL,
  result   TEXT NOT NULL,          -- JSON
  PRIMARY KEY (fen, depth, multipv)
);

CREATE TABLE explanation (
  context_hash TEXT PRIMARY KEY,   -- normalized hash of AnalysisContext
  text         TEXT NOT NULL,
  model        TEXT NOT NULL,
  lang         TEXT NOT NULL,      -- part of the cache key (see 12.6)
  created_at   INTEGER NOT NULL
);

CREATE TABLE book (
  fen        TEXT NOT NULL,
  database   TEXT NOT NULL,        -- masters | lichess. Never mix the two.
  result     TEXT NOT NULL,        -- Explorer API response JSON
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (fen, database)
);
```

## 12. llm — generating explanations

### 12.1 Filtering what gets explained

**Do not call the LLM on every move.** Forty calls for a 40-move game at a few seconds each under `claude -p` takes over two minutes — unusable.

Explanations are generated only for:

```
Blunder / Mistake / Great / Miss
```

That lands at 3-8 calls per game. Whether to include `Inaccuracy` gets decided by measurement.

When the user tries an arbitrary move on the board interactively, an explanation is generated on the spot regardless of classification (the user explicitly asked for it).

### 12.2 Provider abstraction

```rust
#[async_trait]
pub trait Provider {
    async fn explain(
        &self,
        model: &str,
        lang:  &str,                 // "en" | "ja" (see 12.6)
        ctx:   &AnalysisContext,
    ) -> Result<BoxStream<'static, Result<String>>>;

    /// Follow-up questions. Runs as an agent with tools (§12.4)
    async fn ask(
        &self,
        model:    &str,
        lang:     &str,
        session:  &mut QaSession,
        question: &str,
    ) -> Result<BoxStream<'static, Result<String>>>;
}
```

| Implementation | Use |
|---|---|
| `ClaudeCodeProvider` | Spawns `claude -p --output-format stream-json` as a child process. Runs on the subscription, so zero extra cost. **Default** |
| `AnthropicApiProvider` | Calls the Messages API directly via `reqwest` (no official Rust SDK). Lower latency |

Switched with `KIBITZ_LLM=claude-code | anthropic`.

#### Extended thinking has to be off

Measured on the Opera Game: one Japanese explanation took **97.6 s**, of which 94.9 s
was time-to-first-token. The Claude Code CLI enables extended thinking by default, so
Haiku spent 91 seconds thinking before writing a word — **96% of the output tokens were
thinking tokens** (9929 total, ~400 visible). Setting `MAX_THINKING_TOKENS=0` and
`CLAUDE_CODE_DISABLE_THINKING=1` on the child process brings it to **6.5 s**.

This is consistent with the design rather than a compromise of it: the model is not
supposed to reason here. Every fact is already computed; the task is to put them into
words. Thinking budget spent on a task with no reasoning in it is pure latency.

Things that were measured and did **not** matter: the working directory, the prompt
size on its own, `--effort low`, and the output language (English was equally slow).
`--strict-mcp-config --mcp-config '{"mcpServers":{}}'` is also set — it stops the
user's globally configured MCP servers from being launched, worth about 1 s of startup.
That flag has to change when the Phase 3 follow-up mode passes `AnalysisTools` in.

The realistic floor is **~7-9 s per explanation, ~2 s to first token**: roughly 1 s of
Node startup, 1 s of prefill, and 5-6 s of generation. Cutting further means either
asking for shorter output or keeping one process resident via
`--input-format stream-json`.

### 12.3 Model config — one per task

**Explanation and reasoning demand different capabilities**, so the model is configurable per task.

```
KIBITZ_MODEL_NARRATE=claude-haiku-4-5   # explanation (tracing structured data)
KIBITZ_MODEL_REASON=claude-opus-5       # strategic reasoning, follow-ups (judgment needed)
```

Passed as the `--model` flag for `ClaudeCodeProvider`, and the `model` parameter for `AnthropicApiProvider`.

The explanation task looks like pure tracing, but it actually involves **choosing which of the many features matter in this position**. Weakness here produces output that flatly lists every item: "You're up material. You also control the center."

So make it configurable on the assumption that we **start with Haiku and move up to Sonnet if the output is flat**. Reference prices (per 1M tokens, input/output):

| Model | Price |
|---|---|
| `claude-haiku-4-5` | $1 / $5 |
| `claude-sonnet-5` | $3 / $15 |
| `claude-opus-5` | $5 / $25 |

### 12.4 Follow-up mode — hand the engine over as a tool

When the explanation alone is not enough, the user can ask follow-up questions.

**Simply continuing the conversation increases hallucination.** Asked "what about Bxf7+?", the LLM answers from guesswork if it has no data at hand.

The fix is to **turn the LLM into an agent in follow-up mode and hand it the engine as a tool**.

```rust
pub trait AnalysisTools {
    fn analyze_position(&self, fen: &str, depth: Option<u8>) -> Result<Vec<Candidate>>;
    fn analyze_move(&self, fen: &str, san: &str)              -> Result<MoveAnalysis>;
    fn get_features(&self, fen: &str)                         -> Result<StaticDiff>;
    fn lookup_opening(&self, fen: &str)                       -> Result<Option<Opening>>;
}
```

Now the LLM answers **only after actually analyzing**. The room to guess is structurally removed.

#### Implementation approach

Implement `AnalysisTools` as a plain Rust API and **put two thin adapters on top**. The core implementation exists once.

```
AnalysisTools (plain Rust)
  ├─ exposed as an MCP server        → used from claude -p via --mcp-config
  └─ converted to Anthropic tool defs → used from API tool use
```

| Option | Cost | Effort | Verdict |
|---|---|---|---|
| MCP server + `claude -p` | **free** (subscription) | medium (`rmcp` crate exists) | **adopted** |
| Anthropic API tool use | metered | small | kept alongside |
| No tools, preload related positions | free | small | rejected (cannot answer arbitrary "what ifs") |

Follow-ups assume a GUI (a CLI cannot sustain the dialogue), so this ships in Phase 3.

### 12.5 Prompt design

The prompt **structure** is shared by both providers:

```
[fixed]     role / prohibitions / output structure   ← authored per language
[fixed]     chess-term glossary                      ← non-English languages only
[variable]  AnalysisContext JSON
```

The fixed part is language-dependent, not a translated-at-runtime constant: the role/prohibitions/output-structure block is written out for each supported language. The glossary maps chess terms into the target language and is therefore included only for non-English languages — English needs none.

Prohibitions:

- Do not mention pieces, squares, or lines absent from the given JSON
- Do not judge whether the eval is good or bad yourself (follow `classification`)
- Do not pad with generalities; always ground the statement in a concrete fact from the JSON
- **Do not assert the opponent's intent.** `long_pv` assumes best play from both sides; the opponent will not necessarily play it
  - ✗ "Your opponent is going for a kingside attack"
  - ○ "In the engine's line, the d-file opens next and …"

Output structure: conclusion → how the opponent punishes → positional reasons.

**Explicit prompt-cache control exists only in `AnthropicApiProvider`.** Put `cache_control: {"type": "ephemeral"}` at the end of the system block. Under `claude -p`, Claude Code manages the cache itself so we cannot control placement — and process startup cost dominates anyway.

### 12.6 Explanation language

**The output language is a request parameter, not a build-time constant.** It is selected in the UI and travels down to the provider as `lang` (BCP-47-ish short codes: `"en"`, `"ja"`). Supported at launch: English and Japanese; the list is open to extension — adding a language means authoring the fixed prompt block (and its glossary) for it, nothing else.

**The explanation cache key must include the language.** Otherwise a Japanese explanation gets served for an English request — same `AnalysisContext`, same model, different output. The cache key already includes the model; language joins it.

Only the LLM layer is affected. `AnalysisContext` stays language-neutral: it holds SAN, squares, numbers, and enum variants, all of which are language-independent by construction.

## 13. The core of the UX

The moment the user plays a move, `Counterfactual.pv` is animated on the board one move at a time while the LLM's explanation streams in.

| Classification | Line replayed |
|---|---|
| Bad move (Blunder / Mistake / Inaccuracy) | How the opponent punishes it |
| Good move (Great) | How things collapse after the second-best move |

Instead of only reading "…if you take with Nxe4, Qa4+ forks the king and the knight", the pieces actually move. That is what makes it click.

## 14. Phase plan

| Phase | Content | Done when |
|---|---|---|
| **1** | `core` + `engine` + `book` | `kibitz analyze game.pgn` prints `AnalysisContext` JSON. SEE, win-probability conversion, all classifications, feature extraction, motif detection, StrategicOutlook, and book lookup are implemented and pinned down by tests |
| **2** | `llm` | The same CLI prints an analysis report (Markdown) in the selected language. Models are swappable by config. **This is the first point where a finished product exists** |
| **3** | `server` + `web` + `AnalysisTools` | Board + variation tree + line replay + streaming explanations + **follow-up mode** (tool use via the MCP server) |
| **4** | Game import | Bulk fetch from Chess.com / Lichess by username |

The temptation is to build the GUI first, but driving a thin vertical slice through 1→2 turns Phase 3 into "put a board on top of something that already works".

### Phase 1 implementation order

1. `core/eval` + `core/classify` — win-probability conversion and classification (pure functions, testable immediately)
2. `core/see` — SEE
3. `engine` — drive Stockfish, get MultiPV, handle terminal positions
4. `core/feature` — StaticDiff
5. `core/motif` — tactical motif detection
6. `core/outlook` — StrategicOutlook (reuses `feature`; just diffs against the PV terminal position)
7. `book` — opening lookup
8. `cmd` — feed it a PGN, get JSON out

## 15. Open questions

- Great thresholds (must be tuned on real data; most prone to false positives)
- Whether to include `Inaccuracy` among explained moves
- Whether to query the Explorer API's `masters` or `lichess` (general user statistics). `masters` is stricter but may over-report amateurs leaving the book. Make it switchable in `book` and measure in Phase 1
- Accounting for the reader's rating (pairing `Maia` to judge "is this a trap a player at that rating falls for?" would be a differentiator, but Phase 4+)
