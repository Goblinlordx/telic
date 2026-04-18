use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::{Card, PlayCommand};
use crate::game::state::LoveLetterView;

/// Probabilistic Love Letter agent using exact probability distributions.
///
/// Key techniques:
/// 1. Exact probability distribution for opponent's card
/// 2. Baron uses precise win probability, not just threshold
/// 3. Guard guess uses probability-weighted targeting
/// 4. Considers what our play reveals to the opponent
/// 5. Prince targets Princess holders specifically
#[derive(Debug)]
pub struct ProbabilisticAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
}

impl ProbabilisticAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// Compute exact probability distribution for opponent's card.
    /// Returns vec of (card, probability).
    fn opponent_distribution(&self, view: &LoveLetterView) -> Vec<(Card, f64)> {
        let mut counts = std::collections::HashMap::new();
        for &card in Card::all_types() {
            counts.insert(card, card.count_in_deck() as i32);
        }

        // Remove our hand
        for &c in &view.hand {
            *counts.entry(c).or_insert(0) -= 1;
        }

        // Remove all discarded cards
        for pile in &view.discard_piles {
            for &c in pile {
                *counts.entry(c).or_insert(0) -= 1;
            }
        }

        // Remove 1 unknown card for set-aside (reduces total pool but we don't know which)
        // We handle this by noting total unknowns = deck_remaining + 1 (set aside) + 1 (opponent)
        // Opponent has 1 of the remaining cards

        let total: i32 = counts.values().filter(|&&v| v > 0).sum();
        if total <= 0 {
            return Vec::new();
        }

        // If we know their card from Priest, return certainty
        if let Some(known) = view.known_opponent_card {
            return vec![(known, 1.0)];
        }

        counts.into_iter()
            .filter(|&(_, count)| count > 0)
            .map(|(card, count)| (card, count as f64 / total as f64))
            .collect()
    }

    /// Best Guard guess: the card with highest probability (excluding Guard).
    fn best_guard_guess(&self, dist: &[(Card, f64)]) -> Card {
        dist.iter()
            .filter(|(c, _)| *c != Card::Guard)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(c, _)| *c)
            .unwrap_or(Card::Princess)
    }

    /// Probability of winning a Baron comparison with our remaining card.
    fn baron_win_probability(&self, our_value: u8, dist: &[(Card, f64)]) -> f64 {
        let mut win_prob = 0.0;
        let mut lose_prob = 0.0;
        for &(card, prob) in dist {
            if our_value > card.value() {
                win_prob += prob;
            } else if card.value() > our_value {
                lose_prob += prob;
            }
        }
        // Return net advantage (win - lose), not just win probability
        win_prob - lose_prob
    }

    /// Probability that Guard guess hits.
    fn guard_hit_probability(&self, dist: &[(Card, f64)]) -> f64 {
        dist.iter()
            .filter(|(c, _)| *c != Card::Guard)
            .map(|(_, p)| p)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .copied()
            .unwrap_or(0.0)
    }

    /// Probability opponent has Princess (relevant for Prince play).
    fn princess_probability(&self, dist: &[(Card, f64)]) -> f64 {
        dist.iter()
            .find(|(c, _)| *c == Card::Princess)
            .map(|(_, p)| *p)
            .unwrap_or(0.0)
    }

    fn score_play(&mut self, card: Card, other_card: Card, view: &LoveLetterView) -> f64 {
        let dist = self.opponent_distribution(view);
        let opponent_protected = view.opponent_protected;
        let deck_low = view.deck_remaining <= 2;

        // NEVER play Princess
        if card == Card::Princess {
            return -1000.0;
        }

        // Countess rule
        if card == Card::Countess
            && (other_card == Card::King || other_card == Card::Prince)
        {
            return 500.0;
        }

        let mut score = 0.0f64;

        match card {
            Card::Guard => {
                if opponent_protected {
                    // Wasted effect, but Guards are cheap to dump
                    score += 2.0;
                } else {
                    let hit_prob = self.guard_hit_probability(&dist);
                    // Elimination value * probability
                    score += hit_prob * 40.0;
                    // Even a miss is ok — Guards are expendable
                    score += 2.0;
                }
            }
            Card::Priest => {
                if opponent_protected || view.known_opponent_card.is_some() {
                    score -= 1.0; // redundant
                } else {
                    // Information value: helps future Guard guesses and Baron decisions
                    // More valuable early in the game
                    score += 8.0 + view.deck_remaining as f64 * 1.5;
                }
            }
            Card::Baron => {
                if opponent_protected {
                    score -= 3.0;
                } else {
                    let net_advantage = self.baron_win_probability(other_card.value(), &dist);
                    // Positive = likely win, negative = likely lose
                    score += net_advantage * 35.0;
                }
            }
            Card::Handmaid => {
                // Protection value — higher when we have a good card to protect
                score += 4.0;
                if other_card.value() >= 6 {
                    score += 5.0; // protecting a King/Countess/Princess
                }
                if deck_low {
                    score -= 2.0; // less useful near endgame
                }
            }
            Card::Prince => {
                if opponent_protected {
                    score -= 2.0;
                } else {
                    let princess_prob = self.princess_probability(&dist);
                    // If they have Princess, Prince = instant win
                    score += princess_prob * 80.0;
                    // General disruption value
                    score += 3.0;
                    // Late game: forcing discard of a high card is good
                    if deck_low {
                        let avg_opponent_value: f64 = dist.iter()
                            .map(|(c, p)| c.value() as f64 * p)
                            .sum();
                        score += avg_opponent_value * 0.5;
                    }
                }
            }
            Card::King => {
                if opponent_protected {
                    score -= 3.0;
                } else {
                    // Expected value of swap
                    let avg_opponent: f64 = dist.iter()
                        .map(|(c, p)| c.value() as f64 * p)
                        .sum();
                    let our_kept = other_card.value() as f64;
                    // Swap is good if opponent likely has higher card
                    if avg_opponent > our_kept {
                        score += (avg_opponent - our_kept) * 5.0;
                    } else {
                        score -= (our_kept - avg_opponent) * 5.0;
                    }
                }
            }
            Card::Countess => {
                score += 1.0; // safe dump
            }
            Card::Princess => {
                unreachable!()
            }
        }

        // Endgame bonus: prefer to keep higher cards
        if deck_low {
            score += other_card.value() as f64 * 3.0;
        }

        // Slight preference for playing lower-value cards (keep high for endgame)
        score -= card.value() as f64 * 0.3;

        // Tiny random tiebreaker
        score += (self.xorshift() % 100) as f64 * 0.001;

        score
    }
}


