//! P1 solve and read-only solve drivers. The caller owns cancellation and run lifecycle.
use crate::input::{self, Algorithm, SolutionStreets};
use crate::prepare::{self, Prepared, compatibility_hash, display_game, warnings};
use anyhow::{Context, Result, bail};
use hu_engine::{
    DiscountSchedule, F32Storage, I16Storage, MixedStorage, ParConfig, Solver, Storage,
    TerminalEvaluator,
};
use nlh::{PerPlayer, Player};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Progress and durable boundaries from a P1 solve segment.
pub enum Observation {
    ProgressOpened,
    Progress(runfiles::MetricsRow),
    Checkpoint { iterations: u64 },
    Stop { reason: &'static str },
}
/// Display diagnostics emitted at their original computation boundaries.
pub enum Diagnostic {
    Memory(crate::MemoryEstimate),
    Warning(String),
    Done(RunSummary),
    Artifact(crate::artifact::Diagnostic),
}
/// Caller-owned progress persistence and event handling.
pub type Observer<'a> = dyn FnMut(Observation) -> Result<()> + Send + 'a;
/// Caller-owned diagnostic rendering.
pub type Diagnostics<'a> = dyn FnMut(Diagnostic) + Send + 'a;
/// Explicit artifact destinations and continuation state for one P1 segment.
pub struct RunRequest<'a> {
    pub prepared: &'a Prepared,
    pub checkpoint: &'a Path,
    pub solution: &'a Path,
    pub state: Option<crate::checkpoint::CheckpointReader>,
    /// Evaluations through the restored checkpoint, as (iteration, NashConv / 2).
    pub evaluation_history: Vec<(u64, f64)>,
    pub elapsed_before: Duration,
    pub cancel: &'a AtomicBool,
}

// PF10: all scheduling constants live here, independent of execution resources.
const BASE_CHECK_STEP: u64 = 25;
const MIN_CHECK_STEP: u64 = 3;
const MAX_CHECK_STEP: u64 = 50;
const PREDICTION_FRACTION: f64 = 0.8;
const MIN_CONVERGENCE_SLOPE: f64 = 0.05;

/// Deterministic evaluation schedule. `history` is sorted, with unique iterations.
/// A checkpoint between evaluations keeps the schedule anchored at the last check.
pub fn next_evaluation(
    history: &[(u64, f64)],
    target: Option<f64>,
    current: u64,
    max_iterations: u64,
    check_every: input::CheckEvery,
) -> u64 {
    if let input::CheckEvery::Fixed(step) = check_every {
        return current.saturating_add(step).min(max_iterations);
    }
    let base = history.last().map_or(current, |&(t, _)| t);
    let mut step = BASE_CHECK_STEP;
    if let Some(target) = target
        && let [.., (t0, v0), (t1, v1)] = history
        && *t0 > 0
        && t1 > t0
        && v0.is_finite()
        && v1.is_finite()
        && *v0 > 0.0
        && *v1 > 0.0
        && target.is_finite()
        && target > 0.0
    {
        let slope = (v0 / v1).ln() / (*t1 as f64 / *t0 as f64).ln();
        if slope.is_finite() && slope > MIN_CONVERGENCE_SLOPE {
            let prediction = *t1 as f64 * (v1 / target).powf(1.0 / slope);
            let predicted_step = (PREDICTION_FRACTION * (prediction - *t1 as f64)).ceil();
            if predicted_step.is_finite() {
                step = predicted_step.clamp(MIN_CHECK_STEP as f64, MAX_CHECK_STEP as f64) as u64;
            }
        }
    }
    base.saturating_add(step).max(current).min(max_iterations)
}

/// Reconstruct the append-only progress history; the last row wins for duplicates.
/// Missing, unreadable or invalid progress deliberately falls back to no history.
pub fn read_evaluation_history(path: &Path, checkpoint_iteration: u64) -> Vec<(u64, f64)> {
    // The schedule must see the exact in-memory values, so NashConv is parsed
    // with Rust's correctly rounded f64 parser rather than serde_json's.
    #[derive(serde::Deserialize)]
    struct Row<'a> {
        iteration: u64,
        #[serde(borrow)]
        nash_conv: &'a serde_json::value::RawValue,
    }
    fn read(path: &Path, checkpoint_iteration: u64) -> Result<Vec<(u64, f64)>> {
        use std::io::BufRead;
        let mut rows = std::collections::BTreeMap::new();
        for line in std::io::BufReader::new(std::fs::File::open(path)?).lines() {
            let line = line?;
            let row: Row<'_> = serde_json::from_str(&line)?;
            if row.iteration <= checkpoint_iteration {
                let nash_conv: f64 = row.nash_conv.get().parse()?;
                rows.insert(row.iteration, nash_conv / 2.0);
            }
        }
        Ok(rows.into_iter().collect())
    }
    read(path, checkpoint_iteration).unwrap_or_default()
}

