//! P1 throughput harness: times count/build and GameIndex creation, CFR
//! iterations and Solver::evaluate() separately for one `solvers.nlh/v1`
//! P1 config, and reports the process peak memory. Builder node_info is
//! released before solver storage allocation; the index stays alive through
//! all measurements, matching the product. One thread count per process, so
//! every measurement starts from a fresh heap. `buildSecs` includes count;
//! `evaluateSecs` (including iteration bands) measures full EV/BR evaluation,
//! not the historical exploitability-only `evalSecs`. The memory estimate's
//! resident components (for `--threads`) sit beside `gameHeapBytes`, the
//! compiled tree, evaluator and index measured after the build.
//!
//! ```text
//! cargo run --release -p hu-postflop --example p1_bench -- CONFIG \
//!     [--threads N] [--warmup N] [--iters N] [--evals N] [--storage f32|i16|i16-f32avg] [--json PATH] [--bands]
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use hu_engine::{F32Storage, I16Storage, MixedStorage, ParConfig, Solver, Storage};
use hu_postflop::index::GameIndex;
use hu_postflop::input::Storage as StorageKind;
use hu_postflop::{Player, prepare};

struct Args {
    config: PathBuf,
    threads: usize,
    warmup: u64,
    iters: u64,
    evals: u64,
    storage: Option<StorageKind>,
    json: Option<PathBuf>,
    bands: bool,
    experiment_base: Option<PathBuf>,
    experiment_mode: Option<String>,
    experiment_timing: Option<Vec<u64>>,
}

fn parse_args() -> Result<Args> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        config: PathBuf::new(),
        threads: std::thread::available_parallelism()?.get(),
        warmup: 2,
        iters: 10,
        evals: 1,
        storage: None,
        json: None,
        bands: false,
        experiment_base: None,
        experiment_mode: None,
        experiment_timing: None,
    };
    let mut config = None;
    while let Some(arg) = args.next() {
        let mut value = || args.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--threads" => parsed.threads = value()?.parse()?,
            "--warmup" => parsed.warmup = value()?.parse()?,
            "--iters" => parsed.iters = value()?.parse()?,
            "--evals" => parsed.evals = value()?.parse()?,
            "--storage" => {
                parsed.storage = Some(match value()?.as_str() {
                    "f32" => StorageKind::F32,
                    "i16" => StorageKind::I16,
                    "i16-f32avg" => StorageKind::I16F32Avg,
                    other => bail!("unknown storage {other}"),
                })
            }
            "--bands" => parsed.bands = true,
            "--experiment-base" => parsed.experiment_base = Some(value()?.into()),
            "--experiment-mode" => parsed.experiment_mode = Some(value()?),
            "--experiment-timing" => {
                parsed.experiment_timing = Some(
                    value()?
                        .split(',')
                        .map(str::parse)
                        .collect::<Result<_, _>>()?,
                );
            }
            "--json" => parsed.json = Some(value()?.into()),
            other if other.starts_with("--") => bail!("unknown flag {other}"),
            other => config = Some(PathBuf::from(other)),
        }
    }
    parsed.config = config.context("missing CONFIG")?;
    Ok(parsed)
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let raw = std::fs::read_to_string(&args.config)?;
    let t0 = Instant::now();
    let raw = input_with_storage(&raw, &args.config, args.storage)?;
    let prepared = prepare::prepare(&raw, &args.config)?;
    let prepare_secs = t0.elapsed().as_secs_f64();
    // Zero-work measurement must not allocate a potentially tens-of-GB arena.
    if args.iters == 0 && args.evals == 0 {
        let report = serde_json::json!({
            "config": args.config.display().to_string(),
            "threads": args.threads,
            "cfrPrecision": prepared.settings.solver.cfr_precision.name(),
            "estimateF32Bytes": prepared.estimate.f32_bytes,
            "estimateI16Bytes": prepared.estimate.i16_bytes,
            "estimateI16F32avgBytes": prepared.estimate.i16_f32avg_bytes,
            "storageElements": prepared.estimate.f32_bytes / 8,
            "nodes": prepared.estimate.nodes,
            "maxDepth": prepared.estimate.max_depth,
            "maxDeals": prepared.estimate.max_deals,
            "estimateGameBytes": prepared.estimate.game_bytes,
            "estimateThreadBytes": prepared.estimate.thread_bytes,
            "prepareSecs": prepare_secs,
        });
        println!("{}", serde_json::to_string(&report)?);
        if let Some(path) = &args.json {
            std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        }
        return Ok(());
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()?;
    pool.install(|| match prepared.settings.solver.storage {
        StorageKind::F32 => bench::<F32Storage>(&args, &prepared, prepare_secs, "f32"),
        StorageKind::I16 => bench::<I16Storage>(&args, &prepared, prepare_secs, "i16"),
        StorageKind::I16F32Avg => {
            bench::<MixedStorage>(&args, &prepared, prepare_secs, "i16-f32avg")
        }
    })
}

