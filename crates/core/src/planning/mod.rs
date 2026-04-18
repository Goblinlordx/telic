//! Combined GOAP + HTN + Utility AI planning framework.
//!
//! # Architecture
//!
//! Three complementary systems that can be composed in any direction:
//!
//! - **GOAP**: Goal-oriented backward-chaining planner. Boolean beliefs,
//!   preconditions/effects, finds action sequences to achieve goals.
//! - **HTN**: Hierarchical task decomposition. Compound tasks break into
//!   subtasks via methods. Structure for multi-step operations.
//! - **Utility AI**: Numeric belief scoring with response curves. Evaluates
//!   "how useful" each possible action is given current world state.
//!
//! Common patterns:
//! - **Utility → GOAP**: Utility scores "what to do", GOAP plans "how"
//! - **GOAP → HTN → Utility**: GOAP picks goal, HTN structures the plan,
//!   Utility scores individual unit/task assignments
//!
//! # Core types
//!
//! - [`Belief`] — named evaluation (boolean or numeric) about the world
//! - [`ActionDef`] — preconditions + effects + strategy (GOAP action)
//! - [`Goal`] — priority + desired effects
//! - [`Task`] — HTN task (primitive, compound, or dynamic)
//! - [`GoapPlanner`] — backward-chaining search (DFS, A*, Bidirectional)
//! - [`UtilityAction`] — action scored by weighted considerations
//! - [`ResponseCurve`] — maps belief values to 0-1 scores
//! - [`Agent`] — ties everything together, runs the planning loop

pub mod belief;
pub mod action;
pub mod goal;
pub mod planner;
pub mod htn;
pub mod utility;
pub mod agent;
