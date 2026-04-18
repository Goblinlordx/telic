//! Utility scoring system for task/action selection.
//!
//! Utility AI scores every possible action by how "useful" it would be
//! given the current world state. The highest-scoring action wins.
//!
//! Generic over a state type `S` — considerations evaluate directly from
//! `&S` through response curves. No intermediate belief lookup needed.
//!
//! # Lazy evaluation
//!
//! Considerations are closures over `&S` — they only evaluate when
//! `score()` or `score_with_trace()` is called. Unused actions never
//! fire their considerations. This means evaluation is inherently lazy:
//! if you have 10 action templates but only score 3 candidates, only
//! those 3 sets of considerations run.
//!
//! # Explainability
//!
//! Use `score_with_trace()` to get a breakdown of each consideration's
//! contribution to the final score:
//!
//! ```ignore
//! let (score, trace) = attack.score_with_trace(&ctx);
//! for entry in &trace {
//!     println!("{}: raw={:.2} curved={:.2} contribution={:.2}",
//!         entry.name, entry.raw_value, entry.curved_value, entry.contribution);
//! }
//! ```
//!
//! # Smart objects
//!
//! Use the [`ActionSource`] trait to let world objects advertise what
//! actions they support. Agents query visible objects for offered actions
//! instead of hardcoding task generation:
//!
//! ```ignore
//! impl ActionSource<UnitTask> for City {
//!     fn available_actions(&self, viewer: &GameView) -> Vec<UnitTask> {
//!         if self.owner != viewer.player {
//!             vec![UnitTask::capture(self.pos)]
//!         } else { vec![] }
//!     }
//! }
//! ```

use std::sync::Arc;

/// A consideration — one factor that influences an action's score.
///
/// Evaluates a value from state `&S`, maps it through a response curve,
/// and applies a weight.
pub struct Consideration<S> {
    pub name: String,
    /// Extracts a raw value from the state.
    evaluator: Arc<dyn Fn(&S) -> f64 + Send + Sync>,
    /// Transform the raw value into a 0-1 score.
    pub curve: ResponseCurve,
    /// How much this consideration matters (blended into final score).
    pub weight: f64,
}

impl<S> Clone for Consideration<S> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            evaluator: Arc::clone(&self.evaluator),
            curve: self.curve.clone(),
            weight: self.weight,
        }
    }
}

impl<S> std::fmt::Debug for Consideration<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Consideration")
            .field("name", &self.name)
            .field("weight", &self.weight)
            .field("curve", &self.curve)
            .finish()
    }
}

impl<S> Consideration<S> {
    /// Evaluate this consideration against state.
    pub fn evaluate(&self, state: &S) -> f64 {
        let raw = (self.evaluator)(state);
        self.curve.evaluate(raw)
    }

    /// Evaluate with full trace — returns curved value and a trace entry.
    pub fn evaluate_traced(&self, state: &S) -> (f64, TraceEntry) {
        let raw = (self.evaluator)(state);
        let curved = self.curve.evaluate(raw);
        let entry = TraceEntry {
            name: self.name.clone(),
            raw_value: raw,
            curved_value: curved,
            weight: self.weight,
            contribution: 0.0, // filled in by UtilityAction
        };
        (curved, entry)
    }
}

/// A trace entry — explains one consideration's contribution to the final score.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    /// Consideration name.
    pub name: String,
    /// Raw value from the evaluator (before curve).
    pub raw_value: f64,
    /// Value after the response curve.
    pub curved_value: f64,
    /// The consideration's weight.
    pub weight: f64,
    /// Actual contribution to the final score (depends on scoring mode).
    pub contribution: f64,
}

/// Full scoring trace — the score plus a breakdown of how it was computed.
#[derive(Debug, Clone)]
pub struct ScoringTrace {
    /// The action that was scored.
    pub action_name: String,
    /// Base score before considerations.
    pub base_score: f64,
    /// Scoring mode used.
    pub mode: ScoringMode,
    /// Per-consideration breakdown.
    pub entries: Vec<TraceEntry>,
    /// Final computed score.
    pub total_score: f64,
}

