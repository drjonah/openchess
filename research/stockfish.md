# Stockfish Architecture Reference

> **Audience:** agents building, studying, or comparing competitive chess engines.
> **Subject:** [official-stockfish/Stockfish](https://github.com/official-stockfish/Stockfish) — the canonical open-source UCI chess engine (C++, GPL-3.0).
> **Companion doc:** [reckless.md](./reckless.md) covers the same *family* paradigm via a readable Rust engine. Use this file for Stockfish-specific source maps, NNUE topology, and process details.
> **Agent task board:** [tasks.md](./tasks.md) — engine pillars (P1–P8) and parallelizable implementation tasks distilled from this research.
>
> Architecture drifts continuously via Fishtest. Numbers below reflect `master` as researched mid-2026 (Stockfish 18 era / post–threat-input nets). Prefer grepping the live tree over treating constants as frozen.

**Sources:** Stockfish `master`, [README](https://github.com/official-stockfish/Stockfish/blob/master/README.md), [`src/nnue/nnue_architecture.h`](https://github.com/official-stockfish/Stockfish/blob/master/src/nnue/nnue_architecture.h), Chess Programming Wiki, Fishtest, Stockfish NNUE blog.

---

## 1. One-sentence model

**Stockfish = bitboard position + Lazy SMP alpha-beta (PVS) + aggressive selective search + incremental quantized NNUE + shared TT + history/correction heuristics + Fishtest SPRT.**

Uniqueness is not one trick. It is the *integration* of:

1. **Search depth via pruning** — effective branching factor ≪ 2
2. **Eval speed via NNUE** — millions of nodes/sec on CPU SIMD
3. **Empirical tuning at scale** — every functional patch measured on Fishtest

Everything else is engineering: cache layout, NUMA, PGO, hugepages, move ordering.

---

## 2. What makes Stockfish unique

### vs classical hand-crafted evaluation (HCE) engines

| Aspect | Classical engines | Stockfish (today) |
|---|---|---|
| Eval | Hand-tuned terms (mobility, pawn structure, king safety…) | **NNUE only** (classical removed in SF 16, 2023) |
| Tuning | Expert intuition + local tests | Billions of SPRT games on Fishtest |
| Strength driver | Search + HCE quality | Search + network quality + corrections |

Stockfish kept decades of search innovations and replaced the entire static eval with a learned network that still evaluates in microseconds.

### vs Leela Chess Zero / MCTS + deep NN

| | Stockfish | Lc0 |
|---|---|---|
| Search | Alpha-beta + massive pruning | Monte Carlo Tree Search (PUCT) |
| Eval | Small integer NNUE (CPU) | Large neural net (GPU preferred) |
| Nodes/sec | Millions | Tens of thousands |
| Strength driver | **Depth × NPS** | **Policy/value quality** |
| Hardware | Runs anywhere with SIMD | Scales with GPU |

Stockfish pioneered **efficiently updatable NNUE on CPU** while retaining world-class alpha-beta — the hybrid that dominates CPU rankings. Chess has high branching but many tactically forced lines; AB + quiescence + SEE thrives when eval is *fast*.

### Institutional uniqueness

1. **Open source + Fishtest** — distributed SPRT testing; Elo gains are measured, not claimed.
2. **Community velocity** — continuous micro-patches; CONTRIBUTING requires Fishtest for functional changes.
3. **CPU-first engineering** — AVX2/AVX-512/NEON, NUMA replication, large pages, profile-guided builds.
4. **Lazy SMP simplicity** — independent full searches sharing a racy TT; scales without YBWC complexity.

---

## 3. Why it works so well (causal chain)

```
Fast legal movegen (bitboards + magics)
        ↓
Excellent move ordering (TT + histories + SEE)
        ↓
Aggressive safe pruning (NMP, LMR, RFP, ProbCut, singular…)
        ↓
Effective branching factor < 2
        ↓
Depth 30–40+ in typical middlegame time controls
        ↓
NNUE leaf eval accurate enough at that depth
        ↓
+ Correction history closes eval↔search gap
        ↓
+ Lazy SMP + huge TT + Syzygy
        ↓
+ Continuous Fishtest SPRT of every patch
        ↓
Top of CCRL / CCC / TCEC (CPU)
```

**Rule of thumb:** ~1 extra ply ≈ +50–70 Elo. Doubling NPS ≈ ~1 ply. Speed compounds with pruning.

**Failure modes if pieces are missing:**

| Missing piece | Symptom |
|---|---|
| Bad move ordering | Pruning unsafe or ineffective |
| No quiescence | Horizon / tactical blindness |
| Slow eval | Can't reach depth |
| No TT | Massive re-search waste |
| No SPRT testing | Elo noise; false "improvements" |
| Deep GPU net inside AB | Too slow; AB needs NPS |

---

## 4. Repository layout

| Path | Role |
|---|---|
| `README.md` | Overview, compile pointers, Fishtest links |
| `Copying.txt` | **GPL-3.0** |
| `AUTHORS` | Contributors |
| `CONTRIBUTING.md` | Functional changes need Fishtest |
| `CITATION.cff` | Academic citation |
| `.github/` | CI (`stockfish.yml`) |
| `scripts/` | Build/utility helpers |
| `src/` | **All engine code + Makefile** |
| `tests/` | Perft, signature, etc. |
| `*.nnue` | Network weights (embedded in releases; `EvalFile` UCI option) |

Lineage: derived from Glaurung 2.1. Headless UCI engine — no GUI.

---

## 5. `src/` map (agent cheat sheet)

```
src/
├── main.cpp              # init bitboards/attacks → UCIEngine loop
├── engine.cpp/.h         # ★ Engine: owns Position, Options, Threads, TT, NNUE
├── uci.cpp/.h            # UCI protocol
├── ucioption.cpp/.h      # option registry / setoption
├── benchmark.cpp/.h      # bench / speedtest
│
├── types.h               # Value, Depth, Move, Piece, Square…
├── bitboard.cpp/.h       # 64-bit primitives
├── attacks.cpp/.h        # magic bitboard attack tables
├── position.cpp/.h       # ★ Position: FEN, do/undo, Zobrist, pins, checkers
├── score.cpp/.h          # cp / mate / WDL formatting
│
├── search.cpp/.h         # ★ brain (~2k+ lines): ID, PVS, all pruning/extensions
├── movegen.cpp/.h        # generate<CAPTURES|QUIETS|EVASIONS|LEGAL|…>
├── movepick.cpp/.h       # staged MovePicker
├── history.h             # butterfly, capture, continuation, pawn, correction
├── tt.cpp/.h             # shared transposition table
├── timeman.cpp/.h        # clock budget
├── thread.cpp/.h         # ThreadPool, Lazy SMP workers
├── numa.h                # NUMA bind + network replication
├── shm.h / shm_linux.h   # shared-memory NNUE copies
├── memory.cpp/.h         # large-page / aligned alloc
├── misc.cpp/.h           # utilities, version, sync I/O
├── tune.cpp/.h           # SPSA / Fishtest tunable hooks
├── perft.h               # move-count verification
│
├── evaluate.cpp/.h       # thin glue: NNUE + optimism / material / 50-move
├── nnue/                 # ★ network, FT, accumulator, SIMD, features, layers
├── syzygy/               # tablebase probe
├── incbin/               # embed .nnue at compile time
├── universal/            # fat binary / runtime CPU dispatch
└── Makefile              # build, PGO, net download, arch flags
```

### Module roles

| File / dir | Responsibility |
|---|---|
| `engine.*` | Central owner: position, options, threads, TT, network |
| `search.*` | Iterative deepening, PVS, selective search, qsearch |
| `position.*` | Board state machine |
| `movepick.*` | Ordered move iteration |
| `history.h` | Ordering + LMR + eval correction tables |
| `tt.*` | Shared bound/move/score cache |
| `evaluate.*` + `nnue/` | Static evaluation |
| `thread.*` / `numa.h` | Lazy SMP + NUMA |
| `uci.*` | Protocol I/O only |
| `syzygy/` | Perfect endgame probes |

**Search is the coordinator.** Position and eval are services. UCI is I/O.

---

## 6. Layered architecture

```
┌─────────────────────────────────────────────────────────┐
│  UCIEngine (uci.cpp)                                     │
│  position / go / setoption / stop / ucinewgame / bench   │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  Engine (engine.cpp)                                     │
│  Position · OptionsMap · ThreadPool · TT · NNUE Network  │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  ThreadPool — Lazy SMP                                   │
│  Thread[0..N] → Search::Worker (histories, accumulators) │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────┐
│  search.cpp — iterative deepening → search<Root|PV|NonPV>│
│  TT · MovePicker · history · timeman · Syzygy            │
└─────────────┬─────────────────────────────┬─────────────┘
              │                             │
┌─────────────▼─────────────┐ ┌─────────────▼─────────────┐
│  position / movegen       │ │  evaluate → nnue/          │
│  bitboards, attacks, SEE  │ │  AccumulatorStack + net    │
└───────────────────────────┘ └───────────────────────────┘
```

---

## 7. Data flow: position → best move

```
GUI ──UCI──► UCIEngine::loop()
                │
                ▼
         Engine::set_position(fen, moves)
         Engine::go(limits)
                │
                ▼
    ThreadPool::start_thinking()
    ├── root Position on all threads
    ├── RootMoves (legal) + optional Syzygy rank
    └── wake Workers
                │
                ▼
    Worker::start_searching()  [each thread]
    ├── main: TimeManagement::init(); tt.new_search()
    └── iterative_deepening()
                │
                ▼
    for depth = 1 .. MAX_PLY:
    ├── aspiration window around prior score
    ├── search<Root>(α, β, depth)
    │     ├── TT probe → cutoff / TT move
    │     ├── evaluate(pos) → NNUE + corrections
    │     ├── forward pruning (razor, RFP, NMP, ProbCut…)
    │     ├── MovePicker stages
    │     ├── for each move: do_move → LMR/extensions →
    │     │     search child (PVS) → undo_move
    │     └── TT store
    ├── time check / UCI info
    └── …
                │
                ▼
    ThreadPool::get_best_thread()  [vote]
                │
                ▼
    bestmove <m> [ponder <m>]
```

---

## 8. Board representation (`position.*`)

Dual representation (Stockfish-family standard):

- **Bitboards** — piece-type / color / occupancy sets for attacks and generation
- **Mailbox** — O(1) piece-on-square
- **State stack** — undo: Zobrist keys, castling, EP, 50-move, checkers/pins, repetition

Supporting pieces:

| Component | Files | Why |
|---|---|---|
| Magic attacks | `attacks.*` | Fast slider attacks |
| Incremental Zobrist | `position.*` | TT keys without full rehash |
| Pins / checkers | `position.*` | Legality and evasion gen |
| SEE | used from search/movepick | Prune/order captures |
| Chess960 | UCI `UCI_Chess960` | Parameterized castling |

`do_move` / `undo_move` must stay reversible and keep NNUE dirty-feature tracking consistent.

---

## 9. Search architecture (`search.cpp`)

### 9.1 Outer loop: iterative deepening + aspiration

```
for depth = 1, 2, 3, ... until time:
    aspirate window around previous score
    score = search<Root>(α, β, depth)
    on fail-high/low: widen / re-search
    report UCI info; soft/hard stop via TimeManagement
```

**Why ID:** always have a move; TT fills with useful entries; time can stop mid-iteration.

**Aspiration:** narrow `[α, β]` around expected score; widen on fail. Saves nodes when stable.

**MultiPV:** independent root searches per PV line when `MultiPV > 1`.

### 9.2 Inner loop: PVS + node types

`search()` is templated on `NodeType`: `Root` | `PV` | `NonPV`.

Typical node:

1. TT probe → cutoff / TT move
2. Syzygy probe when applicable
3. Static eval (or TT eval) + **correction history**
4. Forward pruning on NonPV/cut nodes (razoring, RFP, NMP, ProbCut)
5. Singular extension / multi-cut / negative extension on TT move
6. Staged `MovePicker`
7. Per move: LMP / futility / history / SEE prune → LMR → null-window → full re-search if needed
8. Update TT, histories, PV

PV nodes search carefully; cut nodes prune hard.

### 9.3 Selective search catalog

| Technique | Where | Idea |
|---|---|---|
| **Alpha-beta / PVS** | `search()` | First move full window; siblings zero-window + re-search |
| **TT cutoff** | TT probe | Reuse bound/score/move |
| **Mate distance pruning** | early search | Bound by mate distance |
| **Razoring** | step ~7 | Very low eval → qsearch |
| **Reverse futility (RFP)** | forward prune | Eval ≫ β at low depth → return |
| **Null move pruning** | step ~9 | Pass move; if still ≥ β, prune (+ verification) |
| **ProbCut** | step ~10 | Shallow capture search to prove cutoff |
| **Singular extensions** | step ~15 | TT move uniquely good → extend |
| **Multi-cut** | step ~15 | Excluding TT move still fails high → prune |
| **Negative extensions** | step ~15 | Non-singular TT move → reduce |
| **LMR** | move loop | Late moves reduced; re-search on fail-high |
| **Futility / LMP** | move loop | Skip quiets that can't raise α |
| **SEE pruning** | movepick + search | Skip losing captures |
| **IIR** | search | Reduce when no TT move |
| **Quiescence** | `qsearch()` | Captures/checks until quiet; stand-pat |

**Improving flag:** position better than ~2 plies ago → less aggressive pruning. Ubiquitous.

### 9.4 Quiescence

At depth ≤ 0, do not trust static eval on a noisy position. Search captures (and often checks) until stand-pat or no improving capture. Prevents horizon effect.

### 9.5 Move ordering (`movepick.cpp`)

Order determines whether pruning is safe. Stages (simplified):

| Stage | Content |
|---|---|
| `MAIN_TT` | TT / hash move first |
| `GOOD_CAPTURE` | MVV-LVA + capture history + SEE |
| `GOOD_QUIET` | butterfly + continuation + low-ply history |
| `BAD_CAPTURE` / `BAD_QUIET` | remainder |
| `EVASION_*` / `QSEARCH_*` / `PROBCUT_*` | specialized |

Histories live in `history.h`: main (butterfly), capture, continuation, pawn, low-ply, plus **correction histories** (pawn / minor / non-pawn / continuation) that adjust static eval toward search truth.

**Agent rule:** never generate all moves unsorted. Stage → score → pick best.

### 9.6 Key search structures (`search.h`)

- `Stack` — per-ply: PV, continuation histories, static eval, move count, reductions
- `RootMove` — score, PV, TB rank, effort / aspiration stats
- `SearchManager` — main-thread time + UCI callbacks + ponder
- `Worker` — per-thread search state, histories, accumulator stack, reductions table

---

## 10. NNUE evaluation (current master)

Classical HCE is **gone**. All static eval is NNUE + post-processing in `evaluate.cpp`.

### 10.1 Design pillars

1. **Sparse inputs** — few active features of a huge space (king-relative pieces + threats)
2. **Incremental updates** — add/subtract weight columns into an **accumulator** on make/unmake
3. **Quantized SIMD** — int8/int16 vector math (`nnue/simd.h`: AVX2 / AVX-512 / NEON / …)

Eval cost stays tiny relative to search → depth wins.

### 10.2 Feature sets

| Feature set | File | Role |
|---|---|---|
| **HalfKAv2_hm** | `nnue/features/half_ka_v2_hm.*` | King + piece-square, horizontal mirror, king buckets |
| **FullThreats** | `nnue/features/full_threats.*` | Attack/threat interactions (~60k dims, incremental dirty tracking) |

HalfKAv2_hm: king-centric features; king always mirrored to e–h files; king moves refresh accumulator; piece moves update incrementally.

### 10.3 Network topology (`nnue_architecture.h`)

```
Input:
  HalfKAv2_hm (PSQFeatureSet) + FullThreats (ThreatFeatureSet)
        ↓
Feature Transformer → L1 = 1024  (per side; clipped pairwise product)
        ↓
Layer stacks × 8  (bucket = (piece_count - 1) / 4)
  fc_0:  AffineTransformSparseInput<1024, 32>     # L2 = 32
  ac_sqr_0 + ac_0: SqrClippedReLU + ClippedReLU → concat 64
  fc_1:  AffineTransform<64, 32>                  # L3 = 32
  ac_sqr_1 + ac_1: → concat
  fc_2:  AffineTransform<…, 1> → scalar positional

PSQT buckets: 8 (material-dependent)
```

`Network::evaluate()` returns `{psqt, positional}`; `evaluate.cpp` blends with optimism, material scaling, and 50-move dampening.

Layer sizes evolve (SFNNv* versions). Shape is always: **wide sparse FT + tiny dense head**.

### 10.4 Accumulator & Finny caches

`nnue/nnue_accumulator.*`:

- `AccumulatorStack` — per-ply states (`MAX_PLY + 1`); push/pop with search
- Incremental forward/backward updates from dirty features
- `find_last_usable_accumulator()` — walk back to a computed state, then replay
- **Finny tables** (`AccumulatorCaches`) — per-thread cache by king square; refresh by diffing from a cached reference instead of full bias rebuild

### 10.5 Inference path

1. Update accumulators
2. `FeatureTransformer::transform()` → clipped sparse L1 input
3. `NetworkArchitecture::propagate()` → sparse affine → activations → scalar
4. Select layer stack + PSQT bucket by piece count
5. Scale to internal centipawn-like units

### 10.6 Training / data

- Networks trained on search-labeled positions (and LC0 training data under **ODbL**)
- Tools historically: nnue-pytorch / related trainers
- Default net downloaded via `make net` or shipped embedded; UCI `EvalFile` overrides

---

## 11. Transposition table (`tt.cpp` / `tt.h`)

- **Single global TT**, shared across threads — intentionally **racy** for speed
- Cluster addressing by Zobrist key
- **~10-byte entries:** key fragment, depth, gen/bound/pv flags, move, value, eval
- Bounds: `BOUND_EXACT` | `BOUND_UPPER` | `BOUND_LOWER`
- Generation aging on `new_search()`; prefer replacing stale / shallower entries
- `hashfull` — permille occupancy for UCI

TT is the primary shared mutable structure in Lazy SMP (plus stop/node atomics). Histories are mostly per-thread; some correction/pawn tables may be NUMA-shared.

---

## 12. Lazy SMP (`thread.cpp`, `numa.h`)

Each thread runs an **independent full search** from the same root. Divergence from TT races, depth offsets, and nondeterministic ordering.

```
start_thinking()
  → main Worker::start_searching()
  → wake helpers
  → each Worker::iterative_deepening()
  → get_best_thread() votes on RootMoves
```

**Vote selection:** weight scores (prefer decisive shorter mates); not "first thread wins."

**NUMA:**

- `NumaPolicy` UCI option (`auto` / `system` / `none` / custom)
- Bind threads to nodes; replicate NNUE weights via shared memory (`shm.h`)
- Shared histories per node where beneficial; Finny caches per thread

**Not** work-stealing split of one tree. Simple; scales to many cores with diminishing returns (memory-bound).

Main thread (idx 0) owns `SearchManager` (time + UCI info); helpers get a null manager.

---

## 13. Time management (`timeman.cpp`)

`TimeManagement::init()` sets `optimumTime` / `maximumTime` from:

- Base clock + increment (or moves-to-go session)
- `Move Overhead` latency reserve
- Ply-dependent scales; ponder adds ~25% to optimum

During ID: `fallingEval`, `bestMoveChanges`, `timeReduction` adapt spend. Soft stop finishes useful work; hard stop aborts.

---

## 14. Syzygy (`syzygy/tbprobe.*`)

When `SyzygyPath` is set:

- Rank root moves in TB positions
- Cut search on decisive probes
- Limits via `SyzygyProbeDepth` / `SyzygyProbeLimit`

Meaningful Elo when available; not required for the architecture.

---

## 15. UCI surface (high-signal options)

| Option | Default (approx.) | Purpose |
|---|---|---|
| `Threads` | 1 | Lazy SMP workers |
| `Hash` | 16 MB | TT size |
| `NumaPolicy` | auto | NUMA placement |
| `MultiPV` | 1 | Multiple PVs |
| `Ponder` | false | Think on opponent time |
| `Move Overhead` | 10 ms | Latency reserve |
| `Skill Level` / `UCI_LimitStrength` / `UCI_Elo` | — | Strength limiting |
| `UCI_ShowWDL` | false | WDL output |
| `EvalFile` | `nn-….nnue` | Network path |
| `SyzygyPath` | "" | Tablebases |
| `UCI_Chess960` | false | FRC |

Commands: `uci`, `isready`, `ucinewgame`, `position`, `go`, `stop`, `ponderhit`, `setoption`, plus debug helpers (`d`, `eval`, `bench`).

---

## 16. Build system

```bash
cd src
make -j profile-build   # recommended (PGO)
make net                # download default .nnue
make help               # arches / targets
```

- Auto-detects CPU (`x86-64-vnni256`, `armv8-dotprod`, …)
- Universal / fat binaries under `universal/` with runtime dispatch
- NNUE embed via `incbin` / embed sources
- Cross-compile: mingw, Android NDK, WASM, etc.

---

## 17. Fishtest — why strength keeps rising

- [tests.stockfishchess.org](https://tests.stockfishchess.org/tests)
- **SPRT** accept/reject for patches
- Functional PRs need linked Fishtest results + new bench signature
- `tune.cpp` hooks SPSA parameter optimization
- Volunteers run [Fishtest Worker](https://github.com/official-stockfish/fishtest)

Without a testing framework like this (or OpenBench), climbing past mid-tier Elo is unreliable.

---

## 18. Evolution timeline

| Era | Change |
|---|---|
| Pre-2020 | Classical HCE + selective search dominance |
| SF 12 (2020) | **NNUE introduced** (+~80–100 Elo); hybrid with classical |
| 2020–2023 | Hybrid refinements; nets grow; HalfKA → HalfKAv2_hm |
| SF 16 (2023) | Classical eval **removed**; NNUE-only |
| SF 17–18 / master | Threat inputs → **FullThreats**; L1/L2 reshapes (e.g. L1=1024, L2=32); correction history; `Engine` refactor; NUMA replication; universal binaries |

---

## 19. License

- Engine: **GPL-3.0** (`Copying.txt`) — copyleft; distribute source with binaries
- LC0 training data used for nets: **ODbL** (separate from GPL)

---

## 20. Mental models for agents

1. **Search quality ≈ ordering × pruning safety × NPS.** Fix ordering before exotic pruning.
2. **NNUE is an eval, not a policy.** It scores positions; search chooses moves.
3. **Every margin is empirical.** Copy structure from Stockfish; do not cargo-cult constants without SPRT.
4. **Correctness first:** perft, bench signatures, legality, draws/repetition/GHI.
5. **Measure Elo with SPRT**, not anecdotes.
6. **Stockfish-family ≠ Stockfish source.** Reckless, Alexandria, Viridithas, Berserk, etc. share the paradigm with different details — see [reckless.md](./reckless.md).

---

## 21. Agent implementation roadmap (Stockfish-family)

Ordered by Elo return for a new engine:

1. Board + legal movegen + make/unmake (bitboards when serious)
2. Negamax alpha-beta + iterative deepening
3. Quiescence + MVV-LVA / SEE
4. TT + TT-move ordering
5. Null move + LMR + basic history
6. Aspiration + PVS node types
7. RFP, LMP, futility, ProbCut, singular extensions (tune carefully)
8. NNUE (known architecture first; train later)
9. Lazy SMP
10. Correction history, threat-aware history, NUMA, PGO, SIMD
11. OpenBench / Fishtest-style SPRT

Do **not** start with MCTS + transformer unless targeting GPU / Lc0-style play.

---

## 22. Quick glossary

| Term | Meaning |
|---|---|
| **PV** | Principal variation — current best line |
| **Cut / All node** | Expected fail-high / fail-low |
| **LMR** | Late move reductions |
| **NMP** | Null move pruning |
| **SEE** | Static exchange evaluation |
| **TT** | Transposition table |
| **NNUE** | Efficiently updatable neural network eval |
| **FT** | Feature transformer (first NNUE layer) |
| **Accumulator** | Cached FT state updated incrementally |
| **Lazy SMP** | Parallel independent searches + shared TT |
| **SPRT** | Sequential probability ratio test for patches |
| **GHI** | Graph history interaction (path-dependent draws vs hash) |
| **Finny table** | King-square accumulator cache for faster refresh |
| **HalfKAv2_hm** | King-relative piece features with horizontal mirror |
| **FullThreats** | Threat/attack feature set in modern SF nets |

---

## 23. External references

- Stockfish repo: https://github.com/official-stockfish/Stockfish
- Stockfish wiki: https://github.com/official-stockfish/Stockfish/wiki
- Introducing NNUE: https://stockfishchess.org/blog/2020/introducing-nnue-evaluation/
- Chess Programming Wiki — Stockfish: https://www.chessprogramming.org/Stockfish
- Chess Programming Wiki — NNUE: https://www.chessprogramming.org/NNUE
- Fishtest: https://tests.stockfishchess.org/tests
- UCI protocol: https://backscattering.de/chess/uci/
- Family companion (Reckless): [reckless.md](./reckless.md)

---

*Last researched: 2026-07-10 against official-stockfish/Stockfish `master` (NNUE: HalfKAv2_hm + FullThreats, L1=1024, L2=L3=32).*
