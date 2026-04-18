use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::SimpleWarsView;

/// Plan-commitment agent — extends utility agent with multi-turn plan tracking.
///
/// Key insight: on hard mode (5-turn sieges), the pure per-turn utility agent
/// keeps reassigning infantry away from half-captured cities because a slightly
/// better-looking capture appears elsewhere. This agent tracks committed plans
/// and only abandons them when the alternative is *significantly* better.
///
/// Architecture:
/// 1. **Observe**: Update memory, assess committed plan validity
/// 2. **Re-assess**: Compute current value of each committed plan vs alternatives
/// 3. **Commit or switch**: Only abandon a plan when alternative > committed * threshold
/// 4. **Execute**: Same tactical layer as utility agent
#[derive(Debug)]
pub struct CommittedAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    actions_this_turn: u32,
    /// Memory: cells we've ever seen
    explored: Vec<Vec<bool>>,
    map_rows: u8,
    map_cols: u8,
    /// Memory: last known enemy positions
    last_known_enemies: Vec<(Pos, u32)>,
    /// Active committed plans — one per unit
    committed_plans: Vec<CommittedPlan>,
    /// How much better an alternative must be to abandon a committed plan.
    /// 1.5 = alternative must be 50% better than current committed value.
    abandonment_threshold: f64,
}

/// A tracked multi-turn plan for a specific unit.
#[derive(Debug, Clone)]
struct CommittedPlan {
    unit_id: u16,
    kind: TaskKind,
    target: Pos,
    /// Value when the plan was first committed
    initial_value: f64,
    /// How many turns this unit has been committed
    turns_committed: u32,
}

#[derive(Debug, Clone)]
struct UnitTask {
    kind: TaskKind,
    target: Pos,
    building_value: f64,
    combat_odds: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum TaskKind {
    Capture,
    Attack,
    Advance,
    Defend,
}

impl CommittedAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            actions_this_turn: 0,
            explored: Vec::new(),
            map_rows: 0,
            map_cols: 0,
            last_known_enemies: Vec::new(),
            committed_plans: Vec::new(),
            abandonment_threshold: 1.5,
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    // =========================================================================
    // Plan management — the core differentiator from utility agent
    // =========================================================================

    /// Remove plans for dead/missing units and invalid targets.
    fn prune_invalid_plans(&mut self, view: &SimpleWarsView) {
        let player = self.player;
        let hq_threat = self.hq_threat_level(view);
        let last_known = &self.last_known_enemies;

        self.committed_plans.retain(|plan| {
            // Unit still alive?
            let unit_alive = view.our_units.iter().any(|u| u.id == plan.unit_id);
            if !unit_alive { return false; }

            match plan.kind {
                TaskKind::Capture => {
                    view.buildings.iter().any(|b| {
                        b.pos == plan.target && b.owner != Some(player)
                    })
                }
                TaskKind::Attack => {
                    let visible_enemy = view.visible_enemy_units.iter()
                        .any(|e| e.pos == plan.target);
                    let memory_enemy = last_known.iter()
                        .any(|(p, _)| *p == plan.target);
                    visible_enemy || memory_enemy
                }
                TaskKind::Defend => {
                    hq_threat > 0.5
                }
                TaskKind::Advance => true,
            }
        });
    }

    /// Compute the current value of a committed plan, accounting for
    /// progress already invested (sunk cost that would be lost on abandon).
    fn committed_plan_value(&self, plan: &CommittedPlan, view: &SimpleWarsView) -> f64 {
        let unit = match view.our_units.iter().find(|u| u.id == plan.unit_id) {
            Some(u) => u,
            None => return 0.0,
        };

        let dist = unit.pos.manhattan_distance(plan.target).max(1) as f64;
        let base_task = UnitTask {
            kind: plan.kind.clone(),
            target: plan.target,
            building_value: self.building_value_at(plan.target, view),
            combat_odds: 1.0,
        };
        let base_value = self.score_unit_task(unit, &base_task, view);

        match plan.kind {
            TaskKind::Capture => {
                // The key insight: capture progress resets when the unit moves away.
                // If we've already invested N turns of capture, abandoning wastes
                // that investment. The commitment bonus scales with progress.
                let building = view.buildings.iter()
                    .find(|b| b.pos == plan.target && b.capturing_player == Some(self.player));

                let progress_fraction = if let Some(b) = building {
                    b.capture_progress as f64 / view.capture_threshold as f64
                } else {
                    0.0
                };

                // Commitment bonus: progress already made amplifies value.
                // At 80% capture progress, value is 5x base (strong commitment).
                // At 0% progress, no bonus.
                let commitment_multiplier = 1.0 + progress_fraction * 4.0;

                // Also factor in: unit is already at target = much higher value
                let proximity_bonus = if dist <= 1.0 { 2.0 } else { 1.0 };

                base_value * commitment_multiplier * proximity_bonus
            }
            TaskKind::Attack => {
                // Slight commitment to ongoing engagements
                let turns_bonus = 1.0 + (plan.turns_committed as f64 * 0.1).min(0.5);
                base_value * turns_bonus
            }
            _ => base_value,
        }
    }

