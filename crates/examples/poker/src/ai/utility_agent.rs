use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::utility::{UtilityAction, ResponseCurve, ScoringMode};

use crate::game::types::*;
use crate::game::state::PokerView;

/// Utility-scored poker agent.
///
/// Each valid action is scored by considerations:
/// - Hand strength (current cards relative to possible hands)
/// - Pot odds (is the call profitable?)
/// - Position advantage (dealer acts last post-flop)
/// - Aggression (betting strong, checking weak)
/// - Opponent behavior tracking
#[derive(Debug)]
pub struct UtilityAgent {
    name: String,
    player: Player,
    seed: u64,
    scorer: UtilityAction<ActionCtx>,
    /// Track opponent aggression: raises / total actions
    opp_total_actions: u32,
    opp_raises: u32,
    opp_folds: u32,
}

struct ActionCtx {
    action_type: ActionType,
    hand_strength: f64,     // 0.0 = trash, 1.0 = nuts
    pot_odds: f64,          // ratio of call cost to pot (0 = free, 1 = pot-sized)
    pot_commitment: f64,    // how much of our stack is in the pot already
    is_dealer: bool,
    street_progress: f64,   // 0 = preflop, 1 = river
    opp_aggression: f64,    // 0 = passive, 1 = very aggressive
    opp_fold_rate: f64,     // how often opponent folds
    to_call_ratio: f64,     // to_call / our_chips (0 = nothing, 1 = all-in to call)
}

#[derive(Debug, Clone, PartialEq)]
enum ActionType {
    Fold,
    Check,
    Call,
    Raise,
    AllIn,
}

