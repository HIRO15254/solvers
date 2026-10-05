//! P2 solve/resume computation with caller-owned observations and diagnostics.
use std::borrow::Cow;
use std::cell::Cell;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::metrics::{MULTIWAY_SCHEMA_VERSION, MultiwayMetricsRow};
use crate::solver::StrategyDriftTracker;
use crate::{ExternalSamplingGame, HoldemGame, MultiwaySolver};
use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use crate::UtilityConfig;
use crate::input::Lowered;
use crate::session;

const APPROXIMATION_NOTICE: &str = "3人以上は多人数・一般和ゲームのregret-minimized approximationです。Nash/GTO保証やexploitability指標ではありません。";
const LIVE_OBSERVATION_PERIOD: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Terminal reason for a P2 solve segment.
pub enum CompletionStatus {
    ResourceLimit,
    #[serde(rename = "sweep-limit")]
    SweepLimit,
    #[serde(rename = "target-reached")]
    TargetReached,
    #[serde(rename = "time-limit")]
    TimeLimit,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationalBoundary {
    Cancelled,
    TimeLimit,
    CheckpointDue,
}

fn operational_boundary(
    cancelled: bool,
    cumulative_solve_time: Duration,
    max_time: Option<Duration>,
    since_checkpoint: Duration,
    checkpoint_interval: Option<Duration>,
    checkpoint_enabled: bool,
) -> Option<OperationalBoundary> {
    if cancelled {
        Some(OperationalBoundary::Cancelled)
    } else if max_time.is_some_and(|limit| cumulative_solve_time >= limit) {
        Some(OperationalBoundary::TimeLimit)
    } else if checkpoint_enabled
        && checkpoint_interval.is_some_and(|interval| since_checkpoint >= interval)
    {
        Some(OperationalBoundary::CheckpointDue)
    } else {
        None
    }
}

fn regular_evaluation_due(sweeps: u64, cadence: u64, target: u64) -> bool {
    sweeps.is_multiple_of(cadence) || sweeps == target
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// Run summary returned after product artifacts have been written.
pub struct RunResult {
    pub schema_version: u16,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<&'static str>,
    pub status: CompletionStatus,
    pub approximate_profile: bool,
    pub approximation_notice: &'static str,
    pub sweeps: u64,
    pub traversals: u64,
    pub infosets: u64,
    pub memory_bytes: u64,
    pub policy_storage: &'static str,
    pub preallocated_nodes: u64,
    pub preallocated_columns: u64,
    pub preallocated_slots: u64,
    pub preallocated_bytes: u64,
    pub preallocated_pages_committed: bool,
    pub policy_arena_limit_bytes: u64,
    pub elapsed_secs: f64,
    pub traversals_per_second: f64,
    pub total_deal_attempts: u64,
    pub mean_deal_attempts: f64,
    /// See `crate::solver::SolverState::hand_updates`. Compare against a
    /// range-based solver's "hands/s".
    pub hand_updates: u64,
    pub hand_updates_per_second: f64,
    pub seats: Vec<crate::metrics::MultiwaySeatMetrics>,
    pub strategy_blocks: usize,
    pub config_hash: String,
    pub effective_config: serde_json::Value,
    pub game_fingerprint: String,
    pub abstraction_fingerprint: String,
    pub algorithm_fingerprint: String,
    pub configuration_fingerprint: String,
    pub profile_type: &'static str,
    pub guarantee_boundary: &'static str,
    pub chip_unit_bb: f64,
    pub utility_unit: &'static str,
    pub started_unix_ms: u64,
    pub finished_unix_ms: u64,
}

/// Cheap data published during training without a held-out evaluation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayLiveObservation {
    pub sweeps: u64,
    pub traversals: u64,
    pub hand_updates: u64,
    pub elapsed_secs: f64,
}

/// Full quality data published at completed evaluation boundaries.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayQualityObservation {
    pub metrics: MultiwayMetricsRow,
}

/// The card abstraction is ready. Published before sweep 0 because a cold
/// EHS² build takes minutes, and a watcher otherwise sees an unexplained
/// silence between `running` and the first progress row.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayAbstractionObservation {
    pub cached: bool,
    pub secs: f64,
}

/// A checkpoint was written; the run is resumable from this sweep count.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayCheckpointObservation {
    pub sweeps: u64,
}

/// The solver reached its terminal status. Published once, before the
/// artifacts are written, so a watcher learns the reason at the moment the
/// solve stops rather than when the last file lands.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayStopObservation {
    pub reason: String,
    pub sweeps: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MultiwayRunObservation {
    /// Open the caller-owned progress stream after session initialization.
    ProgressOpened,
    /// Persist this row in the caller-owned progress stream.
    Progress {
        metrics: MultiwayMetricsRow,
        point: ProgressPoint,
    },
    Abstraction(MultiwayAbstractionObservation),
    Live(MultiwayLiveObservation),
    Quality(MultiwayQualityObservation),
    Checkpoint(MultiwayCheckpointObservation),
    Stop(MultiwayStopObservation),
}

