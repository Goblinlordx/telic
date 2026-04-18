use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::utility::{UtilityAction, ResponseCurve, ScoringMode};

use crate::game::types::*;
use crate::game::state::SplendorView;

/// Splendor agent using the framework's `UtilityAction<S>`.
///
/// Each possible action is scored by a single `UtilityAction<ScoringCtx>`
/// with multiple considerations (points, engine, nobles, efficiency, blocking).
/// The considerations are additive — each contributes independently to the
/// total score. Best-scoring action wins.
///
/// Set `trace_enabled` to print per-consideration scoring breakdowns.
#[derive(Debug)]
pub struct UtilityAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    /// The scoring action — shared across all candidates, evaluates from ScoringCtx.
    scorer: UtilityAction<ScoringCtx>,
    /// If true, print scoring trace for each decision.
    pub trace_enabled: bool,
}

/// Context for scoring a single candidate action.
struct ScoringCtx {
    // Action info
    points_gained: f64,
    bonus_gained: Option<Gem>,
    gold_spent: f64,
    is_buy: bool,
    is_reserve: bool,
    is_take: bool,
    is_pass: bool,
    card_tier: f64,
    // Card efficiency: how much our bonuses discount this card
    discount: f64,
    // How much effective cost remains after bonuses
    effective_cost: f64,
    // Noble proximity: does this bonus reduce deficit toward any noble?
    noble_deficit_reduction: f64,
    // Closest noble deficit after this action
    best_noble_deficit: f64,
    // Blocking: opponent can buy this card next turn
    opp_can_buy_card: bool,
    opp_card_points: f64,
    // Gem taking: how many of the taken gems help us buy our best target?
    gems_toward_target: f64,
    // Game phase: 0.0 = early, 1.0 = late
    game_phase: f64,
    // Point lead (positive = we're ahead)
    point_lead: f64,
    // Whether this card is "free" (bonuses cover full cost)
    is_free: bool,

    // --- Opponent awareness ---
    // How close is the opponent to buying this specific card? (0 = can buy now)
    opp_gems_away: f64,
    // Opponent's closest noble deficit (how close are they to a noble)
    opp_best_noble_deficit: f64,
    // Does buying this card deny it from the opponent?
    opp_wants_this_card: bool,

    // --- Token scarcity ---
    // For gem-taking: how scarce are the taken gems? (avg bank remaining)
    gem_scarcity: f64,
    // For gem-taking: how many taken gems does the opponent need?
    gems_denied_to_opp: f64,
}

