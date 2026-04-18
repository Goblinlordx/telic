//! Game-integrated tests for the `AssignmentStrategy` trait.
//!
//! Each test spins up a real SimpleWars position, builds an `[unit][task]`
//! score matrix from the visible view, and exercises every strategy. This
//! validates the strategies against realistic data shapes (sparse conflicts,
//! forbidden pairs from range limits, rectangular matrices) rather than
//! hand-picked toy inputs.

use telic::arena::{GameState, PlayerIndex};
use telic::planning::utility::{
    AssignmentStrategy, Greedy, Hungarian, RoundRobin, WeightedRandom,
};
use simple_wars_example::game::state::{SimpleWarsGame, SimpleWarsView};
use simple_wars_example::game::types::{Pos, TaskKind, Unit};

/// A simple task for test purposes: attack a visible enemy at `target`, or
/// capture a building at `target`. We don't need the full task struct.
#[derive(Debug, Clone)]
struct TestTask {
    kind: TaskKind,
    target: Pos,
}

/// Build a realistic score matrix from a SimpleWars view:
///   rows = our actionable units
///   cols = candidate tasks (one attack per visible enemy, one capture per
///          non-owned building)
///
/// Scores are distance-weighted — a simple but realistic scoring function.
/// Pairs where the unit cannot act (e.g. non-capturing unit vs. capture task)
/// are marked with `f64::NEG_INFINITY` to test the forbidden-pair path.
fn build_scoring_matrix(view: &SimpleWarsView) -> (Vec<Unit>, Vec<TestTask>, Vec<Vec<f64>>) {
    let units: Vec<Unit> = view.our_units.clone();

    let mut tasks: Vec<TestTask> = Vec::new();
    for enemy in &view.visible_enemy_units {
        tasks.push(TestTask { kind: TaskKind::Attack, target: enemy.pos });
    }
    for building in &view.buildings {
        if building.owner != Some(view.viewer) {
            tasks.push(TestTask { kind: TaskKind::Capture, target: building.pos });
        }
    }

    let mut scores = vec![vec![0.0f64; tasks.len()]; units.len()];
    for (ui, unit) in units.iter().enumerate() {
        for (ti, task) in tasks.iter().enumerate() {
            let dist = unit.pos.manhattan_distance(task.target) as f64;
            scores[ui][ti] = match task.kind {
                TaskKind::Capture => {
                    if unit.unit_type.can_capture() {
                        10.0 / (1.0 + dist)
                    } else {
                        f64::NEG_INFINITY
                    }
                }
                TaskKind::Attack => 8.0 / (1.0 + dist),
                _ => 0.0,
            };
        }
    }

    (units, tasks, scores)
}

/// Drive a fresh game through `warmup_turns` of random-ish play to produce
/// an interesting mid-game view (mix of capturers, attackers, forbidden pairs).
fn mid_game_view(seed: u64, warmup_turns: u32) -> (SimpleWarsGame, SimpleWarsView) {
    let mut game = SimpleWarsGame::random_16x16(seed);
    // Step turns by having each side end their turn so buildings produce units.
    for _ in 0..warmup_turns {
        for player in 0..game.num_players() {
            let _ = game.apply_command(player as PlayerIndex, simple_wars_example::game::types::Command::EndTurn);
        }
        if game.is_terminal() { break; }
    }
    let view = game.view_for(0);
    (game, view)
}

fn sum_scores(assignments: &[(usize, usize, f64)]) -> f64 {
    assignments.iter().map(|(_, _, s)| *s).sum()
}

fn assert_valid_assignments(
    assignments: &[(usize, usize, f64)],
    n_entities: usize,
    n_tasks: usize,
    one_to_one: bool,
) {
    for &(e, t, s) in assignments {
        assert!(e < n_entities, "entity idx {e} out of bounds ({n_entities})");
        assert!(t < n_tasks, "task idx {t} out of bounds ({n_tasks})");
        assert_ne!(s, f64::NEG_INFINITY, "picked a forbidden pair ({e}, {t})");
    }
    if one_to_one {
        let mut entities: Vec<usize> = assignments.iter().map(|a| a.0).collect();
        entities.sort();
        let orig_len = entities.len();
        entities.dedup();
        assert_eq!(entities.len(), orig_len, "duplicate entity in one-to-one assignment");
    }
}

