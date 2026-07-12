# Opening play & book strategy

Research notes on why OpenChess plays odd first moves today, and how to improve opening quality — especially under **short movetime** or **shallow effective depth**.

**Related:** [tasks.md](./tasks.md) (P9 Lichess bot) · [chesswiki §5](./chesswiki.md#5-knowledge-outside-the-search) · [LICHESS §14](./LICHESS.md#14-open-questions) · `testing/books/openings.epd`

---

## 0. Problem statement

Human players expect sensible opening moves (`e4`, `d4`, `Nf3`, …). OpenChess sometimes plays **corner pawn pushes** (`a3`, `h3`, `a6`, `h6`) or other non-theory moves in the first few plies.

This is not one bug — it is a stack of interacting gaps:

| Symptom | Typical cause | Fix class |
| --- | --- | --- |
| `a3` / `a6` on move 1 | Search aborts before depth 1 completes; fallback is first legal move in generation order | Time budget + fallback |
| `Nf3`, `e3`, `c3` at normal time | Bootstrap NNUE has no opening knowledge; shallow search trusts bad eval | Book or trained net |
| Black weaker than White in BvB | Per-side config asymmetry (`bot.black` lower depth/ms) | Config / book |
| SPRT games look fine | `openings.epd` seeds cutechess; TUI bot does not use it | Wire book into play path |

**Key insight:** Opening theory is largely **solved outside search**. Strong engines delegate the first N plies to a book (Polyglot, internal tree, or lichess-bot-style online lookup) and only search once the position leaves book. Expecting alpha-beta + a placeholder NNUE to discover 1. e4 at 50 ms is unrealistic.

---

## 1. Current engine behavior (baseline)

### 1.1 No live opening book

- `testing/books/openings.epd` exists for **OpenBench / cutechess SPRT** only (`testing/README.md`).
- No `src/` module references it. TUI bot and UCI search go straight to `search::go`.
- Lichess integration (P9) lists opening book as an open question — lichess-bot injects book moves before search.

### 1.2 Bootstrap NNUE is not opening-aware

Production eval is a **material-distilled HalfKA bootstrap** (`src/eval/nnue/network.rs`) until Bullet-trained nets ship. It scores material and crude PST-like features; it does not encode decades of human opening theory.

At moderate depth, search can genuinely prefer legal but odd moves (`e2e3`, `c2c3`, lines containing `h2h3`). That is eval weakness, not move-order corruption.

### 1.3 Time-abort fallback → corner pawns

When search stops before finishing even **depth 1**, the best move defaults to `root_moves[0]`:

```221:226:src/search/mod.rs
    td.root_moves = legal.into_iter().map(RootMove::new).collect();
    ...
        best_move: td.root_moves[0].mv,
```

`root_moves` follows **move generation order**. Pawn quiets iterate **a-file → h-file** (`src/board/movegen.rs`), so the first quiet is always **a-pawn one step** (`a2a3` / `a7a6`).

**Trigger chain:**

1. TUI passes **both** depth and movetime (`config.play_go_limits()` → `limits_to_search`).
2. Hard time limit = `movetime − move_overhead` (default overhead 50 ms, `src/time.rs`).
3. At root, search checks hard limit **between moves** (`src/search/alphabeta.rs`).
4. If `movetime_ms ≤ 50`, hard budget is **0 ms** → instant abort → `a2a3`.

Observed (release build):

| `go movetime` | Startpos bestmove |
| --- | --- |
| 50 ms | `a2a3` (no `info` lines) |
| 100 ms | `g1f3` |
| 450 ms | `c2c3` (depth ~12, then soft stop) |

### 1.4 Config defaults (TUI bot)

| Setting | Default | Notes |
| --- | --- | --- |
| `bot.depth` | 8 | Paired with movetime, not depth-only |
| `bot.movetime_ms` | 450 | Min clamp 50 ms |
| BvB per-side | same as bot | User can set asymmetric Black limits |
| Eval bar | depth 6 / 250 ms | Separate from bot search |

Depth alone does not produce corner pawns unless combined with time abort. Default 450 ms is enough to finish depth 1 in release, but still yields non-theory moves via bootstrap eval.

### 1.5 Move ordering is not the root cause

Root move **selection** uses search scores when ID completes. MovePicker (TT → captures → history quiets) only affects which move wins when search **fails to finish** or for node ordering inside the tree. Tests confirm sensible ordering (e.g. `e2e4` ahead of `d2d4` when history favors it).

---

## 2. Why openings fail under quick cutoffs

### 2.1 Search depth vs opening horizon

Opening quality needs either:

- Enough depth to see **central control, development, king safety** pay off (often 8–12+ plies for first-move distinctions), or
- External knowledge (book) so search never has to "discover" theory.

At **depth 1–4**, eval sees mostly static features. A flank pawn push can look equal to `e4` to a weak net. Corner moves from abort are worse: **zero completed iteration**.

### 2.2 Dual limits (depth + movetime)

TUI always sends both when configured. Movetime can cap effective depth before the depth target — especially in **debug builds** (10×+ slower → depth 3–4 at 450 ms).

**Recommendation for play:** prefer **depth-only** when user sets depth and wants quality; or enforce a **minimum opening movetime** floor.

### 2.3 Asymmetric BvB strength

`play_go_limits(mode, side_to_move)` uses `bot.white` / `bot.black` in Bot vs Bot. Low Black movetime + low depth compounds opening weirdness on every Black move, not just move 1.

---

## 3. Design options (ranked)

### Option A — Internal opening book (recommended medium-term)

**What:** Before `search::go`, probe a book for a move; if hit, return immediately.

**Formats:**

| Format | Pros | Cons |
| --- | --- | --- |
| **Polyglot (.bin)** | Industry standard, huge public books | Binary parser, Zobrist key must match |
| **EPD + move list** | Human-readable, already have `openings.epd` | Need per-position move weights / continuations |
| **Embedded Rust tree** | Zero I/O, deterministic, tiny for first 10 plies | Manual maintenance, no community books |

**Integration point:**

```
TUI maybe_start_engine / UCI go handler / future lichess move loop
    → book.probe(position_key, ply)
    → if Some(mv): return mv
    → else: search::go(...)
```

**lichess-bot pattern:** local book file + optional online Lichess opening explorer API. See [LICHESS.md §14](./LICHESS.md#14-open-questions).

**Acceptance criteria:**

- Move 1 from startpos is in `{e4, d4, Nf3, c4, …}` with sensible weights, never `a3` from book miss + abort.
- Book depth configurable (UCI `BookDepth` or config `book.plies`).
- `OwnBook` UCI option (default on for bot/TUI, off for SPRT strength testing).

### Option B — Tiny hardcoded first-move table (recommended short-term)

**What:** Hash map or match on `(key, ply)` for first **8–12 plies** only — a few hundred lines, no Polyglot yet.

**Example first-move whitelist (White):**

| Move | Weight | Rationale |
| --- | --- | --- |
| `e2e4` | 40 | Most common human move |
| `d2d4` | 35 | Queen's pawn main line |
| `g1f3` | 15 | Reti / transpositional |
| `c2c4` | 10 | English |

Black responses keyed on White's first move (e.g. after `e4`: `e7e5`, `c7c5`, `e7e6`, `d7d5`, …).

**Selection:** weighted random (variety for bot) or max-weight (deterministic). lichess-bot often uses weighted choice.

**Pros:** Ships in days; fixes TUI and future Lichess bot immediately.  
**Cons:** Does not scale; transpositions and deep theory need real book.

### Option C — Fix abort fallback (required regardless)

Even with a book, search must not play `a2a3` when time runs out:

1. **Never use `root_moves[0]` as final bestmove** unless at least one full depth iteration completed.
2. On abort with no completed depth: pick **TT best move**, or **highest history / PV move**, or **central pawn / development heuristic** — not generation order.
3. **Enforce `movetime_ms > move_overhead_ms + margin`** (e.g. min 200 ms) in config clamp.
4. Optionally: **opening phase minimum depth** — ignore movetime until `depth >= 4` or `ply <= 10`.

### Option D — Opening-aware eval (long-term)

Train or distill NNUE on opening-heavy data so leaf eval prefers sensible development. Does **not** replace a book for move 1 at bullet time, but reduces odd moves after book exits.

Fishtest uses fixed opening suites for a reason: even SF benefits from books at very short TC.

### Option E — Search-only mitigations (partial)

| Idea | Effect |
| --- | --- |
| Raise default bot movetime (1000+ ms) | Better but burns clock; does not fix 50 ms edge case |
| Depth-only mode in TUI | Removes movetime cap; good for analysis |
| Widen aspiration / more root search | Marginal for move 1 |
| `limit_strength` / Elo (currently unwired) | Future: restrict to plausible move subset |

---

## 4. Proposed implementation phases

### Phase 1 — Stop the bleeding (P9-adjacent)

- [ ] Fix search fallback when zero depths complete (Option C).
- [ ] Config clamp: `movetime_ms >= 200` (or `move_overhead + 100`).
- [ ] Hardcoded first-move table for ply ≤ 2 (Option B).
- [ ] Hook in `tui/session.rs` before `spawn_search` (and UCI `go` for consistency).

### Phase 2 — Real book module

- [ ] `src/book/mod.rs`: probe API `fn best_move(&self, pos: &Position, rng: &mut impl Rng) -> Option<Move>`.
- [ ] Load Polyglot **or** expand `openings.epd` into a keyed move list with weights.
- [ ] Config: `book.enabled`, `book.plies`, `book.path`.
- [ ] UCI: `setoption name OwnBook`, `setoption name BookFile`.

### Phase 3 — Lichess & testing

- [ ] Lichess bot: book before search; map clock to movetime per [LICHESS.md](./LICHESS.md).
- [ ] Keep `openings.epd` for SPRT; optionally align book positions with EPD set for consistent testing.
- [ ] SPRT with `OwnBook off` measures search+eval; bot play uses `OwnBook on`.

---

## 5. Book content guidelines

### 5.1 What to include early

- **Main lines only** for first 6–10 plies: open games, Sicilian, French, Caro-Kann, QGD, KID, Nimzo, English.
- Avoid dubious traps unless weighted low.
- Mirror `openings.epd` IDs where possible for test continuity:

```
startpos, e2e4, d2d4, e4e5, open_italian_path, sicilian, qgd_path, …
```

### 5.2 Weighting

- Human frequency (Lichess opening explorer) or engine consensus (SF at high depth).
- For a **personality bot**: slight randomization among top 3 moves by weight.
- For **strength testing**: fixed book lines or round-robin EPD suite (already in `testing/`).

### 5.3 When to leave book

Common rules:

- **Max ply** (e.g. 12).
- **Min remaining book depth** in line (Polyglot `wikipedia` flag).
- **Score band:** leave if book move is > 30 cp worse than search suggestion (hybrid mode — advanced).

---

## 6. Quick cutoff cheat sheet

| Scenario | Expected without book | With Phase 1+2 |
| --- | --- | --- |
| movetime 50 ms | `a2a3` / `a7a6` | Book or heuristic fallback |
| movetime 450 ms, release | Odd but legal (`c3`, `Nf3`) | `e4` / `d4` / main response |
| movetime 450 ms, debug | Very shallow; weak moves | Book masks slowness |
| BvB Black 500 ms / depth 10 | Weaker Black openings | Symmetric book quality |
| UCI no limits | depth 6 default | Book for ply ≤ N, then search |

---

## 7. Open questions

1. **Polyglot vs internal EPD book first?** Polyglot gives instant access to community books; EPD matches existing test infra.
2. **Hybrid book + search:** always play book move, or allow search override when eval disagrees strongly?
3. **Chess960:** separate book or standard-only bot (see [LICHESS.md §14](./LICHESS.md#14-open-questions)).
4. **Variety vs strength:** weighted random helps bot feel human; bad for deterministic regression tests.
5. **Online book (Lichess explorer):** rate limits, latency, offline fallback — lichess-bot solves this; copy patterns.

---

## 8. References

- [Chess Programming Wiki — Opening Book](https://www.chessprogramming.org/Opening_Book)
- [Polyglot book format](https://www.chessprogramming.org/Polyglot)
- [lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) — book injection before engine search
- [Lichess opening explorer API](https://lichess.org/api#tag/opening-explorer)
- OpenChess: `testing/books/openings.epd`, `src/search/mod.rs`, `src/tui/session.rs`, `src/config.rs`

---

*Last updated: 2026-07-12 — documents observed TUI bot behavior and abort fallback; implementation tracked via tasks P9 / future book task.*