/// Diagnostics emitted at the same boundaries as the solve observations.
#[derive(Clone, Debug)]
pub enum Diagnostic {
    Abstraction(session::AbstractionReady),
    Warning(String),
    Arena(crate::PolicyArenaAllocation),
    Underutilized {
        seats: u64,
        sweep_batch: u64,
        threads: u64,
    },
    SamplesDoubled {
        max_width: f64,
        threshold: f64,
        before: u64,
        after: u64,
    },
    Evaluation(crate::SolverMetrics),
    ResourceCheckpoint(PathBuf),
}

/// The persistence boundary of a progress row, independent of its measured phase.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressPoint {
    Resume,
    Quality,
    Checkpoint,
    Final,
}

/// Typed inputs and artifact destinations for one solve segment.
/// The caller owns run-directory creation, cache-root selection and cancellation.
pub struct RunRequest<'a> {
    pub raw_config: &'a str,
    pub config: Lowered,
    pub output: Option<&'a Path>,
    pub metrics_path: Option<&'a Path>,
    pub checkpoint_path: Option<&'a Path>,
    pub config_hash: [u8; 32],
    pub mwsol_path: Option<&'a Path>,
    pub cancel: Option<&'a AtomicBool>,
    pub cache_root: Option<&'a Path>,
    pub emit_progress: bool,
}

