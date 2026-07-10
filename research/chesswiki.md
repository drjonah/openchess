# Chess Engine Core Ideas (from Chess Programming Wiki)

Research dump distilled from [Chess Programming Wiki](https://www.chessprogramming.org/Main_Page) — the ideas that actually make a strong engine, not a catalog of every page.

**Agent task board:** [tasks.md](./tasks.md) — engine pillars (P1–P8) and parallelizable implementation tasks distilled from this research (with [reckless.md](./reckless.md) / [stockfish.md](./stockfish.md)).

Primary sources: [Getting Started](https://www.chessprogramming.org/Getting_Started), [Board Representation](https://www.chessprogramming.org/Board_Representation), [Search](https://www.chessprogramming.org/Search), [Search Progression](https://www.chessprogramming.org/Search_Progression), [Evaluation](https://www.chessprogramming.org/Evaluation), [NNUE](https://www.chessprogramming.org/NNUE), [Move Ordering](https://www.chessprogramming.org/Move_Ordering), [Transposition Table](https://www.chessprogramming.org/Transposition_Table), [Null Move Pruning](https://www.chessprogramming.org/Null_Move_Pruning), [Late Move Reductions](https://www.chessprogramming.org/Late_Move_Reductions), [Syzygy Bases](https://www.chessprogramming.org/Syzygy_Bases), [Engine Testing](https://www.chessprogramming.org/Engine_Testing), [UCI](https://www.chessprogramming.org/UCI).

---

## 0. The three pillars

A chess engine is three cooperating systems:

1. **Board representation** — legal state, move gen, make/unmake
2. **Search** — look ahead through the game tree under time pressure
3. **Evaluation** — score leaf (and some interior) positions

Everything else (books, tablebases, protocols, testing) exists to feed, constrain, or measure those three.

Shannon (1949) framed the problem: Type A = brute-force to fixed depth; Type B = selective “important” branches. Modern engines are mostly Type A with heavy Type-B-style selectivity (pruning/reductions/extensions).

---

## 1. Board representation (the foundation)

### Must be correct first

- Full chess rules: castling, en passant, promotions, check, stalemate, 50-move, threefold repetition
- State beyond piece placement: side to move, castling rights, EP square, halfmove clock
- **Perft** is the correctness gate for move generation — do not build search on a buggy board

### Representation families

| Style | Idea | Notes |
| --- | --- | --- |
| **Square-centric** | Array of piece codes (mailbox, 0x88, 10×12) | Simple; piece-by-square is O(1) |
| **Piece-centric** | Lists/sets of pieces → squares | Bitboards are the dominant form |
| **Hybrid** | Both | Common: bitboards + 8×8 piece array |

### Bitboards (modern default)

- One 64-bit word per piece type / color / occupancy
- Fast set ops for attacks, blockers, pins, SEE helpers
- Sliding attacks: **magic bitboards** (or PEXT/BMI2) — perfect-hash blocker configs to precomputed attack sets
- Incremental make/unmake must stay cheap; Zobrist keys update with XOR on each change

### Make / unmake

- Search lives or dies on reversible move application
- Store undo info (captured piece, rights, EP, hash) so unmake is exact
- Illegal-move filtering: either generate only legal, or generate pseudo-legal and reject moves that leave king in check

---

## 2. Search — the engine’s brain

### Baseline stack (start here)

From Getting Started / Search Progression:

1. **Negamax** + **alpha-beta** (prefer fail-soft)
2. **Iterative deepening** — time control + move ordering from previous depth
3. **Quiescence search (QS)** — extend captures/checks so leaves are “quiet”
4. **Transposition table** — Zobrist hash; store score + bound + draft + best move
5. **Principal Variation Search (PVS)** — full window on first move, null-window scouts after
6. **Aspiration windows** — narrow root window; widen on fail-high/low

Without QS, the **horizon effect** wrecks tactics. Without a TT + iterative deepening, alpha-beta is far from the minimal tree.

### Move ordering (makes alpha-beta work)

Alpha-beta’s speed ≈ how often the best move is tried first. Typical order:

1. PV / TT (hash) move
2. Good captures / promotions (MVV-LVA, then SEE)
3. Killer moves (quiet cutters)
4. History-ordered quiets (butterfly / relative history)
5. Losing captures (policy varies)

At expected **Cut-nodes**, fail-high on the first move >90% of the time in strong engines — hash move alone often causes ~75% of those cutoffs (Stockfish-class observation on CPW).

### Selectivity — where most Elo lives

CPW’s “Connorpasta” / Improved Connorpasta progression (majority of top-engine Elo):

| Technique | Idea |
| --- | --- |
| **Null Move Pruning (NMP)** | Pass a turn; if reduced null-window search still ≥ β, prune. Disable in check / bare K+P (zugzwang). Reduction R often depth- and eval-dependent |
| **Reverse Futility / Static NMP (RFP)** | If static eval − margin ≥ β, fail high without searching |
| **Late Move Reductions (LMR)** | Late, poorly ordered moves searched shallower; re-search if they beat α. Log-depth × log-move formulas are standard |
| **Late Move Pruning (LMP)** | Skip late quiets entirely at shallow depths |
| **Futility pruning** | Near leaves, skip quiets that can’t raise α given static eval |
| **History heuristic** | Reward quiet moves that cause cutoffs; feed LMR/LMP/ordering |
| **Continuation history (CMH/FMH)** | History conditioned on previous move(s) |
| **Capture history** | Same idea for captures |
| **SEE pruning** | Drop losing captures in QS and main search |
| **Delta pruning (QS)** | Skip captures that can’t improve α even optimistically |
| **History pruning** | Skip quiets with terrible history scores |
| **Internal Iterative Reduction (IIR)** | No TT move → reduce depth slightly (or deepen to find one) |
| **Improving** | Eval better than 2 plies ago → prune/reduce less aggressively |
| **Singular extensions** | TT move uniquely good → extend it |
| **Multicut** | Several moves refute → prune earlier |
| **Double/triple/negative extensions** | Extend more/less based on singular/cutnode signals |
| **Cutnode** | Expected fail-high node; bias reductions/extensions |
| **Static eval correction history** | Learn eval bias by position features |

**Rule of thumb:** add one feature at a time, then **tune parameters** with SPRT. Implementation details matter as much as the idea name.

### Extensions (classic)

- Check extensions
- Recapture / singular / passed-pawn extensions (engine-dependent)
- Mate threat detection (sometimes via null-move)

### Quiescence

- Search captures (and often checks) until quiet
- Order with MVV-LVA / SEE; prune with SEE and delta
- Do **not** probe heavy tablebases in QS (Syzygy guidance)

### Parallel search

- Lazy SMP / shared TT is the practical modern approach
- Harder to debug; get sequential strength first

### Time management

- Simple effective formula: `remaining/20 + increment/2` as a soft cap
- Soft bound (per ID iteration) vs hard bound (must stop)
- Soft-bound progression: node scaling → best-move stability → eval stability
- **Pondering** (think on opponent’s time) when protocol allows

---

## 3. Evaluation

### Purpose

Eval approximates win chance so search can compare positions. Perfect play would only need {−1,0,+1}; reality needs a continuous score **relative to side to move** (Negamax).

Shannon’s classic sketch: material + pawn structure penalties + mobility.

### Hand-crafted evaluation (HCE)

Core terms:

- **Material** (dominant)
- **Piece-square tables (PSTs)**
- **Pawn structure** (doubled, isolated, passed, connected, …)
- **Mobility**, center control, space, tempo
- **King safety** (attack units, shelter, open files)
- Piece quality (outposts, bad bishops, trapped pieces)
- **Tapered eval** — interpolate middlegame ↔ endgame by game phase

Keep features as independent as possible for tuning. Start with material + PSTs; grow carefully.

### NNUE (modern strength ceiling)

[NNUE](https://www.chessprogramming.org/NNUE) = Efficiently Updatable Neural Networks (Yu Nasu / Shogi → Stockfish 2020).

Why it wins:

- Dense positional judgment without hand-authored features
- **Incremental accumulator updates** on make/unmake (only a few input features change)
- Dual perspective (STM / NSTM) concatenated into the first hidden layer
- Integer SIMD inference (int8/int16) keeps NPS competitive despite heavier eval

Basic shape: sparse binary inputs (piece×square×color, often king-bucketed) → wide accumulator → small MLP → scalar score.

Training: supervised from search/self-play data + RL-style refinement. Net quality dominates architecture tweaks once inference is correct.

**Practical path:** HCE to get a working engine → NNUE when search/testing infrastructure is solid.

---

## 4. Memory & hashing

### Zobrist hashing

- Random 64-bit keys for (piece, square), side, castling, EP
- Incremental XOR on make/unmake
- Index = low bits of key; store full key (or upper bits) to detect collisions

### Transposition table contents

- Key, depth (draft), score + bound type (exact / lower / upper)
- Best / refutation move
- Age for replacement across moves

Uses: cutoffs when draft and bound allow; always use hash move for ordering even when draft is shallow.

Replacement: always-replace vs depth-preferred vs multi-bucket — tune empirically.

---

## 5. Knowledge outside the search

### Opening book

- Polyglot / custom books avoid burning time on theory
- UCI GUIs may own the book; engines may also have `OwnBook`
- For testing strength of *search+eval*, use fixed opening suites with color-reversed pairs

### Endgame tablebases (Syzygy)

- **WDL** during search (win/draw/loss under 50-move rule)
- **DTZ** at root to convert wins without unnatural play
- Compact vs Nalimov/Gaviota; 6-man practical, 7-man huge
- Probe in main search (SSD-friendly); skip QS
- Elo gain shrinks as NNUE gets stronger, but still useful for perfect conversion

---

## 6. Protocols & product surface

### UCI (standard)

- Stateless engine: GUI owns game model; engine gets `position` + `go`
- Enables GUIs, cutechess-cli, OpenBench, Fishtest
- Minimal subset is enough to test: `uci`, `isready`, `position`, `go`, `stop`, `bestmove`, info lines
- Time management must interpret `wtime`/`btime`/`winc`/`binc`/`movestogo` carefully

### GUI / CLI ecosystem

- Cute Chess, Arena, etc. for play
- cutechess-cli / fast-chess / OpenBench for science

---

## 7. Scientific development (non-negotiable for strength)

From Getting Started + Engine Testing:

1. **Bug-free board** via Perft and regression suites
2. **UCI + time management** so you can run matches
3. **SPRT self-play** to accept/reject patches (modern gold standard)
4. Do **not** tune primarily on tactical test suites — they don’t track Elo well
5. Match conditions: fixed openings, color reverse, enough games for LOS/Elo error bars
6. Search changes are TC-sensitive; eval/nps changes can use fixed nodes for isolation
7. Frameworks: [OpenBench](https://www.chessprogramming.org/OpenBench), Fishtest-style distributed testing

Many engines stall on rating lists because of **no or bad testing**, not missing ideas.

---

## 8. Recommended build order (synthesis)

### Phase A — Correct skeleton

1. Board + legal move gen + make/unmake
2. Perft suite green
3. Negamax αβ + material eval
4. Iterative deepening + crude time manager
5. UCI enough to play and match

### Phase B — Search that scales

1. QS + TT + PVS + aspiration
2. Move ordering: TT → MVV-LVA → killers → history
3. NMP → LMR → RFP → LMP → futility
4. SEE in QS/main search; history refinements
5. Singular extensions and friends once the above is stable

### Phase C — Eval & knowledge

1. Expand HCE (PSTs, pawns, king safety, tapered)
2. Or jump to NNUE once data/training pipeline exists
3. Opening book + Syzygy when search is trustworthy

### Phase D — Strength process

1. Automated SPRT on every meaningful patch
2. Parameter tuning after each feature
3. Soft/hard time bounds, pondering, SMP last

---

## 9. Idea map (what “success” actually is)

```
Correctness (board/perft)
    → Speed of legal nodes (bitboards/magics/make-unmake)
        → Alpha-beta efficiency (ordering + TT + ID)
            → Selectivity (NMP/LMR/futility/extensions)
                → Leaf quality (HCE → NNUE)
                    → Endgame certainty (Syzygy)
                        → Measured Elo (SPRT/OpenBench)
```

A successful engine is not the one with the longest feature list. It is the one that:

- never lies about legality,
- searches a near-minimal tree under the clock,
- scores positions in a way that correlates with winning,
- and only keeps changes that SPRT says gain Elo.

---

## 10. Hot topics (CPW main page)

Current community gravity wells: **Stockfish**, **NNUE**, **Leela Chess Zero** (MCTS/NN, different paradigm), **Syzygy**.

For a classical alpha-beta engine (most hobby/open engines), the Elo path is still: bitboards → PVS/TT/QS → Connorpasta selectivity → NNUE → rigorous testing.

---

## Quick reference links

| Topic | URL |
| --- | --- |
| Main | https://www.chessprogramming.org/Main_Page |
| Getting Started | https://www.chessprogramming.org/Getting_Started |
| Board Representation | https://www.chessprogramming.org/Board_Representation |
| Search | https://www.chessprogramming.org/Search |
| Search Progression | https://www.chessprogramming.org/Search_Progression |
| Evaluation | https://www.chessprogramming.org/Evaluation |
| NNUE | https://www.chessprogramming.org/NNUE |
| Move Ordering | https://www.chessprogramming.org/Move_Ordering |
| Transposition Table | https://www.chessprogramming.org/Transposition_Table |
| Null Move Pruning | https://www.chessprogramming.org/Null_Move_Pruning |
| Late Move Reductions | https://www.chessprogramming.org/Late_Move_Reductions |
| Syzygy Bases | https://www.chessprogramming.org/Syzygy_Bases |
| Engine Testing | https://www.chessprogramming.org/Engine_Testing |
| UCI | https://www.chessprogramming.org/UCI |
| Opening Book | https://www.chessprogramming.org/Opening_Book |
| Bitboards | https://www.chessprogramming.org/Bitboards |
| Magic Bitboards | https://www.chessprogramming.org/Magic_Bitboards |
