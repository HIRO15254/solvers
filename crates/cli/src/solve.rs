use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use engine::{
    DiscountSchedule, F32Storage, I16Storage, ParConfig, Solver, SolverState, Storage,
    TerminalEvaluator,
};
use game::PayoffPipeline;
use holdem::build_postflop_game;
use nlh::{PerPlayer, Player};

use crate::config::{GameSection, RunSection, SolveConfig, StorageKind, TreeSection};
use crate::postflop_setup;
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
    crate::run_dir::create_or_adopt(out)?;
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
    // while the heads-up path publishes its strategy through the `.sol`
    // viewer artifact, which `export` reads: a second, partial JSON of the
    // same thing would be one more shape to keep in agreement for no gain.
    let paths = &run_paths;
    let metrics = Some(paths.progress.as_path());
    let checkpoint = Some(paths.checkpoint.as_path());
    let sol = Some(paths.solution.as_path());

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
            Some(paths.result.as_path()),
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

    let sol_spec = sol.map(|path| {
        let storage_name = match config.run.storage {
            StorageKind::F32 => "f32",
            StorageKind::I16 => "i16",
        };
        SolExportSpec {
            path: path.to_path_buf(),
            mode: sol_streets,
            config_toml: raw.to_string(),
            storage_name: storage_name.to_string(),
        }
    });

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
        metrics,
        checkpoint_sink,
        sol_spec,
        Some(recorder.events_mut()),
        Some(&crate::CLI_CANCEL),
    );
    let completion = outcome
        .as_ref()
        .ok()
        .map(|summary| write_heads_up_result(&paths.result, game_kind, summary))
        .transpose()?;
    recorder.finish(outcome.map(|_summary| ()), completion)
}

/// Shared publication step for fresh and resumed HU runs.
pub(crate) fn write_heads_up_result(
    path: &Path,
    kind: &str,
    summary: &RunSummary,
) -> Result<String> {
    let recorded = serde_json::json!({
        "kind": kind, "iterations": summary.iterations,
        "wallSecs": summary.wall.as_secs_f64(),
        "explP0": summary.expl_p0, "explP1": summary.expl_p1,
        "nashConv": summary.nash_conv,
    });
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&recorded)?),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(if summary.canceled {
        "cancelled"
    } else {
        "completed"
    }
    .to_string())
}

/// Names the game for a run manifest. These match the config's `kind`.
pub(crate) fn game_kind_name(game: &GameSection) -> &'static str {
    match game {
        GameSection::Postflop { .. } => "postflop",
        GameSection::PreflopMultiway(_) => "preflop-multiway",
    }
}

fn solve_heads_up(
    config: SolveConfig,
    metrics: Option<&Path>,
    checkpoint_sink: Option<(&Path, [u8; 32])>,
    sol_spec: Option<SolExportSpec>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RunSummary> {
    match config.run.storage {
        StorageKind::F32 => run_with_storage_sol::<F32Storage>(
            config,
            metrics,
            checkpoint_sink,
            None,
            sol_spec,
            events,
            cancel,
        ),
        StorageKind::I16 => run_with_storage_sol::<I16Storage>(
            config,
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
    pub elapsed_before: Duration,
    pub quiet: bool,
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
            elapsed_before: Duration::ZERO,
            quiet: false,
        }
    }
}

impl RunHooks<'_> {
    fn log(&self, message: std::fmt::Arguments<'_>) {
        if self.quiet {
            eprintln!("{message}");
        } else {
            println!("{message}");
        }
    }
}

/// Last durable cumulative solve time; truncated tail rows are ignored.
pub(crate) fn previous_elapsed(path: &Path) -> Result<Duration> {
    use std::io::BufRead;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Duration::ZERO),
        Err(error) => return Err(error.into()),
    };
    let mut elapsed = Duration::ZERO;
    for line in std::io::BufReader::new(file).lines() {
        if let Ok(row) = serde_json::from_str::<formats::MetricsRow>(&line?) {
            elapsed = Duration::try_from_secs_f64(row.elapsed_secs)
                .context("invalid recorded elapsed solve time")?;
        }
    }
    Ok(elapsed)
}

