pub type Player = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub row: u8,
    pub col: u8,
}

impl Pos {
    pub fn new(row: u8, col: u8) -> Self { Self { row, col } }

    pub fn manhattan_distance(self, other: Pos) -> u8 {
        self.row.abs_diff(other.row) + self.col.abs_diff(other.col)
    }
}

impl std::fmt::Display for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({},{})", self.row, self.col)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terrain {
    Plains,    // no bonus, move cost 1
    Forest,    // +1 defense, move cost 1, blocks vision
    Mountain,  // +2 defense, move cost 2, infantry only, +2 vision
    City,      // +2 defense, generates 1000 income, capturable
    HQ,        // +3 defense, generates 1000 income, capture = win
}

impl Terrain {
    pub fn defense_bonus(self) -> u8 {
        match self {
            Terrain::Plains => 0,
            Terrain::Forest => 1,
            Terrain::Mountain => 2,
            Terrain::City => 2,
            Terrain::HQ => 3,
        }
    }

    pub fn move_cost(self) -> u8 {
        match self {
            Terrain::Plains => 1,
            Terrain::Forest => 1,
            Terrain::Mountain => 2,
            Terrain::City => 1,
            Terrain::HQ => 1,
        }
    }

    pub fn vision_bonus(self) -> i8 {
        match self {
            Terrain::Mountain => 2,
            Terrain::Forest => -1, // reduces vision when looking through
            _ => 0,
        }
    }

    pub fn blocks_vision(self) -> bool {
        matches!(self, Terrain::Forest | Terrain::Mountain)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitType {
    Infantry,   // cheap, captures buildings, 3 move, 2 vision
    Tank,       // strong, 5 move, 3 vision, can't enter mountains
    Artillery,  // ranged (2-3 range), 3 move, 1 vision, can't move and attack
    Recon,      // fast scout, 6 move, 5 vision, weak
}

impl UnitType {
    pub fn cost(self) -> u32 {
        match self {
            UnitType::Infantry => 1000,
            UnitType::Tank => 3000,
            UnitType::Artillery => 3000,
            UnitType::Recon => 2000,
        }
    }

    pub fn max_hp(self) -> u8 {
        10 // all units have 10 HP (like Advance Wars)
    }

    pub fn move_range(self) -> u8 {
        match self {
            UnitType::Infantry => 3,
            UnitType::Tank => 5,
            UnitType::Artillery => 3,
            UnitType::Recon => 6,
        }
    }

    pub fn vision(self) -> u8 {
        match self {
            UnitType::Infantry => 2,
            UnitType::Tank => 3,
            UnitType::Artillery => 1,
            UnitType::Recon => 5,
        }
    }

    pub fn attack_power(self) -> u8 {
        match self {
            UnitType::Infantry => 5,
            UnitType::Tank => 7,
            UnitType::Artillery => 8,
            UnitType::Recon => 4,
        }
    }

    pub fn can_enter(self, terrain: Terrain) -> bool {
        match terrain {
            Terrain::Mountain => self == UnitType::Infantry,
            _ => true,
        }
    }

    pub fn can_capture(self) -> bool {
        self == UnitType::Infantry
    }

    pub fn is_ranged(self) -> bool {
        self == UnitType::Artillery
    }

    pub fn attack_min_range(self) -> u8 {
        if self.is_ranged() { 2 } else { 1 }
    }

    pub fn attack_max_range(self) -> u8 {
        if self.is_ranged() { 3 } else { 1 }
    }
}

/// A unit on the map.
#[derive(Debug, Clone)]
pub struct Unit {
    pub id: u16,
    pub owner: Player,
    pub unit_type: UnitType,
    pub hp: u8,
    pub pos: Pos,
    pub moved: bool,    // has moved this turn
    pub attacked: bool, // has attacked this turn
    /// Capture progress on a building (0-20, building captured at 20).
    pub capture_progress: u8,
}

impl Unit {
    pub fn new(id: u16, owner: Player, unit_type: UnitType, pos: Pos) -> Self {
        Self {
            id, owner, unit_type, hp: unit_type.max_hp(), pos,
            moved: false, attacked: false, capture_progress: 0,
        }
    }

    /// Effective capture power = HP (so damaged units capture slower).
    pub fn capture_power(&self) -> u8 {
        self.hp
    }
}

/// Ownership info for a building (city/HQ).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Building {
    pub pos: Pos,
    pub terrain: Terrain,
    pub owner: Option<Player>,
    /// Capture progress by the capturing player (0-20).
    pub capture_progress: u8,
    pub capturing_player: Option<Player>,
}

// =========================================================================
// Smart object tasks — buildings and enemies advertise available actions
// =========================================================================

use telic::planning::utility::ActionSource;
use super::state::SimpleWarsView;

/// A task that a world object offers to an agent.
/// Buildings offer capture tasks, enemies offer attack tasks, etc.
#[derive(Debug, Clone)]
pub struct OfferedTask {
    pub kind: TaskKind,
    pub target: Pos,
    /// Pre-computed value (building value, combat odds, etc).
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskKind {
    Capture,
    Attack,
    Defend,
    Advance,
}

/// Buildings advertise capture tasks to non-owning players.
impl ActionSource<OfferedTask, SimpleWarsView> for Building {
    fn available_actions(&self, view: &SimpleWarsView) -> Vec<OfferedTask> {
        if self.owner == Some(view.viewer) { return Vec::new(); }
        if self.terrain != Terrain::City && self.terrain != Terrain::HQ { return Vec::new(); }

        let value = match self.terrain {
            Terrain::HQ => 20.0,
            Terrain::City => {
                let dist_to_us = self.pos.manhattan_distance(view.our_hq) as f64;
                let dist_to_enemy = self.pos.manhattan_distance(view.enemy_hq) as f64;
                if dist_to_us < dist_to_enemy { 6.0 } else { 4.0 }
            }
            _ => 0.0,
        };

        vec![OfferedTask { kind: TaskKind::Capture, target: self.pos, value }]
    }
}

/// Visible enemy units advertise attack tasks.
impl ActionSource<OfferedTask, SimpleWarsView> for Unit {
    fn available_actions(&self, view: &SimpleWarsView) -> Vec<OfferedTask> {
        if self.owner == view.viewer || self.hp == 0 { return Vec::new(); }

        let odds = if self.hp > 0 {
            5.0 / self.unit_type.attack_power() as f64
        } else { 10.0 };

        vec![OfferedTask { kind: TaskKind::Attack, target: self.pos, value: odds }]
    }
}

/// Commands a player can issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Move a unit to a destination.
    Move { unit_id: u16, to: Pos },
    /// Attack a target (after moving, or artillery from position).
    Attack { unit_id: u16, target_pos: Pos },
    /// Move then attack in one action.
    MoveAttack { unit_id: u16, move_to: Pos, target_pos: Pos },
    /// Start/continue capturing a building the unit is on.
    Capture { unit_id: u16 },
    /// Build a unit at HQ.
    Build { unit_type: UnitType },
    /// End turn.
    EndTurn,
}
