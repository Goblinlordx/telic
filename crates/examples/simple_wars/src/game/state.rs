use telic::arena::{GameState, GameOutcome, PlayerIndex, GameView};
use super::types::*;
use super::map::*;

/// What a player can see — fog of war applied.
#[derive(Debug, Clone)]
pub struct SimpleWarsView {
    pub viewer: Player,
    pub turn: u32,
    pub rows: u8,
    pub cols: u8,
    pub grid: Vec<Vec<Terrain>>,
    pub visibility: Vec<Vec<bool>>,
    pub our_units: Vec<Unit>,
    pub visible_enemy_units: Vec<Unit>,
    pub buildings: Vec<Building>,
    pub our_gold: u32,
    pub is_our_turn: bool,
    pub our_hq: Pos,
    pub enemy_hq: Pos,
    pub capture_threshold: u8,
}

impl GameView for SimpleWarsView {
    fn viewer(&self) -> PlayerIndex { self.viewer }
    fn turn(&self) -> u32 { self.turn }
}

/// Game rules configuration.
#[derive(Debug, Clone)]
pub struct GameRules {
    pub starting_gold: u32,
    pub income_per_building: u32,
    pub capture_threshold: u8,
    pub turn_limit: u32,
}

impl GameRules {
    pub fn standard(turn_limit: u32) -> Self {
        Self {
            starting_gold: STARTING_GOLD,
            income_per_building: INCOME_PER_BUILDING,
            capture_threshold: CAPTURE_THRESHOLD,
            turn_limit,
        }
    }

    pub fn hard(turn_limit: u32) -> Self {
        Self {
            starting_gold: HARD_STARTING_GOLD,
            income_per_building: HARD_INCOME_PER_BUILDING,
            capture_threshold: HARD_CAPTURE_THRESHOLD,
            turn_limit,
        }
    }
}

/// Full game state.
#[derive(Debug, Clone)]
pub struct SimpleWarsGame {
    map: MapConfig,
    units: Vec<Unit>,
    buildings: Vec<Building>,
    gold: [u32; 2],
    current_player: Player,
    turn: u32,
    winner: Option<Player>,
    next_unit_id: u16,
    rules: GameRules,
}

impl SimpleWarsGame {
    pub fn new() -> Self {
        Self::with_rules(generate_8x8(), GameRules::standard(100))
    }

    pub fn new_12x12() -> Self {
        Self::with_rules(generate_12x12(), GameRules::standard(150))
    }

    pub fn new_16x16() -> Self {
        Self::with_rules(generate_16x16(), GameRules::standard(200))
    }

    /// 16x16 with hard rules — multi-turn sieges, high-value cities.
    pub fn new_hard() -> Self {
        Self::with_rules(generate_16x16(), GameRules::hard(250))
    }

    /// Random 8x8 map with standard rules.
    pub fn random_8x8(seed: u64) -> Self {
        Self::with_rules(generate_random(8, 8, 4, seed), GameRules::standard(100))
    }

    /// Random 16x16 map with standard rules.
    pub fn random_16x16(seed: u64) -> Self {
        Self::with_rules(generate_random(16, 16, 12, seed), GameRules::standard(200))
    }

    /// Random 16x16 map with hard rules (5-turn sieges).
    pub fn random_16x16_hard(seed: u64) -> Self {
        Self::with_rules(generate_random(16, 16, 12, seed), GameRules::hard(250))
    }

    pub fn with_map(map_config: MapConfig, turn_limit: u32) -> Self {
        Self::with_rules(map_config, GameRules::standard(turn_limit))
    }

    pub fn with_rules(map_config: MapConfig, rules: GameRules) -> Self {
        let buildings = map_config.buildings.clone();
        let rows = map_config.rows;
        let cols = map_config.cols;

        let mut game = Self {
            map: map_config,
            units: Vec::new(),
            buildings,
            gold: [rules.starting_gold; 2],
            current_player: 0,
            turn: 0,
            winner: None,
            next_unit_id: 0,
            rules,
        };

        // Starting units near HQs
        game.spawn_unit(0, UnitType::Infantry, Pos::new(0, 1));
        game.spawn_unit(0, UnitType::Infantry, Pos::new(1, 0));
        game.spawn_unit(1, UnitType::Infantry, Pos::new(rows - 1, cols - 2));
        game.spawn_unit(1, UnitType::Infantry, Pos::new(rows - 2, cols - 1));

        game
    }

    fn spawn_unit(&mut self, owner: Player, unit_type: UnitType, pos: Pos) -> u16 {
        let id = self.next_unit_id;
        self.next_unit_id += 1;
        self.units.push(Unit::new(id, owner, unit_type, pos));
        id
    }