fn batch_step(check_every: input::CheckEvery, remaining: u64) -> u64 {
    match check_every {
        input::CheckEvery::Auto => remaining.min(BASE_CHECK_STEP),
        input::CheckEvery::Fixed(_) => remaining,
    }
}
/// Build/restore and solve P1, writing checkpoint and solution artifacts and returning convergence data.
pub fn run(
    request: RunRequest<'_>,
    observer: &mut Observer<'_>,
    diagnostics: &mut Diagnostics<'_>,
) -> Result<RunSummary> {
    let threads = prepare::threads(request.prepared)?;
    with_threads(threads, || match request.prepared.settings.solver.storage {
        input::Storage::F32 => drive::<F32Storage>(request, observer, diagnostics),
        input::Storage::I16 => drive::<I16Storage>(request, observer, diagnostics),
        input::Storage::I16F32Avg => drive::<MixedStorage>(request, observer, diagnostics),
    })
}

pub(crate) fn query<S: Storage>(
    p: &Prepared,
    iterations: Option<u64>,
    target_nash_conv: Option<f64>,
    cancel: &AtomicBool,
) -> Result<(
    Solver<crate::PostflopEvaluator, S>,
    Vec<crate::PostflopNodeInfo>,
)> {
    crate::prepare::check_memory_limit(p)?;
    let iterations = iterations.unwrap_or(p.settings.solver.stop.max_iterations);
    if iterations == 0 || target_nash_conv.is_some_and(|t| !t.is_finite() || t < 0.0) {
        bail!("NLH003: iterations must be positive and target_nash_conv finite and non-negative");
    }
    let mut game = crate::try_build_postflop_game(&p.config, p.payoff.pipeline())
        .map_err(|e| spot::SpotError::new(spot::Code::NLH003, "tree", e.to_string()))?;
    display_game(&mut game);
    let mut solver = Solver::<_, S>::new(
        game.game,
        schedule(&p.settings.solver.algorithm),
        Some(iterations),
    );
    solver.set_cfr_precision(p.settings.solver.cfr_precision);
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    let start = Instant::now();
    let target = target_nash_conv.map(|t| t / 2.0).or(p.target);
    let check_every = p.settings.solver.stop.check_every;
    let mut history = Vec::new();
    let mut next_check = next_evaluation(&history, target, 0, iterations, check_every);
    while solver.iteration() < iterations {
        if cancel.load(std::sync::atomic::Ordering::SeqCst)
            || p.document
                .spot
                .run
                .max_time_seconds
                .is_some_and(|t| start.elapsed().as_secs_f64() >= t)
        {
            break;
        }
        solver.run(batch_step(check_every, next_check - solver.iteration()));
        if solver.iteration() == next_check {
            if let Some(target) = target {
                let expl = solver.exploitability();
                let value = (expl[Player::P0] + expl[Player::P1]) / 2.0;
                history.push((solver.iteration(), value));
                if value <= target {
                    break;
                }
            }
            next_check = next_evaluation(
                &history,
                target,
                solver.iteration(),
                iterations,
                check_every,
            );
        }
    }
    Ok((solver, game.node_info))
}

pub fn schedule(algorithm: &Algorithm) -> Box<dyn DiscountSchedule> {
    match algorithm {
        Algorithm::Vanilla => Box::new(hu_engine::Vanilla),
        Algorithm::CfrPlus => Box::new(hu_engine::CfrPlus),
        Algorithm::Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        } => Box::new(hu_engine::Dcfr {
            alpha: *alpha,
            beta: *beta,
            gamma: *gamma,
            pow4_reset: *pow4_reset,
        }),
        Algorithm::LinearCfr => Box::new(hu_engine::linear_cfr()),
        Algorithm::HsDcfr { gamma0 } => Box::new(hu_engine::HsDcfr { gamma0: *gamma0 }),
    }
}

