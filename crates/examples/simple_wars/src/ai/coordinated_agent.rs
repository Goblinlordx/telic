use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::utility::{UtilityAction, ResponseCurve, ScoringMode};

use crate::game::types::*;
use crate::game::state::SimpleWarsView;

/// Coordinated agent — built on the framework's generic `UtilityAction<S>`
/// and the `Greedy` assignment strategy for multi-unit coordination.
///
/// Architecture:
///
/// 1. **State context** (`ScoringCtx`): all information needed to score a
///    unit-task pair — the view, the unit, the task, and pre-computed globals.
///    Beliefs are just functions over this context.
///
/// 2. **Utility actions** (`UtilityAction<ScoringCtx>`): four action templates
///    (Capture, Attack, Advance, Defend) with considerations that evaluate
///    directly from `&ScoringCtx` through `ResponseCurve`s.
///
/// 3. **Coordinated assignment** (`Greedy` strategy): scores all unit-task pairs,
///    assigns greedily, and adjusts remaining scores after each assignment for:
///    - Capture spreading (diminishing returns for same city)
///    - Focus fire (bonus for concentrating on tough enemies)
///    - Escort (tanks protect nearby capturing infantry)
/// Which assignment strategy the agent uses to turn the unit-task score
/// matrix into concrete (unit, task) assignments.
///
/// The `GreedyCoordinated` variant runs the full domain-specific coordination
/// callback (capture spreading, focus fire, escort). The others apply their
/// namesake strategy to the raw score matrix with no callback, providing
/// clean ablation baselines: all share the same scoring; only assignment
/// differs.
#[derive(Debug, Clone, Copy)]
pub enum StrategyChoice {
    /// Greedy with the full coordination callback (the historical champion).
    GreedyCoordinated,
    /// Round-robin by entity index (task reuse allowed).
    RoundRobin,
    /// Optimal one-to-one Kuhn-Munkres assignment.
    Hungarian,
    /// Seeded softmax-sampled assignment (one-to-one).
    WeightedRandom(u64),
}

#[derive(Debug)]
pub struct CoordinatedAgent {
    name: String,
    player: PlayerIndex,
    actions_this_turn: u32,
    explored: Vec<Vec<bool>>,
    map_rows: u8,
    map_cols: u8,
    last_known_enemies: Vec<(Pos, u32)>,
    strategy: StrategyChoice,
}

// =========================================================================
// Scoring context — the "state" that beliefs evaluate from
// =========================================================================

/// Everything needed to score a unit-task pair. Passed as `&S` to
/// `UtilityAction<S>::score()`.
///
/// Owns copies of the relevant data so the utility actions can be
/// defined with `UtilityAction<ScoringCtx>` (no lifetime parameter).
struct ScoringCtx {
    // Unit data
    unit_type: UnitType,
    unit_pos: Pos,
    unit_hp: u8,
    // Task data
    task_kind_idx: usize, // 0=capture, 1=attack, 2=advance, 3=defend
    task_target: Pos,
    task_value: f64, // building value or combat odds
    // Global beliefs
    hq_threat: f64,
    our_strength: f64,
    strength_advantage: f64,
    economy: f64,
    // View data needed by considerations
    our_hq: Pos,
    near_hq: bool, // unit within 4 of our HQ
}

impl ScoringCtx {
    fn build(unit: &Unit, task: &UnitTask, view: &SimpleWarsView, player: PlayerIndex) -> Self {
        let hq_threat: f64 = view.visible_enemy_units.iter()
            .map(|e| {
                let dist = e.pos.manhattan_distance(view.our_hq).max(1) as f64;
                e.unit_type.attack_power() as f64 / dist
            }).sum();

        let our_strength: f64 = view.our_units.iter()
            .map(|u| u.unit_type.attack_power() as f64 * (u.hp as f64 / 10.0))
            .sum();

        let enemy_strength: f64 = view.visible_enemy_units.iter()
            .map(|u| u.unit_type.attack_power() as f64 * (u.hp as f64 / 10.0))
            .sum();

        let strength_advantage = if enemy_strength > 0.0 {
            our_strength / enemy_strength
        } else { 2.0 };

        let economy = view.buildings.iter()
            .filter(|b| b.owner == Some(player)).count() as f64;

        ScoringCtx {
            unit_type: unit.unit_type,
            unit_pos: unit.pos,
            unit_hp: unit.hp,
            task_kind_idx: match task.kind {
                TaskKind::Capture => 0,
                TaskKind::Attack => 1,
                TaskKind::Advance => 2,
                TaskKind::Defend => 3,
            },
            task_target: task.target,
            task_value: task.value,
            hq_threat,
            our_strength,
            strength_advantage,
            economy,
            our_hq: view.our_hq,
            near_hq: unit.pos.manhattan_distance(view.our_hq) < 4,
        }
    }
}

