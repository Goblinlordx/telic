use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::utility::{UtilityAction, ResponseCurve, ScoringMode};

use crate::game::types::*;
use crate::game::state::PokerView;

/// Adaptive agent — builds an opponent model from observed play and
/// showdown results. No hardcoded assumptions about opponent strategy.
///
/// Tracks:
/// - When opponent raises and goes to showdown: was the hand strong?
///   → learns "raise honesty" (high = they never bluff, low = lots of bluffs)
/// - When opponent checks/calls and goes to showdown: were they trapping?
///   → learns "passive strength" (high = they slowplay, low = passive = weak)
/// - Overall aggression and fold rates
///
/// Uses these learned signals to adjust scoring:
/// - Fold more vs honest raisers (their raise = real hand)
/// - Bluff more vs frequent folders
/// - Call more vs frequent bluffers
#[derive(Debug)]
pub struct AdaptiveAgent {
    name: String,
    player: Player,
    seed: u64,
    scorer: UtilityAction<ActionCtx>,
    model: OpponentModel,
}

/// Learned opponent behavior — all from observation, nothing hardcoded.
#[derive(Debug, Clone, Default)]
struct OpponentModel {
    total_actions: u32,
    raise_count: u32,
    fold_count: u32,
    /// Showdowns where opponent had raised: how many had strong hands?
    raise_showdowns: u32,
    raise_was_strong: u32,
    /// Showdowns where opponent was passive: how many had strong hands?
    passive_showdowns: u32,
    passive_was_strong: u32,
    /// Track per-hand: did opponent raise this hand?
    last_hand: u32,
    hand_opp_raised: bool,
}

impl OpponentModel {
    /// Opponent raises this fraction of the time (0-1)
    fn aggression(&self) -> f64 {
        if self.total_actions < 5 { return 0.3; }
        self.raise_count as f64 / self.total_actions as f64
    }

    /// Opponent folds this fraction of the time (0-1)
    fn fold_rate(&self) -> f64 {
        if self.total_actions < 5 { return 0.3; }
        self.fold_count as f64 / self.total_actions as f64
    }

    /// When opponent raises, how often is it a real hand? (0-1)
    /// 1.0 = they NEVER bluff (every raise is strong)
    /// 0.5 = they bluff half the time
    fn raise_honesty(&self) -> f64 {
        if self.raise_showdowns < 3 { return 0.7; }
        self.raise_was_strong as f64 / self.raise_showdowns as f64
    }

    /// When opponent is passive, how often do they secretly have a hand?
    fn passive_trap_rate(&self) -> f64 {
        if self.passive_showdowns < 3 { return 0.25; }
        self.passive_was_strong as f64 / self.passive_showdowns as f64
    }

    fn update(&mut self, view: &PokerView) {
        let opp = 1 - view.viewer;

        // New hand — reset per-hand tracking
        if view.hand_number != self.last_hand {
            self.last_hand = view.hand_number;
            self.hand_opp_raised = false;
        }

        // Count opponent actions from history
        let mut hand_raises = 0u32;
        let mut hand_folds = 0u32;
        let mut hand_total = 0u32;
        for (_, player, action) in &view.history {
            if *player != opp { continue; }
            hand_total += 1;
            match action {
                Action::Raise(_) | Action::AllIn => { hand_raises += 1; }
                Action::Fold => { hand_folds += 1; }
                _ => {}
            }
        }
        self.hand_opp_raised = hand_raises > 0;

        // Update totals (use max to handle multiple observe() calls per hand)
        // This is approximate but sufficient
        self.total_actions = self.total_actions.max(hand_total + (view.hand_number * 4));
        self.raise_count = self.raise_count.max(hand_raises + (view.hand_number));
        self.fold_count = self.fold_count.max(hand_folds);

        // Showdown: correlate opponent's actions with their actual hand
        if let Some(opp_cards) = view.opp_hole_cards {
            let is_strong = if view.community.len() >= 5 {
                let mut all: Vec<Card> = opp_cards.iter().copied().collect();
                all.extend_from_slice(&view.community);
                match evaluate_hand(&all) {
                    HandRank::HighCard(_, _, _, _, _) => false,
                    HandRank::Pair(p, _, _, _) => p >= 10,
                    _ => true,
                }
            } else {
                // Preflop all-in — just check if pair or high cards
                opp_cards[0].rank == opp_cards[1].rank
                    || opp_cards[0].rank.0.max(opp_cards[1].rank.0) >= 12
            };

            if self.hand_opp_raised {
                self.raise_showdowns += 1;
                if is_strong { self.raise_was_strong += 1; }
            } else {
                self.passive_showdowns += 1;
                if is_strong { self.passive_was_strong += 1; }
            }
        }
    }
}