/// Shared solve/resume path, including the postflop viewer artifact.
pub(crate) fn run_with_storage_sol<S: Storage>(
    config: SolveConfig,
    metrics: Option<&Path>,
    checkpoint: Option<(&Path, [u8; 32])>,
    resume_state: Option<SolverState>,
    sol: Option<SolExportSpec>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RunSummary> {
    run_with_storage_impl::<S>(
        config,
        metrics,
        checkpoint,
        resume_state,
        sol,
        events,
        cancel,
    )
}

fn run_with_storage_impl<S: Storage>(
    config: SolveConfig,
    metrics: Option<&Path>,
    checkpoint: Option<(&Path, [u8; 32])>,
    resume_state: Option<SolverState>,
    sol: Option<SolExportSpec>,
    events: Option<&mut formats::RunEventLog>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<RunSummary> {
    let rake = crate::economics::build_rake(&config.rake)?;
    let utility = crate::economics::build_utility(&config.utility)?;
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };

    let schedule = postflop_setup::build_schedule(&config.algorithm);
    let schedule_name = schedule.name();

    let elapsed_before = if resume_state.is_some() {
        metrics
            .map(previous_elapsed)
            .transpose()?
            .unwrap_or_default()
    } else {
        Duration::ZERO
    };
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
        elapsed_before,
        quiet: false,
    };

    postflop_setup::with_threads(config.run.threads, || match config.game {
        GameSection::Postflop {
            board,
            oop_range,
            ip_range,
            pot,
            effective_stack,
            iso_merging,
            min_bet,
            preflop_aggressor,
            tree,
        } => solve_postflop::<S>(
            pipeline,
            &board,
            &oop_range,
            &ip_range,
            pot,
            effective_stack,
            iso_merging,
            min_bet,
            tree,
            &preflop_aggressor,
            schedule,
            schedule_name,
            &config.run,
            resume_state,
            sol,
            &mut hooks,
        ),
        GameSection::PreflopMultiway(_) => {
            unreachable!("multiway games dispatch before the HU storage path")
        }
    })
    .map(|summary| RunSummary {
        canceled: hooks.canceled,
        ..summary
    })
}

/// Convergence loop shared by every heads-up caller: run a chunk of
/// iterations, report exploitability, and stop early once `target_nash_conv`
/// or `max_time` is hit. Both stop conditions are evaluated only at
/// `check_every` marks, so a run overshoots by at most one chunk. Generic
/// over both the terminal evaluator and the storage backend so every caller
/// (solve, resume, report, inspect) and both storage backends reuse it
/// unchanged.
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
    if run.check_every == 0 {
        return Err(anyhow!("SLV004: run.check_every must be positive"));
    }
    hooks.start = Instant::now();
    if run
        .max_time_secs
        .is_some_and(|limit| hooks.elapsed_before >= Duration::from_secs(limit))
    {
        hooks.log(format_args!("max_time already reached before resume"));
        if let Some(events) = hooks.events.as_deref_mut() {
            let _ = events.info(formats::RunEventPayload::Stop {
                reason: "time-limit".to_string(),
            });
        }
        return Ok(());
    }
    let mut remaining = run.iterations.saturating_sub(solver.iteration());
    while remaining > 0 {
        let chunk = run.check_every.min(remaining);
        solver.run(chunk);
        remaining -= chunk;
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        hooks.log(format_args!(
            "iter={:>8} expl_p0={:.3e} expl_p1={:.3e} nash_conv={:.3e}",
            solver.iteration(),
            expl[Player::P0],
            expl[Player::P1],
            nash_conv,
        ));

        // Computed before touching `hooks.metrics` so the two field
        // borrows below never overlap.
        let elapsed_secs = (hooks.elapsed_before + hooks.start.elapsed()).as_secs_f64();
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
            hooks.log(format_args!(
                "cancelled at iteration {}",
                solver.iteration()
            ));
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
            hooks.log(format_args!("target nash_conv {target:.3e} reached"));
            break;
        }

        // Checked here rather than mid-chunk so a time-limited run still
        // stops on an exploitability mark, with a checkpoint already
        // written for it.
        if let Some(limit) = run.max_time_secs
            && elapsed_secs >= limit as f64
        {
            hooks.log(format_args!(
                "max_time {limit}s reached at iteration {}",
                solver.iteration()
            ));
            if let Some(events) = hooks.events.as_deref_mut() {
                let _ = events.info(formats::RunEventPayload::Stop {
                    reason: "time-limit".to_string(),
                });
            }
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
    /// Root EV per player, on the family's reporting basis.
    pub ev: PerPlayer<f64>,
    pub wall: Duration,
    pub expl_p0: f64,
    pub expl_p1: f64,
    pub nash_conv: f64,
    /// The run stopped on request at a checkpoint boundary, not because it
    /// reached its budget or its convergence target.
    pub canceled: bool,
}

