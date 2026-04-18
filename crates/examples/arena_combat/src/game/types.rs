pub type Player = usize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self { Self { x, y } }

    pub fn distance(self, other: Vec2) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    pub fn toward(self, target: Vec2, speed: f32) -> Vec2 {
        let dx = target.x - self.x;
        let dy = target.y - self.y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 0.01 { return self; }
        let scale = (speed / dist).min(1.0);
        Vec2::new(self.x + dx * scale, self.y + dy * scale)
    }
}

impl std::fmt::Display for Vec2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:.1},{:.1})", self.x, self.y)
    }
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub id: u16,
    pub owner: Player,
    pub pos: Vec2,
    pub hp: f32,
    pub max_hp: f32,
    pub speed: f32,       // units per second
    pub attack_range: f32,
    pub attack_damage: f32,
    pub attack_cooldown: f32, // seconds between attacks
    pub cooldown_remaining: f32,
}

impl Unit {
    pub fn warrior(id: u16, owner: Player, pos: Vec2) -> Self {
        Self {
            id, owner, pos,
            hp: 100.0, max_hp: 100.0,
            speed: 3.0,
            attack_range: 1.5,
            attack_damage: 15.0,
            attack_cooldown: 0.5,
            cooldown_remaining: 0.0,
        }
    }

    pub fn archer(id: u16, owner: Player, pos: Vec2) -> Self {
        Self {
            id, owner, pos,
            hp: 40.0, max_hp: 40.0,   // fragile — dies fast if caught
            speed: 1.5,                // very slow — warrior (3.0) catches easily
            attack_range: 6.0,
            attack_damage: 8.0,        // low damage alone
            attack_cooldown: 1.2,      // slow fire rate
            cooldown_remaining: 0.0,
        }
    }

    pub fn is_alive(&self) -> bool { self.hp > 0.0 }
    pub fn can_attack(&self) -> bool { self.cooldown_remaining <= 0.0 }
}

/// Commands a player can issue per tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Move a unit toward a position.
    Move { unit_id: u16, target: Vec2 },
    /// Attack a specific enemy unit.
    Attack { unit_id: u16, target_id: u16 },
}
