# telic — Architecture

**telic** (adjective: *directed toward a definite end; purposive*) is an
engine-agnostic game AI framework for Rust. The name reflects the core
design principle: agents are goal-directed decision-makers, and the
framework's job is to give them a clean surface to choose from and a way
to measure how well they chose.

## Overview

The framework provides three layers:

1. **Interface contract** — clean isolation between game and agent, with
   a command-tree API that makes invalid commands unrepresentable.
2. **AI toolkit** — utility scoring, coordinated assignment, GOAP/HTN
   planning primitives. Optional tools — agents can use any, all, or none.
3. **Evaluation** — TrueSkill-rated multi-agent tournaments with
   pairwise head-to-head matrices.

The framework does not prescribe a specific AI approach. Agents can use
utility scoring, GOAP, HTN, neural networks, or hardcoded logic. The
framework provides the boundary (`GameState` + `CommandProvider` +
`GameAgent`) and optional tools.

## Core Traits

### GameState

One trait for all games — turn-based and real-time:

```rust
pub trait GameState: Debug {
    type Command: GameCommand;
    type View: GameView;

    fn view_for(&self, player: PlayerIndex) -> Self::View;
    fn apply_command(&mut self, player: PlayerIndex, command: Self::Command) -> Result<(), String>;
    fn is_terminal(&self) -> bool;
    fn outcome(&self) -> Option<GameOutcome>;
    fn turn_number(&self) -> u32;
    fn num_players(&self) -> usize;
}
```

**Turn-based games**: `apply_command` rejects if it's not that player's turn. The view indicates whose turn it is.

**Real-time games**: `apply_command` accepts from all players each tick. The game collects commands and advances the simulation when all players have submitted.

The framework has no `current_player()` — turn tracking is the game's responsibility. The arena asks every player every tick; the game decides what to accept.

### GameAgent

One trait for all agents. Each tick, the agent receives a tree of valid commands and picks one:

```rust
pub trait GameAgent<V: GameView, C: GameCommand>: Debug {
    fn name(&self) -> &str;
    fn reset(&mut self, player: PlayerIndex);
    fn observe(&mut self, view: &V);
    fn decide(&mut self, view: &V, tree: &CommandTree<C>) -> Option<C>;
    fn on_command_rejected(&mut self, _reason: &str) {}
    fn on_game_over(&mut self, _outcome: &GameOutcome) {}
}
```

Returning `None` is a no-op for that tick (used when the tree is `Empty` — e.g. it's not the player's turn in a turn-based game). An agent that picks only from leaves the tree exposes will never produce a rejected command.

The agent owns its memory as struct fields. The framework provides lifecycle hooks (observe, decide, reset) — the agent decides what to track internally. Serialization is the agent's business, not the framework's.

See [Command Trees](command_tree.md) for the tree API and agent patterns.

### CommandProvider

Every game must provide a `CommandProvider` — a sibling trait to `GameState` that enumerates the valid commands available to each player:

- agents that are architecturally incapable of proposing invalid commands
- UI code that knows exactly which actions to enable/disable/highlight
- structured logging and replay of decision points
- support for continuous parameter spaces (aim angles, velocities, etc.)

```rust
pub trait CommandProvider {
    type State: GameState;
    fn command_tree(
        state: &Self::State,
        player: PlayerIndex,
    ) -> Arc<CommandTree<<Self::State as GameState>::Command>>;
}
```

Implementers can provide this directly on their game type or via a wrapper struct, so `GameState` itself stays free of the dependency. The arena consumes it through `MultiPlayerArena::run::<G, P>(...)`.

### GameView

What a player can see. Game-specific, respects hidden information:

```rust
pub trait GameView: Debug + Clone {
    fn viewer(&self) -> PlayerIndex;
    fn turn(&self) -> u32;
}
```

## Evaluation: MultiPlayerArena

Uses TrueSkill (Bayesian rating via `skillratings` crate, MIT/Apache-2.0) for N-player evaluation:

```rust
let report = MultiPlayerArena::new(2)  // N players per game
    .with_games(1000)
    .add_agent_type(ClosureFactory::new("name", || Box::new(MyAgent::new())))
    .run(|num_players| MyGame::new());
report.print_summary();  // TrueSkill ratings + head-to-head matrix
```

