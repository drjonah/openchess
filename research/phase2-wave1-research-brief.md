# Phase 2 Wave 1 Research Brief

> **Date:** 2026-07-14 · **Audience:** parallel Wave 1 implementers (Q2-01, M2-01, S2-02, explore)  
> **Canonical tasks:** [research/tasks.md](research/tasks.md) · **Plan:** [research/phase2-engine-plan.md](research/phase2-engine-plan.md)

---

## 1. Bullet training → `OCNNv002` (Q2-01 / Q2-02)

### 1.1 Recommended toolchain

| Piece | Recommendation | Why (this repo) |
|---|---|---|
| **Trainer** | [Bullet](https://github.com/jw1912/bullet) (`bullet_lib`, MIT) | Named in research/reckless.md §7.5, tasks.md Q2, phase2-engine-plan.md. Community default for NNUE training. |
| **Utilities** | `bullet-utils` (`convert`, shuffle, interleave) | Bullet docs/3-data.md |
| **Datagen (Phase 2)** | **OpenChess self-play / search-labeled** → `tools/` scripts (Q2-01 deliverable) | No tools/ training pipeline today; bootstrap leaf is material-distilled. Avoid shipping LC0-labeled nets without license review. |
| **Load path (Q2-02)** | Existing UCI `EvalFile` → `Network::load_file` | Already wired in src/uci.rs. Q2-03 embeds bytes; Q2-02 validates via EvalFile first. |
| **GPU** | Optional CUDA/Metal/ROCm Bullet features | CPU training is fine for fixture/smoke. |

**Not recommended for Wave 1:** nnue-pytorch / Stockfish `.nnue` import — different feature set and file format.

### 1.2 OpenChess `OCNNv002` layout (freeze for Wave 1–2)

From src/eval/nnue/network.rs + features.rs:

| Field | Value |
|---|---|
| Magic | `OCNNv002` (8 bytes) |
| Topology | L1=**256**, L2=**32**, L3=**32** (hard-check: `l1 != L1_SIZE` rejects load) |
| Features | HalfKA-style: `FEATURE_COUNT = 64 × 12 × 64 = 49_152` |
| Indexing | `king_orient × (12×64) + slot × 64 + sq_orient`; **own king is never a feature**; Black mirrors ranks (`sq ^ 56`) |
| FT | `ft_w: i16[FEATURE_COUNT × l1]`, `ft_bias: i16[l1]` |
| Head | `fc1: i8[l2 × 2×l1]` + `i32[l2]` → `fc2: i8[l3 × l2]` + `i32[l3]` → `out: i8[l3]` + `i32 out_b` |
| Scale | `i32` divisor (bootstrap uses `1`; loader default `16` if unset in file) |
| Inference | Dual accumulators → ClippedReLU → concat `2×l1` → dense (forward.rs) |
| Post-processing | Corrections in eval/corrections.rs — **not** in the weight file |

**Bullet architecture target:** `HalfKA → 256×2 → 32 → 32 → 1`. Q2-01 **must** verify feature indices match `feature_index()` exactly (fixture parity test).

**Critical open item for Q2-01:** Bullet's `HalfKA` indexer may differ on king inclusion / mirroring vs OpenChess. Do not train until a shared fixture set proves bit-identical active features.

### 1.3 Training data format (Q2-01 → Bullet)

1. **Generate** quiet/search-labeled positions with OpenChess (self-play or fixed-depth search).
2. **Store** as text lines: `FEN | score_cp | result` (`result` = `1.0` / `0.5` / `0.0`, **White-relative**).
3. **Convert:** `bullet-utils convert <in.txt> <out.bullet>`
4. **Shuffle + interleave** before serious training.

**License:** Prefer **self-play / OpenChess search labels**. Do **not** redistribute LC0 training shards or Stockfish Fishtest nets as default embed without license review.

### 1.4 Export: Bullet → `OCNNv002`

Bullet checkpoints write `raw.bin` / `quantised.bin` in column-major layout. **No stock exporter** for `OCNNv002`.

**Q2-01 deliverable:** `tools/nnue-export` (or similar) that:
1. Reads Bullet `quantised.bin` / custom SavedFormat
2. Maps FT + fc1/fc2/out weights to `Network::to_bytes()` layout
3. Sets `scale` (start with `16`)
4. Round-trips: `from_bytes(to_bytes(net))` + feature_index parity + startpos tactical smoke

### 1.5 Concrete next commands for Q2-02 (after Q2-01 lands)

```bash
cargo build --release
# datagen → convert → train → export → EvalFile smoke
# See phase2-engine-plan and tasks.md Q2-02 for full acceptance
```

**Note:** Current testing/sprt.sh does not pass EvalFile to cutechess. Q2-02 should add `--net-a` / `--net-b` or document manual cutechess flags.

---

## 2. UHO / large opening book (M2-01)

### 2.1 Best in-repo references

- testing/README.md — "Replace with UHO / 8moves_v3"
- testing/sprt.sh — `--book` flag, default testing/books/openings.epd
- testing/books/openings.epd — ~20 smoke positions (keep)
- research/openings.md §5.1
- src/book/epd.rs — play book; **Do not** point play book at UHO without task — M2-01 is SPRT-only

### 2.2 Recommended sources

**Primary (CC0):** https://github.com/official-stockfish/books
- `UHO_4060_v3.epd` (~242k) — **Default M2-01 target**
- `8mvs_+90_+99.epd` (~8.5k) — intermediate step
- Avoid UHO 2024 / commercial DB derivatives for redistribution

### 2.3 Layout proposal

```
testing/books/
  openings.epd          # smoke (keep)
  uho_4060_v3.epd       # gitignored, fetched
  README.md             # fetch instructions + SHA256
```

**Policy:** SPRT → OwnBook=false; play/Lichess → OwnBook=true.

### 2.4 Commands

```bash
git clone --depth 1 https://github.com/official-stockfish/books.git /tmp/sf-books
cp /tmp/sf-books/UHO_4060_v3.epd testing/books/uho_4060_v3.epd
shasum -a 256 testing/books/uho_4060_v3.epd
./testing/sprt.sh --book testing/books/openings.epd --st 5 --games 40
./testing/sprt.sh --book testing/books/uho_4060_v3.epd --st 8 --games 2000 --concurrency 4
```

---

## 3. Syzygy (Wave 3 / K2-01 sketch)

Defer to Wave 3. Options: Fathom FFI (Reckless pattern) vs shakmaty-syzygy (pure Rust, GPL-3.0). Default: feature-gated, no TB in default CI. Call sites: WDL in main search; DTZ at root; skip heavy probes in qsearch; UCI SyzygyPath.

---

## 4. Open questions & Wave 1 "do not touch"

### Resolve in Q2-01 (blocking Q2-02)
1. Feature parity: Bullet HalfKA vs feature_index()
2. Exporter quantization / scale
3. Fixture datagen first
4. Two hidden layers in Bullet
5. EvalFile in SPRT harness

### Wave 1 hard avoid
| Agent | Avoid |
|---|---|
| Q2 | testing/books/, selectivity.rs, OCNNv002 magic churn |
| M2 | src/eval/nnue/** |
| S2-02 | selectivity.rs, nnue/ |
| Everyone | margin retune before trained net; OwnBook in SPRT; committing 240k EPD or TBs; LC0 data without license |

---

## 5. Ownership quick reference (Wave 1)

| Slot | Branch hint | Primary paths |
|---|---|---|
| Q2-01 | phase2/q2-01-data-pipeline | tools/, research/, nnue export tests only |
| M2-01 | phase2/m2-01-uho-book | testing/books/, sprt.sh, testing/README.md |
| S2-02 | phase2/s2-02-see-promo | src/board/see.rs |
| Explore | — | This brief; no src/ edits |
