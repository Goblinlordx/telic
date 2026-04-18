use std::collections::HashMap;
use std::time::{Duration, Instant};

use skillratings::MultiTeamOutcome;
use skillratings::trueskill::{TrueSkillConfig, TrueSkillRating, trueskill_multi_team};

use crate::arena::agent::GameAgent;
use crate::arena::game::{GameCommand, GameOutcome, GameState, GameView};
use crate::arena::CommandProvider;

/// An agent factory — creates fresh agent instances by type name.
pub trait AgentFactory<V: GameView, C: GameCommand>: std::fmt::Debug {
    fn type_name(&self) -> &str;
    fn create(&mut self) -> Box<dyn GameAgent<V, C>>;
}

/// Simple factory from a closure.
pub struct ClosureFactory<V: GameView, C: GameCommand> {
    name: String,
    factory: Box<dyn FnMut() -> Box<dyn GameAgent<V, C>>>,
}

impl<V: GameView, C: GameCommand> std::fmt::Debug for ClosureFactory<V, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Factory({})", self.name)
    }
}

impl<V: GameView, C: GameCommand> ClosureFactory<V, C> {
    pub fn new(name: impl Into<String>, f: impl FnMut() -> Box<dyn GameAgent<V, C>> + 'static) -> Self {
        Self { name: name.into(), factory: Box::new(f) }
    }
}

impl<V: GameView, C: GameCommand> AgentFactory<V, C> for ClosureFactory<V, C> {
    fn type_name(&self) -> &str { &self.name }
    fn create(&mut self) -> Box<dyn GameAgent<V, C>> { (self.factory)() }
}

/// Result of a single multi-player game.
#[derive(Debug, Clone)]
pub struct MultiGameResult {
    /// Agent type name per seat.
    pub seats: Vec<String>,
    /// Finish order: seat indices from 1st to last.
    pub finish_order: Vec<usize>,
    /// Winner seat index (first in finish order).
    pub winner: Option<usize>,
    pub turns: u32,
    pub duration: Duration,
}

/// Per-agent-type statistics.
#[derive(Debug, Clone)]
pub struct AgentTypeStats {
    pub type_name: String,
    pub rating_mu: f64,
    pub rating_sigma: f64,
    pub games_played: u32,
    pub wins: u32,
    pub mean_finish: f64,
}

/// Full evaluation report for multi-player arena.
#[derive(Debug)]
pub struct MultiPlayerReport {
    pub agent_stats: Vec<AgentTypeStats>,
    /// Pairwise win rates: matrix[a][b] = how often type a finished above type b
    pub pairwise: HashMap<(String, String), (u32, u32)>, // (wins_a, wins_b)
    pub total_games: u32,
    pub avg_turns: f64,
    pub avg_duration: Duration,
}

impl MultiPlayerReport {
    pub fn print_summary(&self) {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║              MULTI-PLAYER EVALUATION REPORT                 ║");
        println!("╠══════════════════════════════════════════════════════════════╣");
        println!("║  Games: {}  |  Avg turns: {:.1}", self.total_games, self.avg_turns);
        println!("╟──────────────────────────────────────────────────────────────╢");
        println!("║  TrueSkill Ratings:");

        let mut sorted: Vec<&AgentTypeStats> = self.agent_stats.iter().collect();
        sorted.sort_by(|a, b| b.rating_mu.partial_cmp(&a.rating_mu).unwrap());

        for (rank, stats) in sorted.iter().enumerate() {
            let win_pct = if stats.games_played > 0 {
                stats.wins as f64 / stats.games_played as f64 * 100.0
            } else { 0.0 };
            println!("║  #{} {:<16} μ={:.1} σ={:.1}  wins={} ({:.1}%)  avg_finish={:.2}  games={}",
                rank + 1, stats.type_name, stats.rating_mu, stats.rating_sigma,
                stats.wins, win_pct, stats.mean_finish, stats.games_played);
        }

        // Pairwise matrix
        if !self.pairwise.is_empty() {
            println!("╟──────────────────────────────────────────────────────────────╢");
            println!("║  Head-to-Head (row beat col %):");
            let types: Vec<String> = sorted.iter().map(|s| s.type_name.clone()).collect();
            print!("║  {:>12}", "");
            for t in &types { print!(" {:>8}", &t[..t.len().min(8)]); }
            println!();
            for a in &types {
                print!("║  {:>12}", &a[..a.len().min(12)]);
                for b in &types {
                    if a == b {
                        print!("     ---");
                    } else {
                        let key = (a.clone(), b.clone());
                        if let Some((wins_a, wins_b)) = self.pairwise.get(&key) {
                            let total = wins_a + wins_b;
                            if total > 0 {
                                print!("  {:>5.1}%", *wins_a as f64 / total as f64 * 100.0);
                            } else {
                                print!("     n/a");
                            }
                        } else {
                            print!("     n/a");
                        }
                    }
                }
                println!();
            }
        }

        println!("╚══════════════════════════════════════════════════════════════╝");
    }
}

