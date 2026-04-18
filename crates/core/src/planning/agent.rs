use crate::planning::action::ActionDef;
use crate::planning::belief::BeliefSet;
use crate::planning::goal::Goal;
use crate::planning::htn::Task;
use crate::planning::planner::GoapPlanner;

/// The GOAP+HTN agent — ties beliefs, actions, goals, tasks, and the planner together.
///
/// Generic over state type `S` — beliefs query `&S` to evaluate the world.
///
/// Supports three planning modes:
/// 1. **Pure GOAP**: Goals → backward-chain search → action plan
/// 2. **Pure HTN**: Root task → decompose compound tasks → action plan
/// 3. **Hybrid**: HTN for structure, GOAP for tactical decisions within tasks
pub struct Agent<S> {
    pub beliefs: BeliefSet<S>,
    pub actions: Vec<ActionDef>,
    pub goals: Vec<Goal>,
    pub tasks: Vec<Task<S>>,
    pub root_task: Option<String>,

    pub current_goal: Option<Goal>,
    pub current_action_idx: Option<usize>,
    pub action_queue: Vec<String>,
    pub last_goal_name: Option<String>,
}

impl<S> Agent<S> {
    pub fn new(beliefs: BeliefSet<S>, actions: Vec<ActionDef>, goals: Vec<Goal>) -> Self {
        Self {
            beliefs,
            actions,
            goals,
            tasks: Vec::new(),
            root_task: None,
            current_goal: None,
            current_action_idx: None,
            action_queue: Vec::new(),
            last_goal_name: None,
        }
    }

    pub fn with_tasks(mut self, tasks: Vec<Task<S>>) -> Self {
        self.tasks = tasks;
        self
    }

    pub fn with_root_task(mut self, name: impl Into<String>) -> Self {
        self.root_task = Some(name.into());
        self
    }

    /// Main update tick. Pass current world state for belief evaluation.
    pub fn update(&mut self, state: &S, dt: f32) -> TickResult {
        if self.current_action_idx.is_none() {
            let plan_actions = self.calculate_plan(state);
            if let Some(action_names) = plan_actions {
                self.action_queue = action_names;
                return self.advance_action(state);
            } else {
                return TickResult::NoPlan;
            }
        }

        if let Some(idx) = self.current_action_idx {
            let action = &mut self.actions[idx];
            action.update(dt);

            if action.is_complete() {
                action.stop();
                if self.action_queue.is_empty() {
                    self.last_goal_name = self.current_goal.as_ref().map(|g| g.name.clone());
                    self.current_goal = None;
                    self.current_action_idx = None;
                    return TickResult::PlanComplete;
                } else {
                    return self.advance_action(state);
                }
            }

            return TickResult::ActionRunning(action.name.clone());
        }

        TickResult::Idle
    }

    /// Force replan.
    pub fn replan(&mut self) {
        if let Some(idx) = self.current_action_idx {
            self.actions[idx].stop();
        }
        self.current_action_idx = None;
        self.current_goal = None;
        self.action_queue.clear();
    }

    /// Check if a higher-priority goal should interrupt the current plan.
    pub fn check_interrupt(&mut self, state: &S) -> bool {
        let current_priority = self.current_goal.as_ref().map(|g| g.priority).unwrap_or(0);

        let has_higher = self.goals.iter().any(|g| {
            g.priority > current_priority
                && !g.desired_effects.iter().all(|e| self.beliefs.evaluate(e, state))
        });

        if has_higher {
            self.replan();
            true
        } else {
            false
        }
    }

    fn calculate_plan(&self, state: &S) -> Option<Vec<String>> {
        if let Some(ref root_name) = self.root_task {
            if let Some(root) = self.tasks.iter().find(|t| t.name() == root_name) {
                return root.decompose(&self.beliefs, state, &self.actions, &self.tasks);
            }
        }

        let current_priority = self.current_goal.as_ref().map(|g| g.priority).unwrap_or(0);

        let eligible_goals: Vec<Goal> = self
            .goals
            .iter()
            .filter(|g| g.priority >= current_priority)
            .cloned()
            .collect();

        let plan = GoapPlanner::plan(
            &self.beliefs,
            state,
            &self.actions,
            &eligible_goals,
            self.last_goal_name.as_deref(),
        )?;

        Some(plan.actions)
    }

    fn advance_action(&mut self, state: &S) -> TickResult {
        if let Some(action_name) = self.action_queue.first().cloned() {
            self.action_queue.remove(0);

            if let Some(idx) = self.actions.iter().position(|a| a.name == action_name) {
                let preconditions_met = self.actions[idx]
                    .preconditions
                    .iter()
                    .all(|p| self.beliefs.evaluate(p, state));

                if preconditions_met && self.actions[idx].can_perform() {
                    self.actions[idx].start();
                    self.current_action_idx = Some(idx);
                    return TickResult::ActionStarted(action_name);
                } else {
                    self.current_action_idx = None;
                    self.current_goal = None;
                    self.action_queue.clear();
                    return TickResult::PreconditionFailed(action_name);
                }
            }
        }

        TickResult::NoPlan
    }
}

#[derive(Debug, Clone)]
pub enum TickResult {
    NoPlan,
    Idle,
    ActionStarted(String),
    ActionRunning(String),
    PlanComplete,
    PreconditionFailed(String),
}
