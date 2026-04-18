//! Shared propose-and-validate helper for Love Letter agents.

use telic::arena::CommandTree;
use crate::game::types::PlayCommand;

/// Propose a PlayCommand via the closure, verify it's a leaf in the tree.
/// If invalid, fall back to the first leaf in the tree (guaranteed valid).
/// Returns `None` only when the tree is empty.
pub fn propose_and_validate<F: FnMut() -> PlayCommand>(
    tree: &CommandTree<PlayCommand>,
    mut propose: F,
) -> Option<PlayCommand> {
    if tree.is_empty() { return None; }
    let proposed = propose();
    let leaves = tree.flatten();
    if leaves.iter().any(|c| *c == proposed) {
        Some(proposed)
    } else {
        leaves.first().cloned()
    }
}
