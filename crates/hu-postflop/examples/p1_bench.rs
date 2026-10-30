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
//!     [--threads N] [--warmup N] [--iters N] [--evals N] [--storage f32|i16|i16-f32avg] [--json PATH] [--bands] \
//!     [--subtree-only] [--subtree-target NASH_CONV_BB] [--hash-subtree] [--combined-base] [--checkpoint PATH]
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
    subtree_only: bool,
    subtree_target: Option<f64>,
    hash_subtree: bool,
    combined_base: bool,
    checkpoint: Option<PathBuf>,
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
        subtree_only: false,
        subtree_target: None,
        hash_subtree: false,
        combined_base: false,
        checkpoint: None,
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
            "--subtree-only" => parsed.subtree_only = true,
            "--subtree-target" => parsed.subtree_target = Some(value()?.parse()?),
            "--hash-subtree" => parsed.hash_subtree = true,
            "--combined-base" => parsed.combined_base = true,
            "--checkpoint" => parsed.checkpoint = Some(value()?.into()),
            "--bands" => parsed.bands = true,
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
    let checkpoint = args
        .checkpoint
        .as_ref()
        .map(|path| -> Result<_> {
            let state = hu_postflop::checkpoint::CheckpointReader::open(path)?;
            if state.mode == hu_postflop::checkpoint::CheckpointMode::Subtree {
                bail!(
                    "compact subtree checkpoint cannot be loaded by p1_bench; use `solvers resume`"
                );
            }
            if state.config_hash != hu_postflop::prepare::compatibility_hash(&p.effective)? {
                bail!("checkpoint config does not match benchmark config");
            }
            Ok(state)
        })
        .transpose()?;
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
            p.config.scale,
            p.config.chip_rounding,
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
    let targets = if let Some(profile) = &resolved {
        if p.document
            .spot
            .profile
            .as_ref()
            .is_some_and(|p| p.freeze_upstream.is_some())
        {
            if !profile.targets.is_empty()
                && (args.subtree_only || args.subtree_target.is_some() || args.hash_subtree)
            {
                bail!("subtree experiment does not support frequency controllers");
            }
            profile
                .summary
                .resolved_nodes
                .iter()
                .filter(|n| !matches!(n.kind, hu_postflop::profile::ResolvedNodeKind::Frozen))
                .map(|n| n.node)
                .collect::<std::collections::BTreeSet<_>>()
        } else {
            Default::default()
        }
    } else {
        Default::default()
    };
    if (args.subtree_only || args.subtree_target.is_some() || args.hash_subtree)
        && targets.is_empty()
    {
        bail!("subtree experiment requires freeze_upstream and profile targets");
    }
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
    if let Some(state) = checkpoint {
        let iteration = state.iteration;
        solver.restore_stream(iteration, |storage| state.read_storage(storage))?;
    }
    let initial_iteration = solver.iteration();
    let roots = hu_postflop::subtree::roots(&solver, &index, &targets);
    let subtree_elements: usize = roots
        .iter()
        .map(|r| {
            let span = solver.game().tree.storage_spans[r.node as usize];
            span.end - span.start
        })
        .sum();
    let alloc_secs = t.elapsed().as_secs_f64();

    let t = Instant::now();
    for _ in 0..args.warmup {
        if args.subtree_only {
            solver.step_subtrees(&roots);
        } else {
            solver.step();
        }
    }
    let warmup_secs = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let mut bands = Vec::new();
    let mut band_step_secs = [Vec::new(), Vec::new()];
    if args.bands {
        for _ in 0..args.iters {
            let step_start = Instant::now();
            if args.subtree_only {
                solver.step_subtrees(&roots);
            } else {
                solver.step();
            }
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
        for _ in 0..args.iters {
            if args.subtree_only {
                solver.step_subtrees(&roots);
            } else {
                solver.step();
            }
            if let Some(target) = args.subtree_target
                && solver.iteration().is_multiple_of(10)
            {
                let gain = solver.evaluate_subtrees(&roots);
                if gain[Player::P0] + gain[Player::P1] <= target {
                    break;
                }
            }
        }
    }
    // With --bands this wall interval includes the two sampled evaluations;
    // the per-band step times above exclude evaluation.
    let iter_secs = t.elapsed().as_secs_f64();
    let subtree_hash = if args.hash_subtree {
        let hu_engine::StorageArrays::F32 {
            regrets,
            strategy_sum,
        } = solver.storage().arrays()
        else {
            bail!("subtree hash requires f32 storage");
        };
        let mut hash = blake3::Hasher::new();
        for root in &roots {
            let span = solver.game().tree.storage_spans[root.node as usize];
            for values in [
                &regrets[span.start..span.end],
                &strategy_sum[span.start..span.end],
            ] {
                for chunk in values.chunks(1024) {
                    let mut bytes = [0u8; 4096];
                    for (value, out) in chunk.iter().zip(bytes.as_chunks_mut::<4>().0) {
                        *out = value.to_bits().to_le_bytes();
                    }
                    hash.update(&bytes[..chunk.len() * 4]);
                }
            }
        }
        Some(hash.finalize().to_hex().to_string())
    } else {
        None
    };
    if args.combined_base {
        if !args.subtree_only {
            bail!("--combined-base requires --subtree-only");
        }
        let freeze = p
            .document
            .spot
            .profile
            .as_ref()
            .and_then(|profile| profile.freeze_upstream.as_ref())
            .context("--combined-base requires freeze_upstream")?;
        let base = hu_postflop::artifact::load_sol(Path::new(&freeze.solution), 0, None)?;
        let tree = &solver.game().tree;
        let spans: Vec<_> = roots
            .iter()
            .map(|r| tree.storage_spans[r.node as usize])
            .collect();
        let outside: Vec<_> = tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == hu_engine::NodeKind::Action)
            .filter_map(|(id, n)| {
                let r = tree.storage_ref(n);
                (!spans
                    .iter()
                    .any(|s| s.sref_start <= r.index && r.index < s.sref_end))
                .then_some((id as hu_engine::NodeId, r))
            })
            .collect();
        let iteration = solver.iteration();
        solver.restore_stream(iteration, |storage| -> Result<()> {
            let hu_engine::StorageArraysMut::F32 { strategy_sum, .. } = storage.arrays_mut() else {
                bail!("--combined-base requires f32 storage");
            };
            for (id, r) in outside {
                let strategy = base
                    .stored_strategy(id)?
                    .with_context(|| format!("base lacks node {id}"))?;
                strategy_sum[r.offset..r.offset + r.len()].copy_from_slice(&strategy);
            }
            Ok(())
        })?;
    }

    let mut evaluate_secs = Vec::new();
    let mut nash_conv = f64::NAN;
    for _ in 0..args.evals {
        let t = Instant::now();
        let (_, expl) = solver.evaluate();
        evaluate_secs.push(t.elapsed().as_secs_f64());
        nash_conv = expl[Player::P0] + expl[Player::P1];
    }
    let mut subtree_evaluate_secs = Vec::new();
    let mut subtree_gain = nlh::PerPlayer::new(0.0, 0.0);
    for _ in 0..args.evals.max(1) {
        let t = Instant::now();
        subtree_gain = solver.evaluate_subtrees(&roots);
        subtree_evaluate_secs.push(t.elapsed().as_secs_f64());
    }
    let completed_iters = solver.iteration() - initial_iteration - args.warmup;
    let peak = memory::peak_bytes();
    let per_iter = if completed_iters > 0 {
        iter_secs / completed_iters as f64
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
        "iters": completed_iters,
        "iterSecs": iter_secs,
        "secsPerIter": per_iter,
        "evaluateSecs": evaluate_secs,
        "nashConv": nash_conv,
        "afterBuildBytes": after_build_mem,
        "peakBytes": peak,
    });
    report["subtreeStorageElements"] = serde_json::json!(subtree_elements);
    report["subtreeRoots"] = serde_json::json!(roots.len());
    report["subtreeOnly"] = serde_json::json!(args.subtree_only);
    report["combinedBase"] = serde_json::json!(args.combined_base);
    report["subtreeNashConv"] =
        serde_json::json!(subtree_gain[Player::P0] + subtree_gain[Player::P1]);
    report["subtreeHash"] = serde_json::json!(subtree_hash);
    report["subtreeEvaluateSecs"] = serde_json::json!(subtree_evaluate_secs);
    if args.hash_subtree {
        let values: Vec<_> = solver
            .subtree_values(&roots)
            .iter()
            .map(|v| {
                [
                    [v[Player::P0].0, v[Player::P0].1],
                    [v[Player::P1].0, v[Player::P1].1],
                ]
            })
            .collect();
        report["subtreeValues"] = serde_json::json!(values);
    }
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
