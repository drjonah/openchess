# OpenChess Architecture

> **Language:** Rust  
> **Paradigm:** Stockfish-family — bitboards + Lazy SMP alpha-beta (PVS) + selective search + incremental NNUE + shared TT + history/corrections + SPRT  
> **Task board:** [research/tasks.md](research/tasks.md)  
> **Research:** [research/chesswiki.md](research/chesswiki.md) · [research/reckless.md](research/reckless.md) · [research/stockfish.md](research/stockfish.md) · [research/LICHESS.md](research/LICHESS.md) (P9)

This document is the Rust-facing blueprint. Agents implement against [research/tasks.md](research/tasks.md); this file defines **where code lives**, **who owns what**, and **how data flows**.

---

## 1. One-sentence model

**OpenChess = bitboard `Board` + Lazy SMP PVS search + selective pruning + incremental NNUE + shared TT + history heuristics + UCI + TUI + SPRT.**

Search coordinates. Board and eval are services. **UCI** is the machine protocol (cutechess/OpenBench). **TUI** is the human-facing terminal UI. Both are thin I/O fronts over the same lib API.

---

## 2. Design principles

1. **Correctness before Elo** — perft gates board work; no search on a buggy move generator.
2. **Search quality ≈ ordering × pruning safety × NPS** — fix move ordering before exotic pruning.
3. **NNUE is an eval, not a policy** — it scores positions; search chooses moves.
4. **Copy structure, not constants** — Reckless/Stockfish margins are SPRT-tuned for their nets.
5. **One crate, clear modules** — start as a single package (`openchess` lib + binary). Split crates only when build times or feature isolation demand it.
6. **Idiomatic Rust, hot paths stay simple** — prefer clarity until benches justify `unsafe`, SIMD, or lock-free tricks.
7. **Feature flags for optional weight** — e.g. `syzygy` behind Cargo features.

---

## 3. Crate layout

```
openchess/
├── Cargo.toml              # bin + lib; features: syzygy, (later) simd targets
├── ARCHITECTURE.md         # this file
├── AGENTS.md
├── README.md
├── research/               # design research + tasks.md
└── src/
    ├── main.rs             # binary: init → `tui` | `lichess` | `uci` (default)
    ├── lib.rs              # module graph + public run()
    │
    ├── types/              # P1 — vocabulary
    │   ├── mod.rs
    │   ├── bitboard.rs
    │   ├── color.rs
    │   ├── piece.rs
    │   ├── square.rs
    │   ├── moves.rs        # Move encoding
    │   ├── score.rs        # Value / mate distance helpers
    │   └── zobrist.rs      # key tables + Key type
    │
    ├── board/              # P1 — position state machine
    │   ├── mod.rs          # Board, State, dual bitboard+mailbox
    │   ├── makemove.rs     # make/unmake + BoardObserver hooks
    │   ├── movegen.rs      # legal / noisy / quiet / evasions
    │   ├── see.rs          # static exchange evaluation
    │   └── parser.rs       # FEN + UCI move parse
    │
    ├── lookup.rs           # P1 — magic/attack tables, init-once
    │
    ├── chesscom/           # default feature — fetch PGN by URL/username; `openchess chesscom` CLI + TUI import
    ├── lichess/            # optional feature — Bot API daemon; `openchess lichess` CLI only (no TUI)
    │
    ├── search/             # P2 + P5 — brain
    │   ├── mod.rs          # iterative deepening entry
    │   ├── alphabeta.rs    # PVS Root|PV|NonPV, qsearch
    │   ├── selectivity.rs  # NMP, LMR, RFP, LMP, ProbCut, singular…
    │   └── stack.rs        # per-ply Stack, PV
    │
    ├── movepick.rs         # P3 — staged MovePicker
    ├── history.rs          # P3 (+ correction tables for P6)
    │
    ├── transposition.rs    # P4 — clustered TT
    │
    ├── eval/               # P6
    │   ├── mod.rs          # evaluate() glue (HCE or NNUE + corrections)
    │   ├── hce.rs          # material + PSTs (+ optional tapered terms)
    │   └── nnue/           # incremental network (Phase C)
    │       ├── mod.rs
    │       ├── accumulator.rs
    │       ├── forward.rs
    │       └── simd/       # scalar first; avx2/neon later
    │
    ├── uci.rs              # P7 — protocol loop (machine I/O)
    ├── time.rs             # P7 — soft/hard time management
    ├── parameters.rs       # tunable search constants (SPSA later)
    │
    ├── tui/                # P7b — human terminal UI (ratatui)
    │   ├── mod.rs          # event loop, app state
    │   ├── board_view.rs   # Unicode board + last-move highlight
    │   ├── input.rs        # UCI/SAN parse + keybindings
    │   ├── engine_panel.rs # depth, score, PV, nodes, time
    │   └── session.rs      # play modes + EngineSession (uses session::LiveSearch)
    │
    ├── session/            # shared LiveSearch / SearchInfo (TUI + arena)
    ├── arena/              # P11 — bulk Bot-vs-Bot lab (run / watch)
    │
    ├── thread.rs           # P8 — ThreadData, SharedContext
    ├── threadpool.rs       # P8 — Lazy SMP workers
    ├── tb.rs               # P8 — Syzygy (feature = "syzygy")
    │
    └── tools/              # P8-00 — bench, perft, speedtest
        ├── mod.rs
        ├── perft.rs
        └── bench.rs
```

