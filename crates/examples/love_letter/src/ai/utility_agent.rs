use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use telic::planning::utility::{UtilityAction, ResponseCurve, ScoringMode};

use crate::game::types::{Card, PlayCommand};
use crate::game::state::LoveLetterView;

/// Love Letter agent with behavioral memory.
///
/// Tracks opponent's play history via `observe()` and infers their likely
/// hand strength from their choices:
///
/// - **Countess played** → they hold King or Prince (forced rule)
/// - **Handmaid chosen** → likely protecting a high card (5+)
/// - **Baron played & survived** → their remaining card is high
/// - **King played** → they wanted to swap up (had low card, now has ours)
/// - **Prince used on us** → disruption play, less info
///
/// These inferences adjust the probability distribution used for scoring.
#[derive(Debug)]
pub struct UtilityAgent {
    name: String,
    player: PlayerIndex,
    seed: u64,
    scorer: UtilityAction<PlayCtx>,
    memory: Memory,
}

/// Persistent memory across turns — updated in observe().
#[derive(Debug, Clone, Default)]
struct Memory {
    /// Snapshot of opponent's discard pile from last observation.
    prev_opp_discards: Vec<Card>,
    /// Inferred: opponent likely holds a high card (value 5+).
    /// Range: 0.0 (no info) to 1.0 (certain high card).
    opp_high_card_confidence: f64,
    /// Inferred: opponent definitely holds King or Prince (Countess was forced).
    opp_has_king_or_prince: bool,
    /// If we know from Baron comparison what range their card is in.
    /// None = no info, Some(min_value) = their card is at least this.
    opp_card_min_value: Option<u8>,
    /// Turn of last inference (stale after King swap or Prince discard).
    inference_turn: u32,
}

impl Memory {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Detect what opponent played and update inferences.
    fn update(&mut self, view: &LoveLetterView) {
        let opp_idx = 1 - view.viewer;
        let opp_discards = &view.discard_piles[opp_idx];

        // Detect new card(s) played since last observation
        let new_cards: Vec<Card> = if opp_discards.len() > self.prev_opp_discards.len() {
            opp_discards[self.prev_opp_discards.len()..].to_vec()
        } else {
            Vec::new()
        };

        for &card in &new_cards {
            match card {
                Card::Countess => {
                    // Forced play: they MUST have King or Prince still in hand
                    self.opp_has_king_or_prince = true;
                    self.opp_high_card_confidence = 0.9;
                    self.opp_card_min_value = Some(5); // King=6 or Prince=5
                }
                Card::Handmaid => {
                    // Chose protection — suggests they have something worth protecting
                    self.opp_high_card_confidence =
                        (self.opp_high_card_confidence + 0.3).min(0.8);
                }
                Card::Baron => {
                    // They played Baron — if they survived, they were confident
                    // their remaining card is high (otherwise Baron is suicidal).
                    // We can't always tell if they survived from the view alone,
                    // but if they're not eliminated, they won or tied.
                    if !view.opponent_eliminated {
                        self.opp_high_card_confidence =
                            (self.opp_high_card_confidence + 0.4).min(0.9);
                        // If Baron was played confidently, their card is likely 4+
                        self.opp_card_min_value = Some(
                            self.opp_card_min_value.unwrap_or(0).max(4)
                        );
                    }
                }
                Card::King => {
                    // They swapped hands — their new hand is what we had.
                    // Our inference about their OLD hand is now stale.
                    self.opp_high_card_confidence = 0.0;
                    self.opp_has_king_or_prince = false;
                    self.opp_card_min_value = None;
                }
                Card::Prince => {
                    // If used on us, we drew a new card — their inference about us is stale.
                    // If used on themselves (not in this game), same idea.
                    // Their own hand didn't change, so our inference holds.
                }
                Card::Guard => {
                    // Guard is cheap to play — low info. Slight negative signal
                    // (they didn't have better options, or chose the safe play).
                    if self.opp_high_card_confidence > 0.0 {
                        self.opp_high_card_confidence *= 0.9; // slight decay
                    }
                }
                Card::Priest => {
                    // Information play — no strong hand signal
                }
                Card::Princess => {
                    // They played Princess = eliminated (forced by Prince, or mistake)
                }
            }
        }

        self.inference_turn = view.turn;
        self.prev_opp_discards = opp_discards.clone();
    }
}

/// Context for scoring a single card play.
struct PlayCtx {
    card: Card,
    other_card: Card,
    // Probabilities (adjusted by memory)
    guard_hit_prob: f64,
    best_guard_guess: Card,
    baron_net_advantage: f64,
    princess_prob: f64,
    avg_opponent_value: f64,
    // State
    opponent_protected: bool,
    known_opponent: bool,
    deck_remaining: usize,
    deck_low: bool,
    // Memory-derived
    opp_likely_high: f64,
    // Forced
    countess_forced: bool,
}

