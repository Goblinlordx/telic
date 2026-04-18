use std::collections::{BinaryHeap, HashSet};
use std::cmp::Ordering;

use crate::planning::action::ActionDef;
use crate::planning::belief::{BeliefId, BeliefSet};
use crate::planning::goal::Goal;

/// The result of planning — a goal + a sequence of actions to achieve it.
#[derive(Debug)]
pub struct ActionPlan {
    pub goal: Goal,
    pub actions: Vec<String>,
    pub total_cost: f32,
}

/// A search state during planning — represents a partial plan.
#[derive(Clone, Debug)]
struct SearchState {
    /// Actions chosen so far (in reverse order — last chosen first).
    actions: Vec<String>,
    /// Beliefs still unsatisfied (need actions to produce them).
    unsatisfied: HashSet<BeliefId>,
    /// Actions already used (indices into the action list).
    used_actions: HashSet<usize>,
    /// Cost accumulated so far: g(n).
    cost_so_far: f32,
    /// Estimated total cost: f(n) = g(n) + h(n).
    estimated_total: f32,
}

impl PartialEq for SearchState {
    fn eq(&self, other: &Self) -> bool {
        self.estimated_total == other.estimated_total
    }
}

impl Eq for SearchState {}

impl PartialOrd for SearchState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchState {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is max-heap, we want min-cost first → reverse order
        other.estimated_total.partial_cmp(&self.estimated_total)
            .unwrap_or(Ordering::Equal)
    }
}

/// Heuristic function — estimates the remaining cost to satisfy all unsatisfied beliefs.
pub trait PlanHeuristic: Send + Sync {
    /// Estimate the cost to satisfy the remaining unsatisfied beliefs.
    fn estimate(
        &self,
        unsatisfied: &HashSet<BeliefId>,
        actions: &[ActionDef],
    ) -> f32;
}

/// Default heuristic: count unsatisfied beliefs.
#[derive(Debug)]
pub struct CountHeuristic;

impl PlanHeuristic for CountHeuristic {
    fn estimate(&self, unsatisfied: &HashSet<BeliefId>, _actions: &[ActionDef]) -> f32 {
        unsatisfied.len() as f32
    }
}

/// Greedy heuristic: heavily weight the estimate to find plans fast (not optimal).
#[derive(Debug)]
pub struct GreedyHeuristic {
    pub weight: f32,
}

impl PlanHeuristic for GreedyHeuristic {
    fn estimate(&self, unsatisfied: &HashSet<BeliefId>, _actions: &[ActionDef]) -> f32 {
        unsatisfied.len() as f32 * self.weight
    }
}

/// Min-cost heuristic: for each unsatisfied belief, find the cheapest action
/// that could satisfy it. Sum those minimums.
#[derive(Debug)]
pub struct MinCostHeuristic;

impl PlanHeuristic for MinCostHeuristic {
    fn estimate(&self, unsatisfied: &HashSet<BeliefId>, actions: &[ActionDef]) -> f32 {
        let mut total = 0.0f32;
        for belief_id in unsatisfied {
            let min_cost = actions
                .iter()
                .filter(|a| a.effects.contains(belief_id))
                .map(|a| a.cost)
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(f32::MAX);
            if min_cost < f32::MAX {
                total += min_cost;
            }
        }
        total
    }
}

/// Search strategy for the planner.
#[derive(Debug, Clone, Copy)]
pub enum SearchStrategy {
    /// Depth-first search — finds *a* plan quickly, not necessarily optimal.
    Dfs,
    /// A* search with heuristic — finds the optimal (cheapest) plan.
    Astar,
    /// Greedy best-first — fast, follows heuristic greedily, may not be optimal.
    Greedy,
    /// Bidirectional search — searches forward from current state AND backward
    /// from goal, meeting in the middle.
    Bidirectional,
}

/// GOAP Planner with pluggable search strategy and heuristic.
///
/// Generic over state type `S` — beliefs query `&S` to determine what's true.
pub struct GoapPlanner;

impl GoapPlanner {
    /// Plan using the default strategy (A* with min-cost heuristic).
    pub fn plan<S>(
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        goals: &[Goal],
        last_goal: Option<&str>,
    ) -> Option<ActionPlan> {
        Self::plan_with(beliefs, state, actions, goals, last_goal, SearchStrategy::Astar, &MinCostHeuristic)
    }