/// Build and solve a P2 run, writing product artifacts and returning the run summary.
/// The observer handles progress persistence; diagnostics are delivered for display.
pub fn run(
    request: RunRequest<'_>,
    observer: &mut dyn FnMut(MultiwayRunObservation) -> Result<()>,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<RunResult> {
    drive(request, None, false, observer, diagnostics)
}

/// Restore a P2 checkpoint and continue the same solve lifecycle; return its summary.
/// Reset confirmations only when the caller has changed the stop target.
pub fn resume(
    request: RunRequest<'_>,
    checkpoint: &Path,
    reset_confirmations: bool,
    observer: &mut dyn FnMut(MultiwayRunObservation) -> Result<()>,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<RunResult> {
    drive(
        request,
        Some(checkpoint),
        reset_confirmations,
        observer,
        diagnostics,
    )
}

fn drive(
    request: RunRequest<'_>,
    resume_checkpoint: Option<&Path>,
    reset_confirmations: bool,
    observer: &mut dyn FnMut(MultiwayRunObservation) -> Result<()>,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<RunResult> {
    let RunRequest {
        raw_config,
        config,
        output,
        metrics_path,
        checkpoint_path,
        config_hash,
        mwsol_path,
        cancel,
        cache_root,
        emit_progress,
    } = request;
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        checkpoint_path,
        config_hash,
        mwsol_path,
        cancel,
        resume_checkpoint,
        reset_confirmations,
        emit_progress,
        Some(observer),
        cache_root,
        diagnostics,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    raw_config: &str,
    config: Lowered,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: Option<&Path>,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    resume_checkpoint: Option<&Path>,
    reset_confirmations: bool,
    emit_progress: bool,
    mut observer: Option<&mut dyn FnMut(MultiwayRunObservation) -> Result<()>>,
    cache_root: Option<&Path>,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<RunResult> {
    validate_artifact_paths(output, metrics_path, checkpoint_path, mwsol_path)?;
    let effective_toml = raw_config.to_owned();
    let run_settings = config.run.clone();
    let output_settings = config.output.clone();
    let utility_unit = match &config.utility {
        UtilityConfig::ChipEv => "bb",
        UtilityConfig::TournamentIcm { .. } => "prize",
    };
    let algorithm = config.solver;
    #[cfg(not(test))]
    let mut mw_session = session::build_production_multiway_session(
        config,
        effective_toml,
        resume_checkpoint,
        cache_root,
        &mut |ready| diagnostics(Diagnostic::Abstraction(ready)),
    )?;
    #[cfg(test)]
    let _ = cache_root;
    #[cfg(test)]
    let mut mw_session = session::build_test_session(config, effective_toml, resume_checkpoint)?;
    let hits = mw_session
        .solver
        .game()
        .tree_rule_hits()
        .expect("common-input tree measurement");
    for warning in crate::prepare::warnings_for_hits(&mw_session.game_config, &hits) {
        diagnostics(Diagnostic::Warning(warning));
    }
    if let Some(ready) = mw_session.abstraction_ready
        && let Some(observer) = &mut observer
    {
        observer(MultiwayRunObservation::Abstraction(
            MultiwayAbstractionObservation {
                cached: ready.cached,
                secs: ready.secs,
            },
        ))?;
    }
    mw_session.config_toml = raw_config.to_string();
    mw_session.config_hash = config_hash;
    let policy_allocation = mw_session.solver.policy_arena_allocation();
    #[cfg(not(test))]
    let policy_allocation = Some(
        policy_allocation
            .filter(|allocation| allocation.pages_committed)
            .ok_or_else(|| {
                anyhow!("production solver has no page-committed preallocated policy arena")
            })?,
    );
    if emit_progress && let Some(policy_allocation) = policy_allocation {
        diagnostics(Diagnostic::Arena(policy_allocation));
    }

    // One sweep only yields `seats` parallel traversals, so the machine is
    // undersubscribed whenever `seats x sweep_batch < threads`. Purely a
    // hint: changing `solver.batch_sweeps` changes results (see the sweep-batch
    // docs), so it is never adjusted silently.
    if emit_progress {
        let seats = mw_session.game_config.seats.len().max(1) as u64;
        let sweep_batch = mw_session.solver.config().sweep_batch.max(1);
        let threads = mw_session.threads as u64;
        if seats * sweep_batch < threads {
            diagnostics(Diagnostic::Underutilized {
                seats,
                sweep_batch,
                threads,
            });
        }
    }

    if metrics_path.is_some()
        && let Some(observer) = observer.as_deref_mut()
    {
        observer(MultiwayRunObservation::ProgressOpened)?;
    }
    let started_unix_ms = unix_ms()?;
    let started = Instant::now();
    // Seeds `prior` with the current averages (the drift result is
    // discarded), so a resumed run's first drift row measures movement since
    // the checkpoint rather than since an empty profile.
    let mut prior = StrategyDriftTracker::new();
    mw_session
        .solver
        .strategy_drift_refresh_compact(&mut prior)
        .context("seeding multiway strategy drift")?;
    let mut last_row = MultiwayMetricsRow::sampling(mw_session.game_config.seats.len());
    let checkpoint_interval = Some(Duration::try_from_secs_f64(
        run_settings.checkpoint_interval_seconds,
    )?);
    let mut last_checkpoint = Instant::now();
    let mut status = CompletionStatus::SweepLimit;
    let max_time = run_settings
        .max_time_seconds
        .map(Duration::try_from_secs_f64)
        .transpose()?;
    let mut has_evaluation = false;
    // Restore the held-out evaluation sequence and consecutive confirmations.
    let mut stop_rule_state = session::StopRuleState::new(mw_session.evaluation_samples);
    let mut last_live_observation = Instant::now();
    let mut published_live_observation = false;
    let mut last_quality_observation_sweeps = None;
    let cumulative_before = mw_session
        .checkpoint_runtime
        .map_or(0, |runtime| runtime.cumulative_solve_millis);
    if let Some(runtime) = mw_session.checkpoint_runtime {
        stop_rule_state.samples = runtime.evaluation_samples;
        stop_rule_state.confirmations_met = runtime.confirmations_met;
        stop_rule_state.eval_index = runtime.evaluation_sequence;
    }
    if reset_confirmations {
        stop_rule_state.confirmations_met = 0;
    }
    if resume_checkpoint.is_some() {
        let resumed = mw_session.solver.metrics();
        last_row = session::metrics_row(
            &resumed,
            vec![0.0; mw_session.game_config.seats.len()],
            Duration::from_millis(cumulative_before).as_secs_f64(),
            None,
        );
        last_row.phase = "resume-segment".into();
        if metrics_path.is_some()
            && let Some(observer) = observer.as_deref_mut()
        {
            observer(MultiwayRunObservation::Progress {
                metrics: last_row.clone(),
                point: ProgressPoint::Resume,
            })?;
        }
    }

    while mw_session.solver.completed_sweeps() < mw_session.sweeps_target {
        if max_time.is_some_and(|limit| {
            Duration::from_millis(cumulative_before).saturating_add(started.elapsed()) >= limit
        }) {
            status = CompletionStatus::TimeLimit;
            break;
        }
        if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
            status = CompletionStatus::Cancelled;
            break;
        }
        let current = mw_session.solver.completed_sweeps();
        let evaluation_delta =
            session::distance_to_boundary(current, mw_session.evaluation_cadence);
        let chunk = (mw_session.sweeps_target - current)
            .min(evaluation_delta)
            .max(1);
        let chunk_end = current
            .checked_add(chunk)
            .ok_or(crate::solver::SolverError::CounterOverflow)?;
        let mut deferred_live_observation = false;
        let operational_stop = Cell::new(None);
        let mut live_error = None;
        let execution = mw_session.solver.run_sweeps_with_threads_until_observed(
            chunk,
            mw_session.threads,
            || {
                let reason = operational_boundary(
                    cancel.is_some_and(|token| token.load(Ordering::Relaxed)),
                    Duration::from_millis(cumulative_before).saturating_add(started.elapsed()),
                    max_time,
                    last_checkpoint.elapsed(),
                    checkpoint_interval,
                    checkpoint_path.is_some(),
                );
                operational_stop.set(reason);
                reason.is_none()
            },
            |solver| {
                if observer.is_some()
                    && (!published_live_observation
                        || last_live_observation.elapsed() >= LIVE_OBSERVATION_PERIOD)
                {
                    if solver.completed_sweeps() == chunk_end {
                        // The drive loop may immediately run a quality
                        // evaluation or checkpoint at this boundary. Defer
                        // the live event so the UI sees one coherent update
                        // after that work rather than a before/after pair.
                        deferred_live_observation = true;
                    } else {
                        if let Err(error) = publish_live_observation(
                            solver,
                            cumulative_before,
                            &started,
                            &mut observer,
                        ) {
                            live_error = Some(error);
                        }
                        last_live_observation = Instant::now();
                        published_live_observation = true;
                    }
                }
            },
        );
        if let Some(error) = live_error {
            return Err(error);
        }
        match execution {
            Ok(completed) => {
                if completed < chunk {
                    match operational_stop.get() {
                        Some(OperationalBoundary::Cancelled) => {
                            status = CompletionStatus::Cancelled;
                            break;
                        }
                        Some(OperationalBoundary::TimeLimit) => {
                            status = CompletionStatus::TimeLimit;
                            break;
                        }
                        Some(OperationalBoundary::CheckpointDue) => {}
                        None => {
                            return Err(anyhow!(
                                "multiway solver stopped before its chunk boundary without an operational reason"
                            ));
                        }
                    }
                }
            }
            Err(error) => return Err(error).context("running multiway MCCFR"),
        }
        if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
            status = CompletionStatus::Cancelled;
            break;
        }
        if mw_session.solver.completed_sweeps() < mw_session.sweeps_target
            && max_time.is_some_and(|limit| {
                Duration::from_millis(cumulative_before).saturating_add(started.elapsed()) >= limit
            })
        {
            status = CompletionStatus::TimeLimit;
            break;
        }

        let sweeps_now = mw_session.solver.completed_sweeps();
        // Evaluate the operational stop rule on deterministic sweep cadence.
        let stop_check_due = mw_session.stop_rule.is_some()
            && sweeps_now.is_multiple_of(mw_session.evaluation_cadence);
        let regular_evaluation_due = regular_evaluation_due(
            sweeps_now,
            mw_session.evaluation_cadence,
            mw_session.sweeps_target,
        );
        let mut quality_published = false;
        let mut stop_after_boundary = false;

        if stop_check_due {
            let stop_rule = mw_session
                .stop_rule
                .expect("stop_check_due requires a stop rule");
            let samples_before = stop_rule_state.samples;
            let check = session::run_stop_rule_check(
                &mw_session.solver,
                &stop_rule,
                &mut stop_rule_state,
                mw_session.game_config.seats.len(),
                mw_session.threads,
                mw_session.evaluation_seed,
            )?;

            let now = mw_session.solver.metrics();
            let drift = mw_session
                .solver
                .strategy_drift_refresh_compact(&mut prior)
                .context("refreshing multiway strategy drift")?;
            last_row = session::metrics_row(
                &now,
                drift,
                started.elapsed().as_secs_f64(),
                Some(&check.evaluation),
            );
            has_evaluation = true;
            if metrics_path.is_some()
                && let Some(observer) = observer.as_deref_mut()
            {
                observer(MultiwayRunObservation::Progress {
                    metrics: last_row.clone(),
                    point: ProgressPoint::Quality,
                })?;
            }

            if emit_progress && let Some(doubled) = check.samples_doubled_to {
                diagnostics(Diagnostic::SamplesDoubled {
                    max_width: check.max_width,
                    threshold: stop_rule.dev_gain_threshold,
                    before: samples_before,
                    after: doubled,
                });
            }
            publish_observation(&last_row, &mut observer)?;
            quality_published = true;
            last_quality_observation_sweeps = Some(sweeps_now);

            if check.converged {
                status = CompletionStatus::TargetReached;
                stop_after_boundary = true;
            }
        } else if regular_evaluation_due {
            let now = mw_session.solver.metrics();
            let evaluation = mw_session
                .solver
                .evaluate_average_profile(mw_session.evaluation_samples, mw_session.evaluation_seed)
                .context("evaluating held-out multiway profile")?;
            let drift = mw_session
                .solver
                .strategy_drift_refresh_compact(&mut prior)
                .context("refreshing multiway strategy drift")?;
            last_row = session::metrics_row(
                &now,
                drift,
                started.elapsed().as_secs_f64(),
                Some(&evaluation),
            );
            has_evaluation = true;
            if metrics_path.is_some()
                && let Some(observer) = observer.as_deref_mut()
            {
                observer(MultiwayRunObservation::Progress {
                    metrics: last_row.clone(),
                    point: ProgressPoint::Quality,
                })?;
            }
            if emit_progress {
                diagnostics(Diagnostic::Evaluation(now));
            }
            publish_observation(&last_row, &mut observer)?;
            quality_published = true;
            last_quality_observation_sweeps = Some(sweeps_now);
        }
        if quality_published {
            last_live_observation = Instant::now();
            published_live_observation = true;
        }

        let checkpoint_due =
            checkpoint_interval.is_some_and(|interval| last_checkpoint.elapsed() >= interval);
        if let Some(path) = checkpoint_path
            && checkpoint_due
            && !stop_after_boundary
        {
            write_checkpoint(
                &mw_session.solver,
                path,
                raw_config,
                &stop_rule_state,
                mw_session.evaluation_cadence,
                &started,
                cumulative_before,
            )?;
            last_checkpoint = Instant::now();
            if metrics_path.is_some()
                && let Some(observer) = observer.as_deref_mut()
            {
                let checkpoint_metrics = mw_session.solver.metrics();
                let elapsed_secs = Duration::from_millis(cumulative_before)
                    .saturating_add(started.elapsed())
                    .as_secs_f64();
                let checkpoint_event = checkpoint_progress_row(&checkpoint_metrics, elapsed_secs);
                observer(MultiwayRunObservation::Progress {
                    metrics: checkpoint_event,
                    point: ProgressPoint::Checkpoint,
                })?;
            }
            if let Some(observer) = observer.as_deref_mut() {
                observer(MultiwayRunObservation::Checkpoint(
                    MultiwayCheckpointObservation { sweeps: sweeps_now },
                ))?;
            }
        }

        if deferred_live_observation && !quality_published {
            publish_live_observation(
                &mw_session.solver,
                cumulative_before,
                &started,
                &mut observer,
            )?;
            last_live_observation = Instant::now();
            published_live_observation = true;
        }
        if stop_after_boundary {
            break;
        }
    }

    let final_checkpoint = final_checkpoint_path(status, checkpoint_path, output);
    if let Some(path) = final_checkpoint.as_deref() {
        write_checkpoint(
            &mw_session.solver,
            path,
            raw_config,
            &stop_rule_state,
            mw_session.evaluation_cadence,
            &started,
            cumulative_before,
        )?;
        if emit_progress && status == CompletionStatus::ResourceLimit {
            diagnostics(Diagnostic::ResourceCheckpoint(path.to_path_buf()));
        }
    }
    let final_metrics = mw_session.solver.metrics();
    if !has_evaluation || last_row.sweeps != final_metrics.sweeps {
        let evaluation = if status == CompletionStatus::Cancelled {
            None
        } else {
            Some(
                mw_session
                    .solver
                    .evaluate_average_profile(
                        mw_session.evaluation_samples,
                        mw_session.evaluation_seed,
                    )
                    .context("evaluating final held-out multiway profile")?,
            )
        };
        let drift = mw_session
            .solver
            .strategy_drift_refresh_compact(&mut prior)
            .context("refreshing final multiway strategy drift")?;
        last_row = session::metrics_row(
            &final_metrics,
            drift,
            started.elapsed().as_secs_f64(),
            evaluation.as_ref(),
        );
    }
    last_row.phase = match status {
        CompletionStatus::ResourceLimit => "resource_limit",
        CompletionStatus::SweepLimit => "sweep-limit",
        CompletionStatus::TargetReached => "target-reached",
        CompletionStatus::TimeLimit => "time-limit",
        CompletionStatus::Cancelled => "cancelled",
    }
    .to_string();
    if last_quality_observation_sweeps != Some(last_row.sweeps) {
        publish_observation(&last_row, &mut observer)?;
    }
    if let Some(observer) = &mut observer {
        observer(MultiwayRunObservation::Stop(MultiwayStopObservation {
            reason: last_row.phase.clone(),
            sweeps: last_row.sweeps,
        }))?;
    }
    if metrics_path.is_some()
        && let Some(observer) = observer
    {
        observer(MultiwayRunObservation::Progress {
            metrics: last_row.clone(),
            point: ProgressPoint::Final,
        })?;
    }

    // Drift is final. Release the previous profile before solution staging.
    // The stored block count is already available from the final metrics;
    // cancellation/resource stops and runs without a solution need no copy.
    drop(prior);
    let strategy_blocks = usize::try_from(final_metrics.infosets)
        .context("strategy block count exceeds the platform address space")?;
    if let Some(path) = mwsol_path
        && !matches!(
            status,
            CompletionStatus::ResourceLimit | CompletionStatus::Cancelled
        )
    {
        let snapshot = mw_session.solver.snapshot_state();
        let solution = session::make_solution(
            &mw_session.config_toml,
            mw_session.solver.abstraction_fingerprint(),
            mw_session.solver.configuration_fingerprint(),
            mw_session.solver.game(),
            &snapshot,
            &last_row,
        );
        // Solution encoding is independent of live and checkpoint f32 storage.
        let artifact_storage = match output_settings.probability_encoding {
            crate::input::ProbabilityEncoding::U16 => crate::mwsol::MwsolStorage::U16,
            crate::input::ProbabilityEncoding::F32 => crate::mwsol::MwsolStorage::F32,
        };
        crate::mwsol::write_mwsol_with(path, &solution, artifact_storage)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let effective_config = serde_json::to_value(raw_config.parse::<toml::Value>()?)?;
    let game_fingerprint = runfiles::config_hash_hex(&mw_session.solver.game().game_fingerprint());
    let algorithm_fingerprint =
        runfiles::config_hash_hex(&session::multiway_algorithm_fingerprint(&algorithm)?);
    let abstraction_fingerprint =
        runfiles::config_hash_hex(&mw_session.solver.abstraction_fingerprint());
    let configuration_fingerprint =
        runfiles::config_hash_hex(&mw_session.solver.configuration_fingerprint());
    let finished_unix_ms = unix_ms()?;

    let result = RunResult {
        schema_version: MULTIWAY_SCHEMA_VERSION,
        kind: "preflop-multiway",
        game_kind: Some("mw-preflop"),
        config_schema: Some("solvers.nlh/v1"),
        status,
        approximate_profile: true,
        approximation_notice: APPROXIMATION_NOTICE,
        sweeps: final_metrics.sweeps,
        traversals: final_metrics.traversals,
        infosets: final_metrics.infosets,
        memory_bytes: final_metrics.memory_bytes,
        policy_storage: if policy_allocation.is_some_and(|allocation| allocation.pages_committed) {
            "preallocated-all-current-street-buckets"
        } else {
            "research-compatibility"
        },
        preallocated_nodes: policy_allocation.map_or(0, |allocation| allocation.nodes),
        preallocated_columns: policy_allocation.map_or(0, |allocation| allocation.columns),
        preallocated_slots: policy_allocation.map_or(0, |allocation| allocation.slots),
        preallocated_bytes: policy_allocation.map_or(0, |allocation| allocation.bytes),
        preallocated_pages_committed: policy_allocation
            .is_some_and(|allocation| allocation.pages_committed),
        policy_arena_limit_bytes: mw_session.solver.config().max_memory_bytes,
        elapsed_secs: elapsed,
        traversals_per_second: if elapsed > 0.0 {
            final_metrics.traversals as f64 / elapsed
        } else {
            0.0
        },
        total_deal_attempts: final_metrics.total_deal_attempts,
        mean_deal_attempts: final_metrics.mean_deal_attempts,
        hand_updates: final_metrics.hand_updates,
        hand_updates_per_second: if elapsed > 0.0 {
            final_metrics.hand_updates as f64 / elapsed
        } else {
            0.0
        },
        seats: last_row.seats,
        strategy_blocks,
        config_hash: runfiles::config_hash_hex(&mw_session.config_hash),
        effective_config,
        game_fingerprint,
        abstraction_fingerprint,
        algorithm_fingerprint,
        configuration_fingerprint,
        profile_type: "linear-average-regret-minimized-profile",
        guarantee_boundary: APPROXIMATION_NOTICE,
        chip_unit_bb: 0.001,
        utility_unit,
        started_unix_ms,
        finished_unix_ms,
    };
    Ok(result)
}

fn unix_ms() -> Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock predates Unix epoch")?
        .as_millis();
    u64::try_from(millis).context("Unix timestamp does not fit u64")
}

fn validate_artifact_paths(
    output: Option<&Path>,
    metrics: Option<&Path>,
    checkpoint: Option<&Path>,
    mwsol: Option<&Path>,
) -> Result<()> {
    let implicit_checkpoint = checkpoint
        .is_none()
        .then(|| implicit_resource_checkpoint_path(output));
    let checkpoint = checkpoint.or(implicit_checkpoint.as_deref());
    let destinations = [
        ("result output", output),
        ("metrics", metrics),
        ("checkpoint", checkpoint),
        ("multiway solution", mwsol),
    ];
    let mut seen: Vec<(&str, PathBuf)> = Vec::new();
    for (label, path) in destinations {
        let Some(path) = path else {
            continue;
        };
        let identity = artifact_path_identity(path)?;
        if let Some((previous, _)) = seen
            .iter()
            .find(|(_, existing)| artifact_paths_equal(existing, &identity))
        {
            return Err(anyhow!(
                "artifact destinations must be distinct: {previous} and {label} both resolve to {}",
                identity.display()
            ));
        }
        seen.push((label, identity));
    }
    Ok(())
}

fn implicit_resource_checkpoint_path(output: Option<&Path>) -> PathBuf {
    output.map_or_else(
        || PathBuf::from("multiway-resource-limit.mwckpt"),
        |path| path.with_extension("mwckpt"),
    )
}

fn artifact_path_identity(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("resolving the current directory for artifact paths")?
            .join(path)
    };
    let normalized = lexical_normalize(&absolute);
    if normalized.exists() {
        return std::fs::canonicalize(&normalized)
            .with_context(|| format!("resolving artifact path {}", normalized.display()));
    }
    if let (Some(parent), Some(file_name)) = (normalized.parent(), normalized.file_name())
        && let Ok(parent) = std::fs::canonicalize(parent)
    {
        return Ok(parent.join(file_name));
    }
    Ok(normalized)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn artifact_paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn final_checkpoint_path<'a>(
    status: CompletionStatus,
    explicit: Option<&'a Path>,
    output: Option<&Path>,
) -> Option<Cow<'a, Path>> {
    if let Some(path) = explicit {
        return Some(Cow::Borrowed(path));
    }
    (status == CompletionStatus::ResourceLimit)
        .then(|| Cow::Owned(implicit_resource_checkpoint_path(output)))
}

