pub type Player = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
}

impl std::fmt::Display for Suit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Suit::Clubs => write!(f, "c"),
            Suit::Diamonds => write!(f, "d"),
            Suit::Hearts => write!(f, "h"),
            Suit::Spades => write!(f, "s"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rank(pub u8); // 2-14 (14 = Ace)

impl Rank {
    pub const TWO: Rank = Rank(2);
    pub const THREE: Rank = Rank(3);
    pub const FOUR: Rank = Rank(4);
    pub const FIVE: Rank = Rank(5);
    pub const SIX: Rank = Rank(6);
    pub const SEVEN: Rank = Rank(7);
    pub const EIGHT: Rank = Rank(8);
    pub const NINE: Rank = Rank(9);
    pub const TEN: Rank = Rank(10);
    pub const JACK: Rank = Rank(11);
    pub const QUEEN: Rank = Rank(12);
    pub const KING: Rank = Rank(13);
    pub const ACE: Rank = Rank(14);

    pub const ALL: [Rank; 13] = [
        Rank(2), Rank(3), Rank(4), Rank(5), Rank(6), Rank(7),
        Rank(8), Rank(9), Rank(10), Rank(11), Rank(12), Rank(13), Rank(14),
    ];
}

impl std::fmt::Display for Rank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            2..=9 => write!(f, "{}", self.0),
            10 => write!(f, "T"),
            11 => write!(f, "J"),
            12 => write!(f, "Q"),
            13 => write!(f, "K"),
            14 => write!(f, "A"),
            _ => write!(f, "?"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Self { Self { rank, suit } }
}

impl std::fmt::Display for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.rank, self.suit)
    }
}

/// Build a standard 52-card deck.
pub fn standard_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);
    for &rank in &Rank::ALL {
        for &suit in &Suit::ALL {
            deck.push(Card::new(rank, suit));
        }
    }
    deck
}

/// Shuffle a deck with a seed.
pub fn shuffle(deck: &mut Vec<Card>, seed: u64) {
    let mut rng = seed.max(1);
    for i in (1..deck.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let j = rng as usize % (i + 1);
        deck.swap(i, j);
    }
}

/// Betting round stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
    Showdown,
}

/// Actions a player can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Fold,
    Check,
    Call,
    Raise(u32), // raise TO this amount (total bet)
    AllIn,
}

/// Hand rankings (higher = better).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HandRank {
    HighCard(u8, u8, u8, u8, u8),       // 5 kickers descending
    Pair(u8, u8, u8, u8),               // pair rank + 3 kickers
    TwoPair(u8, u8, u8),                // high pair, low pair, kicker
    ThreeOfAKind(u8, u8, u8),           // trips rank + 2 kickers
    Straight(u8),                        // high card of straight
    Flush(u8, u8, u8, u8, u8),          // 5 cards descending
    FullHouse(u8, u8),                   // trips rank, pair rank
    FourOfAKind(u8, u8),                // quads rank, kicker
    StraightFlush(u8),                   // high card
}

/// Evaluate the best 5-card hand from up to 7 cards.
pub fn evaluate_hand(cards: &[Card]) -> HandRank {
    assert!(cards.len() >= 5 && cards.len() <= 7);

    let mut best = HandRank::HighCard(0, 0, 0, 0, 0);

    // Try all 5-card combinations
    let n = cards.len();
    for i in 0..n {
        for j in (i+1)..n {
            for k in (j+1)..n {
                for l in (k+1)..n {
                    for m in (l+1)..n {
                        let hand = [cards[i], cards[j], cards[k], cards[l], cards[m]];
                        let rank = rank_five(&hand);
                        if rank > best {
                            best = rank;
                        }
                    }
                }
            }
        }
    }

    best
}

fn rank_five(cards: &[Card; 5]) -> HandRank {
    let mut ranks: Vec<u8> = cards.iter().map(|c| c.rank.0).collect();
    ranks.sort_unstable_by(|a, b| b.cmp(a)); // descending

    let is_flush = cards.iter().all(|c| c.suit == cards[0].suit);

    // Check straight (including ace-low: A-2-3-4-5)
    let is_straight = is_consecutive(&ranks)
        || ranks == vec![14, 5, 4, 3, 2]; // ace-low straight

    let straight_high = if ranks == vec![14, 5, 4, 3, 2] { 5 } else { ranks[0] };

    if is_flush && is_straight {
        return HandRank::StraightFlush(straight_high);
    }

    // Count rank frequencies
    let mut counts: Vec<(u8, u8)> = Vec::new(); // (count, rank)
    let mut i = 0;
    while i < ranks.len() {
        let rank = ranks[i];
        let mut count = 1u8;
        loop {
            let next = i + count as usize;
            if next >= ranks.len() || ranks[next] != rank { break; }
            count += 1;
        }
        counts.push((count, rank));
        i += count as usize;
    }
    counts.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1))); // by count desc, then rank desc

    match counts.as_slice() {
        [(4, q), (1, k)] => HandRank::FourOfAKind(*q, *k),
        [(3, t), (2, p)] => HandRank::FullHouse(*t, *p),
        _ if is_flush => HandRank::Flush(ranks[0], ranks[1], ranks[2], ranks[3], ranks[4]),
        _ if is_straight => HandRank::Straight(straight_high),
        [(3, t), (1, k1), (1, k2)] => HandRank::ThreeOfAKind(*t, *k1, *k2),
        [(2, p1), (2, p2), (1, k)] => HandRank::TwoPair(*p1, *p2, *k),
        [(2, p), (1, k1), (1, k2), (1, k3)] => HandRank::Pair(*p, *k1, *k2, *k3),
        [(1, a), (1, b), (1, c), (1, d), (1, e)] => HandRank::HighCard(*a, *b, *c, *d, *e),
        _ => HandRank::HighCard(ranks[0], ranks[1], ranks[2], ranks[3], ranks[4]),
    }
}

fn is_consecutive(ranks: &[u8]) -> bool {
    if ranks.len() < 5 { return false; }
    for i in 0..ranks.len()-1 {
        if ranks[i] != ranks[i+1] + 1 { return false; }
    }
    true
}