    /// Plan with a specific search strategy and heuristic.
    pub fn plan_with<S>(
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        goals: &[Goal],
        last_goal: Option<&str>,
        strategy: SearchStrategy,
        heuristic: &dyn PlanHeuristic,
    ) -> Option<ActionPlan> {
        let mut sorted_goals: Vec<&Goal> = goals
            .iter()
            .filter(|g| !g.desired_effects.iter().all(|e| beliefs.evaluate(e, state)))
            .collect();

        sorted_goals.sort_by(|a, b| {
            let a_pri = if Some(a.name.as_str()) == last_goal {
                a.priority as f32 - 0.1
            } else {
                a.priority as f32
            };
            let b_pri = if Some(b.name.as_str()) == last_goal {
                b.priority as f32 - 0.1
            } else {
                b.priority as f32
            };
            b_pri.partial_cmp(&a_pri).unwrap()
        });

        for goal in sorted_goals {
            let unsatisfied: HashSet<BeliefId> = goal
                .desired_effects
                .iter()
                .filter(|e| !beliefs.evaluate(e, state))
                .cloned()
                .collect();

            if unsatisfied.is_empty() {
                continue;
            }

            let result = match strategy {
                SearchStrategy::Dfs => Self::search_dfs(beliefs, state, actions, &unsatisfied),
                SearchStrategy::Astar => Self::search_astar(beliefs, state, actions, &unsatisfied, heuristic),
                SearchStrategy::Greedy => Self::search_astar(beliefs, state, actions, &unsatisfied,
                    &GreedyHeuristic { weight: 5.0 }),
                SearchStrategy::Bidirectional => Self::search_bidirectional(beliefs, state, actions, &unsatisfied),
            };

            if let Some((action_names, cost)) = result {
                return Some(ActionPlan {
                    goal: goal.clone(),
                    actions: action_names,
                    total_cost: cost,
                });
            }
        }

        None
    }

    fn search_astar<S>(
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        initial_unsatisfied: &HashSet<BeliefId>,
        heuristic: &dyn PlanHeuristic,
    ) -> Option<(Vec<String>, f32)> {
        let h = heuristic.estimate(initial_unsatisfied, actions);

        let initial = SearchState {
            actions: Vec::new(),
            unsatisfied: initial_unsatisfied.clone(),
            used_actions: HashSet::new(),
            cost_so_far: 0.0,
            estimated_total: h,
        };

        let mut open = BinaryHeap::new();
        open.push(initial);

        let max_iterations = 10_000;
        let mut iterations = 0;

        while let Some(current) = open.pop() {
            iterations += 1;
            if iterations > max_iterations { break; }

            let still_unsatisfied: HashSet<BeliefId> = current
                .unsatisfied
                .iter()
                .filter(|e| !beliefs.evaluate(e, state))
                .cloned()
                .collect();

            if still_unsatisfied.is_empty() {
                let mut plan = current.actions;
                plan.reverse();
                return Some((plan, current.cost_so_far));
            }

            for (idx, action) in actions.iter().enumerate() {
                if current.used_actions.contains(&idx) { continue; }

                let satisfies_any = action.effects.iter().any(|e| still_unsatisfied.contains(e));
                if !satisfies_any { continue; }

                let mut new_unsatisfied = still_unsatisfied.clone();
                for effect in &action.effects {
                    new_unsatisfied.remove(effect);
                }
                for precondition in &action.preconditions {
                    if !beliefs.evaluate(precondition, state) {
                        new_unsatisfied.insert(precondition.clone());
                    }
                }

                let new_cost = current.cost_so_far + action.cost;
                let h = heuristic.estimate(&new_unsatisfied, actions);

                let mut new_actions = current.actions.clone();
                new_actions.push(action.name.clone());

                let mut new_used = current.used_actions.clone();
                new_used.insert(idx);

                open.push(SearchState {
                    actions: new_actions,
                    unsatisfied: new_unsatisfied,
                    used_actions: new_used,
                    cost_so_far: new_cost,
                    estimated_total: new_cost + h,
                });
            }
        }

        None
    }

    fn search_dfs<S>(
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        initial_unsatisfied: &HashSet<BeliefId>,
    ) -> Option<(Vec<String>, f32)> {
        let mut result_actions = Vec::new();
        let mut result_cost = 0.0;
        let used = HashSet::new();

        if Self::dfs_recurse(beliefs, state, actions, initial_unsatisfied, &used, &mut result_actions, &mut result_cost) {
            result_actions.reverse();
            Some((result_actions, result_cost))
        } else {
            None
        }
    }

