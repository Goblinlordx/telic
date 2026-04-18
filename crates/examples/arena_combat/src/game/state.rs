use telic::arena::{GameState, GameView, GameOutcome, PlayerIndex};
use super::types::*;

const ARENA_WIDTH: f32 = 20.0;
const ARENA_HEIGHT: f32 = 20.0;
const DT: f32 = 1.0 / 60.0;

/// What a player can see — all units are visible (no fog of war).
#[derive(Debug, Clone)]
pub struct CombatView {
    pub viewer: Player,
    pub elapsed: f32,
    pub tick: u32,
    pub our_units: Vec<Unit>,
    pub enemy_units: Vec<Unit>,
    pub arena_width: f32,
    pub arena_height: f32,
}

impl GameView for CombatView {
    fn viewer(&self) -> PlayerIndex { self.viewer }
    fn turn(&self) -> u32 { self.tick }
}

/// Arena combat game — two squads fight on an open field.
///
/// Both players submit commands each tick. The game collects commands
/// from all players, then advances the simulation by DT when all
/// players have submitted. Commands from a player who already submitted
/// this tick are rejected until the tick advances.
#[derive(Debug)]
pub struct ArenaCombatGame {
    units: Vec<Unit>,
    elapsed: f32,
    tick: u32,
    winner: Option<Player>,
    next_id: u16,
    max_time: f32,
    /// Commands pending for current tick, per player
    pending: [Option<Vec<Command>>; 2],
}

impl ArenaCombatGame {
    pub fn standard() -> Self {
        let mut game = Self::empty(120.0);

        game.spawn(|id| Unit::warrior(id, 0, Vec2::new(2.0, 8.0)));
        game.spawn(|id| Unit::warrior(id, 0, Vec2::new(2.0, 10.0)));
        game.spawn(|id| Unit::warrior(id, 0, Vec2::new(2.0, 12.0)));
        game.spawn(|id| Unit::archer(id, 0, Vec2::new(1.0, 9.0)));
        game.spawn(|id| Unit::archer(id, 0, Vec2::new(1.0, 11.0)));

        game.spawn(|id| Unit::warrior(id, 1, Vec2::new(18.0, 8.0)));
        game.spawn(|id| Unit::warrior(id, 1, Vec2::new(18.0, 10.0)));
        game.spawn(|id| Unit::warrior(id, 1, Vec2::new(18.0, 12.0)));
        game.spawn(|id| Unit::archer(id, 1, Vec2::new(19.0, 9.0)));
        game.spawn(|id| Unit::archer(id, 1, Vec2::new(19.0, 11.0)));

        game
    }

    pub fn random(seed: u64) -> Self {
        let mut rng = seed.max(1);
        let mut xorshift = |rng: &mut u64| -> f32 {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            (*rng % 1000) as f32 / 1000.0
        };

        let mut game = Self::empty(120.0);

        for _ in 0..3 {
            let x = 1.0 + xorshift(&mut rng) * 7.0;
            let y = 2.0 + xorshift(&mut rng) * 16.0;
            game.spawn(|id| Unit::warrior(id, 0, Vec2::new(x, y)));
        }
        for _ in 0..2 {
            let x = 1.0 + xorshift(&mut rng) * 5.0;
            let y = 2.0 + xorshift(&mut rng) * 16.0;
            game.spawn(|id| Unit::archer(id, 0, Vec2::new(x, y)));
        }
        for _ in 0..3 {
            let x = 12.0 + xorshift(&mut rng) * 7.0;
            let y = 2.0 + xorshift(&mut rng) * 16.0;
            game.spawn(|id| Unit::warrior(id, 1, Vec2::new(x, y)));
        }
        for _ in 0..2 {
            let x = 14.0 + xorshift(&mut rng) * 5.0;
            let y = 2.0 + xorshift(&mut rng) * 16.0;
            game.spawn(|id| Unit::archer(id, 1, Vec2::new(x, y)));
        }

        game
    }

    fn empty(max_time: f32) -> Self {
        Self {
            units: Vec::new(),
            elapsed: 0.0,
            tick: 0,
            winner: None,
            next_id: 0,
            max_time,
            pending: [None, None],
        }
    }

    fn spawn(&mut self, f: impl FnOnce(u16) -> Unit) {
        let id = self.next_id;
        self.next_id += 1;
        self.units.push(f(id));
    }

