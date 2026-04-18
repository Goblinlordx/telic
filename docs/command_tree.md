# Command Trees

A command tree is the set of commands a player can issue from a given game
state, shaped as a tree so agents can reason about them hierarchically.
Implementing `CommandProvider` for your game is required, and gives agents
a correctness guarantee: **an agent that picks from the tree cannot propose
a command the game would reject.**

## Why

1. **Correctness.** A "propose and hope" API where the agent returns any
   command and the game rejects invalid ones tends to misfire in practice
   — scoring-based agents pick a unit that already moved, aim at an enemy
   out of range, etc. The tree replaces that with "pick from a known-valid
   set."
2. **Logging & debuggability.** Trees are `Debug`-printable. Snapshot the
   tree at each decision, diff across ticks, or replay from logs.
3. **UI reuse.** Renderers query the same `CommandProvider` to light up
   targetable tiles, enable/disable buttons, show cooldowns. One source
   of truth for "what can this player do right now."
4. **Continuous parameters.** `Parametric` nodes with `ParamDomain`s
   handle non-discrete inputs (aim angles, rotation speeds, move vectors)
   — the kind of actions a 3D / FPS / physics game needs.
5. **Hierarchical agents.** Structural sharing via `Arc` and
   implementer-owned caching let trees scale to large action spaces
   without rebuilding everything each tick.

## The tree shape

```rust
pub enum CommandTree<C> {
    Empty,                                  // no valid action
    Leaf(C),                                // a fully-specified command
    Parametric {                            // leaf with continuous params
        label: String,
        builder: Arc<dyn CommandBuilder<C>>,
    },
    Layer {                                 // eager branch
        label: String,
        children: Vec<(String, Arc<CommandTree<C>>)>,
    },
    LazyLayer {                             // deferred branch
        label: String,
        expand: Arc<dyn Fn() -> Vec<(String, Arc<CommandTree<C>>)> + Send + Sync>,
        cache: OnceLock<Vec<(String, Arc<CommandTree<C>>)>>,
    },
}
```

- `Empty` is what a non-current player gets in a turn-based game.
- `Leaf(C)` is the common case: a ready-to-submit discrete command.
- `Parametric` carries a `CommandBuilder` that knows the valid parameter
  domains and builds the concrete `C` once the agent picks values.
- `Layer` is an eager keyed branch. Keys are implementer-chosen
  (`"attack"`, `"unit_42"`, `"(3,5)"`, etc.) and carry no framework
  meaning — they just let hierarchical agents and UIs route by key.
- `LazyLayer` is a deferred branch. Its `expand` closure is called the
  first time any traversal helper needs the children; the result is
  cached in a `OnceLock`. Construct via `CommandTree::lazy_layer(label,
  closure)`. Agents that never descend into a `LazyLayer` never pay the
  cost of enumerating it.

The layering is entirely up to the implementer. A typical turn-based
game uses `action-type → unit → target`. A real-time game might use
`unit → verb` or a flat root with one child per commandable event.

## Parameter domains

```rust
pub enum ParamDomain {
    Continuous { min: f64, max: f64 },
    Discrete(Vec<f64>),
    Int { min: i64, max: i64 },
}
```

A `Parametric` leaf has one `ParamDomain` per parameter. The
`CommandBuilder::build(values: &[f64])` takes the chosen values in the
same order and constructs the concrete `C`.

## Implementing `CommandProvider`

```rust
use telic::arena::{CommandTree, CommandProvider, PlayerIndex};
use std::sync::Arc;

struct MyGameCommands;

impl CommandProvider for MyGameCommands {
    type State = MyGame;

    fn command_tree(
        state: &MyGame,
        player: PlayerIndex,
    ) -> Arc<CommandTree<MyCommand>> {
        if !state.is_turn_of(player) {
            return Arc::new(CommandTree::Empty);
        }

        // Build per-unit subtrees, compose into a root Layer.
        let mut unit_children = Vec::new();
        for unit in state.units_of(player) {
            unit_children.push((
                unit.id.to_string(),
                build_unit_subtree(state, unit),
            ));
        }

        Arc::new(CommandTree::Layer {
            label: "actions".into(),
            children: unit_children,
        })
    }
}
```

### A `Parametric` example — FPS aim

```rust
#[derive(Debug)]
struct AimDelta { max_yaw_rate: f64, max_pitch_rate: f64 }

impl CommandBuilder<Command> for AimDelta {
    fn domains(&self) -> Vec<ParamDomain> {
        vec![
            ParamDomain::Continuous { min: -self.max_yaw_rate, max: self.max_yaw_rate },
            ParamDomain::Continuous { min: -self.max_pitch_rate, max: self.max_pitch_rate },
        ]
    }
    fn build(&self, v: &[f64]) -> Command {
        Command::AimDelta { yaw: v[0], pitch: v[1] }
    }
}

// In command_tree:
CommandTree::Parametric {
    label: "aim".into(),
    builder: Arc::new(AimDelta { max_yaw_rate: 2.0, max_pitch_rate: 1.0 }),
}
```

