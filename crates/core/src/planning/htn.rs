use std::collections::HashSet;

use crate::planning::belief::{BeliefId, BeliefSet};
use crate::planning::action::ActionDef;

/// A task resolver — given beliefs, state, and actions, produces action names.
///
/// This is how tasks delegate planning without being coupled to any specific
/// planner. A resolver could use GOAP, a lookup table, a neural network,
/// or anything else.
pub trait TaskResolver<S>: Send + Sync {
    fn resolve(
        &self,
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        all_tasks: &[Task<S>],
    ) -> Option<Vec<String>>;

    fn debug_name(&self) -> &str;
}

/// An HTN task — either primitive (directly executable), compound (decomposes
/// via methods), or dynamic (delegates to a TaskResolver).
pub enum Task<S> {
    /// Directly maps to a named action.
    Primitive {
        name: String,
        action_name: String,
    },

    /// Decomposes into subtasks via methods.
    /// Methods are tried in order; first whose condition is satisfied wins.
    Compound {
        name: String,
        methods: Vec<Method>,
    },

    /// Delegates to an arbitrary resolver to produce action names.
    Dynamic {
        name: String,
        resolver: Box<dyn TaskResolver<S>>,
    },
}

impl<S> std::fmt::Debug for Task<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::Primitive { name, action_name } => {
                f.debug_struct("Primitive").field("name", name).field("action", action_name).finish()
            }
            Task::Compound { name, methods } => {
                f.debug_struct("Compound").field("name", name).field("methods", &methods.len()).finish()
            }
            Task::Dynamic { name, resolver } => {
                f.debug_struct("Dynamic").field("name", name).field("resolver", &resolver.debug_name()).finish()
            }
        }
    }
}

impl<S> Task<S> {
    pub fn name(&self) -> &str {
        match self {
            Task::Primitive { name, .. } => name,
            Task::Compound { name, .. } => name,
            Task::Dynamic { name, .. } => name,
        }
    }

    /// Decompose this task into a sequence of action names to execute.
    pub fn decompose(
        &self,
        beliefs: &BeliefSet<S>,
        state: &S,
        actions: &[ActionDef],
        all_tasks: &[Task<S>],
    ) -> Option<Vec<String>> {
        match self {
            Task::Primitive { action_name, .. } => Some(vec![action_name.clone()]),

            Task::Compound { methods, .. } => {
                for method in methods {
                    let conditions_met = method
                        .conditions
                        .iter()
                        .all(|c| beliefs.evaluate(c, state));

                    if conditions_met {
                        let mut plan = Vec::new();
                        let mut all_resolved = true;

                        for subtask_name in &method.subtasks {
                            if let Some(subtask) = all_tasks.iter().find(|t| t.name() == subtask_name) {
                                if let Some(sub_plan) = subtask.decompose(beliefs, state, actions, all_tasks) {
                                    plan.extend(sub_plan);
                                } else {
                                    all_resolved = false;
                                    break;
                                }
                            } else {
                                plan.push(subtask_name.clone());
                            }
                        }

                        if all_resolved {
                            return Some(plan);
                        }
                    }
                }
                None
            }

            Task::Dynamic { resolver, .. } => {
                resolver.resolve(beliefs, state, actions, all_tasks)
            }
        }
    }
}

/// An HTN method — one way to decompose a compound task.
#[derive(Debug)]
pub struct Method {
    pub name: String,
    pub conditions: HashSet<BeliefId>,
    pub subtasks: Vec<String>,
}

/// Builder for HTN methods.
pub struct MethodBuilder {
    name: String,
    conditions: HashSet<BeliefId>,
    subtasks: Vec<String>,
}

impl MethodBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            conditions: HashSet::new(),
            subtasks: Vec::new(),
        }
    }

    pub fn condition(mut self, belief: impl Into<BeliefId>) -> Self {
        self.conditions.insert(belief.into());
        self
    }

    pub fn subtask(mut self, task_name: impl Into<String>) -> Self {
        self.subtasks.push(task_name.into());
        self
    }

    pub fn build(self) -> Method {
        Method {
            name: self.name,
            conditions: self.conditions,
            subtasks: self.subtasks,
        }
    }
}

/// Builder for HTN tasks.
pub struct TaskBuilder;

impl TaskBuilder {
    pub fn primitive(name: impl Into<String>, action_name: impl Into<String>) -> Task<()> {
        Task::Primitive {
            name: name.into(),
            action_name: action_name.into(),
        }
    }

    pub fn compound(name: impl Into<String>, methods: Vec<Method>) -> Task<()> {
        Task::Compound {
            name: name.into(),
            methods,
        }
    }

    pub fn dynamic<S>(name: impl Into<String>, resolver: impl TaskResolver<S> + 'static) -> Task<S> {
        Task::Dynamic {
            name: name.into(),
            resolver: Box::new(resolver),
        }
    }
}
