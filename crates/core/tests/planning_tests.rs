use telic::planning::belief::{BeliefBuilder, BeliefSet};
use telic::planning::action::{ActionBuilder, Strategy};
use telic::planning::goal::GoalBuilder;
use telic::planning::planner::GoapPlanner;
use telic::planning::htn::{Task, TaskBuilder, MethodBuilder};

// Shared no-op strategy for GOAP actions
#[derive(Debug)]
struct NoOp;
impl Strategy for NoOp { fn is_complete(&self) -> bool { true } }

// =========================================================================
// GOAP: backward chaining finds valid plans
// =========================================================================

struct World {
    has_wood: bool,
    has_iron: bool,
    has_sword: bool,
}

#[test]
fn goap_finds_simple_plan() {
    let mut beliefs = BeliefSet::new();
    beliefs.add(BeliefBuilder::new("has_wood")
        .condition(|w: &World| w.has_wood).build());
    beliefs.add(BeliefBuilder::new("has_iron")
        .condition(|w: &World| w.has_iron).build());
    beliefs.add(BeliefBuilder::new("has_sword")
        .condition(|w: &World| w.has_sword).build());

    let actions = vec![
        ActionBuilder::new("gather_wood").effect("has_wood").strategy(NoOp).build(),
        ActionBuilder::new("mine_iron").effect("has_iron").strategy(NoOp).build(),
        ActionBuilder::new("forge_sword")
            .precondition("has_wood").precondition("has_iron")
            .effect("has_sword").strategy(NoOp).build(),
    ];

    let goals = vec![
        GoalBuilder::new("get_sword").priority(1).desired_effect("has_sword").build(),
    ];

    let world = World { has_wood: false, has_iron: false, has_sword: false };
    let plan = GoapPlanner::plan(&beliefs, &world, &actions, &goals, None).unwrap();

    assert_eq!(plan.goal.name, "get_sword");
    assert!(plan.actions.contains(&"gather_wood".to_string()));
    assert!(plan.actions.contains(&"mine_iron".to_string()));
    assert!(plan.actions.contains(&"forge_sword".to_string()));
    // forge_sword must come AFTER its preconditions
    let forge_idx = plan.actions.iter().position(|a| a == "forge_sword").unwrap();
    let wood_idx = plan.actions.iter().position(|a| a == "gather_wood").unwrap();
    let iron_idx = plan.actions.iter().position(|a| a == "mine_iron").unwrap();
    assert!(forge_idx > wood_idx);
    assert!(forge_idx > iron_idx);
}

#[test]
fn goap_skips_satisfied_preconditions() {
    let mut beliefs = BeliefSet::new();
    beliefs.add(BeliefBuilder::new("has_wood").condition(|w: &World| w.has_wood).build());
    beliefs.add(BeliefBuilder::new("has_iron").condition(|w: &World| w.has_iron).build());
    beliefs.add(BeliefBuilder::new("has_sword").condition(|w: &World| w.has_sword).build());

    let actions = vec![
        ActionBuilder::new("gather_wood").effect("has_wood").strategy(NoOp).build(),
        ActionBuilder::new("mine_iron").effect("has_iron").strategy(NoOp).build(),
        ActionBuilder::new("forge_sword")
            .precondition("has_wood").precondition("has_iron")
            .effect("has_sword").strategy(NoOp).build(),
    ];

    let goals = vec![
        GoalBuilder::new("get_sword").priority(1).desired_effect("has_sword").build(),
    ];

    // Already have wood — plan should NOT include gather_wood
    let world = World { has_wood: true, has_iron: false, has_sword: false };
    let plan = GoapPlanner::plan(&beliefs, &world, &actions, &goals, None).unwrap();

    assert!(!plan.actions.contains(&"gather_wood".to_string()));
    assert!(plan.actions.contains(&"mine_iron".to_string()));
    assert!(plan.actions.contains(&"forge_sword".to_string()));
}

#[test]
fn goap_returns_none_when_no_plan_exists() {
    let beliefs = BeliefSet::<()>::new();

    let actions = vec![
        // forge_sword needs has_iron, but nothing produces has_iron
        ActionBuilder::new("forge_sword")
            .precondition("has_iron")
            .effect("has_sword").strategy(NoOp).build(),
    ];

    let goals = vec![
        GoalBuilder::new("get_sword").priority(1).desired_effect("has_sword").build(),
    ];

    let plan = GoapPlanner::plan(&beliefs, &(), &actions, &goals, None);
    assert!(plan.is_none());
}

