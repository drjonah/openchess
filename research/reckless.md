# Reckless & Stockfish-Family Architecture Reference

> **Audience:** agents building or studying competitive chess engines.
> **Primary subject:** [Reckless](https://github.com/codedeliveryservice/Reckless) (Rust, AGPL-3.0) — a top-tier Stockfish-family engine.
> **Secondary subject:** the Stockfish architecture paradigm that Reckless (and virtually every top CPU engine) implements.
> **Stockfish-primary map:** [stockfish.md](./stockfish.md) — canonical C++ source layout, current NNUE topology, Fishtest, and Stockfish-specific details.
> **Agent task board:** [tasks.md](./tasks.md) — engine pillars (P1–P8) and parallelizable implementation tasks distilled from this research.
>
> Reckless is not a Stockfish fork, but it is an excellent *readable* embodiment of the same design. Use this doc as a map of that shared architecture, with Reckless as the concrete codebase and Stockfish as the conceptual origin.

**Sources:** Reckless `main` (~v0.10.0-dev / released v0.9.0 Mar 2026), [Stockfish](https://github.com/official-stockfish/Stockfish), Chess Programming Wiki, OpenBench/Fishtest ecosystem.

---

## 1. One-sentence model

**A modern top engine = bitboard board + Lazy SMP alpha-beta (PVS) + aggressive selective search + incremental NNUE eval + shared TT + history heuristics + SPRT testing.**

Everything else is tuning and SIMD.

Stockfish’s uniqueness is not a single trick — it is the *integration* of:

1. **Search depth via pruning** (effective branching factor ≪ 2)
2. **Eval speed via NNUE** (millions of nodes/sec on CPU)
3. **Empirical tuning at scale** (Fishtest / OpenBench SPRT)

Reckless implements the same stack in Rust and ranks ~#2 on SPCC/CCRL (v0.9.0 ≈ 3833 SPCC).

---

## 2. Reckless at a glance

| Property | Value |
|---|---|
| Language | Rust (edition 2024), ~99% Rust |
| Interface | UCI (+ WASM build) |
| Eval | Embedded NNUE (Bullet-trained), SIMD (AVX-512 / AVX2 / NEON / WASM SIMD) |
| Search | Iterative deepening + PVS + full modern selective search |
| Parallelism | Lazy SMP, NUMA-aware replication of NNUE params / correction history |
| Endgames | Optional Syzygy via Fathom (`deps/Fathom`) |
| License | AGPL-3.0 |
| Testing | OpenBench |
| Strength | Top-tier CCC/TCEC participant |

**Entry:** `src/main.rs` → `lib::run()` → `lookup::initialize()` + `nnue::initialize()` → `uci::message_loop()`.

**Release profile:** fat LTO, `panic = abort`, `codegen-units = 1`. Builds: `make` / `make no-syzygy` / `make pgo` / `make wasm`.

---

## 3. Reckless source map

```
src/
├── main.rs              # binary entry
├── lib.rs               # module graph + run()
├── uci.rs               # UCI protocol loop
├── wasm.rs              # WASM bindings
│
├── board.rs             # Board: bitboards + mailbox + state stack
│   ├── makemove.rs      # make/unmake (+ BoardObserver hooks for NNUE)
│   ├── movegen.rs       # legal / noisy / quiet generation
│   ├── see.rs           # Static Exchange Evaluation
│   └── parser.rs        # FEN / UCI move parsing
│
├── types/               # vocabulary: Bitboard, Move, Piece, Square, Score, Zobrist…
├── lookup.rs            # magic/attack tables, cuckoo (repetition)
├── setwise.rs           # setwise attack helpers for threats / movepick
│
├── search.rs            # ★ brain: ID + PVS + all pruning/extensions (~47KB)
├── movepick.rs          # staged move ordering
├── history.rs           # quiet/noisy/pawn/continuation + correction histories
├── stack.rs             # per-ply search stack
├── transposition.rs     # lock-free clustered TT
├── evaluation.rs        # post-NNUE corrections (material, optimism, 50-move)
├── time.rs              # time management
├── parameters.rs        # tunable search constants (SPSA feature)
│
├── nnue.rs              # ★ Network + Parameters + incremental stacks
│   ├── accumulator/     # PST + threat accumulators (incremental FT)
│   ├── forward/         # FT activate → L1 sparse → L2 → L3
│   └── simd/            # avx512 / avx2 / neon / wasm / scalar
│
├── thread.rs            # ThreadData, SharedContext, PV table, UCI info
├── threadpool.rs        # Lazy SMP workers + NUMA binding
├── numa.rs              # NUMA replication of large shared structures
├── tb.rs                # Syzygy probe wrapper (feature = syzygy)
│
└── tools/               # bench, perft, speedtest
```

### Module roles (agent cheat sheet)

| Module | Stockfish analogue | Responsibility |
|---|---|---|
| `board` | `position.cpp` | State, make/unmake, legality, threats, SEE |
| `types` | `types.h`, `bitboard.*` | Core types |
| `search` | `search.cpp` | Alpha-beta / PVS / selective search |
| `movepick` | `movepick.cpp` | Staged ordering |
| `history` | history tables in search | Move ordering + eval correction |
| `transposition` | `tt.cpp` | Shared cache of bounds/moves |
| `nnue` | `nnue/*`, `evaluate.cpp` | Incremental neural eval |
| `thread` / `threadpool` | `thread.cpp` | Lazy SMP |
| `uci` | `uci.cpp` | Protocol |
| `tb` | `syzygy/*` | Perfect endgame |

---

## 4. Stockfish architecture (the reference model)

Use this section when designing *any* Stockfish-family engine. Reckless is one instance; Stockfish is the canonical C++ one.

### 4.1 Layered architecture

```
┌─────────────────────────────────────────────────────────┐
│  UCI (stdin/stdout)                                      │
│  position / go / setoption / stop / ucinewgame           │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  Engine / ThreadPool                                     │
│  Lazy SMP: N workers, shared TT, per-thread state        │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  Search (iterative deepening → PVS)                      │
│  TT · move picker · history · time · TB                  │
└─────────────┬─────────────────────────────┬─────────────┘
              │                             │
┌─────────────▼─────────────┐ ┌─────────────▼─────────────┐
│  Board / Position         │ │  Evaluation (NNUE)         │
│  bitboards + mailbox      │ │  incremental accumulator   │
│  make/unmake, SEE, threats│ │  quantized SIMD forward    │
└───────────────────────────┘ └───────────────────────────┘
```

**Search is the coordinator.** Board and eval are services. UCI is I/O only.

### 4.2 Why this paradigm wins (vs Lc0 / MCTS)

| | Stockfish-family (AB + NNUE) | Lc0-family (MCTS + deep NN) |
|---|---|---|
| Hardware | CPU, SIMD | GPU preferred |
| Nodes/sec | Millions | Tens of thousands |
| Eval quality per node | Good (shallow NNUE) | Excellent (deep net) |
| Search style | Exhaustive + prune | Selective via PUCT |
| Strength driver | **Depth × speed** | **Policy/value quality** |

Chess has a high branching factor but many *tactically forced* lines. Alpha-beta + quiescence + SEE thrives when you can evaluate *fast*. NNUE was designed exactly for that: sparse, incremental, integer, CPU-native.

**Rule of thumb:** ~1 extra ply ≈ +50–70 Elo. Doubling NPS ≈ ~1 ply. Speed compounds with pruning.

### 4.3 What made Stockfish unique historically

1. **Open-source + Fishtest** — distributed SPRT testing (billions of games). Elo gains come from *measured* patches, not intuition. Reckless uses the same idea via OpenBench.
2. **NNUE adoption (2020)** — +80–100 Elo overnight; handcrafted eval later removed entirely (~2023).
3. **Selective search sophistication** — decades of pruning/extension recipes that shrink branching factor from ~35 to <2.
4. **Lazy SMP** — simple, scalable parallelism via shared TT (not YBWC/ ABDADA complexity).
5. **CPU-first engineering** — cache layout, SIMD, hugepages, NUMA, PGO — every cycle matters at the top.

---

## 5. Board representation (Stockfish-family standard)

Reckless `Board` (and Stockfish `Position`) use the dual representation:

- **Bitboards** (`u64`): piece-type sets, color sets, occupancy — for attacks & generation
- **Mailbox** (`[Piece; 64]`): O(1) piece-on-square queries

Plus a **state stack** for undo: keys, castling, EP, 50-move, threats, pinned/pinners/checkers, material, repetition.

### Critical board features agents should copy

| Feature | Why |
|---|---|
| Incremental Zobrist keys | TT indexing without full rehash |
| Incremental threat maps | History indexing + move scoring + legality |
| Pinned / checkers / checking_squares | Fast legality & check detection |
| SEE | Prune losing captures; order good captures |
| Cuckoo hashing | Detect upcoming repetitions before they appear |
| GHI mitigation | Mix 50-move clock into hash (Reckless buckets every 8 plies) |
| Chess960-aware castling | Rook/king landing squares parameterized |

**Make/unmake** must be reversible and notify NNUE via an observer (`BoardObserver` in Reckless) so threat/PST accumulators stay in sync.

---

## 6. Search architecture (the brain)

### 6.1 Outer loop: iterative deepening + aspiration

```
for depth = 1, 2, 3, ... until time:
    aspirate window around previous score
    score = search<root, PV>(alpha, beta, depth)
    on fail-high/low: widen window / re-search
    report UCI info; decide soft/hard stop
```

**Why ID:** always have a best move; TT fills with useful moves; time can stop mid-iteration.

**Aspiration windows:** search a narrow `[α, β]` around the previous score; re-search on fail. Saves nodes when the score is stable.

### 6.2 Inner loop: Principal Variation Search (PVS)

At each node (typed as Root / PV / NonPV in Reckless):

1. Probe TT → possible cutoff + get TT move
2. Probe Syzygy if applicable
3. Evaluate (or reuse TT eval) + apply **correction history**
4. Apply **forward pruning** (razoring, RFP, NMP, ProbCut) on NonPV/cut nodes
5. **Singular extension** of TT move if it alone fails high
6. Generate/order moves via staged picker
7. For each move: LMP / FP / history / SEE pruning → LMR → zero-window search → full window re-search if needed
8. Update TT, histories, PV

**Node types matter:** PV nodes search carefully; cut nodes prune aggressively. Reckless encodes this with `Root` / `PV` / `NonPV` generics.

### 6.3 Selective search catalog (must-know)

These are the Stockfish-family techniques. Reckless implements essentially the full modern set in `search.rs`.

| Technique | Idea | Typical Elo impact (order of magnitude) |
|---|---|---|
| **TT cutoff** | Reuse prior bound/score/move | Essential |
| **Mate distance pruning** | Bound α/β by mate distance | Small but free |
| **Razoring** | Very low eval → drop to qsearch | Modest |
| **Reverse futility (RFP)** | Eval ≫ β at low depth → return | Large |
| **Null move pruning (NMP)** | Pass move; if still ≥ β, prune (with verification) | Very large |
| **ProbCut** | Capture-heavy shallow search to prove cutoff | Large |
| **Singular extensions** | TT move uniquely good → extend 1–3 plies | Very large |
| **Multi-cut** | Singular search fails high → cutoff | Large |
| **Negative extensions** | Non-singular TT move → reduce | Modest |
| **LMP** | Skip late quiet moves | Large |
| **Futility pruning** | Quiet moves can’t raise α → skip | Large |
| **History pruning** | Low-history quiets skipped | Modest |
| **SEE pruning** | Losing captures skipped | Large |
| **LMR** | Late moves searched reduced; re-search if promising | Very large |
| **PVS** | First move full window; rest null-window | Essential |
| **Quiescence** | At leaves, search captures/checks until quiet | Essential |

**Improving flag:** position better than 2 plies ago → less pruning / different margins. Ubiquitous in modern engines.

### 6.4 Quiescence search

At depth ≤ 0, do not call static eval on a “noisy” position. Search captures (and often checks) until stand-pat or no improving capture. Prevents horizon effect.

Stand-pat: if eval ≥ β, cutoff; else raise α and try captures ordered by MVV/history/SEE.

### 6.5 Move ordering (more important than people think)

Order determines pruning effectiveness. Reckless stages (`movepick.rs`):

1. **TT / hash move**
2. **Good noisy** (captures/promotions that pass SEE)
3. **Quiets** (history + continuation + pawn history + threat/escape/offense heuristics)
4. **Bad noisy** (failing SEE)

History tables (Reckless):

- `QuietHistory` — from→to, threat-conditioned
- `NoisyHistory` — piece→to×captured×threat
- `PawnHistory` — pawn-structure keyed
- `ContinuationHistory` — follow-up of previous moves (1,2,4,6 plies)
- **Correction histories** — adjust raw NNUE toward search truth (pawn / non-pawn / continuation)

**Agent rule:** never generate all moves unsorted. Stage + score + pick-best.

### 6.6 Transposition table

Shared, lock-free, clustered (Reckless: 3 entries / 32-byte cluster).

Each entry stores: move, score, raw eval, depth, bound (Exact/Lower/Upper), age, TT-PV flag.

- Index by high bits of Zobrist (fast multiply-shift, not modulo)
- Verify with low 16 bits
- Mate/TB scores adjusted by ply
- Prefetch on make-move
- Age for replacement; quality ≈ depth − age penalty

TT is the *only* major shared mutable structure in Lazy SMP (plus atomics for nodes/stop). Histories are mostly per-thread; correction history may be NUMA-replicated shared.

### 6.7 Lazy SMP

All threads search the **same root** independently. Divergence comes from:

- Different aspiration / depth offsets
- TT races (nondeterministic move order)
- Helper threads often silent (`Report::None`)

Shared: TT, stop flag, node counters, optionally correction history / NNUE weights (NUMA copies).

**Not** work-stealing split of a single tree. Simple and scales to hundreds of threads (diminishing returns; memory-bound).

Reckless extras: NUMA binding, hugepage TT/history alloc on Linux, sharded node counters (cache-line aligned).

---

## 7. NNUE evaluation (why Stockfish “works so well”)

### 7.1 Design constraints (the three pillars)

1. **Sparse inputs** — only ~30 of tens of thousands of features active (piece-square relative to king, etc.)
2. **Incremental updates** — on make/unmake, add/subtract a few weight columns into an **accumulator**; do not recompute FT from scratch
3. **Quantized SIMD** — int8/int16 (and mixed float in later layers) using AVX2/AVX-512/NEON

Result: eval cost is tiny relative to search, so depth wins.

### 7.2 Topology (Reckless concrete numbers)

From `nnue.rs`:

```
Feature Transformer (incremental):
  PST features  + Threat features
  → L1_SIZE = 768  (per perspective, dual accumulators White/Black)

Forward (dense, per eval):
  activate FT (side-to-move oriented)
  → sparse L1 (nnz) → L2_SIZE = 16 → L3_SIZE = 32 → scalar

Bucketing:
  INPUT_BUCKETS = 10  (king-square layout)
  OUTPUT_BUCKETS = 8  (by piece count)
```

Weights embedded at compile time (`include_bytes!(env!("MODEL"))`). Parameters NUMA-replicated for multi-socket.

Stockfish’s exact layer sizes evolve (SFNNv* versions); the *shape* is always: **wide sparse FT + tiny dense head**.

### 7.3 Incremental stack

Reckless keeps per-ply stacks:

- `PstAccumulator` — piece-square transformer state
- `ThreatAccumulator` — threat features (updated via `BoardObserver` during make)

On `evaluate()`: if king bucket/side-flip invalidates incremental path → full refresh; else replay deltas from last accurate ply.

### 7.4 Post-NNUE corrections

Raw network output is not used alone (`evaluation.rs`):

- Scale by material + **optimism** (search-dependent bias)
- Decay with 50-move clock
- Add **correction history** (learned residual from prior searches)
- Clamp away from TB/mate score range

This is a modern Stockfish-family pattern: NNUE is primary; small adaptive corrections close the gap to search reality.

### 7.5 Training ecosystem

- Stockfish: nnue-pytorch / Fishtest data
- Reckless: [Bullet](https://github.com/jw1912/bullet) trainer
- Data: self-play / search-labeled positions at modest depth, billions of samples
- Quantization after float training for inference

---

## 8. Endgame tablebases

Syzygy WDL/DTZ via Fathom (optional). Used to:

- Rank root moves when few pieces remain
- Cut search when probe is decisive
- Soft-stop probing under load (`stop_probing_tb`)

~10 Elo class improvement when available; not required for the architecture to work.

---

## 9. Time management & UCI

UCI options Reckless exposes (Stockfish-similar):

| Option | Role |
|---|---|
| Hash | TT size MB |
| Threads | Lazy SMP workers |
| MultiPV | Search N best lines |
| MoveOverhead | GUI/network latency reserve |
| SyzygyPath | TB path |
| UCI_Chess960 | FRC |
| Minimal | Quiet UCI output |

Soft vs hard stop: soft allows finishing iteration; hard aborts. Best-move stability and score swings adjust spend (standard modern TM).

---

## 10. Why Stockfish (and Reckless) are strong — causal chain

```
Fast legal move gen (bitboards)
        ↓
Excellent move ordering (TT + history + SEE)
        ↓
Aggressive safe pruning (NMP, LMR, RFP, LMP, ProbCut…)
        ↓
Effective branching factor < 2
        ↓
Depth 30–40+ in middlegame time controls
        ↓
NNUE leaf eval accurate enough at that depth
        ↓
+ Lazy SMP + huge TT + TB
        ↓
+ Continuous SPRT testing of every patch
        ↓
Top of CCRL / CCC / TCEC
```

**Failure modes if you omit pieces:**

| Missing piece | Symptom |
|---|---|
| Bad move ordering | Pruning becomes unsafe or ineffective |
| No quiescence | Tactical blindness / horizon |
| Slow eval | Can’t reach depth; loses to shallower-but-faster engines |
| No TT | Massive re-search waste |
| No testing framework | Elo noise; “improvements” that lose strength |
| GPU-deep net in AB | Too slow; AB needs NPS |

---

## 11. Agent implementation roadmap (Stockfish-family)

Ordered by Elo return for a new engine:

1. **Board + legal movegen + make/unmake** (bitboards when serious)
2. **Negamax alpha-beta + iterative deepening**
3. **Quiescence + MVV-LVA / SEE**
4. **TT + TT-move ordering**
5. **Null move + LMR + basic history**
6. **Aspiration + PVS node types**
7. **RFP, LMP, futility, ProbCut, singular extensions** (tune carefully)
8. **NNUE** (start with a known architecture; train later)
9. **Lazy SMP**
10. **Correction history, threat-aware history, NUMA, PGO, SIMD**
11. **OpenBench / SPRT** — without this you will not climb past ~3200 reliably

Do **not** start with MCTS+transformer unless targeting GPU and Lc0-style play.

---

## 12. Reckless ↔ Stockfish concept index

| Concept | Reckless location | Notes |
|---|---|---|
| Iterative deepening | `search::start` | Aspiration + MultiPV |
| PVS / node types | `search::{Root,PV,NonPV}` | Compile-time specialization |
| Qsearch | `search::qsearch` | Stand-pat + captures |
| NMP / RFP / Razoring / ProbCut | `search::search` mid-section | Cut-node gated |
| Singular / multi-cut | same | Extensions 1–3 or −3 |
| LMR / LMP / FP / SEE prune | move loop in `search` | History-aware |
| Move picker stages | `movepick.rs` | Hash → good noisy → quiet → bad |
| Histories | `history.rs` | Quiet/noisy/pawn/cont + correction |
| TT | `transposition.rs` | 3-entry clusters, age, prefetch |
| NNUE | `nnue.rs` + subdirs | Dual PST+threat FT, 768→16→32 |
| Eval glue | `evaluation.rs` | Material/optimism/50-move/corr |
| Lazy SMP | `threadpool.rs` | Shared TT, per-thread `ThreadData` |
| NUMA | `numa.rs` | Replicate NNUE + corrhist |
| UCI | `uci.rs` | + bench/perft/eval/d/speedtest |
| Syzygy | `tb.rs` + Fathom | Feature-gated |

---

## 13. Mental models for agents

1. **Search quality ≈ ordering × pruning safety × NPS.** Fix ordering before adding exotic pruning.
2. **NNUE is an eval function, not a policy.** It does not suggest moves; search does.
3. **Every constant is empirical.** Margins in Reckless (`1140 * depth * depth / 128`, etc.) are SPRT-tuned — copy structure, not magic numbers blindly.
4. **Correctness first:** perft, bench node counts, no illegal moves, draw/repetition/GHI handled.
5. **Measure Elo with SPRT**, not single games.
6. **Stockfish-family ≠ Stockfish source.** Reckless, Alexandria, Viridithas, Berserk, PlentyChess, Obsidian all share the paradigm with different details.

---

## 14. External references

- Reckless: https://github.com/codedeliveryservice/Reckless
- Stockfish: https://github.com/official-stockfish/Stockfish
- Stockfish architecture reference (this repo): [stockfish.md](./stockfish.md)
- Stockfish architecture overview: https://deepwiki.com/official-stockfish/Stockfish
- NNUE principles: https://github.com/official-stockfish/nnue-pytorch
- Chess Programming Wiki: https://www.chessprogramming.org/
- OpenBench: https://github.com/AndyGrant/OpenBench
- Bullet (NNUE trainer): https://github.com/jw1912/bullet
- Background reading: [Why Stockfish is So Good](https://dev.to/djinn/why-stockfish-is-so-good-and-how-you-could-write-a-chess-engine-2lck)

---

## 15. Quick glossary

| Term | Meaning |
|---|---|
| **PV** | Principal variation — current best line |
| **Cut node** | Expected fail-high node; prune hard |
| **All node** | Expected fail-low; all moves searched |
| **LMR** | Late move reductions |
| **NMP** | Null move pruning |
| **SEE** | Static exchange evaluation |
| **TT** | Transposition table |
| **NNUE** | Efficiently updatable neural network eval |
| **FT** | Feature transformer (first NNUE layer) |
| **Accumulator** | Cached FT activation updated incrementally |
| **Lazy SMP** | Parallel search via shared TT, independent threads |
| **SPRT** | Sequential probability ratio test for Elo patches |
| **GHI** | Graph history interaction (hash ignores path-dependent draw rules) |

---

*Last researched: 2026-07-10 against Reckless `main` and public Stockfish architecture docs.*
