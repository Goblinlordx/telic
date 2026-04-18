use std::collections::HashMap;
use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::utility::{
    UtilityAction, ResponseCurve, ScoringMode, Greedy, AssignmentStrategy,
};

use crate::game::types::*;
use crate::game::state::CombatView;

/// Coordinated utility agent for real-time arena combat.
///
/// Uses memory to track enemy movement and predict positions,
/// enabling interception instead of direct pursuit (counters kiting).
#[derive(Debug)]
pub struct UtilityAgent {
    name: String,
    player: Player,
    /// Track previous enemy positions for velocity estimation
    prev_enemy_pos: HashMap<u16, Vec2>,
    /// Estimated enemy velocities (units per second)
    enemy_velocity: HashMap<u16, Vec2>,
    /// Previous elapsed time for computing dt
    prev_elapsed: f32,
}

struct ScoringCtx {
    unit_pos: Vec2,
    unit_range: f64,
    unit_damage: f64,
    unit_is_melee: bool,
    target_pos: Vec2,
    target_predicted_pos: Vec2, // where target will be
    target_hp: f64,
    target_max_hp: f64,
    target_damage: f64,
    target_is_fleeing: bool, // moving away from us
    our_avg_pos: Vec2,
}

impl UtilityAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            player: 0,
            prev_enemy_pos: HashMap::new(),
            enemy_velocity: HashMap::new(),
            prev_elapsed: 0.0,
        }
    }

    /// Predict where a target will be in `lookahead` seconds.
    fn predict_pos(&self, enemy_id: u16, current_pos: Vec2, lookahead: f32) -> Vec2 {
        if let Some(vel) = self.enemy_velocity.get(&enemy_id) {
            Vec2::new(
                (current_pos.x + vel.x * lookahead).clamp(0.0, 20.0),
                (current_pos.y + vel.y * lookahead).clamp(0.0, 20.0),
            )
        } else {
            current_pos
        }
    }

    /// Compute intercept point: where should we move to catch a moving target?
    fn intercept_point(&self, unit: &Unit, enemy: &Unit) -> Vec2 {
        let dist = unit.pos.distance(enemy.pos);
        if dist < 0.1 { return enemy.pos; }

        // Time to reach target at our speed
        let time_to_reach = dist / unit.speed;
        // Predict where target will be
        let predicted = self.predict_pos(enemy.id, enemy.pos, time_to_reach * 0.7);
        predicted
    }

    fn build_scorer() -> UtilityAction<ScoringCtx> {
        UtilityAction::new("target_score")
            .with_base(1.0)
            .with_mode(ScoringMode::Additive)
            // Prefer low-HP targets (focus fire to get kills)
            .consider("low_hp",
                |ctx: &ScoringCtx| {
                    let hp_frac = ctx.target_hp / ctx.target_max_hp;
                    (1.0 - hp_frac) * 15.0
                },
                ResponseCurve::Identity, 1.0)
            // Prefer targets in range
            .consider("in_range",
                |ctx: &ScoringCtx| {
                    let dist = ctx.unit_pos.distance(ctx.target_pos) as f64;
                    if dist <= ctx.unit_range { 10.0 } else { 0.0 }
                },
                ResponseCurve::Identity, 1.0)
            // Prefer closer targets (use predicted pos for melee)
            .consider("proximity",
                |ctx: &ScoringCtx| {
                    let effective_pos = if ctx.unit_is_melee {
                        ctx.target_predicted_pos // chase where they're going
                    } else {
                        ctx.target_pos // archers shoot at current pos
                    };
                    let dist = (ctx.unit_pos.distance(effective_pos) as f64).max(0.1);
                    8.0 / dist
                },
                ResponseCurve::Identity, 1.0)
            // Prefer high-damage targets (eliminate threats)
            .consider("threat",
                |ctx: &ScoringCtx| ctx.target_damage * 0.3,
                ResponseCurve::Identity, 1.0)
            // Melee: prioritize fleeing targets (they're kiting us)
            .consider("chase_kiters",
                |ctx: &ScoringCtx| {
                    if !ctx.unit_is_melee || !ctx.target_is_fleeing { return 0.0; }
                    // Kiting enemies are high-priority — they're ranged and avoiding us
                    8.0
                },
                ResponseCurve::Identity, 1.0)
            // Melee: protect our archers from nearby enemies
            .consider("protect_archers",
                |ctx: &ScoringCtx| {
                    if !ctx.unit_is_melee { return 0.0; }
                    let dist_to_group = ctx.target_pos.distance(ctx.our_avg_pos) as f64;
                    if dist_to_group < 5.0 { 5.0 } else { 0.0 }
                },
                ResponseCurve::Identity, 1.0)
            // Archers: stay at range, prefer targets not too close
            .consider("archer_range_pref",
                |ctx: &ScoringCtx| {
                    if ctx.unit_is_melee { return 0.0; }
                    let dist = ctx.unit_pos.distance(ctx.target_pos) as f64;
                    if dist <= ctx.unit_range && dist > 2.0 { 6.0 }
                    else if dist <= ctx.unit_range { 3.0 }
                    else { 0.0 }
                },
                ResponseCurve::Identity, 1.0)
    }
}

