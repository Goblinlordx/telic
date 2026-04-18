//! `CommandProvider` for Splendor — enumerates every valid action.
//!
//! Tree shape:
//! ```text
//! Layer("actions")
//! ├── "pass"         → Leaf(Pass)
//! ├── "take_three"   → Layer of 10 possible 3-distinct-gem combos, filtered to available
//! ├── "take_two"     → Layer of 5 gems, filtered to 4+ available
//! ├── "reserve"      → Layer of (tier, index) for each face-up card, if < 3 reserved
//! ├── "buy"          → Layer of (tier, index) for each affordable face-up card
//! └── "buy_reserved" → Layer of reserved indices we can afford
//! ```
//! Empty branches are omitted; non-current player gets `CommandTree::Empty`.

use std::sync::Arc;

use telic::arena::{CommandProvider, CommandTree, PlayerIndex};

use super::state::{SplendorGame, SplendorView};
use super::types::*;

pub struct SplendorCommands;

impl CommandProvider for SplendorCommands {
    type State = SplendorGame;

    fn command_tree(
        state: &SplendorGame,
        player: PlayerIndex,
    ) -> Arc<CommandTree<Action>> {
        use telic::arena::GameState;
        Arc::new(build_tree(&state.view_for(player)))
    }
}

const MAX_TOKENS: u8 = 10;
const MAX_RESERVED: usize = 3;

fn build_tree(view: &SplendorView) -> CommandTree<Action> {
    if !view.is_our_turn {
        return CommandTree::Empty;
    }

    let mut children: Vec<(String, Arc<CommandTree<Action>>)> = Vec::new();

    // Pass is always allowed by apply_command — keep it as the always-valid fallback.
    children.push(("pass".into(), Arc::new(CommandTree::Leaf(Action::Pass))));

    if let Some(s) = take_three_subtree(view) {
        children.push(("take_three".into(), Arc::new(s)));
    }
    if let Some(s) = take_two_subtree(view) {
        children.push(("take_two".into(), Arc::new(s)));
    }
    if let Some(s) = reserve_subtree(view) {
        children.push(("reserve".into(), Arc::new(s)));
    }
    if let Some(s) = buy_subtree(view) {
        children.push(("buy".into(), Arc::new(s)));
    }
    if let Some(s) = buy_reserved_subtree(view) {
        children.push(("buy_reserved".into(), Arc::new(s)));
    }

    CommandTree::Layer { label: "actions".into(), children }
}

fn take_three_subtree(view: &SplendorView) -> Option<CommandTree<Action>> {
    if view.our_tokens.total() + 3 > MAX_TOKENS { return None; }
    let mut children: Vec<(String, Arc<CommandTree<Action>>)> = Vec::new();
    // 10 distinct 3-gem combinations.
    for (ai, &a) in Gem::ALL.iter().enumerate() {
        for (bi, &b) in Gem::ALL.iter().enumerate().skip(ai + 1) {
            for &c in Gem::ALL.iter().skip(bi + 1) {
                if view.bank.get(a) == 0 { continue; }
                if view.bank.get(b) == 0 { continue; }
                if view.bank.get(c) == 0 { continue; }
                let key = format!("{}{}{}", a, b, c);
                children.push((
                    key,
                    Arc::new(CommandTree::Leaf(Action::TakeThree([a, b, c]))),
                ));
            }
        }
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "take_three".into(), children })
}

fn take_two_subtree(view: &SplendorView) -> Option<CommandTree<Action>> {
    if view.our_tokens.total() + 2 > MAX_TOKENS { return None; }
    let mut children: Vec<(String, Arc<CommandTree<Action>>)> = Vec::new();
    for &g in Gem::ALL.iter() {
        if view.bank.get(g) < 4 { continue; }
        children.push((
            format!("{}", g),
            Arc::new(CommandTree::Leaf(Action::TakeTwo(g))),
        ));
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "take_two".into(), children })
}

fn reserve_subtree(view: &SplendorView) -> Option<CommandTree<Action>> {
    if view.our_reserved.len() >= MAX_RESERVED { return None; }
    let mut children: Vec<(String, Arc<CommandTree<Action>>)> = Vec::new();
    for (ti, tier_cards) in view.market.iter().enumerate() {
        for (idx, _card) in tier_cards.iter().enumerate() {
            children.push((
                format!("t{}_{}", ti + 1, idx),
                Arc::new(CommandTree::Leaf(Action::Reserve {
                    tier: (ti + 1) as u8,
                    index: idx,
                })),
            ));
        }
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "reserve".into(), children })
}

fn buy_subtree(view: &SplendorView) -> Option<CommandTree<Action>> {
    let mut children: Vec<(String, Arc<CommandTree<Action>>)> = Vec::new();
    for (ti, tier_cards) in view.market.iter().enumerate() {
        for (idx, card) in tier_cards.iter().enumerate() {
            let (afford, _) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
            if !afford { continue; }
            children.push((
                format!("t{}_{}", ti + 1, idx),
                Arc::new(CommandTree::Leaf(Action::Buy {
                    tier: (ti + 1) as u8,
                    index: idx,
                })),
            ));
        }
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "buy".into(), children })
}

fn buy_reserved_subtree(view: &SplendorView) -> Option<CommandTree<Action>> {
    let mut children: Vec<(String, Arc<CommandTree<Action>>)> = Vec::new();
    for (idx, card) in view.our_reserved.iter().enumerate() {
        let (afford, _) = view.our_tokens.can_afford(&card.cost, &view.our_bonuses);
        if !afford { continue; }
        children.push((
            format!("{}", idx),
            Arc::new(CommandTree::Leaf(Action::BuyReserved { index: idx })),
        ));
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "buy_reserved".into(), children })
}
