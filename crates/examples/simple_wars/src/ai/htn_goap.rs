use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::belief::{BeliefBuilder, BeliefSet};
use telic::planning::htn::{Task, TaskBuilder, MethodBuilder};

use crate::game::types::*;
use crate::game::state::SimpleWarsView;

/// HTN agent — demonstrates the framework's `Task<S>::decompose()`.
///
/// Uses HTN to structure turns into phases based on conditions:
///   "play_turn"
///     ├─ if threat_near_hq → [defend, produce]
///     ├─ if need_units     → [produce, expand]
///     ├─ if have_army      → [produce, combat, expand, advance]
///     └─ default           → [produce, expand]
///
/// Each phase executes commands for ALL relevant units before advancing
/// to the next phase.
///
/// NOTE: HTN's advantage is performance (no search), its disadvantage
/// is rigidity (hand-authored structure). HTN is useful when the task
/// structure is well-known and search would be wasteful.
#[derive(Debug)]
pub struct HtnAgent {
    name: String,
    player: PlayerIndex,
    #[allow(dead_code)]
    seed: u64,
    actions_this_turn: u32,
    phases: Vec<String>,
    phase_idx: usize,
    /// Track which units have been handled in the current phase
    units_handled: Vec<u16>,
}

impl HtnAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            actions_this_turn: 0,
            phases: Vec::new(),
            phase_idx: 0,
            units_handled: Vec::new(),
        }
    }

    /// Use the framework's HTN decomposition to plan the turn structure.
    fn plan_turn(&self, view: &SimpleWarsView) -> Vec<String> {
        let our_units = view.our_units.len();
        let enemy_near = view.visible_enemy_units.iter()
            .any(|u| u.pos.manhattan_distance(view.our_hq) <= 3);
        let have_economy = view.buildings.iter()
            .filter(|b| b.owner == Some(self.player)).count() >= 3;

        // Beliefs for HTN method conditions
        let mut beliefs = BeliefSet::<()>::new();
        beliefs.add(BeliefBuilder::new("threat_near_hq")
            .condition(move |_: &()| enemy_near).build());
        beliefs.add(BeliefBuilder::new("need_units")
            .condition(move |_: &()| our_units < 4).build());
        beliefs.add(BeliefBuilder::new("have_army")
            .condition(move |_: &()| our_units >= 3 && have_economy).build());

        // HTN tasks — framework's Task<S>::decompose() does the work
        let tasks: Vec<Task<()>> = vec![
            TaskBuilder::compound("play_turn", vec![
                MethodBuilder::new("emergency")
                    .condition("threat_near_hq")
                    .subtask("phase_defend")
                    .subtask("phase_produce")
                    .build(),
                MethodBuilder::new("early_game")
                    .condition("need_units")
                    .subtask("phase_produce")
                    .subtask("phase_expand")
                    .build(),
                MethodBuilder::new("mid_game")
                    .condition("have_army")
                    .subtask("phase_produce")
                    .subtask("phase_combat")
                    .subtask("phase_expand")
                    .subtask("phase_advance")
                    .build(),
                MethodBuilder::new("default")
                    .subtask("phase_produce")
                    .subtask("phase_expand")
                    .build(),
            ]),
            TaskBuilder::primitive("phase_defend", "defend"),
            TaskBuilder::primitive("phase_produce", "produce"),
            TaskBuilder::primitive("phase_expand", "expand"),
            TaskBuilder::primitive("phase_combat", "combat"),
            TaskBuilder::primitive("phase_advance", "advance"),
        ];

        let actions = vec![];
        let root = tasks.iter().find(|t| t.name() == "play_turn").unwrap();
        root.decompose(&beliefs, &(), &actions, &tasks)
            .unwrap_or_else(|| vec!["produce".into(), "expand".into()])
    }

    /// Execute one command for the current phase.
    /// Returns Some(command) if there's something to do, None if phase is exhausted.
    fn try_phase(&mut self, phase: &str, view: &SimpleWarsView) -> Option<Command> {
        match phase {
            "produce" => self.try_produce(view),
            "defend" => self.try_defend(view),
            "expand" => self.try_expand(view),
            "combat" => self.try_combat(view),
            "advance" => self.try_advance(view),
            _ => None,
        }
    }

    fn try_produce(&mut self, view: &SimpleWarsView) -> Option<Command> {
        if self.units_handled.contains(&u16::MAX) { return None; } // already produced

        let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
        if !hq_free {
            self.units_handled.push(u16::MAX);
            return None;
        }

        let infantry = view.our_units.iter().filter(|u| u.unit_type == UnitType::Infantry).count();
        let total = view.our_units.len();

        let unit_type = if total >= 6 {
            self.units_handled.push(u16::MAX);
            return None;
        } else if infantry < 3 && view.our_gold >= UnitType::Infantry.cost() {
            UnitType::Infantry
        } else if view.our_gold >= UnitType::Tank.cost() {
            UnitType::Tank
        } else if view.our_gold >= UnitType::Infantry.cost() {
            UnitType::Infantry
        } else {
            self.units_handled.push(u16::MAX);
            return None;
        };

        self.units_handled.push(u16::MAX);
        Some(Command::Build { unit_type })
    }

    fn try_defend(&mut self, view: &SimpleWarsView) -> Option<Command> {
        // Attack enemies near HQ
        for unit in &view.our_units {
            if self.units_handled.contains(&unit.id) { continue; }
            if unit.attacked { continue; }
            for enemy in &view.visible_enemy_units {
                if enemy.pos.manhattan_distance(view.our_hq) <= 3 {
                    let dist = unit.pos.manhattan_distance(enemy.pos);
                    if dist >= unit.unit_type.attack_min_range()
                        && dist <= unit.unit_type.attack_max_range()
                    {
                        self.units_handled.push(unit.id);
                        return Some(Command::Attack { unit_id: unit.id, target_pos: enemy.pos });
                    }
                }
            }
        }

        // Move toward HQ
        for unit in &view.our_units {
            if self.units_handled.contains(&unit.id) { continue; }
            if unit.moved { continue; }
            if unit.pos.manhattan_distance(view.our_hq) > 2 {
                if let Some(cmd) = self.try_move_toward(view, unit, view.our_hq) {
                    self.units_handled.push(unit.id);
                    return Some(cmd);
                }
            }
        }

        None
    }

    fn try_expand(&mut self, view: &SimpleWarsView) -> Option<Command> {
        // Capture if on a capturable building
        for unit in &view.our_units {
            if self.units_handled.contains(&unit.id) { continue; }
            if !unit.unit_type.can_capture() || unit.attacked { continue; }
            let on_capturable = view.buildings.iter()
                .any(|b| b.pos == unit.pos && b.owner != Some(self.player));
            if on_capturable {
                self.units_handled.push(unit.id);
                return Some(Command::Capture { unit_id: unit.id });
            }
        }

        // Move infantry toward nearest uncaptured city
        let target = view.buildings.iter()
            .filter(|b| (b.terrain == Terrain::City || b.terrain == Terrain::HQ)
                && b.owner != Some(self.player))
            .min_by_key(|b| {
                view.our_units.iter()
                    .filter(|u| u.unit_type.can_capture() && !u.moved
                        && !self.units_handled.contains(&u.id))
                    .map(|u| u.pos.manhattan_distance(b.pos))
                    .min().unwrap_or(99)
            })
            .map(|b| b.pos);

        if let Some(target) = target {
            for unit in &view.our_units {
                if self.units_handled.contains(&unit.id) { continue; }
                if !unit.unit_type.can_capture() || unit.moved { continue; }
                if let Some(cmd) = self.try_move_toward(view, unit, target) {
                    self.units_handled.push(unit.id);
                    return Some(cmd);
                }
            }
        }

        None
    }

    fn try_combat(&mut self, view: &SimpleWarsView) -> Option<Command> {
        for unit in &view.our_units {
            if self.units_handled.contains(&unit.id) { continue; }
            if unit.attacked { continue; }
            for enemy in &view.visible_enemy_units {
                let dist = unit.pos.manhattan_distance(enemy.pos);
                if dist >= unit.unit_type.attack_min_range()
                    && dist <= unit.unit_type.attack_max_range()
                    && !(unit.unit_type.is_ranged() && unit.moved)
                {
                    self.units_handled.push(unit.id);
                    return Some(Command::Attack { unit_id: unit.id, target_pos: enemy.pos });
                }
            }
        }
        None
    }

    fn try_advance(&mut self, view: &SimpleWarsView) -> Option<Command> {
        for unit in &view.our_units {
            if self.units_handled.contains(&unit.id) { continue; }
            if unit.moved { continue; }
            if let Some(cmd) = self.try_move_toward(view, unit, view.enemy_hq) {
                self.units_handled.push(unit.id);
                return Some(cmd);
            }
        }
        None
    }

    fn try_move_toward(&self, view: &SimpleWarsView, unit: &Unit, target: Pos) -> Option<Command> {
        let mut candidates: Vec<(u8, Pos)> = Vec::new();
        for (dr, dc) in [(-1i8, 0), (1, 0), (0, -1), (0, 1)] {
            let nr = unit.pos.row as i8 + dr;
            let nc = unit.pos.col as i8 + dc;
            if nr < 0 || nr >= view.rows as i8 || nc < 0 || nc >= view.cols as i8 { continue; }
            let npos = Pos::new(nr as u8, nc as u8);
            if !unit.unit_type.can_enter(view.grid[npos.row as usize][npos.col as usize]) { continue; }
            if view.our_units.iter().any(|u| u.id != unit.id && u.pos == npos) { continue; }
            candidates.push((npos.manhattan_distance(target), npos));
        }
        candidates.sort_by_key(|(d, _)| *d);

        if let Some((best_dist, best_pos)) = candidates.first() {
            if *best_dist <= unit.pos.manhattan_distance(target) {
                return Some(Command::Move { unit_id: unit.id, to: *best_pos });
            }
        }
        None
    }
}


