# Unique Chess Engine Ideas Dump

> **Audience:** agents exploring how to *outsmart* mainstream engines, not how to clone them.
> **Companion:** [reckless.md](reckless.md) documents the solved Stockfish-family stack (bitboards + PVS + NNUE + Lazy SMP + SPRT). This doc is the deliberate counterweight.
> **Status:** brainstorm dump. Ideas are marked **[grounded]** (existing work) or **[speculative]** (underexplored / original angles). Not an implementation roadmap.

---

## 1. Framing

Competitive CPU chess is largely a solved *architecture* problem. Every top engine converges on the same recipe: deep selective alpha-beta, incremental neural eval, shared TT, empirical SPRT tuning. Elo gains from that stack are incremental — better pruning constants, wider nets, more Fishtest games.

To beat engines that all share that recipe, you need ideas that attack **shared failure modes**, not ideas that polish the same loop. Engines still systematically fail at:

- **Fortresses** — material advantage that cannot convert; eval stays “winning” forever
- **Horizon effects** — quiet plans and breakthrough sacrifices beyond the search window
- **Closed / static positions** — overconfidence when progress is structural, not tactical
- **OOD / adversarial positions** — endgames and puzzles crafted (or mined) to break eval + search
- **Symmetric optimality assumption** — search assumes the opponent is the same algorithm

The bet: an engine that is *weaker* on CCRL bullet but *stronger* at farming these blind spots can still win match play against Stockfish-family opponents — or at least open a research path that pure SPRT-on-NNUE will never find.

---

## 2. Known attack surfaces

| Weakness | What happens | Why it is shared |
|---|---|---|
| Fortress blindness | Eval claims +3; no progress across depths | Scalar eval + depth-limited search; no convertibility concept |
| Horizon / quiet plans | Breakthrough sac looks losing until depth 40+ | Aggressive pruning + qsearch bias toward forcing lines |
| Closed-position overconfidence | Engine “grinds” into a dead structure | Training / self-play under-samples long locked games |
| Adversarial endgames | Optimal tablebase move missed under node budget | Eval + search disagree with ground truth in sparse regions |
| Inter-engine transfer gaps | Position fools SF@N nodes but not SF@10N | Fingerprints of pruning / TT / eval, not chess truth |
| Anti-computer motifs | Humans (and specialized nets) steer into engine-blind zones | Engines optimize vs themselves, not vs their own failure distribution |

These are the seams. Every idea below should map to at least one of them.

---

## 3. Idea dump

### A. Search / planning (not alpha-beta)

#### A1. Latent-space directional planning **[grounded]**

**Premise:** Embed positions with supervised contrastive learning so distance ≈ evaluative similarity. Define an *advantage vector* (e.g. mean embedding of White mates − mean embedding of Black mates). Select moves that advance the position along that vector; optionally refine with shallow beam search.

**Why it might win:** Replaces million-node trees with geometric planning. Human-like selectivity; may handle long strategic trajectories that alpha-beta never expands.

**Risk:** Greedy beams cannot revise early mistakes; if supervision is Stockfish-derived, you inherit SF’s blind spots. Strength so far is strong but not top-engine (≈2593 Elo @ depth-6 beam in published work).

**Refs:** SOLIS / “Learning to Plan via Supervised Contrastive Learning and Strategic Interpolation” (Hamara et al., 2025).

---

#### A2. Implicit search via discrete diffusion **[grounded]**

**Premise:** Instead of building an explicit tree, train a discrete diffusion model to denoise future board states — “looking into the future world” inside the forward pass (DiffuSearch).

**Why it might win:** Amortizes multi-ply planning into one model call; published results beat one-step policies and MCTS-enhanced policies on action accuracy and puzzles (+~540 Elo in their setup).

**Risk:** Compute profile is GPU-heavy; unclear how it scales under classical time controls vs Stockfish on CPU; may still need a verification search for tactics.

**Refs:** DiffuSearch — “Implicit Search via Discrete Diffusion: A Study on Chess” (2025).

---

#### A3. Searchless / amortized intuition **[grounded]**

**Premise:** Train a large policy/value net (transformer / ViT) to pick moves with *zero* tree search — distill deep search into intuition (DeepMind searchless chess; open replications ~1900–2900 Elo depending on scale).

**Why it might win:** Different error distribution than alpha-beta. As a *component*, a searchless head can flag positions where deep search and intuition disagree (see E3).

**Risk:** Alone, still loses to deep search at equal hardware; tracking-vs-deciding bottlenecks in transformers; not a full replacement for match play yet.

**Refs:** “Grandmaster-Level Chess Without Search” (DeepMind); searchless-chess replications; “Tracking vs. Deciding” (2026).

---

#### A4. Progress-metric / fortress-aware search **[grounded + speculative]**

