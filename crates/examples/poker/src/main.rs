use telic::arena::{MultiPlayerArena, ClosureFactory};
use poker_example::game::state::PokerGame;
use poker_example::game::commands::PokerCommands;
use poker_example::ai::random_agent::RandomAgent;
use poker_example::ai::tight_agent::TightAgent;
use poker_example::ai::utility_agent::UtilityAgent;
use poker_example::ai::adaptive_agent::AdaptiveAgent;

fn main() {
    println!("=== Poker Multi-Agent Evaluation (TrueSkill) ===\n");

    let mut seed = 1u64;

    let report = MultiPlayerArena::new(2) // heads-up for now
        .with_games(1000)
        .with_max_turns(500)
        .with_max_retries(3)
        .add_agent_type(ClosureFactory::new("adaptive", || {
            Box::new(AdaptiveAgent::new("adaptive", 42))
        }))
        .add_agent_type(ClosureFactory::new("utility", || {
            Box::new(UtilityAgent::new("utility", 99))
        }))
        .add_agent_type(ClosureFactory::new("tight", || {
            Box::new(TightAgent::new("tight"))
        }))
        .add_agent_type(ClosureFactory::new("random", || {
            Box::new(RandomAgent::new("random", 123))
        }))
        .run::<PokerGame, PokerCommands>(move |_num_players| {
            seed += 1;
            PokerGame::new(seed)
        });

    report.print_summary();
}
