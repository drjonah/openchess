# Local SPRT / self-play harness (P8-03)

Measure strength patches with fixed-time or fixed-node self-play before merging.

## Prerequisites

- [cutechess-cli](https://github.com/cutechess/cutechess) on `PATH`
- Release binary: `cargo build --release`

## Quick smoke (HCE vs HCE / same engine)

```bash
./testing/sprt.sh --st 5 --games 40
```

This launches two OpenChess workers under cutechess with the opening book in `books/`. Same-binary matches should hover near 50% — use it to verify the harness, not Elo.

## SPRT accept / reject

Default SPRT bounds (Stockfish-family style, Elo scale):

| Parameter | Default | Meaning |
|-----------|---------|---------|
| Elo0 | 0 | H0: no gain |
| Elo1 | 5 | H1: +5 Elo |
| Alpha | 0.05 | Type I error |
| Beta | 0.05 | Type II error |

`sprt.sh` passes these to cutechess `-sprt`. **Accept** means keep the patch; **reject** means revert or retune.

Typical workflow for a patch branch:

```bash
# baseline = main release binary, candidate = your build
./testing/sprt.sh \
  --engine-a ./target/release/openchess \
  --engine-b ./target/release/openchess-candidate \
  --st 8 \
  --games 2000
```

## OpenBench

[`openbench.json`](openbench.json) is a starter worker config sketch for [OpenBench](https://github.com/AndyGrant/OpenBench). Point `source` / `build` at this repo once you run a private instance; local `sprt.sh` is enough for day-to-day patches.

## Opening book

`books/openings.epd` is a short EPD set for smoke tests. Replace with a larger book (e.g. UHO / 8moves_v3) for serious SPRTs.
