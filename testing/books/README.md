# Opening books for SPRT / OpenBench

| File | Role | Positions | Source / license |
|------|------|-----------|------------------|
| `openings.epd` | Smoke / harness check; also embedded by `src/book/epd.rs` for play-book graph | 20 | OpenChess (hand-authored) |
| `8mvs_+90_+99.epd` | **Default strength SPRT** book (UHO / 8-moves class) | 8533 | [official-stockfish/books](https://github.com/official-stockfish/books) · **CC0-1.0** |

## Strength book provenance (M2-01)

`8mvs_+90_+99.epd` is the Unbalanced Human Openings (UHO) 8-move set with white eval in roughly **+0.90 … +0.99** (centipawn scale used by the packagers). It is redistributed unchanged from the Stockfish books collection:

- Upstream: https://github.com/official-stockfish/books (`8mvs_+90_+99.epd.zip`)
- License text: [`LICENSE-CC0-stockfish-books.txt`](LICENSE-CC0-stockfish-books.txt) (CC0 1.0 Universal)
- Typical depth: 8 moves / 16 plies from startpos

Do **not** replace `openings.epd` with this file — the smoke EPD is `include_str!`'d into the engine binary for the interactive OwnBook graph.

## Larger UHO (optional)

For longer SPRTs closer to Fishtest scale, download a bigger pack from the same CC0 repo (not committed here to keep the tree small), e.g.:

```bash
# ~242k positions — UHO_4060_v4
curl -sL -o /tmp/uho.zip \
  https://raw.githubusercontent.com/official-stockfish/books/master/UHO_4060_v4.epd.zip
unzip -o /tmp/uho.zip -d testing/books/
./testing/sprt.sh --book testing/books/UHO_4060_v4.epd --st 8 --games 2000
```

Also available: `8mvs_big_+80_+109.epd.zip` (~26k), `8moves_v3.pgn.zip` (PGN — pass `format=pgn` via a custom cutechess invocation).
