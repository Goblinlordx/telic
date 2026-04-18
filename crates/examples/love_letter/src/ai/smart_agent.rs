use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::{Card, PlayCommand};
use crate::game::state::LoveLetterView;

/// Deduction-based Love Letter agent using card tracking and goal-oriented play.
///
/// Beliefs:
/// - Track all visible cards (discards + our hand) to narrow opponent's possibilities
/// - Use Priest peek info when available
/// - Know when opponent is protected (Handmaid)
///
/// Goals:
/// 1. Eliminate opponent (Guard guess, Baron compare)
/// 2. Protect self (Handmaid, avoid playing Princess)
/// 3. Gain information (Priest)
/// 4. Disrupt opponent (Prince, King)
/// 5. Keep high cards for endgame (deck running out = highest hand wins)
#[derive(Debug)]
pub struct DeductionAgent {
    name: String,
    player: PlayerIndex,
    #[allow(dead_code)]
    seed: u64,
    /// What we know about opponent's card (from Priest or deduction).
    known_opponent: Option<Card>,
    /// Cards we've seen played or are in our hand — everything else could be opponent's.
    seen_cards: Vec<Card>,
}

impl DeductionAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            known_opponent: None,
            seen_cards: Vec::new(),
        }
    }

    #[allow(dead_code)]
    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// What cards could the opponent have? Returns possible cards with counts.
    fn possible_opponent_cards(&self, view: &LoveLetterView) -> Vec<(Card, u8)> {
        // Start with full deck counts, subtract everything visible
        let mut remaining = std::collections::HashMap::new();
        for &card in Card::all_types() {
            remaining.insert(card, card.count_in_deck());
        }

        // Remove cards in our hand
        for &c in &view.hand {
            *remaining.entry(c).or_insert(0) -= 1;
        }

        // Remove cards in both discard piles
        for pile in &view.discard_piles {
            for &c in pile {
                *remaining.entry(c).or_insert(0) -= 1;
            }
        }

        // Remove 1 for the set-aside card (unknown, but reduces total)
        // We don't know which card, so we can't remove a specific one

        remaining.into_iter()
            .filter(|&(_, count)| count > 0)
            .map(|(card, count)| (card, count as u8))
            .collect()
    }

    /// If we know the opponent's card, or can deduce it.
    fn deduce_opponent(&self, view: &LoveLetterView) -> Option<Card> {
        // Direct knowledge from Priest
        if let Some(known) = view.known_opponent_card {
            return Some(known);
        }
        if let Some(known) = self.known_opponent {
            return Some(known);
        }

        // Deduction: if only 1 possible card remains, we know it
        let possible = self.possible_opponent_cards(view);
        // Account for set-aside card: total possible should be opponent_hand(1) + deck + set_aside(1)
        // If there's exactly 1 type of card with count matching what's unaccounted for... complex.
        // Simple version: if only 1 card type possible, that's it
        if possible.len() == 1 {
            return Some(possible[0].0);
        }

        None
    }

    fn choose_guard_guess(&mut self, view: &LoveLetterView) -> Card {
        // If we know their card, guess it
        if let Some(known) = self.deduce_opponent(view) {
            if known != Card::Guard {
                return known;
            }
        }

        // Otherwise, guess the most likely card they could have
        let possible = self.possible_opponent_cards(view);
        let mut best_card = Card::Princess; // default high-value guess
        let mut best_count = 0u8;

        for (card, count) in &possible {
            if *card != Card::Guard && *count > best_count {
                best_count = *count;
                best_card = *card;
            }
        }

        best_card
    }

    fn score_play(&mut self, card: Card, other_card: Card, view: &LoveLetterView) -> f32 {
        let mut score = 0.0f32;
        let opponent_protected = view.opponent_protected;
        let known_opp = self.deduce_opponent(view);
        let deck_low = view.deck_remaining <= 2;

        // NEVER play Princess
        if card == Card::Princess {
            return -1000.0;
        }

        // Countess rule — if forced, just play it
        if card == Card::Countess
            && (other_card == Card::King || other_card == Card::Prince)
        {
            return 500.0; // forced play
        }

        match card {
            Card::Guard => {
                if opponent_protected {
                    score -= 5.0; // wasted, but Guard is low value so ok to dump
                } else if known_opp.is_some() && known_opp.unwrap() != Card::Guard {
                    score += 50.0; // guaranteed elimination!
                } else {
                    // Probabilistic guess — value based on chance of hitting
                    let possible = self.possible_opponent_cards(view);
                    let total: u8 = possible.iter().map(|(_, c)| c).sum();
                    let best_prob = possible.iter()
                        .filter(|(c, _)| *c != Card::Guard)
                        .map(|(_, count)| *count as f32 / total as f32)
                        .max_by(|a, b| a.partial_cmp(b).unwrap())
                        .unwrap_or(0.0);
                    score += best_prob * 20.0;
                }
                // Guards are expendable — slight bonus for playing them
                score += 3.0;
            }
            Card::Priest => {
                if opponent_protected || known_opp.is_some() {
                    score -= 2.0; // useless if protected or already know
                } else {
                    score += 10.0; // information is valuable
                }
            }
            Card::Baron => {
                if opponent_protected {
                    score -= 5.0;
                } else if let Some(opp) = known_opp {
                    if other_card.value() > opp.value() {
                        score += 40.0; // guaranteed win!
                    } else if other_card.value() < opp.value() {
                        score -= 50.0; // guaranteed loss!
                    }
                } else {
                    // Risk/reward based on our remaining card value
                    if other_card.value() >= 5 {
                        score += 8.0; // likely to win comparison
                    } else {
                        score -= 5.0; // risky
                    }
                }
            }
            Card::Handmaid => {
                score += 5.0; // protection is always decent
                if deck_low {
                    score -= 3.0; // less useful late game
                }
            }
            Card::Prince => {
                if opponent_protected {
                    score -= 3.0;
                } else if known_opp == Some(Card::Princess) {
                    score += 60.0; // force them to discard Princess = elimination!
                } else {
                    score += 3.0; // disruption value
                }
            }
            Card::King => {
                if opponent_protected {
                    score -= 3.0;
                } else if let Some(opp) = known_opp {
                    if opp.value() > other_card.value() {
                        score += 15.0; // steal their better card
                    } else {
                        score -= 10.0; // give them our better card
                    }
                } else {
                    score -= 5.0; // risky without info
                }
            }
            Card::Countess => {
                score += 1.0; // safe play, no effect
                // Prefer keeping higher card for endgame
                if other_card.value() > card.value() {
                    score += 3.0;
                }
            }
            Card::Princess => {
                unreachable!() // handled above
            }
        }

        // Endgame: prefer to keep high cards in hand (highest hand wins)
        if deck_low {
            // Score for KEEPING the other card (which is what stays in hand)
            score += other_card.value() as f32 * 2.0;
        }

        score
    }
}


