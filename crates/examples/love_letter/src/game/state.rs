use telic::arena::{GameState, GameOutcome, PlayerIndex, GameView};
use super::types::{Card, PlayCommand, Player};

/// What a player can see.
#[derive(Debug, Clone)]
pub struct LoveLetterView {
    pub viewer: Player,
    pub turn: u32,
    /// True when it's this viewer's turn to act.
    pub is_our_turn: bool,
    /// Our current hand (1 card normally, 2 when it's our turn to choose).
    pub hand: Vec<Card>,
    /// Cards played by each player (face up, visible to all).
    pub discard_piles: [Vec<Card>; 2],
    /// Is the opponent currently protected by Handmaid?
    pub opponent_protected: bool,
    /// Is the opponent eliminated?
    pub opponent_eliminated: bool,
    /// Cards remaining in draw pile (count only, not values).
    pub deck_remaining: usize,
    /// The card set aside face-down at game start (unknown).
    pub has_set_aside: bool,
    /// If we peeked at opponent's hand via Priest, we know it.
    pub known_opponent_card: Option<Card>,
}

impl GameView for LoveLetterView {
    fn viewer(&self) -> PlayerIndex { self.viewer }
    fn turn(&self) -> u32 { self.turn }
}

/// Full game state.
#[derive(Debug, Clone)]
pub struct LoveLetterGame {
    deck: Vec<Card>,
    hands: [Vec<Card>; 2],       // each player holds 1 card (2 during their turn)
    discards: [Vec<Card>; 2],     // face-up played cards
    protected: [bool; 2],         // Handmaid protection
    eliminated: [bool; 2],
    turn: u32,
    current_player: Player,
    winner: Option<Player>,
    #[allow(dead_code)]
    set_aside: Card,              // 1 card removed face-down at start
    /// If a player peeked via Priest, they know the other's card.
    peeked: [Option<Card>; 2],    // peeked[0] = what player 0 knows about player 1
}

impl LoveLetterGame {
    pub fn new(seed: u64) -> Self {
        let mut deck = Self::make_deck();
        Self::shuffle(&mut deck, seed);

        // Set aside 1 card face-down
        let set_aside = deck.pop().unwrap();

        // Deal 1 card to each player
        let hand0 = vec![deck.pop().unwrap()];
        let hand1 = vec![deck.pop().unwrap()];

        let mut game = Self {
            deck,
            hands: [hand0, hand1],
            discards: [Vec::new(), Vec::new()],
            protected: [false, false],
            eliminated: [false, false],
            turn: 0,
            current_player: 0,
            winner: None,
            set_aside,
            peeked: [None, None],
        };

        // Draw a card for the first player (they need 2 to choose from)
        game.draw_for_current_player();
        game
    }

    fn make_deck() -> Vec<Card> {
        let mut deck = Vec::new();
        for &card in Card::all_types() {
            for _ in 0..card.count_in_deck() {
                deck.push(card);
            }
        }
        deck
    }

    fn shuffle(deck: &mut Vec<Card>, seed: u64) {
        let mut rng = seed.max(1);
        for i in (1..deck.len()).rev() {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let j = rng as usize % (i + 1);
            deck.swap(i, j);
        }
    }

    fn draw_for_current_player(&mut self) {
        if let Some(card) = self.deck.pop() {
            self.hands[self.current_player].push(card);
        }
    }

    fn check_countess_rule(&self, player: Player) -> bool {
        // Must play Countess if also holding King or Prince
        let hand = &self.hands[player];
        let has_countess = hand.contains(&Card::Countess);
        let has_king_or_prince = hand.contains(&Card::King) || hand.contains(&Card::Prince);
        has_countess && has_king_or_prince
    }

    fn eliminate(&mut self, player: Player) {
        self.eliminated[player] = true;
        // Discard their hand
        let cards: Vec<Card> = self.hands[player].drain(..).collect();
        self.discards[player].extend(cards);
    }