Features:
- Registers agent types as factories (creates fresh instances per game)
- Random composition sampling (which types fill which seats)
- TrueSkill μ/σ ratings per agent type
- Pairwise head-to-head win rate matrix
- Works for 2+ players, turn-based and real-time

## AI Toolkit

All toolkit components are optional. Agents can use any, all, or none.

### Utility Scoring — `UtilityAction<S>`

Generic over state type `S`. Considerations evaluate directly from `&S` through response curves:

```rust
let scorer = UtilityAction::new("attack")
    .with_mode(ScoringMode::Additive)
    .consider("proximity", |ctx: &Ctx| 1.0 / ctx.distance,
        ResponseCurve::Identity, 1.0)
    .consider("threat", |ctx: &Ctx| ctx.enemy_damage * 0.3,
        ResponseCurve::Identity, 1.0);

let score = scorer.score(&ctx);
```

**Response curves**: Linear, Inverse, Threshold, Boolean, Constant, Identity, Custom.
**Scoring modes**: Multiplicative (veto-style) or Additive (bonus-style).
**Explainability**: `score_with_trace()` returns per-consideration breakdown.

### Coordinated Assignment — `AssignmentStrategy`

Trait for multi-entity task assignment given an `[entity][task]` score matrix.
The framework ships four concrete strategies; add more by implementing the trait:

- **`Greedy`** — globally pick the highest-scoring pair, assign, optionally
  adjust remaining scores, repeat. Each entity assigned at most once. Strongest
  in empirical testing when paired with coordination callbacks.
- **`RoundRobin`** — entities pick their own best task in priority (or index)
  order. Task reuse allowed unless suppressed via coordination callback.
  Useful when initiative / turn order matters.
- **`Hungarian`** — optimal one-to-one assignment (Kuhn-Munkres, O(n³)). Finds
  the global maximum total score. No coordination callback (single-pass).
  Use as theoretical upper bound when evaluating Greedy.
- **`WeightedRandom`** — softmax-sample each entity's task with a seeded RNG.
  Temperature controls randomness. One-to-one by construction. Useful for
  exploration, diversity, or opponent-proof unpredictability.

```rust
// Greedy with coordination
let assignments = Greedy::with_coordination(|entity, task, scores| {
    // diminish same-target captures, boost focus fire, etc.
}).assign(&mut scores);

// Round-robin by priority
let assignments = RoundRobin::with_order(vec![2, 0, 1]).assign(&mut scores);

// Hungarian optimal
let assignments = Hungarian::new().assign(&mut scores);

// Seeded stochastic
let assignments = WeightedRandom::with_temperature(seed, 0.5).assign(&mut scores);
```

`f64::NEG_INFINITY` entries are treated as forbidden pairs by every strategy.

### Smart Objects — `ActionSource<A, V>`

World objects advertise available actions:

```rust
impl ActionSource<Task, View> for Building {
    fn available_actions(&self, view: &View) -> Vec<Task> { ... }
}
```

### Beliefs — `BeliefSet<S>`

Named queries against state `&S`. Boolean beliefs for GOAP preconditions, numeric for utility:

```rust
beliefs.add(BeliefBuilder::new("has_army")
    .condition(|state: &View| state.units.len() >= 5)
    .build());
```

### GOAP Planner

Backward-chaining search (A*, DFS, Greedy, Bidirectional). Useful for determining strategic priorities as weight modifiers on utility scoring.

### HTN Tasks — `Task<S>`

Hierarchical task decomposition. Compound tasks decompose via methods with conditions. An optimization over GOAP (skip the search) when task structure is known.

## State-Generic Design

All types are generic over `&S` (immutable). Beliefs, utility actions, and planners take `&S` to query world state. This matches traditional GOAP (ambient state access) with explicit data flow, and enables future concurrent evaluation.

## Agent Memory

Agents track history via `observe()` (called after every state change). Memory is just struct fields on the agent — the framework doesn't manage it. The agent reads and writes its own memory in `observe()` and `decide()`. `reset()` clears it for new games.

For persistence, the game implementor decides the strategy: serialize the agent struct directly, or replay game events through `observe()` to reconstruct.