impl UtilityAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            seed: seed.max(1),
            scorer: Self::build_scorer(),
            memory: Memory::default(),
        }
    }

    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn build_scorer() -> UtilityAction<PlayCtx> {
        UtilityAction::new("love_letter_play")
            .with_base(0.0)
            .with_mode(ScoringMode::Additive)
            // --- Hard rules ---
            .consider("never_princess",
                |ctx: &PlayCtx| if ctx.card == Card::Princess { -1000.0 } else { 0.0 },
                ResponseCurve::Identity, 1.0)
            .consider("countess_forced",
                |ctx: &PlayCtx| if ctx.countess_forced { 500.0 } else { 0.0 },
                ResponseCurve::Identity, 1.0)
            // --- Elimination plays ---
            .consider("guard_value",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::Guard { return 0.0; }
                    if ctx.opponent_protected { return 2.0; }
                    ctx.guard_hit_prob * 40.0 + 2.0
                },
                ResponseCurve::Identity, 1.0)
            .consider("baron_value",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::Baron { return 0.0; }
                    if ctx.opponent_protected { return -3.0; }
                    // Memory boost: if we believe opponent has high card,
                    // Baron is riskier
                    let risk_adjustment = if ctx.opp_likely_high > 0.5 {
                        -5.0 * ctx.opp_likely_high // penalize Baron when opp is strong
                    } else {
                        0.0
                    };
                    ctx.baron_net_advantage * 35.0 + risk_adjustment
                },
                ResponseCurve::Identity, 1.0)
            .consider("prince_value",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::Prince { return 0.0; }
                    if ctx.opponent_protected { return -2.0; }
                    let mut score = ctx.princess_prob * 80.0 + 3.0;
                    // Memory: if opponent likely has high card, Prince is
                    // great disruption (forces discard of that high card)
                    if ctx.opp_likely_high > 0.3 {
                        score += ctx.opp_likely_high * 10.0;
                    }
                    if ctx.deck_low {
                        score += ctx.avg_opponent_value * 0.5;
                    }
                    score
                },
                ResponseCurve::Identity, 1.0)
            // --- Information value ---
            .consider("priest_value",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::Priest { return 0.0; }
                    if ctx.opponent_protected || ctx.known_opponent { return -1.0; }
                    8.0 + ctx.deck_remaining as f64 * 1.5
                },
                ResponseCurve::Identity, 1.0)
            // --- Protection value ---
            .consider("handmaid_value",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::Handmaid { return 0.0; }
                    let mut score = 4.0;
                    if ctx.other_card.value() >= 6 { score += 5.0; }
                    // Memory: if opponent likely has high card, protection
                    // is more valuable (they might Baron/Prince us)
                    if ctx.opp_likely_high > 0.3 {
                        score += ctx.opp_likely_high * 4.0;
                    }
                    if ctx.deck_low { score -= 2.0; }
                    score
                },
                ResponseCurve::Identity, 1.0)
            // --- Swap value ---
            .consider("king_value",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::King { return 0.0; }
                    if ctx.opponent_protected { return -3.0; }
                    let our_kept = ctx.other_card.value() as f64;
                    // Memory: if we believe opponent has high card, swapping
                    // is even better (we get their high card)
                    let expected_opp = if ctx.opp_likely_high > 0.3 {
                        // Bias expected value upward
                        ctx.avg_opponent_value * (1.0 + ctx.opp_likely_high * 0.3)
                    } else {
                        ctx.avg_opponent_value
                    };
                    if expected_opp > our_kept {
                        (expected_opp - our_kept) * 5.0
                    } else {
                        -(our_kept - expected_opp) * 5.0
                    }
                },
                ResponseCurve::Identity, 1.0)
            // --- Safe dump ---
            .consider("countess_dump",
                |ctx: &PlayCtx| {
                    if ctx.card != Card::Countess { return 0.0; }
                    1.0
                },
                ResponseCurve::Identity, 1.0)
            // --- Card retention ---
            .consider("endgame_retention",
                |ctx: &PlayCtx| {
                    let mut score = 0.0;
                    if ctx.deck_low {
                        score += ctx.other_card.value() as f64 * 3.0;
                    }
                    score -= ctx.card.value() as f64 * 0.3;
                    score
                },
                ResponseCurve::Identity, 1.0)
    }

    // =========================================================================
    // Probability with memory adjustment
    // =========================================================================

    fn opponent_distribution(&self, view: &LoveLetterView) -> Vec<(Card, f64)> {
        let mut counts = std::collections::HashMap::new();
        for &card in Card::all_types() {
            counts.insert(card, card.count_in_deck() as i32);
        }
        for &c in &view.hand {
            *counts.entry(c).or_insert(0) -= 1;
        }
        for pile in &view.discard_piles {
            for &c in pile {
                *counts.entry(c).or_insert(0) -= 1;
            }
        }

        let total: i32 = counts.values().filter(|&&v| v > 0).sum();
        if total <= 0 { return Vec::new(); }

        // If we know from Priest peek
        if let Some(known) = view.known_opponent_card {
            return vec![(known, 1.0)];
        }

        let mut dist: Vec<(Card, f64)> = counts.into_iter()
            .filter(|&(_, count)| count > 0)
            .map(|(card, count)| (card, count as f64 / total as f64))
            .collect();

        // Apply memory-based adjustments
        if self.memory.opp_has_king_or_prince {
            // Countess was forced → they definitely have King or Prince
            // Zero out everything else, redistribute to King/Prince
            let king_prince_total: f64 = dist.iter()
                .filter(|(c, _)| *c == Card::King || *c == Card::Prince)
                .map(|(_, p)| p)
                .sum();

            if king_prince_total > 0.0 {
                for (card, prob) in &mut dist {
                    if *card == Card::King || *card == Card::Prince {
                        *prob /= king_prince_total; // normalize to 1.0
                    } else {
                        *prob = 0.0;
                    }
                }
            }
        } else if let Some(min_val) = self.memory.opp_card_min_value {
            // Baron inference: opponent's card is at least min_val
            // Reduce probability of cards below min_val
            let factor = 0.2; // don't zero out — inference could be wrong
            let mut reduction = 0.0;
            for (card, prob) in &mut dist {
                if card.value() < min_val {
                    reduction += *prob * (1.0 - factor);
                    *prob *= factor;
                }
            }
            // Redistribute to cards >= min_val
            let high_total: f64 = dist.iter()
                .filter(|(c, _)| c.value() >= min_val)
                .map(|(_, p)| p)
                .sum();
            if high_total > 0.0 {
                for (card, prob) in &mut dist {
                    if card.value() >= min_val {
                        *prob += reduction * (*prob / high_total);
                    }
                }
            }
        }

        dist
    }

    fn best_guard_guess(&self, dist: &[(Card, f64)]) -> Card {
        dist.iter()
            .filter(|(c, _)| *c != Card::Guard)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(c, _)| *c)
            .unwrap_or(Card::Princess)
    }

    fn build_play_ctx(&self, card: Card, other_card: Card, view: &LoveLetterView) -> PlayCtx {
        let dist = self.opponent_distribution(view);
        let deck_low = view.deck_remaining <= 2;

        let guard_hit_prob = dist.iter()
            .filter(|(c, _)| *c != Card::Guard)
            .map(|(_, p)| p)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .copied()
            .unwrap_or(0.0);

        let best_guess = self.best_guard_guess(&dist);

        let baron_net = {
            let our_value = other_card.value();
            let mut win = 0.0;
            let mut lose = 0.0;
            for &(c, p) in &dist {
                if our_value > c.value() { win += p; }
                else if c.value() > our_value { lose += p; }
            }
            win - lose
        };

        let princess_prob = dist.iter()
            .find(|(c, _)| *c == Card::Princess)
            .map(|(_, p)| *p)
            .unwrap_or(0.0);

        let avg_opponent_value: f64 = dist.iter()
            .map(|(c, p)| c.value() as f64 * p)
            .sum();

        let countess_forced = card == Card::Countess
            && (other_card == Card::King || other_card == Card::Prince);

        PlayCtx {
            card,
            other_card,
            guard_hit_prob,
            best_guard_guess: best_guess,
            baron_net_advantage: baron_net,
            princess_prob,
            avg_opponent_value,
            opponent_protected: view.opponent_protected,
            known_opponent: view.known_opponent_card.is_some(),
            deck_remaining: view.deck_remaining,
            deck_low,
            opp_likely_high: self.memory.opp_high_card_confidence,
            countess_forced,
        }
    }
}


