use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::CombatView;

/// Focus fire agent — ALL units attack the same target (lowest HP enemy).
/// Maximizes kill speed at the cost of positioning.
#[derive(Debug)]
pub struct FocusFireAgent {
    name: String,
    player: Player,
}

impl FocusFireAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), player: 0 }
    }
}

impl GameAgent<CombatView, Vec<Command>> for FocusFireAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &CombatView) {}

    fn decide(&mut self, view: &CombatView, _tree: &CommandTree<Vec<Command>>) -> Option<Vec<Command>> {
        let mut cmds = Vec::new();
        if view.enemy_units.is_empty() { return Some(cmds); }

        // Pick the global focus target: lowest HP enemy
        let focus_target = view.enemy_units.iter()
            .min_by(|a, b| a.hp.partial_cmp(&b.hp).unwrap())
            .unwrap();

        for unit in &view.our_units {
            let dist = unit.pos.distance(focus_target.pos);
            if dist <= unit.attack_range && unit.can_attack() {
                cmds.push(Command::Attack { unit_id: unit.id, target_id: focus_target.id });
            }
            cmds.push(Command::Move { unit_id: unit.id, target: focus_target.pos });
        }

        Some(cmds)    }
}
