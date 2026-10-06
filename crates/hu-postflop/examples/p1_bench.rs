//! P1 throughput harness: times tree build, CFR iterations and the
//! exploitability evaluation separately for one `solvers.nlh/v1` P1 config,
//! and reports the process peak memory. One thread count per process, so
//! every measurement starts from a fresh heap.
//!
//! ```text
//! cargo run --release -p hu-postflop --example p1_bench -- CONFIG \
//!     [--threads N] [--warmup N] [--iters N] [--evals N] [--storage f32|i16] [--json PATH] [--bands]
//! ```

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use hu_engine::{F32Storage, I16Storage, ParConfig, Solver, Storage};
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
                    other => bail!("unknown storage {other}"),
                })
            }
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
    let mut prepared = prepare::prepare(&raw, &args.config)?;
    if let Some(storage) = args.storage {
        prepared.settings.solver.storage = storage;
    }
    let prepare_secs = t0.elapsed().as_secs_f64();
    // Zero-work measurement must not allocate a potentially tens-of-GB arena.
    if args.iters == 0 && args.evals == 0 {
        let report = serde_json::json!({
            "config": args.config.display().to_string(),
            "threads": args.threads,
            "estimateF32Bytes": prepared.estimate.f32_bytes,
            "estimateI16Bytes": prepared.estimate.i16_bytes,
            "storageElements": prepared.estimate.f32_bytes / 8,
            "nodes": prepared.estimate.nodes,
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
    })
}

fn bench<S: Storage>(
    args: &Args,
    p: &prepare::Prepared,
    prepare_secs: f64,
    storage_name: &str,
) -> Result<()> {
    let t = Instant::now();
    let game = hu_postflop::try_build_postflop_game(&p.config, p.payoff.pipeline())?;
    let build_secs = t.elapsed().as_secs_f64();
    let tree = &game.game.tree;
    let nodes = tree.nodes.len();
    let action_nodes = tree.storage_refs.len();
    let storage_elements = tree.storage_len;
    let deals = tree.deals.len();
    let transitions = tree.transitions.len();
    let transition_entries: usize = tree.transitions.iter().map(|t| t.entries.len()).sum();
    let node_info = game.node_info.len();
    let after_build_mem = memory::current_bytes();

    let t = Instant::now();
    let mut solver = Solver::<_, S>::new(
        game.game,
        hu_postflop::run::schedule(&p.settings.solver.algorithm),
        Some(p.settings.solver.stop.max_iterations),
    );
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    let alloc_secs = t.elapsed().as_secs_f64();

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
                    "evalSecs": eval_start.elapsed().as_secs_f64(),
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

    let mut eval_secs = Vec::new();
    let mut nash_conv = f64::NAN;
    for _ in 0..args.evals {
        let t = Instant::now();
        let expl = solver.exploitability();
        eval_secs.push(t.elapsed().as_secs_f64());
        nash_conv = expl[Player::P0] + expl[Player::P1];
    }
    let peak = memory::peak_bytes();
    let per_iter = if args.iters > 0 {
        iter_secs / args.iters as f64
    } else {
        f64::NAN
    };
    let mut report = serde_json::json!({
        "config": args.config.display().to_string(),
        "threads": args.threads,
        "storage": storage_name,
        "nodes": nodes,
        "actionNodes": action_nodes,
        "storageElements": storage_elements,
        "deals": deals,
        "transitions": transitions,
        "transitionEntries": transition_entries,
        "nodeInfo": node_info,
        "estimateF32Bytes": p.estimate.f32_bytes,
        "estimateI16Bytes": p.estimate.i16_bytes,
        "prepareSecs": prepare_secs,
        "buildSecs": build_secs,
        "allocSecs": alloc_secs,
        "warmupIters": args.warmup,
        "warmupSecs": warmup_secs,
        "iters": args.iters,
        "iterSecs": iter_secs,
        "secsPerIter": per_iter,
        "evalSecs": eval_secs,
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