    fn resolve_play(&mut self, player: Player, card: Card, guard_guess: Option<Card>) {
        let opponent = 1 - player;

        // Clear protection from previous turn
        self.protected[player] = false;

        // Discard the played card
        self.discards[player].push(card);

        // Resolve effect
        match card {
            Card::Guard => {
                if !self.protected[opponent] && !self.eliminated[opponent] {
                    if let Some(guess) = guard_guess {
                        if guess != Card::Guard && !self.hands[opponent].is_empty()
                            && self.hands[opponent][0] == guess
                        {
                            self.eliminate(opponent);
                        }
                    }
                }
            }
            Card::Priest => {
                if !self.protected[opponent] && !self.eliminated[opponent] {
                    if !self.hands[opponent].is_empty() {
                        self.peeked[player] = Some(self.hands[opponent][0]);
                    }
                }
            }
            Card::Baron => {
                if !self.protected[opponent] && !self.eliminated[opponent] {
                    if !self.hands[player].is_empty() && !self.hands[opponent].is_empty() {
                        let my_val = self.hands[player][0].value();
                        let their_val = self.hands[opponent][0].value();
                        if my_val > their_val {
                            self.eliminate(opponent);
                        } else if their_val > my_val {
                            self.eliminate(player);
                        }
                        // Tie = nothing happens
                    }
                }
            }
            Card::Handmaid => {
                self.protected[player] = true;
            }
            Card::Prince => {
                if !self.protected[opponent] && !self.eliminated[opponent] {
                    // Opponent discards and draws
                    if !self.hands[opponent].is_empty() {
                        let discarded = self.hands[opponent].remove(0);
                        self.discards[opponent].push(discarded);
                        if discarded == Card::Princess {
                            self.eliminate(opponent);
                        } else if let Some(new_card) = self.deck.pop() {
                            self.hands[opponent].push(new_card);
                        }
                    }
                }
            }
            Card::King => {
                if !self.protected[opponent] && !self.eliminated[opponent] {
                    // Swap hands
                    self.hands.swap(0, 1);
                    // Fix: if player is 0, hands are now swapped correctly
                    // But we need to handle this based on indices
                    if player == 0 {
                        // hands[0] and hands[1] already swapped
                    } else {
                        // Need to swap back — actually the simple swap works for both
                        self.hands.swap(0, 1);
                        // Wait, let me just do it right
                    }
                    // Actually, swap is symmetric. Let me just swap the contents.
                    let temp = self.hands[player].clone();
                    self.hands[player] = self.hands[opponent].clone();
                    self.hands[opponent] = temp;

                    // Clear peek knowledge since hands changed
                    self.peeked[0] = None;
                    self.peeked[1] = None;
                }
            }
            Card::Countess => {
                // No effect — just played to satisfy the rule
            }
            Card::Princess => {
                // Playing Princess eliminates yourself
                self.eliminate(player);
            }
        }
    }

    fn check_game_over(&mut self) {
        // Someone eliminated
        if self.eliminated[0] && !self.eliminated[1] {
            self.winner = Some(1);
        } else if self.eliminated[1] && !self.eliminated[0] {
            self.winner = Some(0);
        } else if self.eliminated[0] && self.eliminated[1] {
            // Both eliminated (shouldn't happen normally) — draw, give to p0
            self.winner = Some(0);
        }

        // Deck empty — compare remaining hand values
        if self.winner.is_none() && self.deck.is_empty() {
            let v0 = self.hands[0].first().map(|c| c.value()).unwrap_or(0);
            let v1 = self.hands[1].first().map(|c| c.value()).unwrap_or(0);
            if v0 > v1 {
                self.winner = Some(0);
            } else if v1 > v0 {
                self.winner = Some(1);
            } else {
                // Tie — compare total discard values
                let d0: u8 = self.discards[0].iter().map(|c| c.value()).sum();
                let d1: u8 = self.discards[1].iter().map(|c| c.value()).sum();
                self.winner = Some(if d0 >= d1 { 0 } else { 1 });
            }
        }
    }
}

impl GameState for LoveLetterGame {
    type Command = PlayCommand;
    type View = LoveLetterView;

    fn view_for(&self, player: PlayerIndex) -> LoveLetterView {
        let opponent = 1 - player;
        LoveLetterView {
            viewer: player,
            turn: self.turn,
            is_our_turn: self.current_player == player,
            hand: self.hands[player].clone(),
            discard_piles: self.discards.clone(),
            opponent_protected: self.protected[opponent],
            opponent_eliminated: self.eliminated[opponent],
            deck_remaining: self.deck.len(),
            has_set_aside: true,
            known_opponent_card: self.peeked[player],
        }
    }

    fn apply_command(&mut self, player: PlayerIndex, command: PlayCommand) -> Result<(), String> {
        if self.winner.is_some() {
            return Err("Game is over".into());
        }
        if player != self.current_player {
            return Err("Not your turn".into());
        }

        // Must have 2 cards in hand (drawn card + existing)
        if self.hands[player].len() != 2 {
            return Err("Hand should have 2 cards during turn".into());
        }

        // Validate card is in hand
        let pos = self.hands[player].iter().position(|&c| c == command.card)
            .ok_or("Card not in hand")?;

        // Check Countess rule
        if self.check_countess_rule(player) && command.card != Card::Countess {
            return Err("Must play Countess when holding King or Prince".into());
        }

        // Validate Guard guess
        if command.card == Card::Guard {
            if let Some(guess) = command.guard_guess {
                if guess == Card::Guard {
                    return Err("Cannot guess Guard".into());
                }
            }
        }

        // Remove card from hand
        self.hands[player].remove(pos);

        // Resolve
        self.resolve_play(player, command.card, command.guard_guess);
        self.check_game_over();

        // Next turn
        if self.winner.is_none() {
            self.current_player = 1 - self.current_player;
            self.turn += 1;

            // Draw for next player if not eliminated and deck has cards
            if !self.eliminated[self.current_player] && !self.deck.is_empty() {
                self.draw_for_current_player();
            } else if self.deck.is_empty() {
                self.check_game_over();
            }
        }

        Ok(())
    }

    fn is_terminal(&self) -> bool { self.winner.is_some() }
    fn outcome(&self) -> Option<GameOutcome> { self.winner.map(GameOutcome::Winner) }
    fn turn_number(&self) -> u32 { self.turn }
    fn num_players(&self) -> usize { 2 }
}
