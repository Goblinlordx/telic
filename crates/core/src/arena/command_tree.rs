//! Tree of valid commands that a player can issue from a given game state.
//!
//! This replaces the "agent proposes a command, game rejects if invalid"
//! pattern with "game enumerates valid commands, agent picks from the tree."
//! A command-tree agent is architecturally unable to propose a command that
//! the game would reject.
//!
//! # Tree shape
//!
//! Each node is one of four variants:
//! - [`Layer`] — an intermediate branch with keyed children (e.g. a
//!   category like `"attack"` whose children are per-unit subtrees).
//! - [`Leaf`] — a fully-specified command, ready to submit.
//! - [`Parametric`] — a leaf that requires continuous parameter values
//!   (e.g. `RotateAim { yaw, pitch }`). The attached [`CommandBuilder`]
//!   knows the valid parameter domains and how to assemble a concrete
//!   command from chosen values.
//! - [`Empty`] — no valid command (e.g. it's not this player's turn).
//!
//! The layering is implementer-defined. A typical turn-based game might
//! use `action-type → unit → target`; a real-time game might use
//! `unit → verb` or a flat root with one child per commandable event.
//!
//! # Structural sharing
//!
//! Branch children are held behind [`Arc`]. An implementer that maintains
//! its own per-unit / per-subtree cache can reuse unchanged subtrees
//! across ticks by storing `Arc<CommandTree<C>>` and cloning the pointer.
//!
//! # Laziness
//!
//! Branches can be constructed eagerly as [`CommandTree::Layer`] or
//! deferred as [`CommandTree::LazyLayer`]. A lazy branch carries a
//! closure that produces its children on first access; the result is
//! cached in a `OnceLock` so subsequent reads are cheap. Agents that
//! never descend into a lazy branch never pay its enumeration cost —
//! this makes hierarchical traversal essentially free on the paths it
//! prunes.

use std::fmt;
use std::sync::{Arc, OnceLock};

/// Valid-parameter domain for a single scalar in a [`Parametric`] leaf.
#[derive(Debug, Clone)]
pub enum ParamDomain {
    /// A closed real-valued range `[min, max]`.
    Continuous { min: f64, max: f64 },
    /// A finite set of valid discrete values.
    Discrete(Vec<f64>),
    /// A closed integer range `[min, max]` represented as `f64`.
    Int { min: i64, max: i64 },
}

impl ParamDomain {
    /// Clamp a value to the domain. Best-effort — for [`Discrete`], returns
    /// the nearest valid value; for [`Int`], truncates; for [`Continuous`],
    /// clamps to `[min, max]`.
    pub fn clamp(&self, value: f64) -> f64 {
        match self {
            ParamDomain::Continuous { min, max } => value.clamp(*min, *max),
            ParamDomain::Int { min, max } => (value.round() as i64).clamp(*min, *max) as f64,
            ParamDomain::Discrete(values) => {
                if values.is_empty() { return value; }
                let mut best = values[0];
                let mut best_d = (best - value).abs();
                for &v in values.iter().skip(1) {
                    let d = (v - value).abs();
                    if d < best_d {
                        best_d = d;
                        best = v;
                    }
                }
                best
            }
        }
    }

    /// Midpoint of a continuous/int domain; the first element of a discrete set.
    /// A safe "default" value when the agent has no better policy.
    pub fn midpoint(&self) -> f64 {
        match self {
            ParamDomain::Continuous { min, max } => 0.5 * (*min + *max),
            ParamDomain::Int { min, max } => ((*min + *max) as f64) * 0.5,
            ParamDomain::Discrete(values) => values.first().copied().unwrap_or(0.0),
        }
    }
}

/// Assembles a concrete command from chosen parameter values.
///
/// Implemented by the game author for each continuous-parameter action.
/// Expected to be stateless and cheap — construct once per tick, store
/// inside a [`CommandTree::Parametric`] node.
///
/// The `Debug` bound lets trees be introspected and logged; UIs can call
/// [`CommandBuilder::describe`] for human-readable labels.
pub trait CommandBuilder<C>: fmt::Debug + Send + Sync {
    /// One domain per parameter. Agents must pass a `values` slice of the
    /// same length to [`CommandBuilder::build`].
    fn domains(&self) -> Vec<ParamDomain>;

