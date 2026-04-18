use super::types::*;

pub const STARTING_GOLD: u32 = 2000;
pub const INCOME_PER_BUILDING: u32 = 1000;
pub const CAPTURE_THRESHOLD: u8 = 20;

/// Hard mode rules — cities require multi-turn commitment to capture.
pub const HARD_STARTING_GOLD: u32 = 3000;
pub const HARD_INCOME_PER_BUILDING: u32 = 2000;
pub const HARD_CAPTURE_THRESHOLD: u8 = 50; // ~5 turns per city

/// Map configuration.
#[derive(Debug, Clone)]
pub struct MapConfig {
    pub rows: u8,
    pub cols: u8,
    pub grid: Vec<Vec<Terrain>>,
    pub buildings: Vec<Building>,
}

impl MapConfig {
    pub fn in_bounds(&self, pos: Pos) -> bool {
        pos.row < self.rows && pos.col < self.cols
    }

    pub fn terrain_at(&self, pos: Pos) -> Terrain {
        self.grid[pos.row as usize][pos.col as usize]
    }

    pub fn is_visible(&self, visibility: &Vec<Vec<bool>>, pos: Pos) -> bool {
        visibility[pos.row as usize][pos.col as usize]
    }
}

/// Generate the classic 8x8 map.
pub fn generate_8x8() -> MapConfig {
    use Terrain::*;

    let grid = vec![
        vec![HQ,     Plains, Forest,  Plains,  Plains,  Mountain,Plains,  Plains  ],
        vec![Plains, City,   Plains,  Forest,  Plains,  Plains,  Plains,  Plains  ],
        vec![Plains, Plains, Mountain,Plains,  City,    Plains,  Forest,  Plains  ],
        vec![Forest, Plains, Plains,  Plains,  Plains,  Plains,  Plains,  Forest  ],
        vec![Forest, Plains, Plains,  Plains,  Plains,  Plains,  Plains,  Forest  ],
        vec![Plains, Forest, Plains,  City,    Plains,  Mountain,Plains,  Plains  ],
        vec![Plains, Plains, Plains,  Plains,  Forest,  Plains,  City,    Plains  ],
        vec![Plains, Plains, Mountain,Plains,  Plains,  Forest,  Plains,  HQ      ],
    ];

    build_map_config(8, 8, grid)
}

/// Generate a 12x12 map — more room for flanking and multi-front warfare.
pub fn generate_12x12() -> MapConfig {
    use Terrain::*;
    let mut grid = vec![vec![Plains; 12]; 12];

    // HQs
    grid[0][0] = HQ;
    grid[11][11] = HQ;

    // Cities (6 neutral + symmetric)
    grid[1][3] = City;    grid[10][8] = City;
    grid[3][1] = City;    grid[8][10] = City;
    grid[2][8] = City;    grid[9][3] = City;
    grid[5][5] = City;    grid[6][6] = City;

    // Terrain
    grid[1][1] = Forest;  grid[10][10] = Forest;
    grid[2][4] = Forest;  grid[9][7] = Forest;
    grid[4][2] = Mountain;grid[7][9] = Mountain;
    grid[3][6] = Forest;  grid[8][5] = Forest;
    grid[5][0] = Forest;  grid[6][11] = Forest;
    grid[4][8] = Mountain;grid[7][3] = Mountain;
    grid[6][2] = Forest;  grid[5][9] = Forest;
    grid[0][5] = Mountain;grid[11][6] = Mountain;

    build_map_config(12, 12, grid)
}

/// Generate a 16x16 map — large, requires proper scouting and multi-army coordination.
pub fn generate_16x16() -> MapConfig {
    use Terrain::*;
    let mut grid = vec![vec![Plains; 16]; 16];

    // HQs in corners
    grid[0][0] = HQ;
    grid[15][15] = HQ;

    // Many cities
    for &(r, c) in &[
        (1,4), (14,11), (2,9), (13,6), (4,1), (11,14),
        (4,7), (11,8), (7,3), (8,12), (6,10), (9,5),
        (3,13), (12,2), (7,7), (8,8),
    ] {
        grid[r][c] = City;
    }

    // Forests and mountains
    for &(r, c) in &[
        (1,1), (14,14), (2,5), (13,10), (3,3), (12,12),
        (5,0), (10,15), (0,7), (15,8), (6,4), (9,11),
        (4,9), (11,6), (7,1), (8,14), (3,8), (12,7),
    ] {
        grid[r][c] = Forest;
    }
    for &(r, c) in &[
        (2,2), (13,13), (5,5), (10,10), (0,10), (15,5),
        (6,8), (9,7), (4,12), (11,3),
    ] {
        grid[r][c] = Mountain;
    }

    build_map_config(16, 16, grid)
}