/// Multi-player arena — runs N-player games with agent pools and TrueSkill ratings.
///
/// Supports 2+ players. Agent types are registered as factories.
/// The arena samples compositions and tracks ratings per type.
pub struct MultiPlayerArena<V: GameView, C: GameCommand> {
    factories: Vec<Box<dyn AgentFactory<V, C>>>,
    /// Number of players per game.
    pub players_per_game: usize,
    /// Total games to run.
    pub num_games: u32,
    pub max_turns: u32,
    pub max_retries: u32,
}

impl<V: GameView, C: GameCommand> MultiPlayerArena<V, C> {
    pub fn new(players_per_game: usize) -> Self {
        Self {
            factories: Vec::new(),
            players_per_game,
            num_games: 100,
            max_turns: 500,
            max_retries: 5,
        }
    }

    pub fn with_games(mut self, n: u32) -> Self { self.num_games = n; self }
    pub fn with_max_turns(mut self, n: u32) -> Self { self.max_turns = n; self }
    pub fn with_max_retries(mut self, n: u32) -> Self { self.max_retries = n; self }

    /// Register an agent type. The factory will be called to create instances.
    pub fn add_agent_type(mut self, factory: impl AgentFactory<V, C> + 'static) -> Self {
        self.factories.push(Box::new(factory));
        self
    }

    /// Run the evaluation. Each tick the `CommandProvider` enumerates the
    /// valid commands available to each player as a tree, and the agent
    /// picks one via [`GameAgent::decide`]. Non-acting players (empty
    /// trees) are skipped cleanly. Agents that correctly pick only from
    /// leaves of the tree never produce a command the game would reject.
    pub fn run<G, P>(self, game_factory: impl FnMut(usize) -> G) -> MultiPlayerReport
    where
        G: GameState<Command = C, View = V>,
        P: CommandProvider<State = G>,
    {
        self.run_internal(game_factory, |game, agent, player| {
            let tree = P::command_tree(game, player);
            if tree.is_empty() { return false; }
            let view = game.view_for(player);
            let Some(command) = agent.decide(&view, &tree) else {
                return false;
            };
            match game.apply_command(player, command) {
                Ok(()) => true,
                Err(reason) => {
                    agent.on_command_rejected(&reason);
                    false
                }
            }
        })
    }

