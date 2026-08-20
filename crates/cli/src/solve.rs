use std::path::Path;
use std::time::{Duration, Instant};

use abstraction::Ehs2Params;
use anyhow::{Context, Result, anyhow};
use cards::{NUM_CLASSES, NUM_COMBOS, PerPlayer, Player, combo_cards};
use engine::{
    DiscountSchedule, F32Storage, I16Storage, NodeId, NodeKind, ParConfig, Solver, SolverState,
    Storage, TerminalEvaluator,
};
use game::PayoffPipeline;
use holdem::{PostflopEvaluator, build_postflop_game};
use preflop::{
    EquityShowdown, PostflopBets, PreflopConfig, blueprint_memory_usage, build_blueprint_game,
    build_preflop_game,
};
use serde::Serialize;

use crate::config::{
    BetsSection, GameSection, PostflopSection, RunSection, SolveConfig, StorageKind,
};
use crate::postflop_setup;
use crate::preflop_setup;
use crate::sol::{SolExportSpec, SolStreets};

/// Solves `config_path` into the run directory `out`.
///
/// There is one output path: a solve produces a run directory or it produces
/// nothing. That is what lets `status`, `watch`, and the future job daemon
/// treat every run the same way, whichever engine ran it.
pub fn run(
    config_path: &Path,
    out: &Path,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    histories: &[String],
    sol_streets: SolStreets,
) -> Result<()> {
    let raw_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let source_raw = std::str::from_utf8(&raw_bytes).context("config file is not valid UTF-8")?;
    let is_multiway_v1 = crate::multiway_v1::has_v1_schema(source_raw)?;
    let effective_raw = if is_multiway_v1 {
        crate::multiway_v1::apply_solve_overrides(
            source_raw,
            threads,
            memory,
            max_time,
            Some(config_path),
        )?
    } else {
        if threads.is_some() || memory.is_some() || max_time.is_some() {
            return Err(anyhow!(
                "--threads, --memory, and --max-time are Multiway Preflop v1 overrides"
            ));
        }
        source_raw.to_owned()
    };
    let raw = effective_raw.as_str();
    if is_multiway_v1 && histories.iter().any(|history| !history.is_empty()) {
        return Err(anyhow!(
            "Multiway Preflop v1 publishes strategy through its solution artifact, \
             not --history"
        ));
    }
    crate::run_dir::create_empty(out)?;
    let config: SolveConfig =
        crate::config::parse_solve_config_at(raw, config_path).context("parsing config")?;
    let run_paths = if is_multiway_v1 {
        crate::run_dir::RunPaths::multiway(out)
    } else {
        crate::run_dir::RunPaths::heads_up(out)
    };

    // The run directory supplies every artifact path. The two engines differ
    // only in which file each artifact lands in: the multiway path reports
    // its summary as `run.json` and its strategy inside `solution.mwsol`,
    // while the heads-up path writes a separate `strategy.json` and only
    // has a `.sol` viewer artifact for postflop games.
    let paths = &run_paths;
    let (output, metrics, checkpoint, sol) = if is_multiway_v1 {
        (
            Some(paths.result.as_path()),
            Some(paths.progress.as_path()),
            Some(paths.checkpoint.as_path()),
            Some(paths.solution.as_path()),
        )
    } else {
        (
            Some(paths.strategy.as_path()),
            Some(paths.progress.as_path()),
            Some(paths.checkpoint.as_path()),
            matches!(config.game, GameSection::Postflop { .. }).then(|| paths.solution.as_path()),
        )
    };

    if !is_multiway_v1 && matches!(config.game, GameSection::PreflopMultiway(_)) {
        return Err(anyhow!(
            "MWP003: hand-written lowered preflop-multiway configs are not accepted; \
             write schema = {:?} instead",
            crate::multiway_v1::SCHEMA
        ));
    }
    // Hashed from the raw file bytes rather than the parsed struct, so
    // `resume` re-derives the same stamp from the run directory's copy.
    let config_hash = formats::config_hash(raw.as_bytes());
    if matches!(config.game, GameSection::PreflopMultiway(_)) {
        let mut recorder = crate::run_dir::RunRecorder::start(
            &paths.directory,
            "preflop-multiway",
            Some(crate::multiway_v1::SCHEMA.to_string()),
            config_hash,
            raw,
            vec![
                "solve".to_string(),
                config_path.display().to_string(),
                "--out".to_string(),
                paths.directory.display().to_string(),
            ],
        )?;
        let outcome = crate::multiway_solve::run_observed(
            raw,
            config,
            output,
            metrics,
            checkpoint,
            config_hash,
            sol,
            Some(&crate::CLI_CANCEL),
            true,
            &mut |observation| recorder.observe(&observation),
        );
        let completion = crate::run_dir::completion_status(&paths.directory);
        return recorder.finish(outcome, completion);
    }

    let checkpoint_sink = checkpoint.map(|path| (path, config_hash));

    let sol_spec = match sol {
        Some(path) => {
            match &config.game {
                GameSection::Postflop { .. } => {}
                GameSection::Preflop { .. } => {
                    return Err(anyhow!(
                        "--sol export is not supported for preflop configs yet"
                    ));
                }
                _ => {
                    return Err(anyhow!(
                        "--sol export only supports kind = \"postflop\" configs \
                         (kuhn/leduc have no board/street structure to quantize)"
                    ));
                }
            }
            let storage_name = match config.run.storage {
                StorageKind::F32 => "f32",
                StorageKind::I16 => "i16",
            };
            Some(SolExportSpec {
                path: path.to_path_buf(),
                mode: sol_streets,
                config_toml: raw.to_string(),
                storage_name: storage_name.to_string(),
            })
        }
        None => None,
    };

    let game_kind = game_kind_name(&config.game);
    let mut recorder = crate::run_dir::RunRecorder::start(
        &paths.directory,
        game_kind,
        config.schema.clone(),
        config_hash,
        raw,
        vec![
            "solve".to_string(),
            config_path.display().to_string(),
            "--out".to_string(),
            paths.directory.display().to_string(),
        ],
    )?;
    let outcome = solve_heads_up(
        config,
        output,
        histories,
        metrics,
        checkpoint_sink,
        sol_spec,
        Some(recorder.events_mut()),
        Some(&crate::CLI_CANCEL),
    );
    // A heads-up run has no solver-reported completion status: it either
    // reached its iteration budget or its `target_nash_conv`. Record the
    // convergence summary as `run.json` so `status` reports the same shape
    // a multiway run does.
    let completion = match &outcome {
        Ok(summary) => {
            let recorded = serde_json::json!({
                "kind": game_kind,
                "iterations": summary.iterations,
                "wallSecs": summary.wall.as_secs_f64(),
                "explP0": summary.expl_p0,
                "explP1": summary.expl_p1,
                "nashConv": summary.nash_conv,
            });
            std::fs::write(
                &paths.result,
                format!("{}\n", serde_json::to_string_pretty(&recorded)?),
            )
            .with_context(|| format!("writing {}", paths.result.display()))?;
            Some(
                if summary.canceled {
                    "cancelled"
                } else {
                    "completed"
                }
                .to_string(),
            )
        }
        Err(_) => None,
    };
    recorder.finish(outcome.map(|_summary| ()), completion)
}