#[test]
fn goap_picks_highest_priority_unsatisfied_goal() {
    let mut beliefs = BeliefSet::new();
    beliefs.add(BeliefBuilder::new("is_fed").condition(|w: &World| w.has_wood).build()); // reuse field
    beliefs.add(BeliefBuilder::new("has_sword").condition(|w: &World| w.has_sword).build());

    let actions = vec![
        ActionBuilder::new("eat").effect("is_fed").strategy(NoOp).build(),
        ActionBuilder::new("make_sword").effect("has_sword").strategy(NoOp).build(),
    ];

    let goals = vec![
        GoalBuilder::new("survive").priority(3).desired_effect("is_fed").build(),
        GoalBuilder::new("arm_up").priority(1).desired_effect("has_sword").build(),
    ];

    let world = World { has_wood: false, has_iron: false, has_sword: false };
    let plan = GoapPlanner::plan(&beliefs, &world, &actions, &goals, None).unwrap();

    // Should pick "survive" (priority 3) over "arm_up" (priority 1)
    assert_eq!(plan.goal.name, "survive");
}

// =========================================================================
// HTN: task decomposition
// =========================================================================

#[test]
fn htn_primitive_returns_action() {
    let tasks: Vec<Task<()>> = vec![
        TaskBuilder::primitive("do_thing", "action_name"),
    ];
    let beliefs = BeliefSet::<()>::new();

    let result = tasks[0].decompose(&beliefs, &(), &[], &tasks);
    assert_eq!(result, Some(vec!["action_name".to_string()]));
}

#[test]
fn htn_compound_picks_first_valid_method() {
    let mut beliefs = BeliefSet::<()>::new();
    beliefs.add(BeliefBuilder::new("is_hungry")
        .condition(|_: &()| true).build());

    let tasks: Vec<Task<()>> = vec![
        TaskBuilder::compound("root", vec![
            MethodBuilder::new("hungry_path")
                .condition("is_hungry")
                .subtask("eat")
                .build(),
            MethodBuilder::new("default")
                .subtask("idle")
                .build(),
        ]),
        TaskBuilder::primitive("eat", "eat_action"),
        TaskBuilder::primitive("idle", "idle_action"),
    ];

    let result = tasks[0].decompose(&beliefs, &(), &[], &tasks);
    assert_eq!(result, Some(vec!["eat_action".to_string()]));
}

#[test]
fn htn_compound_falls_through_to_default() {
    let mut beliefs = BeliefSet::<()>::new();
    beliefs.add(BeliefBuilder::new("is_hungry")
        .condition(|_: &()| false).build());

    let tasks: Vec<Task<()>> = vec![
        TaskBuilder::compound("root", vec![
            MethodBuilder::new("hungry_path")
                .condition("is_hungry")
                .subtask("eat")
                .build(),
            MethodBuilder::new("default")
                .subtask("idle")
                .build(),
        ]),
        TaskBuilder::primitive("eat", "eat_action"),
        TaskBuilder::primitive("idle", "idle_action"),
    ];

    let result = tasks[0].decompose(&beliefs, &(), &[], &tasks);
    assert_eq!(result, Some(vec!["idle_action".to_string()]));
}

#[test]
fn htn_compound_multi_step_decomposition() {
    let beliefs = BeliefSet::<()>::new();

    let tasks: Vec<Task<()>> = vec![
        TaskBuilder::compound("root", vec![
            MethodBuilder::new("full_sequence")
                .subtask("step_a")
                .subtask("step_b")
                .subtask("step_c")
                .build(),
        ]),
        TaskBuilder::primitive("step_a", "action_a"),
        TaskBuilder::primitive("step_b", "action_b"),
        TaskBuilder::primitive("step_c", "action_c"),
    ];

    let result = tasks[0].decompose(&beliefs, &(), &[], &tasks);
    assert_eq!(result, Some(vec![
        "action_a".to_string(),
        "action_b".to_string(),
        "action_c".to_string(),
    ]));
}

// =========================================================================
// BeliefSet: state queries
// =========================================================================

#[test]
fn beliefs_query_state() {
    let mut beliefs = BeliefSet::new();
    beliefs.add(BeliefBuilder::new("health_low")
        .condition(|hp: &f64| *hp < 30.0).build());
    beliefs.add(BeliefBuilder::new("health_value")
        .numeric(|hp: &f64| *hp).build());

    assert!(beliefs.evaluate("health_low", &20.0));
    assert!(!beliefs.evaluate("health_low", &50.0));
    assert_eq!(beliefs.evaluate_numeric("health_value", &42.0), 42.0);
}

#[test]
fn missing_belief_returns_default() {
    let beliefs = BeliefSet::<()>::new();
    assert!(!beliefs.evaluate("nonexistent", &()));
    assert_eq!(beliefs.evaluate_numeric("nonexistent", &()), 0.0);
}