impl UtilityAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            scorer: Self::build_scorer(),
            opp_total_actions: 0,
            opp_raises: 0,
            opp_folds: 0,
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn build_scorer() -> UtilityAction<ActionCtx> {
        UtilityAction::new("poker_action")
            .with_base(0.0)
            .with_mode(ScoringMode::Additive)

            // --- FOLD: only when hand is weak AND cost is significant ---
            .consider("fold_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Fold { return -100.0; }
                    if ctx.to_call_ratio < 0.01 { return -100.0; } // never fold for free

                    let weakness = 1.0 - ctx.hand_strength;
                    let cost_pressure = ctx.to_call_ratio;

                    // Fold when hand is weak and facing a big bet
                    // Threshold: fold if hand_strength < pot_odds (negative EV to call)
                    let negative_ev = (ctx.pot_odds - ctx.hand_strength).max(0.0);
                    negative_ev * 25.0 + weakness * cost_pressure * 10.0 - 8.0
                },
                ResponseCurve::Identity, 1.0)

            // --- CHECK: safe, free ---
            .consider("check_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Check { return -100.0; }
                    let mut score = 4.0; // baseline — checking is always OK
                    // Trap: check with strong hands early (let opponent bet)
                    if ctx.hand_strength > 0.6 && ctx.street_progress < 0.5 {
                        score += 3.0;
                    }
                    // But don't check strong hands on the river — bet for value
                    if ctx.hand_strength > 0.5 && ctx.street_progress > 0.8 {
                        score -= 5.0;
                    }
                    score
                },
                ResponseCurve::Identity, 1.0)

            // --- CALL: pot odds driven ---
            .consider("call_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Call { return -100.0; }
                    // +EV when hand strength exceeds pot odds
                    let ev = ctx.hand_strength - ctx.pot_odds;
                    let mut score = ev * 35.0;
                    // Pot committed bonus
                    score += ctx.pot_commitment * 4.0;
                    // Calling is safer than raising — slight bonus for medium hands
                    if ctx.hand_strength > 0.25 && ctx.hand_strength < 0.5 {
                        score += 2.0;
                    }
                    score
                },
                ResponseCurve::Identity, 1.0)

            // --- RAISE: aggression with strong hands + positional bluffs ---
            .consider("raise_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::Raise { return -100.0; }

                    let mut score = 0.0;

                    // Value raise: strong hand wants to build pot
                    // Lower threshold (0.35) — raise with top pair+
                    if ctx.hand_strength > 0.35 {
                        score += (ctx.hand_strength - 0.35) * 40.0;
                    }

                    // Position bonus: raising in position is powerful
                    if ctx.is_dealer { score += 4.0; }

                    // Steal blinds preflop with decent hands
                    if ctx.street_progress < 0.1 && ctx.hand_strength > 0.25 {
                        score += 5.0;
                    }

                    // Bluff: only if opponent folds enough to make it profitable
                    if ctx.hand_strength < 0.15 && ctx.opp_fold_rate > 0.35 {
                        score += ctx.opp_fold_rate * 8.0;
                    }

                    // Don't raise medium hands into aggression
                    if ctx.hand_strength < 0.5 && ctx.opp_aggression > 0.4 {
                        score -= 4.0;
                    }

                    score
                },
                ResponseCurve::Identity, 1.0)

            // --- ALL-IN: monsters, desperation, or calculated shoves ---
            .consider("allin_value",
                |ctx: &ActionCtx| {
                    if ctx.action_type != ActionType::AllIn { return -100.0; }

                    let mut score = -8.0; // high bar

                    // Monster hand: get it all in
                    if ctx.hand_strength > 0.80 { score += 30.0; }
                    else if ctx.hand_strength > 0.65 { score += 15.0; }

                    // Short-stacked shove: when call would commit most of stack anyway
                    if ctx.to_call_ratio > 0.4 && ctx.hand_strength > 0.30 {
                        score += 12.0;
                    }

                    // Preflop shove with premium (AA, KK, QQ)
                    if ctx.street_progress < 0.1 && ctx.hand_strength > 0.85 {
                        score += 10.0;
                    }

                    score
                },
                ResponseCurve::Identity, 1.0)
    }

    /// Estimate hand strength as a 0-1 value.
    /// Uses a simple heuristic: evaluate current hand rank relative to
    /// the best/worst possible, adjusted for street.
    fn estimate_hand_strength(view: &PokerView) -> f64 {
        if view.community.is_empty() {
            // Preflop: use card ranks
            Self::preflop_strength(&view.hole_cards)
        } else {
            // Postflop: evaluate actual hand
            let mut all_cards: Vec<Card> = view.hole_cards.iter().copied().collect();
            all_cards.extend_from_slice(&view.community);
            Self::rank_to_strength(&evaluate_hand(&all_cards))
        }
    }

    /// Preflop hand strength using a table-like ranking.
    /// Calibrated so pairs > non-pairs of similar rank,
    /// and suited/connected gets appropriate bonus.
    fn preflop_strength(cards: &[Card; 2]) -> f64 {
        let high = cards[0].rank.0.max(cards[1].rank.0);
        let low = cards[0].rank.0.min(cards[1].rank.0);
        let suited = cards[0].suit == cards[1].suit;
        let pair = cards[0].rank == cards[1].rank;
        let gap = high - low;

        if pair {
            // 22=0.45, 77=0.60, TT=0.72, KK=0.90, AA=0.95
            return 0.35 + (low as f64 - 2.0) * 0.05;
        }

        // Base: high card value (A=14 → 0.35, K=13 → 0.32, 7=0.14)
        let mut score = (high as f64 - 2.0) * 0.027;
        // Kicker bonus
        score += (low as f64 - 2.0) * 0.008;
        // Suited bonus (flush potential)
        if suited { score += 0.06; }
        // Connected bonus (straight potential)
        if gap == 1 { score += 0.04; }
        else if gap == 2 { score += 0.02; }
        // Big gap penalty
        if gap >= 5 { score -= 0.05; }

        // AKs ≈ 0.50, AKo ≈ 0.44, KQs ≈ 0.44, T9s ≈ 0.34, 72o ≈ 0.10
        score.clamp(0.05, 0.50)
    }

    /// Postflop hand strength from evaluated hand rank.
    /// Calibrated so top pair is solidly above the raise threshold.
    fn rank_to_strength(rank: &HandRank) -> f64 {
        match rank {
            HandRank::HighCard(h, _, _, _, _) => {
                // High card only: 0.05 (low) to 0.15 (ace high)
                0.05 + (*h as f64 - 2.0) * 0.008
            }
            HandRank::Pair(p, _, _, _) => {
                // Pair: 0.25 (low pair) to 0.50 (aces)
                0.25 + (*p as f64 - 2.0) * 0.02
            }
            HandRank::TwoPair(h, _, _) => {
                // Two pair: 0.55 to 0.65
                0.55 + (*h as f64 - 2.0) * 0.008
            }
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


impl UtilityAgent {
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
            Street::Preflop => 0.0,
            Street::Flop => 0.33,
            Street::Turn => 0.66,
            Street::River => 1.0,
            Street::Showdown => 1.0,
        };
        let opp_aggression = if self.opp_total_actions > 5 {
            self.opp_raises as f64 / self.opp_total_actions as f64
        } else { 0.3 }; // assume moderate before we have data
        let opp_fold_rate = if self.opp_total_actions > 5 {
            self.opp_folds as f64 / self.opp_total_actions as f64
        } else { 0.3 };
        let to_call_ratio = if view.our_chips > 0 {
            view.to_call as f64 / view.our_chips as f64
        } else { 1.0 };

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
                opp_aggression,
                opp_fold_rate,
                to_call_ratio,
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

impl GameAgent<PokerView, Action> for UtilityAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.opp_total_actions = 0;
        self.opp_raises = 0;
        self.opp_folds = 0;
    }

    fn observe(&mut self, view: &PokerView) {
        // Track opponent behavior from history
        let opp = 1 - self.player;
        let mut raises = 0u32;
        let mut folds = 0u32;
        let mut total = 0u32;
        for (_, player, action) in &view.history {
            if *player == opp {
                total += 1;
                match action {
                    Action::Raise(_) | Action::AllIn => raises += 1,
                    Action::Fold => folds += 1,
                    _ => {}
                }
            }
        }
        self.opp_total_actions = self.opp_total_actions.max(total);
        self.opp_raises = self.opp_raises.max(raises);
        self.opp_folds = self.opp_folds.max(folds);
    }

    fn decide(
        &mut self,
        view: &PokerView,
        tree: &CommandTree<Action>,
    ) -> Option<Action> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
