use telic::planning::utility::*;

// =========================================================================
// ResponseCurve
// =========================================================================

#[test]
fn linear_curve_clamps() {
    let curve = ResponseCurve::Linear { min: 0.0, max: 10.0 };
    assert_eq!(curve.evaluate(0.0), 0.0);
    assert_eq!(curve.evaluate(5.0), 0.5);
    assert_eq!(curve.evaluate(10.0), 1.0);
    assert_eq!(curve.evaluate(-5.0), 0.0);  // clamped
    assert_eq!(curve.evaluate(20.0), 1.0);  // clamped
}

#[test]
fn linear_curve_equal_min_max() {
    let curve = ResponseCurve::Linear { min: 5.0, max: 5.0 };
    assert_eq!(curve.evaluate(5.0), 1.0);
    assert_eq!(curve.evaluate(4.0), 0.0);
}

#[test]
fn inverse_curve() {
    let curve = ResponseCurve::Inverse { steepness: 1.0 };
    assert_eq!(curve.evaluate(0.0), 1.0);       // 1/(1+0) = 1
    assert_eq!(curve.evaluate(1.0), 0.5);       // 1/(1+1) = 0.5
    assert!((curve.evaluate(3.0) - 0.25).abs() < 0.001); // 1/(1+3) = 0.25
}

#[test]
fn threshold_curve() {
    let curve = ResponseCurve::Threshold { threshold: 5.0 };
    assert_eq!(curve.evaluate(4.9), 0.0);
    assert_eq!(curve.evaluate(5.0), 1.0);
    assert_eq!(curve.evaluate(10.0), 1.0);
}

#[test]
fn boolean_curve() {
    assert_eq!(ResponseCurve::Boolean.evaluate(0.0), 0.0);
    assert_eq!(ResponseCurve::Boolean.evaluate(-1.0), 0.0);
    assert_eq!(ResponseCurve::Boolean.evaluate(0.001), 1.0);
    assert_eq!(ResponseCurve::Boolean.evaluate(5.0), 1.0);
}

#[test]
fn constant_curve_ignores_input() {
    let curve = ResponseCurve::Constant(0.7);
    assert_eq!(curve.evaluate(0.0), 0.7);
    assert_eq!(curve.evaluate(999.0), 0.7);
}

#[test]
fn identity_curve_passes_through() {
    assert_eq!(ResponseCurve::Identity.evaluate(3.14), 3.14);
    assert_eq!(ResponseCurve::Identity.evaluate(-5.0), -5.0);
}

#[test]
fn custom_curve() {
    let curve = ResponseCurve::Custom(std::sync::Arc::new(|v| v * v));
    assert_eq!(curve.evaluate(3.0), 9.0);
    assert_eq!(curve.evaluate(0.0), 0.0);
}

// =========================================================================
// UtilityAction scoring
// =========================================================================

struct Ctx {
    value: f64,
    distance: f64,
    threat: f64,
}

#[test]
fn additive_scoring() {
    let action = UtilityAction::new("test")
        .with_base(1.0)
        .with_mode(ScoringMode::Additive)
        .consider("value", |ctx: &Ctx| ctx.value, ResponseCurve::Identity, 1.0)
        .consider("threat", |ctx: &Ctx| ctx.threat, ResponseCurve::Identity, 0.5);

    let ctx = Ctx { value: 10.0, distance: 0.0, threat: 4.0 };
    // base(1.0) + value(10.0 * 1.0) + threat(4.0 * 0.5) = 13.0
    assert_eq!(action.score(&ctx), 13.0);
}

#[test]
fn multiplicative_scoring() {
    let action = UtilityAction::new("test")
        .with_base(10.0)
        .with_mode(ScoringMode::Multiplicative)
        .consider("factor", |ctx: &Ctx| ctx.value, ResponseCurve::Identity, 1.0);

    let ctx = Ctx { value: 2.0, distance: 0.0, threat: 0.0 };
    // base(10.0) * (2.0 * 1.0 + (1.0 - 1.0)) = 10.0 * 2.0 = 20.0
    assert_eq!(action.score(&ctx), 20.0);
}