/// Agent-internal task — extends smart object OfferedTasks with memory-based
/// tasks (from last known enemies) and agent-specific context.
#[derive(Debug, Clone)]
struct UnitTask {
    kind: TaskKind, // uses game-level TaskKind from types.rs
    target: Pos,
    value: f64,     // building value or combat odds
}

impl CoordinatedAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self::with_strategy(name, seed, StrategyChoice::GreedyCoordinated)
    }

    pub fn with_strategy(name: impl Into<String>, _seed: u64, strategy: StrategyChoice) -> Self {
        Self {
            name: name.into(),
            player: 0,
            actions_this_turn: 0,
            explored: Vec::new(),
            map_rows: 0,
            map_cols: 0,
            last_known_enemies: Vec::new(),
            strategy,
        }
    }

    // =========================================================================
    // Utility action definitions — the "beliefs" are closures over ScoringCtx
    // =========================================================================

    /// Build the four utility action templates. Each consideration evaluates
    /// from `&ScoringCtx` and outputs a raw multiplier (not [0,1] normalized).
    /// The Constant(1.0) curve passes through the evaluator's output directly.
    fn build_actions() -> Vec<UtilityAction<ScoringCtx>> {
        // CAPTURE: building_value * economy_modifier * threat_penalty / distance
        let capture = UtilityAction::new("capture")
            .with_base(1.0)
            .with_mode(ScoringMode::Multiplicative)
            .consider("can_capture",
                |ctx: &ScoringCtx| if ctx.unit_type.can_capture() { 1.0 } else { 0.0 },
                ResponseCurve::Identity, 1.0)
            .consider("building_value",
                |ctx: &ScoringCtx| ctx.task_value,
                ResponseCurve::Identity, 1.0)
            .consider("economy_need",
                |ctx: &ScoringCtx| if ctx.economy < 3.0 { 1.5 } else { 1.0 },
                ResponseCurve::Identity, 1.0)
            .consider("hq_safety",
                |ctx: &ScoringCtx| {
                    if ctx.hq_threat > 3.0 && ctx.near_hq { 0.3 } else { 1.0 }
                },
                ResponseCurve::Identity, 1.0)
            .consider("inv_distance",
                |ctx: &ScoringCtx| 1.0 / ctx.unit_pos.manhattan_distance(ctx.task_target).max(1) as f64,
                ResponseCurve::Identity, 1.0);

        // ATTACK: base(4) * aggression * strength_mod * unit_fit / distance
        let attack = UtilityAction::new("attack")
            .with_base(4.0)
            .with_mode(ScoringMode::Multiplicative)
            .consider("aggression",
                |ctx: &ScoringCtx| {
                    if ctx.task_value > 1.2 { ctx.task_value } else { ctx.task_value * 0.5 }
                },
                ResponseCurve::Identity, 1.0)
            .consider("strength_mod",
                |ctx: &ScoringCtx| if ctx.strength_advantage > 1.3 { 1.5 } else { 1.0 },
                ResponseCurve::Identity, 1.0)
            .consider("unit_fitness",
                |ctx: &ScoringCtx| match ctx.unit_type {
                    UnitType::Tank => 1.3,
                    UnitType::Infantry => 0.8,
                    UnitType::Artillery => 1.5,
                    UnitType::Recon => 0.5,
                },
                ResponseCurve::Identity, 1.0)
            .consider("inv_distance",
                |ctx: &ScoringCtx| 1.0 / ctx.unit_pos.manhattan_distance(ctx.task_target).max(1) as f64,
                ResponseCurve::Identity, 1.0);

        // ADVANCE: base(1.5) * strength_mod / distance
        let advance = UtilityAction::new("advance")
            .with_base(1.5)
            .with_mode(ScoringMode::Multiplicative)
            .consider("strength_mod",
                |ctx: &ScoringCtx| if ctx.our_strength > 20.0 { 2.0 } else { 1.0 },
                ResponseCurve::Identity, 1.0)
            .consider("inv_distance",
                |ctx: &ScoringCtx| 1.0 / ctx.unit_pos.manhattan_distance(ctx.task_target).max(1) as f64,
                ResponseCurve::Identity, 1.0);

        // DEFEND: threat_urgency * unit_combat / distance
        let defend = UtilityAction::new("defend")
            .with_base(1.0)
            .with_mode(ScoringMode::Multiplicative)
            .consider("threat_urgency",
                |ctx: &ScoringCtx| (ctx.hq_threat * 2.0).max(1.0),
                ResponseCurve::Identity, 1.0)
            .consider("unit_combat",
                |ctx: &ScoringCtx| ctx.unit_type.attack_power() as f64 / 5.0,
                ResponseCurve::Identity, 1.0)
            .consider("inv_distance",
                |ctx: &ScoringCtx| 1.0 / ctx.unit_pos.manhattan_distance(ctx.task_target).max(1) as f64,
                ResponseCurve::Identity, 1.0);

        vec![capture, attack, advance, defend]
    }

    fn action_index(kind: &TaskKind) -> usize {
        match kind {
            TaskKind::Capture => 0,
            TaskKind::Attack => 1,
            TaskKind::Advance => 2,
            TaskKind::Defend => 3,
        }
    }

    // =========================================================================
    // Build & task generation
    // =========================================================================

    fn desired_build(&self, view: &SimpleWarsView) -> Option<UnitType> {
        let infantry = view.our_units.iter().filter(|u| u.unit_type == UnitType::Infantry).count();
        let tanks = view.our_units.iter().filter(|u| u.unit_type == UnitType::Tank).count();
        let total = view.our_units.len();

        if total >= 6 { return None; }
        if infantry < 3 { return Some(UnitType::Infantry); }
        if tanks < 2 && infantry >= 2 && view.our_gold >= UnitType::Tank.cost() {
            return Some(UnitType::Tank);
        }
        if total < 6 { return Some(UnitType::Infantry); }
        None
    }

    // Note: the old task-generation, per-unit assignment, and execute_unit
    // scaffolding was removed in the tree-API migration. The agent now scores
    // each concrete `Command` from the provider's tree directly via
    // `score_command` below, and the arena's `run::<G, P>(...)` path
    // guarantees every leaf is a valid command the game will accept.
}