    /// Construct the concrete command. `values.len()` must equal
    /// `self.domains().len()`; each value should be within its domain
    /// (the builder is not required to re-clamp).
    fn build(&self, values: &[f64]) -> C;

    /// Human-readable label. Defaults to the `Debug` representation.
    fn describe(&self) -> String {
        format!("{:?}", self)
    }
}

/// A tree of valid commands available to a single player at a single tick.
///
/// See the module-level docs for shape semantics.
pub enum CommandTree<C> {
    /// No valid command — typically returned for a non-current player in
    /// a turn-based game.
    Empty,

    /// A fully-specified command.
    Leaf(C),

    /// A leaf that requires continuous parameter choices.
    Parametric {
        label: String,
        builder: Arc<dyn CommandBuilder<C>>,
    },

    /// An intermediate branch. Children are keyed by a human-meaningful
    /// label (category, unit id as string, etc.) so hierarchical agents
    /// and UIs can route by key.
    Layer {
        label: String,
        children: Vec<(String, Arc<CommandTree<C>>)>,
    },

    /// A branch whose children are computed on first access and cached.
    /// Agents that never descend into a `LazyLayer` never invoke its
    /// `expand` closure — useful when per-branch enumeration is
    /// expensive (e.g. per-unit valid actions in a large RTS) and a
    /// hierarchical agent will only drill into a handful of branches.
    LazyLayer {
        label: String,
        expand: Arc<dyn Fn() -> Vec<(String, Arc<CommandTree<C>>)> + Send + Sync>,
        cache: OnceLock<Vec<(String, Arc<CommandTree<C>>)>>,
    },
}

impl<C: 'static> CommandTree<C> {
    /// Construct a lazy layer. The `expand` closure is called the first
    /// time any traversal helper reaches this node's children; its result
    /// is cached for the lifetime of the tree.
    pub fn lazy_layer<F>(label: impl Into<String>, expand: F) -> Self
    where
        F: Fn() -> Vec<(String, Arc<CommandTree<C>>)> + Send + Sync + 'static,
    {
        CommandTree::LazyLayer {
            label: label.into(),
            expand: Arc::new(expand),
            cache: OnceLock::new(),
        }
    }
}

impl<C: fmt::Debug> fmt::Debug for CommandTree<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandTree::Empty => write!(f, "Empty"),
            CommandTree::Leaf(c) => write!(f, "Leaf({:?})", c),
            CommandTree::Parametric { label, builder } => {
                write!(f, "Parametric({:?} -> {})", label, builder.describe())
            }
            CommandTree::Layer { label, children } => {
                f.debug_struct("Layer")
                    .field("label", label)
                    .field("child_count", &children.len())
                    .finish()
            }
            CommandTree::LazyLayer { label, cache, .. } => {
                f.debug_struct("LazyLayer")
                    .field("label", label)
                    .field("expanded", &cache.get().is_some())
                    .field("child_count", &cache.get().map(|v| v.len()))
                    .finish()
            }
        }
    }
}

impl<C> CommandTree<C> {
    /// True if the tree contains no issuable command.
    pub fn is_empty(&self) -> bool {
        matches!(self, CommandTree::Empty)
    }

    /// Children of a layer, or `None` if this node is a leaf/empty.
    /// For a [`CommandTree::LazyLayer`], the expand closure fires on the
    /// first call and the result is cached for subsequent calls.
    pub fn children(&self) -> Option<&[(String, Arc<CommandTree<C>>)]> {
        match self {
            CommandTree::Layer { children, .. } => Some(children.as_slice()),
            CommandTree::LazyLayer { expand, cache, .. } => {
                Some(cache.get_or_init(|| expand()).as_slice())
            }
            _ => None,
        }
    }

    /// This node's label — only layers and parametric leaves have one.
    pub fn label(&self) -> Option<&str> {
        match self {
            CommandTree::Layer { label, .. } => Some(label.as_str()),
            CommandTree::LazyLayer { label, .. } => Some(label.as_str()),
            CommandTree::Parametric { label, .. } => Some(label.as_str()),
            _ => None,
        }
    }