**Premise:** During iterative deepening, monitor whether backed-up scores *progress* toward a win. Flat scores across depths ⇒ fortress (or need a breakthrough). On detection: (a) scale eval toward draw, and/or (b) force analysis of all moves including material sacs that standard ordering rejects.

**Why it might win:** Directly attacks the #1 shared blind spot. Guid & Bratko showed fortress detection from stagnant ID scores; almost no top engine treats this as a first-class search mode.

**Risk:** False positives in slow-but-winning endgames; expensive “analyze everything” branch; tuning when to trigger is hard under SPRT.

**Refs:** Guid & Bratko, “Detecting Fortresses in Chess” (2012); Chessprogramming Wiki — Fortress.

---

#### A5. Dual-mode search: tactical AB + strategic planner **[speculative]**

**Premise:** Keep a Stockfish-family tactical core for open/forcing positions. When the position is closed/static (pawn locks, low mobility, low volatility), switch to a long-horizon planner (latent beam, diffusion, or progress-metric search) that is allowed to consider “ugly” breakthroughs.

**Why it might win:** Uses the solved stack where it is already near-optimal; only spends exotic compute where AB is known to be weak.

**Risk:** Mode-switch errors lose games instantly; two systems to train and validate; SPRT against pure SF may not reward rare fortress saves enough.

---

### B. Training that targets engines, not abstract Elo

#### B1. Anti-engine curriculum **[grounded]**

**Premise:** Train mostly against Stockfish under a controlled handicap (e.g. PID on WDL-regret / nodes), explicitly farming fortress, horizon, and closed-position mistakes — not pure self-play. Blend with some net-vs-net so you do not overfit to one opponent.

**Why it might win:** DeepFin’s design premise: ~70% games vs handicapped SF, soft/hard WDL disagreement as signal, multi-policy heads. Optimizes for *beating the dominant engine*, not for looking like Lc0.

**Risk:** Overfit to one SF version/settings; may play weird chess that loses to other architectures; strength claims need careful match protocols.

**Refs:** DeepFin TCEC questionnaire (2026-06-18) — anti-engine curriculum, blended WDL, multi-policy heads.

---

#### B2. Continuous adversarial position mining **[grounded + speculative]**

**Premise:** AdvChess / AS-LE finds legal endgame positions where engines deviate from tablebase-optimal play. Turn that into a *training loop*: mine adversarial positions → inject into replay → retrain → re-mine against the new net (and against frozen SF).

**Why it might win:** Systematically patches the sparse regions where eval+search lie; tablebases give ground truth in endgames.

**Risk:** Midgame has no oracle; mined positions may be unreachable in real games; transfer across engines/node budgets is imperfect.

**Refs:** AdvChess — “Discovery of Adversarial Endgame Chess Positions” (ICLR 2026 submission).

---

#### B3. Search-contempt / hard-position forcing **[grounded]**

**Premise:** During MCTS/self-play, bias toward positions the current value net misevaluates (“search-contempt”). Training distribution becomes adversarial to the learner’s own blind spots.

**Why it might win:** Robustness against the same class of attacks others will use on you; more efficient than uniform self-play for covering failure modes.

**Risk:** Can destabilize training; may over-emphasize pathological positions at the cost of ordinary strength.

**Refs:** “Search-contempt: a hybrid MCTS algorithm…” (2025).

---

#### B4. Soft-vs-hard WDL disagreement as the learning signal **[grounded]**

**Premise:** Train value on both game outcome (hard) and oracle/engine WDL (soft). Where they disagree (~majority of samples in DeepFin’s report), the model is forced to learn search-horizon bootstrap: “this looks equal to SF now but the game was won/lost.”

**Why it might win:** Directly teaches long-horizon credit that pure outcome RL learns slowly and pure SF-distillation never learns.

**Risk:** Noisy signal; if the oracle is wrong (fortresses), you bake the error in unless you also have fortress/adversarial mining.

---

### C. Asymmetric / opponent-aware play

#### C1. Opponent-model search against *engines* **[speculative]** (human OM is **[grounded]**)

**Premise:** Classical opponent-model search assumes a non-optimal opponent. Usually aimed at humans (Maia, Antimaia, Jansen). Flip it: learn a model of *Stockfish’s* move distribution under fixed nodes/hash — including systematic pruning mistakes — and search to maximize expected result against *that* policy, not against perfect play.

**Why it might win:** Match play is not vs God; it is vs a specific UCI binary. High-risk lines that SF’s LMR/NMP will discard become correct.

**Risk:** Brittle across SF versions, threads, hash, time control; may lose Elo vs unknown opponents; tournament rules / ethics of “anti-SF book” style prep.

**Refs:** Chessprogramming Wiki — Opponent Model Search; Luke Salamone — opponent modeling vs Maia; Maia / style-embedding literature (for the human case).

---

