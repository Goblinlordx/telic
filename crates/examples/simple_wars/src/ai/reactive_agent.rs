use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SimpleWarsView;


/// Reactive baseline — hardcoded to always attack if an enemy is in range
/// and always capture if standing on a capturable building. Falls back to
/// random movement and occasional infantry builds when no reactive play is
/// available.
///
/// This is a strong baseline, not a random agent — the attack/capture
/// reflexes are the two highest-value plays in SimpleWars.
#[derive(Debug)]
pub struct ReactiveAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    actions_this_turn: u32,
}

impl ReactiveAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self { name: name.into(), player: 0, seed: seed.max(1), actions_this_turn: 0 }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }
}


impl ReactiveAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        self.actions_this_turn += 1;

        // End turn after too many actions to prevent infinite loops
        if self.actions_this_turn > 20 {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }

        // Try to move/attack with unmoved units
        let unmoved: Vec<&Unit> = view.our_units.iter()
            .filter(|u| !u.moved && !u.attacked)
            .collect();

        if !unmoved.is_empty() {
            let unit = unmoved[self.xorshift() as usize % unmoved.len()];
            let uid = unit.id;

            // Check if enemy adjacent — attack
            for enemy in &view.visible_enemy_units {
                let dist = unit.pos.manhattan_distance(enemy.pos);
                if dist >= unit.unit_type.attack_min_range()
                    && dist <= unit.unit_type.attack_max_range()
                {
                    return Command::Attack { unit_id: uid, target_pos: enemy.pos };
                }
            }

            // Try to move toward enemy HQ or a random direction
            // Just pick a random adjacent valid cell
            let directions = [(-1i8, 0), (1, 0), (0, -1), (0, 1)];
            let dir = directions[self.xorshift() as usize % 4];
            let nr = unit.pos.row as i8 + dir.0;
            let nc = unit.pos.col as i8 + dir.1;
            if nr >= 0 && nr < view.rows as i8 && nc >= 0 && nc < view.cols as i8 {
                let to = Pos::new(nr as u8, nc as u8);
                if unit.unit_type.can_enter(view.grid[to.row as usize][to.col as usize]) {
                    return Command::Move { unit_id: uid, to };
                }
            }
        }

        // Try to capture if infantry on capturable building
        for unit in &view.our_units {
            if unit.unit_type.can_capture() && !unit.attacked {
                let on_building = view.buildings.iter()
                    .any(|b| b.pos == unit.pos && b.owner != Some(self.player));
                if on_building {
                    return Command::Capture { unit_id: unit.id };
                }
            }
        }

        // Try to build
        if self.xorshift() % 3 == 0 && view.our_gold >= UnitType::Infantry.cost() {
            return Command::Build { unit_type: UnitType::Infantry };
        }

        self.actions_this_turn = 0;
        Command::EndTurn
    }
}

impl GameAgent<SimpleWarsView, Command> for ReactiveAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
    }
    fn observe(&mut self, _view: &SimpleWarsView) {}

    fn decide(
        &mut self,
        _view: &SimpleWarsView,
        tree: &CommandTree<Command>,
    ) -> Option<Command> {
        // Priority: attack > capture > build > move > end_turn.
        // Pull each sub-layer by key; first non-empty one wins.
        if let Some(attack) = tree.child("attack") {
            if let Some(c) = first_leaf(attack, self) { return Some(c); }
        }
        if let Some(capture) = tree.child("capture") {
            if let Some(c) = first_leaf(capture, self) { return Some(c); }
        }
        // Build only ~1/3 of the time to leave room for other plays.
        if self.xorshift() % 3 == 0 {
            if let Some(build) = tree.child("build") {
                if let Some(c) = first_leaf(build, self) { return Some(c); }
            }
        }
        if let Some(moves) = tree.child("move") {
            if let Some(c) = first_leaf(moves, self) { return Some(c); }
        }
        // Fallback: EndTurn (always present when it's our turn).
        tree.find_leaf(|c| matches!(c, Command::EndTurn)).cloned()
    }

}

/// Pick a random leaf from a subtree. Returns `None` if the subtree has
/// no discrete leaves (e.g. purely parametric or empty).
fn first_leaf(tree: &CommandTree<Command>, agent: &mut ReactiveAgent) -> Option<Command> {
    let leaves = tree.flatten();
    if leaves.is_empty() { return None; }
    let i = (agent.xorshift() as usize) % leaves.len();
    Some(leaves[i].clone())
}