    /// Shared book-keeping: TrueSkill updates, per-type stats, pairwise
    /// head-to-head. `step_player` is called for each (tick × player) and
    /// returns whether that player managed to accept a command.
    fn run_internal<G, S>(
        mut self,
        mut game_factory: impl FnMut(usize) -> G,
        mut step_player: S,
    ) -> MultiPlayerReport
    where
        G: GameState<Command = C, View = V>,
        S: FnMut(&mut G, &mut Box<dyn GameAgent<V, C>>, usize) -> bool,
    {
        let n_types = self.factories.len();
        assert!(n_types > 0, "No agent types registered");
        assert!(self.players_per_game >= 2, "Need at least 2 players");

        let ts_config = TrueSkillConfig::new();

        // TrueSkill ratings per agent type
        let mut ratings: Vec<TrueSkillRating> = (0..n_types)
            .map(|_| TrueSkillRating::new())
            .collect();

        let mut stats: Vec<AgentTypeStats> = self.factories.iter()
            .map(|f| AgentTypeStats {
                type_name: f.type_name().to_string(),
                rating_mu: 25.0,
                rating_sigma: 25.0 / 3.0,
                games_played: 0,
                wins: 0,
                mean_finish: 0.0,
            })
            .collect();

        let mut pairwise: HashMap<(String, String), (u32, u32)> = HashMap::new();
        let mut total_turns = 0u64;
        let mut total_duration = Duration::ZERO;
        let mut rng_seed = 12345u64;

        let max_turns = self.max_turns;
        let ppg = self.players_per_game;

        for _game_idx in 0..self.num_games {
            // Sample a composition: pick type for each seat
            let mut seat_types: Vec<usize> = Vec::with_capacity(ppg);
            for i in 0..ppg {
                rng_seed ^= rng_seed << 13;
                rng_seed ^= rng_seed >> 7;
                rng_seed ^= rng_seed << 17;
                let mut t = rng_seed as usize % n_types;
                // Ensure at least 2 different types when possible
                if i == 1 && t == seat_types[0] && n_types > 1 {
                    t = (t + 1) % n_types;
                }
                seat_types.push(t);
            }

            // Create agents for this game
            let mut agents: Vec<Box<dyn GameAgent<V, C>>> = seat_types.iter()
                .map(|&t| self.factories[t].create())
                .collect();

            // Create game
            let mut game = game_factory(ppg);

            // Reset agents
            for (i, agent) in agents.iter_mut().enumerate() {
                agent.reset(i);
            }

            let start = Instant::now();

            // Initial observation
            for i in 0..ppg {
                agents[i].observe(&game.view_for(i));
            }

            // Per-tick step: each player is asked once via the supplied
            // `step_player` closure. That closure owns the details of how
            // a command is produced (legacy retries on `decide`, or a
            // one-shot tree query via `decide_with_tree`) and whether it
            // was accepted by the game.
            while !game.is_terminal() && game.turn_number() < max_turns {
                let mut any_accepted = false;

                for player in 0..ppg {
                    if step_player(&mut game, &mut agents[player], player) {
                        any_accepted = true;
                    }
                }

                // Observe after all players have acted
                for i in 0..ppg {
                    agents[i].observe(&game.view_for(i));
                }

                // If no player accepted a command this tick, game is stuck
                if !any_accepted { break; }
            }

            let duration = start.elapsed();
            let turns = game.turn_number();
            total_turns += turns as u64;
            total_duration += duration;

            // Determine finish order from outcome
            let finish_order = Self::determine_finish_order(&game, ppg);
            let winner_seat = finish_order.first().copied();

            // Update TrueSkill ratings
            // Each player is a "team" of one
            let teams: Vec<Vec<TrueSkillRating>> = seat_types.iter()
                .map(|&t| vec![ratings[t]])
                .collect();
            let ranks: Vec<usize> = (0..ppg).map(|seat| {
                finish_order.iter().position(|&s| s == seat).unwrap_or(ppg - 1)
            }).collect();

            // trueskill_multi_team expects &[(&[TrueSkillRating], MultiTeamOutcome)]
            // where outcome is the rank (0 = first place)
            let team_refs: Vec<Vec<TrueSkillRating>> = teams;
            let mut input: Vec<(&[TrueSkillRating], skillratings::MultiTeamOutcome)> = Vec::new();
            for (i, team) in team_refs.iter().enumerate() {
                input.push((team.as_slice(), MultiTeamOutcome::new(ranks[i] + 1)));
            }

            if let Ok(new_ratings) = trueskill_multi_team(&input, &ts_config, None) {
                // Map updated ratings back to types
                for (i, new_team) in new_ratings.iter().enumerate() {
                    let type_idx = seat_types[i];
                    if let Some(r) = new_team.first() {
                        ratings[type_idx] = *r;
                    }
                }
            }

            // Update per-type stats
            for (seat, &type_idx) in seat_types.iter().enumerate() {
                stats[type_idx].games_played += 1;
                let finish_pos = finish_order.iter().position(|&s| s == seat)
                    .map(|p| p + 1).unwrap_or(ppg) as f64;
                let n = stats[type_idx].games_played as f64;
                stats[type_idx].mean_finish =
                    stats[type_idx].mean_finish * (n - 1.0) / n + finish_pos / n;

                if winner_seat == Some(seat) {
                    stats[type_idx].wins += 1;
                }
            }

            // Update pairwise
            for i in 0..ppg {
                for j in (i+1)..ppg {
                    let ti = seat_types[i];
                    let tj = seat_types[j];
                    if ti == tj { continue; }

                    let rank_i = ranks[i];
                    let rank_j = ranks[j];

                    let name_i = self.factories[ti].type_name().to_string();
                    let name_j = self.factories[tj].type_name().to_string();

                    let key = (name_i.clone(), name_j.clone());
                    let entry = pairwise.entry(key).or_insert((0, 0));
                    if rank_i < rank_j { entry.0 += 1; }
                    else if rank_j < rank_i { entry.1 += 1; }

                    let key_rev = (name_j, name_i);
                    let entry_rev = pairwise.entry(key_rev).or_insert((0, 0));
                    if rank_j < rank_i { entry_rev.0 += 1; }
                    else if rank_i < rank_j { entry_rev.1 += 1; }
                }
            }
        }

        // Finalize stats with ratings
        for (i, s) in stats.iter_mut().enumerate() {
            s.rating_mu = ratings[i].rating;
            s.rating_sigma = ratings[i].uncertainty;
        }

        MultiPlayerReport {
            agent_stats: stats,
            pairwise,
            total_games: self.num_games,
            avg_turns: total_turns as f64 / self.num_games.max(1) as f64,
            avg_duration: total_duration / self.num_games.max(1),
        }
    }

    fn determine_finish_order<G: GameState<Command = C, View = V>>(
        game: &G, num_players: usize,
    ) -> Vec<usize> {
        match game.outcome() {
            Some(GameOutcome::Winner(w)) => {
                // Winner first, then others
                let mut order = vec![w];
                for i in 0..num_players {
                    if i != w { order.push(i); }
                }
                order
            }
            _ => {
                // No clear winner — all tied
                (0..num_players).collect()
            }
        }
    }
}
