# OpenChess — Phase 2 Task Board

> **Audience:** agents raising Elo and taking the Lichess bot to production-safe play.
> **Paradigm:** Stockfish-family (bitboards + PVS + selective search + NNUE + Lazy SMP + SPRT) — unchanged from Phase 1.
> **Phase 1 archive:** [tasks-phase1.md](./tasks-phase1.md) (complete SF-family skeleton + arena + Lichess CLI).
> **Research sources:** [chesswiki.md](./chesswiki.md) (Phase D) · [LICHESS.md](./LICHESS.md) · [openings.md](./openings.md) · [reckless.md](./reckless.md) · [stockfish.md](./stockfish.md)
> **Out of scope:** speculative ideas in [uniqueideas.md](./uniqueideas.md) — separate track.
>
> **Implementation language: Rust.** Module layout: [ARCHITECTURE.md](../ARCHITECTURE.md). Copy structure from research, retune with SPRT — especially after the trained net lands.
> **NN-last schedule:** [phase2-nn-last-plan.md](./phase2-nn-last-plan.md) — do Lichess / TB / search / throughput / train plumbing **before** Q2-02/Q2-03.

---

## One-sentence model

**Phase 2 = finish production Lichess + TB + search/throughput on the bootstrap leaf, then train/embed NNUE last and re-measure.**

---

## How to use this file (agent rules)

1. **Own one pillar or one task ID at a time.** Do not edit another pillar’s core APIs without updating that pillar’s **Contract** section and notifying the owning agent.
2. **Respect deps.** A task is blocked until every listed dep is marked done (`[x]`). Soft “prefer Q2-03 for Elo” notes are **not** hard deps — see [phase2-nn-last-plan.md](./phase2-nn-last-plan.md).
3. **Acceptance over vibes.** Ship only when the task’s acceptance criteria pass (SPRT, live Lichess smoke, fixed-node bench, unit fixtures).
4. **One strength change at a time.** Functional search/TB/SIMD may land before the trained net behind toggles/feature flags; **Elo claims and margin retunes** after Q2-03 go through SPRT individually.
5. **Mark progress in this file.** Flip `- [ ]` → `- [x]` and note the PR/commit if useful.
6. **Link research.** Each task cites the justifying section; read it before implementing.
7. **Do not re-implement Phase 1.** Book, arena, Lichess CLI skeleton, and search selectivity already shipped — see [tasks-phase1.md](./tasks-phase1.md).
8. **NN last.** Do not start Q2-02 until Wave A–E in [phase2-nn-last-plan.md](./phase2-nn-last-plan.md) are done or explicitly deferred in a task Note.

### Task entry format

| Field | Meaning |
|---|---|
| **ID** | Stable handle, e.g. `L2-02` |
| **Deps** | Other task IDs that must be done first |
| **Parallel-ok** | Pillars/tasks safe to run concurrently |
| **Deliverable** | APIs / docs / modules expected |
| **Acceptance** | Concrete gate |
| **Research** | Pointer into research docs |

---

## Dependency graph

```mermaid
flowchart LR
  L2live[L2_02_03_07]
  K2[K2_Syzygy]
  S2[S2_NMP_Ponder]
  F2[F2_PGO_SIMD]
  Q2prep[Q2_plumbing]
  Q2train[Q2_02_03_LAST]
  M2post[M2_02_03]
  L2rated[L2_06_rated]

  L2live --> Q2train
  K2 --> Q2train
  S2 --> Q2train
  F2 --> Q2train
  Q2prep --> Q2train
  Q2train --> M2post
  Q2train --> L2rated
  M2post --> L2rated
```

| Pillar | Owns | Does not own |
|---|---|---|
| **L2 Lichess go-live** | Live smoke, ops config, accept policy, rated gate, concurrent games | Search/eval training, UCI protocol |
| **Q2 Trained eval** | Data pipeline, Bullet/self-play training, shipping successor net via `EvalFile` / embed | SPRT books, Syzygy, Lichess HTTP |
| **M2 Measurement** | Larger SPRT openings, strength-PR rules, post-net margin retune process | Net architecture, Lichess daemon |
| **K2 Endgame** | Syzygy WDL/DTZ + `SyzygyPath` (Phase 1 **P8-02**) | Eval training, opening book |
| **S2 Search polish** | NMP verification, SEE promo completeness, optional ponder | Leaf net weights, Lichess I/O |
| **F2 Throughput** | PGO build, SIMD NNUE forward (Phase 1 **P8-04**) | Functional strength claims without SPRT |

