//! THROWAWAY experiment (not for merge): chance-sampled MCCFR warm start for
//! the exact DCFR solver.
//!
//! usage: warm_mc CONFIG --threads N --mc ITERS [--mc-every K] [--t0 T]
//!        [--rscale C] [--sscale C] [--target NASHCONV] [--every E] [--max M]
//!
//! Prints JSON lines: phase, iteration, seconds (excluding evaluation),
//! nashConv.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use hu_engine::{
    F32Storage, McCfg, McSolver, ParConfig, Solver, SolverState, StorageState,
};
use hu_postflop::{Player, prepare};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config = None;
    let mut threads = std::thread::available_parallelism()?.get();
    let (mut mc, mut mc_every, mut t0) = (0u64, 0u64, 0u64);
    let (mut rscale, mut sscale) = (1.0f32, 1.0f32);
    let (mut target, mut every, mut max) = (0.0f64, 10u64, 2000u64);
    while let Some(arg) = args.next() {
        let mut value = || args.next().context("missing value");
        match arg.as_str() {
            "--threads" => threads = value()?.parse()?,
            "--mc" => mc = value()?.parse()?,
            "--mc-every" => mc_every = value()?.parse()?,
            "--t0" => t0 = value()?.parse()?,
            "--rscale" => rscale = value()?.parse()?,
            "--sscale" => sscale = value()?.parse()?,
            "--target" => target = value()?.parse()?,
            "--every" => every = value()?.parse()?,
            "--max" => max = value()?.parse()?,
            other => config = Some(PathBuf::from(other)),
        }
    }
    let config = config.context("missing CONFIG")?;
    let raw = std::fs::read_to_string(&config)?;
    let prepared = prepare::prepare(&raw, &config)?;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    pool.install(|| -> Result<()> {
        let build = || hu_postflop::try_build_postflop_game(&prepared.config, prepared.payoff.pipeline());
        let mut elapsed = 0.0f64;
        let mut storage = None;
        if mc > 0 {
            let mut solver = McSolver::<_, F32Storage>::new(build()?.game, McCfg::default());
            let every = if mc_every == 0 { mc } else { mc_every };
            while solver.iteration() < mc {
                let start = Instant::now();
                solver.run(every.min(mc - solver.iteration()));
                elapsed += start.elapsed().as_secs_f64();
                let expl = solver.exploitability();
                println!(
                    "{{\"phase\":\"mc\",\"iteration\":{},\"seconds\":{:.3},\"nashConv\":{}}}",
                    solver.iteration(),
                    elapsed,
                    expl[Player::P0] + expl[Player::P1]
                );
            }
            storage = Some(solver.state().storage);
        }
        let mut solver = Solver::<_, F32Storage>::new(
            build()?.game,
            hu_postflop::run::schedule(&prepared.settings.solver.algorithm),
            Some(prepared.settings.solver.stop.max_iterations),
        );
        solver.set_cfr_precision(prepared.settings.solver.cfr_precision);
        solver.set_par(ParConfig {
            chance_depth: prepared.settings.solver.parallel.chance_depth,
            min_children: prepared.settings.solver.parallel.min_children,
        });
        if let Some(StorageState::F32 {
            mut regrets,
            mut strategy_sum,
        }) = storage
        {
            regrets.iter_mut().for_each(|r| *r *= rscale);
            strategy_sum.iter_mut().for_each(|s| *s *= sscale);
            solver
                .restore_state(SolverState {
                    iteration: t0,
                    storage: StorageState::F32 {
                        regrets,
                        strategy_sum,
                    },
                })
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        let first = solver.iteration();
        while solver.iteration() < first + max {
            let start = Instant::now();
            solver.run(every);
            elapsed += start.elapsed().as_secs_f64();
            let (_, expl) = solver.evaluate();
            let nash_conv = expl[Player::P0] + expl[Player::P1];
            println!(
                "{{\"phase\":\"cfr\",\"iteration\":{},\"seconds\":{:.3},\"nashConv\":{}}}",
                solver.iteration() - first,
                elapsed,
                nash_conv
            );
            if nash_conv <= target {
                break;
            }
        }
        Ok(())
    })
}
