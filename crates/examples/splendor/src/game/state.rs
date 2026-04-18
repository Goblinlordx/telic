use telic::arena::{GameState, GameOutcome, PlayerIndex, GameView};
use super::types::*;
use super::cards::{real_cards, real_nobles, shuffle_cards, shuffle_nobles};

const GEMS_PER_COLOR: u8 = 4; // 2-player rules
const GOLD_TOKENS: u8 = 5;
const MAX_TOKENS: u8 = 10;
const WIN_POINTS: u8 = 15;
const CARDS_PER_ROW: usize = 4;
const MAX_RESERVED: usize = 3;

/// What a player can see.
#[derive(Debug, Clone)]
pub struct SplendorView {
    pub viewer: Player,
    pub turn: u32,
    /// True when it is the viewer's turn to act.
    pub is_our_turn: bool,
    /// Gem tokens available in the bank.
    pub bank: GemSet,
    /// Face-up cards per tier (tier index 0=tier1, 1=tier2, 2=tier3).
    pub market: [Vec<Card>; 3],
    /// Cards remaining in each tier's deck (count only).
    pub deck_sizes: [usize; 3],
    /// Our tokens.
    pub our_tokens: GemSet,
    /// Our purchased cards (visible to all).
    pub our_cards: Vec<Card>,
    /// Our bonus counts per gem.
    pub our_bonuses: [u8; 5],
    /// Our points.
    pub our_points: u8,
    /// Our reserved cards (hidden from opponent).
    pub our_reserved: Vec<Card>,
    /// Opponent's tokens (visible).
    pub opp_tokens: GemSet,
    /// Opponent's purchased cards (visible).
    pub opp_cards: Vec<Card>,
    /// Opponent's bonus counts.
    pub opp_bonuses: [u8; 5],
    /// Opponent's points.
    pub opp_points: u8,
    /// Opponent's reserved card count (can't see which cards).
    pub opp_reserved_count: usize,
    /// Available nobles.
    pub nobles: Vec<Noble>,
}

impl GameView for SplendorView {
    fn viewer(&self) -> PlayerIndex { self.viewer }
    fn turn(&self) -> u32 { self.turn }
}

/// Per-player state.
#[derive(Debug, Clone)]
struct PlayerState {
    tokens: GemSet,
    cards: Vec<Card>,
    bonuses: [u8; 5],
    points: u8,
    reserved: Vec<Card>,
}

impl PlayerState {
    fn new() -> Self {
        Self {
            tokens: GemSet::new(),
            cards: Vec::new(),
            bonuses: [0; 5],
            points: 0,
            reserved: Vec::new(),
        }
    }
}

/// Full game state.
#[derive(Debug, Clone)]
pub struct SplendorGame {
    bank: GemSet,
    decks: [Vec<Card>; 3],
    market: [Vec<Card>; 3],
    players: [PlayerState; 2],
    nobles: Vec<Noble>,
    current_player: Player,
    turn: u32,
    winner: Option<Player>,
}

impl SplendorGame {
    pub fn new(seed: u64) -> Self {
        let all_cards = real_cards();
        let mut nobles = real_nobles();
        shuffle_nobles(&mut nobles, seed.wrapping_add(100));
        nobles.truncate(3); // 2-player: 3 nobles

        let mut tier1: Vec<Card> = all_cards.iter().filter(|c| c.tier == 1).cloned().collect();
        let mut tier2: Vec<Card> = all_cards.iter().filter(|c| c.tier == 2).cloned().collect();
        let mut tier3: Vec<Card> = all_cards.iter().filter(|c| c.tier == 3).cloned().collect();

        shuffle_cards(&mut tier1, seed.wrapping_add(1));
        shuffle_cards(&mut tier2, seed.wrapping_add(2));
        shuffle_cards(&mut tier3, seed.wrapping_add(3));

        // Deal face-up cards
        let m1: Vec<Card> = (0..CARDS_PER_ROW).filter_map(|_| tier1.pop()).collect();
        let m2: Vec<Card> = (0..CARDS_PER_ROW).filter_map(|_| tier2.pop()).collect();
        let m3: Vec<Card> = (0..CARDS_PER_ROW).filter_map(|_| tier3.pop()).collect();

        let mut bank = GemSet::new();
        for g in Gem::ALL {
            bank.set(g, GEMS_PER_COLOR);
        }
        bank.gold = GOLD_TOKENS;

        SplendorGame {
            bank,
            decks: [tier1, tier2, tier3],
            market: [m1, m2, m3],
            players: [PlayerState::new(), PlayerState::new()],
            nobles,
            current_player: 0,
            turn: 0,
            winner: None,
        }
    }

    fn refill_market(&mut self, tier: usize) {
        while self.market[tier].len() < CARDS_PER_ROW {
            if let Some(card) = self.decks[tier].pop() {
                self.market[tier].push(card);
            } else {
                break;
            }
        }
    }

    fn check_nobles(&mut self, player: Player) {
        let bonuses = &self.players[player].bonuses;
        let mut attracted = None;

        for (i, noble) in self.nobles.iter().enumerate() {
            let qualifies = Gem::ALL.iter()
                .all(|&g| bonuses[g.index()] >= noble.required[g.index()]);
            if qualifies {
                attracted = Some(i);
                break;
            }
        }

        if let Some(idx) = attracted {
            let noble = self.nobles.remove(idx);
            self.players[player].points += noble.points;
        }
    }

