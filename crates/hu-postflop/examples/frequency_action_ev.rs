//! Baseline root action values for the N4 frequency experiment.
use std::path::Path;

use anyhow::{Result, ensure};
use hu_engine::{F32Storage, Solver, TerminalEvaluator};
use hu_postflop::{PerPlayer, Player, prepare};
use serde_json::json;

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: frequency_action_ev CONFIG");
    let raw = std::fs::read_to_string(&path)?;
    let p = prepare::prepare(&raw, Path::new(&path))?;
    let mut game = hu_postflop::try_build_postflop_game(&p.config, p.payoff.pipeline())?;
    let space = game.game.evaluator.register_profile_board(&p.config.board);
    ensure!(
        game.game.tree.children(0).len() == 2,
        "expected two actions at root"
    );
    let mut solver = Solver::<_, F32Storage>::new(
        game.game,
        hu_postflop::run::schedule(&p.settings.solver.algorithm),
        Some(p.settings.solver.stop.max_iterations),
    );
    solver.set_cfr_precision(p.settings.solver.cfr_precision);
    solver.run(p.settings.solver.stop.max_iterations);
    let g = solver.game();
    let actor = g.tree.node(0).player;
    let n = g.root_ranges[actor].len();
    let mut mass = vec![0.0; n];
    g.evaluator
        .compat_mass(space, actor, &g.root_ranges[actor.opponent()], &mut mass);
    let reach = PerPlayer::new(
        g.root_ranges[Player::P0].as_slice(),
        g.root_ranges[Player::P1].as_slice(),
    );
    let values: Vec<_> = g
        .tree
        .children(0)
        .map(|child| solver.expected_values_at(child, actor, reach))
        .collect();
    let strategy = solver.average_strategy_at(0);
    let combos = g.evaluator.hands.combos(actor);
    let rows: Vec<_> = (0..n).filter(|&h| mass[h] > 0.0 && g.root_ranges[actor][h] > 0.0).map(|h| {
        let (a, b) = nlh::combo_cards(combos[h] as usize);
        json!({"combo": format!("{a}{b}"), "weight": g.root_ranges[actor][h], "check": strategy[h], "check_ev": values[0][h] / mass[h], "bet_ev": values[1][h] / mass[h]})
    }).collect();
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
