use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::{Card, PlayCommand};
use crate::game::state::LoveLetterView;

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
}


impl RandomAgent {
    fn compute_command(&mut self, view: &LoveLetterView) -> PlayCommand {
        if view.hand.is_empty() {
            return PlayCommand { card: Card::Guard, guard_guess: None };
        }

        // Check Countess rule
        let has_countess = view.hand.contains(&Card::Countess);
        let has_king_or_prince = view.hand.contains(&Card::King) || view.hand.contains(&Card::Prince);
        if has_countess && has_king_or_prince {
            return PlayCommand { card: Card::Countess, guard_guess: None };
        }

        // Pick random card from hand
        let idx = self.xorshift() as usize % view.hand.len();
        let card = view.hand[idx];

        // If Princess, try to play the other card instead
        let card = if card == Card::Princess && view.hand.len() > 1 {
            view.hand[1 - idx]
        } else {
            card
        };

        // Guard guess: random non-Guard card
        let guard_guess = if card == Card::Guard {
            let guessable = [Card::Priest, Card::Baron, Card::Handmaid, Card::Prince,
                           Card::King, Card::Countess, Card::Princess];
            let g_idx = self.xorshift() as usize % guessable.len();
            Some(guessable[g_idx])
        } else {
            None
        };

        PlayCommand { card, guard_guess }
    }
}

impl GameAgent<LoveLetterView, PlayCommand> for RandomAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &LoveLetterView) {}

    fn decide(
        &mut self,
        _view: &LoveLetterView,
        tree: &CommandTree<PlayCommand>,
    ) -> Option<PlayCommand> {
        let leaves = tree.flatten();
        if leaves.is_empty() { return None; }
        let i = (self.xorshift() as usize) % leaves.len();
        Some(leaves[i].clone())
    }
}
