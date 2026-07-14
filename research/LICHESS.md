# Lichess Bot API — Integration Research

> **Audience:** agents wiring OpenChess to play on Lichess (bot-vs-bot testing, live Elo feedback, regression games).
> **Companion docs:** [ARCHITECTURE.md](../ARCHITECTURE.md) · [chesswiki.md](./chesswiki.md) §6 (protocols)
> **Agent task board (Phase 2):** [tasks.md](./tasks.md) — pillar **L2 Lichess go-live** (L2-01..L2-07)  
> **Phase 1 archive:** [tasks-phase1.md](./tasks-phase1.md) — completed **P9** CLI/client (P9-01..P9-07)
> **Primary source:** [Lichess API reference — Bot](https://lichess.org/api#tag/bot/GET/api/stream/event) (OpenAPI v2.0.152)
> **Reference client:** [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) (Python bridge; de-facto spec for edge cases)
> **Rust client (optional dep):** [litchee](https://github.com/obazin/litchee) — async, full API coverage, NDJSON streaming

---

## 1. One-sentence model

**Lichess bot play = one long-lived NDJSON event stream + per-game NDJSON game streams + REST move posts — all authenticated with a Bot-account token; OpenChess sits in the middle calling its own search API instead of spawning a UCI subprocess.**

This is *not* UCI. Moves, clocks, and positions arrive as JSON. The engine lib (`Board`, `search`, `time`) is the brain; a thin `lichess/` I/O module is the network front — analogous to `uci.rs` and `chesscom/`, not a replacement for them.

---

## 2. Why Lichess bots (vs local testing only)

| Goal | Local (cutechess / SPRT) | Lichess bots |
|---|---|---|
| Correctness gates | Perft, fixed-node benches | Same, plus live legality under clock |
| Strength measurement | SPRT vs other engines | Rated games vs hundreds of public bots |
| Real-world I/O stress | UCI stdin parsing | HTTP streaming, reconnects, 429 backoff |
| Visibility | None | Bot TV, public game pages, PGN export |
| TOS | N/A | Must follow [fair-play rules](https://lichess.org/page/fair-play) |

**Sweet spot for OpenChess:** casual **rapid/blitz** challenges against known bots while search/eval are still immature — real games, low ceremony, no tournament infra. Rated games matter later once the engine is stable.

---

## 3. Bot API vs Board API

Lichess exposes two play APIs. **OpenChess must use Bot.**

| | **Board API** | **Bot API** |
|---|---|---|
| Account | Normal Lichess user | **Bot account** (`title: "BOT"`) |
| Engine use | **Forbidden** (TOS violation) | **Allowed** |
| Pools / seeks | Yes (`POST /api/board/seek`) | **No** — challenge games only |
| Tournaments | Yes | **No** |
| Endpoints | `/api/board/...` | `/api/bot/...` |
| Event stream | `GET /api/stream/event` (shared) | Same global stream |

The event stream URL is identical; path prefix on game ops differs (`/api/bot/game/...`).

---

## 4. Account setup

### 4.1 Create and upgrade

1. Register a **fresh** Lichess account (or dedicated alt). The account **must not have played any game** before upgrade.
2. Create a [Personal API access token](https://lichess.org/account/oauth/token/create) with scopes:
   - **Play bot moves** (required)
   - **Read incoming challenges** + **Create, accept, decline challenges** (for matchmaking)
   - Optionally **Read bot games** / chat scopes if we want PGN pull or spectator chat
3. Upgrade irreversibly:
   ```bash
   curl -d '' https://lichess.org/api/bot/account/upgrade \
     -H "Authorization: Bearer $LICHESS_TOKEN"
   ```
4. Verify: `GET /api/account` → `title` should be `"BOT"`.
5. Join the [Lichess Bots team](https://lichess.org/team/lichess-bots) (community norm, not strictly API).

**Irreversible:** the account can only play as a bot afterward. Use a throwaway account for dev.

### 4.2 Token hygiene

- Store in env (`LICHESS_TOKEN`), never commit.
- Tokens may be ≥512 chars; treat as opaque `^[A-Za-z0-9_]+$`.
- Revoke immediately if leaked.
- One token → one active global event stream (opening a new stream closes the previous).

---

## 5. Transport primitives

### 5.1 NDJSON streaming

Several endpoints return **newline-delimited JSON** (`application/x-ndjson`): one JSON object per line.

- **`GET /api/stream/event`** — global user events (challenges, game start/finish)
- **`GET /api/bot/game/stream/{gameId}`** — per-game state
- **`GET /api/bot/online`** — online bot list (no auth)

**Keepalive:** the event stream sends an **empty line every 7 seconds**. Treat empty lines as pings, not errors.

**Parsing pattern:** read line-by-line; `serde_json` each non-empty line; branch on `type` field.

### 5.2 Rate limiting

> Only make **one request at a time**. HTTP **429** → back off (often ~1 minute; some limits longer).

Implications for OpenChess:

- Do not fire move POSTs while a stream read is blocked unless using separate connections carefully; lichess-bot serializes via a client abstraction.
- UltraBullet (¼+0) is **disallowed for bots** (too many requests). Bullet 0+1 and ½+0 are OK.
- Prefer **one game per process** initially to stay under limits.

### 5.3 Move format

Moves are **UCI strings** in the API (same alphabet OpenChess already parses):

- Quiet/capture: `e2e4`, `e7e5`
- Promotion: `e7e8q` (lowercase piece letter)
- Castling: `e1g1`, `e8c8`

Game state carries the **full move list** as a space-separated UCI string in `state.moves`, not individual last-move events. On each `gameState` line, diff against the previous list length to detect new plies (or replay from `initialFen` + all moves — safer on reconnect).

---

## 6. Global event stream (`GET /api/stream/event`)

**Auth required.** This is the bot's "main loop".

### 6.1 Event types

| `type` | Meaning | OpenChess action |
|---|---|---|
| `gameStart` | New game (includes snapshot) | Spawn game handler; open game stream if not already embedded |
| `gameFinish` | Game over | Tear down handler; optionally export PGN |
| `challenge` | Incoming or outgoing challenge | Filter → accept/decline |
| `challengeCanceled` | Challenger withdrew | Clear pending state |
| `challengeDeclined` | Opponent declined our challenge | Matchmaking retry/backoff |

On connect, Lichess **replays all current challenges and active games** — handle idempotently (don't double-accept).

### 6.2 `gameStart` payload (high-signal fields)

```json
{
  "type": "gameStart",
  "game": {
    "gameId": "0FgNPGRz",
    "fullId": "0FgNPGRzhDaW",
    "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "color": "white",
    "variant": { "key": "standard" },
    "speed": "blitz",
    "rated": true,
    "isMyTurn": true,
    "secondsLeft": 300,
    "opponent": { "id": "...", "username": "...", "rating": 1200 }
  }
}
```

Use `gameId` for all `/api/bot/game/{gameId}/...` calls. `fullId` is the player-specific URL id.

**Clock units:** `gameStart.secondsLeft` is in **seconds** (rough snapshot). Per-move clocks in `gameState` use **milliseconds** (`wtime`, `btime`, `winc`, `binc`). Always convert `gameState` fields to `Duration::from_millis` for `time::TimeBudget`.

### 6.3 Incoming `challenge` payload (high-signal fields)

```json
{
  "type": "challenge",
  "challenge": {
    "id": "uGK4MHaQ",
    "status": "created",
    "speed": "rapid",
    "rated": false,
    "variant": { "key": "standard" },
    "challenger": { "id": "bernstein-2ply", "name": "bernstein-2ply", "rating": 1262 },
    "destUser": { "id": "openchess-bot", "name": "OpenChessBot" },
    "timeControl": { "type": "clock", "limit": 300, "increment": 1, "show": "5+1" },
    "color": "random",
    "finalColor": "white"
  }
}
```

Filter on `variant`, `speed`, `rated`, and `timeControl` before `POST /api/challenge/{id}/accept`. On accept, expect `gameStart` with `gameId == challenge.id`.

### 6.4 Architecture sketch (from lichess-bot)

```
┌─────────────────────────────────────────────────────────┐
│  Main loop (process or async task)                      │
│    read /api/stream/event → dispatch by event.type      │
└────────────┬───────────────────────────────┬────────────┘
             │ challenge                      │ gameStart
             ▼                                ▼
     accept/decline REST              ┌──────────────┐
     POST /api/challenge/{id}/accept   │ Game handler │
                                       │  game stream │
                                       │  search loop │
                                       │  POST move   │
                                       └──────────────┘
```

**Phase 1 for OpenChess:** single-threaded, **one concurrent game**, blocking `ureq` streams — matches existing `chesscom` HTTP style.

**Phase 2:** tokio + one task per active game (lichess-bot uses multiprocessing; Rust async is the natural analogue).

---

## 7. Per-game stream (`GET /api/bot/game/stream/{gameId}`)

Long-lived NDJSON. First line is always `gameFull`; subsequent lines are updates.

### 7.1 Event types

| `type` | Contents |
|---|---|
| `gameFull` | Static metadata + embedded `state` (players, variant, clock, `initialFen`, full `moves`) |
| `gameState` | `moves`, `wtime`, `btime`, `winc`, `binc`, draw/takeback flags, `status` |
| `chatLine` | `room` (`player` \| `spectator`), `username`, `text` |
| `opponentGone` | Opponent disconnected; fields for claim win/draw timing |

### 7.2 Clock fields (milliseconds)

`gameState` uses **milliseconds**, not seconds:

| Field | Meaning |
|---|---|
| `wtime` / `btime` | Remaining main time |
| `winc` / `binc` | Increment per move (ms) |
| `status` | `started`, `mate`, `resign`, `stalemate`, `draw`, `aborted`, … |

### 7.3 Making moves

```bash
POST /api/bot/game/{gameId}/move/{uciMove}
# optional: ?offeringDraw=true
```

Response: `{"ok": true}` or `400` with `{"error": "..."}`.

**Promotion:** must include piece in URL path, e.g. `POST .../move/e7e8q`.

### 7.4 Other game actions

| Action | Endpoint |
|---|---|
| Resign | `POST /api/bot/game/{gameId}/resign` |
| Abort | `POST /api/bot/game/{gameId}/abort` (early, both sides) |
| Draw offer/accept | `POST .../draw/{accept}` |
| Takeback | `POST .../takeback/{accept}` |
| Claim win (opponent left) | `POST .../claim-victory` |
| Claim draw (opponent left) | `POST .../claim-draw` |
| Chat | `POST .../chat` (`room`, `text` form body) |

OpenChess v1: implement **move**, **resign**, **abort**; ignore draw/takeback unless we add policy later.

---

## 8. Challenges & bot matchmaking

Bots cannot use pools. All games start from **challenges**.

### 8.1 Receive challenges

Incoming challenges arrive on the event stream (`type: "challenge"`). Accept:

```bash
POST /api/challenge/{challengeId}/accept
```

Decline:

```bash
POST /api/challenge/{challengeId}/decline
# optional body: reason=...
```

List pending: `GET /api/challenge` → `{ "in": [...], "out": [...] }`.

**Filter before accept** (config file):

- `variant.key == "standard"` only (OpenChess has no variants yet)
- `speed` in allowed set (`rapid`, `blitz`, `bullet`, …)
- `rated` true/false per dev vs production policy
- opponent rating band (avoid sandbagging accusations)
- time control sanity (`clock.limit`, `clock.increment`)

Lichess advises **casual** (`rated=false`) while debugging bot logic.

### 8.2 Challenge other bots

1. **Discover opponents:** `GET /api/bot/online?nb=100` (NDJSON stream of bot users; no auth).
2. **Create challenge:**
   ```bash
   POST /api/challenge/{username}
   Content-Type: application/x-www-form-urlencoded

   clock.limit=300&clock.increment=1&rated=false&color=random&variant=standard
   ```
3. Real-time challenges expire in **~20s** if not accepted. Use `keepAliveStream=true` on the create request to hold longer (see API docs).
4. On accept, `gameStart` fires; **`gameId` equals `challengeId`**.

### 8.3 Suggested bot-testing workflow

1. Start OpenChess lichess mode; accept incoming or poll `/api/bot/online`.
2. Challenge a weak known bot (e.g. tutorial bots) with `rated=false`, `5+3` rapid.
3. Log UCI move list + engine score vs game result.
4. Export PGN: `GET /game/export/{gameId}` (standard games API).
5. Scale to rated games only after abort/resign/move-error rate is ~0.

---

## 9. Mapping Lichess clocks → OpenChess `time::TimeBudget`

OpenChess already implements UCI-style TM in `src/time.rs` (`TimeBudget::from_limits`).

**Bridge from a `gameState` line when it's our turn:**

```text
wtime/btime  → Limits.wtime / Limits.btime  (Duration::from_millis)
winc/binc    → Limits.winc / Limits.binc
movestogo    → None (Lichess does not send MTG; soft = remaining/20 + inc/2)
side to move → from Board after applying `moves`
```

Call pattern:

1. Parse `state.moves` → rebuild `Board` (from `initialFen` in `gameFull`, or startpos).
2. If `is_my_turn`, build `Limits { wtime, btime, winc, binc, .. }`.
3. `TimeBudget::from_limits(&limits, stm, DEFAULT_MOVE_OVERHEAD)`.
4. Run `search::go` with stop flag; on completion `POST` the best move.

**Latency budget:** Lichess clock includes network RTT. Keep `DEFAULT_MOVE_OVERHEAD` (50ms) or add a lichess-specific network margin. lichess-bot subtracts engine overhead + lag estimate.

**Correspondence:** `daysPerTurn` instead of `clock`; separate low-priority polling (lichess-bot has a correspondence ping process). Defer until realtime works.

---

## 10. Restrictions & fair play

Hard API limits:

- Challenge games only (no pools, no tournaments via Bot API)
- No UltraBullet for bots
- Must not sandbag, boost, abort constantly, or manipulate ratings ([TOS](https://lichess.org/page/fair-play))

Practical dev norms:

- `rated=false` until move legality and clock usage are solid
- Don't accept every incoming challenge — filter variants/time controls
- Don't run 50 parallel games on a buggy engine (spam/aborts)
- Optional: auto-decline human challenges if we only want bot-vs-bot

---

## 11. OpenChess integration plan

### 11.0 CLI only — no TUI

Lichess bot mode is a **headless daemon**, not a terminal chess board.

- Entry: `openchess lichess run` and `openchess lichess challenge <user>` — structured logs to stdout/stderr.
- Watch games on lichess.org (Bot TV, live page) during development.
- **Do not** add a Lichess panel to `tui/` unless we later want an optional read-only mirror; v1 is CLI-only.

This matches [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) and keeps blocking HTTP streams out of the ratatui event loop.

### 11.1 Module layout (proposed)

Mirror `chesscom/` — feature-gated HTTP client + CLI, no search logic inside, **no TUI**.

```
src/
├── lichess/
│   ├── mod.rs          # errors, re-exports
│   ├── client.rs       # auth header, GET stream, POST helpers, 429 backoff
│   ├── events.rs       # StreamEvent, GameStart, Challenge, …
│   ├── game.rs         # GameHandler: game stream → board → search → move
│   ├── challenge.rs    # accept/decline/create + filters
│   ├── config.rs       # TOML: token path, speeds, rated, opponent filters
│   └── cli.rs          # `openchess lichess run|challenge <user>`
```

**Cargo.toml:**

```toml
[features]
default = ["chesscom"]
lichess = ["dep:ureq"]   # or reqwest + tokio if we need concurrent streams

[[bin]]  # unchanged entry
# main.rs: Some("lichess") => openchess::lichess::cli::run(args)
```

**Dependencies:** `ureq` + `serde_json` suffice for Phase 1 (blocking). Consider `litchee` if we want OAuth/PKCE or async later — don't block Phase 1 on it.

### 11.2 Engine coupling

**Do not** shell out to `openchess uci` for lichess games. Call the library directly (same as TUI session):

```text
gameState → parse moves → Board
         → Limits from wtime/btime/winc/binc
         → search::search(&board, &tt, &limits, &stop)
         → best Move → to UCI string → POST move
```

Reset TT between games; optionally smaller hash for bullet.

### 11.3 Config surface (minimal)

```toml
# lichess.toml (example)
token_env = "LICHESS_TOKEN"
accept_challenges = true
challenge_rated = false
speeds = ["rapid", "blitz"]
variants = ["standard"]
min_opponent_rating = 800
max_opponent_rating = 2000

# outbound matchmaking (optional)
auto_challenge = false
challenge_user = "bernstein-2ply"
clock_limit = 300
clock_increment = 1
```

### 11.4 Error handling & reconnects

| Failure | Behavior |
|---|---|
| Event stream drops | Exponential backoff reconnect; replay handles in-flight games |
| Move POST 400 | Log error + FEN; resign to avoid flagging |
| 429 | Sleep 60s; reduce request rate |
| Illegal engine move | Assert in dev; resign in prod |
| `opponentGone` | Wait for claim window; `claim-victory` if winning on time |

### 11.5 Testing without Lichess

- Unit-test JSON deserialization with fixtures from API docs.
- Mock NDJSON streams (stdin fixtures) for game loop integration tests.
- Manual smoke: casual game vs `@lichess` AI is **not** bot API — need real bot account on staging account.

---

## 12. Phased tasks

### Phase 1 (complete) — CLI / client

Historical checklist: [tasks-phase1.md § P9](./tasks-phase1.md#p9--lichess-bot-cli). Summary of what shipped:

| ID | Deliverable | Acceptance |
|---|---|---|
| **P9-01** | `lichess/client.rs` — auth GET, NDJSON line reader | Parse sample `gameStart` fixture |
| **P9-02** | Event loop skeleton | Connect stream; log challenges/games (dry-run) |
| **P9-03** | Single-game handler | Play one casual game to completion |
| **P9-04** | Challenge filter + accept | Only accept `standard` + configured speeds |
| **P9-05** | Outbound challenge | Challenge named bot; play game |
| **P9-06** | PGN export + game log | Save `GET /game/export/{id}` after `gameFinish` |
| **P9-07** | Reconnect + 429 backoff | Survive forced disconnect in manual test |

Live acceptance for P9-03/05/06/07 was offline-tested; **token smoke moves to Phase 2**.

### Phase 2 — Go-live

Canonical checklist: [tasks.md § L2](./tasks.md#l2--lichess-go-live). Summary:

| ID | Deliverable | Acceptance |
|---|---|---|
| **L2-01** | Operator docs + smoke checklist | Dry-run → play from docs alone |
| **L2-02** | Live casual game smoke | One full casual game; no illegal/time bugs |
| **L2-03** | Reconnect + PGN verify | Forced disconnect recovers; PGN matches site |
| **L2-04** | Ops config file + CLI overrides | File drives accept filter |
| **L2-05** | Bots-preferred / rated-off defaults | No surprise rated human spam |
| **L2-06** | Rated gate after strength bar | Documented SPRT/local bar before `accept_rated` |
| **L2-07** | Concurrent games | ≥2 games stable under rate limits |

---

## 13. API endpoint cheat sheet

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/stream/event` | Global events (auth) |
| `GET` | `/api/bot/online?nb=N` | List online bots |
| `POST` | `/api/bot/account/upgrade` | One-time bot upgrade |
| `GET` | `/api/bot/game/stream/{gameId}` | Game NDJSON stream |
| `POST` | `/api/bot/game/{gameId}/move/{uci}` | Play move |
| `POST` | `/api/bot/game/{gameId}/resign` | Resign |
| `POST` | `/api/bot/game/{gameId}/abort` | Abort |
| `GET` | `/api/challenge` | Pending challenges |
| `POST` | `/api/challenge/{username}` | Challenge user |
| `POST` | `/api/challenge/{id}/accept` | Accept |
| `POST` | `/api/challenge/{id}/decline` | Decline |
| `GET` | `/game/export/{gameId}` | PGN download |

---

## 14. Open questions

1. **Async runtime:** stick with blocking `ureq` + threads for multi-game, or adopt `tokio` early?
2. **Variants:** decline all non-standard until perft exists for Chess960, or scope bot to standard-only permanently?
3. **Rated policy:** at what internal SPRT confidence do we switch `rated=true`?
4. **Opening book:** lichess-bot supports local/online books; reuse later via move injection before search?
5. **Human challengers:** accept or bot-only filter (`opponent.title == "BOT"`)?
6. **litchee vs in-house client:** borrow types only, or depend on crate for maintenance?

---

## 15. References

- [Lichess Bot API docs](https://lichess.org/api#tag/bot/GET/api/stream/event)
- [Lichess Challenges API](https://lichess.org/api#tag/challenges)
- [OpenAPI spec (lichess-org/api)](https://github.com/lichess-org/api/blob/master/doc/specs/lichess-api.yaml)
- [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) — production bridge, multiprocessing model
- [litchee](https://github.com/obazin/litchee) — Rust Lichess client
- [Lichess bots announcement](https://lichess.org/blog/WvDNticAAMu_mHKP/welcome-lichess-bots)
- [Fair play / TOS](https://lichess.org/page/fair-play)