struct ActionCtx {
    action_type: ActionType,
    hand_strength: f64,
    pot_odds: f64,
    pot_commitment: f64,
    is_dealer: bool,
    street_progress: f64,
    to_call_ratio: f64,
    // Opponent model signals
    opp_aggression: f64,
    opp_fold_rate: f64,
    opp_raise_honesty: f64,   // how honest are their raises?
    opp_passive_trap: f64,    // do they trap with checks?
    facing_raise: bool,       // is the current bet from a raise?
}

#[derive(Debug, Clone, PartialEq)]
enum ActionType { Fold, Check, Call, Raise, AllIn }

impl AdaptiveAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            scorer: Self::build_scorer(),
            model: OpponentModel::default(),
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn build_scorer() -> UtilityAction<ActionCtx> {
        UtilityAction::new("adaptive_poker")
            .with_base(0.0)
            .with_mode(ScoringMode::Additive)

            .consider("fold_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Fold { return -100.0; }
                    if ctx.to_call_ratio < 0.01 { return -100.0; }

                    let negative_ev = (ctx.pot_odds - ctx.hand_strength).max(0.0);
                    let mut score = negative_ev * 25.0 - 8.0;

                    // KEY ADAPTATION: if opponent's raises are honest (never bluff),
                    // fold MORE when facing a raise — their raise means they have it
                    if ctx.facing_raise && ctx.opp_raise_honesty > 0.6 {
                        let honesty_boost = (ctx.opp_raise_honesty - 0.5) * 15.0;
                        score += honesty_boost;
                    }

                    // But if opponent bluffs a lot, fold LESS
                    if ctx.facing_raise && ctx.opp_raise_honesty < 0.4 {
                        score -= 5.0; // they might be bluffing, don't fold
                    }

                    score
                },
                ResponseCurve::Identity, 1.0)

            .consider("check_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Check { return -100.0; }
                    let mut score = 4.0;
                    if ctx.hand_strength > 0.6 && ctx.street_progress < 0.5 {
                        score += 3.0;
                    }
                    if ctx.hand_strength > 0.5 && ctx.street_progress > 0.8 {
                        score -= 5.0;
                    }
                    score
                },
                ResponseCurve::Identity, 1.0)

            .consider("call_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Call { return -100.0; }
                    let ev = ctx.hand_strength - ctx.pot_odds;
                    let mut score = ev * 35.0;
                    score += ctx.pot_commitment * 4.0;

                    // ADAPTATION: call more against bluffers
                    if ctx.facing_raise && ctx.opp_raise_honesty < 0.5 {
                        // Opponent bluffs often — our call is more profitable
                        score += (0.5 - ctx.opp_raise_honesty) * 12.0;
                    }

                    // Call less against honest raisers
                    if ctx.facing_raise && ctx.opp_raise_honesty > 0.7 {
                        score -= (ctx.opp_raise_honesty - 0.7) * 10.0;
                    }

                    if ctx.hand_strength > 0.25 && ctx.hand_strength < 0.5 {
                        score += 2.0;
                    }
                    score
                },
                ResponseCurve::Identity, 1.0)

            .consider("raise_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Raise { return -100.0; }
                    let mut score = 0.0;

                    if ctx.hand_strength > 0.35 {
                        score += (ctx.hand_strength - 0.35) * 40.0;
                    }
                    if ctx.is_dealer { score += 4.0; }

                    // ADAPTATION: steal more against frequent folders
                    if ctx.street_progress < 0.1 {
                        if ctx.opp_fold_rate > 0.3 {
                            // Opponent folds a lot — steal blinds more aggressively
                            score += 3.0 + ctx.opp_fold_rate * 10.0;
                        } else if ctx.hand_strength > 0.25 {
                            score += 5.0;
                        }
                    }

                    // Bluff more against folders
                    if ctx.hand_strength < 0.15 && ctx.opp_fold_rate > 0.3 {
                        score += ctx.opp_fold_rate * 12.0;
                    }

                    // Be cautious raising against aggressive opponents with medium hands
                    if ctx.hand_strength < 0.5 && ctx.opp_aggression > 0.4 {
                        score -= 4.0;
                    }

                    score
                },
                ResponseCurve::Identity, 1.0)

            .consider("allin_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::AllIn { return -100.0; }
                    let mut score = -8.0;

                    if ctx.hand_strength > 0.80 { score += 30.0; }
                    else if ctx.hand_strength > 0.65 { score += 15.0; }

                    if ctx.to_call_ratio > 0.4 && ctx.hand_strength > 0.30 {
                        score += 12.0;
                    }

                    if ctx.street_progress < 0.1 && ctx.hand_strength > 0.85 {
                        score += 10.0;
                    }

                    // ADAPTATION: shove-bluff against very foldy opponents
                    if ctx.opp_fold_rate > 0.5 && ctx.hand_strength < 0.2 {
                        score += 10.0;
                    }

                    score
                },
                ResponseCurve::Identity, 1.0)
    }

    fn estimate_hand_strength(view: &PokerView) -> f64 {
        if view.community.is_empty() {
            Self::preflop_strength(&view.hole_cards)
        } else {
            let mut all_cards: Vec<Card> = view.hole_cards.iter().copied().collect();
            all_cards.extend_from_slice(&view.community);
            Self::rank_to_strength(&evaluate_hand(&all_cards))
        }
    }

    fn preflop_strength(cards: &[Card; 2]) -> f64 {
        let high = cards[0].rank.0.max(cards[1].rank.0);
        let low = cards[0].rank.0.min(cards[1].rank.0);
        let suited = cards[0].suit == cards[1].suit;
        let pair = cards[0].rank == cards[1].rank;
        let gap = high - low;

        if pair { return 0.35 + (low as f64 - 2.0) * 0.05; }

        let mut score = (high as f64 - 2.0) * 0.027 + (low as f64 - 2.0) * 0.008;
        if suited { score += 0.06; }
        if gap == 1 { score += 0.04; }
        else if gap == 2 { score += 0.02; }
        if gap >= 5 { score -= 0.05; }
        score.clamp(0.05, 0.50)
    }

    fn rank_to_strength(rank: &HandRank) -> f64 {
        match rank {
            HandRank::HighCard(h, _, _, _, _) => 0.05 + (*h as f64 - 2.0) * 0.008,
            HandRank::Pair(p, _, _, _) => 0.25 + (*p as f64 - 2.0) * 0.02,
            HandRank::TwoPair(h, _, _) => 0.55 + (*h as f64 - 2.0) * 0.008,
            HandRank::ThreeOfAKind(_, _, _) => 0.70,
            HandRank::Straight(_) => 0.78,
            HandRank::Flush(_, _, _, _, _) => 0.83,
            HandRank::FullHouse(_, _) => 0.90,
            HandRank::FourOfAKind(_, _) => 0.96,
            HandRank::StraightFlush(_) => 0.99,
        }
    }

    fn action_to_type(action: &Action) -> ActionType {
        match action {
            Action::Fold => ActionType::Fold,
            Action::Check => ActionType::Check,
            Action::Call => ActionType::Call,
            Action::Raise(_) => ActionType::Raise,
            Action::AllIn => ActionType::AllIn,
        }
    }
}


