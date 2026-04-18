use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::PokerView;

/// Tight-aggressive agent — only plays strong starting hands,
/// bets aggressively when it does play.
#[derive(Debug)]
pub struct TightAgent {
    name: String,
    player: Player,
}

impl TightAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), player: 0 }
    }

    fn preflop_strength(cards: &[Card; 2]) -> u32 {
        let high = cards[0].rank.0.max(cards[1].rank.0);
        let low = cards[0].rank.0.min(cards[1].rank.0);
        let suited = cards[0].suit == cards[1].suit;
        let pair = cards[0].rank == cards[1].rank;
        let gap = high - low;

        if pair { return 40 + (low as u32) * 5; }

        let mut score = (high as u32) * 3 + (low as u32);
        if suited { score += 8; }
        if gap <= 1 { score += 5; }
        if gap <= 2 { score += 2; }
        score
    }

    fn postflop_strength(hole: &[Card; 2], community: &[Card]) -> f64 {
        if community.is_empty() { return 0.5; }

        let mut all_cards: Vec<Card> = hole.iter().copied().collect();
        all_cards.extend_from_slice(community);

        let rank = evaluate_hand(&all_cards);

        match rank {
            HandRank::StraightFlush(_) => 1.0,
            HandRank::FourOfAKind(_, _) => 0.98,
            HandRank::FullHouse(_, _) => 0.95,
            HandRank::Flush(_, _, _, _, _) => 0.90,
            HandRank::Straight(_) => 0.85,
            HandRank::ThreeOfAKind(_, _, _) => 0.75,
            HandRank::TwoPair(_, _, _) => 0.65,
            HandRank::Pair(p, _, _, _) => {
                if p >= 10 { 0.50 } else { 0.35 }
            }
            HandRank::HighCard(h, _, _, _, _) => {
                if h >= 12 { 0.25 } else { 0.15 }
            }
        }
    }

    /// Pick the best action from valid options based on hand strength.
    fn pick_action(&self, view: &PokerView, want_raise: bool, want_call: bool) -> Action {
        let valid = view.valid_actions();

        if want_raise {
            // Try raise first, then all-in, then call, then check
            if let Some(a) = valid.iter().find(|a| matches!(a, Action::Raise(_))) {
                return a.clone();
            }
            if let Some(a) = valid.iter().find(|a| matches!(a, Action::AllIn)) {
                return a.clone();
            }
        }

        if want_call {
            if let Some(a) = valid.iter().find(|a| matches!(a, Action::Call)) {
                return a.clone();
            }
        }

        // Default: check if possible, else fold
        if let Some(a) = valid.iter().find(|a| matches!(a, Action::Check)) {
            return a.clone();
        }

        // Last resort
        valid.first().cloned().unwrap_or(Action::Fold)
    }
}


impl TightAgent {
    fn compute_command(&mut self, view: &PokerView) -> Action {
        match view.street {
            Street::Preflop => {
                let strength = Self::preflop_strength(&view.hole_cards);

                if strength >= 70 {
                    self.pick_action(view, true, true)
                } else if strength >= 45 {
                    self.pick_action(view, false, true)
                } else {
                    // Weak hand — check or fold (don't call large bets)
                    if view.to_call == 0 {
                        self.pick_action(view, false, false)
                    } else if view.to_call <= view.min_raise {
                        self.pick_action(view, false, true)
                    } else {
                        Action::Fold
                    }
                }
            }
            _ => {
                let strength = Self::postflop_strength(&view.hole_cards, &view.community);

                if strength >= 0.75 {
                    self.pick_action(view, true, true)
                } else if strength >= 0.45 {
                    if view.to_call <= view.pot / 2 {
                        self.pick_action(view, false, true)
                    } else {
                        Action::Fold
                    }
                } else {
                    if view.to_call > 0 { Action::Fold }
                    else { self.pick_action(view, false, false) }
                }
            }
        }
    }
}

impl GameAgent<PokerView, Action> for TightAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &PokerView) {}

    fn decide(
        &mut self,
        view: &PokerView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