    /// Decide whether a unit should keep its committed plan or switch.
    /// Returns the task to execute (committed or new).
    fn resolve_assignment(
        &self,
        unit: &Unit,
        committed: Option<&CommittedPlan>,
        tasks: &[UnitTask],
        view: &SimpleWarsView,
    ) -> UnitTask {
        // Find best task from scratch
        let mut best_score = f64::NEG_INFINITY;
        let mut best_task = UnitTask {
            kind: TaskKind::Advance,
            target: view.enemy_hq,
            building_value: 0.0,
            combat_odds: 0.0,
        };

        for task in tasks {
            if matches!(task.kind, TaskKind::Capture) && !unit.unit_type.can_capture() {
                continue;
            }
            let score = self.score_unit_task(unit, task, view);
            if score > best_score {
                best_score = score;
                best_task = task.clone();
            }
        }

        // If unit has a committed plan, compare with hysteresis
        if let Some(plan) = committed {
            let committed_value = self.committed_plan_value(plan, view);

            // Only apply hysteresis when there's real progress to protect.
            // For captures with significant progress, use full threshold.
            // For everything else, use a mild threshold (1.1x).
            let effective_threshold = if plan.kind == TaskKind::Capture {
                let building = view.buildings.iter()
                    .find(|b| b.pos == plan.target && b.capturing_player == Some(self.player));
                let progress = building.map(|b| b.capture_progress).unwrap_or(0);
                let progress_frac = progress as f64 / view.capture_threshold as f64;
                // Scale threshold: 1.1 at 0% → full threshold at 50%+
                if progress_frac > 0.1 {
                    1.1 + (self.abandonment_threshold - 1.1) * (progress_frac * 2.0).min(1.0)
                } else {
                    1.1
                }
            } else {
                1.1
            };

            if best_score > committed_value * effective_threshold {
                best_task
            } else {
                UnitTask {
                    kind: plan.kind.clone(),
                    target: plan.target,
                    building_value: self.building_value_at(plan.target, view),
                    combat_odds: 1.0,
                }
            }
        } else {
            best_task
        }
    }

    /// Update committed plans based on assignments.
    fn update_commitments(&mut self, assignments: &[(u16, UnitTask)], view: &SimpleWarsView) {
        // Pre-compute initial values to avoid borrow conflicts
        let initial_values: Vec<f64> = assignments.iter().map(|(uid, task)| {
            view.our_units.iter()
                .find(|u| u.id == *uid)
                .map(|u| self.score_unit_task(u, task, view))
                .unwrap_or(0.0)
        }).collect();

        for (i, (uid, task)) in assignments.iter().enumerate() {
            let existing_idx = self.committed_plans.iter()
                .position(|p| p.unit_id == *uid);

            if let Some(idx) = existing_idx {
                if self.committed_plans[idx].target == task.target
                    && self.committed_plans[idx].kind == task.kind
                {
                    self.committed_plans[idx].turns_committed += 1;
                } else {
                    self.committed_plans[idx] = CommittedPlan {
                        unit_id: *uid,
                        kind: task.kind.clone(),
                        target: task.target,
                        initial_value: initial_values[i],
                        turns_committed: 0,
                    };
                }
            } else {
                self.committed_plans.push(CommittedPlan {
                    unit_id: *uid,
                    kind: task.kind.clone(),
                    target: task.target,
                    initial_value: initial_values[i],
                    turns_committed: 0,
                });
            }
        }
    }

    // =========================================================================
    // Belief evaluations — same as utility agent
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

    fn hq_threat_level(&self, view: &SimpleWarsView) -> f64 {
        view.visible_enemy_units.iter()
            .map(|e| {
                let dist = e.pos.manhattan_distance(view.our_hq).max(1) as f64;
                e.unit_type.attack_power() as f64 / dist
            }).sum()
    }

    fn our_military_strength(&self, view: &SimpleWarsView) -> f64 {
        view.our_units.iter()
            .map(|u| u.unit_type.attack_power() as f64 * (u.hp as f64 / 10.0))
            .sum()
    }

    fn enemy_visible_strength(&self, view: &SimpleWarsView) -> f64 {
        view.visible_enemy_units.iter()
            .map(|u| u.unit_type.attack_power() as f64 * (u.hp as f64 / 10.0))
            .sum()
    }