**Binary entry:** `main.rs` → `lookup::initialize()` (+ later `nnue::initialize()`) → if argv is `tui` then `tui::run()`, else `uci::message_loop()`.

**Suggested `Cargo.toml` sketch:**

```toml
[package]
name = "openchess"
edition = "2024"          # or 2021 if toolchain requires
license = "GPL-3.0-or-later"  # decide explicitly before first release

[features]
default = []
syzygy = ["dep:fathom-or-equivalent"]

[profile.release]
lto = "fat"
codegen-units = 1
panic = "abort"
```

---

## 4. Layered runtime architecture

```
┌──────────────────────────────┐  ┌──────────────────────────────┐
│  uci (stdin/stdout)          │  │  tui (ratatui / crossterm)   │
│  position / go / setoption   │  │  play / browse / engine pane │
└──────────────┬───────────────┘  └──────────────┬───────────────┘
               │                                 │
               └──────────────┬──────────────────┘
                              │
┌─────────────────────────────▼─────────────────────────────┐
│  session API — set position, apply/undo, go / stop / info │
└─────────────────────────────┬─────────────────────────────┘
                              │
┌─────────────────────────────▼─────────────────────────────┐
│  threadpool — Lazy SMP                                    │
│  shared: TT, stop flag, node counters                     │
│  per-thread: Board copy, histories, NNUE accumulators     │
└─────────────────────────────┬─────────────────────────────┘
                              │
┌─────────────────────────────▼─────────────────────────────┐
│  search — iterative deepening → PVS                       │
│  movepick · history · time · (tb) · selectivity           │
└─────────────┬─────────────────────────────┬───────────────┘
              │                             │
┌─────────────▼─────────────┐ ┌─────────────▼─────────────┐
│  board + lookup           │ │  eval (hce → nnue)         │
│  bitboards, make/unmake,  │ │  incremental accumulator   │
│  movegen, SEE, Zobrist    │ │  quantized forward         │
└───────────────────────────┘ └───────────────────────────┘
```

---

## 5. Pillar → module ownership

| Pillar | Primary modules | Notes |
|---|---|---|
| **P1 Board** | `types/`, `board/`, `lookup`, `tools/perft` | Hard gate: perft suite |
| **P2 Search core** | `search/mod`, `search/alphabeta`, `search/stack` | No pruning recipes here long-term |
| **P3 Ordering** | `movepick`, `history` | Stages: TT → good noisy → quiet → bad |
| **P4 TT** | `transposition` | Shared; intentionally racy under SMP |
| **P5 Selectivity** | `search/selectivity` | One feature per task; hooks into alphabeta |
| **P6 Eval** | `eval/` | HCE first; NNUE when search/testing solid |
| **P7 UCI & time** | `uci`, `time` | No search logic inside `uci.rs` |
| **P7b TUI** | `tui/` | Human UI only; shares session API with UCI; no search logic |
| **P9 Lichess CLI** | `lichess/` | Headless Bot API daemon; calls lib search directly; no TUI |
| **P8 Scale & science** | `thread`, `threadpool`, `tb`, `tools/`, CI | SPRT before strength claims |

Agents must not change another pillar’s public API without updating that pillar’s contract in [research/tasks.md](research/tasks.md).

---

## 6. Core types (Rust sketch)

Keep these small, `Copy` where natural, and free of search state:

```text
Bitboard(u64)
Square(u8)          // 0..63
Color               // White | Black
PieceType           // Pawn..King
Piece               // color + type (or separate mailbox encoding)
Move                // from, to, promo / flags (compact u16 or u32)
CastlingRights
Key / ZobristHash   // u64
Value / Score       // i32 centipawn-like; mate scores distance-adjusted
Bound                // Exact | Lower | Upper
Depth, Ply
```

`Board` holds:

- Piece bitboards + color/occupancy aggregates  
- Mailbox `[Piece; 64]` (or equivalent)  
- Side to move, castling, EP, halfmove, fullmove  
- Incremental Zobrist key  
- Checkers / pinned / (optional) threats  
- State stack for unmake  
- Optional `BoardObserver` callback surface for NNUE dirty features  

---

## 7. Data flow: UCI / TUI → bestmove

```
GUI ──UCI──► uci::message_loop ──┐
Human ─TUI─► tui::run ───────────┼──► session (position / go / stop)
                                 │
                                 ▼
                      threadpool::start_thinking
                      ├── root Board on each worker
                      ├── RootMoves (legal) + optional Syzygy rank
                      └── wake workers
                                 │
                                 ▼
                      per worker: iterative_deepening
                      ├── aspiration window
                      ├── search<Root>(α, β, depth)
                      │     ├── TT probe
                      │     ├── eval (+ corrections)
                      │     ├── forward pruning (P5)
                      │     ├── MovePicker stages (P3)
                      │     ├── make → child search (PVS) → unmake
                      │     └── TT store; history updates
                      └── time check / info callbacks
                                 │
                                 ▼
                      vote best thread → bestmove [ponder]
                      (UCI prints bestmove; TUI applies on board)
```