fn drive<S: Storage>(
    request: RunRequest<'_>,
    observer: &mut Observer<'_>,
    diagnostics: &mut Diagnostics<'_>,
) -> Result<RunSummary> {
    let RunRequest {
        prepared: p,
        checkpoint,
        solution,
        state,
        evaluation_history: mut history,
        elapsed_before,
        cancel,
    } = request;
    crate::prepare::check_memory_limit(p)?;
    diagnostics(Diagnostic::Memory(p.estimate.clone()));
    for warning in warnings(p) {
        diagnostics(Diagnostic::Warning(warning));
    }
    let game = crate::try_build_postflop_game(&p.config, p.payoff.pipeline())
        .map_err(|e| spot::SpotError::new(spot::Code::NLH003, "tree", e.to_string()))?;
    let stop = &p.settings.solver.stop;
    let mut solver = Solver::<_, S>::new(
        game.game,
        schedule(&p.settings.solver.algorithm),
        Some(stop.max_iterations),
    );
    solver.set_cfr_precision(p.settings.solver.cfr_precision);
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    if let Some(state) = state {
        let iteration = state.iteration;
        solver
            .restore_stream(iteration, |storage| state.read_storage(storage))
            .context("restoring checkpoint state")?;
    }
    observer(Observation::ProgressOpened)?;
    let start = Instant::now();
    let mut last_checkpoint = Instant::now();
    let mut saved_iteration = None;
    let mut evaluated = None;
    let interval = Duration::try_from_secs_f64(p.document.spot.run.checkpoint_interval_seconds)?;
    let max_time = p.document.spot.run.max_time_seconds;
    let mut canceled = false;
    let mut reason = "max-iterations";
    let mut next_check = next_evaluation(
        &history,
        p.target,
        solver.iteration(),
        stop.max_iterations,
        stop.check_every,
    );
    while solver.iteration() < stop.max_iterations {
        if matches!(stop.check_every, input::CheckEvery::Auto)
            && history.last().is_some_and(|&(t, v)| {
                t == solver.iteration() && p.target.is_some_and(|target| v <= target)
            })
        {
            reason = "target-reached";
            break;
        }
        if max_time.is_some_and(|limit| (elapsed_before + start.elapsed()).as_secs_f64() >= limit) {
            reason = "time-limit";
            break;
        }
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            canceled = true;
            reason = "cancelled";
            break;
        }
        solver.run(batch_step(
            stop.check_every,
            next_check - solver.iteration(),
        ));
        // A stop between evaluations must summarize the actual checkpoint state.
        evaluated = None;
        let mut target_reached = false;
        if solver.iteration() == next_check {
            let (ev, expl) = solver.evaluate();
            evaluated = Some((ev, expl));
            let nash_conv = expl[Player::P0] + expl[Player::P1];
            observer(Observation::Progress(runfiles::MetricsRow {
                iteration: solver.iteration(),
                elapsed_secs: (elapsed_before + start.elapsed()).as_secs_f64(),
                expl_p0: expl[Player::P0],
                expl_p1: expl[Player::P1],
                nash_conv,
            }))?;
            history.push((solver.iteration(), nash_conv / 2.0));
            target_reached = p.target.is_some_and(|target| nash_conv / 2.0 <= target);
            next_check = next_evaluation(
                &history,
                p.target,
                solver.iteration(),
                stop.max_iterations,
                stop.check_every,
            );
        }
        let elapsed_secs = (elapsed_before + start.elapsed()).as_secs_f64();
        if last_checkpoint.elapsed() >= interval {
            save(&solver, p, checkpoint, elapsed_secs, observer)?;
            last_checkpoint = Instant::now();
            saved_iteration = Some(solver.iteration());
        }
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            canceled = true;
            reason = "cancelled";
            break;
        }
        if target_reached {
            reason = "target-reached";
            break;
        }
        if matches!(stop.check_every, input::CheckEvery::Auto)
            && max_time
                .is_some_and(|limit| (elapsed_before + start.elapsed()).as_secs_f64() >= limit)
        {
            reason = "time-limit";
            break;
        }
    }
    let elapsed = elapsed_before + start.elapsed();
    observer(Observation::Stop { reason })?;
    let offset = p.payoff.ev_offset();
    let (ev, expl) = evaluated.unwrap_or_else(|| solver.evaluate());
    let mut summary = summary_from_evaluation(&solver, elapsed, subgame_ev(ev, offset), expl);
    diagnostics(Diagnostic::Done(summary.clone()));
    summary.canceled = canceled;
    let final_checkpoint =
        p.document.spot.run.final_checkpoint && saved_iteration != Some(solver.iteration());
    let spec = crate::artifact::SolExportSpec {
        path: solution.to_path_buf(),
        mode: match p.settings.output.solution_streets {
            SolutionStreets::Full => crate::artifact::SolStreets::Full,
            SolutionStreets::NoRivers => crate::artifact::SolStreets::NoRivers,
        },
        config_toml: p.effective.clone(),
        storage_name: match p.settings.solver.storage {
            input::Storage::F32 => "f32",
            input::Storage::I16 => "i16",
            input::Storage::I16F32Avg => "i16-f32avg",
        }
        .into(),
    };
    if parallel_finalize(
        final_checkpoint,
        &p.estimate,
        p.settings.solver.storage,
        p.limit,
    ) {
        // join inherits the installed run pool for BOTH closures. In particular,
        // save's current_num_threads and the solution's strategy batches must
        // never fall back to the global pool. Each closure only borrows solver.
        let (checkpoint_result, solution_result) = rayon::join(
            || save(&solver, p, checkpoint, elapsed.as_secs_f64(), observer),
            || {
                crate::artifact::export_sol(
                    &spec,
                    &solver,
                    offset,
                    p.document.spot.context.street,
                    &summary,
                    &mut |d| diagnostics(Diagnostic::Artifact(d)),
                )
            },
        );
        // join waits for both writers, even on error. A successful .sol may
        // remain on checkpoint failure; the caller records the run as failed.
        checkpoint_result?;
        solution_result?;
    } else {
        if final_checkpoint {
            save(&solver, p, checkpoint, elapsed.as_secs_f64(), observer)?;
        }
        solver.release_regrets();
        crate::artifact::export_sol(
            &spec,
            &solver,
            offset,
            p.document.spot.context.street,
            &summary,
            &mut |d| diagnostics(Diagnostic::Artifact(d)),
        )?;
    }
    Ok(summary)
}