## Writing an agent against the tree

Implement `GameAgent::decide(view, tree) -> Option<C>`:

### Random — pick uniformly from flat leaves

```rust
fn decide(&mut self, _v: &View, tree: &CommandTree<Command>) -> Option<Command> {
    let leaves = tree.flatten();
    if leaves.is_empty() { return None; }
    let i = self.rng.gen::<usize>() % leaves.len();
    Some(leaves[i].clone())
}
```

### Utility — score each leaf, take the max

```rust
fn decide(&mut self, view: &View, tree: &CommandTree<Command>) -> Option<Command> {
    tree.argmax(|cmd| self.score_command(cmd, view))
}
```

### Hierarchical — traverse layer-by-layer

```rust
fn decide(&mut self, view: &View, tree: &CommandTree<Command>) -> Option<Command> {
    match tree {
        CommandTree::Leaf(c) => Some(c.clone()),
        CommandTree::Layer { children, .. } => {
            // score each category, descend into the best
            let (_, best) = children.iter()
                .max_by(|a, b| {
                    let sa = self.score_category(&a.0, view);
                    let sb = self.score_category(&b.0, view);
                    sa.partial_cmp(&sb).unwrap()
                })?;
            self.decide(view, best)
        }
        CommandTree::Parametric { builder, .. } => {
            let chosen: Vec<f64> = builder.domains().iter()
                .map(|d| self.pick_value(d, view))
                .collect();
            Some(builder.build(&chosen))
        }
        CommandTree::Empty => None,
    }
}
```

## Running the arena

```rust
let report = MultiPlayerArena::<MyView, MyCommand>::new(2)
    .with_games(1000)
    .add_agent_type(...)
    .run::<MyGame, MyGameCommands>(|_| MyGame::new());
```

The `run` path:
1. Builds a command tree for each player at the start of each tick.
2. Non-acting players get `Empty` trees and are skipped (no wasted
   "not your turn" rejection dance).
3. Active players get their tree and return a chosen command via
   `decide_with_tree`.
4. The tree is the single source of truth — a rejection from
   `apply_command` indicates a bug in the `CommandProvider` or the
   agent's tree traversal, not normal operation.

## Performance notes

- **Structural sharing.** Children are `Arc<CommandTree<C>>`. If your
  game caches per-unit subtrees keyed by unit state, you can assemble
  the per-tick tree with cheap pointer clones instead of re-enumerating
  every move.
- **Laziness.** Use `CommandTree::lazy_layer(label, closure)` when a
  branch is expensive to enumerate and hierarchical agents only drill
  into a few branches per decision. The closure fires on the first call
  to `children()` (or any helper that needs children); subsequent calls
  hit the cached result. Agents that never descend into a lazy branch
  never pay for it. Example — per-unit actions in an RTS:

  ```rust
  // Root is cheap; each unit's action list is built only if the agent
  // actually asks for it.
  let unit_branches: Vec<_> = units.iter().map(|u| {
      let uid = u.id;
      let state_snapshot = state.clone();  // or an Arc<State>
      let lazy = Arc::new(CommandTree::lazy_layer(
          format!("unit_{uid}"),
          move || enumerate_actions_for(&state_snapshot, uid),
      ));
      (format!("unit_{uid}"), lazy)
  }).collect();
  ```

- **Tree size.** For SimpleWars-scale games (~100 valid commands per
  tick), eager construction is sub-millisecond and caching is
  unnecessary. For StarCraft-scale action spaces, use `LazyLayer` at
  the per-unit level and `Arc` caching across ticks.
- **Parametric nodes** are only constructed when the game has
  continuous actions. Discrete-only games pay nothing for the
  parametric path.

## Real-time games

Real-time games (e.g. `arena_combat`, where each tick accepts a
`Vec<Command>` batch from each player) don't fit the tree API naturally
— the per-tick action space is combinatorial across units. These games
can provide a degenerate `CommandProvider` whose tree is a single
`Leaf(Vec::new())` placeholder; the agent ignores the tree content and
returns its own batch from `decide`.

```rust
impl CommandProvider for ArenaCombatCommands {
    type State = ArenaCombatGame;
    fn command_tree(state: &ArenaCombatGame, _player: PlayerIndex)
        -> Arc<CommandTree<Vec<Command>>>
    {
        if state.is_terminal() { Arc::new(CommandTree::Empty) }
        else { Arc::new(CommandTree::Leaf(Vec::new())) }
    }
}
```

If a real-time game does want the tree benefits (logging, UI, parameter
domains for continuous inputs), the cleanest shape is to have the tree
expose per-unit subtrees with `Parametric` leaves for continuous inputs,
and let the agent compose the batch from multiple tree picks per tick.