---

## 8. Concurrency model (Lazy SMP)

- **Not** work-stealing split of one tree. Each thread searches the **same root** independently.
- **Shared:** transposition table, stop/abort flag, aggregate node counters (sharded or atomic).
- **Per-thread:** `Board`, search stack, histories, NNUE accumulator stack, killers, PV.
- TT may be **racy** (Stockfish-family); document that. Prefer `UnsafeCell`/atomics with clear invariants over coarse mutexes on the hot path.
- Main thread owns time management + UCI `info` reporting; helpers stay quiet unless MultiPV/debug needs otherwise.
- NUMA / hugepages / weight replication are **Phase D** polish (`P8-04`), not Phase A.

---

## 9. Evaluation evolution

| Stage | Module path | When |
|---|---|---|
| Material (+ PSTs) | `eval/hce.rs` | Phase A — enough for αβ |
| Tapered HCE extras | `eval/hce.rs` | Optional before NNUE |
| NNUE FT + accumulator | `eval/nnue/` | Phase C — after search/TT/ordering solid |
| Post-NNUE corrections | `eval/mod.rs` + `history` correction tables | With/after NNUE |

**NNUE constraints (must preserve):**

1. Sparse inputs (few active features)  
2. Incremental accumulator updates on make/unmake  
3. Quantized / SIMD-friendly forward (scalar first)

Board never depends on `eval`. Eval may observe board via a trait object or explicit dirty-feature list passed from `makemove`.

---

## 10. Search module split

Keep `search.rs` from becoming a 50KB monolith on day one by splitting early:

| File | Responsibility |
|---|---|
| `search/mod.rs` | `start` / iterative deepening / aspiration / MultiPV |
| `search/alphabeta.rs` | Node-type PVS + qsearch control flow |
| `search/selectivity.rs` | NMP, RFP, razoring, LMR, LMP, futility, ProbCut, IIR, singular |
| `search/stack.rs` | `Stack`, PV helpers |

`selectivity` should expose small functions/hooks called from `alphabeta`, so P5 agents can land one technique without rewriting the whole search.

---

## 11. Cargo features & tooling

| Feature / tool | Purpose |
|---|---|
| default | UCI engine + TUI binary, HCE/NNUE as implemented, no TB |
| `syzygy` | Link tablebase probe (`tb.rs`) |
| `tools/perft` | Correctness gate for P1 |
| `tools/bench` | Fixed-position node signature for regressions |
| OpenBench / cutechess | External; documented under P8 in tasks |

Release builds: fat LTO, single codegen unit, `panic = abort` once the engine is past the skeleton (match Reckless-style release profile when chasing NPS).

---

## 12. Testing strategy (architectural)

```
perft (P1)  →  UCI smoke (P7)  →  fixed-depth/fixed-node bench
                                      →  SPRT self-play (P8)
```

- Do **not** tune primarily on tactical suites.  
- Functional search/eval changes need a measurement plan before merge once Phase B exists.  
- CI should run perft + a short bench at minimum (`P8-00`).

---

## 13. Implementation phases (maps to tasks)

| Phase | Architecture milestone |
|---|---|
| **A — Skeleton** | `types` + `board` + perft; `eval/hce` material; `search` αβ+ID; `uci` minimal; `tui` board sandbox |
| **B — Scalable search** | `transposition`; `movepick`+`history`; qsearch/PVS/aspiration; early `selectivity` |
| **C — Eval ceiling** | `eval/nnue` + observer hooks; corrections |
| **D — Scale & measure** | `threadpool` Lazy SMP; optional `tb`; OpenBench SPRT; SIMD/PGO |

**Phase A parallel tracks:** board (P1), TT stub (P4-01), material eval (P6-01), UCI stub after FEN (P7-01).

---

## 14. Explicit non-goals (this architecture)

- MCTS / Lc0-style GPU nets as the primary search (different product)  
- Speculative tracks in [research/uniqueideas.md](research/uniqueideas.md) — do not reshape this module map for them without a new architecture revision  
- Premature multi-crate workspace, WASM, or NUMA before single-thread strength is real  

---

## 15. Mental model for agents

1. Touch only your pillar’s modules unless updating a documented contract.  
2. Board legality and perft beat clever search every time.  
3. Wire TT and move ordering before stacking P5 techniques.  
4. Measure Elo with SPRT, not anecdotes.  
5. When unsure of shape, prefer the Reckless module boundaries in [research/reckless.md](research/reckless.md) §3 — readable Rust embodiment of this same family.

---

*Architecture for OpenChess (Rust), aligned with research/tasks.md — 2026-07-10.*
