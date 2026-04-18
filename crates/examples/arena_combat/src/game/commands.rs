//! `CommandProvider` for Arena Combat — trivial placeholder.
//!
//! Arena Combat is a real-time game where each tick accepts a `Vec<Command>`
//! batch from each player. The combinatorial per-unit action space doesn't
//! fit the tree API naturally; we provide a degenerate tree (one leaf
//! containing an empty batch) purely to satisfy the framework contract.
//! Agents ignore the tree and return their own batch via `decide`.

use std::sync::Arc;

use telic::arena::{CommandProvider, CommandTree, PlayerIndex};

use super::state::ArenaCombatGame;
use super::types::Command;

pub struct ArenaCombatCommands;

impl CommandProvider for ArenaCombatCommands {
    type State = ArenaCombatGame;

    fn command_tree(
        state: &ArenaCombatGame,
        _player: PlayerIndex,
    ) -> Arc<CommandTree<Vec<Command>>> {
        use telic::arena::GameState;
        if state.is_terminal() {
            Arc::new(CommandTree::Empty)
        } else {
            // Placeholder: agent's decide ignores this and returns its own batch.
            Arc::new(CommandTree::Leaf(Vec::new()))
        }
    }
}
