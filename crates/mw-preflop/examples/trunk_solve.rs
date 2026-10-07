//! Experimental deterministic DCFR solver for the public L0 preflop trunk.
use anyhow::{Context, Result, bail, ensure};
use mw_preflop::trunk::{
    l0::{self, EvaluationOptions, Model, SolveOptions, SolveTimings, Tree},
    l1,
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

fn sampling(value: &str) -> Result<l1::Sampling> {
    match value {
        "random" => Ok(l1::Sampling::Random),
        "stratified" => Ok(l1::Sampling::Stratified),
        _ => bail!("board sampling must be random or stratified"),
    }
}

fn main() -> Result<()> {
    let mut leaf_model = String::from("l0");
    let mut ehs2_cache = None;
    let mut l1_options = l1::Options::default();
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
                "trunk_solve --config TOML [--leaf-model l0|l1] [--ehs2-cache PATH (required for l1)] [--l1-boards 1] [--l1-seed 0] [--l1-eval-boards 1024 (even, >=2)] [--l1-eval-seed 0] [--l1-train-control false] [--l1-eval-control false] [--l1-sampling random|stratified] [--l1-eval-sampling random|stratified] [--tables-dir PATH] [--t3-samples 4096] [--t3-seed 0] [--k4-samples 2048] [--seed 0] [--solver-k4-samples N] [--solver-k4-min-samples M (requires N)] [--threads N] [--iterations 1000] [--eval-every 100] [--target-nash-conv X] [--alpha 1.5] [--beta 0] [--gamma 2] [--print-every 0] [--output JSON] [--output-profile JSON]\n--k4-samples and --seed define the model/evaluator; solver K4 flags use iteration-varying samples only during solving."
            );
            return Ok(());
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--leaf-model" => leaf_model = value,
            "--ehs2-cache" => ehs2_cache = Some(PathBuf::from(value)),
            "--l1-boards" => l1_options.l1_boards = value.parse()?,
            "--l1-seed" => l1_options.l1_seed = value.parse()?,
            "--l1-eval-boards" => l1_options.l1_eval_boards = value.parse()?,
            "--l1-eval-seed" => l1_options.l1_eval_seed = value.parse()?,
            "--l1-train-control" => l1_options.l1_train_control = value.parse()?,
            "--l1-eval-control" => l1_options.l1_eval_control = value.parse()?,
            "--l1-sampling" => l1_options.l1_sampling = sampling(&value)?,
            "--l1-eval-sampling" => l1_options.l1_eval_sampling = sampling(&value)?,
            "--config" => config = Some(PathBuf::from(value)),
            "--tables-dir" => tables_dir = value.into(),
            "--t3-samples" => t3_samples = value.parse()?,
            "--t3-seed" => t3_seed = value.parse()?,
            "--k4-samples" => model_options.k4_samples = value.parse()?,
            "--solver-k4-samples" => options.k4_samples = Some(value.parse()?),
            "--solver-k4-min-samples" => options.k4_min_samples = Some(value.parse()?),
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
    ensure!(
        options.k4_samples != Some(0),
        "solver K4 samples must be positive"
    );
    ensure!(
        options
            .k4_min_samples
            .is_none_or(|min| min >= 1 && options.k4_samples.is_some_and(|n| min <= n)),
        "--solver-k4-min-samples requires --solver-k4-samples and 1 <= min <= samples"
    );
    ensure!(
        leaf_model == "l0" || leaf_model == "l1",
        "--leaf-model must be l0 or l1"
    );
    ensure!(
        leaf_model != "l1" || ehs2_cache.is_some(),
        "--ehs2-cache is required for l1"
    );
    ensure!(
        l1_options.l1_boards > 0
            && l1_options.l1_eval_boards >= 2
            && l1_options.l1_eval_boards.is_multiple_of(2),
        "invalid L1 board counts"
    );
    let source = config.context("--config is required")?;
    ensure!(threads != Some(0), "threads must be positive");
    let total = Instant::now();
    let game = l0::game_from_config(&std::fs::read_to_string(&source)?, &source)?;
    let start = Instant::now();
    let tree = Tree::build_with(
        &game,
        if leaf_model == "l1" {
            l0::FlopLeaves::L1
        } else {
            l0::FlopLeaves::Checkdown
        },
    )?;
    if leaf_model == "l0" && tree.flop_leaves > 0 {
        println!("Postflop menus solved in L0 Checkdown mode.");
    }
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
    if leaf_model == "l1" {
        l1_options.trunk = options;
        return run_l1(
            &source,
            ehs2_cache.as_ref().unwrap(),
            &tree,
            &model,
            &pool,
            l1_options,
            print_every,
            output.as_deref(),
            output_profile.as_deref(),
            &t2,
            &t3,
            tree_seconds,
            tables_seconds,
            model_seconds,
            total,
        );
    }
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

#[allow(clippy::too_many_arguments)]
fn run_l1(
    source: &Path,
    cache: &Path,
    tree: &Tree,
    model: &Model<'_>,
    pool: &rayon::ThreadPool,
    options: l1::Options,
    print_every: u64,
    output: Option<&Path>,
    output_profile: Option<&Path>,
    t2: &HuShowdownTable,
    t3: &ThreeWayTable,
    tree_seconds: f64,
    tables_seconds: f64,
    model_seconds: f64,
    total: Instant,
) -> Result<()> {
    use mw_preflop::{
        Street,
        card_abstraction::{Ehs2Abstraction, Ehs2Params},
    };
    let counts = &model.game().config().abstraction;
    let params = Ehs2Params {
        flop_buckets: u32::from(counts.flop_buckets),
        turn_buckets: u32::from(counts.turn_buckets),
        river_buckets: u32::from(counts.river_buckets),
    };
    ensure!(
        [
            params.flop_buckets,
            params.turn_buckets,
            params.river_buckets
        ]
        .iter()
        .all(|&n| n > 0 && n <= u16::MAX as u32),
        "invalid bucket counts"
    );
    let abstraction = Ehs2Abstraction::load_or_build(
        params,
        &[Street::Flop, Street::Turn, Street::River],
        Some(cache),
    );
    let mut hasher = blake3::Hasher::new();
    let bytes = std::io::copy(
        &mut std::fs::File::open(cache).with_context(|| format!("reading {}", cache.display()))?,
        &mut hasher,
    )?;
    let ehs2_metadata = json!({
        "path": cache,
        "bucket_counts": params,
        "bytes": bytes,
        "blake3": hasher.finalize().to_hex().as_str(),
    });
    let storage = l1::Strategies::new(tree, &abstraction);
    println!(
        "L1: {} leaves; {} postflop decisions; {} slots; {} storage bytes",
        storage.leaves.len(),
        storage.nodes,
        storage.slots,
        storage.storage_bytes()
    );
    drop(storage);
    let mut previous = l1::Timings::default();
    let mut iteration_start = Instant::now();
    let observer = |p: &l1::Progress<'_>| {
        let now = Instant::now();
        if print_every > 0 && p.iteration > 0 && p.iteration.is_multiple_of(print_every) {
            let (t, q) = (p.timings, &previous);
            println!(
                "Iteration {}: wall {:.6}s; reaches {:.6}; t2 {:.6}; t3 {:.6}; k4 {:.6}; update {:.6}; \
                 board preparation {:.6}; postflop {:.6}; evaluation {:.6}",
                p.iteration,
                now.duration_since(iteration_start).as_secs_f64(),
                t.trunk.reaches - q.trunk.reaches,
                t.trunk.t2 - q.trunk.t2,
                t.trunk.t3 - q.trunk.t3,
                t.trunk.k4 - q.trunk.k4,
                t.trunk.update - q.trunk.update,
                t.board_preparation - q.board_preparation,
                t.postflop - q.postflop,
                t.trunk.evaluation - q.trunk.evaluation
            );
        }
        if let Some(c) = p.checkpoint {
            let e = &c.evaluation;
            let gains =
                |f: fn(&l1::SeatEvaluation) -> f64| e.seats.iter().map(f).collect::<Vec<_>>();
            println!(
                "Checkpoint {}: NashConv {:.12}; held {:.12}; auxiliary {:.12}; auxiliary held {:.12}; \
                 gains {:?}; held gains {:?}; auxiliary gains {:?}; auxiliary held gains {:?}; \
                 evaluation {:.6}s",
                c.iteration,
                e.nash_conv,
                e.held_nash_conv,
                e.auxiliary_nash_conv,
                e.auxiliary_held_nash_conv,
                gains(|s| s.gain),
                gains(|s| s.held_gain),
                gains(|s| s.auxiliary_gain),
                gains(|s| s.auxiliary_held_gain),
                e.seconds
            );
        }
        previous = p.timings.clone();
        iteration_start = now;
    };
    let solution = pool.install(|| l1::solve(tree, model, &abstraction, options, observer))?;
    println!(
        "Iterations: {}; reached target: {}; solve phases {:?}",
        solution.iterations, solution.reached_target, solution.timings
    );
    for warning in &model.warnings {
        println!("Warning: {warning}");
    }
    if let Some(path) = output_profile {
        write_json(path, &solution.average.export(tree))?;
    }
    if let Some(path) = output {
        write_json(
            path,
            &json!({
                "format": "p2-trunk-solve",
                "version": 2,
                "leaf_model": "l1",
                "k4": {
                    "samples": model.evaluation_options().k4_samples,
                    "seed": model.evaluation_options().seed,
                },
                "source": source,
                "game_fingerprint": blake3::Hash::from(tree.game_fingerprint).to_hex().as_str(),
                "options": options,
                "ehs2": ehs2_metadata,
                "tables": {
                    "t2": {"payload_blake3": t2.payload_hash().to_hex().as_str()},
                    "t3": {"payload_blake3": t3.payload_hash().to_hex().as_str()},
                },
                "iterations": solution.iterations,
                "reached_target": solution.reached_target,
                "l1": {
                    "leaves": solution.postflop.leaves.len(),
                    "postflop_nodes": solution.postflop.nodes,
                    "slots": solution.postflop.slots,
                    "storage_bytes": solution.postflop.storage_bytes(),
                },
                "terminals": {"counts_by_k": tree.terminal_counts()},
                "checkpoints": solution.checkpoints,
                "warnings": model.warnings,
                "timings": {
                    "tree": tree_seconds,
                    "tables": tables_seconds,
                    "model": model_seconds,
                    "solve": solution.timings,
                    "total": total.elapsed().as_secs_f64(),
                },
            }),
        )?;
    }
    Ok(())
}