impl ProbabilisticAgent {
    fn compute_command(&mut self, view: &LoveLetterView) -> PlayCommand {
        if view.hand.len() < 2 {
            let card = view.hand.first().copied().unwrap_or(Card::Guard);
            let guess = if card == Card::Guard {
                let dist = self.opponent_distribution(view);
                Some(self.best_guard_guess(&dist))
            } else { None };
            return PlayCommand { card, guard_guess: guess };
        }

        let card_a = view.hand[0];
        let card_b = view.hand[1];

        // Countess rule
        let has_countess = card_a == Card::Countess || card_b == Card::Countess;
        let has_kp = view.hand.contains(&Card::King) || view.hand.contains(&Card::Prince);
        if has_countess && has_kp {
            return PlayCommand { card: Card::Countess, guard_guess: None };
        }

        let score_a = self.score_play(card_a, card_b, view);
        let score_b = self.score_play(card_b, card_a, view);

        let chosen = if score_a > score_b { card_a } else { card_b };

        let guard_guess = if chosen == Card::Guard {
            let dist = self.opponent_distribution(view);
            Some(self.best_guard_guess(&dist))
        } else {
            None
        };

        PlayCommand { card: chosen, guard_guess }
    }
}

impl GameAgent<LoveLetterView, PlayCommand> for ProbabilisticAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
    }

    fn observe(&mut self, _view: &LoveLetterView) {}

    fn decide(
        &mut self,
        view: &LoveLetterView,
        tree: &CommandTree<PlayCommand>,
    ) -> Option<PlayCommand> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
