use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Unique identifier for a belief.
pub type BeliefId = String;

/// A belief value — either boolean or numeric.
///
/// Boolean beliefs are for GOAP preconditions/effects ("is_healthy", "at_food").
/// Numeric beliefs are for utility scoring ("threat_level", "combat_odds").
///
/// Numeric beliefs also evaluate as boolean: value > 0.0 = true.
#[derive(Debug, Clone)]
pub enum BeliefValue {
    Boolean(bool),
    Numeric(f64),
}

impl BeliefValue {
    pub fn as_bool(&self) -> bool {
        match self {
            BeliefValue::Boolean(b) => *b,
            BeliefValue::Numeric(v) => *v > 0.0,
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            BeliefValue::Boolean(b) => if *b { 1.0 } else { 0.0 },
            BeliefValue::Numeric(v) => *v,
        }
    }
}

/// A belief is a named query against world state `S`.
///
/// In traditional GOAP, beliefs have ambient access to game objects.
/// In Rust, we pass state explicitly as `&S` — same concept, just
/// honest about the data flow. Beliefs only observe, never mutate.
///
/// Can return boolean ("am I healthy?") or numeric ("what is the threat level?").
pub struct Belief<S> {
    pub name: BeliefId,
    evaluator: Arc<dyn Fn(&S) -> BeliefValue + Send + Sync>,
}

impl<S> Clone for Belief<S> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            evaluator: Arc::clone(&self.evaluator),
        }
    }
}

impl<S> Belief<S> {
    /// Evaluate against state, returning the full BeliefValue.
    pub fn value(&self, state: &S) -> BeliefValue {
        (self.evaluator)(state)
    }

    /// Evaluate as boolean (for GOAP preconditions).
    pub fn evaluate(&self, state: &S) -> bool {
        self.value(state).as_bool()
    }

    /// Evaluate as f64 (for utility scoring).
    pub fn evaluate_numeric(&self, state: &S) -> f64 {
        self.value(state).as_f64()
    }
}

impl<S> fmt::Debug for Belief<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Belief({})", self.name)
    }
}

/// Builder for constructing beliefs.
pub struct BeliefBuilder<S> {
    name: BeliefId,
    evaluator: Option<Arc<dyn Fn(&S) -> BeliefValue + Send + Sync>>,
}

impl<S: 'static> BeliefBuilder<S> {
    pub fn new(name: impl Into<BeliefId>) -> Self {
        Self { name: name.into(), evaluator: None }
    }

    /// Set a boolean condition — queries state and returns true/false.
    pub fn condition(mut self, f: impl Fn(&S) -> bool + Send + Sync + 'static) -> Self {
        self.evaluator = Some(Arc::new(move |s| BeliefValue::Boolean(f(s))));
        self
    }

    /// Set a numeric evaluator — queries state and returns a number.
    pub fn numeric(mut self, f: impl Fn(&S) -> f64 + Send + Sync + 'static) -> Self {
        self.evaluator = Some(Arc::new(move |s| BeliefValue::Numeric(f(s))));
        self
    }

    pub fn build(self) -> Belief<S> {
        Belief {
            name: self.name,
            evaluator: self.evaluator.unwrap_or_else(|| Arc::new(|_| BeliefValue::Boolean(false))),
        }
    }
}

/// A collection of beliefs that an agent holds about the world.
///
/// Generic over state type `S` — all beliefs query `&S` to produce values.
pub struct BeliefSet<S> {
    beliefs: HashMap<BeliefId, Belief<S>>,
}

impl<S> Default for BeliefSet<S> {
    fn default() -> Self {
        Self { beliefs: HashMap::new() }
    }
}

impl<S> BeliefSet<S> {
    pub fn new() -> Self { Self::default() }

    pub fn add(&mut self, belief: Belief<S>) {
        self.beliefs.insert(belief.name.clone(), belief);
    }

    pub fn get(&self, name: &str) -> Option<&Belief<S>> {
        self.beliefs.get(name)
    }

    /// Evaluate a belief as boolean (for GOAP preconditions).
    pub fn evaluate(&self, name: &str, state: &S) -> bool {
        self.beliefs.get(name).map(|b| b.evaluate(state)).unwrap_or(false)
    }

    /// Evaluate a belief as f64 (for utility scoring).
    pub fn evaluate_numeric(&self, name: &str, state: &S) -> f64 {
        self.beliefs.get(name).map(|b| b.evaluate_numeric(state)).unwrap_or(0.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BeliefId, &Belief<S>)> {
        self.beliefs.iter()
    }
}

impl<S> fmt::Debug for BeliefSet<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.beliefs.keys().map(|s| s.as_str()).collect();
        f.debug_struct("BeliefSet")
            .field("beliefs", &names)
            .finish()
    }
}
