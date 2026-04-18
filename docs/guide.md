# Usage Guide

This walkthrough targets **telic** — a goal-directed game AI framework for
Rust. The crate name is `telic`, imports are `use telic::...`.

## Step 1: Define Your Game

Implement `GameState` and `GameView`:

```rust
use telic::arena::{GameState, GameView, GameOutcome, PlayerIndex};

#[derive(Debug, Clone)]
struct MyView {
    viewer: usize,
    turn: u32,
    // ... what the player can see
}

impl GameView for MyView {
    fn viewer(&self) -> PlayerIndex { self.viewer }
    fn turn(&self) -> u32 { self.turn }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MyCommand { Move(u16, u8, u8), Attack(u16, u16), EndTurn }

struct MyGame { /* ... */ }

impl GameState for MyGame {
    type Command = MyCommand;
    type View = MyView;

    fn view_for(&self, player: PlayerIndex) -> MyView { /* ... */ }
    fn apply_command(&mut self, player: PlayerIndex, cmd: MyCommand) -> Result<(), String> {
        // Reject if not this player's turn (turn-based)
        // Or accept from all players each tick (real-time)
        /* ... */
    }
    fn is_terminal(&self) -> bool { /* ... */ }
    fn outcome(&self) -> Option<GameOutcome> { /* ... */ }
    fn turn_number(&self) -> u32 { /* ... */ }
    fn num_players(&self) -> usize { 2 }
}
```

**Turn-based**: `apply_command` tracks whose turn it is internally and rejects commands from the wrong player.

**Real-time**: `apply_command` collects commands from all players and ticks the simulation when all have submitted. Use `Vec<Command>` as the command type for batched per-tick commands.

## Step 2: Build Agents

### Minimal agent

```rust
use telic::arena::{GameAgent, PlayerIndex};

#[derive(Debug)]
struct MyAgent { name: String, player: usize }

impl GameAgent<MyView, MyCommand> for MyAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &MyView) {}
    fn decide(&mut self, view: &MyView) -> MyCommand { /* ... */ }
}
```

### Utility-scored agent

Define a context struct and score actions:

```rust
use telic::planning::utility::{UtilityAction, ResponseCurve, ScoringMode};

struct ScoringCtx { distance: f64, value: f64, threat: f64 }

let scorer = UtilityAction::new("my_action")
    .with_base(0.0)
    .with_mode(ScoringMode::Additive)
    .consider("value", |ctx: &ScoringCtx| ctx.value,
        ResponseCurve::Identity, 1.0)
    .consider("proximity", |ctx: &ScoringCtx| 1.0 / ctx.distance,
        ResponseCurve::Identity, 1.0)
    .consider("danger", |ctx: &ScoringCtx| if ctx.threat > 5.0 { -3.0 } else { 0.0 },
        ResponseCurve::Identity, 1.0);

let score = scorer.score(&ScoringCtx { distance: 3.0, value: 10.0, threat: 2.0 });
```

### Multi-entity coordination

Pick an assignment strategy and invoke it on an `[entity][task]` score matrix.
`Greedy` picks the globally best pair each step; `RoundRobin` picks in entity
priority order. Both accept a coordination callback.

```rust
use telic::planning::utility::{Greedy, RoundRobin, AssignmentStrategy};

let mut scores: Vec<Vec<f64>> = /* score matrix: [entity][task] */;

// Greedy with coordination adjustments after each pick
let assignments = Greedy::with_coordination(|entity_idx, task_idx, scores| {
    // diminish same-target captures, boost focus fire, etc.
}).assign(&mut scores);

// Or: round-robin in a custom priority order (no coordination)
let assignments = RoundRobin::with_order(vec![2, 0, 1]).assign(&mut scores);
```

### Smart objects

```rust
use telic::planning::utility::ActionSource;

impl ActionSource<MyTask, MyView> for Building {
    fn available_actions(&self, view: &MyView) -> Vec<MyTask> {
        if self.owner != view.viewer { vec![MyTask::capture(self.pos)] }
        else { vec![] }
    }
}
```

### Debugging with traces

```rust
let trace = scorer.score_with_trace(&ctx);
println!("{}", trace);
// === my_action (score: 13.33, base: 0.00, mode: Additive) ===
//   value         raw=  10.000  curved=  10.000  w=1.00  contrib=  10.000
//   proximity     raw=   0.333  curved=   0.333  w=1.00  contrib=   0.333
//   danger        raw=   0.000  curved=   0.000  w=1.00  contrib=   0.000
```

### Opponent modeling

Track behavior in `observe()`, use in scoring:

```rust
fn observe(&mut self, view: &MyView) {
    // Track what opponent did → correlate with outcomes
    if let Some(opp_cards) = view.opp_revealed_cards {
        if self.opp_raised_this_hand {
            self.raise_showdowns += 1;
            if is_strong_hand(opp_cards) { self.raise_was_strong += 1; }
        }
    }
}

// In scoring: adjust based on learned opponent behavior
let raise_honesty = self.raise_was_strong as f64 / self.raise_showdowns as f64;
// High honesty → fold more to their raises (they never bluff)
```

## Step 3: Evaluate

```rust
use telic::arena::{MultiPlayerArena, ClosureFactory};

let report = MultiPlayerArena::new(2)  // or 3, 4, 6 for multiplayer
    .with_games(1000)
    .with_max_turns(500)
    .add_agent_type(ClosureFactory::new("my_agent", || Box::new(MyAgent::new())))
    .add_agent_type(ClosureFactory::new("baseline", || Box::new(RandomAgent::new())))
    .run(|num_players| MyGame::new());

report.print_summary();
```

Output includes TrueSkill ratings (μ/σ), win rates, average finish position, and a pairwise head-to-head matrix.

For randomized games:

```rust
let mut seed = 1u64;
let report = arena.run(move |_| {
    seed += 1;
    MyGame::new_random(seed)
});
```

## Step 4: Iterate

1. Start with a random baseline agent
2. Build a hand-coded agent with simple heuristics
3. Build a utility-scored agent using `UtilityAction<S>`
4. Add coordination if multi-entity (`Greedy` / `RoundRobin` via `AssignmentStrategy`)
5. Add opponent awareness (track their actions in memory)
6. Use `score_with_trace()` to debug unexpected decisions
7. Run TrueSkill evaluation on randomized games
8. Compare μ values — a gap of ~3 points ≈ 75% win rate

## Optional: GOAP for Strategic Weighting

If your game has strategic phases (expand vs attack vs defend), GOAP can determine emphasis:

```rust
use telic::planning::belief::{BeliefBuilder, BeliefSet};
use telic::planning::planner::GoapPlanner;

let plan = GoapPlanner::plan(&beliefs, &view, &actions, &goals, None);

// Use as WEIGHT MODIFIER, not filter — all tasks always available
let capture_weight = if priority == "expand" { 1.5 } else { 0.8 };
```

GOAP should tilt scoring emphasis, not gate which tasks are available.
