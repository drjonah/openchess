# OpenChess NNUE training pipeline (Q2)

> **Status:** Q2-01 data pipeline shipped. Full net training is **Q2-02**.
> **Task board:** [tasks.md](./tasks.md) · research: [reckless §7.5](./reckless.md#75-training-ecosystem) · [Bullet](https://github.com/jw1912/bullet)

This document is the repro path for producing **Bullet-ready** quiet-position training data matching OpenChess’s HalfKA L1=256 bootstrap (`OCNNv002`).

---

## 1. Goal

Replace the material-distilled bootstrap leaf with a search/self-play-trained net:

| Task | Scope |
|---|---|
| **Q2-01** (this doc) | Data pipeline + fixture dataset |
| **Q2-02** | Train successor net; load via `EvalFile` / candidate embed |
| **Q2-03** | Default embed of trained net |

---

## 2. Agreed export format (Bullet text)

OpenChess emits **Bullet text**, one sample per line:

```text
<FEN> | <score> | <result>
```

| Field | Meaning |
|---|---|
| `FEN` | Full 6-field FEN |
| `score` | **White-relative** evaluation in centipawns |
| `result` | **White-relative** WDL: `1.0` / `0.5` / `0.0` |

Bullet’s `bullet-utils` (or the trainer’s convert path) turns this into the binary `ChessBoard` / “bulletformat” loader format. See [Bullet docs — Training Data](https://github.com/jw1912/bullet/blob/main/docs/3-data.md).

**Why text first:** simple to validate, git-friendly fixtures, no dependency on binpack tooling for Q2-01. Production-scale Q2-02 runs may convert to Viriformat/binpack later; the CLI still speaks text.

Comments (`# …`) and blank lines are allowed and ignored by `nnue-data validate`.

---

## 3. Pipeline stages

```
opening seeds (EPD/FEN)
        ↓
self-play (search @ play-depth, optional seeded random moves)
        ↓
quiet filter (not in check; no capture with SEE ≥ 0)
        ↓
label (search @ label-depth → white-relative CP; skip mate scores)
        ↓
attach game WDL (or 0.5 if max-plies hit)
        ↓
Bullet text file
        ↓  (Q2-02)
Bullet train → quantize → OCNNv00x / EvalFile
```

### Quiet filter

A position is kept when:

1. Side to move is **not in check**, and
2. No legal capture has **SEE ≥ 0** (no free/equal takes).

This matches common NNUE practice: train on quiet positions, let search handle tactics.

### Scores

Search returns **side-to-move-relative** scores. The exporter flips the sign for Black so Bullet always sees white-relative CP.

---

## 4. How to run

### Fixture (acceptance for Q2-01)

From the repo root:

```bash
./tools/nnue-data/run_fixture.sh
```

Or:

```bash
cargo run -q -- nnue-data fixture \
  --openings tools/nnue-data/fixtures/openings.epd \
  --output tools/nnue-data/out/fixture_bullet.txt
cargo run -q -- nnue-data validate tools/nnue-data/out/fixture_bullet.txt
```

Expected: a small `fixture_bullet.txt` with valid `FEN | score | result` lines; command exits 0.

### Larger generate run (still Q2-01 tooling)

```bash
cargo build --release
./target/release/openchess nnue-data generate \
  --openings tools/nnue-data/fixtures/openings.epd \
  --games 64 \
  --play-depth 6 \
  --label-depth 6 \
  --min-ply 6 \
  --max-plies 120 \
  --seed 42 \
  --random-move-prob 0.1 \
  --output tools/nnue-data/out/dev64.txt
```

### Validate any file

```bash
./target/release/openchess nnue-data validate path/to/data.txt
```

---

## 5. Layout

| Path | Role |
|---|---|
| `src/tools/nnue_data/` | Generator, quiet filter, Bullet text I/O, CLI |
| `tools/nnue-data/fixtures/openings.epd` | Tiny seed openings for the fixture |
| `tools/nnue-data/run_fixture.sh` | One-command fixture repro |
| `tools/nnue-data/out/` | Generated data (gitignored) |
| `research/nnue-training.md` | This document |

CLI entry: `openchess nnue-data {generate,validate,fixture}`.

---

## 6. Engine topology note (for Q2-02)

Inference net today:

- Features: HalfKA (`FEATURE_COUNT = 64×12×64`)
- Accumulator L1 = **256**
- Dense head L2 = L3 = **32**
- File magic: `OCNNv002`

Freeze this layout while training the first successor unless Q2-02 explicitly versions a new magic (coordinate with F2 SIMD).

Bullet example nets often use Chess768 / different L1 widths — Q2-02 must either:

1. Train a HalfKA→256→32→32 graph and export quantized weights into `OCNNv002`, or
2. Bump magic / L1 and update `src/eval/nnue/` together.

---

## 7. Q2-02 handoff (not done here)

1. Scale data: deeper labels, more games, shuffled interleaved files.
2. Convert text → Bullet binary (`bullet-utils`).
3. Train with `bullet_lib` example matching HalfKA + OpenChess widths.
4. Quantize / pack into `OCNNv002` (or successor).
5. Smoke: startpos + tactical positions; fixed-node bench or local SPRT vs bootstrap.

---

## 8. Unit tests

```bash
cargo test --lib tools::nnue_data::
```

Covers Bullet line round-trip, quiet filter fixtures, and an in-process fixture generate → validate path.
