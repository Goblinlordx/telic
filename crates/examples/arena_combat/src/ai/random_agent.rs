use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::CombatView;

#[derive(Debug)]
pub struct RandomAgent {
    name: String,
    player: Player,
    seed: u64,
}

impl RandomAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self { name: name.into(), player: 0, seed: seed.max(1) }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }
}

impl GameAgent<CombatView, Vec<Command>> for RandomAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &CombatView) {}

    fn decide(&mut self, view: &CombatView, _tree: &CommandTree<Vec<Command>>) -> Option<Vec<Command>> {
        let mut cmds = Vec::new();

        for unit in &view.our_units {
            if view.enemy_units.is_empty() { break; }

            let target_idx = self.xorshift() as usize % view.enemy_units.len();
            let enemy = &view.enemy_units[target_idx];

            let dist = unit.pos.distance(enemy.pos);
            if dist <= unit.attack_range && unit.can_attack() {
                cmds.push(Command::Attack { unit_id: unit.id, target_id: enemy.id });
            } else {
                cmds.push(Command::Move { unit_id: unit.id, target: enemy.pos });
            }
        }

        Some(cmds)    }
}
