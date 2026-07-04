use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cards::{NUM_COMBOS, Player, combo_cards};
use engine::{
    DiscountSchedule, F32Storage, I16Storage, NodeId, NodeKind, ParConfig, Solver, SolverState,
    Storage, TerminalEvaluator,
};
use game::PayoffPipeline;
use holdem::{PostflopEvaluator, build_postflop_game};
use serde::Serialize;

use crate::config::{BetsSection, GameSection, RunSection, SolveConfig, StorageKind};
use crate::postflop_setup;

pub fn run(
    config_path: &Path,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
    checkpoint: Option<&Path>,
    iterations: Option<u64>,
) -> Result<()> {
    let raw_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let raw = std::str::from_utf8(&raw_bytes).context("config file is not valid UTF-8")?;
    let mut config: SolveConfig = toml::from_str(raw).context("parsing config")?;
    if let Some(it) = iterations {
        config.run.iterations = it;
    }
    // Hashed from the raw file bytes, not the parsed/overridden struct: a
    // `--iterations` override must not change what a checkpoint is stamped
    // with, since `resume` re-derives the same hash from the same file.
    let config_hash = formats::config_hash(&raw_bytes);
    let checkpoint_sink = checkpoint.map(|path| (path, config_hash));

    match config.run.storage {
        StorageKind::F32 => run_with_storage::<F32Storage>(
            config,
            output,
            histories,
            metrics,
            checkpoint_sink,
            None,
        ),
        StorageKind::I16 => run_with_storage::<I16Storage>(
            config,
            output,
            histories,
            metrics,
            checkpoint_sink,
            None,
        ),
    }
    .map(|_summary| ())
}

/// Optional side effects hooked into [`run_loop`]'s exploitability-check
/// cadence: appending a metrics row and autosaving a checkpoint. `solve`,
/// `resume`, and `bench` all go through this; `inspect` passes
/// [`RunHooks::none`] and gets the loop's convergence behavior with
/// neither.
pub(crate) struct RunHooks<'a> {
    pub metrics: Option<&'a mut formats::MetricsWriter>,
    /// Checkpoint path and the config-file hash to stamp it with.
    pub checkpoint: Option<(&'a Path, [u8; 32])>,
    /// Wall-clock reference for `MetricsRow::elapsed_secs`, taken once at
    /// the start of the (possibly resumed) solve.
    pub start: Instant,
}

impl RunHooks<'static> {
    pub fn none() -> Self {
        RunHooks {
            metrics: None,
            checkpoint: None,
            start: Instant::now(),
        }
    }
}

/// Solves (or resumes) `config` with storage backend `S`, dispatching on
/// the game kind. Shared by `solve`, `resume`, and `bench` so there is
/// exactly one convergence loop and one export path in the codebase.
pub(crate) fn run_with_storage<S: Storage>(
    config: SolveConfig,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
    checkpoint: Option<(&Path, [u8; 32])>,
    resume_state: Option<SolverState>,
) -> Result<RunSummary> {
    let rake = postflop_setup::build_rake(&config.rake);
    let utility = postflop_setup::build_utility(&config.utility);
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };

    let schedule = postflop_setup::build_schedule(&config.algorithm);
    let schedule_name = schedule.name();

    let mut metrics_writer = metrics
        .map(formats::MetricsWriter::create_or_append)
        .transpose()?;
    let mut hooks = RunHooks {
        metrics: metrics_writer.as_mut(),
        checkpoint,
        start: Instant::now(),
    };

    match config.game {
        GameSection::Kuhn => solve_toy::<S>(
            "kuhn",
            game::kuhn(pipeline),
            schedule,
            schedule_name,
            &config.run,
            output,
            resume_state,
            &mut hooks,
        ),
        GameSection::Leduc => solve_toy::<S>(
            "leduc",
            game::leduc(pipeline),
            schedule,
            schedule_name,
            &config.run,
            output,
            resume_state,
            &mut hooks,
        ),
        GameSection::Postflop {
            board,
            oop_range,
            ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
        } => solve_postflop::<S>(
            pipeline,
            &board,
            &oop_range,
            &ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
            schedule,
            schedule_name,
            &config.run,
            output,
            histories,
            resume_state,
            &mut hooks,
        ),
    }
}