---

## Critical path (NN last)

1. **Lichess live** (L2-02 → L2-03 → L2-07) — needs bot token; bootstrap net OK.
2. **Syzygy** (K2-01) — implement now; Elo claim after embed.
3. **Search polish** (S2-01, S2-03) — toggles; Lichess ponder-off; re-measure after embed.
4. **Throughput** (F2-01, F2-02) — vs bootstrap; **freeze `OCNNv002`**.
5. **Train plumbing** (feature parity, exporter, EvalFile SPRT) — see [phase2-nn-last-plan.md](./phase2-nn-last-plan.md) Wave E.
6. **LAST — trained NNUE** (Q2-02 → Q2-03).
7. **Post-net only** (M2-02, M2-03, L2-06) — one strength change at a time.

Done already: L2-01/04/05, Q2-01, M2-01, S2-02.

---

## Parallelism matrix

| Now working on | Can also run |
|---|---|
| L2-02 live smoke | K2-01; Q2 plumbing (parity/exporter); F2 design |
| L2-03 / L2-07 | K2-01; S2-01 or S2-03; F2-01/02 |
| K2-01 Syzygy | L2-*; S2-03; F2-*; Q2 plumbing — **not** another search margin PR |
| S2-01 NMP verify | L2-*; K2; F2 — **not** S2-03 in the same strength PR |
| F2-01 / F2-02 | L2-*; K2; Q2 plumbing — **no** `OCNNv002` topology churn |
| Q2-02 / Q2-03 (last) | Docs only; no parallel strength merges |
| M2-03 retune | K2 re-measure; **not** another retune at once |

---

## Non-goals

- Speculative search/eval from [uniqueideas.md](./uniqueideas.md)
- Chess.com as a strength path
- TUI Lichess mirror panel
- Replacing the Stockfish-family stack (MCTS-only, GPU searchless, etc.)
- Rebuilding Phase 1 book / arena / Lichess CLI from scratch

---

## L2 — Lichess go-live

**Contract:** Make `openchess lichess` production-safe for casual play, then rated. Own smoke checklists, accept-policy defaults, config files, and (later) multi-game concurrency. Do not change search internals here. Phase 1 CLI/game loop lives in `src/lichess/` ([tasks-phase1.md](./tasks-phase1.md) P9).