    fn our_economy(&self, view: &SimpleWarsView) -> usize {
        view.buildings.iter().filter(|b| b.owner == Some(self.player)).count()
    }

    fn building_value_at(&self, pos: Pos, view: &SimpleWarsView) -> f64 {
        view.buildings.iter()
            .find(|b| b.pos == pos)
            .map(|b| self.building_value(b, view))
            .unwrap_or(0.0)
    }

    fn building_value(&self, building: &Building, view: &SimpleWarsView) -> f64 {
        match building.terrain {
            Terrain::HQ => 20.0,
            Terrain::City => {
                let dist_to_us = building.pos.manhattan_distance(view.our_hq) as f64;
                let dist_to_enemy = building.pos.manhattan_distance(view.enemy_hq) as f64;
                if dist_to_us < dist_to_enemy { 6.0 } else { 4.0 }
            }
            _ => 0.0,
        }
    }

    fn can_win_fight(&self, unit: &Unit, enemy: &Unit) -> f64 {
        let our_power = unit.unit_type.attack_power() as f64 * (unit.hp as f64 / 10.0);
        let their_power = enemy.unit_type.attack_power() as f64 * (enemy.hp as f64 / 10.0);
        if their_power == 0.0 { return 10.0; }
        our_power / their_power
    }

    // =========================================================================
    // Utility scoring — same formulas as utility agent
    // =========================================================================

    fn score_unit_task(&self, unit: &Unit, task: &UnitTask, view: &SimpleWarsView) -> f64 {
        let dist = unit.pos.manhattan_distance(task.target).max(1) as f64;
        let hq_threat = self.hq_threat_level(view);
        let our_strength = self.our_military_strength(view);
        let economy = self.our_economy(view);

        match task.kind {
            TaskKind::Capture => {
                if !unit.unit_type.can_capture() { return -100.0; }
                let value = task.building_value;
                let econ_modifier = if economy < 3 { 1.5 } else { 1.0 };
                let threat_penalty = if hq_threat > 3.0
                    && unit.pos.manhattan_distance(view.our_hq) < 4
                { 0.3 } else { 1.0 };
                value * econ_modifier * threat_penalty / dist
            }
            TaskKind::Attack => {
                let combat_odds = task.combat_odds;
                let aggression = if combat_odds > 1.2 { combat_odds } else { combat_odds * 0.5 };
                let strength_modifier = if our_strength > self.enemy_visible_strength(view) * 1.3 {
                    1.5
                } else {
                    1.0
                };
                let unit_fit = match unit.unit_type {
                    UnitType::Tank => 1.3,
                    UnitType::Infantry => 0.8,
                    UnitType::Artillery => 1.5,
                    UnitType::Recon => 0.5,
                };
                4.0 * aggression * strength_modifier * unit_fit / dist
            }
            TaskKind::Advance => {
                let base = 1.5;
                let strength_mod = if our_strength > 20.0 { 2.0 } else { 1.0 };
                base * strength_mod / dist
            }
            TaskKind::Defend => {
                let urgency = (hq_threat * 2.0).max(1.0);
                let unit_fit = unit.unit_type.attack_power() as f64 / 5.0;
                urgency * unit_fit / dist
            }
        }
    }

    /// Generate all possible tasks with pre-computed context.
    fn generate_tasks(&self, view: &SimpleWarsView) -> Vec<UnitTask> {
        let mut tasks = Vec::new();

        for building in &view.buildings {
            if building.owner != Some(self.player)
                && (building.terrain == Terrain::City || building.terrain == Terrain::HQ)
            {
                tasks.push(UnitTask {
                    kind: TaskKind::Capture,
                    target: building.pos,
                    building_value: self.building_value(building, view),
                    combat_odds: 0.0,
                });
            }
        }

        for enemy in &view.visible_enemy_units {
            let avg_odds = if enemy.hp > 0 {
                5.0 / enemy.unit_type.attack_power() as f64
            } else { 10.0 };
            tasks.push(UnitTask {
                kind: TaskKind::Attack,
                target: enemy.pos,
                building_value: 0.0,
                combat_odds: avg_odds,
            });
        }

        if view.visible_enemy_units.is_empty() {
            for (pos, _) in &self.last_known_enemies {
                tasks.push(UnitTask {
                    kind: TaskKind::Attack,
                    target: *pos,
                    building_value: 0.0,
                    combat_odds: 1.0,
                });
            }
        }

        let hq_threat = self.hq_threat_level(view);
        if hq_threat > 1.0 {
            tasks.push(UnitTask {
                kind: TaskKind::Defend,
                target: view.our_hq,
                building_value: 0.0,
                combat_odds: 0.0,
            });
        }

        tasks.push(UnitTask {
            kind: TaskKind::Advance,
            target: view.enemy_hq,
            building_value: 0.0,
            combat_odds: 0.0,
        });

        tasks
    }

