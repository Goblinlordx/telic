use std::collections::HashSet;
use std::fmt;

use crate::planning::belief::BeliefId;

/// Trait for the strategy pattern — the actual executable behavior of an action.
///
/// Actions are decoupled from their execution: the GOAP planner reasons about
/// preconditions/effects, but the strategy handles "how do I actually do this."
pub trait Strategy: fmt::Debug + Send + Sync {
    /// Can this strategy begin executing right now?
    fn can_perform(&self) -> bool {
        true
    }

    /// Is this strategy finished?
    fn is_complete(&self) -> bool;

    /// Called when the action starts executing.
    fn start(&mut self) {}

    /// Called every tick while the action is running.
    fn update(&mut self, _dt: f32) {}

    /// Called when the action stops (complete or interrupted).
    fn stop(&mut self) {}
}

/// An action the agent can perform.
///
/// Actions have:
/// - **Preconditions**: beliefs that must be true before this action can execute
/// - **Effects**: beliefs that will be true after this action completes
/// - **Cost**: how expensive this action is (planner prefers cheaper plans)
/// - **Strategy**: the actual behavior to execute
pub struct ActionDef {
    pub name: String,
    pub cost: f32,
    pub preconditions: HashSet<BeliefId>,
    pub effects: HashSet<BeliefId>,
    strategy: Box<dyn Strategy>,
}

impl ActionDef {
    pub fn strategy(&self) -> &dyn Strategy {
        &*self.strategy
    }

    pub fn strategy_mut(&mut self) -> &mut dyn Strategy {
        &mut *self.strategy
    }

    pub fn is_complete(&self) -> bool {
        self.strategy.is_complete()
    }

    pub fn can_perform(&self) -> bool {
        self.strategy.can_perform()
    }

    pub fn start(&mut self) {
        self.strategy.start();
    }

    pub fn update(&mut self, dt: f32) {
        self.strategy.update(dt);
    }

    pub fn stop(&mut self) {
        self.strategy.stop();
    }
}

impl fmt::Debug for ActionDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Action")
            .field("name", &self.name)
            .field("cost", &self.cost)
            .field("preconditions", &self.preconditions)
            .field("effects", &self.effects)
            .finish()
    }
}

/// Builder for constructing actions.
pub struct ActionBuilder {
    name: String,
    cost: f32,
    preconditions: HashSet<BeliefId>,
    effects: HashSet<BeliefId>,
    strategy: Option<Box<dyn Strategy>>,
}

impl ActionBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cost: 1.0,
            preconditions: HashSet::new(),
            effects: HashSet::new(),
            strategy: None,
        }
    }

    pub fn cost(mut self, cost: f32) -> Self {
        self.cost = cost;
        self
    }

    pub fn precondition(mut self, belief: impl Into<BeliefId>) -> Self {
        self.preconditions.insert(belief.into());
        self
    }

    pub fn effect(mut self, belief: impl Into<BeliefId>) -> Self {
        self.effects.insert(belief.into());
        self
    }

    pub fn strategy(mut self, strategy: impl Strategy + 'static) -> Self {
        self.strategy = Some(Box::new(strategy));
        self
    }

    pub fn build(self) -> ActionDef {
        ActionDef {
            name: self.name,
            cost: self.cost,
            preconditions: self.preconditions,
            effects: self.effects,
            strategy: self.strategy.expect("Action must have a strategy"),
        }
    }
}