    fn get_unit(&self, id: u16) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id && u.hp > 0)
    }

    fn get_unit_mut(&mut self, id: u16) -> Option<&mut Unit> {
        self.units.iter_mut().find(|u| u.id == id && u.hp > 0)
    }

    fn hq_pos(&self, player: Player) -> Pos {
        self.buildings.iter()
            .find(|b| b.terrain == Terrain::HQ && b.owner == Some(player))
            .map(|b| b.pos)
            .unwrap_or(if player == 0 { Pos::new(0, 0) } else {
                Pos::new(self.map.rows - 1, self.map.cols - 1)
            })
    }

    fn collect_income(&mut self, player: Player) {
        let income: u32 = self.buildings.iter()
            .filter(|b| b.owner == Some(player)).count() as u32 * self.rules.income_per_building;
        self.gold[player] += income;
    }

    fn reset_units(&mut self, player: Player) {
        for unit in &mut self.units {
            if unit.owner == player && unit.hp > 0 {
                unit.moved = false;
                unit.attacked = false;
            }
        }
    }

    fn score(&self, player: Player) -> u32 {
        let buildings = self.buildings.iter()
            .filter(|b| b.owner == Some(player)).count() as u32;
        let unit_hp: u32 = self.units.iter()
            .filter(|u| u.owner == player && u.hp > 0)
            .map(|u| u.hp as u32).sum();
        buildings * 100 + unit_hp + self.gold[player] / 100
    }

    fn check_score_victory(&mut self, turn_limit: u32) {
        if self.winner.is_some() || self.turn < turn_limit { return; }
        let s0 = self.score(0);
        let s1 = self.score(1);
        if s0 > s1 { self.winner = Some(0); }
        else if s1 > s0 { self.winner = Some(1); }
        else { self.winner = Some(0); }
    }

    fn resolve_combat(&mut self, attacker_id: u16, target_pos: Pos) -> Result<(), String> {
        let attacker = self.get_unit(attacker_id).ok_or("Attacker not found")?;
        let a_type = attacker.unit_type;
        let a_hp = attacker.hp;
        let a_pos = attacker.pos;
        let a_owner = attacker.owner;

        let target = self.units.iter()
            .find(|u| u.pos == target_pos && u.owner != a_owner && u.hp > 0)
            .ok_or("No enemy at target")?;
        let t_id = target.id;
        let t_type = target.unit_type;

        let dist = a_pos.manhattan_distance(target_pos);
        if dist < a_type.attack_min_range() || dist > a_type.attack_max_range() {
            return Err("Target out of range".into());
        }

        let terrain_def = self.map.terrain_at(target_pos).defense_bonus();
        let attack = a_type.attack_power() as f32 * (a_hp as f32 / 10.0);
        let defense = terrain_def as f32 * (self.get_unit(t_id).unwrap().hp as f32 / 10.0);
        let damage = ((attack - defense) * 1.0).max(1.0).min(10.0) as u8;

        if let Some(target) = self.units.iter_mut().find(|u| u.id == t_id) {
            target.hp = target.hp.saturating_sub(damage);
        }

        let target_alive = self.get_unit(t_id).map_or(false, |u| u.hp > 0);
        if target_alive && dist == 1 && !t_type.is_ranged() {
            let t_hp_now = self.get_unit(t_id).unwrap().hp;
            let counter = t_type.attack_power() as f32 * (t_hp_now as f32 / 10.0);
            let a_def = self.map.terrain_at(a_pos).defense_bonus() as f32 * (a_hp as f32 / 10.0);
            let counter_dmg = ((counter - a_def) * 0.8).max(0.0).min(10.0) as u8;
            if let Some(a) = self.get_unit_mut(attacker_id) {
                a.hp = a.hp.saturating_sub(counter_dmg);
            }
        }

        if let Some(a) = self.get_unit_mut(attacker_id) { a.attacked = true; }
        self.units.retain(|u| u.hp > 0);
        Ok(())
    }

    fn process_capture(&mut self, unit_id: u16) -> Result<(), String> {
        let unit = self.get_unit(unit_id).ok_or("Unit not found")?;
        if !unit.unit_type.can_capture() { return Err("Only infantry can capture".into()); }
        let pos = unit.pos;
        let owner = unit.owner;
        let cap = unit.capture_power();

        let building = self.buildings.iter_mut()
            .find(|b| b.pos == pos && b.owner != Some(owner))
            .ok_or("No capturable building here")?;

        if building.capturing_player != Some(owner) {
            building.capture_progress = 0;
            building.capturing_player = Some(owner);
        }
        building.capture_progress += cap;

        if building.capture_progress >= self.rules.capture_threshold {
            building.owner = Some(owner);
            building.capture_progress = 0;
            building.capturing_player = None;
            if building.terrain == Terrain::HQ { self.winner = Some(owner); }
        }

        if let Some(u) = self.get_unit_mut(unit_id) { u.moved = true; u.attacked = true; }
        Ok(())
    }

    fn check_elimination(&mut self) {
        if self.winner.is_some() { return; }
        for p in 0..2 {
            let has_units = self.units.iter().any(|u| u.owner == p && u.hp > 0);
            if !has_units {
                let can_build = self.buildings.iter().any(|b| b.terrain == Terrain::HQ && b.owner == Some(p))
                    && self.gold[p] >= UnitType::Infantry.cost();
                if !can_build { self.winner = Some(1 - p); }
            }
        }
    }

    fn compute_visibility(&self, player: Player) -> Vec<Vec<bool>> {
        let mut vis = compute_visibility(&self.units, player, &self.map);
        for building in &self.buildings {
            if building.owner == Some(player) {
                for dr in -1i8..=1 {
                    for dc in -1i8..=1 {
                        let r = building.pos.row as i8 + dr;
                        let c = building.pos.col as i8 + dc;
                        if r >= 0 && r < self.map.rows as i8 && c >= 0 && c < self.map.cols as i8 {
                            vis[r as usize][c as usize] = true;
                        }
                    }
                }
            }
        }
        vis
    }
}

