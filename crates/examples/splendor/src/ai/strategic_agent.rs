use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SplendorView;

/// A composable strategy — scores a potential action given the game state.
/// Multiple strategies can be combined with weights.
pub trait Strategy: std::fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn score_action(&self, action: &ScoredAction, view: &SplendorView) -> f64;
}

/// An action with pre-computed context for strategies to evaluate.
#[derive(Debug, Clone)]
pub struct ScoredAction {
    pub action: Action,
    /// If this is a buy, the card being bought.
    pub card: Option<Card>,
    /// Points gained from this action.
    pub points_gained: u8,
    /// Bonus gem gained.
    pub bonus_gained: Option<Gem>,
    /// Gold spent.
    pub gold_spent: u8,
}

// --- Composable strategies ---

/// Buy the highest-point card available.
#[derive(Debug)]
pub struct PointsStrategy;

impl Strategy for PointsStrategy {
    fn name(&self) -> &str { "points" }
    fn score_action(&self, sa: &ScoredAction, _view: &SplendorView) -> f64 {
        sa.points_gained as f64 * 10.0
    }
}

/// Prioritize building an engine (bonuses) over raw points.
#[derive(Debug)]
pub struct EngineStrategy;

impl Strategy for EngineStrategy {
    fn name(&self) -> &str { "engine" }
    fn score_action(&self, sa: &ScoredAction, view: &SplendorView) -> f64 {
        if let Some(gem) = sa.bonus_gained {
            let current = view.our_bonuses[gem.index()];
            if current == 0 {
                8.0 // new color = high value
            } else if current <= 2 {
                4.0
            } else {
                1.0 // diminishing returns
            }
        } else {
            0.0
        }
    }
}

/// Skip tier 1, go straight for tier 2/3.
#[derive(Debug)]
pub struct SkipTier1Strategy;

impl Strategy for SkipTier1Strategy {
    fn name(&self) -> &str { "skip_t1" }
    fn score_action(&self, sa: &ScoredAction, view: &SplendorView) -> f64 {
        if let Some(ref card) = sa.card {
            match card.tier {
                1 => {
                    // Penalize tier 1 buys unless they're free (all bonuses cover cost)
                    let total_cost: u8 = Gem::ALL.iter()
                        .map(|&g| card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]))
                        .sum();
                    if total_cost == 0 {
                        3.0 // free card, sure
                    } else {
                        -8.0 // spending gems on tier 1 is wasteful
                    }
                }
                2 => 5.0,
                3 => 8.0,
                _ => 0.0,
            }
        } else {
            0.0
        }
    }
}

/// Target nobles — prefer bonuses that get us closer to attracting a noble.
#[derive(Debug)]
pub struct NobleStrategy;

impl Strategy for NobleStrategy {
    fn name(&self) -> &str { "noble" }
    fn score_action(&self, sa: &ScoredAction, view: &SplendorView) -> f64 {
        let Some(gem) = sa.bonus_gained else { return 0.0 };

        let mut score = 0.0;
        for noble in &view.nobles {
            let deficit = noble.required[gem.index()]
                .saturating_sub(view.our_bonuses[gem.index()]);
            if deficit > 0 {
                // How close are we to this noble overall?
                let total_deficit: u8 = Gem::ALL.iter()
                    .map(|&g| noble.required[g.index()].saturating_sub(view.our_bonuses[g.index()]))
                    .sum();
                if total_deficit <= 3 {
                    score += 12.0; // very close to noble
                } else if total_deficit <= 5 {
                    score += 5.0;
                }
            }
        }
        score
    }
}

/// Block opponent — take gems or reserve cards they need.
#[derive(Debug)]
pub struct BlockStrategy;