impl HtnAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        self.actions_this_turn += 1;
        if self.actions_this_turn > 30 {
            self.actions_this_turn = 0;
            self.phases.clear();
            self.phase_idx = 0;
            self.units_handled.clear();
            return Command::EndTurn;
        }

        // Plan phases at start of turn via HTN decomposition
        if self.phases.is_empty() {
            self.phases = self.plan_turn(view);
            self.phase_idx = 0;
            self.units_handled.clear();
        }

        // Try current phase, advance when exhausted
        while self.phase_idx < self.phases.len() {
            let phase = self.phases[self.phase_idx].clone();
            if let Some(cmd) = self.try_phase(&phase, view) {
                return cmd;
            }
            // Phase exhausted — advance to next
            self.phase_idx += 1;
            self.units_handled.clear(); // reset for next phase
        }

        // All phases done
        self.actions_this_turn = 0;
        self.phases.clear();
        self.phase_idx = 0;
        self.units_handled.clear();
        Command::EndTurn
    }
}

impl GameAgent<SimpleWarsView, Command> for HtnAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
        self.phases.clear();
        self.phase_idx = 0;
        self.units_handled.clear();
    }

    fn observe(&mut self, _view: &SimpleWarsView) {}

    fn decide(
        &mut self,
        view: &SimpleWarsView,
        tree: &CommandTree<Command>,
    ) -> Option<Command> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
