//! Integration tests for `SimpleWarsCommands` — the `CommandProvider` impl
//! for SimpleWars. Exercises the tree shape against real game positions
//! and asserts every leaf is accepted by `apply_command`.

use telic::arena::{CommandProvider, CommandTree, GameState, PlayerIndex};
use simple_wars_example::game::commands::SimpleWarsCommands;
use simple_wars_example::game::state::SimpleWarsGame;
use simple_wars_example::game::types::Command;

/// Drive a game through some ticks of EndTurn so buildings produce units
/// and we reach a mid-game state with a rich command set.
fn warm_game(seed: u64, end_turn_cycles: u32) -> SimpleWarsGame {
    let mut game = SimpleWarsGame::random_16x16(seed);
    for _ in 0..end_turn_cycles {
        for player in 0..game.num_players() {
            let _ = game.apply_command(player as PlayerIndex, Command::EndTurn);
        }
        if game.is_terminal() { break; }
    }
    game
}

#[test]
fn non_current_player_gets_empty_tree() {
    let game = warm_game(11, 0);
    // After game start, whoever current_player() picks is the acting player.
    // The other player's tree must be Empty.
    let p0_empty = SimpleWarsCommands::command_tree(&game, 0).is_empty();
    let p1_empty = SimpleWarsCommands::command_tree(&game, 1).is_empty();
    // Exactly one should be empty (turn-based).
    assert_ne!(p0_empty, p1_empty, "exactly one player should have an empty tree");
}

#[test]
fn tree_contains_end_turn_for_acting_player() {
    let game = warm_game(7, 0);
    for player in 0..game.num_players() {
        let tree = SimpleWarsCommands::command_tree(&game, player);
        if tree.is_empty() { continue; }
        // Must contain an EndTurn leaf
        let found = tree.find_leaf(|c| matches!(c, Command::EndTurn));
        assert!(found.is_some(), "acting player's tree should include EndTurn");
    }
}

#[test]
fn every_leaf_is_accepted_by_apply_command() {
    // For a handful of seeds and warm-up cycles, grab the tree, flatten it,
    // and verify each leaf is valid (apply_command returns Ok) on a fresh
    // clone of the game. This is the core correctness property: the tree
    // contains ONLY valid commands.
    for seed in 1..8u64 {
        for warmup in [0u32, 2, 4] {
            let game = warm_game(seed, warmup);
            for player in 0..game.num_players() {
                let tree = SimpleWarsCommands::command_tree(&game, player);
                if tree.is_empty() { continue; }
                let leaves = tree.flatten();
                assert!(!leaves.is_empty(), "non-empty tree should have leaves");
                for cmd in leaves {
                    let mut probe = game.clone();
                    let res = probe.apply_command(player as PlayerIndex, cmd.clone());
                    assert!(
                        res.is_ok(),
                        "seed={seed} warmup={warmup} player={player} cmd={cmd:?} rejected: {:?}",
                        res
                    );
                }
            }
        }
    }
}

#[test]
fn tree_has_expected_top_level_structure() {
    let game = warm_game(3, 2);
    for player in 0..game.num_players() {
        let tree = SimpleWarsCommands::command_tree(&game, player);
        if tree.is_empty() { continue; }
        // Root is a Layer
        let children = tree.children().expect("root should be a Layer");
        // end_turn must be there
        assert!(children.iter().any(|(k, _)| k == "end_turn"));
        // Every child key is one of the expected categories
        let valid_keys: &[&str] = &["end_turn", "build", "capture", "attack", "move"];
        for (key, _) in children {
            assert!(
                valid_keys.contains(&key.as_str()),
                "unexpected top-level key: {key}"
            );
        }
    }
}

#[test]
fn flatten_matches_leaf_count() {
    let game = warm_game(5, 3);
    let tree = SimpleWarsCommands::command_tree(&game, 0);
    let flat = tree.flatten();
    assert_eq!(flat.len(), tree.leaf_count());
}