impl std::fmt::Display for ScoringTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== {} (score: {:.3}, base: {:.3}, mode: {:?}) ===",
            self.action_name, self.total_score, self.base_score, self.mode)?;
        for e in &self.entries {
            writeln!(f, "  {:<20} raw={:>8.3}  curved={:>8.3}  w={:.2}  contrib={:>8.3}",
                e.name, e.raw_value, e.curved_value, e.weight, e.contribution)?;
        }
        Ok(())
    }
}

/// Response curves — how a raw value maps to a 0-1 score.
#[derive(Clone)]
pub enum ResponseCurve {
    /// Linear: score = clamp((value - min) / (max - min), 0, 1)
    Linear { min: f64, max: f64 },
    /// Inverse: score = 1.0 / (1.0 + value * steepness)
    /// Good for "closer is better" (distance)
    Inverse { steepness: f64 },
    /// Threshold: 1.0 if value >= threshold, 0.0 otherwise
    Threshold { threshold: f64 },
    /// Boolean: 1.0 if value > 0, 0.0 otherwise
    Boolean,
    /// Constant: always returns the given value (ignores input)
    Constant(f64),
    /// Identity: returns the input value unchanged (passthrough)
    Identity,
    /// Custom function
    Custom(Arc<dyn Fn(f64) -> f64 + Send + Sync>),
}

impl std::fmt::Debug for ResponseCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linear { min, max } => write!(f, "Linear({min}..{max})"),
            Self::Inverse { steepness } => write!(f, "Inverse({steepness})"),
            Self::Threshold { threshold } => write!(f, "Threshold({threshold})"),
            Self::Boolean => write!(f, "Boolean"),
            Self::Constant(v) => write!(f, "Constant({v})"),
            Self::Identity => write!(f, "Identity"),
            Self::Custom(_) => write!(f, "Custom(fn)"),
        }
    }
}

impl ResponseCurve {
    pub fn evaluate(&self, value: f64) -> f64 {
        match self {
            ResponseCurve::Linear { min, max } => {
                if max == min { return if value >= *min { 1.0 } else { 0.0 }; }
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            }
            ResponseCurve::Inverse { steepness } => {
                1.0 / (1.0 + value * steepness)
            }
            ResponseCurve::Threshold { threshold } => {
                if value >= *threshold { 1.0 } else { 0.0 }
            }
            ResponseCurve::Boolean => {
                if value > 0.0 { 1.0 } else { 0.0 }
            }
            ResponseCurve::Constant(v) => *v,
            ResponseCurve::Identity => value,
            ResponseCurve::Custom(f) => f(value),
        }
    }
}

/// A scored action — something an agent could do, with considerations
/// that determine how useful it would be given state `S`.
pub struct UtilityAction<S> {
    pub name: String,
    pub considerations: Vec<Consideration<S>>,
    /// Base score before considerations are applied.
    pub base_score: f64,
    /// Scoring mode: how considerations combine.
    pub mode: ScoringMode,
}

/// How considerations combine into a final score.
#[derive(Debug, Clone, Copy)]
pub enum ScoringMode {
    /// Multiply: base * c1 * c2 * ... (default, good for veto-style)
    Multiplicative,
    /// Add: base + w1*c1 + w2*c2 + ... (good for additive bonuses)
    Additive,
}

impl<S> Clone for UtilityAction<S> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            considerations: self.considerations.clone(),
            base_score: self.base_score,
            mode: self.mode,
        }
    }
}

impl<S> std::fmt::Debug for UtilityAction<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UtilityAction")
            .field("name", &self.name)
            .field("base_score", &self.base_score)
            .field("mode", &self.mode)
            .field("considerations", &self.considerations)
            .finish()
    }
}

