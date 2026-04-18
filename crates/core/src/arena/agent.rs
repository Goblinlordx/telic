use crate::arena::game::{GameView, GameCommand, PlayerIndex};
use crate::arena::command_tree::CommandTree;

/// How an agent interacts with a game.
///
/// The agent:
/// - Receives observations (game views)
/// - Decides what to do (internally using GOAP, HTN, ML, hardcoded logic, whatever)
/// - Produces commands for the game
///
/// Different agents playing the same game can have completely different
/// internal representations. One might use GOAP with 50 beliefs, another
/// might use a simple heuristic. The arena doesn't care — it just asks
/// for commands and measures results.
pub trait GameAgent<V: GameView, C: GameCommand>: std::fmt::Debug {
    /// Agent's display name (for reporting).
    fn name(&self) -> &str;

    /// Called at the start of a new game. Reset internal state.
    fn reset(&mut self, player: PlayerIndex);

    /// Called whenever the game state changes (own turn or observation).
    /// The agent can update its beliefs, trigger replanning, etc.
    fn observe(&mut self, view: &V);

    /// Called when it's this agent's turn. Given the tree of valid
    /// commands for this player at this tick, return the chosen command,
    /// or `None` when the tree is [`CommandTree::Empty`] (e.g. it's not
    /// the player's turn in a turn-based game).
    ///
    /// The agent can either traverse the tree hierarchically, flatten
    /// it and score leaves, or sample uniformly — see `docs/command_tree.md`
    /// for the common patterns. An agent that picks only from leaves the
    /// tree exposes will never produce a command the game would reject.
    fn decide(&mut self, view: &V, tree: &CommandTree<C>) -> Option<C>;

    /// Called when a command was rejected by the game. With the tree API
    /// this should never fire in correct code — the tree guarantees the
    /// agent selects only valid commands. Kept for the legacy `decide`
    /// path and as a diagnostic hook.
    fn on_command_rejected(&mut self, _reason: &str) {}

    /// Called when the game ends. Optional — for learning agents that
    /// need to process the outcome.
    fn on_game_over(&mut self, _outcome: &super::game::GameOutcome) {}
}
