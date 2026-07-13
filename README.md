# OpenChess

Free, open-source chess engine written in Rust. Includes a terminal UI for play, a UCI mode for chess GUIs, an arena lab for bulk Bot-vs-Bot self-play, and an optional Lichess bot client for headless online play.

## Repository structure

```
openchess-tui/
├── src/           # Engine library + binary (board, search, eval, tui, arena, uci, lichess, …)
├── tests/         # Integration tests (perft, movegen, etc.)
├── research/      # Design notes and task board
├── ARCHITECTURE.md
├── AGENTS.md
├── Cargo.toml
└── README.md
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for module layout and design details.
Contributing / SPRT: [CONTRIBUTING.md](CONTRIBUTING.md) · [testing/](testing/).

## Build and run

Requires a recent stable Rust (edition 2024).

```bash
cargo build
cargo run -- tui
```

For a release build:

```bash
cargo build --release
./target/release/openchess tui
```

With no arguments (`cargo run`), the binary starts in UCI mode instead of the TUI.

### Arena lab (Bot vs Bot)

Run many concurrent local self-play games for development and tuning — not formal SPRT.

```bash
# 8 games at depth 8, up to 4 searches in flight, write PGNs
cargo run --release -- arena run --games 8 --depth 8 --concurrency 4 --pgn-dir /tmp/arena

# Asymmetric strength (odd games swap colors for balance)
cargo run --release -- arena run --games 16 --white-depth 12 --black-depth 6 --concurrency 4

# Live text monitor
cargo run --release -- arena watch --games 4 --depth 6 --concurrency 2
```

Details: [research/ARENA.md](research/ARENA.md) · task board P11 in [research/tasks.md](research/tasks.md).

### Lichess bot (optional)

Lichess support is feature-gated and CLI-only — no TUI panel. Set `LICHESS_TOKEN` in your environment (see `.env.example`); you need a [bot account](https://lichess.org/api#tag/bot).

```bash
cargo run --features lichess -- lichess account
cargo run --features lichess -- lichess run --dry-run
```

Details: [research/LICHESS.md](research/LICHESS.md) · task board P9 in [research/tasks.md](research/tasks.md).