#### C2. Trap policy maximizing opponent blunder probability **[grounded]** (vs humans) / **[speculative]** (vs engines)

**Premise:** Policy that maximizes P(opponent blunder | style/skill model) × value-if-blunder, not minimax value. Plays “trappy” chess; wins faster against imperfect opponents.

**Why it might win:** Against humans, already shown to finish games much faster than SF. Against engines, combine with C1’s engine fingerprint.

**Risk:** Against near-perfect opponents, traps are just bluffs; negative EV if the model of the opponent is wrong.

---

#### C3. Engine-fingerprint exploitation **[grounded]**

**Premise:** Adversarial positions often transfer poorly across node budgets and engines. Build a library of positions where SF@low-nodes ≠ tablebase / ≠ SF@high-nodes, and steer games toward those structures (opening book + midgame attractors).

**Why it might win:** Turns known non-transferability into a match weapon under fixed time controls.

**Risk:** Strong opponents with more time/nodes escape the fingerprint; may not generalize beyond prepared lines.

---

### D. Evaluation outside scalar centipawns

#### D1. Multi-head / multi-objective eval **[grounded]**

**Premise:** Alongside WDL/value: volatility, moves-left, fortress-likelihood, convertibility, threat maps. Search uses these heads for extensions, pruning, and mode switches (A5), not just a single scalar.

**Why it might win:** Scalar cp cannot express “winning but unconvertible” or “equal but explosive.” Aux heads (KataGo-style, DeepFin) stabilize representation and enable smarter search control.

**Risk:** More loss terms to balance; heads can be ignored by search if not wired in carefully.

---

#### D2. Causal / counterfactual fortress test **[grounded]**

**Premise:** Fortresses can be invariant to adding certain attacking material — they defy naive probabilistic “more material ⇒ better.” Use counterfactual probes (add a piece, re-eval) as a causal test; if outcome unchanged, flag fortress and scale scores.

**Why it might win:** Directly encodes the logical structure of fortresses that NNUE regression misses.

**Risk:** Expensive probes; false fortress flags; needs a curated fortress benchmark (existing datasets help).

**Refs:** “Chess fortresses, a causal test for state of the art Symbolic [Neuro] architectures” (2021).

---

#### D3. Geometric eval in embedding space **[grounded]**

**Premise:** Do not regress to SF centipawns. Score by projection onto an advantage axis in a contrastive latent space (A1). Move value = Δprojection.

**Why it might win:** Eval and planning share one representation; may generalize differently than NNUE distilled from search.

**Risk:** Still often SF-supervised; calibration for mate scores / time management is non-trivial.

---

#### D4. Explicit convertibility / progress heads **[speculative]**

**Premise:** Train a head that predicts whether a “winning” eval will still be winning in N plies *without* tactical forcing — i.e. predicted progress. Penalize stagnant winning scores in training and in search (contempt toward fortress-shaped wins).

**Why it might win:** Makes A4’s progress metric learnable instead of hand-ruled.

**Risk:** Labeling requires deep search or long rollouts; circular dependency with the thing you are trying to fix.

---

### E. Speculative / underexplored originals

#### E1. Meta-search over search policies **[speculative]**

**Premise:** Learn a controller that, per position (or per game phase), chooses pruning aggression, LMR schedule, qsearch depth, or even algorithm class (AB vs MCTS vs latent beam). Search hyperparameters become a policy, not constants.

**Why it might win:** One fixed selective-search recipe is a compromise; closed positions want different selectivity than tactical melees.

**Risk:** Huge hyperparameter surface; hard to SPRT; instability.

---

#### E2. Program-synthesis plans + shallow verify **[speculative]**

**Premise:** A model emits a short symbolic plan (“rook lift → g-pawn break → sac on h7”) as a structured object. Shallow search only verifies and refines the plan instead of rediscovering it from move ordering.

**Why it might win:** Compresses long quiet plans that AB never orders highly enough to see.

**Risk:** Plan language design; invalid plans; verification cost; almost no prior art in competitive engines.

---

#### E3. Ensemble disagreement as a node budget signal **[speculative]**

**Premise:** Run cheap SF-like eval + NN/MCTS head + searchless head in parallel. Where they *agree*, move fast. Where they *disagree*, dump the remaining node budget into deep search / adversarial probes.

**Why it might win:** Disagreement is a proxy for “someone is wrong” — often the shared blind spot. Allocates compute to the seams in §2.

**Risk:** Three systems’ latency; correlated failures (all trained on similar data) look like agreement.

---

#### E4. Temporal credit for quiet moves **[speculative]**

**Premise:** Mine move pairs where depth-20 eval ≈ equal but depth-40+ (or long rollout / tablebase) shows a win. Train policy/value specifically on those quiet improving moves; upweight them in ordering.

**Why it might win:** Directly attacks horizon bias in both training data and move ordering.