fn input_with_storage(raw: &str, path: &Path, storage: Option<StorageKind>) -> Result<String> {
    let Some(storage) = storage else {
        return Ok(raw.into());
    };
    // Apply the override before normalization makes omitted defaults explicit.
    let mut document = spot::Document::parse(raw, path)?;
    document
        .solver
        .insert("storage".into(), toml::Value::try_from(storage)?);
    Ok(document.normalize(&hu_postflop::input::P1Sections)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hu_postflop::input::{Algorithm, Settings};

    #[test]
    fn storage_override_resolves_omitted_reset_and_preserves_explicit_values() {
        let base = include_str!("../../../examples/hu-postflop/river_small.toml");
        let path = Path::new("input.toml");
        for (original, storage, explicit) in [
            ("f32", StorageKind::I16, None),
            ("i16", StorageKind::F32, None),
            ("i16", StorageKind::I16F32Avg, None),
            ("f32", StorageKind::I16, Some(false)),
            ("i16", StorageKind::F32, Some(true)),
        ] {
            let mut raw = format!("{base}\n[solver]\nstorage = '{original}'\n");
            if let Some(reset) = explicit {
                raw.push_str(&format!("[solver.algorithm]\npow4_reset = {reset}\n"));
            }
            let effective = input_with_storage(&raw, path, Some(storage)).unwrap();
            let document = spot::Document::parse(&effective, path).unwrap();
            let settings =
                Settings::parse(&document.spot, &document.solver, &document.output).unwrap();
            assert_eq!(settings.solver.storage, storage.into());
            let Algorithm::Dcfr { pow4_reset, .. } = settings.solver.algorithm else {
                panic!("expected dcfr");
            };
            assert_eq!(pow4_reset, explicit.unwrap_or(storage == StorageKind::I16));
        }
    }
}

fn bench<S: Storage>(
    args: &Args,
    p: &prepare::Prepared,
    prepare_secs: f64,
    storage_name: &str,
) -> Result<()> {
    let experiment_started = Instant::now();
    let t = Instant::now();
    let (game, index, resolved, node_info) = if p.document.spot.profile.is_some() {
        let (game, index, resolved) = hu_postflop::run::build_profiled(p)?;
        let node_info = None;
        (game, index, resolved, node_info)
    } else {
        let built = hu_postflop::try_build_postflop_game(&p.config, p.payoff.pipeline())?;
        let node_info = Some(built.node_info.len());
        let index = GameIndex::from_game(
            &built.game,
            &built.node_info,
            p.document.spot.context.street,
        );
        (built.game, index, None, node_info)
    };
    let tree = &game.tree;
    let nodes = tree.nodes.len();
    let action_nodes = tree.storage_refs.len();
    let storage_elements = tree.storage_len;
    let deals = tree.deals.len();
    let transitions = tree.transitions.len();
    let transition_entries: usize = tree.transitions.iter().map(|t| t.entries.len()).sum();
    let game_heap_bytes = tree.heap_bytes() + game.evaluator.heap_bytes() + index.heap_bytes();
    let hands = [
        game.evaluator.hands.len(Player::P0),
        game.evaluator.hands.len(Player::P1),
    ];
    hu_postflop::run::trim_builder_heap();
    let build_secs = t.elapsed().as_secs_f64();
    let after_build_mem = memory::current_bytes();

    let t = Instant::now();
    let mut solver = Solver::<_, S>::new(
        game,
        hu_postflop::run::schedule(&p.settings.solver.algorithm),
        Some(p.settings.solver.stop.max_iterations),
    );
    solver.set_cfr_precision(p.settings.solver.cfr_precision);
    let _controller = hu_postflop::run::install_profile(&mut solver, resolved, None)?;
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    let alloc_secs = t.elapsed().as_secs_f64();
    if let Some(mode) = &args.experiment_mode {
        return warm_experiment(args, p, solver, mode, experiment_started, prepare_secs);
    }

    let t = Instant::now();
    solver.run(args.warmup);
    let warmup_secs = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let mut bands = Vec::new();
    let mut band_step_secs = [Vec::new(), Vec::new()];
    if args.bands {
        for _ in 0..args.iters {
            let step_start = Instant::now();
            solver.step();
            let seconds = step_start.elapsed().as_secs_f64();
            let iteration = solver.iteration();
            if iteration <= 50 {
                band_step_secs[0].push(seconds);
            } else if (250..=300).contains(&iteration) {
                band_step_secs[1].push(seconds);
            }
            if matches!(iteration, 50 | 300) {
                let eval_start = Instant::now();
                let (_, expl) = solver.evaluate();
                let index = usize::from(iteration == 300);
                let steps = &band_step_secs[index];
                bands.push(serde_json::json!({
                    "firstIteration": if iteration == 50 { 1 } else { 250 },
                    "lastIteration": iteration, "samples": steps.len(),
                    "stepSecs": steps,
                    "secsPerIter": steps.iter().sum::<f64>() / steps.len() as f64,
                    "evaluateSecs": eval_start.elapsed().as_secs_f64(),
                    "nashConv": expl[Player::P0] + expl[Player::P1],
                }));
            }
        }
    } else {
        solver.run(args.iters);
    }
    // With --bands this wall interval includes the two sampled evaluations;
    // the per-band step times above exclude evaluation.
    let iter_secs = t.elapsed().as_secs_f64();

    let mut evaluate_secs = Vec::new();
    let mut nash_conv = f64::NAN;
    for _ in 0..args.evals {
        let t = Instant::now();
        let (_, expl) = solver.evaluate();
        evaluate_secs.push(t.elapsed().as_secs_f64());
        nash_conv = expl[Player::P0] + expl[Player::P1];
    }
    let peak = memory::peak_bytes();
    let per_iter = if args.iters > 0 {
        iter_secs / args.iters as f64
    } else {
        f64::NAN
    };
    // The estimate for this process's worker count rather than the config's.
    let estimate = hu_postflop::MemoryEstimate {
        threads: args.threads as u64,
        ..p.estimate.clone()
    };
    let mut report = serde_json::json!({
        "config": args.config.display().to_string(),
        "threads": args.threads,
        "cfrPrecision": p.settings.solver.cfr_precision.name(),
        "storage": storage_name,
        "nodes": nodes,
        "actionNodes": action_nodes,
        "storageElements": storage_elements,
        "deals": deals,
        "transitions": transitions,
        "transitionEntries": transition_entries,
        "nodeInfo": node_info,
        "hands": hands,
        "maxDepth": p.estimate.max_depth,
        "maxDeals": p.estimate.max_deals,
        "estimateF32Bytes": p.estimate.f32_bytes,
        "estimateI16Bytes": p.estimate.i16_bytes,
        "estimateI16F32avgBytes": p.estimate.i16_f32avg_bytes,
        "estimateGameBytes": p.estimate.game_bytes,
        "estimateThreadBytes": p.estimate.thread_bytes,
        "estimateProfileBytes": p.estimate.profile_bytes,
        "estimateResidentBytes": estimate.resident_bytes(),
        "estimateCheckpointBytes": estimate.checkpoint_bytes(),
        "estimateRequiredBytes": estimate.required_bytes(p.settings.solver.storage),
        "gameHeapBytes": game_heap_bytes,
        "prepareSecs": prepare_secs,
        "buildSecs": build_secs,
        "allocSecs": alloc_secs,
        "warmupIters": args.warmup,
        "warmupSecs": warmup_secs,
        "iters": args.iters,
        "iterSecs": iter_secs,
        "secsPerIter": per_iter,
        "evaluateSecs": evaluate_secs,
        "nashConv": nash_conv,
        "afterBuildBytes": after_build_mem,
        "peakBytes": peak,
    });
    if args.bands {
        report["iterationBands"] = serde_json::json!(bands);
    }
    println!("{}", serde_json::to_string(&report)?);
    if let Some(path) = &args.json {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    }
    drop(index);
    Ok(())
}

// Experiment only: this does not alter relock/resume semantics.
fn warm_experiment<S: Storage>(
    args: &Args,
    p: &prepare::Prepared,
    mut solver: Solver<hu_postflop::PostflopEvaluator, S>,
    mode: &str,
    experiment_started: Instant,
    prepare_secs: f64,
) -> Result<()> {
    use hu_engine::{NodeKind, StorageArraysMut};
    let base = args
        .experiment_base
        .as_ref()
        .context("--experiment-base is required")?;
    let sol = hu_postflop::artifact::load_sol(&base.join("solution.sol"), 0, None)?;
    let tree = &solver.game().tree;
    let old = &sol.game.tree;
    if sol.config.board != p.config.board
        || sol.config.pot != p.config.pot
        || sol.config.effective_stack != p.config.effective_stack
        || old.nodes.len() != tree.nodes.len()
        || old.nodes.iter().zip(&tree.nodes).any(|(a, b)| {
            (a.kind, a.player, a.num_children, a.first_child, a.aux)
                != (b.kind, b.player, b.num_children, b.first_child, b.aux)
        })
        || old.storage_refs.len() != tree.storage_refs.len()
        || old.deals.len() != tree.deals.len()
        || old
            .storage_refs
            .iter()
            .zip(&tree.storage_refs)
            .any(|(a, b)| {
                (a.offset, a.num_actions, a.num_hands) != (b.offset, b.num_actions, b.num_hands)
            })
        || old
            .deals
            .iter()
            .zip(&tree.deals)
            .any(|(a, b)| a.weight.to_bits() != b.weight.to_bits() || a.maps != b.maps)
    {
        bail!("base and profile trees differ; warm transfer is not supported");
    }
    let target_node = tree.nodes.iter().enumerate().find_map(|(id, node)| {
        (node.kind == NodeKind::Action
            && solver.profile().is_some_and(|profile| {
                profile.locks[node.aux as usize].is_some()
                    || profile.incentives[node.aux as usize].is_some()
            }))
        .then_some(id as u32)
    });
    match mode {
        "cold" => {}
        "checkpoint" | "reset-restart" | "reset-continue" => {
            let reader =
                hu_postflop::checkpoint::CheckpointReader::open(&base.join("checkpoint.ckpt"))?;
            let iteration = if mode == "reset-restart" {
                0
            } else {
                reader.iteration
            };
            solver.restore_stream(iteration, |storage| {
                reader.read_storage(storage)?;
                if mode != "checkpoint" {
                    match storage.arrays_mut() {
                        StorageArraysMut::F32 { strategy_sum, .. } => strategy_sum.fill(0.0),
                        _ => bail!("experiment supports f32 only"),
                    }
                }
                Ok::<_, anyhow::Error>(())
            })?;
        }
        _ if mode.starts_with("strategy-") => {
            let weight: f32 = mode["strategy-".len()..].parse()?;
            if !weight.is_finite() || weight <= 0.0 {
                bail!("strategy weight must be positive and finite");
            }
            let refs = tree.storage_refs.clone();
            let mut node_for_ref = vec![0; refs.len()];
            for (id, node) in tree.nodes.iter().enumerate() {
                if node.kind == NodeKind::Action {
                    node_for_ref[node.aux as usize] = id as u32;
                }
            }
            solver.restore_stream(0, |storage| {
                let StorageArraysMut::F32 {
                    regrets,
                    strategy_sum,
                } = storage.arrays_mut()
                else {
                    bail!("experiment supports f32 only");
                };
                for (r, node) in refs.iter().zip(node_for_ref) {
                    let strategy = sol
                        .stored_strategy(node)?
                        .context("solution must contain all streets")?;
                    for (slot, probability) in regrets[r.offset..r.offset + strategy.len()]
                        .iter_mut()
                        .zip(strategy)
                    {
                        *slot = probability * weight;
                    }
                }
                strategy_sum.fill(0.0);
                Ok::<_, anyhow::Error>(())
            })?;
        }
        _ => bail!("unknown experiment mode {mode}"),
    }
    let target = p.target.context("experiment requires a 0.1% pot target")?;
    let start_iteration = solver.iteration();
    let setup_secs = prepare_secs + experiment_started.elapsed().as_secs_f64();
    let started = Instant::now();
    let mut hits = Vec::new();
    let mut next = target;
    if let Some(points) = &args.experiment_timing
        && (points.len() != 2 || points[0] > points[1] || points[1] > args.iters)
    {
        bail!("--experiment-timing requires two ordered iteration counts within --iters");
    }
    for added in 0..=args.iters {
        if args
            .experiment_timing
            .as_ref()
            .is_some_and(|points| !points.contains(&added))
        {
            if added < args.iters {
                solver.step();
            }
            continue;
        }
        if args.experiment_timing.is_none() && hits.len() >= 2 && added % 10 != 0 {
            if added < args.iters {
                solver.step();
            }
            continue;
        }
        let (ev, expl) = solver.evaluate();
        let nash = expl[Player::P0] + expl[Player::P1];
        if args.experiment_timing.is_some() && nash / 2.0 > next {
            bail!("lab-derived target was not met at iteration {added}");
        }
        if nash / 2.0 <= next {
            let true_ev = solver.true_expected_values();
            let target_detail = target_node.map(|node| {
                use hu_engine::TerminalEvaluator;
                let game = solver.game();
                let tree_node = game.tree.node(node);
                let player = tree_node.player;
                let reach = hu_engine::reach_at(
                    &game.tree,
                    nlh::PerPlayer::new(
                        game.root_ranges[Player::P0].as_slice(),
                        game.root_ranges[Player::P1].as_slice(),
                    ),
                    node,
                    |id, _, out| out.copy_from_slice(&solver.average_strategy_at(id)),
                );
                let incentive = solver
                    .profile()
                    .and_then(|profile| profile.incentives[tree_node.aux as usize].as_ref());
                let action_ev = incentive.map(|bonus| {
                    let mut mass = vec![0.0; reach[player].len()];
                    game.evaluator.compat_mass(
                        bonus.space,
                        player,
                        &reach[player.opponent()],
                        &mut mass,
                    );
                    let mut values = Vec::new();
                    for (a, child) in game.tree.children(node).enumerate() {
                        let raw = solver.expected_values_at(
                            child,
                            player,
                            nlh::PerPlayer::new(
                                reach[Player::P0].as_slice(),
                                reach[Player::P1].as_slice(),
                            ),
                        );
                        values.push(
                            raw.iter()
                                .zip(&mass)
                                .enumerate()
                                .map(|(h, (&value, &m))| {
                                    if m > 0.0 {
                                        Some(value / m + bonus.bonus[a * mass.len() + h])
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>(),
                        );
                    }
                    values
                });
                serde_json::json!({"reach": reach[player], "actionEv": action_ev})
            });
            hits.push(serde_json::json!({
                "addedIterations": added, "iteration": solver.iteration(),
                "elapsedSecsLab": started.elapsed().as_secs_f64(),
                "totalSecs": prepare_secs + experiment_started.elapsed().as_secs_f64(),
                "nashConv": nash, "objectiveEv": [ev[Player::P0], ev[Player::P1]],
                "trueEv": [true_ev[Player::P0], true_ev[Player::P1]],
                "rootStrategy": solver.average_strategy_at(0),
                "targetNode": target_node,
                "targetStrategy": target_node.map(|node| solver.average_strategy_at(node)),
                "targetDetail": target_detail,
            }));
            if let Some(path) = &args.json {
                let partial = serde_json::json!({
                    "mode": mode, "startIteration": start_iteration, "hits": hits,
                    "maxAddedIterations": args.iters, "targetExploitability": target,
                    "setupSecs": setup_secs,
                });
                std::fs::write(path, serde_json::to_string_pretty(&partial)?)?;
            }
            if hits.len() == 4 {
                break;
            }
            next = target * [0.5, 0.2, 0.1][hits.len() - 1];
        }
        if added < args.iters {
            solver.step();
        }
    }
    let report = serde_json::json!({
        "mode": mode, "startIteration": start_iteration, "hits": hits,
        "maxAddedIterations": args.iters, "targetExploitability": target, "setupSecs": setup_secs,
    });
    println!("{}", serde_json::to_string(&report)?);
    if let Some(path) = &args.json {
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    }
    Ok(())
}

mod memory {
    #[cfg(windows)]
    fn counters() -> Option<(u64, u64)> {
        use windows_sys::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        // SAFETY: the pseudo handle is always valid and `counters` is writable
        // with its size supplied.
        let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
        (ok != 0).then_some((
            counters.WorkingSetSize as u64,
            counters.PeakWorkingSetSize as u64,
        ))
    }

    #[cfg(unix)]
    fn counters() -> Option<(u64, u64)> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let field = |name: &str| -> Option<u64> {
            let line = status.lines().find(|l| l.starts_with(name))?;
            let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
            Some(kib * 1024)
        };
        Some((field("VmRSS:")?, field("VmHWM:")?))
    }

    #[cfg(not(any(windows, unix)))]
    fn counters() -> Option<(u64, u64)> {
        None
    }

    pub fn current_bytes() -> Option<u64> {
        counters().map(|c| c.0)
    }

    pub fn peak_bytes() -> Option<u64> {
        counters().map(|c| c.1)
    }
}
