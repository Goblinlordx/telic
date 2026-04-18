use telic::arena::{MultiPlayerArena, ClosureFactory};
use simple_wars_example::game::state::{SimpleWarsGame, SimpleWarsView};
use simple_wars_example::game::types::Command;
use simple_wars_example::game::commands::SimpleWarsCommands;
use simple_wars_example::ai::random_agent::RandomAgent;
use simple_wars_example::ai::reactive_agent::ReactiveAgent;
use simple_wars_example::ai::strategic_agent::HandCodedAgent;
use simple_wars_example::ai::utility_agent::UtilityAgent;
use simple_wars_example::ai::coordinated_agent::{CoordinatedAgent, StrategyChoice};
use simple_wars_example::ai::hybrid_agent::HybridAgent;
use simple_wars_example::ai::htn_goap::HtnAgent;

fn make_arena(games: u32) -> MultiPlayerArena<SimpleWarsView, Command> {
    MultiPlayerArena::<SimpleWarsView, Command>::new(2)
        .with_games(games)
        .with_max_turns(500)
        .with_max_retries(3)
        .add_agent_type(ClosureFactory::new("random", || Box::new(RandomAgent::new("random", 123))))
        .add_agent_type(ClosureFactory::new("reactive", || Box::new(ReactiveAgent::new("reactive", 321))))
        .add_agent_type(ClosureFactory::new("hand-coded", || Box::new(HandCodedAgent::new("hand-coded", 999))))
        .add_agent_type(ClosureFactory::new("utility", || Box::new(UtilityAgent::new("utility", 99))))
        .add_agent_type(ClosureFactory::new("coord-greedy", || Box::new(
            CoordinatedAgent::with_strategy("coord-greedy", 42, StrategyChoice::GreedyCoordinated))))
        .add_agent_type(ClosureFactory::new("coord-roundrobin", || Box::new(
            CoordinatedAgent::with_strategy("coord-roundrobin", 42, StrategyChoice::RoundRobin))))
        .add_agent_type(ClosureFactory::new("coord-hungarian", || Box::new(
            CoordinatedAgent::with_strategy("coord-hungarian", 42, StrategyChoice::Hungarian))))
        .add_agent_type(ClosureFactory::new("coord-wrand", || Box::new(
            CoordinatedAgent::with_strategy("coord-wrand", 42, StrategyChoice::WeightedRandom(42)))))
        .add_agent_type(ClosureFactory::new("hybrid", || Box::new(HybridAgent::new("hybrid", 42))))
        .add_agent_type(ClosureFactory::new("htn", || Box::new(HtnAgent::new("htn", 42))))
}

fn main() {
    const GAMES_PER_MAP: u32 = 2000;

    println!("=== SimpleWars Arena (8x8 random, {} games) ===\n", GAMES_PER_MAP);
    let mut seed = 1000u64;
    make_arena(GAMES_PER_MAP)
        .run::<SimpleWarsGame, SimpleWarsCommands>(move |_| { seed += 1; SimpleWarsGame::random_8x8(seed) })
        .print_summary();

    println!("\n=== SimpleWars Arena (16x16 random, {} games) ===\n", GAMES_PER_MAP);
    let mut seed = 2000u64;
    make_arena(GAMES_PER_MAP)
        .run::<SimpleWarsGame, SimpleWarsCommands>(move |_| { seed += 1; SimpleWarsGame::random_16x16(seed) })
        .print_summary();

    println!("\n=== SimpleWars Arena (16x16 HARD, {} games) ===\n", GAMES_PER_MAP);
    let mut seed = 3000u64;
    make_arena(GAMES_PER_MAP)
        .run::<SimpleWarsGame, SimpleWarsCommands>(move |_| { seed += 1; SimpleWarsGame::random_16x16_hard(seed) })
        .print_summary();
}