    fn check_winner(&mut self) {
        // Check at end of a full round (both players have gone)
        if self.current_player == 0 && self.turn > 0 {
            let p0 = self.players[0].points;
            let p1 = self.players[1].points;
            if p0 >= WIN_POINTS || p1 >= WIN_POINTS {
                if p0 > p1 {
                    self.winner = Some(0);
                } else if p1 > p0 {
                    self.winner = Some(1);
                } else {
                    // Tie: fewer cards wins
                    let c0 = self.players[0].cards.len();
                    let c1 = self.players[1].cards.len();
                    self.winner = Some(if c0 <= c1 { 0 } else { 1 });
                }
            }
        }
    }

    fn do_buy(&mut self, player: Player, card: Card) {
        let p = &mut self.players[player];
        let (can_afford, gold_needed) = p.tokens.can_afford(&card.cost, &p.bonuses);
        if !can_afford { return; }

        // Pay gems
        for g in Gem::ALL {
            let effective = card.cost.get(g).saturating_sub(p.bonuses[g.index()]);
            let from_tokens = effective.min(p.tokens.get(g));
            p.tokens.sub(g, from_tokens);
            self.bank.add(g, from_tokens);
        }
        // Pay remaining with gold
        p.tokens.gold -= gold_needed;
        self.bank.gold += gold_needed;

        // Gain bonus and points
        p.bonuses[card.bonus.index()] += 1;
        p.points += card.points;
        p.cards.push(card);
    }
}

impl GameState for SplendorGame {
    type Command = Action;
    type View = SplendorView;

    fn view_for(&self, player: PlayerIndex) -> SplendorView {
        let opp = 1 - player;
        SplendorView {
            viewer: player,
            turn: self.turn,
            is_our_turn: self.current_player == player,
            bank: self.bank,
            market: self.market.clone(),
            deck_sizes: [self.decks[0].len(), self.decks[1].len(), self.decks[2].len()],
            our_tokens: self.players[player].tokens,
            our_cards: self.players[player].cards.clone(),
            our_bonuses: self.players[player].bonuses,
            our_points: self.players[player].points,
            our_reserved: self.players[player].reserved.clone(),
            opp_tokens: self.players[opp].tokens,
            opp_cards: self.players[opp].cards.clone(),
            opp_bonuses: self.players[opp].bonuses,
            opp_points: self.players[opp].points,
            opp_reserved_count: self.players[opp].reserved.len(),
            nobles: self.nobles.clone(),
        }
    }

    fn apply_command(&mut self, player: PlayerIndex, action: Action) -> Result<(), String> {
        if self.winner.is_some() { return Err("Game over".into()); }
        if player != self.current_player { return Err("Not your turn".into()); }

        let p = &self.players[player];

        match action {
            Action::TakeThree(gems) => {
                // Validate: 3 different gems, all available
                if gems[0] == gems[1] || gems[1] == gems[2] || gems[0] == gems[2] {
                    return Err("Must take 3 different gems".into());
                }
                for &g in &gems {
                    if self.bank.get(g) == 0 {
                        return Err(format!("No {} available", g));
                    }
                }
                if p.tokens.total() + 3 > MAX_TOKENS {
                    return Err("Would exceed 10 tokens".into());
                }

                for &g in &gems {
                    self.bank.sub(g, 1);
                    self.players[player].tokens.add(g, 1);
                }
            }
            Action::TakeTwo(gem) => {
                if self.bank.get(gem) < 4 {
                    return Err("Need 4+ gems to take 2".into());
                }
                if p.tokens.total() + 2 > MAX_TOKENS {
                    return Err("Would exceed 10 tokens".into());
                }

                self.bank.sub(gem, 2);
                self.players[player].tokens.add(gem, 2);
            }
            Action::Reserve { tier, index } => {
                let ti = (tier - 1) as usize;
                if ti >= 3 { return Err("Invalid tier".into()); }
                if index >= self.market[ti].len() { return Err("Invalid card index".into()); }
                if self.players[player].reserved.len() >= MAX_RESERVED {
                    return Err("Already have 3 reserved".into());
                }

                let card = self.market[ti].remove(index);
                self.players[player].reserved.push(card);

                if self.bank.gold > 0 {
                    self.bank.gold -= 1;
                    self.players[player].tokens.gold += 1;
                }

                self.refill_market(ti);
            }
            Action::Buy { tier, index } => {
                let ti = (tier - 1) as usize;
                if ti >= 3 { return Err("Invalid tier".into()); }
                if index >= self.market[ti].len() { return Err("Invalid card index".into()); }

                let card = self.market[ti][index].clone();
                let (can_afford, _) = p.tokens.can_afford(&card.cost, &p.bonuses);
                if !can_afford { return Err("Can't afford".into()); }

                let card = self.market[ti].remove(index);
                self.do_buy(player, card);
                self.refill_market(ti);
            }
            Action::BuyReserved { index } => {
                if index >= self.players[player].reserved.len() {
                    return Err("Invalid reserved index".into());
                }

                let card = self.players[player].reserved[index].clone();
                let (can_afford, _) = p.tokens.can_afford(&card.cost, &p.bonuses);
                if !can_afford { return Err("Can't afford".into()); }

                let card = self.players[player].reserved.remove(index);
                self.do_buy(player, card);
            }
            Action::Pass => {}
        }

        self.check_nobles(player);
        self.current_player = 1 - self.current_player;
        self.turn += 1;
        self.check_winner();

        Ok(())
    }

    fn is_terminal(&self) -> bool { self.winner.is_some() }
    fn outcome(&self) -> Option<GameOutcome> { self.winner.map(GameOutcome::Winner) }
    fn turn_number(&self) -> u32 { self.turn }
    fn num_players(&self) -> usize { 2 }
}
