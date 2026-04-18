use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SplendorView;

/// Path-planning agent — computes the shortest purchase path to 15 points.
///
/// Each turn:
/// 1. Look at all visible cards (market + reserved)
/// 2. Find the cheapest combination that reaches 15 points
/// 3. Take the action that's on that path (buy if affordable, else take gems toward it)
///
/// "Cheapest" = fewest total turns to execute the plan, accounting for
/// bonuses gained along the way reducing future costs.
#[derive(Debug)]
pub struct PathAgent {
    name: String,
    player: PlayerIndex,
    #[allow(dead_code)]
    seed: u64,
}

/// A planned purchase in the path.
#[derive(Debug, Clone)]
struct PlannedBuy {
    card: Card,
    tier: u8,
    index: usize,
    is_reserved: bool,
    /// Effective cost after bonuses accumulated from earlier buys in the path.
    #[allow(dead_code)]
    effective_cost: u8,
}

/// A complete path to victory.
#[derive(Debug, Clone)]
struct WinPath {
    buys: Vec<PlannedBuy>,
    #[allow(dead_code)]
    total_points: u8,
    /// Estimated turns to execute this entire path.
    estimated_turns: f64,
}

impl PathAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self { name: name.into(), player: 0, seed: seed.max(1) }
    }

    #[allow(dead_code)]
    fn xorshift(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// Compute effective gem cost of a card given current + accumulated bonuses.
    fn effective_cost(card: &Card, bonuses: &[u8; 5]) -> u8 {
        Gem::ALL.iter()
            .map(|&g| card.cost.get(g).saturating_sub(bonuses[g.index()]))
            .sum()
    }

    /// How many turns of gem-taking to afford this effective cost,
    /// given current tokens and gold.
    fn turns_to_afford(_effective_cost: u8, tokens: &GemSet, card: &Card, bonuses: &[u8; 5]) -> f64 {
        let mut still_need = 0u8;
        let available_gold = tokens.gold;

        for g in Gem::ALL {
            let need = card.cost.get(g).saturating_sub(bonuses[g.index()]);
            let have = tokens.get(g);
            if need > have {
                still_need += need - have;
            }
        }

        // Gold covers some
        still_need = still_need.saturating_sub(available_gold);

        if still_need == 0 {
            0.0 // can buy now
        } else {
            // Each turn we take ~3 gems, so roughly still_need / 3 turns
            (still_need as f64 / 3.0).ceil()
        }
    }

    /// Find the best path to 15 points from current state.
    fn find_best_path(&self, view: &SplendorView) -> Option<WinPath> {
        // Gather all available cards
        let mut candidates: Vec<(Card, u8, usize, bool)> = Vec::new(); // (card, tier, index, is_reserved)

        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                candidates.push((card.clone(), (tier + 1) as u8, idx, false));
            }
        }
        for (idx, card) in view.our_reserved.iter().enumerate() {
            candidates.push((card.clone(), card.tier, idx, true));
        }

        // Sort by points-per-effective-cost (best value first)
        candidates.sort_by(|a, b| {
            let eff_a = Self::effective_cost(&a.0, &view.our_bonuses);
            let eff_b = Self::effective_cost(&b.0, &view.our_bonuses);
            let val_a = if eff_a == 0 { 100.0 } else { a.0.points as f64 / eff_a as f64 };
            let val_b = if eff_b == 0 { 100.0 } else { b.0.points as f64 / eff_b as f64 };
            val_b.partial_cmp(&val_a).unwrap()
        });

        let points_needed = 15u8.saturating_sub(view.our_points);
        if points_needed == 0 {
            return Some(WinPath { buys: Vec::new(), total_points: view.our_points, estimated_turns: 0.0 });
        }

        // Greedy path building: accumulate purchases that get us to 15 points
        // Try multiple starting points and pick the fastest path
        let mut best_path: Option<WinPath> = None;

        // Strategy 1: greedily pick highest value cards
        if let Some(path) = self.build_greedy_path(&candidates, view, points_needed) {
            if best_path.as_ref().map_or(true, |b| path.estimated_turns < b.estimated_turns) {
                best_path = Some(path);
            }
        }

        // Strategy 2: for each high-point card, build a path through it
        for i in 0..candidates.len().min(6) {
            if candidates[i].0.points >= 3 {
                if let Some(path) = self.build_path_through(&candidates, view, points_needed, i) {
                    if best_path.as_ref().map_or(true, |b| path.estimated_turns < b.estimated_turns) {
                        best_path = Some(path);
                    }
                }
            }
        }

        best_path
    }

    /// Build a greedy path: always pick the best available card next.
    fn build_greedy_path(
        &self,
        candidates: &[(Card, u8, usize, bool)],
        view: &SplendorView,
        points_needed: u8,
    ) -> Option<WinPath> {
        let mut buys = Vec::new();
        let mut accumulated_bonuses = view.our_bonuses;
        let mut accumulated_points = 0u8;
        let mut tokens = view.our_tokens;
        let mut used = vec![false; candidates.len()];
        let mut total_turns = 0.0;

        while accumulated_points < points_needed && buys.len() < 8 {
            // Find best next card to buy
            let mut best_idx = None;
            let mut best_value = f64::NEG_INFINITY;

            for (i, (card, _, _, _)) in candidates.iter().enumerate() {
                if used[i] { continue; }

                let eff = Self::effective_cost(card, &accumulated_bonuses);
                let turns = Self::turns_to_afford(eff, &tokens, card, &accumulated_bonuses);

                // Value = points gained per turn invested
                // Free cards (0 effective cost) that give a bonus are very valuable
                let value = if turns == 0.0 {
                    card.points as f64 * 20.0 + 10.0 // huge bonus for free buys
                } else {
                    card.points as f64 / turns - turns * 0.5 // penalize slow cards
                };

                // Bonus for cards whose bonus helps future purchases in our plan
                let bonus_value = if card.points == 0 && eff <= 3 {
                    5.0 // cheap engine card
                } else {
                    0.0
                };

                let total_value = value + bonus_value;

                if total_value > best_value {
                    best_value = total_value;
                    best_idx = Some(i);
                }
            }

            let Some(idx) = best_idx else { break };
            let (card, tier, card_idx, is_reserved) = &candidates[idx];
            used[idx] = true;

            let eff = Self::effective_cost(card, &accumulated_bonuses);
            let turns = Self::turns_to_afford(eff, &tokens, card, &accumulated_bonuses);

            buys.push(PlannedBuy {
                card: card.clone(),
                tier: *tier,
                index: *card_idx,
                is_reserved: *is_reserved,
                effective_cost: eff,
            });

            total_turns += turns + 1.0; // +1 for the buy action itself
            accumulated_bonuses[card.bonus.index()] += 1;
            accumulated_points += card.points;

            // Simulate spending tokens (rough)
            for g in Gem::ALL {
                let need = card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]);
                tokens.sub(g, need.min(tokens.get(g)));
            }
        }

        // Add noble points if we qualify
        for noble in &view.nobles {
            let qualifies = Gem::ALL.iter()
                .all(|&g| accumulated_bonuses[g.index()] >= noble.required[g.index()]);
            if qualifies {
                accumulated_points += noble.points;
                break;
            }
        }

        if accumulated_points >= points_needed {
            Some(WinPath {
                buys,
                total_points: view.our_points + accumulated_points,
                estimated_turns: total_turns,
            })
        } else {
            None
        }
    }

    /// Build a path that goes through a specific high-value card.
    fn build_path_through(
        &self,
        candidates: &[(Card, u8, usize, bool)],
        view: &SplendorView,
        points_needed: u8,
        target_idx: usize,
    ) -> Option<WinPath> {
        let target = &candidates[target_idx].0;
        let mut buys = Vec::new();
        let mut bonuses = view.our_bonuses;
        let mut points = 0u8;
        let tokens = view.our_tokens;
        let mut total_turns = 0.0;
        let mut used = vec![false; candidates.len()];

        // First: find cheap cards that give bonuses needed for the target
        for (i, (card, tier, idx, is_reserved)) in candidates.iter().enumerate() {
            if i == target_idx || used[i] { continue; }
            if card.points > 1 { continue; } // only consider engine cards

            let eff = Self::effective_cost(card, &bonuses);
            if eff > 4 { continue; } // too expensive for an engine card

            // Does this bonus help afford the target?
            let target_need = target.cost.get(card.bonus).saturating_sub(bonuses[card.bonus.index()]);
            if target_need > 0 {
                let turns = Self::turns_to_afford(eff, &tokens, card, &bonuses);
                buys.push(PlannedBuy {
                    card: card.clone(), tier: *tier, index: *idx,
                    is_reserved: *is_reserved, effective_cost: eff,
                });
                total_turns += turns + 1.0;
                bonuses[card.bonus.index()] += 1;
                points += card.points;
                used[i] = true;

                if buys.len() >= 4 { break; } // don't over-invest in engine
            }
        }

        // Then buy the target
        let eff = Self::effective_cost(target, &bonuses);
        let turns = Self::turns_to_afford(eff, &tokens, target, &bonuses);
        buys.push(PlannedBuy {
            card: target.clone(),
            tier: candidates[target_idx].1,
            index: candidates[target_idx].2,
            is_reserved: candidates[target_idx].3,
            effective_cost: eff,
        });
        total_turns += turns + 1.0;
        bonuses[target.bonus.index()] += 1;
        points += target.points;
        used[target_idx] = true;

        // Fill remaining points with whatever's cheapest
        while points < points_needed && buys.len() < 8 {
            let mut best = None;
            let mut best_val = f64::NEG_INFINITY;

            for (i, (card, _, _, _)) in candidates.iter().enumerate() {
                if used[i] || card.points == 0 { continue; }
                let eff = Self::effective_cost(card, &bonuses);
                let turns = Self::turns_to_afford(eff, &tokens, card, &bonuses);
                let val = card.points as f64 / (turns + 1.0);
                if val > best_val {
                    best_val = val;
                    best = Some(i);
                }
            }

            let Some(i) = best else { break };
            let (card, tier, idx, is_reserved) = &candidates[i];
            let eff = Self::effective_cost(card, &bonuses);
            let turns = Self::turns_to_afford(eff, &tokens, card, &bonuses);
            buys.push(PlannedBuy {
                card: card.clone(), tier: *tier, index: *idx,
                is_reserved: *is_reserved, effective_cost: eff,
            });
            total_turns += turns + 1.0;
            bonuses[card.bonus.index()] += 1;
            points += card.points;
            used[i] = true;
        }

        if points >= points_needed {
            Some(WinPath { buys, total_points: view.our_points + points, estimated_turns: total_turns })
        } else {
            None
        }
    }

    /// Given a planned path, what should we do THIS turn?
    fn next_action_for_path(&mut self, path: &WinPath, view: &SplendorView) -> Action {
        if path.buys.is_empty() {
            return Action::Pass;
        }

        let next = &path.buys[0];

        // Can we buy it now?
        let (can_afford, _) = view.our_tokens.can_afford(&next.card.cost, &view.our_bonuses);
        if can_afford {
            if next.is_reserved {
                return Action::BuyReserved { index: next.index };
            } else {
                return Action::Buy { tier: next.tier, index: next.index };
            }
        }

        // Can't afford yet — take gems toward it
        let mut needed_gems: Vec<(Gem, u8)> = Vec::new();
        for g in Gem::ALL {
            let need = next.card.cost.get(g).saturating_sub(view.our_bonuses[g.index()]);
            let have = view.our_tokens.get(g);
            if need > have {
                needed_gems.push((g, need - have));
            }
        }
        needed_gems.sort_by(|a, b| b.1.cmp(&a.1)); // most needed first

        // Try take 3 different
        if view.our_tokens.total() + 3 <= 10 {
            let mut to_take = Vec::new();
            let mut used = [false; 5];

            // Priority: gems we need
            for &(g, _) in &needed_gems {
                if to_take.len() >= 3 { break; }
                if !used[g.index()] && view.bank.get(g) > 0 {
                    to_take.push(g);
                    used[g.index()] = true;
                }
            }
            // Fill with any available
            for g in Gem::ALL {
                if to_take.len() >= 3 { break; }
                if !used[g.index()] && view.bank.get(g) > 0 {
                    to_take.push(g);
                    used[g.index()] = true;
                }
            }

            if to_take.len() == 3 {
                return Action::TakeThree([to_take[0], to_take[1], to_take[2]]);
            }
        }

        // Try take 2 of most needed
        for &(g, _) in &needed_gems {
            if view.bank.get(g) >= 4 && view.our_tokens.total() + 2 <= 10 {
                return Action::TakeTwo(g);
            }
        }

        // Fallback: take any 3
        let available: Vec<Gem> = Gem::ALL.iter()
            .filter(|&&g| view.bank.get(g) > 0)
            .copied().collect();
        if available.len() >= 3 && view.our_tokens.total() + 3 <= 10 {
            return Action::TakeThree([available[0], available[1], available[2]]);
        }

        Action::Pass
    }

    /// Find any card we can buy for free or very cheap (0-1 effective gems).
    /// These are always worth buying — free bonuses accelerate everything.
    fn find_opportunistic_buy(&self, view: &SplendorView) -> Option<Action> {
        let mut best: Option<(f64, Action)> = None;

        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                let (can_afford, gold) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                if !can_afford { continue; }

                let eff = Self::effective_cost(card, &view.our_bonuses);
                // Score: heavily prefer free cards, also like cheap cards with points
                let score = if eff == 0 {
                    50.0 + card.points as f64 * 10.0
                } else if eff <= 2 && gold == 0 {
                    20.0 + card.points as f64 * 10.0
                } else if card.points >= 3 {
                    card.points as f64 * 10.0  // always buy high-point cards we can afford
                } else {
                    continue; // not opportunistic enough
                };

                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((score, Action::Buy { tier: (tier + 1) as u8, index: idx }));
                }
            }
        }

        // Also check reserved cards
        for (idx, card) in view.our_reserved.iter().enumerate() {
            let (can_afford, _) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
            if !can_afford { continue; }

            let eff = Self::effective_cost(card, &view.our_bonuses);
            let score = if eff == 0 { 50.0 } else { 0.0 } + card.points as f64 * 10.0;
            if best.as_ref().map_or(true, |(s, _)| score > *s) {
                best = Some((score, Action::BuyReserved { index: idx }));
            }
        }

        best.map(|(_, action)| action)
    }

    /// Find the best affordable card (any card we can buy, scored by points/value).
    fn find_best_affordable(&self, view: &SplendorView) -> Option<Action> {
        let mut best: Option<(f64, Action)> = None;

        for tier in 0..3 {
            for (idx, card) in view.market[tier].iter().enumerate() {
                let (can_afford, gold) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
                if !can_afford { continue; }
                let score = card.points as f64 * 10.0 + 5.0 - gold as f64;
                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((score, Action::Buy { tier: (tier + 1) as u8, index: idx }));
                }
            }
        }
        for (idx, card) in view.our_reserved.iter().enumerate() {
            let (can_afford, gold) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
            if !can_afford { continue; }
            let score = card.points as f64 * 10.0 + 5.0 - gold as f64;
            if best.as_ref().map_or(true, |(s, _)| score > *s) {
                best = Some((score, Action::BuyReserved { index: idx }));
            }
        }

        best.map(|(_, action)| action)
    }
}


impl PathAgent {
    fn compute_command(&mut self, view: &SplendorView) -> Action {
        // 1. Always grab free/cheap opportunistic buys
        if let Some(action) = self.find_opportunistic_buy(view) {
            return action;
        }

        // 2. Follow the planned path
        if let Some(path) = self.find_best_path(view) {
            let action = self.next_action_for_path(&path, view);
            if !matches!(action, Action::Pass) {
                return action;
            }
        }

        // 3. If we're stuck (token-capped, no gems to take), buy anything affordable
        if let Some(action) = self.find_best_affordable(view) {
            return action;
        }

        // 4. Take any gems if possible
        let available: Vec<Gem> = Gem::ALL.iter()
            .filter(|&&g| view.bank.get(g) > 0)
            .copied().collect();
        if available.len() >= 3 && view.our_tokens.total() + 3 <= 10 {
            return Action::TakeThree([available[0], available[1], available[2]]);
        }
        if available.len() >= 1 {
            for &g in &available {
                if view.bank.get(g) >= 4 && view.our_tokens.total() + 2 <= 10 {
                    return Action::TakeTwo(g);
                }
            }
        }

        Action::Pass
    }
}

impl GameAgent<SplendorView, Action> for PathAgent {
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