impl CoordinatedAgent {
    // =========================================================================
    // Tree-based scoring — each command in the tree is scored independently
    // by mapping it back to a (unit, task) pair and running the existing
    // utility-action pipeline. Move commands score against whichever task
    // target they most progress toward.
    // =========================================================================

    fn score_command(&self, cmd: &Command, view: &SimpleWarsView) -> f64 {
        match cmd {
            Command::EndTurn => -1.0, // baseline; beaten by anything with positive score
            Command::Build { unit_type } => self.score_build(*unit_type, view),
            Command::Capture { unit_id } => self.score_capture(*unit_id, view),
            Command::Attack { unit_id, target_pos } => {
                self.score_attack(*unit_id, *target_pos, view)
            }
            Command::Move { unit_id, to } => self.score_move(*unit_id, *to, view),
            Command::MoveAttack { .. } => -1e9, // not enumerated by the tree
        }
    }

    fn score_build(&self, ut: UnitType, view: &SimpleWarsView) -> f64 {
        // Prefer the unit type our build heuristic wants; accept others at
        // lower priority so we still build if that's the only option.
        match self.desired_build(view) {
            Some(wanted) if wanted == ut => 60.0,
            Some(_) => 15.0,
            None => -50.0,
        }
    }

    fn score_capture(&self, unit_id: u16, view: &SimpleWarsView) -> f64 {
        let Some(unit) = view.our_units.iter().find(|u| u.id == unit_id) else { return -1e9 };
        let building = view.buildings.iter().find(|b| b.pos == unit.pos);
        let value = building.map(|b| self.building_value_at(b, view)).unwrap_or(4.0);
        // Captures score via the capture utility action, but with distance = 0
        // (we're standing on the target). Use a synthetic task.
        let task = UnitTask { kind: TaskKind::Capture, target: unit.pos, value };
        let ctx = ScoringCtx::build(unit, &task, view, self.player);
        let actions = Self::build_actions();
        // Boost: capturing when already on the building is the single highest-
        // value action available. Add a flat bonus to out-compete high-value
        // moves and attacks.
        actions[Self::action_index(&TaskKind::Capture)].score(&ctx) * 3.0 + 10.0
    }

