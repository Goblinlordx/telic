use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SimpleWarsView;


/// Hand-coded agent with prioritized goals (no AI framework):
/// 1. Build units (economy into military)
/// 2. Capture nearby neutral cities (expand income)
/// 3. Attack visible enemies (especially weak ones)
/// 4. Advance toward enemy HQ
/// 5. Scout with recon if available
#[derive(Debug)]
pub struct HandCodedAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    actions_this_turn: u32,
}

impl HandCodedAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self { name: name.into(), player: 0, seed: seed.max(1), actions_this_turn: 0 }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn step_toward(from: Pos, to: Pos, rows: u8, cols: u8) -> Pos {
        let dr = (to.row as i8 - from.row as i8).signum();
        let dc = (to.col as i8 - from.col as i8).signum();
        if from.row.abs_diff(to.row) >= from.col.abs_diff(to.col) {
            let nr = (from.row as i8 + dr) as u8;
            if nr < rows { Pos::new(nr, from.col) } else { from }
        } else {
            let nc = (from.col as i8 + dc) as u8;
            if nc < cols { Pos::new(from.row, nc) } else { from }
        }
    }

    fn nearest_neutral_city(&self, from: Pos, view: &SimpleWarsView) -> Option<Pos> {
        view.buildings.iter()
            .filter(|b| b.terrain == Terrain::City && b.owner.is_none())
            .min_by_key(|b| from.manhattan_distance(b.pos))
            .map(|b| b.pos)
    }

    fn nearest_enemy_city(&self, from: Pos, view: &SimpleWarsView) -> Option<Pos> {
        let enemy = 1 - self.player;
        view.buildings.iter()
            .filter(|b| b.owner == Some(enemy))
            .min_by_key(|b| from.manhattan_distance(b.pos))
            .map(|b| b.pos)
    }

    fn weakest_visible_enemy<'a>(&self, from: Pos, view: &'a SimpleWarsView) -> Option<&'a Unit> {
        view.visible_enemy_units.iter()
            .min_by_key(|u| (u.hp, from.manhattan_distance(u.pos)))
    }

    fn decide_unit_action(&mut self, unit: &Unit, view: &SimpleWarsView) -> Option<Command> {
        let uid = unit.id;

        // 1. If on a capturable building, capture it
        if unit.unit_type.can_capture() && !unit.attacked {
            let on_capturable = view.buildings.iter()
                .any(|b| b.pos == unit.pos && b.owner != Some(self.player));
            if on_capturable {
                return Some(Command::Capture { unit_id: uid });
            }
        }

        // 2. If enemy adjacent and we can attack, attack
        if !unit.attacked && !unit.unit_type.is_ranged() {
            for enemy in &view.visible_enemy_units {
                let dist = unit.pos.manhattan_distance(enemy.pos);
                if dist == 1 {
                    return Some(Command::Attack { unit_id: uid, target_pos: enemy.pos });
                }
            }
        }

        // 3. Artillery: attack in range without moving
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

        // 4. Move toward a goal
        if !unit.moved {
            let target = if unit.unit_type.can_capture() {
                // Infantry: go capture nearest neutral city, or enemy city
                self.nearest_neutral_city(unit.pos, view)
                    .or_else(|| self.nearest_enemy_city(unit.pos, view))
                    .unwrap_or(view.enemy_hq)
            } else if unit.unit_type == UnitType::Recon {
                // Recon: scout toward enemy side
                view.enemy_hq
            } else {
                // Tanks/artillery: move toward nearest visible enemy, or enemy HQ
                self.weakest_visible_enemy(unit.pos, view)
                    .map(|e| e.pos)
                    .unwrap_or(view.enemy_hq)
            };

            // Try all 4 directions, pick the one that gets closest to target
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
            candidates.sort_by_key(|(dist, _)| *dist);
            if let Some((_, best_pos)) = candidates.first() {
                if best_pos.manhattan_distance(target) < unit.pos.manhattan_distance(target) {
                    return Some(Command::Move { unit_id: uid, to: *best_pos });
                }
                // If no direction gets closer, still move if we can (avoid gridlock)
                if !candidates.is_empty() {
                    // Pick one that's at least not further
                    let sidestep = candidates.iter()
                        .find(|(d, _)| *d <= unit.pos.manhattan_distance(target));
                    if let Some((_, pos)) = sidestep {
                        return Some(Command::Move { unit_id: uid, to: *pos });
                    }
                }
            }
        }

        None
    }
}


impl HandCodedAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        self.actions_this_turn += 1;
        if self.actions_this_turn > 30 {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }

        // Check if HQ is free before building
        let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
        let our_unit_count = view.our_units.len();
        let our_infantry = view.our_units.iter().filter(|u| u.unit_type == UnitType::Infantry).count();
        let our_tanks = view.our_units.iter().filter(|u| u.unit_type == UnitType::Tank).count();

        if hq_free {
            if our_infantry < 3 && view.our_gold >= UnitType::Infantry.cost() {
                return Command::Build { unit_type: UnitType::Infantry };
            }
            if our_tanks < 2 && our_infantry >= 2 && view.our_gold >= UnitType::Tank.cost() {
                return Command::Build { unit_type: UnitType::Tank };
            }
            if our_unit_count < 6 && view.our_gold >= UnitType::Infantry.cost() {
                return Command::Build { unit_type: UnitType::Infantry };
            }
        }

        // Issue commands to units that haven't acted yet
        // Prioritize: capturing units, then attacking units, then moving units
        let actionable: Vec<Unit> = view.our_units.iter()
            .filter(|u| !u.moved || !u.attacked)
            .cloned()
            .collect();

        for unit in &actionable {
            if let Some(cmd) = self.decide_unit_action(unit, view) {
                return cmd;
            }
        }

        self.actions_this_turn = 0;
        Command::EndTurn
    }
}

impl GameAgent<SimpleWarsView, Command> for HandCodedAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
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
