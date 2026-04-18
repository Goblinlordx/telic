use std::fmt;

/// Index of a player in the game (0, 1, 2, ...).
pub type PlayerIndex = usize;

/// The outcome of a completed game.
#[derive(Debug, Clone)]
pub enum GameOutcome {
    /// A player won.
    Winner(PlayerIndex),
    /// The game ended in a draw.
    Draw,
}

/// A command issued by an agent to the game.
///
/// Commands are game-specific — the game defines what commands it accepts,
/// not the agent. The agent's job is to produce valid commands.
///
/// This is intentionally opaque to the arena. The arena doesn't inspect
/// commands, it just passes them from agent to game.
pub trait GameCommand: fmt::Debug + Clone {}

/// Blanket impl — any Debug + Clone type can be a command.
impl<T: fmt::Debug + Clone> GameCommand for T {}

/// What the game exposes to agents each turn.
///
/// This is NOT "all valid actions." This is the observable world state
/// from a specific player's perspective (respecting hidden information).
///
/// The agent decides what to do with this information based on its own
/// beliefs, goals, and action definitions.
pub trait GameView: fmt::Debug + Clone {
    /// Which player is this view for?
    fn viewer(&self) -> PlayerIndex;

    /// What turn number is it?
    fn turn(&self) -> u32;
}

/// The game interface — how the arena interacts with the game.
///
/// The game:
/// - Produces views (what each player can see)
/// - Accepts or rejects commands (what players want to do)
/// - Determines when the game is over and who won
///
/// Works for both turn-based and real-time games:
/// - **Turn-based**: `apply_command` rejects if it's not that player's turn.
///   The view should indicate whose turn it is.
/// - **Real-time**: `apply_command` accepts from all players each tick.
///   The game advances its simulation when appropriate.
///
/// The arena asks every player every tick. The game decides what to accept.
pub trait GameState: fmt::Debug {
    /// The command type this game accepts.
    type Command: GameCommand;
    /// The view type this game provides to agents.
    type View: GameView;

    /// Get the current view for a specific player.
    /// This respects hidden information — each player sees only what they should.
    fn view_for(&self, player: PlayerIndex) -> Self::View;

    /// Apply a command from a player. Returns Ok if valid, Err with reason if not.
    /// Turn-based games reject if it's not this player's turn.
    /// Real-time games accept from all players each tick.
    fn apply_command(&mut self, player: PlayerIndex, command: Self::Command) -> Result<(), String>;

    /// Is the game over?
    fn is_terminal(&self) -> bool;

    /// The outcome, if the game is over.
    fn outcome(&self) -> Option<GameOutcome>;

    /// Current turn/tick number.
    fn turn_number(&self) -> u32;

    /// How many players are in this game?
    fn num_players(&self) -> usize;
}
