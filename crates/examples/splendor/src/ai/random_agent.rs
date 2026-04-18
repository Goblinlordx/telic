use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SplendorView;

#[derive(Debug)]
pub struct RandomAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
}

impl RandomAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self { name: name.into(), player: 0, seed: seed.max(1) }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn random_take_three(&mut self, view: &SplendorView) -> Option<Action> {
        let available: Vec<Gem> = Gem::ALL.iter()
            .filter(|&&g| view.bank.get(g) > 0)
            .copied().collect();
        if available.len() >= 3 && view.our_tokens.total() + 3 <= 10 {
            let i = self.xorshift() as usize % available.len();
            let mut j = self.xorshift() as usize % available.len();
            while j == i { j = self.xorshift() as usize % available.len(); }
            let mut k = self.xorshift() as usize % available.len();
            while k == i || k == j { k = self.xorshift() as usize % available.len(); }
            Some(Action::TakeThree([available[i], available[j], available[k]]))
        } else {
            None
        }
    }
}


impl RandomAgent {
    fn compute_command(&mut self, view: &SplendorView) -> Action {
        // Try to buy something random
        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                let (can, _) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                if can && self.xorshift() % 3 == 0 {
                    return Action::Buy { tier: (tier + 1) as u8, index: idx };
                }
            }
        }

        // Otherwise take gems
        if let Some(action) = self.random_take_three(view) {
            return action;
        }

        Action::Pass
    }
}

impl GameAgent<SplendorView, Action> for RandomAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &SplendorView) {}

    fn decide(
        &mut self,
        _view: &SplendorView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        let leaves = tree.flatten();
        if leaves.is_empty() { return None; }
        let i = (self.xorshift() as usize) % leaves.len();
        Some(leaves[i].clone())
    }
}
