//! Game-agnostic evaluation arena for testing agents.
//!
//! # Usage
//!
//! 1. Implement `GameState` for your game
//! 2. Implement `GameAgent` for each AI you want to test
//! 3. Register agent types, run evaluation, get TrueSkill ratings
//!
//! ```ignore
//! let report = MultiPlayerArena::new(2)
//!     .with_games(1000)
//!     .add_agent_type(ClosureFactory::new("agent_a", || Box::new(AgentA::new())))
//!     .add_agent_type(ClosureFactory::new("agent_b", || Box::new(AgentB::new())))
//!     .run(|_num_players| MyGame::new());
//! report.print_summary();
//! ```

pub mod game;
pub mod agent;
pub mod multiplayer;
pub mod command_tree;

pub use game::{GameState, GameView, GameCommand, PlayerIndex, GameOutcome};
pub use agent::GameAgent;
pub use multiplayer::{MultiPlayerArena, MultiPlayerReport, AgentFactory, ClosureFactory};
pub use command_tree::{CommandTree, CommandBuilder, ParamDomain};

use std::sync::Arc;

/// Enumerates the valid commands available to a player from a given game state.
///
/// Implement this to opt a game into the command-tree API: agents that use
/// the tree are architecturally unable to propose a command the game would
/// reject. Implementers can provide this directly on a game type or via a
/// wrapper struct, so `GameState` itself stays free of this dependency.
///
/// See [`CommandTree`] for the tree shape.
pub trait CommandProvider {
    /// The game state this provider queries against.
    type State: GameState;

    /// Return the tree of commands the given player may issue from `state`.
    /// Return [`CommandTree::Empty`] when the player has no valid action
    /// (e.g. it's not their turn in a turn-based game).
    fn command_tree(
        state: &Self::State,
        player: PlayerIndex,
    ) -> Arc<CommandTree<<Self::State as GameState>::Command>>;
}