#[test]
fn multiplicative_weight_blends_toward_one() {
    let action = UtilityAction::new("test")
        .with_base(10.0)
        .with_mode(ScoringMode::Multiplicative)
        .consider("half_weight", |ctx: &Ctx| ctx.value, ResponseCurve::Identity, 0.5);

    let ctx = Ctx { value: 0.0, distance: 0.0, threat: 0.0 };
    // base(10.0) * (0.0 * 0.5 + 0.5) = 10.0 * 0.5 = 5.0
    assert_eq!(action.score(&ctx), 5.0);
}

#[test]
fn zero_consideration_kills_multiplicative() {
    let action = UtilityAction::new("test")
        .with_base(10.0)
        .with_mode(ScoringMode::Multiplicative)
        .consider("veto", |_: &Ctx| 0.0, ResponseCurve::Identity, 1.0)
        .consider("bonus", |_: &Ctx| 100.0, ResponseCurve::Identity, 1.0);

    let ctx = Ctx { value: 0.0, distance: 0.0, threat: 0.0 };
    // base(10.0) * 0.0 * ... = 0.0
    assert_eq!(action.score(&ctx), 0.0);
}

// =========================================================================
// score_with_trace
// =========================================================================

#[test]
fn trace_matches_score() {
    let action = UtilityAction::new("traced")
        .with_base(5.0)
        .with_mode(ScoringMode::Additive)
        .consider("a", |ctx: &Ctx| ctx.value, ResponseCurve::Identity, 1.0)
        .consider("b", |ctx: &Ctx| ctx.distance, ResponseCurve::Identity, 2.0);

    let ctx = Ctx { value: 3.0, distance: 4.0, threat: 0.0 };

    let score = action.score(&ctx);
    let trace = action.score_with_trace(&ctx);

    assert_eq!(score, trace.total_score);
    assert_eq!(trace.entries.len(), 2);
    assert_eq!(trace.entries[0].name, "a");
    assert_eq!(trace.entries[0].raw_value, 3.0);
    assert_eq!(trace.entries[0].contribution, 3.0); // 3.0 * weight 1.0
    assert_eq!(trace.entries[1].name, "b");
    assert_eq!(trace.entries[1].contribution, 8.0); // 4.0 * weight 2.0
}

// =========================================================================
// rank_actions / best_action
// =========================================================================

#[test]
fn rank_actions_sorts_descending() {
    let actions = vec![
        UtilityAction::new("low").with_base(1.0).with_mode(ScoringMode::Additive),
        UtilityAction::new("high").with_base(10.0).with_mode(ScoringMode::Additive),
        UtilityAction::new("mid").with_base(5.0).with_mode(ScoringMode::Additive),
    ];

    let ranked = rank_actions::<()>(&actions, &());
    assert_eq!(ranked[0].1, 1); // "high" at index 1
    assert_eq!(ranked[1].1, 2); // "mid" at index 2
    assert_eq!(ranked[2].1, 0); // "low" at index 0
}

#[test]
fn best_action_picks_highest() {
    let actions: Vec<UtilityAction<()>> = vec![
        UtilityAction::new("a").with_base(3.0),
        UtilityAction::new("b").with_base(7.0),
    ];
    let (score, idx) = best_action(&actions, &()).unwrap();
    assert_eq!(idx, 1);
    assert_eq!(score, 7.0);
}

#[test]
fn best_action_empty_returns_none() {
    let actions: Vec<UtilityAction<()>> = vec![];
    assert!(best_action(&actions, &()).is_none());
}

// =========================================================================
// Assignment strategies
// =========================================================================

