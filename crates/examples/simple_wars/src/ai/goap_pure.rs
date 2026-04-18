use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::belief::{BeliefBuilder, BeliefSet};
use telic::planning::goal::GoalBuilder;
use telic::planning::planner::{GoapPlanner, SearchStrategy, MinCostHeuristic};

use crate::game::types::*;
use crate::game::state::SimpleWarsView;


/// Pure GOAP agent — uses the planning framework to select goals,
/// then executes commands that advance toward the selected goal.
///
/// Beliefs are re-evaluated every turn from the game view.
/// The planner picks the highest-priority achievable goal.
/// The agent then issues commands that match the first action in the plan.
#[derive(Debug)]
pub struct GoapPureAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    actions_this_turn: u32,
    /// Current plan: which "mode" we're in
    current_mode: Mode,
}

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Idle,
    Build,
    Expand,    // capture neutral cities
    Attack,    // push toward enemy
    Defend,    // protect our territory
}

impl GoapPureAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            actions_this_turn: 0,
            current_mode: Mode::Idle,
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// Evaluate beliefs from the current game view and use GOAP to pick a mode.
    fn plan_mode(&self, view: &SimpleWarsView) -> Mode {
        let our_units = view.our_units.len();
        let our_infantry = view.our_units.iter().filter(|u| u.unit_type == UnitType::Infantry).count();
        let our_buildings = view.buildings.iter().filter(|b| b.owner == Some(self.player)).count();
        let enemy_visible = !view.visible_enemy_units.is_empty();
        let neutral_cities = view.buildings.iter()
            .filter(|b| b.terrain == Terrain::City && b.owner.is_none())
            .count();
        let enemy_near_hq = view.visible_enemy_units.iter()
            .any(|u| u.pos.manhattan_distance(view.our_hq) <= 3);

        // Build beliefs
        let mut beliefs = BeliefSet::new();

        let need_units = our_units < 4;
        let have_army = our_units >= 3;
        let cities_available = neutral_cities > 0;
        let have_infantry_for_capture = our_infantry >= 2;
        let threat_detected = enemy_near_hq;
        let have_economy = our_buildings >= 3;

        beliefs.add(BeliefBuilder::new("need_units").condition(move |_: &()| need_units).build());
        beliefs.add(BeliefBuilder::new("have_army").condition(move |_: &()| have_army).build());
        beliefs.add(BeliefBuilder::new("cities_available").condition(move |_: &()| cities_available).build());
        beliefs.add(BeliefBuilder::new("have_capturers").condition(move |_: &()| have_infantry_for_capture).build());
        beliefs.add(BeliefBuilder::new("threat_detected").condition(move |_: &()| threat_detected).build());
        beliefs.add(BeliefBuilder::new("have_economy").condition(move |_: &()| have_economy).build());
        beliefs.add(BeliefBuilder::new("enemy_visible").condition(move |_: &()| enemy_visible).build());
        // Terminal beliefs (goals we want to achieve)
        beliefs.add(BeliefBuilder::new("units_built").condition(|_: &()| false).build());
        beliefs.add(BeliefBuilder::new("territory_expanded").condition(|_: &()| false).build());
        beliefs.add(BeliefBuilder::new("enemy_engaged").condition(|_: &()| false).build());
        beliefs.add(BeliefBuilder::new("hq_defended").condition(|_: &()| false).build());

        // Define actions (abstract — these map to modes, not game commands)
        use telic::planning::action::{ActionBuilder, Strategy};

        #[derive(Debug)]
        struct NoopStrategy;
        impl Strategy for NoopStrategy {
            fn is_complete(&self) -> bool { true }
        }

        let actions = vec![
            ActionBuilder::new("do_build")
                .precondition("need_units")
                .effect("units_built")
                .cost(1.0)
                .strategy(NoopStrategy)
                .build(),
            ActionBuilder::new("do_expand")
                .precondition("have_capturers")
                .precondition("cities_available")
                .effect("territory_expanded")
                .cost(2.0)
                .strategy(NoopStrategy)
                .build(),
            ActionBuilder::new("do_attack")
                .precondition("have_army")
                .precondition("have_economy")
                .effect("enemy_engaged")
                .cost(3.0)
                .strategy(NoopStrategy)
                .build(),
            ActionBuilder::new("do_defend")
                .precondition("threat_detected")
                .effect("hq_defended")
                .cost(1.0)
                .strategy(NoopStrategy)
                .build(),
        ];

        // Define goals with priorities
        let goals = vec![
            GoalBuilder::new("defend_hq")
                .priority(4) // highest — protect HQ above all
                .desired_effect("hq_defended")
                .build(),
            GoalBuilder::new("grow_army")
                .priority(3)
                .desired_effect("units_built")
                .build(),
            GoalBuilder::new("capture_cities")
                .priority(2)
                .desired_effect("territory_expanded")
                .build(),
            GoalBuilder::new("push_enemy")
                .priority(1)
                .desired_effect("enemy_engaged")
                .build(),
        ];

        // Run the planner
        let plan = GoapPlanner::plan_with(
            &beliefs, &(), &actions, &goals, None,
            SearchStrategy::Astar, &MinCostHeuristic,
        );

        match plan {
            Some(p) => {
                match p.actions.first().map(|s| s.as_str()) {
                    Some("do_defend") => Mode::Defend,
                    Some("do_build") => Mode::Build,
                    Some("do_expand") => Mode::Expand,
                    Some("do_attack") => Mode::Attack,
                    _ => Mode::Expand, // default
                }
            }
            None => Mode::Build, // fallback
        }
    }

    fn step_toward(from: Pos, to: Pos, rows: u8, cols: u8) -> Option<Pos> {
        let mut candidates: Vec<(u8, Pos)> = Vec::new();
        for (dr, dc) in [(-1i8, 0), (1, 0), (0, -1), (0, 1)] {
            let nr = from.row as i8 + dr;
            let nc = from.col as i8 + dc;
            if nr >= 0 && nr < rows as i8 && nc >= 0 && nc < cols as i8 {
                let npos = Pos::new(nr as u8, nc as u8);
                candidates.push((npos.manhattan_distance(to), npos));
            }
        }
        candidates.sort_by_key(|(d, _)| *d);
        candidates.first().map(|(_, p)| *p)
    }

    fn execute_mode(&mut self, mode: &Mode, view: &SimpleWarsView) -> Command {
        match mode {
            Mode::Build => {
                let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
                if hq_free && view.our_gold >= UnitType::Infantry.cost() {
                    let our_infantry = view.our_units.iter()
                        .filter(|u| u.unit_type == UnitType::Infantry).count();
                    let our_tanks = view.our_units.iter()
                        .filter(|u| u.unit_type == UnitType::Tank).count();
                    if our_infantry >= 3 && our_tanks < 2 && view.our_gold >= UnitType::Tank.cost() {
                        return Command::Build { unit_type: UnitType::Tank };
                    }
                    return Command::Build { unit_type: UnitType::Infantry };
                }
                // If can't build, move units off HQ then fall through to expand
                self.execute_unit_commands(view, &view.enemy_hq)
            }
            Mode::Expand => {
                // Find nearest neutral city and send infantry there
                let target = view.buildings.iter()
                    .filter(|b| b.terrain == Terrain::City && b.owner.is_none())
                    .min_by_key(|b| {
                        view.our_units.iter()
                            .filter(|u| u.unit_type == UnitType::Infantry)
                            .map(|u| u.pos.manhattan_distance(b.pos))
                            .min().unwrap_or(99)
                    })
                    .map(|b| b.pos)
                    .unwrap_or(view.enemy_hq);
                self.execute_unit_commands(view, &target)
            }
            Mode::Attack => {
                self.execute_unit_commands(view, &view.enemy_hq)
            }
            Mode::Defend => {
                self.execute_unit_commands(view, &view.our_hq)
            }
            Mode::Idle => Command::EndTurn,
        }
    }

    fn execute_unit_commands(&mut self, view: &SimpleWarsView, target: &Pos) -> Command {
        // Capture if possible
        for unit in &view.our_units {
            if unit.unit_type.can_capture() && !unit.attacked {
                let on_capturable = view.buildings.iter()
                    .any(|b| b.pos == unit.pos && b.owner != Some(self.player));
                if on_capturable {
                    return Command::Capture { unit_id: unit.id };
                }
            }
        }

        // Attack adjacent enemies
        for unit in &view.our_units {
            if unit.attacked { continue; }
            for enemy in &view.visible_enemy_units {
                let dist = unit.pos.manhattan_distance(enemy.pos);
                if dist >= unit.unit_type.attack_min_range()
                    && dist <= unit.unit_type.attack_max_range()
                    && !(unit.unit_type.is_ranged() && unit.moved)
                {
                    return Command::Attack { unit_id: unit.id, target_pos: enemy.pos };
                }
            }
        }

        // Move unmoved units toward target
        for unit in &view.our_units {
            if unit.moved { continue; }
            if let Some(step) = Self::step_toward(unit.pos, *target, view.rows, view.cols) {
                if unit.unit_type.can_enter(view.grid[step.row as usize][step.col as usize]) {
                    let occupied = view.our_units.iter().any(|u| u.id != unit.id && u.pos == step);
                    if !occupied {
                        return Command::Move { unit_id: unit.id, to: step };
                    }
                }
            }
        }

        Command::EndTurn
    }
}


impl GoapPureAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        self.actions_this_turn += 1;
        if self.actions_this_turn > 30 {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }

        // Re-evaluate mode at start of each turn
        if self.actions_this_turn == 1 {
            self.current_mode = self.plan_mode(view);
        }

        let mode = self.current_mode.clone();
        let cmd = self.execute_mode(&mode, view);

        if cmd == Command::EndTurn {
            self.actions_this_turn = 0;
        }

        cmd
    }
}

impl GameAgent<SimpleWarsView, Command> for GoapPureAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
        self.current_mode = Mode::Idle;
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
