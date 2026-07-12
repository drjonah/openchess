# Contributing to OpenChess

## Correctness gates (required)

Before opening a PR that touches board, search, or eval:

```bash
./scripts/ci.sh
```

This runs unit/integration tests, the fixed-node bench signature, and a UCI smoke check.

## Strength patches (required)

Functional changes that can affect playing strength (search, eval, selectivity, ordering, TM) must include **at least one** of:

1. **Fixed-node bench note** — `bench` total nodes (and depth) before/after, with a short explanation if the signature changes.
2. **SPRT / self-play result** — link or paste output from the local harness in [`testing/`](testing/).

Do not merge untuned pruning/margin cargo-cult without measurement.

## Local SPRT

See [`testing/README.md`](testing/README.md). Short path:

```bash
cargo build --release
./testing/sprt.sh --st 8 --games 200
```

Requires [cutechess-cli](https://github.com/cutechess/cutechess) on `PATH`.

## Scope notes

- One selective-search feature at a time (see [research/tasks.md](research/tasks.md)).
- Copy structure from Stockfish/Reckless research docs; retune constants with SPRT.
- Speculative ideas stay in [research/uniqueideas.md](research/uniqueideas.md) — not the default strength path.
