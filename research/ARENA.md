# Arena Lab — Bulk Bot-vs-Bot Battles

> **Audience:** agents implementing OpenChess **P11 Arena lab** — running many concurrent local Bot-vs-Bot games for development, tuning, and observation.
> **Companion docs:** [ARCHITECTURE.md](../ARCHITECTURE.md) §3 (dual fronts) · [tasks-phase1.md § P11](./tasks-phase1.md#p11--arena-lab-bulk-bot-vs-bot) · [chesswiki §7](./chesswiki.md#7-measurement--elo) (informal dev runs vs SPRT) · [LICHESS.md](./LICHESS.md) (online play, contrast) · [openings.md](./openings.md) (book hook, TUI-04)
> **Task board:** pillar **P11** (P11-01..P11-09)
>
> **Not** formal SPRT ([tasks.md § M2](./tasks.md#m2--measurement) / Phase 1 [P8-03](./tasks-phase1.md#p8--scale--science) owns `testing/sprt.sh`). **Not** online Lichess ([tasks.md § L2](./tasks.md#l2--lichess-go-live) / Phase 1 [P9](./tasks-phase1.md#p9--lichess-bot-cli)). **Not** the single-game human TUI ([P7b](./tasks-phase1.md#p7b--terminal-ui-ratatui)). The arena is an in-process lab for watching and tuning the engine against itself at scale.

---

## 1. One-sentence model

**A bulk bot battle = `N` independent local game slots (each its own `Board` + move history + per-side strength + optional background search) advanced by a fair scheduler, observable live through read-only snapshots, and driven by either a headless batch runner (`arena run`) or an interactive inspector TUI (`arena`/`arena watch`).**

The arena reuses the exact engine the TUI and UCI already call — `search::go` behind a background thread — but owns *many* concurrent sessions instead of one. Isolation between slots is the core invariant: no slot may observe or mutate another slot's board, TT, or histories.

---

## 2. Why an arena (vs what already exists)

| Capability | Where it lives today | Gap the arena fills |
|---|---|---|
| One Bot-vs-Bot game, live | `tui/session.rs` `PlayMode::BotVsBot` | Only **one** game; blocks on a single ratatui screen |
| Per-side strength (White vs Black) | `config::SideStrength`, `play_go_limits` | Single game; no per-slot override at runtime |
| Formal strength verdict | `testing/sprt.sh` (cutechess/OpenBench) | External process, unattended, **no live inspection** |
| Online rated feedback | `lichess/` (P9) | Network, one game, no local batch tuning |

**Sweet spot:** "start 16 self-play games at mixed strengths, walk away for a batch summary, *or* tab through them live to watch where the engine goes wrong." This is the informal-development counterpart to SPRT — fast feedback, no Elo claim.

---

## 3. Relationship to existing modules (reuse, don't duplicate)

The arena must **not** re-implement search threading. `tui/session.rs` already contains the whole pattern:

- `EngineSession::spawn_search(board, limits) -> LiveSearch` — clones the `Board`, spawns a thread running `search::go` with a private `TranspositionTable`, and publishes a `LiveInfoSnapshot` (depth/score/nodes/time/pv) via `Arc<Mutex<..>>` plus a final `SearchResult`.
- `LiveSearch { stop: Arc<AtomicBool>, result: Arc<Mutex<Option<SearchResult>>>, live_info: Arc<Mutex<LiveInfoSnapshot>>, handle }` — poll-and-join lifecycle.
- `poll_bot()` / `finish_search()` — non-blocking "is the result ready?" then apply the move.
- `config::SideStrength { depth, movetime_ms }` and `Config::side_go_limits(color)` — per-color limits.
- `board::GameResult` — `Ongoing | Checkmate | Stalemate | DrawRepetition | DrawFiftyMove | DrawInsufficientMaterial` and `is_over()`.
- `tui/game.rs` `AnalyzedGame` / `PlyRecord` — SAN + UCI move transcript already used by the move panel.
- `tui/san.rs` `format_san` — SAN rendering for PGN / move lists.

**P11-09 (shared session refactor)** extracts the search-spawn/poll/limits plumbing out of `EngineSession` into a reusable `GameSession` so both `tui/` and `arena/` call one implementation. Do this *early-ish* but not first: land the arena against a thin copy or a minimal extracted core, prove it works (P11-01/02), then converge the TUI onto it (P11-09) so TUI behavior is verified unchanged.

---

## 4. Module layout (proposed)

New crate-internal module `src/arena/`, mirroring the `tui/` / `lichess/` front pattern. No search logic inside; it orchestrates sessions.

```
src/
├── arena/
│   ├── mod.rs          # re-exports, ArenaError, `run(args)` CLI dispatch
│   ├── slot.rs         # GameSlot: Board + transcript + SideStrength + status + LiveSearch
│   ├── runner.rs       # Arena: Vec<GameSlot> + fair scheduler tick()
│   ├── snapshot.rs     # GameSnapshot (read-only live view for inspectors)
│   ├── profile.rs      # ArenaProfile { white, black } named strength presets
│   ├── batch.rs        # headless `arena run` loop + stdout summary + PGN dir
│   ├── export.rs       # PGN writer + JSONL event stream + session log
│   ├── watch.rs        # ratatui inspector: game list + detail drill-down
│   └── cli.rs          # arg parse: `arena run|watch` (+ bare `arena` → watch)
```

**`main.rs` dispatch** (alongside `tui` / `uci` / `lichess`):

```text
Some("arena") => openchess::arena::run(args),
```

Behind a Cargo feature? **No** — the arena reuses only in-tree deps (`search`, `ratatui`, `crossterm` already pulled in by `tui`). Keep it in the default build so `cargo test` exercises it. (If ratatui-free batch use is ever needed, split `watch.rs` behind a feature later.)

---

## 5. Core data model

### 5.1 `GameSlot` (`slot.rs`)

Owns one game. Isolation: each slot clones its own `Board` and, when searching, `spawn_search` gives it a **private** `TranspositionTable` (as `EngineSession::spawn_search` does today) — no shared TT across slots.

```text
struct GameSlot {
    id: usize,
    board: Board,                 // startpos or a seeded opening line
    start_fen: String,            // for PGN [FEN] + replay
    transcript: Vec<PlyRecord>,   // reuse tui::game::PlyRecord (SAN + UCI)
    white: SideStrength,          // per-slot White limits
    black: SideStrength,          // per-slot Black limits
    profile: Option<String>,      // name of applied ArenaProfile, if any
    status: SlotStatus,
    live: Option<LiveSearch>,     // background search for the side to move
    last_info: SearchInfo,        // most recent depth/score/nodes/time
    last_move: Option<Move>,
    result: GameResult,
    ply_limit: usize,             // hard cap (e.g. 400 plies) → adjudicate draw
}

enum SlotStatus { Idle, Thinking, Paused, Finished }
```

`SlotStatus` vs `GameResult`: `status` is the *scheduler* state (is a search in flight / is the slot runnable), `result` is the *chess* outcome. A slot becomes `Finished` when `result.is_over()` or `ply_limit` is hit.

**Slot lifecycle (per move):**

1. Scheduler picks a runnable slot (`Idle`, not game-over).
2. `limits = side_go_limits(board.side_to_move())` from this slot's `white`/`black`.
3. `live = spawn_search(board.clone(), limits_to_search(limits))`; `status = Thinking`.
4. On a later tick, if `live.result` is `Some`, join, apply `best_move` to `board`, push `PlyRecord`, recompute `result`, set `status = Idle` (or `Finished`).

This is `EngineSession::poll_bot` / `finish_search` generalized to N slots.

### 5.2 `Arena` + scheduler (`runner.rs`)

```text
struct Arena {
    slots: Vec<GameSlot>,
    max_concurrent_searches: usize,  // cap live threads (default = num_cpus or a config value)
    rng: ...,                        // for weighted book / opening seeding (P11 + TUI-04)
}
```

**Fair scheduling (P11-01 acceptance = "advance one thinking slot at a time"):** the runner does not spawn N searches at once on N cores blindly — that starves interactive responsiveness and can oversubscribe CPUs. Two supported modes:

- **Serial (default, deterministic-ish):** at most one search in flight; round-robin over runnable slots so no slot is starved. Simplest to reason about; matches the "advance one thinking slot at a time" acceptance.
- **Bounded parallel (opt-in):** up to `max_concurrent_searches` live threads, filled round-robin. Each search is single-threaded (`threads: 1`) so the arena controls total CPU load; do **not** stack Lazy SMP (`Threads>1`) per slot on top of many slots.

`tick()` is non-blocking: poll finished searches, apply their moves, then top up runnable slots to the concurrency cap. The batch runner calls `tick()` in a loop with a short sleep; the inspector calls `tick()` from its event loop between input polls (same shape as `tui/mod.rs`).

### 5.3 `GameSnapshot` (`snapshot.rs`, P11-03)

Read-only, cloneable view an inspector can take **while other slots keep searching**. Never holds a lock across the UI render.

```text
struct GameSnapshot {
    id: usize,
    fen: String,
    ply: usize,
    side_to_move: Color,
    transcript: Vec<String>,        // SAN (or UCI) tokens for the move panel
    last_move: Option<String>,
    info: SearchInfo,               // depth/score/pv/nodes/time (reuse tui::session::SearchInfo)
    eval_white_cp: Option<i32>,     // White-relative eval (stm_score_to_white)
    material: MaterialBalance,      // per-side piece counts + centipawn sum
    status: SlotStatus,
    result: GameResult,
    profile: Option<String>,
}
```

`eval_white_cp` reuses `tui::session::stm_score_to_white`. `material` reuses the P6 material eval / the existing `tui/material.rs` counting so the "material matches manual count" acceptance holds. Snapshots are built from the slot's `last_info` + `board`, not by starting new searches.

### 5.4 `ArenaProfile` (`profile.rs`, P11-07)

```text
struct ArenaProfile { name: String, white: SideStrength, black: SideStrength }
```

Named presets in config/TOML (shape matches `config.json` `bot.white` / `bot.black`). Assigning a profile to a slot copies its `white`/`black` into the slot and records `profile = Some(name)`. Supports a tournament layout like "strong vs weak" across many slots, alternating colors.

---

## 6. CLI surface

```
openchess arena run   --games N [--depth D] [--movetime MS]
                      [--white-depth D --white-movetime MS]
                      [--black-depth D --black-movetime MS]
                      [--profile FILE] [--concurrency K]
                      [--pgn-dir DIR] [--jsonl] [--max-plies N]
openchess arena watch                # interactive inspector (ratatui)
openchess arena                      # bare → watch
```

- **`arena run`** (P11-02): headless. Advance all slots to completion; print a summary to stdout (`White wins / draws / Black wins`, average plies, per-profile breakdown). Exit 0 on clean completion. Optional `--pgn-dir` writes one PGN per finished game; `--jsonl` emits a `move`/`eval`/`finish` event stream for scripting.
- **`arena watch`** (P11-04): open the inspector TUI on a running arena.

Argument parsing follows the existing hand-rolled style in `chesscom/cli.rs` / `lichess/cli.rs` (no new arg-parser dependency).

---

## 7. Inspector TUI (P11-04)

Two-pane ratatui layout, reusing `tui/board_view.rs`, `tui/eval_bar.rs`, `tui/move_list.rs`, `tui/material.rs`, `tui/engine_panel.rs`:

```
┌ Games ─────────────────┐┌ Detail (slot 3) ───────────────┐
│> 0  Rxx  e4 …   +0.3 ▶ ││   [board]      eval ▏+0.7       │
│  1  Dxx  Nf3 …  =0.0 ▶ ││   1. e4 e5 2. Nf3 Nc6 …         │
│  2  Wxx  …      #     ││   material: = (0)               │
│  3  Bxx  …    -1.2 ▶ ││   engine: d12 -120 12k n 240ms  │
│ …                      ││                                 │
└────────────────────────┘└─────────────────────────────────┘
```

- **Game list:** id, ply, last move (SAN), White-relative eval, status glyph (`▶` thinking / `⏸` paused / `#`/`½` finished).
- **Detail pane:** board (flip-able), full move list, eval bar, material line, engine panel (depth/score/PV/nodes/time) when that slot is thinking.
- **Keys:** `↑/↓` or number to select a slot, `Enter` drill in, `Esc`/`q` back to list, `f` flip, plus control/edit keys below. Crucially, **the arena keeps ticking while you inspect** — returning to the list shows all slots advanced.

Blocking rule (same as LICHESS §11.0 / ratatui): searches run on background threads; the event loop only polls snapshots and results. Never call `search::go` on the UI thread.

---

## 8. Runtime control & editing

### 8.1 Per-slot strength editing (P11-05)

Inspector overlay edits the selected slot's `white`/`black` `SideStrength` (depth + movetime), or applies a named profile. **Changes take effect on that side's next move** — never interrupt an in-flight search (let it finish, then the new limits apply). Optional "mirror to all slots". Clamp with the same `MIN/MAX_DEPTH`, `MIN/MAX_MOVETIME_MS` bounds as `config.rs`.

### 8.2 Per-slot game control (P11-06)

- **Pause/resume:** `Paused` slots are skipped by the scheduler; other slots keep going. Pausing sets the stop flag on any in-flight search and discards its move (or lets it finish and holds — prefer: let finish, then hold, to avoid wasted work). Resuming returns the slot to `Idle`.
- **Restart:** new game, same strengths → `board = startpos()` (or `start_fen`), clear transcript/result.
- **Step one move:** when paused, manually advance exactly one ply (spawn one search, apply, re-pause).
- **Abort:** stop search, mark `Finished` with no result (adjudicated `*`).

### 8.3 Draw / length adjudication

Long self-play games between weak bots can wander. Enforce `ply_limit` (default e.g. 300–400 plies) → adjudicate a draw and mark `Finished`. Also honor `GameResult` draws (repetition/50-move/insufficient/stalemate) exactly as `EngineSession` already does — see the `threefold_repetition_ends_game_and_stops_bots` test as the reference behavior.

---

## 9. Export & session log (P11-08)

- **PGN per finished game:** new `export.rs` writer building standard PGN from `start_fen` + `transcript` SAN (via `tui/san.rs`), with `[Event "OpenChess Arena"]`, `[White]/[Black]` = strength/profile label, and `[Result]` from `GameResult`. There is no PGN *writer* yet (only readers under `chesscom`/`tui/import.rs`); this task adds one. Reuse the P9-06 export shape when it lands, but the arena writer is in-tree (no network).
- **Session log:** append `{result, plies, profile}` per finished slot to a session log file (under `~/.cache/openchess/arena/` or `--pgn-dir`).
- **JSONL event stream (`--jsonl`):** one JSON object per line — `{"type":"move", "slot":3, "ply":21, "uci":"g1f3", "eval_cp":34}`, `{"type":"finish", "slot":3, "result":"draw", "plies":86}`. Enables external tooling to tail results. `serde_json` is already a dependency.

---

## 10. Concurrency & correctness invariants

1. **Slot isolation:** each `GameSlot` owns its `Board`; `spawn_search` clones the board and uses a **per-search** `TranspositionTable`. No shared mutable state between slots. (Acceptance P11-01: "slots do not share board state".)
2. **Single-threaded per search:** searches run with `Limits.threads = 1`. The arena scales by running *more games*, not by giving each game Lazy SMP; that keeps CPU accounting predictable and avoids N×M thread explosions.
3. **Bounded concurrency:** cap live search threads (`max_concurrent_searches`) so an arena of 64 slots does not spawn 64 threads on an 8-core box.
4. **Non-blocking ticks:** `tick()` and snapshot reads never block on a search; they poll `LiveSearch.result` / `live_info` (the `try`-style pattern already in `poll_bot`).
5. **Clean teardown:** on quit/abort, set every slot's stop flag and join threads (mirror `stop_thinking_quiet`).
6. **Determinism caveat:** wall-clock `movetime` limits make games non-deterministic. For reproducible batches, prefer `--depth`/`--nodes` limits (nodes fully deterministic given fixed net + single thread).

---

## 11. Testing strategy

Arena code is library code → unit/integration tests, no live UI needed (same discipline as `EngineSession` tests):

- **Slot plays to a result:** with tiny limits (`depth: 1`), a slot reaches `Finished` with a legal `GameResult` and never an illegal move (reuse the movegen/legality guarantees; assert transcript legality by replaying from `start_fen`).
- **Isolation:** `Arena::new(4)` then advance to completion; assert four independent transcripts, none sharing board state, no panics.
- **Scheduler fairness:** every runnable slot advances (no slot stuck at ply 0 while others finish).
- **Snapshot while thinking:** take a `GameSnapshot` of slot A while slot B searches; material count matches a hand-computed fixture; eval updates after a move.
- **Strength edit timing:** raising a side's depth is reflected on that side's *next* search, not the current one.
- **Adjudication:** a forced repetition ends the slot as `DrawRepetition` (port the existing session test); `ply_limit` forces a draw finish.
- **PGN round-trip:** export a finished slot, re-import via the existing PGN reader, assert same moves.

Keep per-test limits shallow (`depth: 1`, or short movetime) so `cargo test` stays fast, exactly as `session.rs` tests do.

---

## 12. Phased build order (maps to P11 tasks)

| Phase | Tasks | Milestone |
|---|---|---|
| **A — Engine of the arena** | P11-01, P11-02 | `Arena` with N isolated slots; headless `arena run --games N` completes unattended with a stdout summary |
| **B — Observe** | P11-03, P11-04 | Live `GameSnapshot` + inspector TUI; tab through games mid-flight |
| **C — Control** | P11-05, P11-06 | Runtime per-slot strength edits + pause/resume/restart/step/abort |
| **D — Scale & record** | P11-07, P11-08 | Named profiles + slot assignment; PGN/JSONL/session-log export |
| **X — Converge** | P11-09 | Extract shared `GameSession` from `tui/session.rs`; TUI + arena share one search-spawn path (TUI behavior unchanged) |

Dependencies (from [tasks-phase1.md](./tasks-phase1.md)): P11-01 needs P2-02 (iterative deepening ✅), P7-02 (time mgmt ✅), P1-10 (perft ✅) — **all met**; P11 shipped complete. TUI-04 / P10 book hooks also landed.

---

## 13. Open questions

1. **Concurrency default:** serial (one search at a time) for the first cut, or bounded-parallel keyed to core count? (Recommend serial for P11-01 acceptance, add a `--concurrency` flag in P11-02.)
2. **Opening diversity:** seed slots from `book/` (P10) or a fixed EPD opening set so self-play isn't all the same line? Defer to TUI-04/P10 landing; expose a `--openings FILE` hook.
3. **Where to extract `GameSession`:** `src/session/` (new top-level) vs `arena/game.rs` reused by TUI? ARCHITECTURE §3 hints "extract shared session from `tui/session.rs`". Recommend a `session` module that both `tui/` and `arena/` depend on (P11-09).
4. **Result adjudication policy:** default ply cap value, and whether to also adjudicate by eval (e.g. |eval| > 10 for K consecutive plies → resign) as SPRT harnesses do.
5. **Determinism knob:** expose `--nodes` limit for reproducible batches, given movetime is wall-clock non-deterministic.

---

## 14. References

- `src/tui/session.rs` — `EngineSession`, `spawn_search`, `LiveSearch`, `poll_bot`, `finish_search`, `stm_score_to_white` (the pattern to generalize)
- `src/config.rs` — `SideStrength`, `Config::side_go_limits`, clamp bounds
- `src/search/mod.rs` — `go`, `Limits`, `SearchResult`, `ThreadData`
- `src/board/draw.rs` — `GameResult`
- `src/tui/game.rs` — `AnalyzedGame`, `PlyRecord`, `MoveClass`
- [tasks-phase1.md § P11](./tasks-phase1.md#p11--arena-lab-bulk-bot-vs-bot) — completed Phase 1 arena checklist
- [ARCHITECTURE.md](../ARCHITECTURE.md) §3 (module layout), §7 (data flow), §8 (concurrency)
- [chesswiki §7](./chesswiki.md#7-measurement--elo) — informal dev runs vs SPRT gate

---

*Arena lab plan drafted 2026-07-12 from the P11 pillar and the existing `tui/session.rs` engine-session pattern.*