    /// Assign units using commitment-aware scoring.
    fn assign_units(&self, view: &SimpleWarsView) -> Vec<(u16, UnitTask)> {
        let tasks = self.generate_tasks(view);
        let mut assignments = Vec::new();

        for unit in &view.our_units {
            if unit.moved && unit.attacked { continue; }

            let committed = self.committed_plans.iter()
                .find(|p| p.unit_id == unit.id);

            let task = self.resolve_assignment(unit, committed, &tasks, view);
            assignments.push((unit.id, task));
        }

        assignments
    }

    // =========================================================================
    // Tactical execution — same as utility agent
    // =========================================================================

    fn execute_unit(&self, unit: &Unit, task: &UnitTask, view: &SimpleWarsView) -> Option<Command> {
        let uid = unit.id;

        // ALWAYS: capture if on a capturable building
        if unit.unit_type.can_capture() && !unit.attacked {
            let on_capturable = view.buildings.iter()
                .any(|b| b.pos == unit.pos && b.owner != Some(self.player));
            if on_capturable {
                return Some(Command::Capture { unit_id: uid });
            }
        }

        // ALWAYS: attack adjacent enemies
        if !unit.attacked && !unit.unit_type.is_ranged() {
            for enemy in &view.visible_enemy_units {
                if unit.pos.manhattan_distance(enemy.pos) == 1 {
                    return Some(Command::Attack { unit_id: uid, target_pos: enemy.pos });
                }
            }
        }

        // Artillery: attack in range
        if unit.unit_type.is_ranged() && !unit.attacked && !unit.moved {
            for enemy in &view.visible_enemy_units {
                let dist = unit.pos.manhattan_distance(enemy.pos);
                if dist >= unit.unit_type.attack_min_range()
                    && dist <= unit.unit_type.attack_max_range()
                {
                    return Some(Command::Attack { unit_id: uid, target_pos: enemy.pos });
                }
            }
        }

        // Move toward task target
        if !unit.moved {
            let target = task.target;
            let mut candidates: Vec<(u8, Pos)> = Vec::new();
            for (dr, dc) in [(-1i8, 0), (1, 0), (0, -1), (0, 1)] {
                let nr = unit.pos.row as i8 + dr;
                let nc = unit.pos.col as i8 + dc;
                if nr < 0 || nr >= view.rows as i8 || nc < 0 || nc >= view.cols as i8 { continue; }
                let npos = Pos::new(nr as u8, nc as u8);
                if !unit.unit_type.can_enter(view.grid[npos.row as usize][npos.col as usize]) { continue; }
                let occupied = view.our_units.iter().any(|u| u.id != uid && u.pos == npos);
                if occupied { continue; }
                candidates.push((npos.manhattan_distance(target), npos));
            }
            candidates.sort_by_key(|(d, _)| *d);

            if let Some((best_dist, best_pos)) = candidates.first() {
                let current_dist = unit.pos.manhattan_distance(target);
                if *best_dist < current_dist {
                    return Some(Command::Move { unit_id: uid, to: *best_pos });
                }
                if let Some((_, pos)) = candidates.iter().find(|(d, _)| *d <= current_dist) {
                    return Some(Command::Move { unit_id: uid, to: *pos });
                }
            }
        }

        None
    }
}


impl CommittedAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        self.actions_this_turn += 1;
        if self.actions_this_turn > 30 {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }

        // Build if possible
        let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
        if hq_free {
            if let Some(unit_type) = self.desired_build(view) {
                if view.our_gold >= unit_type.cost() {
                    return Command::Build { unit_type };
                }
            }
        }

        // Assign units with commitment-aware scoring
        let assignments = self.assign_units(view);

        // Update commitments based on assignments
        self.update_commitments(&assignments, view);

        // Execute first actionable unit's task
        for (uid, task) in &assignments {
            let unit = match view.our_units.iter().find(|u| u.id == *uid) {
                Some(u) => u,
                None => continue,
            };
            if unit.moved && unit.attacked { continue; }

            if let Some(cmd) = self.execute_unit(unit, task, view) {
                return cmd;
            }
        }

        self.actions_this_turn = 0;
        Command::EndTurn
    }
}

impl GameAgent<SimpleWarsView, Command> for CommittedAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
        self.explored.clear();
        self.map_rows = 0;
        self.map_cols = 0;
        self.last_known_enemies.clear();
        self.committed_plans.clear();
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

        // Prune plans that are no longer valid
        self.prune_invalid_plans(view);
    }

    fn decide(
        &mut self,
        view: &SimpleWarsView,
        tree: &CommandTree<Command>,
    ) -> Option<Command> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