impl UtilityAgent {
    fn compute_command(&mut self, view: &LoveLetterView) -> PlayCommand {
        if view.hand.len() < 2 {
            let card = view.hand.first().copied().unwrap_or(Card::Guard);
            let guess = if card == Card::Guard {
                let dist = self.opponent_distribution(view);
                Some(self.best_guard_guess(&dist))
            } else { None };
            return PlayCommand { card, guard_guess: guess };
        }

        let card_a = view.hand[0];
        let card_b = view.hand[1];

        // Countess rule
        let has_countess = card_a == Card::Countess || card_b == Card::Countess;
        let has_kp = view.hand.contains(&Card::King) || view.hand.contains(&Card::Prince);
        if has_countess && has_kp {
            return PlayCommand { card: Card::Countess, guard_guess: None };
        }

        let ctx_a = self.build_play_ctx(card_a, card_b, view);
        let ctx_b = self.build_play_ctx(card_b, card_a, view);

        let score_a = self.scorer.score(&ctx_a) + (self.xorshift() % 100) as f64 * 0.001;
        let score_b = self.scorer.score(&ctx_b) + (self.xorshift() % 100) as f64 * 0.001;

        let chosen = if score_a > score_b { card_a } else { card_b };

        let guard_guess = if chosen == Card::Guard {
            let dist = self.opponent_distribution(view);
            Some(self.best_guard_guess(&dist))
        } else { None };

        PlayCommand { card: chosen, guard_guess }
    }
}

impl GameAgent<LoveLetterView, PlayCommand> for UtilityAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.memory.reset();
    }

    fn observe(&mut self, view: &LoveLetterView) {
        self.memory.update(view);
    }

    fn decide(
        &mut self,
        view: &LoveLetterView,
        tree: &CommandTree<PlayCommand>,
    ) -> Option<PlayCommand> {
        crate::ai::tree_helpers::propose_and_validate(tree, || self.compute_command(view))
    }
}
