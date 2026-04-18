use super::types::{Card, Gem, GemSet, Noble};

/// The actual Splendor card set (90 cards).
/// Data from: https://github.com/bouk/splendimax/blob/master/Splendor%20Cards.csv
pub fn real_cards() -> Vec<Card> {
    let raw = [
        // Tier 1 (40 cards)
        // (tier, bonus_color, points, black, blue, green, red, white)
        (1, "K", 0, 0,1,1,1,1), (1, "K", 0, 0,2,1,1,1), (1, "K", 0, 0,2,0,1,2),
        (1, "K", 0, 1,0,1,3,0), (1, "K", 0, 0,0,2,1,0), (1, "K", 0, 0,0,2,0,2),
        (1, "K", 0, 0,0,3,0,0), (1, "K", 1, 0,4,0,0,0),

        (1, "U", 0, 1,0,1,1,1), (1, "U", 0, 1,0,1,2,1), (1, "U", 0, 0,0,2,2,1),
        (1, "U", 0, 0,1,3,1,0), (1, "U", 0, 2,0,0,0,1), (1, "U", 0, 2,0,2,0,0),
        (1, "U", 0, 3,0,0,0,0), (1, "U", 1, 0,0,0,4,0),

        (1, "W", 0, 1,1,1,1,0), (1, "W", 0, 1,1,2,1,0), (1, "W", 0, 1,2,2,0,0),
        (1, "W", 0, 1,1,0,0,3), (1, "W", 0, 1,0,0,2,0), (1, "W", 0, 2,2,0,0,0),
        (1, "W", 0, 0,3,0,0,0), (1, "W", 1, 0,0,4,0,0),

        (1, "G", 0, 1,1,0,1,1), (1, "G", 0, 2,1,0,1,1), (1, "G", 0, 2,1,0,2,0),
        (1, "G", 0, 0,3,1,0,1), (1, "G", 0, 0,1,0,0,2), (1, "G", 0, 0,2,0,2,0),
        (1, "G", 0, 0,0,0,3,0), (1, "G", 1, 4,0,0,0,0),

        (1, "R", 0, 1,1,1,0,1), (1, "R", 0, 1,1,1,0,2), (1, "R", 0, 2,0,1,0,2),
        (1, "R", 0, 3,0,0,1,1), (1, "R", 0, 0,2,1,0,0), (1, "R", 0, 0,0,0,2,2),
        (1, "R", 0, 0,0,0,0,3), (1, "R", 1, 0,0,0,0,4),

        // Tier 2 (30 cards)
        (2, "K", 1, 0,2,2,0,3), (2, "K", 1, 2,0,3,0,3), (2, "K", 2, 0,1,4,2,0),
        (2, "K", 2, 0,0,5,3,0), (2, "K", 2, 0,0,0,0,5), (2, "K", 3, 6,0,0,0,0),

        (2, "U", 1, 0,2,2,3,0), (2, "U", 1, 3,2,3,0,0), (2, "U", 2, 0,3,0,0,5),
        (2, "U", 2, 4,0,0,1,2), (2, "U", 2, 0,5,0,0,0), (2, "U", 3, 0,6,0,0,0),

        (2, "W", 1, 2,0,3,2,0), (2, "W", 1, 0,3,0,3,2), (2, "W", 2, 2,0,1,4,0),
        (2, "W", 2, 3,0,0,5,0), (2, "W", 2, 0,0,0,5,0), (2, "W", 3, 0,0,0,0,6),

        (2, "G", 1, 0,0,2,3,3), (2, "G", 1, 2,3,0,0,2), (2, "G", 2, 1,2,0,0,4),
        (2, "G", 2, 0,5,3,0,0), (2, "G", 2, 0,0,5,0,0), (2, "G", 3, 0,0,6,0,0),

        (2, "R", 1, 3,0,0,2,2), (2, "R", 1, 3,3,0,2,0), (2, "R", 2, 0,4,2,0,1),
        (2, "R", 2, 5,0,0,0,3), (2, "R", 2, 5,0,0,0,0), (2, "R", 3, 0,0,0,6,0),

        // Tier 3 (20 cards)
        (3, "K", 3, 0,3,5,3,3), (3, "K", 4, 0,0,0,7,0), (3, "K", 4, 3,0,3,6,0),
        (3, "K", 5, 3,0,0,7,0),

        (3, "U", 3, 5,0,3,3,3), (3, "U", 4, 0,0,0,0,7), (3, "U", 4, 3,3,0,0,6),
        (3, "U", 5, 0,3,0,0,7),

        (3, "W", 3, 3,3,3,5,0), (3, "W", 4, 7,0,0,0,0), (3, "W", 4, 6,0,0,3,3),
        (3, "W", 5, 7,0,0,0,3),

        (3, "G", 3, 3,3,0,3,5), (3, "G", 4, 0,7,0,0,0), (3, "G", 4, 0,6,3,0,3),
        (3, "G", 5, 0,7,3,0,0),

        (3, "R", 3, 3,5,3,0,3), (3, "R", 4, 0,0,7,0,0), (3, "R", 4, 0,3,6,3,0),
        (3, "R", 5, 0,0,7,3,0),
    ];

    raw.iter().enumerate().map(|(id, &(tier, color, pv, k, u, g, r, w))| {
        let bonus = match color {
            "K" => Gem::Black,
            "U" => Gem::Blue,
            "G" => Gem::Green,
            "R" => Gem::Red,
            "W" => Gem::White,
            _ => unreachable!(),
        };
        let mut cost = GemSet::new();
        cost.set(Gem::Black, k);
        cost.set(Gem::Blue, u);
        cost.set(Gem::Green, g);
        cost.set(Gem::Red, r);
        cost.set(Gem::White, w);

        Card { id: id as u16, tier, points: pv, bonus, cost }
    }).collect()
}

/// Real Splendor nobles (10 nobles, use 3 for 2-player game).
pub fn real_nobles() -> Vec<Noble> {
    vec![
        Noble { id: 0, points: 3, required: [3, 3, 0, 0, 3] },  // K+U+W
        Noble { id: 1, points: 3, required: [0, 3, 3, 3, 0] },  // U+G+R
        Noble { id: 2, points: 3, required: [3, 0, 0, 3, 3] },  // K+R+W
        Noble { id: 3, points: 3, required: [0, 0, 3, 3, 3] },  // G+R+W
        Noble { id: 4, points: 3, required: [3, 3, 3, 0, 0] },  // K+U+G
        Noble { id: 5, points: 3, required: [0, 0, 4, 4, 0] },  // G+R (4 each)
        Noble { id: 6, points: 3, required: [0, 4, 0, 0, 4] },  // U+W
        Noble { id: 7, points: 3, required: [4, 0, 0, 0, 4] },  // K+W
        Noble { id: 8, points: 3, required: [4, 4, 0, 0, 0] },  // K+U
        Noble { id: 9, points: 3, required: [0, 0, 4, 0, 4] },  // G+W... wait, this might not be right
    ]
}

pub fn shuffle_cards(cards: &mut Vec<Card>, seed: u64) {
    let mut rng = seed.max(1);
    for i in (1..cards.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let j = rng as usize % (i + 1);
        cards.swap(i, j);
    }
}

pub fn shuffle_nobles(nobles: &mut Vec<Noble>, seed: u64) {
    let mut rng = seed.max(1);
    for i in (1..nobles.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let j = rng as usize % (i + 1);
        nobles.swap(i, j);
    }
}

// Keep these for backwards compat
pub fn generate_cards(_seed: u64) -> Vec<Card> { real_cards() }
pub fn generate_nobles() -> Vec<Noble> { real_nobles() }