impl<S> UtilityAction<S> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            considerations: Vec::new(),
            base_score: 1.0,
            mode: ScoringMode::Multiplicative,
        }
    }

    pub fn with_base(mut self, base: f64) -> Self {
        self.base_score = base;
        self
    }

    pub fn with_mode(mut self, mode: ScoringMode) -> Self {
        self.mode = mode;
        self
    }

    /// Add a consideration with a closure that extracts a value from state.
    pub fn consider(
        mut self,
        name: impl Into<String>,
        evaluator: impl Fn(&S) -> f64 + Send + Sync + 'static,
        curve: ResponseCurve,
        weight: f64,
    ) -> Self {
        self.considerations.push(Consideration {
            name: name.into(),
            evaluator: Arc::new(evaluator),
            curve,
            weight,
        });
        self
    }

    /// Compute the total score for this action given state.
    pub fn score(&self, state: &S) -> f64 {
        match self.mode {
            ScoringMode::Multiplicative => {
                let mut total = self.base_score;
                for c in &self.considerations {
                    let curved = c.evaluate(state);
                    total *= curved * c.weight + (1.0 - c.weight);
                }
                total
            }
            ScoringMode::Additive => {
                let mut total = self.base_score;
                for c in &self.considerations {
                    let curved = c.evaluate(state);
                    total += curved * c.weight;
                }
                total
            }
        }
    }

    /// Compute the score with a full trace of each consideration's contribution.
    /// Use this for debugging and explainability — shows exactly why a score
    /// was computed the way it was.
    pub fn score_with_trace(&self, state: &S) -> ScoringTrace {
        let mut entries = Vec::with_capacity(self.considerations.len());
        let total = match self.mode {
            ScoringMode::Multiplicative => {
                let mut total = self.base_score;
                for c in &self.considerations {
                    let (curved, mut entry) = c.evaluate_traced(state);
                    let blended = curved * c.weight + (1.0 - c.weight);
                    let before = total;
                    total *= blended;
                    entry.contribution = total - before;
                    entries.push(entry);
                }
                total
            }
            ScoringMode::Additive => {
                let mut total = self.base_score;
                for c in &self.considerations {
                    let (curved, mut entry) = c.evaluate_traced(state);
                    let contrib = curved * c.weight;
                    entry.contribution = contrib;
                    total += contrib;
                    entries.push(entry);
                }
                total
            }
        };

        ScoringTrace {
            action_name: self.name.clone(),
            base_score: self.base_score,
            mode: self.mode,
            entries,
            total_score: total,
        }
    }
}

