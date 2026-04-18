use telic::arena::{MultiPlayerArena, ClosureFactory};
use splendor_example::game::state::SplendorGame;
use splendor_example::game::commands::SplendorCommands;
use splendor_example::ai::random_agent::RandomAgent;
use splendor_example::ai::strategic_agent::*;
use splendor_example::ai::path_agent::PathAgent;
use splendor_example::ai::utility_agent::UtilityAgent;

fn main() {
    println!("=== Splendor Arena (TrueSkill) ===\n");

    let mut game_seed: u64 = 1;

    let report = MultiPlayerArena::new(2)
        .with_games(1000)
        .with_max_turns(300)
        .add_agent_type(ClosureFactory::new("utility", || {
            Box::new(UtilityAgent::new("utility", 42))
        }))
        .add_agent_type(ClosureFactory::new("path", || {
            Box::new(PathAgent::new("path", 42))
        }))
        .add_agent_type(ClosureFactory::new("engine", || {
            Box::new(StrategicAgent::engine_builder("engine", 999))
        }))
        .add_agent_type(ClosureFactory::new("balanced", || {
            Box::new(StrategicAgent::balanced("balanced", 999))
        }))
        .add_agent_type(ClosureFactory::new("random", || {
            Box::new(RandomAgent::new("random", 123))
        }))
        .run::<SplendorGame, SplendorCommands>(move |_num_players| {
            game_seed = game_seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            SplendorGame::new(game_seed)
        });

    report.print_summary();
}
