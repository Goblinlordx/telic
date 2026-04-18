//! Shared helpers for enumerating valid commands from a view.
//!
//! Agents should use these instead of proposing commands speculatively —
//! the game rejects invalid commands, and a rejected command is a wasted
//! decision. The goal: agents only ever select from known-valid options.

use crate::game::state::SimpleWarsView;
use crate::game::types::*;
use telic::arena::PlayerIndex;

/// Return every valid `Command` the given player can issue from this view.
/// Includes `EndTurn` as always-valid (when it is the player's turn).
///
/// Notes / caveats:
/// - Uses the view's knowledge only — no peeking at hidden state.
/// - `Move` is enumerated per adjacent tile (4-neighbour, one step). Agents
///   that need multi-step pathing should enumerate waypoints themselves.
/// - `MoveAttack` is intentionally not enumerated here — it can be composed
///   from `Move` + `Attack` and adds combinatorial breadth. Add if needed.
pub fn enumerate_commands(view: &SimpleWarsView, player: PlayerIndex) -> Vec<Command> {
    let mut cmds: Vec<Command> = Vec::new();

    if !view.is_our_turn {
        // Only `EndTurn` is ever accepted on someone else's turn, and even
        // then the game will typically reject it. Return empty so callers
        // just skip to a no-op.
        return cmds;
    }

    cmds.push(Command::EndTurn);

    // Build commands — HQ must be unoccupied and gold must cover cost.
    let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
    if hq_free {
        for ut in [UnitType::Infantry, UnitType::Tank, UnitType::Artillery, UnitType::Recon] {
            if view.our_gold >= ut.cost() {
                cmds.push(Command::Build { unit_type: ut });
            }
        }
    }

    for unit in &view.our_units {
        let uid = unit.id;

        // Capture — infantry on an enemy building, not already attacked this turn.
        if !unit.attacked && unit.unit_type.can_capture() {
            let on_capturable = view.buildings.iter()
                .any(|b| b.pos == unit.pos && b.owner != Some(player));
            if on_capturable {
                cmds.push(Command::Capture { unit_id: uid });
            }
        }

        // Attack (from current position) — ranged units must not have moved.
        if !unit.attacked {
            let ranged = unit.unit_type.is_ranged();
            if !(ranged && unit.moved) {
                for enemy in &view.visible_enemy_units {
                    let dist = unit.pos.manhattan_distance(enemy.pos);
                    if dist >= unit.unit_type.attack_min_range()
                        && dist <= unit.unit_type.attack_max_range()
                    {
                        cmds.push(Command::Attack {
                            unit_id: uid,
                            target_pos: enemy.pos,
                        });
                    }
                }
            }
        }

        // Move — one step to an adjacent passable tile not occupied by an ally.
        if !unit.moved {
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
                if view.our_units.iter().any(|u| u.id != uid && u.pos == to) {
                    continue;
                }
                cmds.push(Command::Move { unit_id: uid, to });
            }
        }
    }

    cmds
}

/// Predicate: would this command be accepted right now?
/// Equivalent to `enumerate_commands(view, player).contains(&cmd)` but more
/// useful for validate-before-return in speculative agents.
pub fn is_valid(view: &SimpleWarsView, player: PlayerIndex, cmd: &Command) -> bool {
    if !view.is_our_turn {
        return false;
    }
    match cmd {
        Command::EndTurn => true,
        Command::Build { unit_type } => {
            let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
            hq_free && view.our_gold >= unit_type.cost()
        }
        Command::Capture { unit_id } => {
            let Some(u) = view.our_units.iter().find(|u| u.id == *unit_id) else { return false; };
            if u.attacked || !u.unit_type.can_capture() { return false; }
            view.buildings.iter().any(|b| b.pos == u.pos && b.owner != Some(player))
        }
        Command::Attack { unit_id, target_pos } => {
            let Some(u) = view.our_units.iter().find(|u| u.id == *unit_id) else { return false; };
            if u.attacked { return false; }
            if u.unit_type.is_ranged() && u.moved { return false; }
            let Some(enemy) = view.visible_enemy_units.iter().find(|e| e.pos == *target_pos) else { return false; };
            let dist = u.pos.manhattan_distance(enemy.pos);
            dist >= u.unit_type.attack_min_range() && dist <= u.unit_type.attack_max_range()
        }
        Command::Move { unit_id, to } => {
            let Some(u) = view.our_units.iter().find(|u| u.id == *unit_id) else { return false; };
            if u.moved { return false; }
            if to.row >= view.rows || to.col >= view.cols { return false; }
            let terrain = view.grid[to.row as usize][to.col as usize];
            if !u.unit_type.can_enter(terrain) { return false; }
            if view.our_units.iter().any(|other| other.id != *unit_id && other.pos == *to) {
                return false;
            }
            // Adjacent-only check matches our enumerate_commands; multi-step
            // paths would need a reachability query.
            u.pos.manhattan_distance(*to) == 1
        }
        Command::MoveAttack { .. } => {
            // Not validated here — use Move + Attack as separate commands.
            false
        }
    }
}
