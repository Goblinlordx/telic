//! Shared propose-and-validate helper for Poker agents.

use telic::arena::CommandTree;
use crate::game::types::Action;

/// Propose an Action via the closure, verify it's a leaf in the tree.
/// If invalid (shouldn't happen for correct agents), fall back to the first
/// leaf. Returns `None` when the tree is empty.
pub fn propose_and_validate<F: FnMut() -> Action>(
    tree: &CommandTree<Action>,
    mut propose: F,
) -> Option<Action> {
    if tree.is_empty() { return None; }
    let proposed = propose();
    let leaves = tree.flatten();
    if leaves.iter().any(|a| *a == proposed) {
        Some(proposed)
    } else {
        leaves.first().cloned()
    }
}