    fn dfs_recurse<S>(
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        unsatisfied: &HashSet<BeliefId>,
        used: &HashSet<usize>,
        plan: &mut Vec<String>,
        cost: &mut f32,
    ) -> bool {
        let still_needed: HashSet<BeliefId> = unsatisfied
            .iter()
            .filter(|e| !beliefs.evaluate(e, state))
            .cloned()
            .collect();

        if still_needed.is_empty() { return true; }

        let mut candidates: Vec<usize> = (0..actions.len())
            .filter(|i| !used.contains(i))
            .collect();
        candidates.sort_by(|&a, &b| actions[a].cost.partial_cmp(&actions[b].cost).unwrap());

        for idx in candidates {
            let action = &actions[idx];

            let satisfies_any = action.effects.iter().any(|e| still_needed.contains(e));
            if !satisfies_any { continue; }

            let mut new_unsatisfied = still_needed.clone();
            for effect in &action.effects {
                new_unsatisfied.remove(effect);
            }
            for precondition in &action.preconditions {
                if !beliefs.evaluate(precondition, state) {
                    new_unsatisfied.insert(precondition.clone());
                }
            }

            let mut new_used = used.clone();
            new_used.insert(idx);

            if Self::dfs_recurse(beliefs, state, actions, &new_unsatisfied, &new_used, plan, cost) {
                plan.push(action.name.clone());
                *cost += action.cost;
                return true;
            }
        }

        false
    }

    fn search_bidirectional<S>(
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        goal_unsatisfied: &HashSet<BeliefId>,
    ) -> Option<(Vec<String>, f32)> {
        let mut forward: Vec<(HashSet<BeliefId>, Vec<String>, f32)> = Vec::new();
        let mut backward: Vec<(HashSet<BeliefId>, Vec<String>, f32)> = Vec::new();

        let current_produced: HashSet<BeliefId> = actions
            .iter()
            .flat_map(|a| a.effects.iter().chain(a.preconditions.iter()))
            .filter(|b| beliefs.evaluate(b, state))
            .cloned()
            .collect();
        forward.push((current_produced.clone(), Vec::new(), 0.0));
        backward.push((goal_unsatisfied.clone(), Vec::new(), 0.0));

        let mut forward_used: HashSet<usize> = HashSet::new();
        let mut backward_used: HashSet<usize> = HashSet::new();

        let max_iterations = 100;

        for _ in 0..max_iterations {
            let mut new_forward = Vec::new();
            for (produced, fwd_plan, fwd_cost) in &forward {
                for (idx, action) in actions.iter().enumerate() {
                    if forward_used.contains(&idx) { continue; }
                    let preconds_met = action.preconditions.iter()
                        .all(|p| produced.contains(p) || beliefs.evaluate(p, state));
                    if !preconds_met { continue; }

                    let mut new_produced = produced.clone();
                    for effect in &action.effects {
                        new_produced.insert(effect.clone());
                    }
                    let mut new_plan = fwd_plan.clone();
                    new_plan.push(action.name.clone());
                    new_forward.push((new_produced, new_plan, fwd_cost + action.cost));
                    forward_used.insert(idx);
                }
            }
            forward.extend(new_forward);

            let mut new_backward = Vec::new();
            for (needed, bwd_plan, bwd_cost) in &backward {
                for (idx, action) in actions.iter().enumerate() {
                    if backward_used.contains(&idx) { continue; }
                    let satisfies_any = action.effects.iter().any(|e| needed.contains(e));
                    if !satisfies_any { continue; }

                    let mut new_needed = needed.clone();
                    for effect in &action.effects {
                        new_needed.remove(effect);
                    }
                    for precondition in &action.preconditions {
                        if !beliefs.evaluate(precondition, state) {
                            new_needed.insert(precondition.clone());
                        }
                    }

                    let mut new_plan = bwd_plan.clone();
                    new_plan.push(action.name.clone());
                    new_backward.push((new_needed, new_plan, bwd_cost + action.cost));
                    backward_used.insert(idx);
                }
            }
            backward.extend(new_backward);

            let mut best: Option<(Vec<String>, f32)> = None;

            for (needed, bwd_plan, bwd_cost) in &backward {
                if needed.is_empty() || needed.iter().all(|n| beliefs.evaluate(n, state)) {
                    let mut plan = bwd_plan.clone();
                    plan.reverse();
                    let cost = *bwd_cost;
                    if best.as_ref().map_or(true, |(_, c)| cost < *c) {
                        best = Some((plan, cost));
                    }
                }

                for (produced, fwd_plan, fwd_cost) in &forward {
                    let all_satisfied = needed.iter().all(|n| produced.contains(n));
                    if all_satisfied {
                        let mut plan = fwd_plan.clone();
                        let mut bwd_reversed = bwd_plan.clone();
                        bwd_reversed.reverse();
                        plan.extend(bwd_reversed);
                        let cost = fwd_cost + bwd_cost;
                        if best.as_ref().map_or(true, |(_, c)| cost < *c) {
                            best = Some((plan, cost));
                        }
                    }
                }
            }

            if let Some(result) = best {
                return Some(result);
            }
        }

        None
    }
}