    fn score_attack(&self, unit_id: u16, target: Pos, view: &SimpleWarsView) -> f64 {
        let Some(unit) = view.our_units.iter().find(|u| u.id == unit_id) else { return -1e9 };
        let Some(enemy) = view.visible_enemy_units.iter().find(|e| e.pos == target) else {
            return -1e9;
        };
        let combat_odds = self.combat_odds(unit, enemy);
        let task = UnitTask {
            kind: TaskKind::Attack,
            target,
            value: combat_odds,
        };
        let ctx = ScoringCtx::build(unit, &task, view, self.player);
        let actions = Self::build_actions();
        actions[Self::action_index(&TaskKind::Attack)].score(&ctx)
    }

    fn score_move(&self, unit_id: u16, to: Pos, view: &SimpleWarsView) -> f64 {
        let Some(unit) = view.our_units.iter().find(|u| u.id == unit_id) else { return -1e9 };
        let from = unit.pos;
        let actions = Self::build_actions();
        let mut best: f64 = -1e6;

        // For each possible task, score the unit as if it had moved to `to`
        // and then discount by a "committed but not arrived" penalty. We only
        // consider tasks where the move strictly decreases distance to the
        // target — otherwise moving toward that task is anti-productive.
        let score_for_task = |task: &UnitTask, a: &[UtilityAction<ScoringCtx>]| -> f64 {
            let old_dist = from.manhattan_distance(task.target) as i32;
            let new_dist = to.manhattan_distance(task.target) as i32;
            if new_dist >= old_dist { return -1e6; }

            // Synthesize a unit located at `to` for scoring purposes.
            let projected = Unit { pos: to, ..unit.clone() };
            let ctx = ScoringCtx::build(&projected, task, view, self.player);
            let raw = a[Self::action_index(&task.kind)].score(&ctx);
            // Discount: we're only progressing toward the task, not completing it.
            raw * 0.7
        };

        // Capture tasks for every non-owned building reachable by capturers.
        if unit.unit_type.can_capture() {
            for building in &view.buildings {
                if building.owner == Some(self.player) { continue; }
                let task = UnitTask {
                    kind: TaskKind::Capture,
                    target: building.pos,
                    value: self.building_value_at(building, view),
                };
                best = best.max(score_for_task(&task, &actions));
            }
        }

        // Attack tasks for visible enemies.
        for enemy in &view.visible_enemy_units {
            let task = UnitTask {
                kind: TaskKind::Attack,
                target: enemy.pos,
                value: self.combat_odds(unit, enemy),
            };
            best = best.max(score_for_task(&task, &actions));
        }

        // Attack tasks from memory (if no visible enemies).
        if view.visible_enemy_units.is_empty() {
            for (pos, _) in &self.last_known_enemies {
                let task = UnitTask {
                    kind: TaskKind::Attack,
                    target: *pos,
                    value: 1.0,
                };
                best = best.max(score_for_task(&task, &actions));
            }
        }

        // Advance toward enemy HQ.
        let advance = UnitTask {
            kind: TaskKind::Advance,
            target: view.enemy_hq,
            value: 0.0,
        };
        best = best.max(score_for_task(&advance, &actions));

        // Defend if HQ threatened.
        let hq_threat: f64 = view.visible_enemy_units.iter()
            .map(|e| {
                let dist = e.pos.manhattan_distance(view.our_hq).max(1) as f64;
                e.unit_type.attack_power() as f64 / dist
            }).sum();
        if hq_threat > 1.0 {
            let defend = UnitTask {
                kind: TaskKind::Defend,
                target: view.our_hq,
                value: 0.0,
            };
            best = best.max(score_for_task(&defend, &actions));
        }

        best
    }

