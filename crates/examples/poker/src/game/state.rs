use telic::arena::{GameState, GameOutcome, PlayerIndex, GameView};
use super::types::*;

const STARTING_SMALL_BLIND: u32 = 25;
const STARTING_BIG_BLIND: u32 = 50;
const STARTING_CHIPS: u32 = 10000; // 200 big blinds — deep stacked tournament
/// Blinds increase every 20 hands (~WSOP pace)
const BLIND_INCREASE_INTERVAL: u32 = 20;

/// What a player can see — their hole cards + community cards + betting info.
#[derive(Debug, Clone)]
pub struct PokerView {
    pub viewer: Player,
    pub hand_number: u32,
    /// Our 2 hole cards.
    pub hole_cards: [Card; 2],
    /// Community cards (0 preflop, 3 flop, 4 turn, 5 river).
    pub community: Vec<Card>,
    /// Current betting street.
    pub street: Street,
    /// Our chip stack.
    pub our_chips: u32,
    /// Opponent's chip stack.
    pub opp_chips: u32,
    /// Current pot size.
    pub pot: u32,
    /// How much we have bet this round.
    pub our_bet: u32,
    /// How much opponent has bet this round.
    pub opp_bet: u32,
    /// Amount to call (opp_bet - our_bet).
    pub to_call: u32,
    /// Minimum raise amount.
    pub min_raise: u32,
    /// Are we the dealer (button) this hand? Dealer acts last post-flop.
    pub is_dealer: bool,
    /// True when it is this viewer's turn to act.
    pub is_our_turn: bool,
    /// Betting history for this hand: (street, player, action).
    pub history: Vec<(Street, Player, Action)>,
    /// Opponent's hole cards — only revealed at showdown.
    pub opp_hole_cards: Option<[Card; 2]>,
}

impl GameView for PokerView {
    fn viewer(&self) -> PlayerIndex { self.viewer }
    fn turn(&self) -> u32 { self.hand_number }
}

impl PokerView {
    /// Valid actions the player can take right now.
    pub fn valid_actions(&self) -> Vec<Action> {
        let mut actions = Vec::new();

        if self.to_call > 0 {
            actions.push(Action::Fold);
            actions.push(Action::Call);
            // Can raise if we have chips beyond the call
            let after_call = self.our_chips.saturating_sub(self.to_call);
            if after_call > 0 {
                let min_raise_to = self.opp_bet + self.min_raise;
                if min_raise_to <= self.our_chips + self.our_bet {
                    actions.push(Action::Raise(min_raise_to));
                }
                actions.push(Action::AllIn);
            }
        } else {
            actions.push(Action::Check);
            // Can bet (raise from 0)
            if self.our_chips > 0 {
                let min_raise_to = self.our_bet + self.min_raise;
                if min_raise_to <= self.our_chips + self.our_bet {
                    actions.push(Action::Raise(min_raise_to));
                }
                actions.push(Action::AllIn);
            }
        }

        actions
    }
}

/// Full game state for heads-up No Limit Hold'em.
///
/// A "game" is a series of hands. The match ends when one player
/// runs out of chips.
#[derive(Debug)]
pub struct PokerGame {
    deck: Vec<Card>,
    hole_cards: [[Card; 2]; 2],
    community: Vec<Card>,
    street: Street,
    chips: [u32; 2],
    pot: u32,
    bets: [u32; 2],      // current round bets
    current_player: Player,
    dealer: Player,       // button/dealer rotates each hand
    hand_number: u32,
    winner: Option<Player>,
    hand_over: bool,
    /// Number of actions in current betting round (to detect check-check)
    round_actions: u32,
    last_raiser: Option<Player>,
    total_actions: u32, // safety: cap actions per hand to prevent infinite raise loops
    history: Vec<(Street, Player, Action)>,
    seed: u64,
}

