use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::PokerView;

/// Random agent — picks a random valid action each turn.
#[derive(Debug)]
pub struct RandomAgent {
    name: String,
    player: Player,
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
}


impl RandomAgent {
    fn compute_command(&mut self, view: &PokerView) -> Action {
        let valid = view.valid_actions();
        if valid.is_empty() { return Action::Fold; }
        let idx = self.xorshift() as usize % valid.len();
        valid[idx].clone()
    }
}

impl GameAgent<PokerView, Action> for RandomAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &PokerView) {}

    fn decide(
        &mut self,
        _view: &PokerView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        let leaves = tree.flatten();
        if leaves.is_empty() { return None; }
        let idx = (self.xorshift() as usize) % leaves.len();
        Some(leaves[idx].clone())
    }
}