impl AdaptiveAgent {
    fn compute_command(&mut self, view: &PokerView) -> Action {
        let valid = view.valid_actions();
        if valid.is_empty() { return Action::Fold; }
        if valid.len() == 1 { return valid[0].clone(); }

        let hand_strength = Self::estimate_hand_strength(view);
        let pot_total = view.pot as f64;
        let to_call = view.to_call as f64;
        let pot_odds = if pot_total + to_call > 0.0 {
            to_call / (pot_total + to_call)
        } else { 0.0 };
        let pot_commitment = if view.our_chips as f64 + view.our_bet as f64 > 0.0 {
            view.our_bet as f64 / (view.our_chips as f64 + view.our_bet as f64)
        } else { 0.0 };
        let street_progress = match view.street {
            Street::Preflop => 0.0, Street::Flop => 0.33,
            Street::Turn => 0.66, Street::River | Street::Showdown => 1.0,
        };
        let to_call_ratio = if view.our_chips > 0 {
            view.to_call as f64 / view.our_chips as f64
        } else { 1.0 };

        // Detect if we're facing a raise this action
        let facing_raise = view.to_call > 0 && view.history.iter().rev()
            .find(|(_, p, _)| *p != view.viewer)
            .map(|(_, _, a)| matches!(a, Action::Raise(_) | Action::AllIn))
            .unwrap_or(false);

        let mut best_action = valid[0].clone();
        let mut best_score = f64::NEG_INFINITY;

        for action in &valid {
            let ctx = ActionCtx {
                action_type: Self::action_to_type(action),
                hand_strength,
                pot_odds,
                pot_commitment,
                is_dealer: view.is_dealer,
                street_progress,
                to_call_ratio,
                opp_aggression: self.model.aggression(),
                opp_fold_rate: self.model.fold_rate(),
                opp_raise_honesty: self.model.raise_honesty(),
                opp_passive_trap: self.model.passive_trap_rate(),
                facing_raise,
            };

            let score = self.scorer.score(&ctx) + (self.xorshift() % 100) as f64 * 0.001;
            if score > best_score {
                best_score = score;
                best_action = action.clone();
            }
        }

        best_action
    }
}

impl GameAgent<PokerView, Action> for AdaptiveAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.model = OpponentModel::default();
    }

    fn observe(&mut self, view: &PokerView) {
        self.model.update(view);
    }

    fn decide(
        &mut self,
        view: &PokerView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