impl UtilityAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            scorer: Self::build_scorer(),
            trace_enabled: false,
        }
    }

    /// Enable scoring trace output (for debugging).
    pub fn with_trace(mut self) -> Self {
        self.trace_enabled = true;
        self
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn build_scorer() -> UtilityAction<ScoringCtx> {
        UtilityAction::new("splendor_action")
            .with_base(0.0)
            .with_mode(ScoringMode::Additive)
            // --- Points: direct victory progress ---
            .consider("points",
                |ctx: &ScoringCtx| {
                    let mut v = ctx.points_gained * 10.0;
                    // Points matter more in late game
                    v *= 1.0 + ctx.game_phase * 0.5;
                    v
                },
                ResponseCurve::Identity, 1.0)
            // --- Engine building: bonus value ---
            .consider("engine",
                |ctx: &ScoringCtx| {
                    if ctx.bonus_gained.is_none() { return 0.0; }
                    // Engine matters more early game
                    let phase_weight = 1.5 - ctx.game_phase;
                    // Noble-relevant bonuses are worth more
                    let noble_boost = if ctx.noble_deficit_reduction > 0.0 { 1.5 } else { 1.0 };
                    // Free cards are always great for engine
                    let free_boost = if ctx.is_free { 2.0 } else { 1.0 };
                    5.0 * phase_weight * noble_boost * free_boost
                },
                ResponseCurve::Identity, 1.0)
            // --- Noble proximity ---
            .consider("noble",
                |ctx: &ScoringCtx| {
                    if ctx.noble_deficit_reduction <= 0.0 { return 0.0; }
                    match ctx.best_noble_deficit as u8 {
                        0 => 15.0, // this action triggers noble (3 pts!)
                        1 => 10.0,
                        2 => 6.0,
                        3 => 3.0,
                        _ => 1.0,
                    }
                },
                ResponseCurve::Identity, 1.0)
            // --- Efficiency: prefer cheap buys ---
            .consider("efficiency",
                |ctx: &ScoringCtx| {
                    if !ctx.is_buy { return 0.0; }
                    // Reward discount from bonuses
                    let discount_bonus = ctx.discount * 2.0;
                    // Penalize gold spending
                    let gold_penalty = ctx.gold_spent * -2.0;
                    // Free cards are great
                    let free_bonus = if ctx.is_free { 8.0 } else { 0.0 };
                    discount_bonus + gold_penalty + free_bonus
                },
                ResponseCurve::Identity, 1.0)
            // --- Tier preference (game phase aware) ---
            .consider("tier",
                |ctx: &ScoringCtx| {
                    if !ctx.is_buy { return 0.0; }
                    if ctx.is_free { return 3.0; } // free = always good regardless of tier
                    match ctx.card_tier as u8 {
                        1 => {
                            // Tier 1: good early for engine, wasteful late
                            if ctx.game_phase < 0.3 { 2.0 }
                            else if ctx.effective_cost <= 2.0 { 1.0 }
                            else { -4.0 }
                        }
                        2 => 3.0, // tier 2 is solid mid-game
                        3 => {
                            // Tier 3: great if affordable
                            if ctx.effective_cost <= 4.0 { 5.0 } else { 2.0 }
                        }
                        _ => 0.0,
                    }
                },
                ResponseCurve::Identity, 1.0)
            // --- Blocking: deny opponent, but only cards we'd also want ---
            .consider("blocking",
                |ctx: &ScoringCtx| {
                    if !ctx.is_reserve { return 0.0; }
                    if !ctx.opp_can_buy_card && ctx.opp_gems_away > 1.0 { return 0.0; }

                    // Is this card useful to US? (points or good bonus)
                    let self_value = ctx.opp_card_points * 2.0; // we could buy it later from reserve

                    // Only block if the card has value to us too, OR opponent
                    // is about to win and we must deny
                    let opp_close_to_win = ctx.point_lead < -3.0;
                    if self_value <= 2.0 && !opp_close_to_win { return 0.0; }

                    let mut score = ctx.opp_card_points * 2.0;
                    if ctx.opp_can_buy_card { score += 4.0; } // they can buy NOW
                    if opp_close_to_win { score += 5.0; } // desperate denial
                    if ctx.point_lead > 5.0 { score *= 0.3; }
                    score
                },
                ResponseCurve::Identity, 1.0)
            // --- Gem taking: prefer gems that help buy our targets ---
            .consider("gem_targeting",
                |ctx: &ScoringCtx| {
                    if !ctx.is_take { return 0.0; }
                    ctx.gems_toward_target * 2.0
                },
                ResponseCurve::Identity, 1.0)
            // --- Opponent race: urgency when they're close ---
            .consider("opp_race",
                |ctx: &ScoringCtx| {
                    if !ctx.is_buy { return 0.0; }
                    let mut score = 0.0;
                    // Buying a card the opponent wants is a bonus, but only
                    // if the card is already good for us (points or engine value).
                    // Don't buy a useless card just to deny.
                    let card_is_good_for_us = ctx.points_gained >= 1.0
                        || ctx.is_free
                        || ctx.noble_deficit_reduction > 0.0;

                    if ctx.opp_wants_this_card && card_is_good_for_us {
                        score += 4.0;
                        score += ctx.opp_card_points; // higher urgency for high-point cards
                    }
                    // If opponent is close to a noble, rush our own high-point buys
                    if ctx.opp_best_noble_deficit <= 2.0 && ctx.points_gained >= 2.0 {
                        score += ctx.points_gained * 1.5;
                    }
                    score
                },
                ResponseCurve::Identity, 1.0)
            // --- Gem denial: taking gems the opponent needs ---
            .consider("gem_denial",
                |ctx: &ScoringCtx| {
                    if !ctx.is_take { return 0.0; }
                    let mut score = 0.0;
                    score += ctx.gems_denied_to_opp * 2.5;
                    score += ctx.gem_scarcity * 1.5;
                    if ctx.point_lead > 5.0 { score *= 0.4; }
                    score
                },
                ResponseCurve::Identity, 1.0)
            // --- Pass penalty ---
            .consider("pass_penalty",
                |ctx: &ScoringCtx| {
                    if ctx.is_pass { -20.0 } else { 0.0 }
                },
                ResponseCurve::Identity, 1.0)
    }

    // =========================================================================
    // Action enumeration + context building
    // =========================================================================

    fn enumerate_and_score(&mut self, view: &SplendorView) -> Action {
        let game_phase = (view.our_points.max(view.opp_points) as f64) / 15.0;
        let point_lead = view.our_points as f64 - view.opp_points as f64;

        // Find our best target card (most valuable affordable-soon card)
        let best_target = self.find_best_target(view);

        let mut best_action = Action::Pass;
        let mut best_score = f64::NEG_INFINITY;

        // Score all buy actions (market)
        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                let (can, gold_needed) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                if !can { continue; }

                let ctx = self.build_buy_ctx(card, gold_needed, view, game_phase, point_lead);
                let score = self.scorer.score(&ctx) + (self.xorshift() % 100) as f64 * 0.0001;

                if score > best_score {
                    best_score = score;
                    best_action = Action::Buy { tier: (tier + 1) as u8, index: idx };
                }
            }
        }

        // Score buy reserved
        for (idx, card) in view.our_reserved.iter().enumerate() {
            let (can, gold_needed) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
            if !can { continue; }

            let ctx = self.build_buy_ctx(card, gold_needed, view, game_phase, point_lead);
            let score = self.scorer.score(&ctx) + (self.xorshift() % 100) as f64 * 0.0001;

            if score > best_score {
                best_score = score;
                best_action = Action::BuyReserved { index: idx };
            }
        }

        // Score reserve actions
        if view.our_reserved.len() < 3 {
            for tier in 0..3 {
                for (idx, card) in view.market[tier].iter().enumerate() {
                    let ctx = self.build_reserve_ctx(card, view, game_phase, point_lead);
                    let score = self.scorer.score(&ctx) + (self.xorshift() % 100) as f64 * 0.0001;

                    if score > best_score {
                        best_score = score;
                        best_action = Action::Reserve { tier: (tier + 1) as u8, index: idx };
                    }
                }
            }
        }

        // Score take-3 combinations
        let available: Vec<Gem> = Gem::ALL.iter()
            .filter(|&&g| view.bank.get(g) > 0)
            .copied().collect();

        if available.len() >= 3 && view.our_tokens.total() + 3 <= 10 {
            for i in 0..available.len() {
                for j in (i+1)..available.len() {
                    for k in (j+1)..available.len() {
                        let gems = [available[i], available[j], available[k]];
                        let toward = best_target.as_ref()
                            .map(|t| self.gems_toward_card(&gems, t, view))
                            .unwrap_or(0.0);

                        let (denied, scarcity) = self.gem_take_metrics(&gems, view);
                        let ctx = ScoringCtx {
                            points_gained: 0.0, bonus_gained: None, gold_spent: 0.0,
                            is_buy: false, is_reserve: false, is_take: true, is_pass: false,
                            card_tier: 0.0, discount: 0.0, effective_cost: 0.0,
                            noble_deficit_reduction: 0.0, best_noble_deficit: 99.0,
                            opp_can_buy_card: false, opp_card_points: 0.0,
                            gems_toward_target: toward,
                            game_phase, point_lead, is_free: false,
                            opp_gems_away: 0.0,
                            opp_best_noble_deficit: self.opp_best_noble_deficit(view),
                            opp_wants_this_card: false,
                            gem_scarcity: scarcity,
                            gems_denied_to_opp: denied,
                        };
                        let score = self.scorer.score(&ctx) + (self.xorshift() % 100) as f64 * 0.0001;

                        if score > best_score {
                            best_score = score;
                            best_action = Action::TakeThree(gems);
                        }
                    }
                }
            }
        }

        // Score take-2
        for &g in &Gem::ALL {
            if view.bank.get(g) >= 4 && view.our_tokens.total() + 2 <= 10 {
                let gems = [g, g, g]; // just for scoring: 2 of same
                let toward = best_target.as_ref()
                    .map(|t| {
                        let need = t.cost.get(g).saturating_sub(view.our_bonuses[g.index()]);
                        let have = view.our_tokens.get(g);
                        (need.saturating_sub(have)).min(2) as f64
                    })
                    .unwrap_or(0.0);

                let (denied, scarcity) = self.gem_take_metrics(&[g, g], view);
                let ctx = ScoringCtx {
                    points_gained: 0.0, bonus_gained: None, gold_spent: 0.0,
                    is_buy: false, is_reserve: false, is_take: true, is_pass: false,
                    card_tier: 0.0, discount: 0.0, effective_cost: 0.0,
                    noble_deficit_reduction: 0.0, best_noble_deficit: 99.0,
                    opp_can_buy_card: false, opp_card_points: 0.0,
                    gems_toward_target: toward,
                    game_phase, point_lead, is_free: false,
                    opp_gems_away: 0.0,
                    opp_best_noble_deficit: self.opp_best_noble_deficit(view),
                    opp_wants_this_card: false,
                    gem_scarcity: scarcity,
                    gems_denied_to_opp: denied,
                };
                let score = self.scorer.score(&ctx) + (self.xorshift() % 100) as f64 * 0.0001;

                if score > best_score {
                    best_score = score;
                    best_action = Action::TakeTwo(g);
                }
            }
        }

        // Pass (fallback with penalty)
        let pass_ctx = ScoringCtx {
            points_gained: 0.0, bonus_gained: None, gold_spent: 0.0,
            is_buy: false, is_reserve: false, is_take: false, is_pass: true,
            card_tier: 0.0, discount: 0.0, effective_cost: 0.0,
            noble_deficit_reduction: 0.0, best_noble_deficit: 99.0,
            opp_can_buy_card: false, opp_card_points: 0.0,
            gems_toward_target: 0.0, game_phase, point_lead, is_free: false,
            opp_gems_away: 0.0,
            opp_best_noble_deficit: self.opp_best_noble_deficit(view),
            opp_wants_this_card: false,
            gem_scarcity: 0.0, gems_denied_to_opp: 0.0,
        };
        let pass_score = self.scorer.score(&pass_ctx);
        if pass_score > best_score {
            best_action = Action::Pass;
        }

        // Explainability: print scoring trace for the chosen action
        if self.trace_enabled {
            // Rebuild context for the winning action to get trace
            let trace_ctx = match &best_action {
                Action::Buy { tier, index } => {
                    let ti = (*tier - 1) as usize;
                    let card = &view.market[ti][*index];
                    let (_, gold) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                    Some(self.build_buy_ctx(card, gold, view, game_phase, point_lead))
                }
                Action::BuyReserved { index } => {
                    let card = &view.our_reserved[*index];
                    let (_, gold) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                    Some(self.build_buy_ctx(card, gold, view, game_phase, point_lead))
                }
                Action::Reserve { tier, index } => {
                    let ti = (*tier - 1) as usize;
                    let card = &view.market[ti][*index];
                    Some(self.build_reserve_ctx(card, view, game_phase, point_lead))
                }
                _ => None, // take/pass — less interesting to trace
            };

            if let Some(ctx) = trace_ctx {
                let trace = self.scorer.score_with_trace(&ctx);
                println!("[turn {}] chose {:?} (score {:.2})", view.turn, best_action, best_score);
                print!("{}", trace);
            }
        }

        best_action
    }

    // =========================================================================
    // Context builders
    // =========================================================================

    fn build_buy_ctx(&self, card: &Card, gold_needed: u8, view: &SplendorView,
                     game_phase: f64, point_lead: f64) -> ScoringCtx {
        let nominal_cost: u8 = card.cost.gems.iter().sum();
        let effective_cost: u8 = Gem::ALL.iter()
            .map(|&g| card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]))
            .sum();
        let discount = nominal_cost.saturating_sub(effective_cost) as f64;

        let (noble_deficit_reduction, best_noble_deficit) =
            self.noble_impact(card.bonus, view);

        let opp_gems = self.opp_gems_away_from(card, view);
        let opp_wants = opp_gems <= 2.0 && card.points >= 2;

        ScoringCtx {
            points_gained: card.points as f64,
            bonus_gained: Some(card.bonus),
            gold_spent: gold_needed as f64,
            is_buy: true, is_reserve: false, is_take: false, is_pass: false,
            card_tier: card.tier as f64,
            discount,
            effective_cost: effective_cost as f64,
            noble_deficit_reduction,
            best_noble_deficit,
            opp_can_buy_card: false,
            opp_card_points: card.points as f64,
            gems_toward_target: 0.0,
            game_phase, point_lead,
            is_free: effective_cost == 0,
            opp_gems_away: opp_gems,
            opp_best_noble_deficit: self.opp_best_noble_deficit(view),
            opp_wants_this_card: opp_wants,
            gem_scarcity: 0.0,
            gems_denied_to_opp: 0.0,
        }
    }

    fn build_reserve_ctx(&self, card: &Card, view: &SplendorView,
                         game_phase: f64, point_lead: f64) -> ScoringCtx {
        let (opp_can, _) = view.opp_tokens.can_afford(&card.cost, &view.opp_bonuses);
        let opp_gems = self.opp_gems_away_from(card, view);

        ScoringCtx {
            points_gained: 0.0, bonus_gained: None, gold_spent: 0.0,
            is_buy: false, is_reserve: true, is_take: false, is_pass: false,
            card_tier: card.tier as f64,
            discount: 0.0, effective_cost: 0.0,
            noble_deficit_reduction: 0.0, best_noble_deficit: 99.0,
            opp_can_buy_card: opp_can,
            opp_card_points: card.points as f64,
            gems_toward_target: 0.0,
            game_phase, point_lead, is_free: false,
            opp_gems_away: opp_gems,
            opp_best_noble_deficit: self.opp_best_noble_deficit(view),
            opp_wants_this_card: opp_can && card.points >= 3,
            gem_scarcity: 0.0,
            gems_denied_to_opp: 0.0,
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// How close is the opponent to their nearest noble?
    fn opp_best_noble_deficit(&self, view: &SplendorView) -> f64 {
        let mut best = 99.0;
        for noble in &view.nobles {
            let deficit: u8 = Gem::ALL.iter()
                .map(|&g| noble.required[g.index()].saturating_sub(view.opp_bonuses[g.index()]))
                .sum();
            if (deficit as f64) < best { best = deficit as f64; }
        }
        best
    }

    /// How many gems away is the opponent from buying a card?
    fn opp_gems_away_from(&self, card: &Card, view: &SplendorView) -> f64 {
        let mut needed = 0u8;
        for &g in &Gem::ALL {
            let effective = card.cost.get(g).saturating_sub(view.opp_bonuses[g.index()]);
            needed += effective.saturating_sub(view.opp_tokens.get(g));
        }
        needed as f64
    }

    /// For gem-taking: compute denial and scarcity metrics.
    fn gem_take_metrics(&self, gems: &[Gem], view: &SplendorView) -> (f64, f64) {
        let mut denied = 0.0;
        let mut scarcity = 0.0;

        for &g in gems {
            // Does opponent need this gem? Check if they have few and cards cost it
            let opp_has = view.opp_tokens.get(g);
            let bank_has = view.bank.get(g);

            // Scarcity: fewer in bank = more valuable to take
            if bank_has <= 2 { scarcity += 1.0; }
            if bank_has <= 1 { scarcity += 1.0; }

            // Denial: opponent has few of this gem and likely needs it
            if opp_has < 2 {
                // Check if any market cards need this gem
                let opp_needs_gem = view.market.iter().flatten()
                    .any(|c| {
                        let eff = c.cost.get(g).saturating_sub(view.opp_bonuses[g.index()]);
                        eff > opp_has && c.points >= 2
                    });
                if opp_needs_gem { denied += 1.0; }
            }
        }

        (denied, scarcity)
    }

    /// How much does gaining this bonus reduce our deficit toward any noble?
    fn noble_impact(&self, bonus: Gem, view: &SplendorView) -> (f64, f64) {
        let mut best_reduction = 0.0;
        let mut best_deficit = 99.0;

        for noble in &view.nobles {
            let current_deficit: u8 = Gem::ALL.iter()
                .map(|&g| noble.required[g.index()].saturating_sub(view.our_bonuses[g.index()]))
                .sum();

            let deficit_of_this_gem = noble.required[bonus.index()]
                .saturating_sub(view.our_bonuses[bonus.index()]);

            if deficit_of_this_gem > 0 {
                let new_deficit = current_deficit - 1;
                let reduction = 1.0;
                if reduction > best_reduction || new_deficit < best_deficit as u8 {
                    best_reduction = reduction;
                    best_deficit = new_deficit as f64;
                }
            }
        }

        (best_reduction, best_deficit)
    }

    /// Find the most valuable card we're close to affording.
    fn find_best_target(&self, view: &SplendorView) -> Option<Card> {
        let mut best: Option<(f64, Card)> = None;

        for tier in 0..3 {
            for card in &view.market[tier] {
                let effective_cost: u8 = Gem::ALL.iter()
                    .map(|&g| {
                        let need = card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]);
                        need.saturating_sub(view.our_tokens.get(g))
                    }).sum();

                if effective_cost > 6 { continue; } // too far away

                let value = card.points as f64 * 10.0
                    + if effective_cost == 0 { 50.0 } else { 0.0 }
                    - effective_cost as f64 * 2.0;

                if best.as_ref().map_or(true, |(v, _)| value > *v) {
                    best = Some((value, card.clone()));
                }
            }
        }

        best.map(|(_, c)| c)
    }

    /// How many of these 3 gems help us buy the target card?
    fn gems_toward_card(&self, gems: &[Gem; 3], target: &Card, view: &SplendorView) -> f64 {
        let mut count = 0.0;
        for &g in gems {
            let need = target.cost.get(g).saturating_sub(view.our_bonuses[g.index()]);
            let have = view.our_tokens.get(g);
            if have < need {
                count += 1.0;
            }
        }
        count
    }
}


impl UtilityAgent {
    fn compute_command(&mut self, view: &SplendorView) -> Action {
        self.enumerate_and_score(view)
    }
}

impl GameAgent<SplendorView, Action> for UtilityAgent {
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