impl Strategy for BlockStrategy {
    fn name(&self) -> &str { "block" }
    fn score_action(&self, sa: &ScoredAction, view: &SplendorView) -> f64 {
        let mut score = 0.0;

        match &sa.action {
            Action::TakeThree(_) | Action::TakeTwo(_) => {
                let gems_taken: Vec<Gem> = match &sa.action {
                    Action::TakeThree(g) => g.to_vec(),
                    Action::TakeTwo(g) => vec![*g, *g],
                    _ => vec![],
                };

                for &g in &gems_taken {
                    // Check if opponent seems to want this gem
                    // (they have cards that cost this gem and few tokens of it)
                    let opp_wants = view.opp_tokens.get(g) < 2;
                    if opp_wants {
                        score += 2.0;
                    }
                }
            }
            Action::Reserve { tier, index } => {
                let ti = (*tier - 1) as usize;
                if ti < 3 && *index < view.market[ti].len() {
                    let card = &view.market[ti][*index];
                    // Is the opponent close to affording this card?
                    let (opp_can, _) = view.opp_tokens.can_afford(&card.cost, &view.opp_bonuses);
                    if opp_can && card.points >= 3 {
                        score += 15.0; // deny high-point card opponent can buy next turn
                    } else if card.points >= 4 {
                        score += 8.0; // deny any high-point card
                    }
                }
            }
            _ => {}
        }

        // Penalty: don't block if we're far ahead
        let point_lead = view.our_points as i32 - view.opp_points as i32;
        if point_lead > 5 {
            score *= 0.3; // don't need to block when winning big
        }

        score
    }
}

/// Efficiency strategy — prefer actions that don't waste resources.
#[derive(Debug)]
pub struct EfficiencyStrategy;

impl Strategy for EfficiencyStrategy {
    fn name(&self) -> &str { "efficiency" }
    fn score_action(&self, sa: &ScoredAction, view: &SplendorView) -> f64 {
        let mut score = 0.0;

        // Penalize passing
        if matches!(sa.action, Action::Pass) {
            return -20.0;
        }

        // Prefer buying cards that are cheap for us (bonuses cover most cost)
        if let Some(ref card) = sa.card {
            let actual_cost: u8 = Gem::ALL.iter()
                .map(|&g| card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]))
                .sum();
            let nominal_cost: u8 = card.cost.gems.iter().sum();
            let discount = nominal_cost.saturating_sub(actual_cost);
            score += discount as f64 * 2.0; // reward efficient buys
        }

        // Penalize taking gems we already have a lot of
        if let Action::TakeThree(gems) = &sa.action {
            for &g in gems {
                if view.our_tokens.get(g) >= 3 {
                    score -= 2.0; // diminishing returns
                }
            }
        }

        // Penalize gold spending
        score -= sa.gold_spent as f64 * 1.5;

        score
    }
}

// --- The composable agent ---

/// Strategic agent — combines multiple strategies with weights.
#[derive(Debug)]
pub struct StrategicAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    strategies: Vec<(f64, Box<dyn Strategy>)>, // (weight, strategy)
}

