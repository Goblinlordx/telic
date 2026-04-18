use poker_example::game::state::PokerGame;
use poker_example::game::types::*;
use telic::arena::GameState;

/// Try applying a command as player 0, then player 1 if that fails.
/// Returns the player who succeeded and Ok(()), or the last error.
fn try_both_players(game: &mut PokerGame, action: Action) -> Result<usize, String> {
    match game.apply_command(0, action.clone()) {
        Ok(()) => Ok(0),
        Err(_) => {
            game.apply_command(1, action).map(|()| 1)
        }
    }
}

#[test]
fn single_hand_fold() {
    let mut game = PokerGame::new(42);

    println!("Hand: {}", game.turn_number());

    // Whichever player's turn it is, fold
    let result = try_both_players(&mut game, Action::Fold);
    println!("Fold result: {:?}", result);
    println!("Terminal: {}", game.is_terminal());
    println!("Hand after fold: {}", game.turn_number());

    // Should have advanced to next hand
    assert!(!game.is_terminal(), "Game should continue after one fold");
    assert!(game.turn_number() >= 1, "Should be on hand 1+");
}

#[test]
fn single_hand_check_check_through() {
    let mut game = PokerGame::new(42);
    let mut actions = 0;

    while !game.is_terminal() && actions < 100 {
        // Try each player — the game rejects the wrong one
        for p in 0..2 {
            let view = game.view_for(p);

            let action = if view.to_call > 0 {
                Action::Call
            } else {
                Action::Check
            };

            if game.apply_command(p, action.clone()).is_ok() {
                println!("  action {}: p{} {:?} (street={:?}, to_call={}, our_bet={}, opp_bet={}, hand={})",
                    actions, p, action, view.street, view.to_call, view.our_bet, view.opp_bet, view.hand_number);
                actions += 1;
                break;
            }
        }
        if game.is_terminal() { break; }
    }

    println!("Completed {} actions, terminal: {}, hand: {}",
        actions, game.is_terminal(), game.turn_number());
}

#[test]
fn full_match_terminates() {
    use poker_example::ai::tight_agent::TightAgent;
    use poker_example::ai::random_agent::RandomAgent;
    use poker_example::game::commands::PokerCommands;
    use telic::arena::{CommandProvider, GameAgent};

    let mut game = PokerGame::new(42);
    let mut tight = TightAgent::new("tight");
    let mut random = RandomAgent::new("random", 123);
    tight.reset(0);
    random.reset(1);

    let mut total_actions = 0u32;

    while !game.is_terminal() && game.turn_number() < 100 {
        // Try each player — the game rejects the wrong one
        let mut acted = false;
        for current in 0..2 {
            let view = game.view_for(current);

            let tree = PokerCommands::command_tree(&game, current);
            let Some(action) = (if current == 0 {
                tight.decide(&view, &tree)
            } else {
                random.decide(&view, &tree)
            }) else { continue; };

            match game.apply_command(current, action.clone()) {
                Ok(()) => {
                    acted = true;
                    total_actions += 1;
                    break;
                }
                Err(_) => continue,
            }
        }

        if !acted { break; }

        if total_actions > 5000 {
            panic!("Exceeded 5000 actions — infinite loop");
        }
    }

    println!("Match finished: {} actions, {} hands, terminal={}",
        total_actions, game.turn_number(), game.is_terminal());
}

#[test]
fn multiple_seeds_terminate() {
    use poker_example::ai::tight_agent::TightAgent;
    use poker_example::ai::random_agent::RandomAgent;
    use poker_example::game::commands::PokerCommands;
    use telic::arena::{CommandProvider, GameAgent};

    for seed in 1..=20 {
        let mut game = PokerGame::new(seed);
        let mut tight = TightAgent::new("tight");
        let mut random = RandomAgent::new("random", seed * 100 + 1);
        tight.reset(0);
        random.reset(1);

        let mut total_actions = 0u32;
        while !game.is_terminal() && game.turn_number() < 100 {
            let mut acted = false;
            for current in 0..2 {
                let view = game.view_for(current);
                let tree = PokerCommands::command_tree(&game, current);
                let Some(action) = (if current == 0 {
                    tight.decide(&view, &tree)
                } else {
                    random.decide(&view, &tree)
                }) else { continue; };
                if total_actions > 4990 && seed == 2 {
                    println!("  #{}: p{} {:?} street={:?} to_call={} bets={}/{} chips={}/{} pot={} hand={}",
                        total_actions, current, action, view.street, view.to_call,
                        view.our_bet, view.opp_bet, view.our_chips, view.opp_chips, view.pot,
                        game.turn_number());
                }
                if game.apply_command(current, action).is_ok() {
                    acted = true;
                    total_actions += 1;
                    break;
                }
            }
            if !acted { break; }
            if total_actions > 5000 {
                panic!("Seed {} stuck: {} actions, hand {}", seed, total_actions, game.turn_number());
            }
        }
        println!("Seed {}: {} actions, {} hands", seed, total_actions, game.turn_number());
    }
}