/// Evaluate a set of utility actions and return them sorted by score (best first).
pub fn rank_actions<S>(actions: &[UtilityAction<S>], state: &S) -> Vec<(f64, usize)> {
    let mut scored: Vec<(f64, usize)> = actions.iter()
        .enumerate()
        .map(|(i, a)| (a.score(state), i))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// Pick the best action. Returns (score, index) or None if empty.
pub fn best_action<S>(actions: &[UtilityAction<S>], state: &S) -> Option<(f64, usize)> {
    rank_actions(actions, state).into_iter().next()
}

// =========================================================================
// Smart objects — world objects advertise available actions
// =========================================================================

/// A source of actions — world objects implement this to advertise what
/// actions they support. This inverts the pattern from "agent hardcodes
/// all possible actions" to "world tells agent what's available."
///
/// From F.E.A.R.: a door knows it can be opened, a city knows it can be
/// captured. The agent queries all visible objects and scores the offered
/// actions rather than maintaining a fixed list.
///
/// `A` is the action/task type that the agent scores.
/// `V` is the view/context needed to determine what's available.
pub trait ActionSource<A, V> {
    /// Return the actions this object currently offers, given the viewer's context.
    /// May return an empty vec if no actions are available (e.g., already captured city).
    fn available_actions(&self, context: &V) -> Vec<A>;
}

/// Collect actions from all sources. Convenience function that queries
/// multiple smart objects and flattens into a single action list.
pub fn collect_actions<A, V>(sources: &[&dyn ActionSource<A, V>], context: &V) -> Vec<A> {
    sources.iter()
        .flat_map(|s| s.available_actions(context))
        .collect()
}

// =========================================================================
// Coordinated assignment
// =========================================================================

/// Strategy for assigning N entities to M tasks given a score matrix.
///
/// Takes `scores[entity][task]` and returns `(entity, task, score)` tuples.
/// Different strategies have different semantics (optimal vs. greedy,
/// one-task-per-entity vs. contention-allowed, etc). Implementations may
/// mutate the score matrix during assignment (e.g. to apply coordination
/// adjustments between picks).
pub trait AssignmentStrategy {
    fn assign(&mut self, scores: &mut Vec<Vec<f64>>) -> Vec<(usize, usize, f64)>;
}

type NoopAdjust = fn(usize, usize, &mut Vec<Vec<f64>>);
fn noop_adjust(_: usize, _: usize, _: &mut Vec<Vec<f64>>) {}

/// Greedy assignment: globally pick the highest-scoring (entity, task) pair,
/// assign it, optionally adjust remaining scores, repeat until every entity
/// is assigned or no valid pair remains. Each entity is assigned at most once;
/// task reuse is controlled by the adjust callback.
///
/// This is the strongest strategy in empirical testing when coordination
/// callbacks are used (capture-spreading, focus fire, escort).
pub struct Greedy<F = NoopAdjust>
where
    F: FnMut(usize, usize, &mut Vec<Vec<f64>>),
{
    adjust: F,
}

impl Greedy<NoopAdjust> {
    /// Greedy assignment with no coordination adjustments.
    pub fn new() -> Self {
        Self { adjust: noop_adjust }
    }
}

impl Default for Greedy<NoopAdjust> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: FnMut(usize, usize, &mut Vec<Vec<f64>>)> Greedy<F> {
    /// Greedy assignment with an adjustment callback invoked after each
    /// pick: `(entity_idx, task_idx, &mut scores)`. Use it to diminish
    /// same-target contention, boost focus fire, zero out used tasks, etc.
    pub fn with_coordination(adjust: F) -> Self {
        Self { adjust }
    }
}

impl<F: FnMut(usize, usize, &mut Vec<Vec<f64>>)> AssignmentStrategy for Greedy<F> {
    fn assign(&mut self, scores: &mut Vec<Vec<f64>>) -> Vec<(usize, usize, f64)> {
        let num_entities = scores.len();
        let mut assigned = vec![false; num_entities];
        let mut assignments = Vec::new();

        for _ in 0..num_entities {
            let mut best_score = f64::NEG_INFINITY;
            let mut best_e = 0;
            let mut best_t = 0;

            for (ei, row) in scores.iter().enumerate() {
                if assigned[ei] { continue; }
                for (ti, &score) in row.iter().enumerate() {
                    if score > best_score {
                        best_score = score;
                        best_e = ei;
                        best_t = ti;
                    }
                }
            }

            if best_score == f64::NEG_INFINITY { break; }

            assigned[best_e] = true;
            assignments.push((best_e, best_t, best_score));
            (self.adjust)(best_e, best_t, scores);
        }

        assignments
    }
}

/// Round-robin assignment: entities pick in priority order (or index order
/// if unspecified), each taking their own highest-scoring task. Task reuse
/// is allowed unless suppressed by the coordination callback.
///
/// Use this when turn order or initiative matters — e.g. fastest units pick
/// first, and later units adapt via the adjust callback.
pub struct RoundRobin<F = NoopAdjust>
where
    F: FnMut(usize, usize, &mut Vec<Vec<f64>>),
{
    order: Option<Vec<usize>>,
    adjust: F,
}

impl RoundRobin<NoopAdjust> {
    /// Round-robin in entity index order, no coordination.
    pub fn new() -> Self {
        Self { order: None, adjust: noop_adjust }
    }

    /// Round-robin in the given priority order, no coordination.
    pub fn with_order(order: Vec<usize>) -> Self {
        Self { order: Some(order), adjust: noop_adjust }
    }
}

impl Default for RoundRobin<NoopAdjust> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: FnMut(usize, usize, &mut Vec<Vec<f64>>)> RoundRobin<F> {
    /// Round-robin with an adjustment callback after each pick.
    /// `order` is `None` for entity index order.
    pub fn with_coordination(order: Option<Vec<usize>>, adjust: F) -> Self {
        Self { order, adjust }
    }
}

impl<F: FnMut(usize, usize, &mut Vec<Vec<f64>>)> AssignmentStrategy for RoundRobin<F> {
    fn assign(&mut self, scores: &mut Vec<Vec<f64>>) -> Vec<(usize, usize, f64)> {
        let num_entities = scores.len();
        let order: Vec<usize> = self
            .order
            .clone()
            .unwrap_or_else(|| (0..num_entities).collect());
        let mut assignments = Vec::new();

        for ei in order {
            if ei >= num_entities { continue; }
            let mut best_score = f64::NEG_INFINITY;
            let mut best_t = 0;
            for (ti, &score) in scores[ei].iter().enumerate() {
                if score > best_score {
                    best_score = score;
                    best_t = ti;
                }
            }
            if best_score == f64::NEG_INFINITY { continue; }
            assignments.push((ei, best_t, best_score));
            (self.adjust)(ei, best_t, scores);
        }

        assignments
    }
}

/// Hungarian (Kuhn-Munkres) assignment: optimal one-to-one matching that
/// maximizes the total score. O(n³) in the larger dimension.
///
/// Each entity is assigned to at most one task; each task to at most one
/// entity. Score pairs of `f64::NEG_INFINITY` are treated as forbidden
/// (the algorithm never picks them). For rectangular matrices the excess
/// rows/columns remain unassigned.
///
/// Use this as the theoretical upper bound when evaluating `Greedy` — any
/// gap is the price of not looking ahead.
pub struct Hungarian;

impl Hungarian {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Hungarian {
    fn default() -> Self {
        Self::new()
    }
}

impl AssignmentStrategy for Hungarian {
    fn assign(&mut self, scores: &mut Vec<Vec<f64>>) -> Vec<(usize, usize, f64)> {
        let n = scores.len();
        if n == 0 { return vec![]; }
        let m = scores[0].len();
        if m == 0 { return vec![]; }

        let size = n.max(m);
        // Sentinel: strictly greater than any possible optimal sum, but
        // scaled to the input range to preserve f64 precision. A fixed 1e18
        // would cause catastrophic cancellation with small scores.
        let mut max_abs = 1.0f64;
        for row in scores.iter() {
            for &s in row.iter() {
                if s != f64::NEG_INFINITY && s.abs() > max_abs {
                    max_abs = s.abs();
                }
            }
        }
        let forbidden = max_abs * (size as f64 + 1.0) + 1.0;

        // Build square cost matrix for MINIMIZATION (negate scores).
        let mut c = vec![vec![forbidden; size]; size];
        for i in 0..n {
            for j in 0..m {
                let s = scores[i][j];
                c[i][j] = if s == f64::NEG_INFINITY { forbidden } else { -s };
            }
        }

        // Kuhn-Munkres with potentials. 1-indexed arrays; p[j] is the row
        // currently assigned to column j, 0 if none.
        let mut u = vec![0.0f64; size + 1];
        let mut v = vec![0.0f64; size + 1];
        let mut p = vec![0usize; size + 1];
        let mut way = vec![0usize; size + 1];

        for i in 1..=size {
            p[0] = i;
            let mut j0 = 0usize;
            let mut minv = vec![f64::INFINITY; size + 1];
            let mut used = vec![false; size + 1];

            loop {
                used[j0] = true;
                let i0 = p[j0];
                let mut delta = f64::INFINITY;
                let mut j1 = 0usize;

                for j in 1..=size {
                    if !used[j] {
                        let cur = c[i0 - 1][j - 1] - u[i0] - v[j];
                        if cur < minv[j] {
                            minv[j] = cur;
                            way[j] = j0;
                        }
                        if minv[j] < delta {
                            delta = minv[j];
                            j1 = j;
                        }
                    }
                }

                for j in 0..=size {
                    if used[j] {
                        u[p[j]] += delta;
                        v[j] -= delta;
                    } else {
                        minv[j] -= delta;
                    }
                }

                j0 = j1;
                if p[j0] == 0 { break; }
            }

            loop {
                let j1 = way[j0];
                p[j0] = p[j1];
                j0 = j1;
                if j0 == 0 { break; }
            }
        }

        // Extract real assignments: skip padded rows/cols and forbidden pairs.
        let mut assignments = Vec::new();
        for j in 1..=size {
            let i = p[j];
            if i == 0 { continue; }
            let ei = i - 1;
            let ti = j - 1;
            if ei >= n || ti >= m { continue; }
            let s = scores[ei][ti];
            if s == f64::NEG_INFINITY { continue; }
            assignments.push((ei, ti, s));
        }
        // Return in score-descending order for consistency with Greedy.
        assignments.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        assignments
    }
}

/// Weighted-random assignment: softmax-sample each entity's task from the
/// remaining unassigned tasks, using score as the logit. Temperature controls
/// randomness — low temperature is near-greedy, high temperature is near-uniform.
///
/// Entities are processed in index order. Seeded xorshift64 provides
/// reproducible results. Useful for exploration, diversity, or humanlike
/// unpredictability (opponent-proof against deterministic counters).
pub struct WeightedRandom {
    rng_state: u64,
    temperature: f64,
}

impl WeightedRandom {
    /// Default temperature of 1.0. Seed 0 is remapped to a non-zero value
    /// since xorshift64 degenerates on zero state.
    pub fn new(seed: u64) -> Self {
        let rng_state = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
        Self { rng_state, temperature: 1.0 }
    }

    /// Higher temperature = more random. Must be > 0.
    pub fn with_temperature(seed: u64, temperature: f64) -> Self {
        let mut s = Self::new(seed);
        s.temperature = temperature.max(1.0e-6);
        s
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    fn next_unit(&mut self) -> f64 {
        // Uniform [0, 1), 53-bit precision.
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

impl AssignmentStrategy for WeightedRandom {
    fn assign(&mut self, scores: &mut Vec<Vec<f64>>) -> Vec<(usize, usize, f64)> {
        let n = scores.len();
        if n == 0 { return vec![]; }
        let m = scores[0].len();
        if m == 0 { return vec![]; }

        let mut task_used = vec![false; m];
        let mut assignments = Vec::new();

        for ei in 0..n {
            // Find max over remaining valid tasks for numerical stability.
            let mut max_s = f64::NEG_INFINITY;
            for (ti, &s) in scores[ei].iter().enumerate() {
                if task_used[ti] { continue; }
                if s == f64::NEG_INFINITY { continue; }
                if s > max_s { max_s = s; }
            }
            if max_s == f64::NEG_INFINITY { continue; }

            // Compute softmax weights.
            let mut total = 0.0f64;
            let mut weights: Vec<(usize, f64)> = Vec::new();
            for (ti, &s) in scores[ei].iter().enumerate() {
                if task_used[ti] { continue; }
                if s == f64::NEG_INFINITY { continue; }
                let w = ((s - max_s) / self.temperature).exp();
                total += w;
                weights.push((ti, w));
            }
            if weights.is_empty() || total <= 0.0 { continue; }

            // Sample.
            let pick = self.next_unit() * total;
            let mut cumulative = 0.0f64;
            let mut chosen = weights[0].0;
            for (ti, w) in &weights {
                cumulative += *w;
                if cumulative >= pick {
                    chosen = *ti;
                    break;
                }
            }

            task_used[chosen] = true;
            assignments.push((ei, chosen, scores[ei][chosen]));
        }

        assignments
    }
}