    /// Advance simulation by one tick using pending commands.
    fn sim_tick(&mut self) {
        let cmds: Vec<Vec<Command>> = self.pending.iter()
            .map(|p| p.clone().unwrap_or_default())
            .collect();
        self.pending = [None, None];

        // Tick cooldowns
        for unit in &mut self.units {
            if unit.cooldown_remaining > 0.0 {
                unit.cooldown_remaining = (unit.cooldown_remaining - DT).max(0.0);
            }
        }

        // Process movements
        for player_cmds in &cmds {
            for cmd in player_cmds {
                if let Command::Move { unit_id, target } = cmd {
                    if let Some(unit) = self.units.iter_mut()
                        .find(|u| u.id == *unit_id && u.is_alive())
                    {
                        let clamped = Vec2::new(
                            target.x.clamp(0.0, ARENA_WIDTH),
                            target.y.clamp(0.0, ARENA_HEIGHT),
                        );
                        unit.pos = unit.pos.toward(clamped, unit.speed * DT);
                    }
                }
            }
        }

        // Process attacks
        for player_cmds in &cmds {
            for cmd in player_cmds {
                if let Command::Attack { unit_id, target_id } = cmd {
                    let (a_pos, a_owner, a_range, a_dmg, can) = {
                        match self.units.iter().find(|u| u.id == *unit_id && u.is_alive()) {
                            Some(u) => (u.pos, u.owner, u.attack_range, u.attack_damage, u.can_attack()),
                            None => continue,
                        }
                    };
                    let t = match self.units.iter().find(|u| u.id == *target_id && u.is_alive()) {
                        Some(u) => u,
                        None => continue,
                    };
                    if t.owner == a_owner || !can { continue; }
                    if a_pos.distance(t.pos) <= a_range {
                        if let Some(t) = self.units.iter_mut().find(|u| u.id == *target_id) {
                            t.hp -= a_dmg;
                            if t.hp <= 0.0 { t.hp = 0.0; }
                        }
                        if let Some(a) = self.units.iter_mut().find(|u| u.id == *unit_id) {
                            a.cooldown_remaining = a.attack_cooldown;
                        }
                    }
                }
            }
        }

        self.elapsed += DT;
        self.tick += 1;

        // Check winner
        let p0 = self.units.iter().any(|u| u.owner == 0 && u.is_alive());
        let p1 = self.units.iter().any(|u| u.owner == 1 && u.is_alive());

        if !p0 && p1 { self.winner = Some(1); }
        else if p0 && !p1 { self.winner = Some(0); }
        else if !p0 && !p1 { self.winner = Some(0); }

        if self.elapsed >= self.max_time && self.winner.is_none() {
            let hp0: f32 = self.units.iter().filter(|u| u.owner == 0 && u.is_alive()).map(|u| u.hp).sum();
            let hp1: f32 = self.units.iter().filter(|u| u.owner == 1 && u.is_alive()).map(|u| u.hp).sum();
            self.winner = Some(if hp0 >= hp1 { 0 } else { 1 });
        }
    }
}

impl GameState for ArenaCombatGame {
    type Command = Vec<Command>; // batch of commands per tick
    type View = CombatView;

    fn view_for(&self, player: PlayerIndex) -> CombatView {
        CombatView {
            viewer: player,
            elapsed: self.elapsed,
            tick: self.tick,
            our_units: self.units.iter().filter(|u| u.owner == player && u.is_alive()).cloned().collect(),
            enemy_units: self.units.iter().filter(|u| u.owner != player && u.is_alive()).cloned().collect(),
            arena_width: ARENA_WIDTH,
            arena_height: ARENA_HEIGHT,
        }
    }

    fn apply_command(&mut self, player: PlayerIndex, commands: Vec<Command>) -> Result<(), String> {
        if self.winner.is_some() { return Err("Game over".into()); }
        if player > 1 { return Err("Invalid player".into()); }
        if self.pending[player].is_some() { return Err("Already submitted this tick".into()); }

        self.pending[player] = Some(commands);

        // When all players have submitted, tick the simulation
        if self.pending.iter().all(|p| p.is_some()) {
            self.sim_tick();
        }

        Ok(())
    }

    fn is_terminal(&self) -> bool { self.winner.is_some() }
    fn outcome(&self) -> Option<GameOutcome> { self.winner.map(GameOutcome::Winner) }
    fn turn_number(&self) -> u32 { self.tick }
    fn num_players(&self) -> usize { 2 }
}
