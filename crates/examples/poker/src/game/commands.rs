//! `CommandProvider` for Poker — reuses the existing `valid_actions()`
//! helper on `PokerView` to enumerate betting actions.

use std::sync::Arc;

use telic::arena::{CommandProvider, CommandTree, PlayerIndex};

use super::state::{PokerGame, PokerView};
use super::types::Action;

pub struct PokerCommands;

impl CommandProvider for PokerCommands {
    type State = PokerGame;

    fn command_tree(
        state: &PokerGame,
        player: PlayerIndex,
    ) -> Arc<CommandTree<Action>> {
        use telic::arena::GameState;
        Arc::new(build_tree(&state.view_for(player)))
    }
}

fn build_tree(view: &PokerView) -> CommandTree<Action> {
    if !view.is_our_turn {
        return CommandTree::Empty;
    }
    let actions = view.valid_actions();
    if actions.is_empty() {
        return CommandTree::Empty;
    }

    let children: Vec<(String, Arc<CommandTree<Action>>)> = actions.into_iter()
        .map(|a| (action_key(&a), Arc::new(CommandTree::Leaf(a))))
        .collect();

    CommandTree::Layer { label: "actions".into(), children }
}

fn action_key(a: &Action) -> String {
    match a {
        Action::Fold => "fold".into(),
        Action::Check => "check".into(),
        Action::Call => "call".into(),
        Action::Raise(amt) => format!("raise_{}", amt),
        Action::AllIn => "all_in".into(),
    }
}
