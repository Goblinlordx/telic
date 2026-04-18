//! `CommandProvider` for Love Letter — enumerates every valid card play.
//!
//! Tree shape:
//! ```text
//! Layer("play")
//! ├── "countess" → Leaf (forced when Countess rule triggers)
//! ├── "<card>"   → Leaf for non-Guard cards
//! └── "guard"    → Layer of 7 guesses (all cards except Guard)
//! ```

use std::sync::Arc;

use telic::arena::{CommandProvider, CommandTree, PlayerIndex};

use super::state::{LoveLetterGame, LoveLetterView};
use super::types::{Card, PlayCommand};

pub struct LoveLetterCommands;

impl CommandProvider for LoveLetterCommands {
    type State = LoveLetterGame;

    fn command_tree(
        state: &LoveLetterGame,
        player: PlayerIndex,
    ) -> Arc<CommandTree<PlayCommand>> {
        use telic::arena::GameState;
        Arc::new(build_tree(&state.view_for(player)))
    }
}

fn build_tree(view: &LoveLetterView) -> CommandTree<PlayCommand> {
    if !view.is_our_turn { return CommandTree::Empty; }
    // Should have 2 cards in hand during turn. If not, game state is off.
    if view.hand.len() != 2 { return CommandTree::Empty; }

    // Countess rule: must play Countess when also holding King or Prince.
    let has_countess = view.hand.contains(&Card::Countess);
    let has_king_or_prince =
        view.hand.contains(&Card::King) || view.hand.contains(&Card::Prince);
    let countess_forced = has_countess && has_king_or_prince;

    let mut children: Vec<(String, Arc<CommandTree<PlayCommand>>)> = Vec::new();

    for &card in &view.hand {
        if countess_forced && card != Card::Countess { continue; }

        if card == Card::Guard {
            // Guard needs a guess — one leaf per possible guess (any card except Guard).
            let mut guard_children: Vec<(String, Arc<CommandTree<PlayCommand>>)> = Vec::new();
            for &guess in Card::all_types() {
                if guess == Card::Guard { continue; }
                guard_children.push((
                    format!("{}", guess),
                    Arc::new(CommandTree::Leaf(PlayCommand {
                        card: Card::Guard,
                        guard_guess: Some(guess),
                    })),
                ));
            }
            if !guard_children.is_empty() {
                children.push((
                    "guard".into(),
                    Arc::new(CommandTree::Layer {
                        label: "guard_guess".into(),
                        children: guard_children,
                    }),
                ));
            }
        } else {
            children.push((
                card_key(card).into(),
                Arc::new(CommandTree::Leaf(PlayCommand {
                    card,
                    guard_guess: None,
                })),
            ));
        }
    }

    if children.is_empty() {
        return CommandTree::Empty;
    }

    CommandTree::Layer { label: "play".into(), children }
}

fn card_key(c: Card) -> &'static str {
    match c {
        Card::Guard => "guard",
        Card::Priest => "priest",
        Card::Baron => "baron",
        Card::Handmaid => "handmaid",
        Card::Prince => "prince",
        Card::King => "king",
        Card::Countess => "countess",
        Card::Princess => "princess",
    }
}
