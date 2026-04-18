use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::SimpleWarsView;

/// Per-unit utility scoring agent.
///
/// Scores every unit-task pair independently using inline utility scoring
/// (task_priority * context_modifiers / distance), assigns the best task
/// to each unit.
///
/// This was the first agent to beat hand-coded (100% on all map sizes).
/// The coordinated agent improves on this by adding capture-spreading,
/// focus fire, and escort patterns via the `Greedy` assignment strategy.
#[derive(Debug)]
pub struct UtilityAgent {
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
}

impl UtilityAgent {
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
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    // =========================================================================
    // Layer 1: GOAP — Evaluate strategic situation, determine build priority
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

    // =========================================================================
    // Layer 2: HTN — For each unit, find the best task via utility scoring
    // =========================================================================

    // ---- Belief evaluations (context for scoring) ----

    fn hq_threat_level(&self, view: &SimpleWarsView) -> f64 {
        let threats: f64 = view.visible_enemy_units.iter()
            .map(|e| {
                let dist = e.pos.manhattan_distance(view.our_hq).max(1) as f64;
                e.unit_type.attack_power() as f64 / dist
            }).sum();
        threats
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

    fn building_value(&self, building: &Building, view: &SimpleWarsView) -> f64 {
        match building.terrain {
            Terrain::HQ => 20.0, // capturing enemy HQ is game-winning
            Terrain::City => {
                // Closer to our side = more valuable (safer to hold)
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
        our_power / their_power // >1 = we're favored
    }

    // ---- Scoring with beliefs ----

    fn score_unit_task(&self, unit: &Unit, task: &UnitTask, view: &SimpleWarsView) -> f64 {
        let dist = unit.pos.manhattan_distance(task.target).max(1) as f64;
        let hq_threat = self.hq_threat_level(view);
        let our_strength = self.our_military_strength(view);
        let economy = self.our_economy(view);

        match task.kind {
            TaskKind::Capture => {
                if !unit.unit_type.can_capture() { return -100.0; }
                let value = task.building_value;
                // Prioritize capture more when economy is low
                let econ_modifier = if economy < 3 { 1.5 } else { 1.0 };
                // Deprioritize if HQ is threatened and unit is near HQ
                let threat_penalty = if hq_threat > 3.0
                    && unit.pos.manhattan_distance(view.our_hq) < 4
                { 0.3 } else { 1.0 };
                value * econ_modifier * threat_penalty / dist
            }
            TaskKind::Attack => {
                let combat_odds = task.combat_odds;
                // Only attack when we're favored
                let aggression = if combat_odds > 1.2 { combat_odds } else { combat_odds * 0.5 };
                // More aggressive when we have strength advantage
                let strength_modifier = if our_strength > self.enemy_visible_strength(view) * 1.3 {
                    1.5
                } else {
                    1.0
                };
                // Prefer tanks for attacking
                let unit_fit = match unit.unit_type {
                    UnitType::Tank => 1.3,
                    UnitType::Infantry => 0.8,
                    UnitType::Artillery => 1.5, // great at range
                    UnitType::Recon => 0.5, // weak, don't send to fight
                };
                4.0 * aggression * strength_modifier * unit_fit / dist
            }
            TaskKind::Advance => {
                // Low priority, gets units moving toward enemy
                let base = 1.5;
                // Higher priority when we have strong army
                let strength_mod = if our_strength > 20.0 { 2.0 } else { 1.0 };
                base * strength_mod / dist
            }
            TaskKind::Defend => {
                // Priority scales with threat level
                let urgency = (hq_threat * 2.0).max(1.0);
                // Prefer strong units for defense
                let unit_fit = unit.unit_type.attack_power() as f64 / 5.0;
                urgency * unit_fit / dist
            }
        }
    }

    /// Generate all possible tasks with pre-computed context.
    fn generate_tasks(&self, view: &SimpleWarsView) -> Vec<UnitTask> {
        let mut tasks = Vec::new();

        // Capture tasks with building values
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

        // Attack tasks with combat odds
        for enemy in &view.visible_enemy_units {
            // Pre-compute average combat odds (will be refined per-unit in scoring)
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

        // Attack from memory
        if view.visible_enemy_units.is_empty() {
            for (pos, _) in &self.last_known_enemies {
                tasks.push(UnitTask {
                    kind: TaskKind::Attack,
                    target: *pos,
                    building_value: 0.0,
                    combat_odds: 1.0, // unknown, assume even
                });
            }
        }

        // Defend HQ if threatened
        let hq_threat = self.hq_threat_level(view);
        if hq_threat > 1.0 {
            tasks.push(UnitTask {
                kind: TaskKind::Defend,
                target: view.our_hq,
                building_value: 0.0,
                combat_odds: 0.0,
            });
        }

        // Advance toward enemy HQ
        tasks.push(UnitTask {
            kind: TaskKind::Advance,
            target: view.enemy_hq,
            building_value: 0.0,
            combat_odds: 0.0,
        });

        tasks
    }

    /// Assign each unit to its best task using utility scoring.
    fn assign_units(&self, view: &SimpleWarsView) -> Vec<(u16, UnitTask)> {
        let tasks = self.generate_tasks(view);
        let mut assignments = Vec::new();

        for unit in &view.our_units {
            if unit.moved && unit.attacked { continue; }

            let mut best_score = f64::NEG_INFINITY;
            let mut best_task = UnitTask { kind: TaskKind::Advance, target: view.enemy_hq, building_value: 0.0, combat_odds: 0.0 };

            for task in &tasks {
                // Infantry-only capture check
                if matches!(task.kind, TaskKind::Capture) && !unit.unit_type.can_capture() {
                    continue;
                }

                let score = self.score_unit_task(unit, task, view);
                if score > best_score {
                    best_score = score;
                    best_task = task.clone();
                }
            }

            assignments.push((unit.id, best_task));
        }

        assignments
    }

    // =========================================================================
    // Layer 3: Tactics — Execute the best command for a unit given its task
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
                // Sidestep to avoid gridlock
                if let Some((_, pos)) = candidates.iter().find(|(d, _)| *d <= current_dist) {
                    return Some(Command::Move { unit_id: uid, to: *pos });
                }
            }
        }

        None
    }
}

#[derive(Debug, Clone)]
struct UnitTask {
    kind: TaskKind,
    target: Pos,
    building_value: f64,
    combat_odds: f64,
}

#[derive(Debug, Clone)]
enum TaskKind {
    Capture,
    Attack,
    Advance,
    Defend,
}


impl UtilityAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        self.actions_this_turn += 1;
        if self.actions_this_turn > 30 {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }

        // Layer 1: Build if possible (GOAP determines what to build)
        let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
        if hq_free {
            if let Some(unit_type) = self.desired_build(view) {
                if view.our_gold >= unit_type.cost() {
                    return Command::Build { unit_type };
                }
            }
        }

        // Layer 2: Assign every unit to its best task (HTN utility scoring)
        let assignments = self.assign_units(view);

        // Layer 3: Execute first actionable unit's task
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

impl GameAgent<SimpleWarsView, Command> for UtilityAgent {
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
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
