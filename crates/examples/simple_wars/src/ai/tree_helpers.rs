//! Shared helpers used by SimpleWars agents during the migration to the
//! command-tree API. Lets complex agents keep their internal planning
//! logic (`decide(view)`) and validate its output against the tree
//! rather than duplicating enumeration code in every agent.

use telic::arena::CommandTree;
use crate::game::types::Command;

/// Propose a command via the closure, then validate it against the tree.
/// If the proposal is a leaf in the tree, return it. Otherwise fall back
/// to the tree's `EndTurn` leaf (always present when the tree is non-empty
/// on a turn-based game).
///
/// Returns `None` only when the tree is `Empty` (not our turn).
pub fn propose_and_validate<F: FnMut() -> Command>(
    tree: &CommandTree<Command>,
    mut propose: F,
) -> Option<Command> {
    if tree.is_empty() { return None; }
    let proposed = propose();
    let leaves = tree.flatten();
    if leaves.iter().any(|c| *c == proposed) {
        Some(proposed)
    } else {
        tree.find_leaf(|c| matches!(c, Command::EndTurn)).cloned()
    }
}