fn write_checkpoint<A: crate::MultiwayAbstraction>(
    solver: &MultiwaySolver<HoldemGame<A>>,
    path: &Path,
    raw_config: &str,
    stop_state: &session::StopRuleState,
    evaluation_cadence: u64,
    started: &Instant,
    cumulative_before: u64,
) -> Result<()> {
    let current = solver.completed_sweeps();
    let runtime = crate::checkpoint::CheckpointRuntimeState {
        confirmations_met: stop_state.confirmations_met,
        next_evaluation_sweep: current
            .saturating_add(session::distance_to_boundary(current, evaluation_cadence)),
        evaluation_samples: stop_state.samples,
        evaluation_sequence: stop_state.eval_index,
        cumulative_solve_millis: cumulative_before
            .saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
    };
    crate::checkpoint::MultiwayCheckpoint::write_solver_atomic(solver, path, raw_config, runtime)
        .with_context(|| format!("writing {}", path.display()))
}

fn checkpoint_progress_row(
    metrics: &crate::SolverMetrics,
    elapsed_secs: f64,
) -> MultiwayMetricsRow {
    let mut row = session::metrics_row(
        metrics,
        vec![0.0; metrics.average_positive_regret.len()],
        elapsed_secs,
        None,
    );
    row.phase = "checkpoint".into();
    row
}

