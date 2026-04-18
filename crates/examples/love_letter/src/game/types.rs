pub type Player = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Card {
    Guard,     // 1, x5 — guess opponent's card
    Priest,    // 2, x2 — peek at opponent's hand
    Baron,     // 3, x2 — compare hands, lower is out
    Handmaid,  // 4, x2 — protection until next turn
    Prince,    // 5, x2 — force opponent to discard and draw
    King,      // 6, x1 — swap hands
    Countess,  // 7, x1 — must play if holding King or Prince
    Princess,  // 8, x1 — eliminated if played
}

impl Card {
    pub fn value(self) -> u8 {
        match self {
            Card::Guard => 1,
            Card::Priest => 2,
            Card::Baron => 3,
            Card::Handmaid => 4,
            Card::Prince => 5,
            Card::King => 6,
            Card::Countess => 7,
            Card::Princess => 8,
        }
    }

    pub fn count_in_deck(self) -> u8 {
        match self {
            Card::Guard => 5,
            Card::Priest => 2,
            Card::Baron => 2,
            Card::Handmaid => 2,
            Card::Prince => 2,
            Card::King => 1,
            Card::Countess => 1,
            Card::Princess => 1,
        }
    }

    pub fn all_types() -> &'static [Card] {
        &[
            Card::Guard, Card::Priest, Card::Baron, Card::Handmaid,
            Card::Prince, Card::King, Card::Countess, Card::Princess,
        ]
    }
}

impl std::fmt::Display for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Card::Guard => "Guard(1)",
            Card::Priest => "Priest(2)",
            Card::Baron => "Baron(3)",
            Card::Handmaid => "Handmaid(4)",
            Card::Prince => "Prince(5)",
            Card::King => "King(6)",
            Card::Countess => "Countess(7)",
            Card::Princess => "Princess(8)",
        };
        write!(f, "{}", name)
    }
}

/// A command: which card to play, and any required choice (Guard guess target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayCommand {
    /// Which card from hand to play (0 or 1, since hand has 2 cards during turn).
    pub card: Card,
    /// For Guard: which card to guess the opponent has (cannot guess Guard).
    pub guard_guess: Option<Card>,
}
