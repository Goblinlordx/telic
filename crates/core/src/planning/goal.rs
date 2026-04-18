use std::collections::HashSet;

use crate::planning::belief::BeliefId;

/// A goal the agent wants to achieve.
///
/// Goals are simply a priority level + a set of desired effects (beliefs
/// we want to be true). The planner finds a sequence of actions that
/// produces these effects.
#[derive(Debug, Clone)]
pub struct Goal {
    pub name: String,
    pub priority: u32,
    pub desired_effects: HashSet<BeliefId>,
}

/// Builder for constructing goals.
pub struct GoalBuilder {
    name: String,
    priority: u32,
    desired_effects: HashSet<BeliefId>,
}

impl GoalBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            priority: 1,
            desired_effects: HashSet::new(),
        }
    }

    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    pub fn desired_effect(mut self, belief: impl Into<BeliefId>) -> Self {
        self.desired_effects.insert(belief.into());
        self
    }

    pub fn build(self) -> Goal {
        Goal {
            name: self.name,
            priority: self.priority,
            desired_effects: self.desired_effects,
        }
    }
}
