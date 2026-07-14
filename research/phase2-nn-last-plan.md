# Phase 2 — NN-last execution plan

> **Policy (2026-07-14):** Ship everything that does not require a trained net **before** Q2-02/Q2-03.  
> **Canonical board:** [tasks.md](./tasks.md) · prior Wave 1 plan: [phase2-engine-plan.md](./phase2-engine-plan.md)  
> **Rationale:** Training is the longest, least-parallelizable Elo lever. Doing Lichess, TB, search polish, throughput, and train plumbing first means the train run is a final swap + re-measure, not a blocker for the rest of Phase 2.

---

## 1. Already done (do not redo)

| ID | Status |
|---|---|
| L2-01, L2-04, L2-05 | Ops docs, config, casual defaults |
| Q2-01 | Bullet-ready datagen |
| M2-01 | UHO-class SPRT book |
| S2-02 | SEE promo recaptures |

---

## 2. New critical path (NN last)

```text
A  Lichess live          L2-02 → L2-03 → L2-07
B  Endgame               K2-01 Syzygy (feature-gated)
C  Search product        S2-01 NMP verify · S2-03 Ponder (toggles; Lichess ponder off)
D  Throughput            F2-01 PGO · F2-02 SIMD (vs bootstrap; freeze OCNNv002)
E  Train plumbing        Q2-P* feature parity · exporter · EvalFile SPRT flags
F  LAST — train & ship   Q2-02 → Q2-03
G  Post-net only         M2-02 · M2-03 · L2-06 rated gate
```

**Rule:** After F, re-baseline once. Then one strength change at a time (existing tasks.md rule #4).

---

## 3. Waves (maximize parallelism)

### Wave A — Lichess production path (ops + code)

| Order | Task | Notes |
|---|---|---|
| A1 | **L2-02** Live casual smoke | Needs `LICHESS_TOKEN`; bootstrap net is fine |
| A2 | **L2-03** Reconnect + PGN verify | After A1 |
| A3 | **L2-07** Concurrent games | After A2; lift `max_concurrent_games` clamp |

**Agents:** 1 Lichess implementer; optional shell for live runs. **No** search/eval edits.

### Wave B — Syzygy (parallel with A after A1 starts)

| Task | Notes |
|---|---|
| **K2-01** | Implement WDL in search, DTZ at root, `SyzygyPath`; skip qsearch probes; feature-gate; **no TB in default CI** |
| Elo | Prefer measuring after Q2-03; correctness fixtures now |

**Agents:** 1 TB owner (`src/tb.rs` + probe sites). May run || Wave A and Wave C if file ownership holds.

### Wave C — Search polish (parallel with B)

| Task | Notes |
|---|---|
| **S2-01** | NMP verification behind toggle; smoke vs bootstrap; **re-SPRT after Q2-03** |
| **S2-03** | UCI `Ponder` + ponderhit; **Lichess stays ponder-off** |

**Agents:** Prefer one search owner for both, or S2-01 then S2-03 sequentially on `selectivity` / UCI to avoid collisions. Do **not** stack both as “Elo claims” in one PR.

### Wave D — Throughput (parallel with B/C; freeze net format)

| Task | Notes |
|---|---|
| **F2-01** | Documented PGO release build + NPS delta vs non-PGO on bootstrap |
| **F2-02** | SIMD forward = scalar on fixtures; NPS on `bench` |

**Constraint:** Do **not** change `OCNNv002` / L1=256 while F2 is in flight. Q2-02 must target that frozen layout.

### Wave E — Train plumbing (no weight training yet)

These are **not** separate task IDs on the board yet; treat as Q2-02 prep checklist (or add `Q2-P1..P3` if agents need IDs):

| Prep | Deliverable |
|---|---|
| **Q2-P1** Feature parity | Bullet HalfKA indices == `feature_index()` on ≥100 FENs |
| **Q2-P2** Exporter | Bullet `quantised.bin` → `OCNNv002` bytes; round-trip load |
| **Q2-P3** Harness | `sprt.sh` / cutechess `EvalFile` (or `--net-a`/`--net-b`) for candidate vs bootstrap |

**Agents:** 1 NNUE tooling owner (`tools/`, `src/eval/nnue/` tests only). Can || Waves A–D.

### Wave F — LAST: train & embed

| Task | Notes |
|---|---|
| **Q2-02** | Scale data → Bullet train → load via `EvalFile`; beat bootstrap (bench or SPRT) |
| **Q2-03** | Default embed; `EvalFile` still overrides |

Only start F when Waves A–E acceptance is green (or explicitly deferred with a Note).

### Wave G — Post-net only (after Q2-03)

| Task | Notes |
|---|---|
| **M2-02** | Strength-PR gate text for the shipped net |
| **M2-03** | Margin retune — **one SPRT at a time** |
| **L2-06** | Rated gate after L2-02/03 + M2-02 + Q2-03 |
| Re-check | S2-01 / K2 / F2 NPS under the new leaf if claims matter |

---

## 4. Dep / measurement policy changes

| Task | Old habit | NN-last policy |
|---|---|---|
| K2-01 | Prefer wait for Q2-03 | **Code now**; Elo/SPRT after embed |
| S2-01, S2-03 | Listed under Q2-03 | **Code now** behind toggles; re-measure after embed |
| F2-01, F2-02 | After Q2-03 | **Build now** against bootstrap; freeze topology |
| M2-02, M2-03, L2-06 | After Q2-03 | **Still after Q2-03** |
| Q2-02, Q2-03 | Mid critical path | **Final implementation wave** |

---

## 5. Ownership (anti-collision)

| Wave | Paths |
|---|---|
| A L2 | `src/lichess/**`, Lichess docs |
| B K2 | `src/tb.rs` (new), search probe hooks, Cargo `syzygy` feature |
| C S2 | `src/search/selectivity.rs`, UCI ponder wiring |
| D F2 | `src/eval/nnue/forward.rs`, `simd/`, build docs/scripts |
| E Q2-P | `tools/`, nnue export/tests; **no** topology churn |
| F/G Q2+M2 | embed + CONTRIBUTING + selectivity margins |

---

## 6. Success criteria before training

- [ ] L2-02 + L2-03 done (game URL + reconnect evidence in tasks.md Notes)
- [ ] L2-07 done or explicitly deferred with reason
- [ ] K2-01: 5-man fixture probes green behind feature flag
- [ ] S2-01 + S2-03 merged with toggles / Lichess ponder-off
- [ ] F2-01 documented; F2-02 scalar==SIMD on fixtures
- [ ] Q2-P1..P3 done so training is “run + load,” not “invent exporter”
- [ ] `./scripts/ci.sh` green on `main`

Then and only then: **Q2-02 → Q2-03 → Wave G**.

---

## 7. Operator checklist (start tomorrow)

1. Confirm `cutechess-cli` installed; smoke `./testing/sprt.sh --book testing/books/openings.epd --st 5 --games 40`.
2. Spawn Wave A (L2-02) with token; spawn Wave B (K2) and Wave E (Q2-P) in parallel.
3. After L2-03: L2-07; after K2 spike: S2-01 or F2.
4. Do **not** start Bullet overnight train until Wave E exporter round-trips.