**Risk:** Expensive label generation; rare examples; may overfit to specific endgame patterns.

---

#### E5. Closed-position specialist subnet **[speculative]**

**Premise:** Detect locked pawn structures / low-open-file positions and route to a specialist net + search config trained only on closed games (including anti-engine closed curricula).

**Why it might win:** Closed positions are exactly where general engines overclaim and under-plan; specialization is cheap if routing is accurate.

**Risk:** Routing errors; specialist may be weak when the position opens after a break.

---

#### E6. Tablebase-shaped midgame (backward distillation) **[speculative]**

**Premise:** From 6–7 man tablebases, walk *backward* through reverse-move generation into 8–12 man and midgame-like positions, labeling convertibility / WDL. Distill that truth into eval long before Syzygy probes apply.

**Why it might win:** Injects endgame *truth* into regions where NNUE is only imitating search.

**Risk:** Reverse-move explosion; illegal/unreachable positions; distribution shift from real games.

---

#### E7. Adversarial self-play with a “liar” eval **[speculative]**

**Premise:** Freeze a SF-like eval (or a distilled NNUE). Train an adversary whose reward is to reach positions where liar-eval is wrong vs ground truth (tablebase, deep search, or game outcome). Train the main engine on those positions (and optionally to beat the adversary).

**Why it might win:** Explicitly manufactures the failure distribution of the dominant paradigm; related to B2/B3 but with a dedicated liar agent.

**Risk:** Adversary finds unreachable curiosities; main engine becomes a specialist anti-liar, not a general player.

---

#### E8. Information-theoretic move selection (search entropy / TT pollution) **[speculative]**

**Premise:** Under fixed time, prefer moves that maximize the opponent’s remaining uncertainty — e.g. maximize entropy of their root policy, force TT misses, or push into positions where their selective search is known to prune incorrectly (ties to C1/C3).

**Why it might win:** Classical engines assume symmetric compute; asymmetric “make their search worse” is almost unused in modern NNUE engines.

**Risk:** Hard to estimate; may choose objectively weak moves; ethically/competitively spicy in engine matches.

---

## 4. What not to chase (for this goal)

Out of scope if the goal is *outsmarting classical engines in standard chess*:

- **Quantum chess variants** — different game; does not transfer to FIDE chess strength
- **Pure LLM chat play** — fun, weak, wrong compute profile for match play
- **“Just bigger NNUE / more Fishtest”** — stays inside the solved stack; see [reckless.md](reckless.md)
- **Copying Lc0/AlphaZero wholesale** — strong, but same self-play orthodoxy DeepFin deliberately left

---

## 5. Suggested reading order for agents

1. Internal: [reckless.md](reckless.md) — know what you are *not* reinventing  
2. Attack surfaces: Guid & Bratko fortresses; AdvChess adversarial endgames  
3. Anti-engine training: DeepFin TCEC questionnaire  
4. Alternative planning: SOLIS (latent), DiffuSearch (diffusion), DeepMind searchless  
5. Opponent modeling: CPW Opponent Model Search; Maia / Antimaia for the human case  

---

## 6. Pointers

| Topic | Link |
|---|---|
| Reckless / SF-family reference (this repo) | [reckless.md](reckless.md) |
| SOLIS latent planning | https://arxiv.org/html/2506.04892 |
| DiffuSearch | https://arxiv.org/pdf/2502.19805 |
| DeepMind searchless chess | https://arxiv.org/abs/2402.04494 |
| Searchless chess (open replication) | https://github.com/mateuszgrzyb-pl/searchless-chess |
| Tracking vs. Deciding (searchless transformers) | https://arxiv.org/html/2603.29761 |
| AdvChess adversarial endgames | https://openreview.net/forum?id=nIYtXmeY3F |
| Search-contempt MCTS | https://arxiv.org/html/2504.07757 |
| DeepFin anti-engine curriculum | https://wiki.chessdom.org/Deepfin_questionnaire_20260618 |
| Fortress detection (Guid & Bratko) | https://ev.fe.uni-lj.si/1-2-2012/Guid.pdf |
| Fortress (CPW) | https://www.chessprogramming.org/Fortress |
| Opponent Model Search (CPW) | https://www.chessprogramming.org/Opponent_Model_Search |
| Opponent modeling vs Maia | https://blog.lukesalamone.com/posts/winning-faster-than-stockfish/ |
| AlphaZero puzzle / blind-spot analysis | https://arxiv.org/abs/2308.09175 |

---

## 7. One-line shortlist (if you only try three)

1. **Anti-engine curriculum + soft/hard WDL** (B1/B4) — train to beat SF’s actual mistakes.  
2. **Progress / fortress-aware search** (A4/D4) — stop claiming wins you cannot convert.  
3. **Ensemble disagreement budgeting** (E3) — spend nodes where paradigms conflict.