impl DeductionAgent {
    fn compute_command(&mut self, view: &LoveLetterView) -> PlayCommand {
        if view.hand.len() < 2 {
            let card = view.hand.first().copied().unwrap_or(Card::Guard);
            let guard_guess = if card == Card::Guard {
                Some(self.choose_guard_guess(view))
            } else { None };
            return PlayCommand { card, guard_guess };
        }

        let card_a = view.hand[0];
        let card_b = view.hand[1];

        // Check Countess rule
        let has_countess = card_a == Card::Countess || card_b == Card::Countess;
        let has_king_or_prince = view.hand.contains(&Card::King) || view.hand.contains(&Card::Prince);
        if has_countess && has_king_or_prince {
            return PlayCommand { card: Card::Countess, guard_guess: None };
        }

        // Score both options
        let score_a = self.score_play(card_a, card_b, view);
        let score_b = self.score_play(card_b, card_a, view);

        let chosen = if score_a > score_b { card_a } else { card_b };

        let guard_guess = if chosen == Card::Guard {
            Some(self.choose_guard_guess(view))
        } else {
            None
        };

        PlayCommand { card: chosen, guard_guess }
    }
}

impl GameAgent<LoveLetterView, PlayCommand> for DeductionAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.known_opponent = None;
        self.seen_cards.clear();
    }

    fn observe(&mut self, view: &LoveLetterView) {
        self.known_opponent = view.known_opponent_card;
        self.seen_cards.clear();
        for &c in &view.hand {
            self.seen_cards.push(c);
        }
        for pile in &view.discard_piles {
            for &c in pile {
                self.seen_cards.push(c);
            }
        }
    }

    fn decide(
        &mut self,
        view: &LoveLetterView,
        tree: &CommandTree<PlayCommand>,
    ) -> Option<PlayCommand> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