/// Generate a random symmetric map of the given size with a seed.
///
/// Maps are always diagonally symmetric (what's at (r,c) for player 0
/// is at (rows-1-r, cols-1-c) for player 1) so both sides are fair.
pub fn generate_random(rows: u8, cols: u8, num_cities: u8, seed: u64) -> MapConfig {
    use Terrain::*;
    let mut rng = seed.max(1);

    let mut xorshift = |rng: &mut u64| -> u64 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };

    let mut grid = vec![vec![Plains; cols as usize]; rows as usize];

    // HQs in corners
    grid[0][0] = HQ;
    grid[(rows - 1) as usize][(cols - 1) as usize] = HQ;

    // Place cities symmetrically
    let mut cities_placed = 0u8;
    let mut attempts = 0u32;
    while cities_placed < num_cities && attempts < 500 {
        attempts += 1;
        let r = (xorshift(&mut rng) % rows as u64) as usize;
        let c = (xorshift(&mut rng) % cols as u64) as usize;
        let mr = (rows - 1) as usize - r;
        let mc = (cols - 1) as usize - c;

        // Skip corners (HQ), skip if already placed, skip if too close to HQ
        if (r == 0 && c == 0) || (r == mr && c == mc) { continue; }
        if grid[r][c] != Plains || grid[mr][mc] != Plains { continue; }
        let dist_hq0 = r + c;
        let dist_hq1 = mr + mc;
        if dist_hq0 < 2 || dist_hq1 < 2 { continue; }

        // Don't place on the exact mirror if it's the same cell
        if r == mr && c == mc {
            grid[r][c] = City;
            cities_placed += 1;
        } else {
            grid[r][c] = City;
            grid[mr][mc] = City;
            cities_placed += 2;
        }
    }

    // Place forests symmetrically
    let num_forests = (rows as u32 * cols as u32) / 8;
    let mut placed = 0u32;
    attempts = 0;
    while placed < num_forests && attempts < 500 {
        attempts += 1;
        let r = (xorshift(&mut rng) % rows as u64) as usize;
        let c = (xorshift(&mut rng) % cols as u64) as usize;
        let mr = (rows - 1) as usize - r;
        let mc = (cols - 1) as usize - c;

        if grid[r][c] != Plains { continue; }
        if grid[mr][mc] != Plains && !(r == mr && c == mc) { continue; }

        grid[r][c] = Forest;
        if !(r == mr && c == mc) {
            grid[mr][mc] = Forest;
            placed += 1;
        }
        placed += 1;
    }

    // Place mountains symmetrically
    let num_mountains = (rows as u32 * cols as u32) / 16;
    placed = 0;
    attempts = 0;
    while placed < num_mountains && attempts < 500 {
        attempts += 1;
        let r = (xorshift(&mut rng) % rows as u64) as usize;
        let c = (xorshift(&mut rng) % cols as u64) as usize;
        let mr = (rows - 1) as usize - r;
        let mc = (cols - 1) as usize - c;

        if grid[r][c] != Plains { continue; }
        if grid[mr][mc] != Plains && !(r == mr && c == mc) { continue; }

        grid[r][c] = Mountain;
        if !(r == mr && c == mc) {
            grid[mr][mc] = Mountain;
            placed += 1;
        }
        placed += 1;
    }

    build_map_config(rows, cols, grid)
}