/// Names the game for a run manifest. These match the config's `kind`.
pub(crate) fn game_kind_name(game: &GameSection) -> &'static str {
    match game {
        GameSection::Kuhn => "kuhn",
        GameSection::Leduc => "leduc",
        GameSection::Postflop { .. } => "postflop",
        GameSection::Preflop { .. } => "preflop",
        GameSection::PreflopMultiway(_) => "preflop-multiway",
    }
}

#[allow(clippy::too_many_arguments)]
fn solve_heads_up(
    config: SolveConfig,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
    checkpoint_sink: Option<(&Path, [u8; 32])>,
    sol_spec: Option<SolExportSpec>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RunSummary> {
    match config.run.storage {
        StorageKind::F32 => run_with_storage_sol::<F32Storage>(
            config,
            output,
            histories,
            metrics,
            checkpoint_sink,
            None,
            sol_spec,
            events,
            cancel,
        ),
        StorageKind::I16 => run_with_storage_sol::<I16Storage>(
            config,
            output,
            histories,
            metrics,
            checkpoint_sink,
            None,
            sol_spec,
            events,
            cancel,
        ),
    }
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
    /// Run event log, when the solve owns a run directory. Checkpoint
    /// events go here so `watch` shows the same lifecycle for a heads-up
    /// run as for a multiway one.
    pub events: Option<&'a mut formats::RunEventLog>,
    /// Cooperative cancel flag. Checked once per exploitability check, so
    /// Ctrl-C stops at a checkpoint boundary and leaves the run resumable
    /// rather than killing it mid-iteration.
    pub cancel: Option<&'a std::sync::atomic::AtomicBool>,
    /// Set when [`Self::cancel`] fired, so the caller records the run as
    /// canceled rather than completed.
    pub canceled: bool,
    /// Wall-clock reference for `MetricsRow::elapsed_secs`, taken once at
    /// the start of the (possibly resumed) solve.
    pub start: Instant,
}

impl RunHooks<'static> {
    pub fn none() -> Self {
        RunHooks {
            metrics: None,
            checkpoint: None,
            events: None,
            cancel: None,
            canceled: false,
            start: Instant::now(),
        }
    }
}

