use telic::arena::{MultiPlayerArena, ClosureFactory};
use love_letter_example::game::state::LoveLetterGame;
use love_letter_example::game::commands::LoveLetterCommands;
use love_letter_example::ai::random_agent::RandomAgent;
use love_letter_example::ai::smart_agent::DeductionAgent;
use love_letter_example::ai::smart_v2::ProbabilisticAgent;
use love_letter_example::ai::utility_agent::UtilityAgent;

fn main() {
    println!("=== Love Letter Arena (TrueSkill) ===\n");

    let report = MultiPlayerArena::new(2)
        .with_games(5000)
        .with_max_turns(50)
        .add_agent_type(ClosureFactory::new("utility", || {
            Box::new(UtilityAgent::new("utility", 42))
        }))
        .add_agent_type(ClosureFactory::new("probabilistic", || {
            Box::new(ProbabilisticAgent::new("probabilistic", 42))
        }))
        .add_agent_type(ClosureFactory::new("deduction", || {
            Box::new(DeductionAgent::new("deduction", 42))
        }))
        .add_agent_type(ClosureFactory::new("random", || {
            Box::new(RandomAgent::new("random", 123))
        }))
        .run::<LoveLetterGame, LoveLetterCommands>({
            let mut game_seed: u64 = 1;
            move |_num_players| {
                game_seed = game_seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                LoveLetterGame::new(game_seed)
            }
        });

    report.print_summary();
}
