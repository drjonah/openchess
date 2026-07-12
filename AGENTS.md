# AGENTS.md

OpenChess is an open-source chess engine written in Rust.

## Context

- Early-stage project; prefer simple, correct Rust over premature optimization.
- Rust module layout and ownership: [ARCHITECTURE.md](ARCHITECTURE.md).
- Agent task board: [research/tasks.md](research/tasks.md).
- Design research lives in `research/` — read those when exploring search/eval ideas.
- Lichess bot integration (CLI only, no TUI): [research/LICHESS.md](research/LICHESS.md) · tasks **P9**.
- Stockfish-family paradigm; copy structure from research, not magic constants. Speculative ideas stay in `research/uniqueideas.md`.

## Conventions

- Idiomatic Rust; keep modules small and focused.
- Prefer clarity in board representation, move generation, and search until benchmarks justify complexity.
- Follow standard rust doc standards.

