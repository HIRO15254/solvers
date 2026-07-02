use std::path::Path;

use anyhow::{Context, Result, bail};
use cards::{Chips, Player};
use engine::{
    CfrPlus, Dcfr, DiscountSchedule, F32Storage, HsDcfr, NodeKind, Solver, Vanilla, linear_cfr,
};
use game::{
    ChipEv, GgPreflopRake, Icm, NoRake, PayoffPipeline, PercentCapRake, RakeModel, UtilityModel,
};
use serde::Serialize;

use crate::config::{AlgorithmSection, RakeSection, RunSection, SolveConfig, UtilitySection};

pub fn run(config_path: &Path, output: Option<&Path>) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config: SolveConfig = toml::from_str(&raw).context("parsing config")?;

    let rake: Box<dyn RakeModel> = match config.rake {
        RakeSection::None => Box::new(NoRake),
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => Box::new(PercentCapRake {
            rate,
            cap,
            no_flop_no_drop,
        }),
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => Box::new(GgPreflopRake {
            rate,
            cap,
            exempt_pot: Chips(exempt_pot),
        }),
    };
    let utility: Box<dyn UtilityModel> = match config.utility {
        UtilitySection::ChipEv => Box::new(ChipEv),
        UtilitySection::Icm { payouts } => Box::new(Icm { payouts }),
    };
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };

    let toy = match config.game.kind.as_str() {
        "kuhn" => game::kuhn(pipeline),
        "leduc" => game::leduc(pipeline),
        other => bail!("unknown game kind {other:?} (expected \"kuhn\" or \"leduc\")"),
    };

    let schedule: Box<dyn DiscountSchedule> = match config.algorithm {
        AlgorithmSection::Vanilla => Box::new(Vanilla),
        AlgorithmSection::CfrPlus => Box::new(CfrPlus),
        AlgorithmSection::Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        } => Box::new(Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        }),
        AlgorithmSection::LinearCfr => Box::new(linear_cfr()),
        AlgorithmSection::HsDcfr { gamma0 } => Box::new(HsDcfr { gamma0 }),
    };
    let schedule_name = schedule.name();

    let node_info = toy.node_info.clone();
    let mut solver = Solver::<_, F32Storage>::new(toy.game, schedule, Some(config.run.iterations));

    println!(
        "game={} schedule={} iterations={}",
        config.game.kind, schedule_name, config.run.iterations
    );
    let start = std::time::Instant::now();
    run_loop(&mut solver, &config.run);
    let elapsed = start.elapsed();

    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    let value = solver.expected_value(Player::P0);
    println!(
        "done: iterations={} wall={:.2}s value_p0={:.6} nash_conv={:.3e}",
        solver.iteration(),
        elapsed.as_secs_f64(),
        value,
        nash_conv,
    );

    if let Some(path) = output {
        let report = export(&config.game.kind, &node_info, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }
    Ok(())
}

fn run_loop(solver: &mut Solver<game::ToyEvaluator, F32Storage>, run: &RunSection) {
    let mut remaining = run.iterations;
    while remaining > 0 {
        let chunk = run.check_every.min(remaining);
        solver.run(chunk);
        remaining -= chunk;
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        println!(
            "iter={:>8} expl_p0={:.3e} expl_p1={:.3e} nash_conv={:.3e}",
            solver.iteration(),
            expl[Player::P0],
            expl[Player::P1],
            nash_conv,
        );
        if let Some(target) = run.target_nash_conv
            && nash_conv < target
        {
            println!("target nash_conv {target:.3e} reached");
            break;
        }
    }
}

#[derive(Serialize)]
struct StrategyReport {
    game: String,
    iterations: u64,
    expected_value_p0: f64,
    exploitability: [f64; 2],
    nodes: Vec<NodeReport>,
}

#[derive(Serialize)]
struct NodeReport {
    history: String,
    player: usize,
    actions: Vec<String>,
    /// Per hand: action distribution (indexed like `actions`).
    strategy: Vec<Vec<f32>>,
}

fn export(
    game_kind: &str,
    node_info: &[game::ToyNodeInfo],
    solver: &Solver<game::ToyEvaluator, F32Storage>,
) -> StrategyReport {
    let tree = &solver.game().tree;
    let mut nodes = Vec::new();
    for node_id in 0..tree.nodes.len() as u32 {
        let node = tree.node(node_id);
        if node.kind != NodeKind::Action {
            continue;
        }
        let info = &node_info[tree.tags[node_id as usize] as usize];
        let sigma = solver.average_strategy_at(node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        let strategy = (0..num_hands)
            .map(|h| (0..num_actions).map(|a| sigma[a * num_hands + h]).collect())
            .collect();
        nodes.push(NodeReport {
            history: info.history.clone(),
            player: node.player.index(),
            actions: info.actions.clone(),
            strategy,
        });
    }
    let expl = solver.exploitability();
    StrategyReport {
        game: game_kind.to_string(),
        iterations: solver.iteration(),
        expected_value_p0: solver.expected_value(Player::P0),
        exploitability: [expl[Player::P0], expl[Player::P1]],
        nodes,
    }
}