#[test]
fn greedy_assigns_best_pairs() {
    // 2 entities, 2 tasks
    let mut scores = vec![
        vec![10.0, 5.0],  // entity 0 prefers task 0
        vec![3.0, 8.0],   // entity 1 prefers task 1
    ];

    let result = Greedy::new().assign(&mut scores);

    assert_eq!(result.len(), 2);
    // Best pair is entity 0 → task 0 (score 10)
    assert_eq!(result[0], (0, 0, 10.0));
    // Then entity 1 → task 1 (score 8)
    assert_eq!(result[1], (1, 1, 8.0));
}

#[test]
fn greedy_coordination_callback_adjusts() {
    // 2 entities, 1 task — without adjustment both want the same task
    let mut scores = vec![
        vec![10.0],
        vec![9.0],
    ];

    let mut adjusted = false;
    let result = Greedy::with_coordination(|_ei, _ti, scores: &mut Vec<Vec<f64>>| {
        // After first assignment, zero out the task for everyone else
        for row in scores.iter_mut() {
            row[0] = f64::NEG_INFINITY;
        }
        adjusted = true;
    }).assign(&mut scores);

    assert!(adjusted);
    assert_eq!(result.len(), 1); // only 1 assignment (second entity has no valid task)
    assert_eq!(result[0], (0, 0, 10.0));
}

#[test]
fn greedy_handles_contention() {
    // 3 entities all want task 0, but callback diminishes after each
    let mut scores = vec![
        vec![10.0, 2.0],
        vec![9.0, 3.0],
        vec![8.0, 4.0],
    ];

    let result = Greedy::with_coordination(|_ei, ti, scores: &mut Vec<Vec<f64>>| {
        if ti == 0 {
            // Halve task 0 score for remaining
            for row in scores.iter_mut() {
                row[0] *= 0.5;
            }
        }
    }).assign(&mut scores);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].1, 0); // first gets task 0 (score 10)
    // After halving: entity 1 sees task 0 as 4.5, task 1 as 3.0 → still task 0
    assert_eq!(result[1].1, 0); // second also gets task 0 (score 4.5)
    // After halving again: entity 2 sees task 0 as 2.0, task 1 as 4.0 → task 1
    assert_eq!(result[2].1, 1); // third gets task 1
}

#[test]
fn round_robin_picks_in_entity_order() {
    // Each entity picks its own best task; task reuse allowed
    let mut scores = vec![
        vec![10.0, 5.0],  // entity 0 best: task 0
        vec![3.0, 8.0],   // entity 1 best: task 1
        vec![9.0, 2.0],   // entity 2 best: task 0
    ];

    let result = RoundRobin::new().assign(&mut scores);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0], (0, 0, 10.0));
    assert_eq!(result[1], (1, 1, 8.0));
    // Entity 2 also picks task 0 — RoundRobin allows reuse
    assert_eq!(result[2], (2, 0, 9.0));
}

#[test]
fn round_robin_respects_priority_order() {
    // Entity 2 picks first via priority order
    let mut scores = vec![
        vec![10.0, 5.0],
        vec![3.0, 8.0],
        vec![9.0, 2.0],
    ];

    let result = RoundRobin::with_order(vec![2, 0, 1]).assign(&mut scores);

    assert_eq!(result.len(), 3);
    assert_eq!(result[0], (2, 0, 9.0));
    assert_eq!(result[1], (0, 0, 10.0));
    assert_eq!(result[2], (1, 1, 8.0));
}

#[test]
fn round_robin_coordination_prevents_reuse() {
    // Use coordination callback to zero out assigned tasks, forcing variety
    let mut scores = vec![
        vec![10.0, 5.0],
        vec![9.0, 8.0],  // without coord, this entity picks task 0 too
    ];

    let result = RoundRobin::with_coordination(None, |_ei, ti, scores: &mut Vec<Vec<f64>>| {
        for row in scores.iter_mut() {
            row[ti] = f64::NEG_INFINITY;
        }
    }).assign(&mut scores);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0], (0, 0, 10.0));
    // Entity 1 must now pick task 1 since task 0 is blocked
    assert_eq!(result[1], (1, 1, 8.0));
}

// =========================================================================
// Hungarian
// =========================================================================