**Research:** [LICHESS.md](./LICHESS.md) · [LICHESS §11](./LICHESS.md#110-cli-only--no-tui) · [LICHESS §14](./LICHESS.md#14-open-questions)

### Tasks

- [x] **L2-01** — Operator docs: token setup + smoke checklist  
  - **Deps:** none  
  - **Parallel-ok:** Q2-01, M2-01  
  - **Deliverable:** README / CONTRIBUTING section: create bot account, `LICHESS_TOKEN`, `lichess account`, `lichess run` (dry-run) → `--play`, challenge flow  
  - **Acceptance:** Operator can go dry-run → play from docs alone without reading research docs  
  - **Research:** [LICHESS §4](./LICHESS.md#4-account-setup) · README Lichess section  
  - **Note:** README “Lichess bot” setup + smoke checklist; `.env.example` comments; examples under `examples/lichess.{toml,json}`.

- [ ] **L2-02** — Live casual game smoke  
  - **Deps:** L2-01  
  - **Parallel-ok:** L2-04, Q2-01  
  - **Deliverable:** Manual (or scripted) run completing one casual (`rated=false`) game vs a weak online bot  
  - **Acceptance:** Full game completes; no illegal moves; no time forfeit caused by engine/bot bugs; note game URL in task Note  
  - **Research:** [LICHESS §8](./LICHESS.md#8-challenges--bot-matchmaking) · closes Phase 1 P9-03 / P9-05 live notes  
  - **Note:** Blocked on operator bot token + live run (ops path documented in L2-01).

- [ ] **L2-03** — Live reconnect + PGN verify  
  - **Deps:** L2-02  
  - **Parallel-ok:** L2-04, L2-05  
  - **Deliverable:** Forced event-stream disconnect recovers without double-accept; `pgn::export_game` output matches lichess.org for a played game  
  - **Acceptance:** Reconnect survives manual kill of stream; exported PGN movetext/result match site; Note with evidence  
  - **Research:** [LICHESS §11.4](./LICHESS.md#114-error-handling--reconnects) · closes Phase 1 P9-06 / P9-07 live notes

- [x] **L2-04** — Ops config file + CLI overrides  
  - **Deps:** none (code) / prefer L2-01 for docs  
  - **Parallel-ok:** L2-02, L2-05, Q2-*  
  - **Deliverable:** Load Lichess accept/matchmaking policy from TOML or JSON on disk; CLI flags override file; shape matches `LichessConfig` (speeds, rated, humans, rating band, variants)  
  - **Acceptance:** Config file alone drives accept filter without recompile; unit tests cover load + override precedence  
  - **Research:** [LICHESS §11.3](./LICHESS.md#113-config-surface-minimal) · Phase 1 note that TOML mapping was future  
  - **Note:** `LichessConfig::load_from_path` + `ConfigOverrides`; `--config` / `--speeds` / policy flags on `lichess run`.

- [x] **L2-05** — Default policy: bots-preferred, rated off  
  - **Deps:** L2-04  
  - **Parallel-ok:** L2-02, L2-03  
  - **Deliverable:** Defaults decline rated until L2-06; humans opt-in (`accept_humans` default false or documented bot-preferred); speeds/variants stay standard-safe  
  - **Acceptance:** Default config declines surprise rated human challenges; bot-vs-bot casual still accepted  
  - **Research:** [LICHESS §10](./LICHESS.md#10-restrictions--fair-play) · [LICHESS §14 #5](./LICHESS.md#14-open-questions)  
  - **Note:** Defaults `accept_rated=false`, `accept_humans=false`; ponder off documented on Lichess path.

- [ ] **L2-06** — Rated gate  
  - **Deps:** L2-02, L2-03, M2-02, Q2-03  
  - **Parallel-ok:** K2-01, F2-*  
  - **Deliverable:** Documented strength bar (local SPRT / arena smoke) before enabling `accept_rated`; config default flip + CONTRIBUTING note  
  - **Acceptance:** Checklist in CONTRIBUTING/README; default stays casual until bar met and task marked done with evidence  
  - **Research:** [LICHESS §14 #3](./LICHESS.md#14-open-questions)  
  - **Note:** Blocked on Q2-03 + M2-02 (+ live L2-02/03). Defaults remain casual.

- [ ] **L2-07** — Concurrent games  
  - **Deps:** L2-02, L2-03  
  - **Parallel-ok:** F2-*, K2-01, S2-*, Q2 plumbing  
  - **Deliverable:** Tokio or thread-per-game; ≥2 concurrent Lichess games under Bot API rate limits; still serializes REST where required  
  - **Acceptance:** Two concurrent casual games complete without 429 storms or illegal moves  
  - **Research:** [LICHESS §6.4](./LICHESS.md#64-architecture-sketch-from-lichess-bot) Phase 2 · [LICHESS §14 #1](./LICHESS.md#14-open-questions)  
  - **Note:** Prep only — `max_concurrent_games` in config, clamped to `1` until this task lands. Schedule **before** Q2-02 per [phase2-nn-last-plan.md](./phase2-nn-last-plan.md).

---

## Q2 — Trained eval

**Contract:** Replace the material-distilled bootstrap NNUE with a trained net. Own data pipeline, training repro, embedding/`EvalFile` ship. Search still owns when to evaluate; corrections/`eval/` module layout stay.

**Research:** [chesswiki §3](./chesswiki.md#3-evaluation) · [chesswiki Phase C–D](./chesswiki.md#phase-c--eval--knowledge) · [reckless §7](./reckless.md#7-nnue-evaluation-why-stockfish-works-so-well) · Phase 1 P6-06 Note

### Tasks

- [x] **Q2-01** — Training data pipeline  
  - **Deps:** none  
  - **Parallel-ok:** L2-01..L2-04, M2-01  
  - **Deliverable:** Doc + tool path producing Bullet-ready (or agreed format) quiet-position / self-play data; small fixture dataset builds end-to-end  
  - **Acceptance:** Repro steps in `research/` or `tools/`; fixture run completes on a developer machine  
  - **Research:** reckless / stockfish NNUE training notes · chesswiki NNUE  
  - **Note:** Bullet text (`FEN \| score \| result`) via `openchess nnue-data`; docs in [nnue-training.md](./nnue-training.md); fixture `./tools/nnue-data/run_fixture.sh`. Branch `phase2/q2-01-training-data`.

- [ ] **Q2-02** — Train successor net  
  - **Deps:** Q2-01; prefer Wave A–E in [phase2-nn-last-plan.md](./phase2-nn-last-plan.md) done first (NN last)  
  - **Parallel-ok:** docs only while training — no parallel strength merges  
  - **Deliverable:** Trained net loads via existing `EvalFile` and/or embed as `OCNNv00x` successor to bootstrap  
  - **Acceptance:** Startpos + tactical smoke stable; beats bootstrap on fixed-node bench **or** wins a local SPRT vs bootstrap  
  - **Research:** Phase 1 P6-05/P6-06 · stockfish Network::evaluate  
  - **Note:** **Do not start until** feature parity + exporter + EvalFile harness (Wave E) are ready.

- [ ] **Q2-03** — Ship default embedded net  
  - **Deps:** Q2-02  
  - **Parallel-ok:** M2-01, M2-02, F2 design  
  - **Deliverable:** Default build embeds trained net; `EvalFile` docs updated; OwnBook play policy unchanged  
  - **Acceptance:** Fresh `cargo build` / release plays with trained net without extra flags; UCI `EvalFile` still overrides  
  - **Research:** UCI EvalFile (Phase 1 P7-03)

---

## M2 — Measurement

**Contract:** Own strength science: larger SPRT openings, PR gates, post-net retune discipline. Do not invent search features here — schedule them under S2/K2 and measure.

**Research:** [chesswiki §7](./chesswiki.md#7-scientific-development-non-negotiable-for-strength) · [chesswiki Phase D](./chesswiki.md#phase-d--strength-process) · `testing/sprt.sh` · CONTRIBUTING

### Tasks

- [x] **M2-01** — Larger SPRT opening set  
  - **Deps:** none  
  - **Parallel-ok:** Q2-*, L2-01..L2-04  
  - **Deliverable:** Grow past smoke `testing/books/openings.epd` toward UHO / 8moves-class set; wire `testing/sprt.sh`  
  - **Acceptance:** `testing/sprt.sh` runs on the new book with `OwnBook=false`; documented in `testing/README.md`  
  - **Research:** chesswiki Engine Testing · stockfish Fishtest opening practice  
  - **Note:** Default book is `testing/books/8mvs_+90_+99.epd` (8533 UHO 8-move positions, CC0 from official-stockfish/books). Smoke keeps `--book testing/books/openings.epd`. `OwnBook=false` unchanged in `sprt.sh`.

- [ ] **M2-02** — Strength-PR gate tightened  
  - **Deps:** Q2-03, M2-01  
  - **Parallel-ok:** K2-01, L2-05  
  - **Deliverable:** CONTRIBUTING guidance: min SPRT / bench signature expectations after trained net ships  
  - **Acceptance:** CONTRIBUTING matches practice; links Phase 2 acceptance for strength PRs  
  - **Research:** Phase 1 P8-03 · CONTRIBUTING strength-PR rule

- [ ] **M2-03** — Post-net margin retune pass  
  - **Deps:** Q2-03, M2-01  
  - **Parallel-ok:** K2-01 (prefer sequential if same agent)  
  - **Deliverable:** Retune P5 margins/constants for the new leaf — **one change SPRT’d at a time**  
  - **Acceptance:** At least one accepted SPRT win documented in task Note or PR  
  - **Research:** chesswiki Selectivity · Phase 1 P5 “copy structure, not constants”

---

## K2 — Endgame (Syzygy)

**Contract:** Tablebase probes in search and at root. Carry-forward of Phase 1 **P8-02**. Skip heavy probes in qsearch. UCI `SyzygyPath`.

**Research:** [chesswiki Syzygy](./chesswiki.md) · reckless `tb.rs` · stockfish syzygy · Phase 1 P8-02

### Tasks

- [ ] **K2-01** — Syzygy WDL + DTZ  
  - **Deps:** M2-01 (book for later measurement)  
  - **Parallel-ok:** L2-02..L2-07, S2-03, F2-*, Q2 plumbing  
  - **Deliverable:** WDL probe in search; DTZ at root; `SyzygyPath` UCI/option; skip heavy probes in qsearch  
  - **Acceptance:** Known 5-man wins return TB scores/mate bounds; root ranking prefers DTZ progress  
  - **Research:** chesswiki Syzygy Bases · Phase 1 P8-02  
  - **Note:** **Implement before Q2-02** (NN-last). Elo/SPRT claims prefer after Q2-03; feature-gate; no TB download in default CI.

---

## S2 — Search polish

**Contract:** Small correctness/strength polish on the existing PVS stack. Under NN-last, implement NMP verify / ponder **before** training; re-measure Elo after Q2-03. Do not stack multiple unmeasured strength claims in one PR.

**Research:** chesswiki NMP / SEE · Phase 1 P5-01 Note · Phase 1 P1-08 Note · chesswiki Phase D ponder · [phase2-nn-last-plan.md](./phase2-nn-last-plan.md)

### Tasks

- [ ] **S2-01** — NMP verification search  
  - **Deps:** M2-01  
  - **Parallel-ok:** L2-*, K2-01, F2-*; not S2-03 in the same strength PR  
  - **Deliverable:** Verification re-search behind NMP fail-high; feature toggle  
  - **Acceptance:** Toggleable; fixed-node smoke vs baseline documents node effect; **re-SPRT after Q2-03** if claiming Elo  
  - **Research:** chesswiki NMP · Phase 1 P5-01 (no verification yet)  
  - **Note:** **Code before Q2-02** (NN-last). Bootstrap measurement is fine for merge; retune narrative after embed.

- [x] **S2-02** — SEE recapture promotions  
  - **Deps:** none  
  - **Parallel-ok:** S2-01, L2-*, Q2-*  
  - **Deliverable:** Model promotion on recapture swaps in SEE  
  - **Acceptance:** Fixture set covers promo recapture signs (winning/losing)  
  - **Research:** Phase 1 P1-08 Note · chesswiki SEE  
  - **Note:** Done — pawn recaptures onto the promo rank get queen-promo bonus; `tests/see.rs` covers winning/losing signs.

- [ ] **S2-03** — Optional ponder  
  - **Deps:** none (UCI product surface)  
  - **Parallel-ok:** F2-*, L2-*, K2-01; not S2-01 in the same strength PR  
  - **Deliverable:** UCI `Ponder`; legal ponderhit path; **off by default** for Lichess daemon  
  - **Acceptance:** GUI ponderhit plays legal move; Lichess path remains ponder-off  
  - **Research:** chesswiki Phase D · stockfish ponder  
  - **Note:** **Code before Q2-02** (NN-last). No dependency on trained weights.

---

## F2 — Throughput

**Contract:** Raise NPS without changing chess semantics. Carry-forward of Phase 1 **P8-04**. Under NN-last, implement against bootstrap while **freezing `OCNNv002`**; re-check NPS after Q2-03 if needed.

**Research:** reckless release profile / simd · stockfish Makefile PGO · Phase 1 P8-04 · [phase2-nn-last-plan.md](./phase2-nn-last-plan.md)

### Tasks

- [ ] **F2-01** — PGO build  
  - **Deps:** none (freeze net format)  
  - **Parallel-ok:** F2-02, L2-*, K2-01, Q2 plumbing  
  - **Deliverable:** Profile-guided / documented release build instructions; CI or docs entry  
  - **Acceptance:** Documented release profile; measurable NPS uplift vs non-PGO on bench  
  - **Research:** Phase 1 P8-04 · stockfish PGO  
  - **Note:** **Before Q2-02.** Do not change topology while measuring.

- [ ] **F2-02** — SIMD NNUE forward  
  - **Deps:** none (must match current `OCNNv002` / L1=256)  
  - **Parallel-ok:** F2-01, L2-*, K2-01  
  - **Deliverable:** SIMD path for NNUE forward on target CPU; scalar fallback  
  - **Acceptance:** Correct vs scalar on fixture positions; NPS uplift on `bench`  
  - **Research:** Phase 1 P8-04 · reckless simd · stockfish NNUE SIMD  
  - **Note:** **Before Q2-02.** Coordinate with Q2 if magic/L1 would change — prefer freeze.

---

## Research index

| Doc | Use for |
|---|---|
| [tasks-phase1.md](./tasks-phase1.md) | Completed Phase 1 pillars P1–P11 |
| [LICHESS.md](./LICHESS.md) | Bot API, CLI daemon, challenges (L2) |
| [chesswiki.md](./chesswiki.md) | Concepts, Phase D strength process |
| [openings.md](./openings.md) | Book policy; SPRT vs play (M2) |
| [reckless.md](./reckless.md) | Rust SF-family reference |
| [stockfish.md](./stockfish.md) | Canonical C++ layout, NNUE, Fishtest |
| [ARENA.md](./ARENA.md) | Local Bot-vs-Bot lab (shipped in Phase 1) |
| [nnue-training.md](./nnue-training.md) | Q2 Bullet data pipeline + train handoff |
| [uniqueideas.md](./uniqueideas.md) | Non-goals for this board |

---

*Phase 1 archived in [tasks-phase1.md](./tasks-phase1.md) (2026-07-14). Phase 2 board opened 2026-07-14 — priorities: trained eval, SPRT, Lichess go-live.*