    fn building_value_at(&self, b: &Building, view: &SimpleWarsView) -> f64 {
        match b.terrain {
            Terrain::HQ => 20.0,
            Terrain::City => {
                let dist_to_us = b.pos.manhattan_distance(view.our_hq) as f64;
                let dist_to_enemy = b.pos.manhattan_distance(view.enemy_hq) as f64;
                if dist_to_us < dist_to_enemy { 6.0 } else { 4.0 }
            }
            _ => 0.0,
        }
    }

    fn combat_odds(&self, attacker: &Unit, defender: &Unit) -> f64 {
        let a = attacker.unit_type.attack_power() as f64 * (attacker.hp as f64 / 10.0);
        let d = defender.unit_type.attack_power() as f64 * (defender.hp as f64 / 10.0);
        if d == 0.0 { 10.0 } else { a / d }
    }
}

impl GameAgent<SimpleWarsView, Command> for CoordinatedAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
        self.explored.clear();
        self.map_rows = 0;
        self.map_cols = 0;
        self.last_known_enemies.clear();
    }

    fn observe(&mut self, view: &SimpleWarsView) {
        if self.explored.is_empty() {
            self.map_rows = view.rows;
            self.map_cols = view.cols;
            self.explored = vec![vec![false; view.cols as usize]; view.rows as usize];
        }
        for r in 0..view.rows as usize {
            for c in 0..view.cols as usize {
                if view.visibility[r][c] { self.explored[r][c] = true; }
            }
        }
        for enemy in &view.visible_enemy_units {
            self.last_known_enemies.retain(|(p, _)| *p != enemy.pos);
            self.last_known_enemies.push((enemy.pos, view.turn));
        }
        self.last_known_enemies.retain(|(_, turn)| view.turn - turn < 10);
    }

    fn decide(
        &mut self,
        view: &SimpleWarsView,
        tree: &CommandTree<Command>,
    ) -> Option<Command> {
        if tree.is_empty() { return None; }

        self.actions_this_turn += 1;
        if self.actions_this_turn > 30 {
            self.actions_this_turn = 0;
            return tree.find_leaf(|c| matches!(c, Command::EndTurn)).cloned();
        }

        // Genuine tree traversal: score every leaf using our domain logic.
        // Coordination across units isn't needed here — only one command is
        // played per tick, so we just want the globally-best command.
        //
        // `StrategyChoice` originally controlled per-unit assignment when
        // multiple units needed tasks in a single decision call. In a
        // one-command-per-tick tree-based arena, the strategies collapse
        // to "argmax across leaves" (for Greedy/Hungarian/RoundRobin) or
        // "softmax-sample" (for WeightedRandom). Keeping the enum lets us
        // still distinguish stochastic vs deterministic variants.
        let best = match self.strategy {
            StrategyChoice::WeightedRandom(seed) => {
                self.sample_weighted(tree, view, seed)
            }
            _ => tree.argmax(|cmd| self.score_command(cmd, view)),
        };

        if matches!(best, Some(Command::EndTurn)) {
            self.actions_this_turn = 0;
        }
        best
    }
}

impl CoordinatedAgent {
    /// Softmax-sample a tree leaf using the agent's command scorer.
    /// Deterministic per (seed, tick) via the agent's internal counter.
    fn sample_weighted(
        &mut self,
        tree: &CommandTree<Command>,
        view: &SimpleWarsView,
        seed: u64,
    ) -> Option<Command> {
        let leaves = tree.flatten();
        if leaves.is_empty() { return None; }
        let scores: Vec<f64> = leaves.iter()
            .map(|c| self.score_command(c, view))
            .collect();
        let max = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let weights: Vec<f64> = scores.iter().map(|s| (s - max).exp()).collect();
        let total: f64 = weights.iter().sum();
        if total <= 0.0 { return Some(leaves[0].clone()); }

        // Mix seed with actions_this_turn so successive ticks get different draws.
        let mut r = seed.wrapping_mul(6364136223846793005)
            .wrapping_add(self.actions_this_turn as u64);
        r ^= r << 13; r ^= r >> 7; r ^= r << 17;
        let u = (r >> 11) as f64 / ((1u64 << 53) as f64);
        let pick = u * total;
        let mut cum = 0.0;
        for (c, w) in leaves.iter().zip(&weights) {
            cum += *w;
            if cum >= pick { return Some(c.clone()); }
        }
        Some(leaves.last().cloned().unwrap())
    }
}