/// Final convergence summary, shared by every heads-up caller.
///
/// `ev` is already on the family's reporting basis — for postflop that is
/// the subgame-start basis (see [`crate::postflop_setup::subgame_ev`]). Both
/// players' numbers are printed because `ev_p1 == -ev_p0` is not guaranteed:
/// rake makes any game general-sum, and postflop's basis makes the pair sum
/// to the starting pot rather than to zero.
pub(crate) fn print_done<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
    ev: PerPlayer<f64>,
) -> RunSummary {
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    println!(
        "done: iterations={} wall={:.2}s ev_p0={:.6} ev_p1={:.6} nash_conv={:.3e}",
        solver.iteration(),
        elapsed.as_secs_f64(),
        ev[Player::P0],
        ev[Player::P1],
        nash_conv,
    );
    RunSummary {
        canceled: false,
        iterations: solver.iteration(),
        wall: elapsed,
        ev,
        expl_p0: expl[Player::P0],
        expl_p1: expl[Player::P1],
        nash_conv,
    }
}

/// The solver's own root values, before the family's reporting offset (see
/// [`crate::postflop_setup::subgame_ev`]) is applied.
pub(crate) fn solver_ev<E: TerminalEvaluator, S: Storage>(solver: &Solver<E, S>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver.expected_value(Player::P0),
        solver.expected_value(Player::P1),
    )
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
    min_bet: u32,
    tree: TreeSection,
    preflop_aggressor: &str,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
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
        min_bet,
        tree.lower()?,
        preflop_aggressor,
    )?;

    // Cheap dry run before committing to the (possibly very large) real
    // build, so an oversized config fails fast with a size estimate instead
    // of silently eating memory.
    let estimate = holdem::memory_usage(&config);
    postflop_setup::print_memory_estimate(&estimate);
    // The dry run walks every decision node exactly like the real build
    // (`Counting` mirrors `Builder`), so it already has the full answer to
    // "did any node ever satisfy this rule?" -- no need to also inspect the
    // real build's own `rule_hits` for a single-config run like this one.
    postflop_setup::warn_unmatched_rules(&config.streets, &estimate.rule_hits);

    // Captured before `pipeline` is consumed by the build: every EV this
    // run reports is on the subgame-start basis, and this is the constant
    // that puts it there.
    let ev_offset = postflop_setup::subgame_ev_offset(&config, pipeline.utility);
    let pf_game = build_postflop_game(&config, pipeline);

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
    let elapsed = hooks.elapsed_before + start.elapsed();
    let summary = print_done(
        &solver,
        elapsed,
        postflop_setup::subgame_ev(solver_ev(&solver), ev_offset),
    );
    checkpoint_now(&solver, hooks)?;

    if let Some(spec) = &sol {
        let start_street = crate::sol::start_street_from_board_len(config.board.len());
        crate::sol::export_sol(spec, &solver, ev_offset, start_street, &summary)?;
    }

    Ok(summary)
}
