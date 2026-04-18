//! Shared propose-and-validate helper for Splendor agents.

use telic::arena::CommandTree;
use crate::game::types::Action;

/// Propose an action via the closure, verify it's a leaf in the tree,
/// and fall back to `Pass` otherwise. Returns `None` only on an empty tree.
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
        tree.find_leaf(|a| matches!(a, Action::Pass)).cloned()
    }
}
