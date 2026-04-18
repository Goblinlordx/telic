# telic — Empirical Findings

Results from testing telic across 5 game genres with TrueSkill-rated evaluation.

## TrueSkill Rankings by Game

### SimpleWars — Turn-Based Strategy

6 agents, 500 games on random 16x16 maps.

| # | Agent | μ | Approach |
|---|-------|---|---------|
| 1 | **coordinated** | 27.1 | Framework UtilityAction + Greedy strategy + ActionSource |
| 2 | utility | 26.0 | Inline utility scoring per unit |
| 3 | hand-coded | 25.9 | If/else priorities |
| 4 | hybrid | 25.6 | GOAP beliefs + utility weight modifiers |
| 5 | random | 25.2 | Random valid moves |
| 6 | htn | 24.6 | HTN task decomposition |

Key head-to-head: coordinated beats utility 67%, hand-coded 41%, htn 71%.

### Splendor — Engine-Building Card Game

5 agents, 1000 games with real card data.

| # | Agent | μ | Approach |
|---|-------|---|---------|
| 1 | **utility** | 29.2 | Framework UtilityAction + opponent awareness + denial |
| 2 | engine | 24.6 | Composable strategy voting (engine focus) |
| 3 | balanced | 24.0 | Composable strategy voting (balanced) |
| 4 | path | 23.8 | Path planning to 15 points |
| 5 | random | 16.4 | Random |

Utility dominates: 76% vs engine, 82% vs balanced, 81% vs path, 99% vs random.

### Love Letter — Hidden Information Card Game

4 agents, 5000 games.

| # | Agent | μ | Approach |
|---|-------|---|---------|
| 1 | **utility** | 26.0 | Framework UtilityAction + behavioral memory |
| 2 | probabilistic | 25.4 | Exact probability distributions |
| 3 | deduction | 25.3 | Card tracking + deduction |
| 4 | random | 22.5 | Random |

Tight margins between smart agents (~55% vs random each). High variance game (~3.5 turns). Behavioral memory (tracking what opponent played to infer hand strength) provides small but consistent edge.

### Poker — Texas Hold'em (Hidden Info + Betting)

4 agents, 1000 games, 200BB deep stacks, blinds every 20 hands.

| # | Agent | μ | Approach |
|---|-------|---|---------|
| 1 | **adaptive** | 26.4 | Utility + learned opponent model (showdown correlation) |
| 2 | utility | 24.9 | Utility + pot odds + position |
| 3 | tight | 23.9 | Hand-coded tight-aggressive |
| 4 | random | 21.0 | Random |

Adaptive beats tight 72% — learned that tight never bluffs (raise_honesty=high), so folds to their raises and steals their blinds. A 23-point improvement from opponent modeling alone.

### Arena Combat — Real-Time Squad Combat

6 agents, 500 games on random maps. 60fps tick-based simulation.

| # | Agent | μ | Approach |
|---|-------|---|---------|
| 1 | **kite** | 34.4 | Hand-coded archer retreat + warrior screen |
| 2 | utility | 32.1 | Framework UtilityAction + velocity intercept |
| 3 | rush | 30.9 | Charge nearest enemy |
| 4 | focus_fire | 21.5 | All attack lowest HP |
| 5 | random | 16.4 | Random |
| 6 | flanker | 16.1 | Split and attack from two sides |

Kite dominates through the warrior-screen + archer-kite tactic. Utility's intercept prediction partially counters it (32% vs kite) but can't fully overcome the range advantage.

## Key Findings

### 1. Utility Scoring Is Universally Effective

Framework-based utility agents are #1 or #2 in 4 of 5 games. The pattern works across strategy, card games, poker, and real-time combat.

### 2. Coordination Is the Differentiator for Multi-Unit Games

The `Greedy` assignment strategy with capture-spreading, focus fire, and escort coordination callbacks produces the strongest SimpleWars agent. The coordination layer adds ~20% win rate over per-unit-independent utility.

### 3. Opponent Modeling Matters

The adaptive poker agent's learned opponent model (correlating raises with showdown hand strength) produces a 23-point improvement over the utility agent that doesn't model opponents. Love Letter's behavioral memory similarly improves performance.

### 4. GOAP Works Best as a Weight Modifier

When GOAP constrains which actions are available (filtering), performance drops. When GOAP sets weight multipliers on all actions (tilting), performance improves. All tasks should always be available — strategic priority adjusts emphasis, not access.

### 5. HTN Is an Optimization, Not a Capability

HTN provides the same decomposition that GOAP discovers dynamically via backward-chaining. HTN's advantage is performance (no search); its disadvantage is rigidity (hand-authored structure). In our testing, HTN is the weakest smart agent in SimpleWars.

### 6. Plan Commitment Is Redundant

Distance-weighted utility scoring naturally produces commitment as an emergent property. An infantry sitting on a city (distance=1) always outscores distant alternatives. Explicit plan tracking adds complexity without benefit.

### 7. Complex Coordination Can Hurt

Pincer and herding strategies for anti-kiting performed worse than simple focused pursuit. Splitting forces reduces concentration. The right amount of complexity depends on the game — don't assume more = better.

### 8. Games Must Terminate Quickly

Poker's infinite loop (all-in with unequal bets, zero chips to equalize) taught us: always verify games terminate. Escalating pressure (blinds, score tiebreaks, time limits) prevents stalls.

### 9. Denial Should Serve Self-Interest

In Splendor, taking gems to deny the opponent only helps when those gems also advance your own goals. Pure denial with no self-benefit is a wasted turn. Denial has value but should be capped relative to self-progress.

## Framework Pieces by Proven Value

| Piece | Value | Evidence |
|-------|-------|---------|
| MultiPlayerArena + TrueSkill | **Essential** | Only way to rank agents meaningfully |
| UtilityAction\<S\> | **High** | #1 agent in 4/5 games |
| AssignmentStrategy (Greedy) | **High** | +20% in SimpleWars coordination |
| Opponent modeling (memory) | **High** | +23 points in poker |
| ResponseCurve::Identity | **Medium** | Required for ratio-based scoring |
| ActionSource trait | **Medium** | Cleaner task generation |
| score_with_trace() | **Medium** | Essential for debugging |
| BeliefSet\<S\> | **Medium** | Used by GOAP weight modifier |
| GoapPlanner | **Low-Medium** | Helps as weight selector only |
| Task::decompose (HTN) | **Low** | Working but weakest smart agent |
| Agent tick loop | **Unused** | Not used by any winning agent |

## Competitive Landscape

No existing Rust crate combines utility scoring + GOAP + coordination + TrueSkill evaluation. The framework is unique in:

- **Engine-agnostic** — not coupled to Bevy, Unity, or any renderer
- **Unified traits** — one GameState/GameAgent for turn-based and real-time
- **State-generic** — beliefs and scoring take `&S`, not global blackboards
- **Built-in evaluation** — TrueSkill-rated multi-agent tournaments with head-to-head matrices
- **Proven across 5 genres** — strategy, card games (2 types), poker, real-time combat