/// Convergence loop shared by every game: run a chunk of iterations, report
/// exploitability, and stop early once `target_nash_conv` is hit. Generic
/// over both the terminal evaluator and the storage backend so toy games,
/// postflop subgames, and both storage backends all reuse it unchanged.
///
/// `run.iterations` is the TOTAL iteration target, not a delta: computing
/// `remaining` from `run.iterations - solver.iteration()` (instead of
/// assuming a fresh solver at iteration 0) is what lets `resume` reuse this
/// function unmodified after restoring a checkpoint mid-run.
pub(crate) fn run_loop<E: TerminalEvaluator, S: Storage>(
    solver: &mut Solver<E, S>,
    run: &RunSection,
    hooks: &mut RunHooks<'_>,
) -> Result<()> {
    let mut remaining = run.iterations.saturating_sub(solver.iteration());
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

        // Computed before touching `hooks.metrics` so the two field
        // borrows below never overlap.
        let elapsed_secs = hooks.start.elapsed().as_secs_f64();
        if let Some(writer) = hooks.metrics.as_deref_mut() {
            writer.append(&formats::MetricsRow {
                iteration: solver.iteration(),
                elapsed_secs,
                expl_p0: expl[Player::P0],
                expl_p1: expl[Player::P1],
                nash_conv,
            })?;
        }
        checkpoint_now(solver, hooks)?;

        if let Some(target) = run.target_nash_conv
            && nash_conv < target
        {
            println!("target nash_conv {target:.3e} reached");
            break;
        }
    }
    Ok(())
}

/// Writes a checkpoint right now if `hooks` configures one — used both at
/// every `run_loop` exploitability check and once more after the loop
/// finishes, so the final solver state is always saved even when the run
/// converges (or hits its iteration cap) between two `check_every` marks.
fn checkpoint_now<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    hooks: &RunHooks<'_>,
) -> Result<()> {
    if let Some((path, hash)) = hooks.checkpoint {
        formats::write_checkpoint(path, hash, &solver.state())?;
    }
    Ok(())
}

/// Final convergence numbers, both printed by [`print_done`] and returned
/// so callers that need them programmatically (`bench`'s comparison table)
/// don't have to scrape stdout or recompute exploitability.
pub(crate) struct RunSummary {
    pub iterations: u64,
    pub wall: Duration,
    pub expl_p0: f64,
    pub expl_p1: f64,
    pub nash_conv: f64,
}

/// Final convergence summary, shared by every game.
pub(crate) fn print_done<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
) -> RunSummary {
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
    RunSummary {
        iterations: solver.iteration(),
        wall: elapsed,
        expl_p0: expl[Player::P0],
        expl_p1: expl[Player::P1],
        nash_conv,
    }
}