/// Solves (or resumes) `config` with storage backend `S`, dispatching on
/// the game kind. Shared by `solve`, `resume`, and `bench` so there is
/// exactly one convergence loop and one export path in the codebase.
///
/// Never exports a `.sol` artifact -- see [`run_with_storage_sol`], the
/// entry point `solve::run` uses instead, which does. Keeping this
/// signature unchanged means `resume` and `bench` (which never export
/// `.sol` files) don't need to touch their call sites for the `.sol`
/// feature at all.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_with_storage<S: Storage>(
    config: SolveConfig,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
    checkpoint: Option<(&Path, [u8; 32])>,
    resume_state: Option<SolverState>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RunSummary> {
    run_with_storage_impl::<S>(
        config,
        output,
        histories,
        metrics,
        checkpoint,
        resume_state,
        None,
        events,
        cancel,
    )
}

/// Same as [`run_with_storage`], but additionally exports a `.sol` viewer
/// artifact once the run completes (postflop configs only) when `sol` is
/// `Some`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_with_storage_sol<S: Storage>(
    config: SolveConfig,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
    checkpoint: Option<(&Path, [u8; 32])>,
    resume_state: Option<SolverState>,
    sol: Option<SolExportSpec>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RunSummary> {
    run_with_storage_impl::<S>(
        config,
        output,
        histories,
        metrics,
        checkpoint,
        resume_state,
        sol,
        events,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_with_storage_impl<S: Storage>(
    config: SolveConfig,
    output: Option<&Path>,
    histories: &[String],
    metrics: Option<&Path>,
    checkpoint: Option<(&Path, [u8; 32])>,
    resume_state: Option<SolverState>,
    sol: Option<SolExportSpec>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
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
        events,
        cancel,
        canceled: false,
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
            sol,
            &mut hooks,
        ),
        GameSection::Preflop {
            effective_stack_bb,
            sb_bb,
            open_sizes_bb,
            raise_factors,
            max_raises,
            include_allin,
            allow_limp,
            sb_range,
            bb_range,
            equity_realization,
            equity_cache,
            postflop,
        } => solve_preflop::<S>(
            pipeline,
            effective_stack_bb,
            sb_bb,
            open_sizes_bb,
            raise_factors,
            max_raises,
            include_allin,
            allow_limp,
            sb_range,
            bb_range,
            equity_realization,
            equity_cache,
            postflop,
            schedule,
            schedule_name,
            &config.run,
            output,
            histories,
            resume_state,
            &mut hooks,
        ),
        GameSection::PreflopMultiway(_) => {
            unreachable!("multiway games dispatch before the HU storage path")
        }
    }
    .map(|summary| RunSummary {
        canceled: hooks.canceled,
        ..summary
    })
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

        if hooks
            .cancel
            .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::SeqCst))
        {
            println!("cancelled at iteration {}", solver.iteration());
            hooks.canceled = true;
            if let Some(events) = hooks.events.as_deref_mut() {
                let _ = events.info(formats::RunEventPayload::Stop {
                    reason: "cancelled".to_string(),
                });
            }
            break;
        }

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
    hooks: &mut RunHooks<'_>,
) -> Result<()> {
    let Some((path, hash)) = hooks.checkpoint else {
        return Ok(());
    };
    formats::write_checkpoint(path, hash, &solver.state())?;
    if let Some(events) = hooks.events.as_deref_mut() {
        let _ = events.info(formats::RunEventPayload::Checkpoint {
            sweeps: solver.iteration(),
        });
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
    /// The run stopped on request at a checkpoint boundary, not because it
    /// reached its budget or its convergence target.
    pub canceled: bool,
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
        canceled: false,
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
    sol: Option<SolExportSpec>,
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

    if let Some(spec) = &sol {
        let start_street = crate::sol::start_street_from_board_len(config.board.len());
        crate::sol::export_sol(spec, &solver, start_street, &summary)?;
    }

    Ok(summary)
}

/// Builds the 169-class preflop trunk config and dispatches on whether
/// `[game.postflop]` was configured: `None` keeps today's equity-showdown
/// continuation model unchanged; `Some` extends the trunk into a bucketed
/// blueprint postflop model (`solve_preflop_bucketed`).
#[allow(clippy::too_many_arguments)]
fn solve_preflop<S: Storage>(
    pipeline: PayoffPipeline<'_>,
    effective_stack_bb: f64,
    sb_bb: f64,
    open_sizes_bb: Vec<f64>,
    raise_factors: Vec<Vec<f64>>,
    max_raises: u32,
    include_allin: bool,
    allow_limp: bool,
    sb_range: Option<String>,
    bb_range: Option<String>,
    equity_realization: [f64; 2],
    equity_cache: Option<std::path::PathBuf>,
    postflop: Option<PostflopSection>,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
    histories: &[String],
    resume_state: Option<SolverState>,
    hooks: &mut RunHooks<'_>,
) -> Result<RunSummary> {
    let config = preflop_setup::build_preflop_config(
        effective_stack_bb,
        sb_bb,
        open_sizes_bb,
        raise_factors,
        max_raises,
        include_allin,
        allow_limp,
        sb_range.as_deref(),
        bb_range.as_deref(),
    )?;

    // Cheap dry run before committing to the (possibly large) real build.
    // This is the 169-class trunk's own size; the bucketed path prints a
    // second, full-tree estimate once its postflop bets/artifacts are known.
    let estimate = preflop::memory_usage(&config);
    preflop_setup::print_memory_estimate(estimate);

    match postflop {
        None => solve_preflop_showdown::<S>(
            pipeline,
            config,
            equity_realization,
            equity_cache.as_deref(),
            schedule,
            schedule_name,
            run,
            output,
            histories,
            resume_state,
            hooks,
        ),
        Some(section) => solve_preflop_bucketed::<S>(
            pipeline,
            config,
            section,
            equity_cache.as_deref(),
            schedule,
            schedule_name,
            run,
            output,
            histories,
            resume_state,
            hooks,
        ),
    }
}

/// The original (slice-1) preflop path: continuations resolve via the
/// equity-showdown model on the 169-class trunk directly, no postflop
/// betting tree at all.
#[allow(clippy::too_many_arguments)]
fn solve_preflop_showdown<S: Storage>(
    pipeline: PayoffPipeline<'_>,
    config: PreflopConfig,
    equity_realization: [f64; 2],
    equity_cache: Option<&Path>,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
    histories: &[String],
    resume_state: Option<SolverState>,
    hooks: &mut RunHooks<'_>,
) -> Result<RunSummary> {
    let table = preflop_setup::load_or_compute_equity_table(equity_cache);
    let model = EquityShowdown {
        realization: PerPlayer::new(equity_realization[0], equity_realization[1]),
    };
    let pf_game = build_preflop_game(&config, &table, &model, pipeline);

    // Resolve the requested export histories to node ids (and root's own
    // action labels for the summary line below) while the built tree and
    // node_info are still both in hand; `pf_game.game` moves into the
    // solver right after.
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
    let root_tag = pf_game.game.tree.tags[0] as usize;
    let root_actions = pf_game.node_info[root_tag].actions.clone();

    if let Some(n) = run.threads {
        // Ignore "already initialized": tests and repeated calls within one
        // process may have set the global pool already.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    let mut solver = Solver::<_, S>::new(pf_game.game, schedule, Some(run.iterations));
    // Preflop trunks have no chance nodes, so `ParConfig` is inert here --
    // set unconditionally anyway to keep this code path uniform with
    // `solve_postflop`.
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
        "game=preflop schedule={} iterations={}",
        schedule_name, run.iterations
    );
    let start = Instant::now();
    run_loop(&mut solver, run, hooks)?;
    let elapsed = start.elapsed();
    let summary = print_done(&solver, elapsed);
    checkpoint_now(&solver, hooks)?;

    print_preflop_root_summary(&solver, &root_actions);

    if let Some(path) = output {
        let report = export_preflop(&resolved, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }

    Ok(summary)
}

/// The bucketed blueprint path (`[game.postflop]` present): extends the
/// 169-class trunk with an EHS² bucket abstraction's postflop betting,
/// solved through the same `run_loop`/checkpoint/metrics/history/output
/// machinery as every other game.
#[allow(clippy::too_many_arguments)]
fn solve_preflop_bucketed<S: Storage>(
    pipeline: PayoffPipeline<'_>,
    config: PreflopConfig,
    section: PostflopSection,
    equity_cache: Option<&Path>,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
    histories: &[String],
    resume_state: Option<SolverState>,
    hooks: &mut RunHooks<'_>,
) -> Result<RunSummary> {
    if section.model != "bucketed" {
        return Err(anyhow!(
            "game.postflop.model = {:?} is not supported (the only implemented value is \"bucketed\")",
            section.model
        ));
    }

    println!(
        "postflop: model=bucketed buckets(flop/turn/river)={}/{}/{} \
         bets(flop/turn/river)={:?}/{:?}/{:?} max_raises={} include_allin={}",
        section.flop_buckets,
        section.turn_buckets,
        section.river_buckets,
        section.bets_flop,
        section.bets_turn,
        section.bets_river,
        section.max_raises,
        section.include_allin,
    );
    println!(
        "WARNING: a cold EHS2 abstraction + blueprint artifact build takes on the order of \
         10 minutes in release mode; abstraction-cache/artifacts-cache make reruns instant."
    );

    let table = preflop_setup::load_or_compute_equity_table(equity_cache);

    let abs_params = Ehs2Params {
        flop_buckets: section.flop_buckets,
        turn_buckets: section.turn_buckets,
        river_buckets: section.river_buckets,
    };
    let abs =
        preflop_setup::load_or_build_abstraction(abs_params, section.abstraction_cache.as_deref());
    let artifacts =
        preflop_setup::load_or_build_artifacts(&abs, section.artifacts_cache.as_deref());

    let bets = PostflopBets {
        flop: PerPlayer::new(section.bets_flop.clone(), section.bets_flop),
        turn: PerPlayer::new(section.bets_turn.clone(), section.bets_turn),
        river: PerPlayer::new(section.bets_river.clone(), section.bets_river),
        max_raises: section.max_raises,
        include_allin: section.include_allin,
    };

    // Cheap dry run over the full (trunk + postflop) tree before committing
    // to the real build.
    let estimate = blueprint_memory_usage(&config, &bets, &artifacts);
    preflop_setup::print_memory_estimate(estimate);

    let bp_game = build_blueprint_game(&config, &bets, &table, &artifacts, pipeline);

    // Resolve the requested export histories to node ids -- `--history`
    // strings work across `/` postflop street boundaries automatically,
    // since `node_by_history` just matches the full recorded history
    // string regardless of which street it ends on.
    let mut resolved = Vec::new();
    for history in histories {
        match bp_game.node_by_history(history) {
            Some(node_id) => {
                let tag = bp_game.game.tree.tags[node_id as usize] as usize;
                let info = &bp_game.node_info[tag];
                let player = bp_game.game.tree.node(node_id).player.index();
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
    let root_tag = bp_game.game.tree.tags[0] as usize;
    let root_actions = bp_game.node_info[root_tag].actions.clone();

    if let Some(n) = run.threads {
        // Ignore "already initialized": tests and repeated calls within one
        // process may have set the global pool already.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    let mut solver = Solver::<_, S>::new(bp_game.game, schedule, Some(run.iterations));
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
        "game=preflop-bucketed schedule={} iterations={}",
        schedule_name, run.iterations
    );
    let start = Instant::now();
    run_loop(&mut solver, run, hooks)?;
    let elapsed = start.elapsed();
    let summary = print_done(&solver, elapsed);
    checkpoint_now(&solver, hooks)?;

    print_preflop_root_summary(&solver, &root_actions);

    if let Some(path) = output {
        let report = export_preflop(&resolved, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }

    Ok(summary)
}

/// Compact root summary: for each root action, the range-mass-weighted
/// aggregate frequency over the SB's 169 classes, e.g. `root: Fold 12.3% |
/// All-in 87.7%`.
///
/// Generic over `E` (not just `PreflopEvaluator`) so the bucketed blueprint
/// game (`BlueprintEvaluator`) reuses it unchanged: the root of a blueprint
/// game is still the 169-class trunk root regardless of what its
/// continuations look like postflop.
fn print_preflop_root_summary<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    root_actions: &[String],
) {
    let tree = &solver.game().tree;
    let node = tree.node(0);
    let sref = tree.storage_ref(node);
    let avg = solver.average_strategy_at(0);
    let root_range = &solver.game().root_ranges[Player::P0];
    let freqs = postflop_setup::action_frequencies(
        &avg,
        root_range,
        sref.num_actions as usize,
        sref.num_hands as usize,
    );
    let parts: Vec<String> = root_actions
        .iter()
        .zip(freqs.iter())
        .map(|(label, freq)| format!("{label} {:.1}%", freq * 100.0))
        .collect();
    println!("root: {}", parts.join(" | "));
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

#[derive(Serialize)]
struct PreflopReport {
    game: String,
    iterations: u64,
    expected_value_p0: f64,
    exploitability: [f64; 2],
    /// The 169 class labels ("AA", "AKs", "AKo", ...), in class-index order,
    /// so notebooks can index every entry's strategy rows without depending
    /// on `preflop::class_label` themselves.
    class_labels: Vec<String>,
    entries: Vec<PreflopHistoryEntry>,
}

#[derive(Serialize)]
struct PreflopHistoryEntry {
    history: String,
    player: usize,
    actions: Vec<String>,
    /// Action-major: `strategy[a][h]` is hand `h`'s probability of action
    /// `a`, one row per action (indexed like `actions`). `h` ranges over the
    /// 169 preflop classes for trunk histories, or the acting street's
    /// bucket count for a bucketed-postflop history (`--history` strings
    /// cross `/` street boundaries transparently, see `node_by_history`).
    strategy: Vec<Vec<f32>>,
}

/// Generic over `E` so both the equity-showdown trunk (`PreflopEvaluator`,
/// every node is 169-class) and the bucketed blueprint game
/// (`BlueprintEvaluator`, postflop nodes are bucket-dimensioned) share this
/// export -- `BlueprintGame` exposes the same node_info/history shape as
/// `PreflopGame`, so `ResolvedHistory` needs no changes either.
fn export_preflop<E: TerminalEvaluator, S: Storage>(
    entries: &[ResolvedHistory],
    solver: &Solver<E, S>,
) -> PreflopReport {
    let tree = &solver.game().tree;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let node = tree.node(entry.node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        let sigma = solver.average_strategy_at(entry.node_id);
        let strategy: Vec<Vec<f32>> = (0..num_actions)
            .map(|a| sigma[a * num_hands..(a + 1) * num_hands].to_vec())
            .collect();
        out.push(PreflopHistoryEntry {
            history: entry.history.clone(),
            player: entry.player,
            actions: entry.actions.clone(),
            strategy,
        });
    }
    let expl = solver.exploitability();
    PreflopReport {
        game: "preflop".to_string(),
        iterations: solver.iteration(),
        expected_value_p0: solver.expected_value(Player::P0),
        exploitability: [expl[Player::P0], expl[Player::P1]],
        class_labels: (0..NUM_CLASSES).map(preflop::class_label).collect(),
        entries: out,
    }
}
