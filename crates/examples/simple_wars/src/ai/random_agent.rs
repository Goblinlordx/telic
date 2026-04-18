use telic::arena::{CommandTree, GameAgent, PlayerIndex};
use crate::game::types::*;
use crate::game::state::SimpleWarsView;

/// True random agent — enumerates every plausibly-valid command and picks
/// one uniformly at random, including `EndTurn`. Serves as the proper floor
/// for evaluation: any smart agent should decisively beat this.
///
/// This is *not* the same as the reactive baseline, which hardcodes the
/// attack-if-in-range and capture-if-on-building reflexes.
#[derive(Debug)]
pub struct RandomAgent {
    name: String,
    player: PlayerIndex,
    rng_state: u64,
    actions_this_turn: u32,
}

impl RandomAgent {
    pub fn new(name: impl Into<String>, seed: u64) -> Self {
        Self {
            name: name.into(),
            player: 0,
            rng_state: seed.max(1),
            actions_this_turn: 0,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        self.rng_state
    }

    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        let i = (self.next_u64() as usize) % items.len();
        items[i].clone()
    }

    fn enumerate_commands(&self, view: &SimpleWarsView) -> Vec<Command> {
        let mut cmds: Vec<Command> = Vec::new();

        // EndTurn is always valid.
        cmds.push(Command::EndTurn);

        // Build commands: if HQ is unoccupied and we can afford the unit.
        let hq_free = !view.our_units.iter().any(|u| u.pos == view.our_hq);
        if hq_free {
            for ut in [UnitType::Infantry, UnitType::Tank, UnitType::Artillery, UnitType::Recon] {
                if view.our_gold >= ut.cost() {
                    cmds.push(Command::Build { unit_type: ut });
                }
            }
        }

        // Per-unit commands.
        for unit in &view.our_units {
            let uid = unit.id;

            // Capture — infantry on enemy building.
            if !unit.attacked && unit.unit_type.can_capture() {
                let on_capturable = view.buildings.iter()
                    .any(|b| b.pos == unit.pos && b.owner != Some(self.player));
                if on_capturable {
                    cmds.push(Command::Capture { unit_id: uid });
                }
            }

            // Attack — any visible enemy in range (attack-in-place).
            if !unit.attacked {
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

            // Move — any adjacent valid cell (terrain passable, not occupied by ally).
            if !unit.moved {
                let directions = [(-1i8, 0), (1, 0), (0, -1), (0, 1)];
                for (dr, dc) in directions {
                    let nr = unit.pos.row as i8 + dr;
                    let nc = unit.pos.col as i8 + dc;
                    if nr < 0 || nr >= view.rows as i8 || nc < 0 || nc >= view.cols as i8 {
                        continue;
                    }
                    let to = Pos::new(nr as u8, nc as u8);
                    if !unit.unit_type.can_enter(view.grid[to.row as usize][to.col as usize]) {
                        continue;
                    }
                    // Don't move into an ally.
                    if view.our_units.iter().any(|u| u.id != uid && u.pos == to) {
                        continue;
                    }
                    cmds.push(Command::Move { unit_id: uid, to });
                }
            }
        }

        cmds
    }
}


impl RandomAgent {
    fn compute_command(&mut self, view: &SimpleWarsView) -> Command {
        // Legacy path — kept for the old run() arena. Does its own
        // validity enumeration since there's no tree in this path.
        self.actions_this_turn += 1;
        if self.actions_this_turn > 40 {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }

        let cmds = self.enumerate_commands(view);
        if cmds.is_empty() {
            self.actions_this_turn = 0;
            return Command::EndTurn;
        }
        let picked = self.pick(&cmds);
        if matches!(picked, Command::EndTurn) {
            self.actions_this_turn = 0;
        }
        picked
    }
}

impl GameAgent<SimpleWarsView, Command> for RandomAgent {
    fn name(&self) -> &str { &self.name }

    fn reset(&mut self, player: PlayerIndex) {
        self.player = player;
        self.actions_this_turn = 0;
    }

    fn observe(&mut self, _view: &SimpleWarsView) {}

    fn decide(
        &mut self,
        _view: &SimpleWarsView,
        tree: &CommandTree<Command>,
    ) -> Option<Command> {
        // Tree path — the tree is already guaranteed to contain only
        // valid commands, so pure-random sampling is trivial.
        let leaves = tree.flatten();
        if leaves.is_empty() { return None; }
        let i = (self.next_u64() as usize) % leaves.len();
        let picked = leaves[i].clone();
        if matches!(picked, Command::EndTurn) {
            self.actions_this_turn = 0;
        } else {
            self.actions_this_turn += 1;
        }
        Some(picked)
    }
}