impl GameAgent<CombatView, Vec<Command>> for UtilityAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.prev_enemy_pos.clear();
        self.enemy_velocity.clear();
        self.prev_elapsed = 0.0;
    }

    fn observe(&mut self, view: &CombatView) {
        let dt = view.elapsed - self.prev_elapsed;
        self.prev_elapsed = view.elapsed;
        if dt <= 0.0 { return; }

        // Update velocity estimates from position deltas
        for enemy in &view.enemy_units {
            if let Some(prev) = self.prev_enemy_pos.get(&enemy.id) {
                let vx = (enemy.pos.x - prev.x) / dt;
                let vy = (enemy.pos.y - prev.y) / dt;
                // Smooth velocity with exponential moving average
                let alpha = 0.3;
                let old = self.enemy_velocity.get(&enemy.id)
                    .copied().unwrap_or(Vec2::new(0.0, 0.0));
                self.enemy_velocity.insert(enemy.id, Vec2::new(
                    old.x * (1.0 - alpha) + vx * alpha,
                    old.y * (1.0 - alpha) + vy * alpha,
                ));
            }
            self.prev_enemy_pos.insert(enemy.id, enemy.pos);
        }

        // Clean up dead enemies
        let alive_ids: Vec<u16> = view.enemy_units.iter().map(|u| u.id).collect();
        self.prev_enemy_pos.retain(|id, _| alive_ids.contains(id));
        self.enemy_velocity.retain(|id, _| alive_ids.contains(id));
    }

    fn decide(&mut self, view: &CombatView, _tree: &CommandTree<Vec<Command>>) -> Option<Vec<Command>> {
        if view.our_units.is_empty() || view.enemy_units.is_empty() {
            return Some(Vec::new());
        }

        let scorer = Self::build_scorer();

        let our_avg_pos = {
            let n = view.our_units.len() as f32;
            let sx: f32 = view.our_units.iter().map(|u| u.pos.x).sum();
            let sy: f32 = view.our_units.iter().map(|u| u.pos.y).sum();
            Vec2::new(sx / n, sy / n)
        };

        // Build score matrix
        let mut scores: Vec<Vec<f64>> = view.our_units.iter().map(|unit| {
            view.enemy_units.iter().map(|enemy| {
                let predicted = self.predict_pos(enemy.id, enemy.pos, 1.0);

                // Is this enemy moving away from us? (kiting)
                let is_fleeing = if let Some(vel) = self.enemy_velocity.get(&enemy.id) {
                    let to_us = Vec2::new(
                        unit.pos.x - enemy.pos.x,
                        unit.pos.y - enemy.pos.y,
                    );
                    // Dot product: positive = moving toward us, negative = away
                    let dot = vel.x * to_us.x + vel.y * to_us.y;
                    dot < -0.5 // moving away meaningfully
                } else {
                    false
                };

                let ctx = ScoringCtx {
                    unit_pos: unit.pos,
                    unit_range: unit.attack_range as f64,
                    unit_damage: unit.attack_damage as f64,
                    unit_is_melee: unit.attack_range < 3.0,
                    target_pos: enemy.pos,
                    target_predicted_pos: predicted,
                    target_hp: enemy.hp as f64,
                    target_max_hp: enemy.max_hp as f64,
                    target_damage: enemy.attack_damage as f64,
                    target_is_fleeing: is_fleeing,
                    our_avg_pos,
                };
                scorer.score(&ctx)
            }).collect()
        }).collect();

        // Coordinated assignment with focus fire
        let assignments = Greedy::with_coordination(|_ei, ti, scores: &mut Vec<Vec<f64>>| {
            let enemy = &view.enemy_units[ti];
            if enemy.hp < enemy.max_hp * 0.5 {
                for (ui, row) in scores.iter_mut().enumerate() {
                    let unit = &view.our_units[ui];
                    if unit.pos.distance(enemy.pos) <= unit.attack_range + 3.0 {
                        row[ti] *= 1.5;
                    }
                }
            }
        }).assign(&mut scores);

        // Generate commands
        let mut cmds = Vec::new();
        for &(ei, ti, _) in &assignments {
            let unit = &view.our_units[ei];
            let enemy = &view.enemy_units[ti];
            let dist = unit.pos.distance(enemy.pos);

            if dist <= unit.attack_range && unit.can_attack() {
                cmds.push(Command::Attack { unit_id: unit.id, target_id: enemy.id });
            }

            if unit.attack_range < 3.0 {
                // Melee: intercept fleeing targets, direct chase otherwise
                let is_fleeing = self.enemy_velocity.get(&enemy.id)
                    .map(|vel| {
                        let to_us_x = unit.pos.x - enemy.pos.x;
                        let to_us_y = unit.pos.y - enemy.pos.y;
                        vel.x * to_us_x + vel.y * to_us_y < -0.5
                    })
                    .unwrap_or(false);

                let move_target = if is_fleeing {
                    self.intercept_point(unit, enemy)
                } else {
                    enemy.pos
                };
                cmds.push(Command::Move { unit_id: unit.id, target: move_target });
            } else {
                // Archer: proactive kiting — maintain optimal range (4-5 units)
                // Find closest enemy to us (not just assigned target)
                let closest_dist = view.enemy_units.iter()
                    .map(|e| unit.pos.distance(e.pos))
                    .min_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap_or(99.0);

                if closest_dist < 4.0 {
                    // Too close — retreat away from nearest enemy
                    let nearest = view.enemy_units.iter()
                        .min_by(|a, b| unit.pos.distance(a.pos)
                            .partial_cmp(&unit.pos.distance(b.pos)).unwrap())
                        .unwrap();
                    let dx = unit.pos.x - nearest.pos.x;
                    let dy = unit.pos.y - nearest.pos.y;
                    let flee = Vec2::new(
                        (unit.pos.x + dx * 3.0).clamp(0.5, 19.5),
                        (unit.pos.y + dy * 3.0).clamp(0.5, 19.5),
                    );
                    cmds.push(Command::Move { unit_id: unit.id, target: flee });
                } else if dist > unit.attack_range * 0.85 {
                    // Too far from target — close gap to get in range
                    cmds.push(Command::Move { unit_id: unit.id, target: enemy.pos });
                }
                // else: in sweet spot (4-5 range), hold position and shoot
            }
        }

        Some(cmds)    }
}
