use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::CombatView;

/// Rush agent — every unit charges the nearest enemy. No coordination.
/// Simple but effective when you have a numbers advantage.
#[derive(Debug)]
pub struct RushAgent {
    name: String,
    player: Player,
}

impl RushAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), player: 0 }
    }
}

impl GameAgent<CombatView, Vec<Command>> for RushAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &CombatView) {}

    fn decide(&mut self, view: &CombatView, _tree: &CommandTree<Vec<Command>>) -> Option<Vec<Command>> {
        let mut cmds = Vec::new();

        for unit in &view.our_units {
            // Find nearest enemy
            let nearest = view.enemy_units.iter()
                .min_by(|a, b| {
                    let da = unit.pos.distance(a.pos);
                    let db = unit.pos.distance(b.pos);
                    da.partial_cmp(&db).unwrap()
                });

            if let Some(enemy) = nearest {
                let dist = unit.pos.distance(enemy.pos);
                if dist <= unit.attack_range && unit.can_attack() {
                    cmds.push(Command::Attack { unit_id: unit.id, target_id: enemy.id });
                }
                cmds.push(Command::Move { unit_id: unit.id, target: enemy.pos });
            }
        }

        Some(cmds)    }
}
