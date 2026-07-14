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

Run many concurrent local self-play games for development and tuning. This is an in-process lab — not formal SPRT (`testing/sprt.sh`) and not online Lichess.

```bash
openchess arena run …    # headless batch; prints a W/D/L summary
openchess arena watch …  # interactive inspector (default if you omit the subcommand)
openchess arena …        # same as watch
```

Prefer a release build for usable think times:

```bash
cargo build --release
./target/release/openchess arena watch --games 4 --depth 6 --concurrency 2
```

#### Headless batch (`arena run`)

Advances all games to completion, then prints a one-line summary:

```text
games=8 white_wins=3 black_wins=2 draws=3 unfinished=0 avg_plies=42.5
```

```bash
# 8 games at depth 8, up to 4 searches in flight, write one PGN per game
cargo run --release -- arena run --games 8 --depth 8 --concurrency 4 --pgn-dir /tmp/arena

# Asymmetric strength (odd slots swap colors so each side plays both colors)
cargo run --release -- arena run --games 16 --white-depth 12 --black-depth 6 --concurrency 4

# Stream JSONL move/finish events on stdout (summary goes to stderr)
cargo run --release -- arena run --games 4 --depth 4 --jsonl
```

Useful flags (shared by `run` and `watch`):

| Flag | Meaning |
|------|---------|
| `--games N` | Concurrent game slots (default 1) |
| `--depth D` | Search depth for both sides (default 6) |
| `--movetime MS` | Per-move time limit; `0` = depth-only |
| `--white-depth` / `--black-depth` | Per-side depth override |
| `--white-movetime` / `--black-movetime` | Per-side movetime override |
| `--concurrency K` | Max searches in flight (default 1 = serial) |
| `--hash MB` | Per-search TT size (default 8) |
| `--max-plies N` | Adjudicate a draw after N plies (default 400) |
| `--pgn-dir DIR` | Write one PGN per finished game (`run` only) |
| `--jsonl` | Emit JSONL `move`/`finish` events (`run` only) |
| `--profile FILE` | JSON strength profiles assigned across slots |
| `--no-alternate-colors` | Keep White/Black strengths fixed (no color swap) |

#### Interactive inspector (`arena watch`)

Live two-pane UI: game list on the left, selected game (board, moves, eval, engine panel) on the right. Games keep advancing while you browse.

```bash
cargo run --release -- arena watch --games 4 --depth 6 --concurrency 2
```

| Key | Action |
|-----|--------|
| `↑` / `↓` (or `k` / `j`) | Select slot |
| `f` | Flip board |
| `p` / `r` | Pause / resume selected slot |
| `n` | Restart selected slot |
| `s` | Step one ply (when paused) |
| `a` | Abort selected slot |
| `[` / `]` | Lower / raise depth for the side to move (next move) |
| `{` / `}` | Lower / raise movetime for the side to move |
| `m` | Mirror this slot’s strengths to all slots |
| `q` / `Esc` | Quit |

Design notes: [research/ARENA.md](research/ARENA.md) · Phase 1 tasks **P11** in [research/tasks-phase1.md](research/tasks-phase1.md).

### Lichess bot (optional)

Lichess support is feature-gated and CLI-only — no TUI panel. You need a dedicated [bot account](https://lichess.org/api#tag/bot) and a personal API token. **Never commit the token.**

#### Setup

1. Register a fresh Lichess account that has **not** played any games.
2. Create a [Personal API access token](https://lichess.org/account/oauth/token/create) with bot play + challenge scopes.
3. Export it (see [`.env.example`](.env.example)):
   ```bash
   export LICHESS_TOKEN='…'   # do not put real tokens in git
   ```
4. One-time irreversible upgrade:
   ```bash
   curl -d '' https://lichess.org/api/bot/account/upgrade \
     -H "Authorization: Bearer $LICHESS_TOKEN"
   ```
5. Verify:
   ```bash
   cargo run --features lichess -- lichess account
   ```
   Expect `title: BOT`.

#### Smoke checklist (casual)

Safe defaults: **casual only**, **bots preferred** (`accept_humans=false`), **one game**, **ponder off**. Copy [`examples/lichess.toml`](examples/lichess.toml) if you want a file-driven policy.

```bash
# 1) Dry-run — connect and log events; never POST
cargo run --features lichess -- lichess run --dry-run
# optional: --config examples/lichess.toml

# 2) Play mode — accept filtered challenges and play (still casual / bots-only by default)
cargo run --features lichess -- lichess run --play --config examples/lichess.toml

# 3) Outbound casual challenge (5+3) to a known weak bot, then play in the run process above
cargo run --features lichess -- lichess challenge <bot-username> \
  --clock-limit 300 --clock-increment 3
```

CLI flags override the config file (`--accept-rated`, `--accept-humans`, `--bots-only`, `--speeds blitz,rapid`, `--token-env VAR`). Do **not** pass `--accept-rated` until the L2-06 strength gate in [research/tasks.md](research/tasks.md).

After a game finishes, PGN is written under the OpenChess cache (`…/openchess/lichess/{gameId}.pgn`). Watch the live board on lichess.org.

#### Ops notes

- **Ponder:** always off on the Lichess path (search only on our turn).
- **Rated:** blocked by default; enabling it is an explicit ops decision after engine strength evidence (L2-06).
- **Concurrency:** still one game per process until L2-07; `max_concurrent_games` in the config is clamped to `1`.

Details: [research/LICHESS.md](research/LICHESS.md) · Phase 2 **L2** in [research/tasks.md](research/tasks.md) · Phase 1 CLI **P9** in [research/tasks-phase1.md](research/tasks-phase1.md).