fn publish_observation(
    metrics: &MultiwayMetricsRow,
    observer: &mut Option<&mut dyn FnMut(MultiwayRunObservation) -> Result<()>>,
) -> Result<()> {
    let Some(observer) = observer.as_deref_mut() else {
        return Ok(());
    };
    observer(MultiwayRunObservation::Quality(
        MultiwayQualityObservation {
            metrics: metrics.clone(),
        },
    ))?;
    Ok(())
}

fn publish_live_observation(
    solver: &MultiwaySolver<HoldemGame<crate::MultiwayAbstractionBackend>>,
    cumulative_before: u64,
    started: &Instant,
    observer: &mut Option<&mut dyn FnMut(MultiwayRunObservation) -> Result<()>>,
) -> Result<()> {
    let Some(observer) = observer.as_deref_mut() else {
        return Ok(());
    };
    observer(MultiwayRunObservation::Live(MultiwayLiveObservation {
        sweeps: solver.completed_sweeps(),
        traversals: solver.traversals(),
        hand_updates: solver.hand_updates(),
        elapsed_secs: Duration::from_millis(cumulative_before)
            .saturating_add(started.elapsed())
            .as_secs_f64(),
    }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_boundaries_are_prioritized_and_checkpoint_needs_a_sink() {
        let second = Duration::from_secs(1);
        assert_eq!(
            operational_boundary(true, second, Some(second), second, Some(second), true,),
            Some(OperationalBoundary::Cancelled)
        );
        assert_eq!(
            operational_boundary(false, second, Some(second), second, Some(second), true,),
            Some(OperationalBoundary::TimeLimit)
        );
        assert_eq!(
            operational_boundary(false, Duration::ZERO, None, second, Some(second), true),
            Some(OperationalBoundary::CheckpointDue)
        );
        assert_eq!(
            operational_boundary(false, Duration::ZERO, None, second, Some(second), false),
            None
        );
        assert_eq!(
            operational_boundary(
                false,
                Duration::ZERO,
                Some(second),
                second - Duration::from_nanos(1),
                Some(second),
                true,
            ),
            None
        );
    }

    #[test]
    fn checkpoint_progress_uses_current_counters_without_quality_estimates() {
        let metrics = crate::SolverMetrics {
            sweeps: 12,
            traversals: 72,
            infosets: 34,
            memory_bytes: 56,
            total_deal_attempts: 90,
            mean_deal_attempts: 1.25,
            hand_updates: 789,
            average_positive_regret: vec![1.5, 2.5],
        };
        let row = checkpoint_progress_row(&metrics, 4.0);
        assert_eq!(row.phase, "checkpoint");
        assert_eq!(row.sweeps, 12);
        assert_eq!(row.traversals, 72);
        assert_eq!(row.infosets, 34);
        assert_eq!(row.memory_bytes, 56);
        assert_eq!(row.elapsed_secs, 4.0);
        assert_eq!(row.traversals_per_second, 18.0);
        assert_eq!(row.hand_updates, 789);
        assert_eq!(row.hand_updates_per_second, 197.25);
        assert_eq!(row.seats.len(), 2);
        assert!(row.seats.iter().all(|seat| {
            seat.profile_ev.is_none()
                && seat.deviation_gain_lower_bound.is_none()
                && seat.strategy_drift_l1 == 0.0
        }));
        assert_eq!(row.seats[0].average_positive_regret, 1.5);
        assert_eq!(row.seats[1].average_positive_regret, 2.5);
        assert!(!regular_evaluation_due(12, 64, 100));
        assert!(regular_evaluation_due(64, 64, 100));
        assert!(regular_evaluation_due(100, 64, 100));
    }

    #[test]
    fn implicit_checkpoint_is_reserved_for_resource_limits() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("profile.json");
        let expected = dir.path().join("profile.mwckpt");

        assert_eq!(
            final_checkpoint_path(CompletionStatus::ResourceLimit, None, Some(&output)).as_deref(),
            Some(expected.as_path())
        );
        assert_eq!(
            final_checkpoint_path(CompletionStatus::ResourceLimit, None, None).as_deref(),
            Some(Path::new("multiway-resource-limit.mwckpt"))
        );
        assert!(final_checkpoint_path(CompletionStatus::SweepLimit, None, Some(&output)).is_none());
        assert!(final_checkpoint_path(CompletionStatus::Cancelled, None, Some(&output)).is_none());
        let explicit = dir.path().join("explicit.mwckpt");
        assert_eq!(
            final_checkpoint_path(CompletionStatus::SweepLimit, Some(&explicit), Some(&output))
                .as_deref(),
            Some(explicit.as_path())
        );
    }

    #[test]
    fn artifact_destinations_must_be_pairwise_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let paths = [
            dir.path().join("result.json"),
            dir.path().join("metrics.jsonl"),
            dir.path().join("state.mwckpt"),
            dir.path().join("strategy.mwsol"),
        ];
        validate_artifact_paths(
            Some(&paths[0]),
            Some(&paths[1]),
            Some(&paths[2]),
            Some(&paths[3]),
        )
        .unwrap();

        for left in 0..paths.len() {
            for right in (left + 1)..paths.len() {
                let mut selected = [
                    paths[0].as_path(),
                    paths[1].as_path(),
                    paths[2].as_path(),
                    paths[3].as_path(),
                ];
                selected[right] = selected[left];
                let error = validate_artifact_paths(
                    Some(selected[0]),
                    Some(selected[1]),
                    Some(selected[2]),
                    Some(selected[3]),
                )
                .unwrap_err();
                assert!(
                    error
                        .to_string()
                        .contains("artifact destinations must be distinct"),
                    "pair {left}/{right}: {error:#}"
                );
            }
        }
    }

    #[test]
    fn artifact_destination_aliases_and_implicit_checkpoint_collisions_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("nested").join("..").join("result.json");
        let metrics = dir.path().join("result.json");
        let checkpoint = dir.path().join("state.mwckpt");
        assert!(
            validate_artifact_paths(Some(&output), Some(&metrics), Some(&checkpoint), None)
                .is_err()
        );

        let output = dir.path().join("result.json");
        let implicit = dir.path().join("result.mwckpt");
        assert!(validate_artifact_paths(Some(&output), Some(&implicit), None, None).is_err());

        let output = dir.path().join("result.mwckpt");
        assert!(validate_artifact_paths(Some(&output), None, None, None).is_err());
    }
    #[test]
    fn typed_stop_rule_distinguishes_target_reached_from_sweep_limit() {
        let smoke = include_str!("../tests/fixtures/preflop_multiway_v1_3max_smoke.toml");
        for (confirmations, expected_sweeps, expected_status) in
            [(1, 1, "target-reached"), (5, 2, "sweep-limit")]
        {
            let raw = smoke.replace(
                "confirmations = 1",
                &format!("confirmations = {confirmations}"),
            );
            let p = crate::prepare::prepare(&raw, Path::new("smoke.toml")).unwrap();
            let directory = tempfile::tempdir().unwrap();
            let output = directory.path().join("result.json");
            let mut quality = Vec::new();
            let summary = run(
                RunRequest {
                    raw_config: &p.effective,
                    config: p.lowered,
                    output: Some(&output),
                    metrics_path: None,
                    checkpoint_path: None,
                    config_hash: runfiles::config_hash(p.effective.as_bytes()),
                    mwsol_path: None,
                    cancel: None,
                    cache_root: None,
                    emit_progress: false,
                },
                &mut |observation| {
                    if let MultiwayRunObservation::Quality(o) = observation {
                        quality.push(o.metrics.sweeps);
                    }
                    Ok(())
                },
                &mut |_| {},
            )
            .unwrap();
            let result = serde_json::to_value(summary).unwrap();
            assert_eq!(result["status"], expected_status);
            assert_eq!(result["sweeps"], expected_sweeps);
            assert_eq!(quality.iter().filter(|s| **s == expected_sweeps).count(), 1);
        }
    }
}