#[allow(clippy::too_many_arguments)]
fn solve_toy<S: Storage>(
    kind: &str,
    toy: game::ToyGame,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
    resume_state: Option<SolverState>,
    hooks: &mut RunHooks<'_>,
) -> Result<RunSummary> {
    let node_info = toy.node_info.clone();
    let mut solver = Solver::<_, S>::new(toy.game, schedule, Some(run.iterations));
    if let Some(state) = resume_state {
        solver.restore_state(state).context(
            "restoring checkpoint state (does its storage backend match `run.storage`?)",
        )?;
    }

    println!(
        "game={} schedule={} iterations={}",
        kind, schedule_name, run.iterations
    );
    let start = Instant::now();
    run_loop(&mut solver, run, hooks)?;
    let elapsed = start.elapsed();
    let summary = print_done(&solver, elapsed);
    checkpoint_now(&solver, hooks)?;

    if let Some(path) = output {
        let report = export_toy(kind, &node_info, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }
    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn solve_postflop<S: Storage>(
    pipeline: PayoffPipeline<'_>,
    board: &str,
    oop_range: &str,
    ip_range: &str,
    pot: u32,
    effective_stack: u32,
    iso_merging: bool,
    bets: BetsSection,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
    histories: &[String],
    resume_state: Option<SolverState>,
    hooks: &mut RunHooks<'_>,
) -> Result<RunSummary> {
    let config = postflop_setup::build_postflop_config(
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        iso_merging,
        bets,
    )?;

    // Cheap dry run before committing to the (possibly very large) real
    // build, so an oversized config fails fast with a size estimate instead
    // of silently eating memory.
    let estimate = holdem::memory_usage(&config);
    postflop_setup::print_memory_estimate(estimate);

    let pf_game = build_postflop_game(&config, pipeline);

    // Resolve the requested export histories to node ids (and their
    // player/action labels) while the built tree and node_info are still
    // both in hand; `pf_game.game` moves into the solver right after.
    let mut resolved = Vec::new();
    for history in histories {
        match pf_game.node_by_history(history) {
            Some(node_id) => {
                let tag = pf_game.game.tree.tags[node_id as usize] as usize;
                let info = &pf_game.node_info[tag];
                let player = pf_game.game.tree.node(node_id).player.index();
                resolved.push(ResolvedHistory {
                    history: history.clone(),
                    node_id,
                    player,
                    actions: info.actions.clone(),
                });
            }
            None => {
                eprintln!("warning: unknown history {history:?}, skipping");
            }
        }
    }

    if let Some(n) = run.threads {
        // Ignore "already initialized": tests and repeated calls within one
        // process may have set the global pool already.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    let mut solver = Solver::<_, S>::new(pf_game.game, schedule, Some(run.iterations));
    solver.set_par(ParConfig {
        chance_depth: run.par_chance_depth.unwrap_or(2),
        min_children: run.par_min_children.unwrap_or(12),
    });
    if let Some(state) = resume_state {
        solver.restore_state(state).context(
            "restoring checkpoint state (does its storage backend match `run.storage`?)",
        )?;
    }

    println!(
        "game=postflop schedule={} iterations={}",
        schedule_name, run.iterations
    );
    let start = Instant::now();
    run_loop(&mut solver, run, hooks)?;
    let elapsed = start.elapsed();
    let summary = print_done(&solver, elapsed);
    checkpoint_now(&solver, hooks)?;

    if let Some(path) = output {
        let report = export_postflop(&resolved, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }
    Ok(summary)
}

/// A requested `--history` resolved against the built tree, before the tree
/// moves into the solver.
struct ResolvedHistory {
    history: String,
    node_id: NodeId,
    player: usize,
    actions: Vec<String>,
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

fn export_toy<S: Storage>(
    game_kind: &str,
    node_info: &[game::ToyNodeInfo],
    solver: &Solver<game::ToyEvaluator, S>,
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

#[derive(Serialize)]
struct PostflopReport {
    game: String,
    iterations: u64,
    expected_value_p0: f64,
    exploitability: [f64; 2],
    entries: Vec<HistoryEntry>,
}

#[derive(Serialize)]
struct HistoryEntry {
    history: String,
    player: usize,
    actions: Vec<String>,
    /// One row per combo with non-zero range weight for the acting player:
    /// `[combo_index, "AhKs", [action probabilities...]]`.
    strategy: Vec<(usize, String, Vec<f32>)>,
}

fn export_postflop<S: Storage>(
    entries: &[ResolvedHistory],
    solver: &Solver<PostflopEvaluator, S>,
) -> PostflopReport {
    let tree = &solver.game().tree;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let node = tree.node(entry.node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        debug_assert_eq!(num_hands, NUM_COMBOS);
        let sigma = solver.average_strategy_at(entry.node_id);
        // The game's own root ranges, not the raw user-specified `Range`:
        // these are already zeroed for combos that conflict with the
        // starting board, so board-blocked hands (e.g. a pocket pair whose
        // rank sits on the board) never leak into the export with
        // meaningless untouched storage values.
        let range = &solver.game().root_ranges[Player::from_index(entry.player)];

        let mut strategy = Vec::new();
        for combo in 0..num_hands {
            if range[combo] <= 0.0 {
                continue;
            }
            let (hi, lo) = combo_cards(combo);
            let probs: Vec<f32> = (0..num_actions)
                .map(|a| sigma[a * num_hands + combo])
                .collect();
            strategy.push((combo, format!("{hi}{lo}"), probs));
        }
        out.push(HistoryEntry {
            history: entry.history.clone(),
            player: entry.player,
            actions: entry.actions.clone(),
            strategy,
        });
    }
    let expl = solver.exploitability();
    PostflopReport {
        game: "postflop".to_string(),
        iterations: solver.iteration(),
        expected_value_p0: solver.expected_value(Player::P0),
        exploitability: [expl[Player::P0], expl[Player::P1]],
        entries: out,
    }
}