impl PokerGame {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            deck: Vec::new(),
            hole_cards: [[Card::new(Rank::TWO, Suit::Clubs); 2]; 2],
            community: Vec::new(),
            street: Street::Preflop,
            chips: [STARTING_CHIPS, STARTING_CHIPS],
            pot: 0,
            bets: [0, 0],
            current_player: 0,
            dealer: 0,
            hand_number: 0,
            winner: None,
            hand_over: false,
            round_actions: 0,
            last_raiser: None,
            total_actions: 0,
            history: Vec::new(),
            seed,
        };
        game.deal_hand();
        game
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn deal_hand(&mut self) {
        // Shuffle deck
        self.deck = standard_deck();
        shuffle(&mut self.deck, self.seed);
        self.xorshift(); // advance seed for next hand

        self.community.clear();
        self.street = Street::Preflop;
        self.bets = [0, 0];
        self.pot = 0;
        self.hand_over = false;
        self.round_actions = 0;
        self.last_raiser = None;
        self.total_actions = 0;
        self.history.clear();

        // Deal hole cards
        self.hole_cards[0] = [self.deck.pop().unwrap(), self.deck.pop().unwrap()];
        self.hole_cards[1] = [self.deck.pop().unwrap(), self.deck.pop().unwrap()];

        // Post blinds: dealer posts small blind, other posts big blind
        // Blinds escalate every BLIND_INCREASE_INTERVAL hands (tournament style)
        let (current_sb, current_bb) = self.current_blinds();

        let sb_player = self.dealer;
        let bb_player = 1 - self.dealer;

        let sb = current_sb.min(self.chips[sb_player]);
        let bb = current_bb.min(self.chips[bb_player]);

        self.chips[sb_player] -= sb;
        self.bets[sb_player] = sb;
        self.chips[bb_player] -= bb;
        self.bets[bb_player] = bb;
        self.pot = sb + bb;

        // Preflop: dealer (SB) acts first
        self.current_player = self.dealer;
    }

    fn advance_street(&mut self) {
        // Move bets to pot (already tracked in pot)
        self.bets = [0, 0];
        self.round_actions = 0;
        self.last_raiser = None;

        match self.street {
            Street::Preflop => {
                // Deal flop (3 cards)
                self.deck.pop(); // burn
                self.community.push(self.deck.pop().unwrap());
                self.community.push(self.deck.pop().unwrap());
                self.community.push(self.deck.pop().unwrap());
                self.street = Street::Flop;
            }
            Street::Flop => {
                self.deck.pop(); // burn
                self.community.push(self.deck.pop().unwrap());
                self.street = Street::Turn;
            }
            Street::Turn => {
                self.deck.pop(); // burn
                self.community.push(self.deck.pop().unwrap());
                self.street = Street::River;
            }
            Street::River => {
                self.street = Street::Showdown;
                self.resolve_showdown();
                return;
            }
            Street::Showdown => return,
        }

        // Post-flop: non-dealer acts first
        self.current_player = 1 - self.dealer;
    }

    fn resolve_showdown(&mut self) {
        let cards_0: Vec<Card> = self.hole_cards[0].iter()
            .chain(self.community.iter()).copied().collect();
        let cards_1: Vec<Card> = self.hole_cards[1].iter()
            .chain(self.community.iter()).copied().collect();

        let rank_0 = evaluate_hand(&cards_0);
        let rank_1 = evaluate_hand(&cards_1);

        if rank_0 > rank_1 {
            self.chips[0] += self.pot;
        } else if rank_1 > rank_0 {
            self.chips[1] += self.pot;
        } else {
            // Split pot
            self.chips[0] += self.pot / 2;
            self.chips[1] += self.pot / 2;
        }

        self.pot = 0;
        self.hand_over = true;
        self.start_next_hand();
    }

    fn fold_to(&mut self, winner: Player) {
        self.chips[winner] += self.pot;
        self.pot = 0;
        self.hand_over = true;
        self.start_next_hand();
    }

    fn start_next_hand(&mut self) {
        self.hand_number += 1;
        self.dealer = 1 - self.dealer;

        // Check if match is over (someone has 0 chips)
        if self.chips[0] == 0 {
            self.winner = Some(1);
        } else if self.chips[1] == 0 {
            self.winner = Some(0);
        } else {
            self.deal_hand();
        }
    }

    fn current_blinds(&self) -> (u32, u32) {
        let level = self.hand_number / BLIND_INCREASE_INTERVAL;
        let multiplier = 1u32 << level.min(6); // double each level, cap at 64x
        (STARTING_SMALL_BLIND * multiplier, STARTING_BIG_BLIND * multiplier)
    }

    fn is_betting_complete(&self) -> bool {
        if self.round_actions < 2 { return false; }
        // Betting complete when bets are equal, OR a player is all-in (can't add more)
        if self.bets[0] == self.bets[1] { return true; }
        // If either player has 0 chips, they can't match — round is done
        self.chips[0] == 0 || self.chips[1] == 0
    }
}

