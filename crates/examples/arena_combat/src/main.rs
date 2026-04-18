// Arena Combat uses the tree API with a trivial placeholder
// `CommandProvider`. The real-time `Vec<Command>` batch action space doesn't
// map cleanly onto a tree, so the tree is a degenerate single-leaf; agents
// ignore it and return their own batch. See docs/command_tree.md.

use telic::arena::{MultiPlayerArena, ClosureFactory};
use arena_combat_example::game::state::ArenaCombatGame;
use arena_combat_example::game::commands::ArenaCombatCommands;
use arena_combat_example::ai::random_agent::RandomAgent;
use arena_combat_example::ai::rush_agent::RushAgent;
use arena_combat_example::ai::focus_fire_agent::FocusFireAgent;
use arena_combat_example::ai::kite_agent::KiteAgent;
use arena_combat_example::ai::flanker_agent::FlankerAgent;
use arena_combat_example::ai::utility_agent::UtilityAgent;

fn main() {
    println!("=== Arena Combat (TrueSkill) ===\n");

    let mut seed = 1000u64;
    let report = MultiPlayerArena::new(2)
        .with_games(500)
        .with_max_turns(7200) // 120 seconds at 60fps
        .add_agent_type(ClosureFactory::new("utility", || Box::new(UtilityAgent::new("utility"))))
        .add_agent_type(ClosureFactory::new("kite", || Box::new(KiteAgent::new("kite"))))
        .add_agent_type(ClosureFactory::new("rush", || Box::new(RushAgent::new("rush"))))
        .add_agent_type(ClosureFactory::new("focus_fire", || Box::new(FocusFireAgent::new("focus_fire"))))
        .add_agent_type(ClosureFactory::new("flanker", || Box::new(FlankerAgent::new("flanker"))))
        .add_agent_type(ClosureFactory::new("random", || Box::new(RandomAgent::new("random", 123))))
        .run::<ArenaCombatGame, ArenaCombatCommands>(move |_| {
            seed += 1;
            ArenaCombatGame::random(seed)
        });

    report.print_summary();
}
