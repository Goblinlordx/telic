pub type Player = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gem {
    White,
    Blue,
    Green,
    Red,
    Black,
}

impl Gem {
    pub const ALL: [Gem; 5] = [Gem::White, Gem::Blue, Gem::Green, Gem::Red, Gem::Black];

    pub fn index(self) -> usize {
        match self {
            Gem::White => 0,
            Gem::Blue => 1,
            Gem::Green => 2,
            Gem::Red => 3,
            Gem::Black => 4,
        }
    }
}

impl std::fmt::Display for Gem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Gem::White => write!(f, "W"),
            Gem::Blue => write!(f, "U"),
            Gem::Green => write!(f, "G"),
            Gem::Red => write!(f, "R"),
            Gem::Black => write!(f, "B"),
        }
    }
}

/// Gem counts — 5 regular colors + gold (wild).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GemSet {
    pub gems: [u8; 5],
    pub gold: u8,
}

impl GemSet {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, gem: Gem) -> u8 { self.gems[gem.index()] }
    pub fn set(&mut self, gem: Gem, val: u8) { self.gems[gem.index()] = val; }
    pub fn add(&mut self, gem: Gem, n: u8) { self.gems[gem.index()] += n; }
    pub fn sub(&mut self, gem: Gem, n: u8) { self.gems[gem.index()] = self.gems[gem.index()].saturating_sub(n); }

    pub fn total(&self) -> u8 {
        self.gems.iter().sum::<u8>() + self.gold
    }

    /// Can we afford a cost, given bonuses? Returns (affordable, gold_needed).
    pub fn can_afford(&self, cost: &GemSet, bonuses: &[u8; 5]) -> (bool, u8) {
        let mut gold_needed = 0u8;
        for g in Gem::ALL {
            let effective_cost = cost.get(g).saturating_sub(bonuses[g.index()]);
            if self.get(g) < effective_cost {
                gold_needed += effective_cost - self.get(g);
            }
        }
        (gold_needed <= self.gold, gold_needed)
    }
}

/// A development card.
#[derive(Debug, Clone)]
pub struct Card {
    pub id: u16,
    pub tier: u8,      // 1, 2, or 3
    pub points: u8,
    pub bonus: Gem,     // permanent gem bonus when purchased
    pub cost: GemSet,
}

/// A noble tile (bonus points for collecting bonuses).
#[derive(Debug, Clone)]
pub struct Noble {
    pub id: u8,
    pub points: u8,     // always 3
    pub required: [u8; 5], // bonus gems needed to attract
}

/// Actions a player can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Take 3 different gem tokens.
    TakeThree([Gem; 3]),
    /// Take 2 of the same gem (must be 4+ available).
    TakeTwo(Gem),
    /// Reserve a face-up card (gain 1 gold).
    Reserve { tier: u8, index: usize },
    /// Buy a face-up card.
    Buy { tier: u8, index: usize },
    /// Buy a reserved card.
    BuyReserved { index: usize },
    /// Pass (shouldn't normally happen).
    Pass,
}