impl GameState for PokerGame {
    type Command = Action;
    type View = PokerView;

    fn view_for(&self, player: PlayerIndex) -> PokerView {
        let opp = 1 - player;
        PokerView {
            viewer: player,
            hand_number: self.hand_number,
            hole_cards: self.hole_cards[player],
            community: self.community.clone(),
            street: self.street,
            our_chips: self.chips[player],
            opp_chips: self.chips[opp],
            pot: self.pot,
            our_bet: self.bets[player],
            opp_bet: self.bets[opp],
            to_call: self.bets[opp].saturating_sub(self.bets[player]),
            min_raise: self.current_blinds().1,
            is_dealer: self.dealer == player,
            is_our_turn: self.current_player == player && !self.hand_over,
            history: self.history.clone(),
            opp_hole_cards: if self.street == Street::Showdown || self.hand_over {
                Some(self.hole_cards[opp])
            } else {
                None
            },
        }
    }

    fn apply_command(&mut self, player: PlayerIndex, action: Action) -> Result<(), String> {
        if self.winner.is_some() { return Err("Match is over".into()); }
        if player != self.current_player { return Err("Not your turn".into()); }

        self.total_actions += 1;

        // Safety: prevent infinite loops
        let action = if self.total_actions > 100 {
            let to_call = self.bets[1 - player].saturating_sub(self.bets[player]);
            if to_call > 0 { Action::Call } else { Action::Check }
        } else {
            action
        };

        let opp = 1 - player;
        let to_call = self.bets[opp].saturating_sub(self.bets[player]);

        match &action {
            Action::Fold => {
                self.history.push((self.street, player, action.clone()));
                self.fold_to(opp);
                return Ok(());
            }
            Action::Check => {
                if to_call > 0 { return Err("Can't check, must call or fold".into()); }
                self.history.push((self.street, player, action.clone()));
            }
            Action::Call => {
                if to_call == 0 { return Err("Nothing to call, use check".into()); }
                let actual = to_call.min(self.chips[player]);
                self.chips[player] -= actual;
                self.bets[player] += actual;
                self.pot += actual;
                self.history.push((self.street, player, action.clone()));
            }
            Action::Raise(amount) => {
                let total_bet = *amount;
                let current_bb = self.current_blinds().1;
                if total_bet < self.bets[opp] + current_bb && total_bet < self.chips[player] + self.bets[player] {
                    return Err("Raise too small".into());
                }
                let additional = total_bet.saturating_sub(self.bets[player]).min(self.chips[player]);
                self.chips[player] -= additional;
                self.bets[player] += additional;
                self.pot += additional;
                self.last_raiser = Some(player);
                self.round_actions = 1; // reset — opponent needs to respond
                self.history.push((self.street, player, action.clone()));
                self.current_player = opp;
                return Ok(());
            }
            Action::AllIn => {
                let all_chips = self.chips[player];
                self.chips[player] = 0;
                self.bets[player] += all_chips;
                self.pot += all_chips;

                if self.bets[player] > self.bets[opp] {
                    self.last_raiser = Some(player);
                    self.round_actions = 1;
                }

                self.history.push((self.street, player, action.clone()));

                // If both all-in or opponent already matched, run out community
                if self.chips[opp] == 0 || self.bets[player] <= self.bets[opp] {
                    // Run out remaining streets
                    while self.street != Street::Showdown && !self.hand_over {
                        self.advance_street();
                    }
                    return Ok(());
                }

                self.current_player = opp;
                return Ok(());
            }
        }

        self.round_actions += 1;

        // Check if betting round is complete
        if self.is_betting_complete() {
            self.advance_street();
        } else {
            self.current_player = opp;
        }

        Ok(())
    }

    fn is_terminal(&self) -> bool { self.winner.is_some() }
    fn outcome(&self) -> Option<GameOutcome> { self.winner.map(GameOutcome::Winner) }
    fn turn_number(&self) -> u32 { self.hand_number }
    fn num_players(&self) -> usize { 2 }
}
