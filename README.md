# OpenChess

Free, open-source chess engine written in Rust. Includes a terminal UI for play, plus a UCI mode for use with chess GUIs.


## Repository structure

```
openchess-tui/
├── src/           # Engine library + binary (board, search, eval, tui, uci, …)
├── tests/         # Integration tests (perft, movegen, etc.)
├── research/      # Design notes and task board
├── ARCHITECTURE.md
├── AGENTS.md
├── Cargo.toml
└── README.md
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for module layout and design details.

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
