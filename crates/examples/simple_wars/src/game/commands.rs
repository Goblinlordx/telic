//! `CommandProvider` for SimpleWars — enumerates every valid command as a
//! tree, grouped by command type.
//!
//! Tree shape:
//! ```text
//! Layer("actions")
//! ├── "end_turn"  → Leaf(EndTurn)
//! ├── "build"     → Layer
//! │     ├── "infantry" → Leaf(Build Infantry)
//! │     └── ...
//! ├── "capture"   → Layer
//! │     ├── "unit_3" → Leaf(Capture {unit_id: 3})
//! │     └── ...
//! ├── "attack"    → Layer
//! │     ├── "1_to_(3,5)" → Leaf(Attack {...})
//! │     └── ...
//! └── "move"      → Layer
//!       ├── "1_to_(2,5)" → Leaf(Move {...})
//!       └── ...
//! ```
//!
//! Empty branches (e.g. no unit can attack this tick) are omitted, so the
//! shape is minimal for the current state. On the non-current player's
//! turn the tree is `Empty`.

use std::sync::Arc;

use telic::arena::{CommandProvider, CommandTree, PlayerIndex};

use super::state::{SimpleWarsGame, SimpleWarsView};
use super::types::*;

pub struct SimpleWarsCommands;

impl CommandProvider for SimpleWarsCommands {
    type State = SimpleWarsGame;

    fn command_tree(
        state: &SimpleWarsGame,
        player: PlayerIndex,
    ) -> Arc<CommandTree<Command>> {
        // Delegate to view-based construction so agents and tests can also
        // call it directly without a full game handle.
        Arc::new(build_tree(&state.view_for_player(player), player))
    }
}

/// Build the command tree from a view. Exposed so agents/tests can reuse
/// the same enumeration without going through a `&SimpleWarsGame`.
pub fn build_tree(view: &SimpleWarsView, player: PlayerIndex) -> CommandTree<Command> {
    if !view.is_our_turn {
        return CommandTree::Empty;
    }

    let mut root_children: Vec<(String, Arc<CommandTree<Command>>)> = Vec::new();

    // End turn is always valid on our turn.
    root_children.push((
        "end_turn".into(),
        Arc::new(CommandTree::Leaf(Command::EndTurn)),
    ));

    // Build commands — require free HQ and affordable cost.
    if let Some(build_layer) = build_subtree(view) {
        root_children.push(("build".into(), Arc::new(build_layer)));
    }

    // Capture commands — infantry on an enemy building.
    if let Some(capture_layer) = capture_subtree(view, player) {
        root_children.push(("capture".into(), Arc::new(capture_layer)));
    }

    // Attack commands — one per (unit, visible enemy in range).
    if let Some(attack_layer) = attack_subtree(view) {
        root_children.push(("attack".into(), Arc::new(attack_layer)));
    }

    // Move commands — one per (unit, adjacent passable unoccupied cell).
    if let Some(move_layer) = move_subtree(view) {
        root_children.push(("move".into(), Arc::new(move_layer)));
    }

    CommandTree::Layer {
        label: "actions".into(),
        children: root_children,
    }
}

fn build_subtree(view: &SimpleWarsView) -> Option<CommandTree<Command>> {
    let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
    if !hq_free { return None; }

    let mut children: Vec<(String, Arc<CommandTree<Command>>)> = Vec::new();
    for ut in [UnitType::Infantry, UnitType::Tank, UnitType::Artillery, UnitType::Recon] {
        if view.our_gold < ut.cost() { continue; }
        children.push((
            unit_type_key(ut).into(),
            Arc::new(CommandTree::Leaf(Command::Build { unit_type: ut })),
        ));
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "build".into(), children })
}

fn capture_subtree(view: &SimpleWarsView, player: PlayerIndex) -> Option<CommandTree<Command>> {
    let mut children: Vec<(String, Arc<CommandTree<Command>>)> = Vec::new();
    for unit in &view.our_units {
        if unit.attacked || !unit.unit_type.can_capture() { continue; }
        let on_capturable = view.buildings.iter()
            .any(|b| b.pos == unit.pos && b.owner != Some(player));
        if !on_capturable { continue; }
        children.push((
            format!("unit_{}", unit.id),
            Arc::new(CommandTree::Leaf(Command::Capture { unit_id: unit.id })),
        ));
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "capture".into(), children })
}

fn attack_subtree(view: &SimpleWarsView) -> Option<CommandTree<Command>> {
    let mut children: Vec<(String, Arc<CommandTree<Command>>)> = Vec::new();
    for unit in &view.our_units {
        if unit.attacked { continue; }
        if unit.unit_type.is_ranged() && unit.moved { continue; }
        for enemy in &view.visible_enemy_units {
            let dist = unit.pos.manhattan_distance(enemy.pos);
            if dist < unit.unit_type.attack_min_range() { continue; }
            if dist > unit.unit_type.attack_max_range() { continue; }
            children.push((
                format!("{}_to_({},{})", unit.id, enemy.pos.row, enemy.pos.col),
                Arc::new(CommandTree::Leaf(Command::Attack {
                    unit_id: unit.id,
                    target_pos: enemy.pos,
                })),
            ));
        }
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "attack".into(), children })
}

fn move_subtree(view: &SimpleWarsView) -> Option<CommandTree<Command>> {
    let mut children: Vec<(String, Arc<CommandTree<Command>>)> = Vec::new();
    for unit in &view.our_units {
        if unit.moved { continue; }
        for (dr, dc) in [(-1i8, 0), (1, 0), (0, -1), (0, 1)] {
            let nr = unit.pos.row as i8 + dr;
            let nc = unit.pos.col as i8 + dc;
            if nr < 0 || nr >= view.rows as i8 || nc < 0 || nc >= view.cols as i8 {
                continue;
            }
            let to = Pos::new(nr as u8, nc as u8);
            if !unit.unit_type.can_enter(view.grid[to.row as usize][to.col as usize]) {
                continue;
            }
            if view.our_units.iter().any(|u| u.id != unit.id && u.pos == to) {
                continue;
            }
            children.push((
                format!("{}_to_({},{})", unit.id, to.row, to.col),
                Arc::new(CommandTree::Leaf(Command::Move { unit_id: unit.id, to })),
            ));
        }
    }
    if children.is_empty() { return None; }
    Some(CommandTree::Layer { label: "move".into(), children })
}

fn unit_type_key(ut: UnitType) -> &'static str {
    match ut {
        UnitType::Infantry => "infantry",
        UnitType::Tank => "tank",
        UnitType::Artillery => "artillery",
        UnitType::Recon => "recon",
    }
}

// Helper on the game so we can build the tree from a &SimpleWarsGame
// without exposing its internals.
impl SimpleWarsGame {
    pub(crate) fn view_for_player(&self, player: PlayerIndex) -> SimpleWarsView {
        use telic::arena::GameState;
        self.view_for(player)
    }
}