impl StrategicAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            strategies: Vec::new(),
        }
    }

    pub fn with_strategy(mut self, weight: f64, strategy: impl Strategy + 'static) -> Self {
        self.strategies.push((weight, Box::new(strategy)));
        self
    }

    /// Pre-built: balanced strategy.
    pub fn balanced(name: impl Into<String>, seed: u64) -> Self {
        Self::new(name, seed)
            .with_strategy(1.0, PointsStrategy)
            .with_strategy(0.8, EngineStrategy)
            .with_strategy(0.5, NobleStrategy)
            .with_strategy(0.5, BlockStrategy)
            .with_strategy(0.7, EfficiencyStrategy)
    }

    /// Pre-built: rush high tiers.
    pub fn rusher(name: impl Into<String>, seed: u64) -> Self {
        Self::new(name, seed)
            .with_strategy(1.5, PointsStrategy)
            .with_strategy(0.3, EngineStrategy)
            .with_strategy(1.2, SkipTier1Strategy)
            .with_strategy(0.3, NobleStrategy)
            .with_strategy(0.5, EfficiencyStrategy)
    }

    /// Pre-built: engine builder.
    pub fn engine_builder(name: impl Into<String>, seed: u64) -> Self {
        Self::new(name, seed)
            .with_strategy(0.5, PointsStrategy)
            .with_strategy(1.5, EngineStrategy)
            .with_strategy(1.0, NobleStrategy)
            .with_strategy(0.3, BlockStrategy)
            .with_strategy(0.8, EfficiencyStrategy)
    }

    /// Pre-built: blocker.
    pub fn blocker(name: impl Into<String>, seed: u64) -> Self {
        Self::new(name, seed)
            .with_strategy(0.8, PointsStrategy)
            .with_strategy(0.5, EngineStrategy)
            .with_strategy(0.5, NobleStrategy)
            .with_strategy(1.5, BlockStrategy)
            .with_strategy(0.5, EfficiencyStrategy)
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn enumerate_actions(&self, view: &SplendorView) -> Vec<ScoredAction> {
        let mut actions = Vec::new();

        // Buy actions (market)
        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                let (can, gold_needed) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                if can {
                    actions.push(ScoredAction {
                        action: Action::Buy { tier: (tier + 1) as u8, index: idx },
                        card: Some(card.clone()),
                        points_gained: card.points,
                        bonus_gained: Some(card.bonus),
                        gold_spent: gold_needed,
                    });
                }
            }
        }

        // Buy reserved
        for (idx, card) in view.our_reserved.iter().enumerate() {
            let (can, gold_needed) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
            if can {
                actions.push(ScoredAction {
                    action: Action::BuyReserved { index: idx },
                    card: Some(card.clone()),
                    points_gained: card.points,
                    bonus_gained: Some(card.bonus),
                    gold_spent: gold_needed,
                });
            }
        }

        // Reserve actions
        if view.our_reserved.len() < 3 {
            for tier in 0..3 {
                for (idx, card) in view.market[tier].iter().enumerate() {
                    actions.push(ScoredAction {
                        action: Action::Reserve { tier: (tier + 1) as u8, index: idx },
                        card: Some(card.clone()),
                        points_gained: 0,
                        bonus_gained: None,
                        gold_spent: 0,
                    });
                }
            }
        }

        // Take 3 different gems
        let available: Vec<Gem> = Gem::ALL.iter()
            .filter(|&&g| view.bank.get(g) > 0)
            .copied().collect();

        if available.len() >= 3 && view.our_tokens.total() + 3 <= 10 {
            // Generate a few gem combinations (not all — too many)
            for i in 0..available.len() {
                for j in (i+1)..available.len() {
                    for k in (j+1)..available.len() {
                        actions.push(ScoredAction {
                            action: Action::TakeThree([available[i], available[j], available[k]]),
                            card: None,
                            points_gained: 0,
                            bonus_gained: None,
                            gold_spent: 0,
                        });
                    }
                }
            }
        }

        // Take 2 of same
        for &g in &Gem::ALL {
            if view.bank.get(g) >= 4 && view.our_tokens.total() + 2 <= 10 {
                actions.push(ScoredAction {
                    action: Action::TakeTwo(g),
                    card: None,
                    points_gained: 0,
                    bonus_gained: None,
                    gold_spent: 0,
                });
            }
        }

        // Pass (fallback)
        actions.push(ScoredAction {
            action: Action::Pass,
            card: None,
            points_gained: 0,
            bonus_gained: None,
            gold_spent: 0,
        });

        actions
    }
}


impl StrategicAgent {
    fn compute_command(&mut self, view: &SplendorView) -> Action {
        let candidates = self.enumerate_actions(view);

        let mut best_action = Action::Pass;
        let mut best_score = f64::NEG_INFINITY;

        for sa in &candidates {
            let mut total = 0.0;
            for (weight, strategy) in &self.strategies {
                total += weight * strategy.score_action(sa, view);
            }
            // Tiny tiebreaker
            total += (self.xorshift() % 100) as f64 * 0.0001;

            if total > best_score {
                best_score = total;
                best_action = sa.action.clone();
            }
        }

        best_action
    }
}

impl GameAgent<SplendorView, Action> for StrategicAgent {
    fn name(&self) -> &str { &self.name }
    fn reset(&mut self, player: PlayerIndex) { self.player = player; }
    fn observe(&mut self, _view: &SplendorView) {}

    fn decide(
        &mut self,
        view: &SplendorView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
