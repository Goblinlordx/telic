use crate::game::types::*;
use crate::game::state::ArenaCombatGame;
use telic::arena::{GameState, GameAgent};

const DISPLAY_WIDTH: usize = 40;
const DISPLAY_HEIGHT: usize = 20;

/// Render the game state as ASCII art.
pub fn render(game: &ArenaCombatGame, elapsed: f32) {
    let view_0 = game.view_for(0);

    let all_units: Vec<(&Unit, char)> = view_0.our_units.iter()
        .map(|u| {
            let ch = if u.attack_range > 3.0 { 'a' } else { 'w' };
            (u, ch)
        })
        .chain(view_0.enemy_units.iter().map(|u| {
            let ch = if u.attack_range > 3.0 { 'A' } else { 'W' };
            (u, ch)
        }))
        .collect();

    let mut grid = vec![vec!['.'; DISPLAY_WIDTH]; DISPLAY_HEIGHT];

    for x in 0..DISPLAY_WIDTH {
        grid[0][x] = '-';
        grid[DISPLAY_HEIGHT - 1][x] = '-';
    }
    for y in 0..DISPLAY_HEIGHT {
        grid[y][0] = '|';
        grid[y][DISPLAY_WIDTH - 1] = '|';
    }

    for (unit, ch) in &all_units {
        let x = ((unit.pos.x / view_0.arena_width) * (DISPLAY_WIDTH - 2) as f32) as usize + 1;
        let y = ((unit.pos.y / view_0.arena_height) * (DISPLAY_HEIGHT - 2) as f32) as usize + 1;
        let x = x.clamp(1, DISPLAY_WIDTH - 2);
        let y = y.clamp(1, DISPLAY_HEIGHT - 2);
        grid[y][x] = *ch;
    }

    print!("\x1B[2J\x1B[H");
    println!("  Time: {:.1}s  |  P0 (w/a): {} alive  |  P1 (W/A): {} alive",
        elapsed, view_0.our_units.len(), view_0.enemy_units.len());

    let p0_hp: f32 = view_0.our_units.iter().map(|u| u.hp).sum();
    let p1_hp: f32 = view_0.enemy_units.iter().map(|u| u.hp).sum();
    println!("  P0 HP: {:.0}  |  P1 HP: {:.0}", p0_hp, p1_hp);
    println!();

    for row in &grid {
        let line: String = row.iter().collect();
        println!("  {}", line);
    }

    println!();
    println!("  Legend: w=warrior a=archer (P0 lowercase, P1 UPPERCASE)");
}

/// Run a single game with visual output.
pub fn watch_game<A0, A1>(
    mut game: ArenaCombatGame,
    agent_a: &mut A0,
    agent_b: &mut A1,
    display_fps: f32,
    max_time: f32,
)
where
    A0: GameAgent<crate::game::state::CombatView, Vec<Command>>,
    A1: GameAgent<crate::game::state::CombatView, Vec<Command>>,
{
    let display_interval = 1.0 / display_fps;
    let mut display_accumulator = 0.0;
    let dt = 1.0 / 60.0;

    agent_a.reset(0);
    agent_b.reset(1);

    agent_a.observe(&game.view_for(0));
    agent_b.observe(&game.view_for(1));

    render(&game, 0.0);

    use telic::arena::{CommandProvider, CommandTree};
    use crate::game::commands::ArenaCombatCommands;

    while !game.is_terminal() && game.turn_number() < (max_time / dt) as u32 {
        let tree0: std::sync::Arc<CommandTree<Vec<Command>>> =
            ArenaCombatCommands::command_tree(&game, 0);
        let tree1: std::sync::Arc<CommandTree<Vec<Command>>> =
            ArenaCombatCommands::command_tree(&game, 1);
        let cmds_0 = agent_a.decide(&game.view_for(0), &tree0).unwrap_or_default();
        let cmds_1 = agent_b.decide(&game.view_for(1), &tree1).unwrap_or_default();

        game.apply_command(0, cmds_0).ok();
        game.apply_command(1, cmds_1).ok();

        agent_a.observe(&game.view_for(0));
        agent_b.observe(&game.view_for(1));

        display_accumulator += dt;
        if display_accumulator >= display_interval {
            display_accumulator -= display_interval;
            let elapsed = game.turn_number() as f32 * dt;
            render(&game, elapsed);
            std::thread::sleep(std::time::Duration::from_millis(
                (display_interval * 1000.0) as u64
            ));
        }
    }

    let elapsed = game.turn_number() as f32 * dt;
    render(&game, elapsed);
    if let Some(outcome) = game.outcome() {
        println!("  GAME OVER: {:?}", outcome);
    }
}
