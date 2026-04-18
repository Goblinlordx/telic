use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SplendorView;

/// Greedy agent — always buys the highest-point card it can afford.
/// If can't buy, takes gems toward the best available card.
/// Represents "competent but not strategic" play.
#[derive(Debug)]
pub struct GreedyAgent {
    name: String,
    player: PlayerIndex,
    #[allow(dead_code)]
    seed: u64,
}

impl GreedyAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self { name: name.into(), player: 0, seed: seed.max(1) }
    }

    #[allow(dead_code)]
    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// Score a card for purchasing priority.
    fn card_score(&self, card: &Card, view: &SplendorView) -> f64 {
        let mut score = card.points as f64 * 10.0;

        // Bonus toward nobles
        for noble in &view.nobles {
            let deficit = noble.required[card.bonus.index()]
                .saturating_sub(view.our_bonuses[card.bonus.index()]);
            if deficit > 0 {
                score += 3.0; // this bonus helps toward a noble
            }
        }

        // Prefer bonuses we don't have many of yet
        if view.our_bonuses[card.bonus.index()] == 0 {
            score += 2.0;
        }

        // Slight preference for cheaper cards (efficiency)
        let total_cost: u8 = card.cost.gems.iter().sum();
        score -= total_cost as f64 * 0.3;

        score
    }

    /// Find the best card to work toward (considering what we can almost afford).
    fn best_target(&self, view: &SplendorView) -> Option<(Card, Vec<Gem>)> {
        let mut best: Option<(f64, Card, Vec<Gem>)> = None;

        for tier in 0..3 {
            for card in &view.market[tier] {
                let score = self.card_score(card, view);
                let needed = self.gems_needed(card, view);

                // Prefer cards we're closer to affording
                let turns_away = needed.len() as f64;
                let adjusted = score - turns_away * 2.0;

                if best.as_ref().map_or(true, |(s, _, _)| adjusted > *s) {
                    best = Some((adjusted, card.clone(), needed));
                }
            }
        }

        best.map(|(_, card, needed)| (card, needed))
    }

    /// Which gems do we still need to buy a card?
    fn gems_needed(&self, card: &Card, view: &SplendorView) -> Vec<Gem> {
        let mut needed = Vec::new();
        for g in Gem::ALL {
            let effective = card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]);
            let have = view.our_tokens.get(g) + view.our_tokens.gold; // gold can substitute
            if effective > have {
                for _ in 0..(effective - have) {
                    needed.push(g);
                }
            }
        }
        needed
    }

    fn take_gems_toward(&mut self, needed: &[Gem], view: &SplendorView) -> Action {
        if view.our_tokens.total() + 3 > 10 {
            // Token limit — take 2 of something useful if possible
            for &g in needed {
                if view.bank.get(g) >= 4 && view.our_tokens.total() + 2 <= 10 {
                    return Action::TakeTwo(g);
                }
            }
            return Action::Pass;
        }

        // Take up to 3 different needed gems
        let mut to_take: Vec<Gem> = Vec::new();
        let mut used = [false; 5];

        for &g in needed {
            if to_take.len() >= 3 { break; }
            if !used[g.index()] && view.bank.get(g) > 0 {
                to_take.push(g);
                used[g.index()] = true;
            }
        }

        // Fill remaining slots with other available gems
        if to_take.len() < 3 {
            for g in Gem::ALL {
                if to_take.len() >= 3 { break; }
                if !used[g.index()] && view.bank.get(g) > 0 {
                    to_take.push(g);
                    used[g.index()] = true;
                }
            }
        }

        if to_take.len() == 3 {
            Action::TakeThree([to_take[0], to_take[1], to_take[2]])
        } else if to_take.len() >= 1 {
            // Can't take 3 different — try take 2
            for &g in &to_take {
                if view.bank.get(g) >= 4 && view.our_tokens.total() + 2 <= 10 {
                    return Action::TakeTwo(g);
                }
            }
            Action::Pass
        } else {
            Action::Pass
        }
    }
}


impl GreedyAgent {
    fn compute_command(&mut self, view: &SplendorView) -> Action {
        // 1. Buy the best card we can afford
        let mut best_buy: Option<(f64, u8, usize)> = None;

        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                let (can, _) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                if can {
                    let score = self.card_score(card, view);
                    if best_buy.as_ref().map_or(true, |(s, _, _)| score > *s) {
                        best_buy = Some((score, (tier + 1) as u8, idx));
                    }
                }
            }
        }

        // Also check reserved cards
        for (idx, card) in view.our_reserved.iter().enumerate() {
            let (can, _) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
            if can {
                let score = self.card_score(card, view);
                if best_buy.as_ref().map_or(true, |(s, _, _)| score > *s) {
                    return Action::BuyReserved { index: idx };
                }
            }
        }

        if let Some((_, tier, idx)) = best_buy {
            return Action::Buy { tier, index: idx };
        }

        // 2. Take gems toward best target card
        if let Some((_target, needed)) = self.best_target(view) {
            return self.take_gems_toward(&needed, view);
        }

        // 3. Fallback: take any available gems
        let available: Vec<Gem> = Gem::ALL.iter()
            .filter(|&&g| view.bank.get(g) > 0)
            .copied().collect();
        if available.len() >= 3 && view.our_tokens.total() + 3 <= 10 {
            Action::TakeThree([available[0], available[1], available[2]])
        } else {
            Action::Pass
        }
    }
}

impl GameAgent<SplendorView, Action> for GreedyAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &SplendorView) {}

    fn decide(
        &mut self,
        view: &SplendorView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