fn parallel_finalize(
    final_checkpoint: bool,
    estimate: &crate::MemoryEstimate,
    backend: input::Storage,
    limit: u64,
) -> bool {
    final_checkpoint
        && estimate
            .parallel_save_bytes(backend)
            .is_some_and(|peak| peak <= limit)
}

fn save<S: Storage>(
    solver: &Solver<crate::PostflopEvaluator, S>,
    p: &Prepared,
    checkpoint: &Path,
    elapsed: f64,
    observer: &mut Observer<'_>,
) -> Result<()> {
    crate::checkpoint::write_storage_with_config(
        checkpoint,
        compatibility_hash(&p.effective)?,
        solver.iteration(),
        solver.storage(),
        &p.effective,
        elapsed,
        rayon::current_num_threads(),
    )?;
    observer(Observation::Checkpoint {
        iterations: solver.iteration(),
    })?;
    Ok(())
}

pub(crate) fn with_threads<T: Send>(
    threads: Option<usize>,
    work: impl FnOnce() -> Result<T> + Send,
) -> Result<T> {
    match threads {
        Some(threads) => rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?
            .install(work),
        None => work(),
    }
}

pub(crate) fn subgame_ev(solver_ev: PerPlayer<f64>, offset: PerPlayer<f64>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver_ev[Player::P0] + offset[Player::P0],
        solver_ev[Player::P1] + offset[Player::P1],
    )
}

pub(crate) fn solver_ev<E: TerminalEvaluator, S: Storage>(solver: &Solver<E, S>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver.expected_value(Player::P0),
        solver.expected_value(Player::P1),
    )
}

/// Final convergence and cancellation data, on the subgame-start reporting basis.
#[derive(Clone)]
pub struct RunSummary {
    pub iterations: u64,
    /// Root EV per player, on the subgame-start reporting basis.
    pub ev: PerPlayer<f64>,
    pub wall: Duration,
    pub expl_p0: f64,
    pub expl_p1: f64,
    pub nash_conv: f64,
    /// The run stopped on request at a checkpoint boundary, not because it
    /// reached its budget or its convergence target.
    pub canceled: bool,
}

pub(crate) fn summarize<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
    ev: PerPlayer<f64>,
) -> RunSummary {
    let expl = solver.exploitability();
    summary_from_evaluation(solver, elapsed, ev, expl)
}

