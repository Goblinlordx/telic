use telic::arena::{CommandTree, GameAgent, PlayerIndex};

use crate::game::types::*;
use crate::game::state::CombatView;

/// Kite agent — archers maintain distance and shoot, warriors screen.
///
/// Archers retreat if enemies get too close while firing at range.
/// Warriors advance to intercept enemies heading for archers.
#[derive(Debug)]
pub struct KiteAgent {
    name: String,
    player: Player,
}

impl KiteAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), player: 0 }
    }

    fn is_archer(unit: &Unit) -> bool {
        unit.attack_range > 3.0
    }
}

impl GameAgent<CombatView, Vec<Command>> for KiteAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &CombatView) {}

    fn decide(&mut self, view: &CombatView, _tree: &CommandTree<Vec<Command>>) -> Option<Vec<Command>> {
        let mut cmds = Vec::new();
        if view.enemy_units.is_empty() { return Some(cmds); }

        // Find our warrior center (where our screen is)
        let warriors: Vec<&Unit> = view.our_units.iter()
            .filter(|u| !Self::is_archer(u)).collect();
        let warrior_center = if warriors.is_empty() {
            None
        } else {
            let n = warriors.len() as f32;
            Some(Vec2::new(
                warriors.iter().map(|u| u.pos.x).sum::<f32>() / n,
                warriors.iter().map(|u| u.pos.y).sum::<f32>() / n,
            ))
        };

        for unit in &view.our_units {
            if Self::is_archer(unit) {
                // Archer behavior: shoot weakest in-range, kite away from closest
                let in_range: Vec<&Unit> = view.enemy_units.iter()
                    .filter(|e| unit.pos.distance(e.pos) <= unit.attack_range)
                    .collect();

                // Attack weakest in range
                if let Some(target) = in_range.iter()
                    .min_by(|a, b| a.hp.partial_cmp(&b.hp).unwrap())
                {
                    if unit.can_attack() {
                        cmds.push(Command::Attack { unit_id: unit.id, target_id: target.id });
                    }
                }

                // Kite: if any enemy is within 3.0, move away from them
                let closest_enemy = view.enemy_units.iter()
                    .min_by(|a, b| {
                        unit.pos.distance(a.pos).partial_cmp(&unit.pos.distance(b.pos)).unwrap()
                    });

                if let Some(enemy) = closest_enemy {
                    let dist = unit.pos.distance(enemy.pos);
                    if dist < 4.0 {
                        // Run away from the enemy
                        let dx = unit.pos.x - enemy.pos.x;
                        let dy = unit.pos.y - enemy.pos.y;
                        let flee_target = Vec2::new(
                            (unit.pos.x + dx * 3.0).clamp(0.5, view.arena_width - 0.5),
                            (unit.pos.y + dy * 3.0).clamp(0.5, view.arena_height - 0.5),
                        );
                        cmds.push(Command::Move { unit_id: unit.id, target: flee_target });
                    } else if dist > unit.attack_range * 0.8 {
                        // Move closer to get in range but not too close
                        let optimal = unit.pos.toward(enemy.pos, 1.0);
                        cmds.push(Command::Move { unit_id: unit.id, target: optimal });
                    }
                }
            } else {
                // Warrior behavior: intercept enemies heading for our archers
                // Find the enemy closest to our archer line / warrior center
                let screen_target = if let Some(wc) = warrior_center {
                    view.enemy_units.iter()
                        .min_by(|a, b| {
                            let da = wc.distance(a.pos);
                            let db = wc.distance(b.pos);
                            da.partial_cmp(&db).unwrap()
                        })
                } else {
                    // No warrior center, just find nearest
                    view.enemy_units.iter()
                        .min_by(|a, b| {
                            unit.pos.distance(a.pos).partial_cmp(&unit.pos.distance(b.pos)).unwrap()
                        })
                };

                if let Some(enemy) = screen_target {
                    let dist = unit.pos.distance(enemy.pos);
                    if dist <= unit.attack_range && unit.can_attack() {
                        cmds.push(Command::Attack { unit_id: unit.id, target_id: enemy.id });
                    }
                    cmds.push(Command::Move { unit_id: unit.id, target: enemy.pos });
                }
            }
        }

        Some(cmds)    }
}
