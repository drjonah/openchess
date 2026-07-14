# Local SPRT / self-play harness (P8-03 / M2-01)

Measure strength patches with fixed-time or fixed-node self-play before merging.

## Prerequisites

- [cutechess-cli](https://github.com/cutechess/cutechess) on `PATH`
- Release binary: `cargo build --release`

## Quick smoke (HCE vs HCE / same engine)

Harness wiring only — use the tiny smoke book:

```bash
./testing/sprt.sh --st 5 --games 40 --book testing/books/openings.epd
```

Same-binary matches should hover near 50%. This verifies cutechess + `OwnBook=false`, not Elo.

## Strength SPRT (default book)

By default `sprt.sh` seeds openings from the UHO-class book
`testing/books/8mvs_+90_+99.epd` (8533 positions, CC0 — see
[`books/README.md`](books/README.md)).

```bash
# baseline = main release binary, candidate = your build
./testing/sprt.sh \
  --engine-a ./target/release/openchess \
  --engine-b ./target/release/openchess-candidate \
  --st 8 \
  --games 2000
```

Override the book when needed:

```bash
./testing/sprt.sh --book testing/books/openings.epd          # smoke
./testing/sprt.sh --book /path/to/UHO_4060_v4.epd            # larger UHO
```

## SPRT accept / reject

Default SPRT bounds (Stockfish-family style, Elo scale):

| Parameter | Default | Meaning |
|-----------|---------|---------|
| Elo0 | 0 | H0: no gain |
| Elo1 | 5 | H1: +5 Elo |
| Alpha | 0.05 | Type I error |
| Beta | 0.05 | Type II error |

`sprt.sh` passes these to cutechess `-sprt`. **Accept** means keep the patch; **reject** means revert or retune.

## OpenBench

[`openbench.json`](openbench.json) is a starter worker config sketch for [OpenBench](https://github.com/AndyGrant/OpenBench). Point `source` / `build` at this repo once you run a private instance; local `sprt.sh` is enough for day-to-day patches. The default book path matches the UHO-class SPRT book above.

## Opening books

| Book | Use |
|------|-----|
| `books/8mvs_+90_+99.epd` | Default for serious / strength SPRTs (M2-01) |
| `books/openings.epd` | Fast smoke only (`--book …`) |

Provenance, license (CC0), and optional larger UHO downloads: [`books/README.md`](books/README.md).

### Book policy: SPRT vs play (P10-07)

There are two distinct uses of openings, and they must stay separated:

- **Strength SPRT** measures search + eval quality. It runs with the engine's
  internal opening book **off** (`option.OwnBook=false`, already set by
  `sprt.sh`) and lets cutechess seed **fixed, shared** openings from the EPD
  suite (default: UHO-class `8mvs_+90_+99.epd`). This keeps both engines on
  identical lines so the result reflects the patch, not book luck.
- **Interactive play** (TUI / GUI / Lichess bot) keeps the internal book
  **on** by default (`OwnBook true`), so the engine opens with human-sensible
  theory (`e4` / `d4` / `Nf3` / `c4`) instead of relying on shallow search.

Do not enable `OwnBook` for strength runs, and do not remove EPD seeding from
`sprt.sh` — the two mechanisms are complementary, not interchangeable.
