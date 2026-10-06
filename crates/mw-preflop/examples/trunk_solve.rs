//! Experimental deterministic DCFR solver for the public L0 preflop trunk.
use anyhow::{Context, Result, bail, ensure};
use mw_preflop::trunk::{
    l0::{self, EvaluationOptions, Model, SolveOptions, SolveTimings, Tree},
    tables::{HuShowdownTable, ThreeWayTable},
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut config = None;
    let mut tables_dir = PathBuf::from(".cache/p2-trunk");
    let mut t3_samples = 4096;
    let mut t3_seed = 0;
    let mut model_options = EvaluationOptions::default();
    let mut options = SolveOptions::default();
    let mut threads = None;
    let mut print_every = 0;
    let mut output = None;
    let mut output_profile = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "trunk_solve --config TOML [--tables-dir PATH] [--t3-samples 4096] [--t3-seed 0] [--k4-samples 2048] [--seed 0] [--threads N] [--iterations 1000] [--eval-every 100] [--target-nash-conv X] [--alpha 1.5] [--beta 0] [--gamma 2] [--print-every 0] [--output JSON] [--output-profile JSON]"
            );
            return Ok(());
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(value)),
            "--tables-dir" => tables_dir = value.into(),
            "--t3-samples" => t3_samples = value.parse()?,
            "--t3-seed" => t3_seed = value.parse()?,
            "--k4-samples" => model_options.k4_samples = value.parse()?,
            "--seed" => model_options.seed = value.parse()?,
            "--threads" => threads = Some(value.parse::<usize>()?),
            "--iterations" => options.iterations = value.parse()?,
            "--eval-every" => options.eval_every = value.parse()?,
            "--target-nash-conv" => options.target_nash_conv = Some(value.parse()?),
            "--alpha" => options.alpha = value.parse()?,
            "--beta" => options.beta = value.parse()?,
            "--gamma" => options.gamma = value.parse()?,
            "--print-every" => print_every = value.parse::<u64>()?,
            "--output" => output = Some(PathBuf::from(value)),
            "--output-profile" => output_profile = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {arg}"),
        }
    }
    let source = config.context("--config is required")?;
    ensure!(threads != Some(0), "threads must be positive");
    let total = Instant::now();
    let game = l0::game_from_config(&std::fs::read_to_string(&source)?, &source)?;
    let start = Instant::now();
    let tree = Tree::build(&game)?;
    let tree_seconds = start.elapsed().as_secs_f64();
    println!(
        "Tree: {} decisions, {} terminals ({tree_seconds:.3}s)",
        tree.nodes.iter().filter(|n| n.actor.is_some()).count(),
        tree.terminal_counts().iter().sum::<usize>()
    );
    let start = Instant::now();
    let t2 = HuShowdownTable::load_or_build(&tables_dir)?;
    let t3 = ThreeWayTable::load_or_build(&tables_dir, t3_samples, t3_seed)?;
    let tables_seconds = start.elapsed().as_secs_f64();
    let tables = (&t2, &t3);
    let start = Instant::now();
    let model = Model::new(&game, &tables, model_options)?;
    let model_seconds = start.elapsed().as_secs_f64();
    println!("Tables: {tables_seconds:.3}s; model: {model_seconds:.3}s");
    let mut builder = rayon::ThreadPoolBuilder::new();
    if let Some(threads) = threads {
        builder = builder.num_threads(threads);
    }
    let pool = builder.build()?;
    let mut previous = SolveTimings::default();
    let mut iteration_start = Instant::now();
    let observer = |progress: &l0::Progress<'_>| {
        let now = Instant::now();
        if print_every > 0
            && progress.iteration > 0
            && progress.iteration.is_multiple_of(print_every)
        {
            let t = progress.timings;
            println!(
                "Iteration {}: wall {:.6}s; reaches {:.6}; t2 {:.6}; t3 {:.6}; k4 {:.6}; update {:.6}; evaluation {:.6}",
                progress.iteration,
                now.duration_since(iteration_start).as_secs_f64(),
                t.reaches - previous.reaches,
                t.t2 - previous.t2,
                t.t3 - previous.t3,
                t.k4 - previous.k4,
                t.update - previous.update,
                t.evaluation - previous.evaluation
            );
        }
        if let Some(c) = progress.checkpoint {
            println!(
                "Checkpoint {}: NashConv {:.12}; gains {:?}; {:.6}s",
                c.iteration,
                c.nash_conv,
                c.seats.iter().map(|s| s.gain).collect::<Vec<_>>(),
                c.seconds
            );
        }
        previous = progress.timings.clone();
        iteration_start = Instant::now();
    };
    let solution = pool.install(|| l0::solve(&tree, &model, options, observer))?;
    println!(
        "Iterations: {}; reached target: {}; solve phases {:?}",
        solution.iterations, solution.reached_target, solution.timings
    );
    for warning in &model.warnings {
        println!("Warning: {warning}");
    }
    if let Some(path) = output_profile {
        write_json(&path, &solution.average.export(&tree))?;
    }
    if let Some(path) = output {
        let fingerprint: String = tree
            .game_fingerprint
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        write_json(
            &path,
            &json!({
                "format": "p2-trunk-solve",
                "version": 1,
                "source": source,
                "game_fingerprint": fingerprint,
                "tables": {
                    "t2": {"file": "t2-v1.bin", "payload_blake3": t2.payload_hash().to_hex().as_str()},
                    "t3": {
                        "file": format!("t3-v1-n{t3_samples}-seed{t3_seed}.bin"),
                        "payload_blake3": t3.payload_hash().to_hex().as_str(),
                        "samples": t3_samples,
                        "seed": t3_seed
                    }
                },
                "k4": {"samples": model_options.k4_samples, "seed": model_options.seed},
                "options": options,
                "iterations": solution.iterations,
                "reached_target": solution.reached_target,
                "checkpoints": solution.checkpoints,
                "terminals": {"counts_by_k": tree.terminal_counts()},
                "warnings": model.warnings,
                "timings": {
                    "tree": tree_seconds,
                    "tables": tables_seconds,
                    "model": model_seconds,
                    "solve": solution.timings,
                    "total": total.elapsed().as_secs_f64()
                }
            }),
        )?;
    }
    Ok(())
}