fn sum_scores(assignments: &[(usize, usize, f64)]) -> f64 {
    assignments.iter().map(|(_, _, s)| *s).sum()
}

// Helper: force Greedy to one-to-one (matches Hungarian's semantics) by
// zeroing out used tasks via the coordination callback.
fn greedy_one_to_one(scores: &mut Vec<Vec<f64>>) -> Vec<(usize, usize, f64)> {
    Greedy::with_coordination(|_ei, ti, s: &mut Vec<Vec<f64>>| {
        for row in s.iter_mut() {
            row[ti] = f64::NEG_INFINITY;
        }
    }).assign(scores)
}

#[test]
fn hungarian_beats_one_to_one_greedy() {
    // Case where greedy's local choice blocks a better global assignment:
    //   one-to-one greedy picks (0, 0) with 10, forcing entity 1 to task 1 = 0.
    //   hungarian finds (0, 1) at 9 + (1, 0) at 9 = 18.
    let mut scores = vec![
        vec![10.0, 9.0],
        vec![9.0, 0.0],
    ];

    let greedy = greedy_one_to_one(&mut scores.clone());
    let hungarian = Hungarian::new().assign(&mut scores);

    assert_eq!(greedy.len(), 2);
    assert_eq!(hungarian.len(), 2);
    assert!((sum_scores(&hungarian) - 18.0).abs() < 1e-9);
    assert!((sum_scores(&greedy) - 10.0).abs() < 1e-9);
    assert!(sum_scores(&hungarian) > sum_scores(&greedy));
}

#[test]
fn hungarian_matches_greedy_when_non_conflicting() {
    // When preferences don't conflict, even one-to-one greedy is optimal.
    let scores = vec![
        vec![10.0, 1.0],
        vec![1.0, 10.0],
    ];
    let hungarian = Hungarian::new().assign(&mut scores.clone());
    let greedy = greedy_one_to_one(&mut scores.clone());
    assert!((sum_scores(&hungarian) - sum_scores(&greedy)).abs() < 1e-9);
}

#[test]
fn hungarian_handles_rectangular_more_tasks() {
    // 2 entities, 3 tasks → 2 assignments, one task unused.
    let mut scores = vec![
        vec![5.0, 1.0, 10.0],
        vec![8.0, 3.0, 2.0],
    ];
    let result = Hungarian::new().assign(&mut scores);
    assert_eq!(result.len(), 2);
    // Optimal: entity 0 → task 2 (10), entity 1 → task 0 (8) = 18
    assert!((sum_scores(&result) - 18.0).abs() < 1e-9);
    // Each entity appears once
    let mut entities: Vec<usize> = result.iter().map(|(e, _, _)| *e).collect();
    entities.sort();
    assert_eq!(entities, vec![0, 1]);
}

#[test]
fn hungarian_handles_rectangular_more_entities() {
    // 3 entities, 2 tasks → 2 assignments, one entity unused.
    let mut scores = vec![
        vec![5.0, 1.0],
        vec![8.0, 3.0],
        vec![2.0, 10.0],
    ];
    let result = Hungarian::new().assign(&mut scores);
    assert_eq!(result.len(), 2);
    // Optimal: entity 1 → task 0 (8), entity 2 → task 1 (10) = 18
    assert!((sum_scores(&result) - 18.0).abs() < 1e-9);
}

#[test]
fn hungarian_respects_forbidden_pairs() {
    // NEG_INFINITY = forbidden (e.g. unit can't reach target)
    let mut scores = vec![
        vec![10.0, f64::NEG_INFINITY],
        vec![5.0, 5.0],
    ];
    let result = Hungarian::new().assign(&mut scores);
    // Optimal: (0, 0) at 10 + (1, 1) at 5 = 15
    assert_eq!(result.len(), 2);
    assert!((sum_scores(&result) - 15.0).abs() < 1e-9);
    assert!(result.iter().all(|&(_, _, s)| s != f64::NEG_INFINITY));
}