    /// Count every discrete leaf reachable from this node (ignores
    /// parametric leaves — they represent a continuous family, not a
    /// finite count).
    ///
    /// Walking into a [`CommandTree::LazyLayer`] forces its expansion.
    pub fn leaf_count(&self) -> usize {
        match self {
            CommandTree::Leaf(_) => 1,
            CommandTree::Layer { children, .. } => {
                children.iter().map(|(_, c)| c.leaf_count()).sum()
            }
            CommandTree::LazyLayer { .. } => {
                self.children()
                    .map(|cs| cs.iter().map(|(_, c)| c.leaf_count()).sum())
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Walk every discrete leaf. Parametric leaves are skipped — they
    /// have no single representative command. Forces expansion of every
    /// [`CommandTree::LazyLayer`] reached.
    pub fn for_each_leaf<F: FnMut(&C)>(&self, mut f: F) {
        fn walk<C, F: FnMut(&C)>(node: &CommandTree<C>, f: &mut F) {
            match node {
                CommandTree::Leaf(c) => f(c),
                CommandTree::Layer { children, .. } => {
                    for (_, child) in children {
                        walk(child, f);
                    }
                }
                CommandTree::LazyLayer { .. } => {
                    if let Some(children) = node.children() {
                        for (_, child) in children {
                            walk(child, f);
                        }
                    }
                }
                _ => {}
            }
        }
        walk(self, &mut f);
    }

    /// Find the first discrete leaf whose command matches the predicate.
    /// Forces expansion of each [`CommandTree::LazyLayer`] reached.
    pub fn find_leaf<F: Fn(&C) -> bool>(&self, pred: F) -> Option<&C> {
        fn walk<'a, C, F: Fn(&C) -> bool>(node: &'a CommandTree<C>, pred: &F) -> Option<&'a C> {
            match node {
                CommandTree::Leaf(c) if pred(c) => Some(c),
                CommandTree::Layer { children, .. } => {
                    for (_, child) in children {
                        if let Some(hit) = walk(child, pred) {
                            return Some(hit);
                        }
                    }
                    None
                }
                CommandTree::LazyLayer { .. } => {
                    if let Some(children) = node.children() {
                        for (_, child) in children {
                            if let Some(hit) = walk(child, pred) {
                                return Some(hit);
                            }
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        walk(self, &pred)
    }

    /// Find a child of a layer by its key. Forces expansion of a
    /// [`CommandTree::LazyLayer`] but only of this node — the matched
    /// child's subtree stays lazy.
    pub fn child(&self, key: &str) -> Option<&CommandTree<C>> {
        match self {
            CommandTree::Layer { children, .. } => children
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, child)| child.as_ref()),
            CommandTree::LazyLayer { .. } => {
                self.children()?
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, child)| child.as_ref())
            }
            _ => None,
        }
    }
}

impl<C: Clone> CommandTree<C> {
    /// Collect every discrete leaf command into a flat vector.
    /// Parametric leaves are excluded.
    pub fn flatten(&self) -> Vec<C> {
        let mut out = Vec::new();
        self.for_each_leaf(|c| out.push(c.clone()));
        out
    }

    /// Score every discrete leaf and return the highest-scoring one.
    /// Returns `None` if the tree has no discrete leaves.
    pub fn argmax<F: Fn(&C) -> f64>(&self, score: F) -> Option<C> {
        let mut best: Option<(C, f64)> = None;
        self.for_each_leaf(|c| {
            let s = score(c);
            if best.as_ref().map(|(_, b)| s > *b).unwrap_or(true) {
                best = Some((c.clone(), s));
            }
        });
        best.map(|(c, _)| c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Commands in these tests are ints for simplicity.
    type C = i32;

    fn leaf(c: C) -> Arc<CommandTree<C>> { Arc::new(CommandTree::Leaf(c)) }

    fn layer(label: &str, children: Vec<(&str, Arc<CommandTree<C>>)>) -> Arc<CommandTree<C>> {
        Arc::new(CommandTree::Layer {
            label: label.into(),
            children: children.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        })
    }

    #[test]
    fn empty_is_empty() {
        let t: CommandTree<C> = CommandTree::Empty;
        assert!(t.is_empty());
        assert_eq!(t.leaf_count(), 0);
        assert!(t.flatten().is_empty());
    }

    #[test]
    fn leaf_counts_itself() {
        let t = CommandTree::Leaf(42);
        assert!(!t.is_empty());
        assert_eq!(t.leaf_count(), 1);
        assert_eq!(t.flatten(), vec![42]);
    }

    #[test]
    fn layer_aggregates_children() {
        let tree = layer("root", vec![
            ("a", leaf(1)),
            ("b", layer("sub", vec![
                ("x", leaf(2)),
                ("y", leaf(3)),
            ])),
        ]);
        assert_eq!(tree.leaf_count(), 3);
        let mut flat = tree.flatten();
        flat.sort();
        assert_eq!(flat, vec![1, 2, 3]);
    }

    #[test]
    fn find_leaf_walks_tree() {
        let tree = layer("root", vec![
            ("a", leaf(1)),
            ("b", leaf(7)),
            ("c", leaf(3)),
        ]);
        assert_eq!(tree.find_leaf(|&x| x == 7), Some(&7));
        assert_eq!(tree.find_leaf(|&x| x == 99), None);
    }

    #[test]
    fn argmax_picks_highest_score() {
        let tree = layer("root", vec![
            ("a", leaf(1)),
            ("b", leaf(5)),
            ("c", leaf(3)),
        ]);
        let best = tree.argmax(|&c| c as f64);
        assert_eq!(best, Some(5));
    }

    #[test]
    fn child_lookup_by_key() {
        let tree = layer("root", vec![
            ("attack", leaf(10)),
            ("move", leaf(20)),
        ]);
        assert!(matches!(tree.child("attack"), Some(CommandTree::Leaf(10))));
        assert!(matches!(tree.child("move"), Some(CommandTree::Leaf(20))));
        assert!(tree.child("missing").is_none());
    }

    #[test]
    fn param_domain_clamps() {
        let d = ParamDomain::Continuous { min: 0.0, max: 10.0 };
        assert_eq!(d.clamp(-5.0), 0.0);
        assert_eq!(d.clamp(5.0), 5.0);
        assert_eq!(d.clamp(15.0), 10.0);
        assert_eq!(d.midpoint(), 5.0);
    }

    #[test]
    fn int_domain_rounds_and_clamps() {
        let d = ParamDomain::Int { min: 0, max: 5 };
        assert_eq!(d.clamp(2.7), 3.0);
        assert_eq!(d.clamp(-100.0), 0.0);
        assert_eq!(d.clamp(100.0), 5.0);
    }

    #[test]
    fn discrete_domain_snaps_to_nearest() {
        let d = ParamDomain::Discrete(vec![-1.0, 0.0, 1.0]);
        assert_eq!(d.clamp(-0.9), -1.0);
        assert_eq!(d.clamp(0.4), 0.0);
        assert_eq!(d.clamp(0.6), 1.0);
        assert_eq!(d.midpoint(), -1.0);  // first element
    }

    // A trivial parametric builder for testing.
    #[derive(Debug)]
    struct ScaleBy(f64);

    impl CommandBuilder<f64> for ScaleBy {
        fn domains(&self) -> Vec<ParamDomain> {
            vec![ParamDomain::Continuous { min: 0.0, max: 1.0 }]
        }
        fn build(&self, values: &[f64]) -> f64 {
            values[0] * self.0
        }
        fn describe(&self) -> String {
            format!("ScaleBy({})", self.0)
        }
    }

    #[test]
    fn parametric_leaf_builds_command() {
        let tree: CommandTree<f64> = CommandTree::Parametric {
            label: "scale".into(),
            builder: Arc::new(ScaleBy(10.0)),
        };
        if let CommandTree::Parametric { builder, .. } = &tree {
            let domains = builder.domains();
            assert_eq!(domains.len(), 1);
            let v = builder.build(&[0.5]);
            assert!((v - 5.0).abs() < 1e-9);
        } else {
            panic!("expected Parametric");
        }
    }

    #[test]
    fn debug_format_is_useful() {
        let tree: CommandTree<f64> = CommandTree::Parametric {
            label: "scale".into(),
            builder: Arc::new(ScaleBy(10.0)),
        };
        let s = format!("{:?}", tree);
        assert!(s.contains("scale"));
        assert!(s.contains("ScaleBy(10)"));
    }

    #[test]
    fn structural_sharing_works() {
        let shared = leaf(42);
        let tree = layer("root", vec![
            ("a", shared.clone()),
            ("b", shared.clone()),
        ]);
        // Both branches point to the same Arc — constructing the tree did
        // not clone the leaf itself.
        if let CommandTree::Layer { children, .. } = &*tree {
            assert!(Arc::ptr_eq(&children[0].1, &children[1].1));
        } else {
            panic!();
        }
        assert_eq!(tree.leaf_count(), 2);
    }

    // -------- LazyLayer tests --------

    #[test]
    fn lazy_layer_expands_on_access() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();

        let tree: CommandTree<C> = CommandTree::lazy_layer("root", move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            vec![("a".into(), leaf(1)), ("b".into(), leaf(2))]
        });

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let children = tree.children().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = tree.children().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_layer_flatten_works() {
        let tree: CommandTree<C> = CommandTree::lazy_layer("root", || {
            vec![("a".into(), leaf(1)), ("b".into(), leaf(2)), ("c".into(), leaf(3))]
        });
        let mut flat = tree.flatten();
        flat.sort();
        assert_eq!(flat, vec![1, 2, 3]);
        assert_eq!(tree.leaf_count(), 3);
    }

    #[test]
    fn lazy_layer_nested_inside_layer() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();

        let lazy = Arc::new(CommandTree::<C>::lazy_layer("expensive", move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            vec![("deep".into(), leaf(99))]
        }));
        let tree = layer("root", vec![
            ("cheap", leaf(1)),
            ("expensive", lazy),
        ]);

        // Targeted descent into the cheap branch: lazy branch stays unexpanded.
        let cheap = tree.child("cheap").unwrap();
        assert!(matches!(cheap, CommandTree::Leaf(1)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // Obtaining a reference to the lazy branch also does not force
        // expansion — only calling `children()` (or a helper that needs
        // them) forces it.
        let expensive_ref = tree.child("expensive").unwrap();
        assert!(matches!(expensive_ref, CommandTree::LazyLayer { .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // Now force expansion.
        let _ = expensive_ref.children();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_layer_child_lookup_forces_only_this_layer() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let outer = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(AtomicUsize::new(0));
        let outer_c = outer.clone();
        let inner_c = inner.clone();

        let tree: CommandTree<C> = CommandTree::lazy_layer("outer", move || {
            outer_c.fetch_add(1, Ordering::SeqCst);
            let inner_c2 = inner_c.clone();
            let inner_tree = Arc::new(CommandTree::<C>::lazy_layer("inner", move || {
                inner_c2.fetch_add(1, Ordering::SeqCst);
                vec![("x".into(), leaf(7))]
            }));
            vec![("nested".into(), inner_tree)]
        });

        let nested = tree.child("nested").unwrap();
        assert_eq!(outer.load(Ordering::SeqCst), 1);
        assert_eq!(inner.load(Ordering::SeqCst), 0);
        let _ = nested.children();
        assert_eq!(inner.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_layer_debug_marks_expansion_state() {
        let tree: CommandTree<C> = CommandTree::lazy_layer("root", || vec![
            ("a".into(), leaf(1)),
        ]);
        let s = format!("{:?}", tree);
        assert!(s.contains("LazyLayer"));
        assert!(s.contains("expanded: false"));
        // Trigger expansion then re-check Debug.
        let _ = tree.children();
        let s = format!("{:?}", tree);
        assert!(s.contains("expanded: true"));
        assert!(s.contains("child_count: Some(1)"));
    }
}
