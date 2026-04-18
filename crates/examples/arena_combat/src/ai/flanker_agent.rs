use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::CombatView;

/// Flanker agent — splits warriors into two groups that attack from
/// opposite sides while archers stay centered and fire.
#[derive(Debug)]
pub struct FlankerAgent {
    name: String,
    player: Player,
}

impl FlankerAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), player: 0 }
    }

    fn is_archer(unit: &Unit) -> bool {
        unit.attack_range > 3.0
    }
}

impl GameAgent<CombatView, Vec<Command>> for FlankerAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &CombatView) {}

    fn decide(&mut self, view: &CombatView, _tree: &CommandTree<Vec<Command>>) -> Option<Vec<Command>> {
        let mut cmds = Vec::new();
        if view.enemy_units.is_empty() { return Some(cmds); }

        // Enemy center
        let en = view.enemy_units.len() as f32;
        let enemy_center = Vec2::new(
            view.enemy_units.iter().map(|u| u.pos.x).sum::<f32>() / en,
            view.enemy_units.iter().map(|u| u.pos.y).sum::<f32>() / en,
        );

        // Flank targets: above and below enemy center
        let flank_high = Vec2::new(enemy_center.x, (enemy_center.y + 6.0).min(view.arena_height - 1.0));
        let flank_low = Vec2::new(enemy_center.x, (enemy_center.y - 6.0).max(1.0));

        let mut warrior_idx = 0;
        for unit in &view.our_units {
            if Self::is_archer(unit) {
                // Archers: shoot weakest in range, hold position
                let target = view.enemy_units.iter()
                    .filter(|e| unit.pos.distance(e.pos) <= unit.attack_range)
                    .min_by(|a, b| a.hp.partial_cmp(&b.hp).unwrap());

                if let Some(enemy) = target {
                    if unit.can_attack() {
                        cmds.push(Command::Attack { unit_id: unit.id, target_id: enemy.id });
                    }
                } else {
                    // Move toward enemy center to get in range
                    cmds.push(Command::Move { unit_id: unit.id, target: enemy_center });
                }
            } else {
                // Warriors: alternate flank direction
                let flank_target = if warrior_idx % 2 == 0 { flank_high } else { flank_low };
                warrior_idx += 1;

                // If near flank position, engage nearest enemy
                let dist_to_flank = unit.pos.distance(flank_target);
                if dist_to_flank < 3.0 {
                    // Arrived at flank, attack nearest
                    let nearest = view.enemy_units.iter()
                        .min_by(|a, b| {
                            unit.pos.distance(a.pos).partial_cmp(&unit.pos.distance(b.pos)).unwrap()
                        });
                    if let Some(enemy) = nearest {
                        if unit.pos.distance(enemy.pos) <= unit.attack_range && unit.can_attack() {
                            cmds.push(Command::Attack { unit_id: unit.id, target_id: enemy.id });
                        }
                        cmds.push(Command::Move { unit_id: unit.id, target: enemy.pos });
                    }
                } else {
                    // Move to flank position
                    cmds.push(Command::Move { unit_id: unit.id, target: flank_target });
                }
            }
        }

        Some(cmds)    }
}