#[test]
fn hungarian_never_worse_than_one_to_one_greedy() {
    // Property: under matching one-to-one semantics, Hungarian total >= Greedy total.
    let matrices = vec![
        vec![vec![3.0, 1.0, 2.0], vec![1.0, 4.0, 5.0], vec![2.0, 6.0, 1.0]],
        vec![vec![7.0, 3.0, 2.0, 1.0], vec![1.0, 8.0, 3.0, 2.0],
             vec![2.0, 1.0, 9.0, 3.0], vec![3.0, 2.0, 1.0, 10.0]],
        vec![vec![5.0, 5.0], vec![5.0, 5.0]],
        vec![vec![10.0, 9.0, 8.0], vec![9.0, 8.0, 7.0], vec![8.0, 7.0, 6.0]],
    ];
    for m in matrices {
        let h = Hungarian::new().assign(&mut m.clone());
        let g = greedy_one_to_one(&mut m.clone());
        assert!(sum_scores(&h) + 1e-9 >= sum_scores(&g),
            "Hungarian {} should be >= one-to-one Greedy {} for matrix {:?}",
            sum_scores(&h), sum_scores(&g), m);
    }
}

// =========================================================================
// WeightedRandom
// =========================================================================

#[test]
fn weighted_random_produces_valid_assignments() {
    let mut scores = vec![
        vec![10.0, 1.0, 5.0],
        vec![2.0, 8.0, 3.0],
        vec![4.0, 6.0, 9.0],
    ];
    let result = WeightedRandom::new(42).assign(&mut scores);
    assert_eq!(result.len(), 3);
    // Each entity used once
    let mut es: Vec<usize> = result.iter().map(|(e, _, _)| *e).collect();
    es.sort();
    assert_eq!(es, vec![0, 1, 2]);
    // Each task used at most once
    let mut ts: Vec<usize> = result.iter().map(|(_, t, _)| *t).collect();
    ts.sort();
    ts.dedup();
    assert_eq!(ts.len(), result.len());
}

#[test]
fn weighted_random_is_deterministic_with_seed() {
    let scores_template = vec![
        vec![10.0, 5.0, 2.0],
        vec![3.0, 8.0, 4.0],
        vec![1.0, 2.0, 9.0],
    ];
    let r1 = WeightedRandom::new(12345).assign(&mut scores_template.clone());
    let r2 = WeightedRandom::new(12345).assign(&mut scores_template.clone());
    assert_eq!(r1, r2);
}

#[test]
fn weighted_random_different_seeds_can_differ() {
    // Low temperature so picks are almost greedy — but with ties, seeds should diverge.
    let scores_template = vec![
        vec![5.0, 5.0, 5.0],
        vec![5.0, 5.0, 5.0],
        vec![5.0, 5.0, 5.0],
    ];
    let mut seen = std::collections::HashSet::new();
    for seed in 1..20u64 {
        let r = WeightedRandom::new(seed).assign(&mut scores_template.clone());
        let tasks: Vec<usize> = r.iter().map(|(_, t, _)| *t).collect();
        seen.insert(tasks);
    }
    // With 3! = 6 valid permutations and uniform scores, we should see variety.
    assert!(seen.len() > 1, "expected seeds to produce different orderings");
}

#[test]
fn weighted_random_low_temperature_approaches_greedy() {
    // Tiny temperature → near-deterministic argmax per entity.
    let scores_template = vec![
        vec![100.0, 1.0, 1.0],
        vec![1.0, 100.0, 1.0],
        vec![1.0, 1.0, 100.0],
    ];
    for seed in 1..10u64 {
        let r = WeightedRandom::with_temperature(seed, 0.001).assign(&mut scores_template.clone());
        // With huge score gaps, every entity should pick its obvious max.
        let total: f64 = r.iter().map(|(_, _, s)| *s).sum();
        assert!((total - 300.0).abs() < 1e-6, "low-T seed {seed} got {total}, expected 300");
    }
}
