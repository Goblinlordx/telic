# telic

*(adjective)* **directed toward a definite end; purposive.**

`telic` is an engine-agnostic game AI framework for Rust. It gives you:

- a clean **interface contract** between game and agent — a command-tree
  API that makes *invalid commands unrepresentable*;
- an **AI toolkit** with utility scoring, coordinated assignment, and
  GOAP/HTN planning primitives (optional — agents can use any, all, or
  none);
- an **evaluation arena** with TrueSkill ratings and pairwise head-to-head
  tournaments for comparing AI strategies objectively.

Built across 5 example games — strategy, card games, poker, real-time
squad combat — and tuned against 30,000+ tournament games.

## Install

```toml
[dependencies]
telic = "0.1"
```

## Quick start

A telic game implements three traits:

```rust
use telic::arena::{GameState, GameView, CommandProvider, CommandTree, PlayerIndex};

impl GameState for MyGame { /* apply_command, view_for, is_terminal, ... */ }
impl CommandProvider for MyGameCommands { /* command_tree for each player */ }
```

An agent picks from the tree of valid commands:

```rust
use telic::arena::{GameAgent, CommandTree};

impl GameAgent<MyView, MyCommand> for MyAgent {
    fn decide(&mut self, view: &MyView, tree: &CommandTree<MyCommand>) -> Option<MyCommand> {
        tree.argmax(|cmd| self.score(cmd, view))
    }
    // ... lifecycle hooks ...
}
```

Evaluate against other agents:

```rust
use telic::arena::{MultiPlayerArena, ClosureFactory};

let report = MultiPlayerArena::new(2)
    .with_games(1000)
    .add_agent_type(ClosureFactory::new("mine", || Box::new(MyAgent::new())))
    .add_agent_type(ClosureFactory::new("baseline", || Box::new(RandomAgent::new())))
    .run::<MyGame, MyGameCommands>(|_| MyGame::new());
report.print_summary();
```

## Why a command tree?

Most game-AI patterns have the agent *propose* a command, and the game
*reject* it if invalid. In practice, scoring-based agents misfire — they
pick a unit that already moved, aim at an enemy out of range, try to
spend gold they don't have. The rejected command then either breaks the
game loop or wastes retries.

`telic` flips this. The `CommandProvider` enumerates every valid command
as a tree:

```
Layer("actions")
├── "end_turn" → Leaf(EndTurn)
├── "attack"   → Layer
│     ├── "unit_1" → Leaf(Attack { unit_id: 1, target: (3,5) })
│     └── "unit_2" → Leaf(Attack { ... })
├── "capture"  → Layer(...)
└── "move"     → Layer(...)
```

The agent picks from leaves. By construction it *cannot* return a
command the game would reject.

The tree supports:

- **Structural sharing** via `Arc<CommandTree<C>>` — reuse unchanged
  subtrees across ticks.
- **Laziness** via `LazyLayer` — branches enumerate on first access;
  agents that never descend into a branch never pay the cost.
- **Continuous parameters** via `Parametric` leaves with
  `ParamDomain::Continuous { min, max }` — for aim angles, rotation
  velocities, move vectors, and other non-enumerable inputs.

See [docs/command_tree.md](docs/command_tree.md) for the full API and
agent patterns (random, utility, hierarchical, FPS aim).

## Toolkit highlights

- `UtilityAction<S>` — compose multi-factor scorers with response curves
  (Linear, Inverse, Threshold, Custom) over arbitrary state `S`.
- `AssignmentStrategy` — multi-entity task assignment with four
  built-in strategies: `Greedy` (with coordination callback), `Hungarian`
  (Kuhn-Munkres optimal), `RoundRobin`, `WeightedRandom` (softmax-sampled).
- `BeliefSet<S>` — named boolean/numeric queries over state, for GOAP
  preconditions or utility considerations.
- `GoapPlanner` — backward-chaining search (A*, DFS, Bidirectional).
- `Task<S>` — HTN hierarchical decomposition.

## Examples

| Game | Genre | Scale |
|---|---|---|
| [`simple_wars`](crates/examples/simple_wars) | Turn-based strategy (Advance Wars micro) | 16×16 grid, fog of war |
| [`splendor`](crates/examples/splendor) | Engine-building card game | Real cards, 2P |
| [`love_letter`](crates/examples/love_letter) | Hidden-info card game | 8-card deck, deduction |
| [`poker`](crates/examples/poker) | Texas Hold'em | Deep-stack heads-up, escalating blinds |
| [`arena_combat`](crates/examples/arena_combat) | Real-time squad combat | 60 fps tick-based |

Run any tournament with `cargo run --release -p <example>-example`.

## Empirical findings

Summary of what works across ~6000 tournament games per genre:

- **Utility scoring** is universally effective — the top-1 or top-2 AI
  in 4 of 5 games.
- **Coordinated assignment** (`Greedy` with coordination callback) adds
  ~20% win rate for multi-unit strategy games.
- **Opponent modeling** (learning raise-honesty from showdowns) adds a
  23-point TrueSkill bump in poker.
- **GOAP** works best as a *weight modifier* on utility scoring, not as
  an action filter.
- **HTN** is a performance optimization (skip search) not a capability
  difference.

See [docs/findings.md](docs/findings.md) for the full data.

## Documentation

- [Architecture](docs/architecture.md) — trait design, module layout.
- [Usage Guide](docs/guide.md) — step-by-step tutorial.
- [Command Trees](docs/command_tree.md) — the valid-action API in depth.
- [Findings](docs/findings.md) — tournament results and lessons.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