impl GameState for SimpleWarsGame {
    type Command = Command;
    type View = SimpleWarsView;

    fn view_for(&self, player: PlayerIndex) -> SimpleWarsView {
        let vis = self.compute_visibility(player);
        let enemy = 1 - player;

        SimpleWarsView {
            viewer: player,
            turn: self.turn,
            rows: self.map.rows,
            cols: self.map.cols,
            grid: self.map.grid.clone(),
            visibility: vis.clone(),
            our_units: self.units.iter().filter(|u| u.owner == player && u.hp > 0).cloned().collect(),
            visible_enemy_units: self.units.iter()
                .filter(|u| u.owner == enemy && u.hp > 0 && vis[u.pos.row as usize][u.pos.col as usize])
                .cloned().collect(),
            buildings: self.buildings.clone(),
            our_gold: self.gold[player],
            is_our_turn: self.current_player == player,
            our_hq: self.hq_pos(player),
            enemy_hq: self.hq_pos(enemy),
            capture_threshold: self.rules.capture_threshold,
        }
    }

    fn apply_command(&mut self, player: PlayerIndex, command: Command) -> Result<(), String> {
        if self.winner.is_some() { return Err("Game over".into()); }
        if player != self.current_player { return Err("Not your turn".into()); }

        match command {
            Command::Move { unit_id, to } => {
                let unit = self.get_unit(unit_id).ok_or("Unit not found")?;
                if unit.owner != player { return Err("Not your unit".into()); }
                if unit.moved { return Err("Already moved".into()); }
                let reachable = reachable_positions(unit, &self.map, &self.units);
                if !reachable.contains(&to) { return Err("Can't reach".into()); }

                let old_pos = self.get_unit(unit_id).unwrap().pos;
                if old_pos != to {
                    if let Some(b) = self.buildings.iter_mut()
                        .find(|b| b.pos == old_pos && b.capturing_player == Some(player))
                    {
                        b.capture_progress = 0;
                        b.capturing_player = None;
                    }
                }
                if let Some(u) = self.get_unit_mut(unit_id) { u.pos = to; u.moved = true; }
                Ok(())
            }
            Command::Attack { unit_id, target_pos } => {
                let unit = self.get_unit(unit_id).ok_or("Unit not found")?;
                if unit.owner != player { return Err("Not your unit".into()); }
                if unit.attacked { return Err("Already attacked".into()); }
                if unit.unit_type.is_ranged() && unit.moved { return Err("Artillery can't move and attack".into()); }
                self.resolve_combat(unit_id, target_pos)?;
                self.check_elimination();
                Ok(())
            }
            Command::MoveAttack { unit_id, move_to, target_pos } => {
                let unit = self.get_unit(unit_id).ok_or("Unit not found")?;
                if unit.owner != player { return Err("Not your unit".into()); }
                if unit.unit_type.is_ranged() { return Err("Artillery can't move and attack".into()); }
                self.apply_command(player, Command::Move { unit_id, to: move_to })?;
                self.apply_command(player, Command::Attack { unit_id, target_pos })?;
                Ok(())
            }
            Command::Capture { unit_id } => { self.process_capture(unit_id) }
            Command::Build { unit_type } => {
                if self.gold[player] < unit_type.cost() { return Err("Can't afford".into()); }
                let hq = self.hq_pos(player);
                if self.units.iter().any(|u| u.pos == hq && u.hp > 0) { return Err("HQ occupied".into()); }
                self.gold[player] -= unit_type.cost();
                let id = self.spawn_unit(player, unit_type, hq);
                if let Some(u) = self.get_unit_mut(id) { u.moved = true; u.attacked = true; }
                Ok(())
            }
            Command::EndTurn => {
                let next = 1 - self.current_player;
                self.collect_income(next);
                self.reset_units(next);
                self.current_player = next;
                if next == 0 {
                    self.turn += 1;
                    self.check_score_victory(self.rules.turn_limit);
                }
                Ok(())
            }
        }
    }

    fn is_terminal(&self) -> bool { self.winner.is_some() }
    fn outcome(&self) -> Option<GameOutcome> { self.winner.map(GameOutcome::Winner) }
    fn turn_number(&self) -> u32 { self.turn }
    fn num_players(&self) -> usize { 2 }
}