fn summary_from_evaluation<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
    ev: PerPlayer<f64>,
    expl: PerPlayer<f64>,
) -> RunSummary {
    let nash_conv = expl[Player::P0] + expl[Player::P1];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_values_round_trip_without_changing_schedule_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(runfiles::RUN_PROGRESS_FILE);
        let mut writer = runfiles::MetricsWriter::create_or_append(&path).unwrap();
        let mut bits = 123456789_u64;
        let mut expected = Vec::new();
        for iteration in 1..=256 {
            bits = bits.wrapping_mul(6364136223846793005).wrapping_add(1);
            let value = f64::from_bits((bits & ((1_u64 << 52) - 1)) | (1023_u64 << 52));
            writer
                .append(&runfiles::MetricsRow {
                    iteration,
                    elapsed_secs: 0.0,
                    expl_p0: value,
                    expl_p1: value,
                    nash_conv: value * 2.0,
                })
                .unwrap();
            expected.push((iteration, value));
        }
        drop(writer);
        assert_eq!(read_evaluation_history(&path, 256), expected);
    }

    #[test]
    fn adaptive_schedule_is_bounded_and_uses_only_evaluations() {
        let next = |h: &[(u64, f64)], target, current, max| {
            next_evaluation(h, target, current, max, input::CheckEvery::Auto)
        };
        assert_eq!(next(&[], Some(1.0), 0, 1000), 25);
        assert_eq!(next(&[], Some(1.0), 77, 1000), 102);
        assert_eq!(next(&[(25, 4.0)], Some(1.0), 25, 1000), 50);
        let h = [(25, 4.0), (50, 2.0)]; // slope 1, arrival 100, ceil(.8*50)=40
        assert_eq!(next(&h, Some(1.0), 50, 1000), 90);
        assert_eq!(next(&h, Some(1.0), 75, 1000), 90);
        assert_eq!(next(&h, None, 50, 1000), 75);
        assert_eq!(next(&h, Some(0.01), 50, 1000), 100); // upper bound
        assert_eq!(next(&h, Some(1.99), 50, 1000), 53); // lower bound
        assert_eq!(next(&h, Some(3.0), 50, 1000), 53); // prediction in the past
        assert_eq!(next(&h, Some(1.0), 50, 61), 61);
        assert_eq!(next(&[], Some(1.0), 0, 9), 9);
        for h in [
            [(25, 2.0), (50, 2.0)],
            [(25, 2.0), (50, 1.99)], // slope <= .05
            [(25, 1.0), (50, 2.0)],
            [(25, f64::NAN), (50, 2.0)],
            [(25, 2.0), (50, f64::INFINITY)],
            [(0, 4.0), (50, 2.0)],
            [(25, 0.0), (50, 2.0)],
        ] {
            assert_eq!(next(&h, Some(1.0), 50, 1000), 75);
        }
        assert_eq!(
            next(&[(25, 4.0), (50, 2.0)], Some(f64::MIN_POSITIVE), 50, 1000),
            75
        );
        assert_eq!(
            next_evaluation(&h, Some(1.0), 77, 1000, input::CheckEvery::Fixed(25)),
            102
        );
        assert_eq!(batch_step(input::CheckEvery::Auto, 50), 25);
        assert_eq!(batch_step(input::CheckEvery::Auto, 7), 7);
        assert_eq!(batch_step(input::CheckEvery::Fixed(50), 50), 50);
    }

    #[test]
    fn parallel_finalization_requires_a_save_and_room_for_both_codecs() {
        let estimate = crate::MemoryEstimate {
            f32_bytes: 100,
            i16_bytes: 60,
            i16_f32avg_bytes: 80,
            save_bytes: 40,
            compression_bytes: 10,
            ..Default::default()
        };
        for (backend, peak) in [
            (input::Storage::F32, 160),
            (input::Storage::I16, 120),
            (input::Storage::I16F32Avg, 140),
        ] {
            assert_eq!(estimate.parallel_save_bytes(backend), Some(peak));
            assert!(!parallel_finalize(true, &estimate, backend, peak - 1));
            assert!(parallel_finalize(true, &estimate, backend, peak));
            assert!(parallel_finalize(true, &estimate, backend, peak + 1));
            assert!(!parallel_finalize(false, &estimate, backend, u64::MAX));
        }
        for (storage, workspace, codec) in [(u64::MAX, 1, 0), (0, 0, u64::MAX)] {
            let overflow = crate::MemoryEstimate {
                f32_bytes: storage,
                save_bytes: workspace,
                compression_bytes: codec,
                ..Default::default()
            };
            assert!(!parallel_finalize(
                true,
                &overflow,
                input::Storage::F32,
                u64::MAX
            ));
        }
    }
}