fn build_map_config(rows: u8, cols: u8, grid: Vec<Vec<Terrain>>) -> MapConfig {
    let mut buildings = Vec::new();

    for r in 0..rows {
        for c in 0..cols {
            let terrain = grid[r as usize][c as usize];
            match terrain {
                Terrain::HQ => {
                    let owner = if r < rows / 2 { Some(0) } else { Some(1) };
                    buildings.push(Building {
                        pos: Pos::new(r, c),
                        terrain,
                        owner,
                        capture_progress: 0,
                        capturing_player: None,
                    });
                }
                Terrain::City => {
                    buildings.push(Building {
                        pos: Pos::new(r, c),
                        terrain,
                        owner: None,
                        capture_progress: 0,
                        capturing_player: None,
                    });
                }
                _ => {}
            }
        }
    }

    MapConfig { rows, cols, grid, buildings }
}

/// Compute visibility for a player given their units.
pub fn compute_visibility(
    units: &[Unit],
    player: Player,
    map: &MapConfig,
) -> Vec<Vec<bool>> {
    let mut visible = vec![vec![false; map.cols as usize]; map.rows as usize];

    for unit in units {
        if unit.owner != player { continue; }

        let vision = unit.unit_type.vision() as i8
            + map.terrain_at(unit.pos).vision_bonus();
        let vision = vision.max(1) as u8;

        for dr in -(vision as i8)..=(vision as i8) {
            for dc in -(vision as i8)..=(vision as i8) {
                let dist = dr.unsigned_abs() + dc.unsigned_abs();
                if dist > vision { continue; }

                let r = unit.pos.row as i8 + dr;
                let c = unit.pos.col as i8 + dc;
                if r < 0 || r >= map.rows as i8 || c < 0 || c >= map.cols as i8 { continue; }

                let blocked = if dist > 1 {
                    is_blocked(unit.pos, Pos::new(r as u8, c as u8), map)
                } else {
                    false
                };

                if !blocked {
                    visible[r as usize][c as usize] = true;
                }
            }
        }
    }

    visible
}

fn is_blocked(src: Pos, dst: Pos, map: &MapConfig) -> bool {
    if src.manhattan_distance(dst) <= 2 { return false; }

    let dr = dst.row as i8 - src.row as i8;
    let dc = dst.col as i8 - src.col as i8;
    let steps = dr.unsigned_abs().max(dc.unsigned_abs());
    if steps <= 1 { return false; }

    for step in 1..steps {
        let t = step as f32 / steps as f32;
        let r = (src.row as f32 + dr as f32 * t).round() as i8;
        let c = (src.col as f32 + dc as f32 * t).round() as i8;
        if r >= 0 && r < map.rows as i8 && c >= 0 && c < map.cols as i8 {
            if map.grid[r as usize][c as usize].blocks_vision() {
                return true;
            }
        }
    }

    false
}

/// Find reachable positions for a unit.
pub fn reachable_positions(unit: &Unit, map: &MapConfig, all_units: &[Unit]) -> Vec<Pos> {
    let mut cost = vec![vec![u8::MAX; map.cols as usize]; map.rows as usize];
    let mut result = Vec::new();

    let max_move = unit.unit_type.move_range();
    cost[unit.pos.row as usize][unit.pos.col as usize] = 0;

    let mut queue = vec![(unit.pos, 0u8)];

    while let Some((pos, spent)) = queue.pop() {
        for (dr, dc) in [(-1i8, 0), (1, 0), (0, -1), (0, 1)] {
            let nr = pos.row as i8 + dr;
            let nc = pos.col as i8 + dc;
            if nr < 0 || nr >= map.rows as i8 || nc < 0 || nc >= map.cols as i8 { continue; }

            let npos = Pos::new(nr as u8, nc as u8);
            let terrain = map.terrain_at(npos);

            if !unit.unit_type.can_enter(terrain) { continue; }

            let mc = terrain.move_cost();
            let new_cost = spent + mc;
            if new_cost > max_move { continue; }

            if new_cost < cost[npos.row as usize][npos.col as usize] {
                cost[npos.row as usize][npos.col as usize] = new_cost;

                let enemy_blocks = all_units.iter()
                    .any(|u| u.pos == npos && u.owner != unit.owner && u.hp > 0);
                if enemy_blocks { continue; }

                let friendly_blocks = all_units.iter()
                    .any(|u| u.id != unit.id && u.pos == npos && u.owner == unit.owner && u.hp > 0);

                if !friendly_blocks {
                    result.push(npos);
                }

                queue.push((npos, new_cost));
            }
        }
    }

    result
}