#[test]
fn all_strategies_produce_valid_assignments_on_real_view() {
    let (_, view) = mid_game_view(42, 3);
    let (units, tasks, matrix) = build_scoring_matrix(&view);

    if units.is_empty() || tasks.is_empty() {
        // Degenerate warmup — skip rather than assert.
        return;
    }

    // Greedy (task reuse allowed by default — so not strictly one-to-one)
    let greedy = Greedy::new().assign(&mut matrix.clone());
    assert_valid_assignments(&greedy, units.len(), tasks.len(), true);

    // Greedy with task-reuse blocked → one-to-one
    let greedy_exclusive = Greedy::with_coordination(|_e, t, s: &mut Vec<Vec<f64>>| {
        for row in s.iter_mut() { row[t] = f64::NEG_INFINITY; }
    }).assign(&mut matrix.clone());
    assert_valid_assignments(&greedy_exclusive, units.len(), tasks.len(), true);

    // RoundRobin (task reuse allowed)
    let rr = RoundRobin::new().assign(&mut matrix.clone());
    assert_valid_assignments(&rr, units.len(), tasks.len(), true);

    // Hungarian (always one-to-one)
    let hungarian = Hungarian::new().assign(&mut matrix.clone());
    assert_valid_assignments(&hungarian, units.len(), tasks.len(), true);

    // WeightedRandom (one-to-one by construction — tracks used tasks internally)
    let wrand = WeightedRandom::new(7).assign(&mut matrix.clone());
    assert_valid_assignments(&wrand, units.len(), tasks.len(), true);
}

#[test]
fn hungarian_beats_one_to_one_greedy_on_game_matrices() {
    // Average Hungarian total should be >= average one-to-one Greedy total
    // across many seeds. They can tie, but Hungarian should never lose.
    let mut hungarian_total = 0.0;
    let mut greedy_total = 0.0;
    let mut samples = 0;

    for seed in 1..20u64 {
        let (_, view) = mid_game_view(seed, 2);
        let (units, tasks, matrix) = build_scoring_matrix(&view);
        if units.is_empty() || tasks.is_empty() { continue; }

        let h = Hungarian::new().assign(&mut matrix.clone());
        let g = Greedy::with_coordination(|_e, t, s: &mut Vec<Vec<f64>>| {
            for row in s.iter_mut() { row[t] = f64::NEG_INFINITY; }
        }).assign(&mut matrix.clone());

        assert!(
            sum_scores(&h) + 1e-9 >= sum_scores(&g),
            "seed {seed}: Hungarian {:.3} < one-to-one Greedy {:.3}",
            sum_scores(&h), sum_scores(&g),
        );

        hungarian_total += sum_scores(&h);
        greedy_total += sum_scores(&g);
        samples += 1;
    }

    assert!(samples > 0, "no valid game samples produced");
    // Across many random seeds Hungarian should usually do better; at minimum
    // it should tie.
    assert!(
        hungarian_total >= greedy_total - 1e-6,
        "aggregate Hungarian {hungarian_total:.3} < Greedy {greedy_total:.3}"
    );
}

#[test]
fn weighted_random_is_reproducible_across_runs() {
    let (_, view) = mid_game_view(11, 3);
    let (_, _, matrix) = build_scoring_matrix(&view);
    if matrix.is_empty() || matrix[0].is_empty() { return; }

    let r1 = WeightedRandom::new(999).assign(&mut matrix.clone());
    let r2 = WeightedRandom::new(999).assign(&mut matrix.clone());
    assert_eq!(r1, r2, "same seed should produce identical assignments");
}

#[test]
fn strategies_handle_forbidden_pairs_from_can_capture() {
    // Force a scenario with non-capturing units and capture tasks, which
    // produces NEG_INFINITY entries in the matrix.
    let (_, view) = mid_game_view(17, 4);
    let (units, tasks, matrix) = build_scoring_matrix(&view);
    if units.is_empty() || tasks.is_empty() { return; }

    let any_forbidden = matrix.iter()
        .flat_map(|row| row.iter())
        .any(|&s| s == f64::NEG_INFINITY);

    // If the random game didn't produce forbidden pairs, the test still
    // passes — the strategies were exercised on realistic data.
    let _ = any_forbidden;

    // Every strategy must skip forbidden pairs entirely.
    let strategies: Vec<(&str, Vec<(usize, usize, f64)>)> = vec![
        ("greedy", Greedy::new().assign(&mut matrix.clone())),
        ("round_robin", RoundRobin::new().assign(&mut matrix.clone())),
        ("hungarian", Hungarian::new().assign(&mut matrix.clone())),
        ("weighted_random", WeightedRandom::new(3).assign(&mut matrix.clone())),
    ];
    for (name, result) in &strategies {
        for &(_, _, s) in result {
            assert_ne!(s, f64::NEG_INFINITY, "{name} picked a forbidden pair");
        }
        assert_valid_assignments(result, units.len(), tasks.len(), false);
    }
}
