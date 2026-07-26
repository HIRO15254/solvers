use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use formats::{MULTIWAY_SCHEMA_VERSION, MultiwayMetricsRow, MultiwayMetricsWriter};
use multiway::solver::{HistoryKey, InfoKey};
use multiway::{ExternalSamplingGame, HoldemGame, MultiwaySolver};
use serde::Serialize;

use crate::config::{GameSection, SolveConfig, StorageKind, UtilitySection};
use crate::session;

const APPROXIMATION_NOTICE: &str = "3人以上は多人数・一般和ゲームのregret-minimized approximationです。Nash/GTO保証やexploitability指標ではありません。";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompletionStatus {
    Completed,
    ResourceLimit,
    #[serde(rename = "sweep-limit")]
    SweepLimit,
    #[serde(rename = "target-reached")]
    TargetReached,
    #[serde(rename = "time-limit")]
    TimeLimit,
    Cancelled,
    /// The convergence stop rule (`run.stop_dev_gain`) fired: the maximum
    /// per-seat held-out deviation-gain-lower-bound CI upper bound stayed
    /// below the configured threshold for `run.stop_confirmations`
    /// consecutive wall-clock-spaced evaluations. See
    /// `crate::session::StopRule`.
    Converged,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultV2 {
    schema_version: u16,
    kind: &'static str,
    status: CompletionStatus,
    approximate_profile: bool,
    approximation_notice: &'static str,
    sweeps: u64,
    traversals: u64,
    infosets: u64,
    memory_bytes: u64,
    policy_storage: &'static str,
    preallocated_nodes: u64,
    preallocated_columns: u64,
    preallocated_slots: u64,
    preallocated_bytes: u64,
    preallocated_pages_committed: bool,
    policy_arena_limit_bytes: u64,
    elapsed_secs: f64,
    traversals_per_second: f64,
    total_deal_attempts: u64,
    mean_deal_attempts: f64,
    /// See `multiway::solver::SolverState::hand_updates`. Compare against a
    /// range-based solver's "hands/s".
    hand_updates: u64,
    hand_updates_per_second: f64,
    seats: Vec<formats::MultiwaySeatMetrics>,
    strategy_blocks: usize,
    config_hash: String,
    effective_config: serde_json::Value,
    game_fingerprint: String,
    abstraction_fingerprint: String,
    algorithm_fingerprint: String,
    configuration_fingerprint: String,
    profile_type: &'static str,
    guarantee_boundary: &'static str,
    chip_unit_bb: f64,
    utility_unit: &'static str,
    started_unix_ms: u64,
    finished_unix_ms: u64,
}

/// One visited root information set in a live, linear-average strategy
/// observation. Missing root buckets remain absent instead of being
/// synthesized as a uniform strategy.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRootStrategyEntry {
    pub key: InfoKey,
    pub actions: Vec<String>,
    pub probabilities: Vec<f32>,
    pub weight: f64,
}

/// Owned data published at completed evaluation boundaries.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiwayRunObservation {
    pub metrics: MultiwayMetricsRow,
    pub root_strategy: Vec<LiveRootStrategyEntry>,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    raw_config: &str,
    config: SolveConfig,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: Option<&Path>,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    emit_progress: bool,
) -> Result<()> {
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        checkpoint_path,
        config_hash,
        mwsol_path,
        cancel,
        None,
        false,
        emit_progress,
        None,
    )
}

/// Starts a production solve and publishes owned progress observations.
#[allow(clippy::too_many_arguments)]
pub fn run_observed(
    raw_config: &str,
    config: SolveConfig,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: Option<&Path>,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    emit_progress: bool,
    observer: &mut dyn FnMut(MultiwayRunObservation),
) -> Result<()> {
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        checkpoint_path,
        config_hash,
        mwsol_path,
        cancel,
        None,
        false,
        emit_progress,
        Some(observer),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn resume(
    raw_config: &str,
    config: SolveConfig,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: &Path,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    reset_confirmations: bool,
    emit_progress: bool,
) -> Result<()> {
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        Some(checkpoint_path),
        config_hash,
        mwsol_path,
        cancel,
        Some(checkpoint_path),
        reset_confirmations,
        emit_progress,
        None,
    )
}

/// Resumes a production solve and publishes owned progress observations.
#[allow(clippy::too_many_arguments)]
pub fn resume_observed(
    raw_config: &str,
    config: SolveConfig,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: &Path,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    reset_confirmations: bool,
    emit_progress: bool,
    observer: &mut dyn FnMut(MultiwayRunObservation),
) -> Result<()> {
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        Some(checkpoint_path),
        config_hash,
        mwsol_path,
        cancel,
        Some(checkpoint_path),
        reset_confirmations,
        emit_progress,
        Some(observer),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    raw_config: &str,
    config: SolveConfig,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: Option<&Path>,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    resume_checkpoint: Option<&Path>,
    reset_confirmations: bool,
    emit_progress: bool,
    mut observer: Option<&mut dyn FnMut(MultiwayRunObservation)>,
) -> Result<()> {
    validate_artifact_paths(output, metrics_path, checkpoint_path, mwsol_path)?;
    if !matches!(config.game, GameSection::PreflopMultiway(_)) {
        return Err(anyhow!(
            "multiway solve path requires kind = \"preflop-multiway\""
        ));
    }
    // Canonical v1 TOML is already materialized with all supported CLI
    // overrides before this function is called. Keep it in that schema:
    // lowering `discount.kind = "none"` deliberately uses `u64::MAX` in the
    // runtime struct, which TOML cannot represent and therefore must not be
    // round-tripped through the legacy shared schema. Historical research
    // configs can still carry a legacy `--iterations` override in `config`,
    // so only that path is re-serialized.
    let effective_toml = if crate::multiway_v1::has_v1_schema(raw_config)? {
        raw_config.to_owned()
    } else {
        toml::to_string(&config).context("re-serializing the effective multiway config")?
    };
    #[cfg(not(any(feature = "research", test)))]
    let mut mw_session =
        session::build_production_multiway_session(&effective_toml, resume_checkpoint)?;
    #[cfg(any(feature = "research", test))]
    let mut mw_session = session::build_multiway_session(&effective_toml, resume_checkpoint)?;
    mw_session.config_toml = raw_config.to_string();
    mw_session.config_hash = config_hash;
    let policy_allocation = mw_session.solver.policy_arena_allocation();
    #[cfg(not(any(feature = "research", test)))]
    let policy_allocation = Some(
        policy_allocation
            .filter(|allocation| allocation.pages_committed)
            .ok_or_else(|| {
                anyhow!("production solver has no page-committed preallocated policy arena")
            })?,
    );
    if emit_progress && let Some(policy_allocation) = policy_allocation {
        eprintln!(
            "policy arena committed before sweep 0: nodes={} columns={} slots={} bytes={}",
            policy_allocation.nodes,
            policy_allocation.columns,
            policy_allocation.slots,
            policy_allocation.bytes
        );
    }

    // One sweep only yields `seats` parallel traversals, so the machine is
    // undersubscribed whenever `seats x sweep_batch < threads`. Purely a
    // hint: changing `run.sweep_batch` changes results (see the sweep-batch
    // docs), so it is never adjusted silently.
    if emit_progress {
        let seats = mw_session.game_config.seats.len().max(1) as u64;
        let sweep_batch = mw_session.solver.config().sweep_batch.max(1);
        let threads = mw_session.threads as u64;
        if seats * sweep_batch < threads {
            eprintln!(
                "hint: {seats} seats x run.sweep_batch {sweep_batch} = {} parallel traversals \
                 < {threads} threads; run.sweep_batch = {} would use every core",
                seats * sweep_batch,
                threads.div_ceil(seats)
            );
        }
    }

    let mut metrics_writer = metrics_path
        .map(MultiwayMetricsWriter::create_or_append)
        .transpose()
        .context("opening multiway metrics")?;
    let started_unix_ms = unix_ms()?;
    let started = Instant::now();
    // Seeds `prior` with the current averages (the drift result is
    // discarded), so a resumed run's first drift row measures movement since
    // the checkpoint rather than since an empty profile.
    let mut prior: HashMap<InfoKey, Vec<f32>> = HashMap::new();
    mw_session.solver.strategy_drift_refresh(&mut prior);
    let mut last_row = MultiwayMetricsRow::sampling(mw_session.game_config.seats.len());
    let is_v1 = crate::multiway_v1::has_v1_schema(raw_config).unwrap_or(false);
    let checkpoint_interval = if is_v1 {
        Some(Duration::from_secs(
            crate::multiway_v1::checkpoint_interval_secs(raw_config)?,
        ))
    } else {
        None
    };
    let mut last_checkpoint = Instant::now();
    let mut status = if is_v1 {
        CompletionStatus::SweepLimit
    } else {
        CompletionStatus::Completed
    };
    let max_time = if is_v1 {
        crate::multiway_v1::max_time_secs(raw_config)?.map(Duration::from_secs)
    } else {
        None
    };
    let mut has_evaluation = false;
    // Convergence stop rule (`run.stop_dev_gain`) state; see
    // `session::StopRuleState` and the stop-rule block inside the drive loop
    // below for why this check is wall-clock-driven rather than
    // cadence-driven: with `stop_dev_gain` set, `sweeps_target` is a safety
    // cap, not a target, so the sweep count a converged run actually stops
    // at is machine-dependent (documented on `run.stop_dev_gain`).
    let mut stop_rule_state = session::StopRuleState::new(mw_session.evaluation_samples);
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
        if let Some(writer) = metrics_writer.as_mut() {
            writer
                .append(&last_row)
                .context("writing resume progress event")?;
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
        let checkpoint_delta = mw_session
            .checkpoint_every
            .map(|cadence| session::distance_to_boundary(current, cadence))
            .unwrap_or(u64::MAX);
        let chunk = (mw_session.sweeps_target - current)
            .min(evaluation_delta)
            .min(checkpoint_delta)
            .max(1);
        match mw_session
            .solver
            .run_sweeps_with_threads_until(chunk, mw_session.threads, || {
                !cancel.is_some_and(|token| token.load(Ordering::Relaxed))
            }) {
            Ok(completed) => {
                if completed < chunk {
                    status = CompletionStatus::Cancelled;
                    break;
                }
            }
            Err(multiway::solver::SolverError::MemoryLimit { .. }) => {
                status = CompletionStatus::ResourceLimit;
                break;
            }
            Err(error) => return Err(error).context("running multiway MCCFR"),
        }
        if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
            status = CompletionStatus::Cancelled;
            break;
        }

        let sweeps_now = mw_session.solver.completed_sweeps();
        if sweeps_now % mw_session.evaluation_cadence == 0 || sweeps_now == mw_session.sweeps_target
        {
            let now = mw_session.solver.metrics();
            let evaluation = mw_session
                .solver
                .evaluate_average_profile(mw_session.evaluation_samples, mw_session.evaluation_seed)
                .context("evaluating held-out multiway profile")?;
            let drift = mw_session.solver.strategy_drift_refresh(&mut prior);
            last_row = session::metrics_row(
                &now,
                drift,
                started.elapsed().as_secs_f64(),
                Some(&evaluation),
            );
            has_evaluation = true;
            if let Some(writer) = metrics_writer.as_mut() {
                writer
                    .append(&last_row)
                    .context("writing multiway metrics")?;
            }
            if emit_progress {
                eprintln!(
                    "sweeps={:>8} traversals={:>10} infosets={:>9} regret_proxy={:.3e} memory={}MiB",
                    now.sweeps,
                    now.traversals,
                    now.infosets,
                    mean(&now.average_positive_regret),
                    now.memory_bytes / (1024 * 1024),
                );
            }
            publish_observation(&mw_session, &last_row, &mut observer);
        }
        let checkpoint_due = mw_session
            .checkpoint_every
            .is_some_and(|cadence| sweeps_now % cadence == 0)
            || checkpoint_interval.is_some_and(|interval| last_checkpoint.elapsed() >= interval);
        if let Some(path) = checkpoint_path
            && checkpoint_due
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
            if let Some(writer) = metrics_writer.as_mut() {
                let mut checkpoint_event = last_row.clone();
                checkpoint_event.phase = "checkpoint".into();
                writer
                    .append(&checkpoint_event)
                    .context("writing checkpoint progress event")?;
            }
        }

        // v1 evaluates the operational stop rule on deterministic sweep
        // cadence. Legacy configs retain their documented wall-clock trigger.
        if let Some(stop_rule) = mw_session.stop_rule
            && ((is_v1 && sweeps_now % mw_session.evaluation_cadence == 0)
                || (!is_v1
                    && stop_rule_state.last_eval.elapsed().as_secs_f64()
                        >= stop_rule.eval_period_secs))
        {
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
            let drift = mw_session.solver.strategy_drift_refresh(&mut prior);
            last_row = session::metrics_row(
                &now,
                drift,
                started.elapsed().as_secs_f64(),
                Some(&check.evaluation),
            );
            has_evaluation = true;
            if let Some(writer) = metrics_writer.as_mut() {
                writer
                    .append(&last_row)
                    .context("writing multiway metrics")?;
            }

            if emit_progress && let Some(doubled) = check.samples_doubled_to {
                eprintln!(
                    "stop-rule: max CI width {:.6} exceeds threshold {:.6}; \
                     doubling evaluation samples {samples_before} -> {doubled}",
                    check.max_width, stop_rule.dev_gain_threshold
                );
            }
            publish_observation(&mw_session, &last_row, &mut observer);

            if check.converged {
                status = if is_v1 {
                    CompletionStatus::TargetReached
                } else {
                    CompletionStatus::Converged
                };
                break;
            }
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
            eprintln!("resource_limit checkpoint: {}", path.display());
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
        let drift = mw_session.solver.strategy_drift_refresh(&mut prior);
        last_row = session::metrics_row(
            &final_metrics,
            drift,
            started.elapsed().as_secs_f64(),
            evaluation.as_ref(),
        );
    }
    last_row.phase = match status {
        CompletionStatus::Completed => "completed",
        CompletionStatus::ResourceLimit => "resource_limit",
        CompletionStatus::SweepLimit => "sweep-limit",
        CompletionStatus::TargetReached => "target-reached",
        CompletionStatus::TimeLimit => "time-limit",
        CompletionStatus::Cancelled => "cancelled",
        CompletionStatus::Converged => "converged",
    }
    .to_string();
    publish_observation(&mw_session, &last_row, &mut observer);
    if let Some(writer) = metrics_writer.as_mut() {
        writer
            .append(&last_row)
            .context("writing final multiway metrics")?;
    }

    let snapshot = mw_session.solver.snapshot_state();
    if let Some(path) = mwsol_path
        && !matches!(
            status,
            CompletionStatus::ResourceLimit | CompletionStatus::Cancelled
        )
    {
        let solution = session::make_solution(
            &mw_session.config_toml,
            mw_session.solver.abstraction_fingerprint(),
            mw_session.solver.configuration_fingerprint(),
            mw_session.solver.game(),
            &snapshot,
            &last_row,
        );
        // `run.storage` selects the artifact encoding only; the live MCCFR
        // state and `.mwckpt` checkpoints stay f32 regardless.
        let artifact_storage = if is_v1 {
            match crate::multiway_v1::probability_encoding(raw_config)? {
                crate::multiway_v1::ProbabilityEncoding::U16 => formats::MwsolStorage::U16,
                crate::multiway_v1::ProbabilityEncoding::F32 => formats::MwsolStorage::F32,
            }
        } else {
            match mw_session.storage {
                StorageKind::F32 => formats::MwsolStorage::F32,
                StorageKind::I16 => formats::MwsolStorage::I16,
            }
        };
        formats::write_mwsol_with(path, &solution, artifact_storage)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let effective = crate::config::parse_solve_config(raw_config)?;
    let algorithm_material = serde_json::to_vec(&effective.algorithm)?;
    let effective_config = if is_v1 {
        crate::multiway_v1::normalized_config(raw_config)?
    } else {
        serde_json::to_value(&effective)?
    };
    let utility_unit = match &effective.utility {
        UtilitySection::ChipEv => "bb",
        UtilitySection::TournamentIcm { .. } | UtilitySection::Icm { .. } => "prize",
    };
    let game_fingerprint = formats::config_hash_hex(&mw_session.solver.game().game_fingerprint());
    let algorithm_fingerprint =
        formats::config_hash_hex(&formats::config_hash(algorithm_material.as_slice()));
    let abstraction_fingerprint =
        formats::config_hash_hex(&mw_session.solver.abstraction_fingerprint());
    let configuration_fingerprint =
        formats::config_hash_hex(&mw_session.solver.configuration_fingerprint());
    let finished_unix_ms = unix_ms()?;

    let result = ResultV2 {
        schema_version: MULTIWAY_SCHEMA_VERSION,
        kind: "preflop-multiway",
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
        strategy_blocks: snapshot.policies.len(),
        config_hash: formats::config_hash_hex(&mw_session.config_hash),
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
    let json = serde_json::to_string_pretty(&result)?;
    if let Some(path) = output {
        write_atomic(path, format!("{json}\n").as_bytes())
            .with_context(|| format!("atomically writing {}", path.display()))?;
    } else {
        println!("{json}");
    }
    if emit_progress {
        let exit_code = match status {
            CompletionStatus::ResourceLimit => 75,
            CompletionStatus::Cancelled => 130,
            _ => 0,
        };
        crate::CLI_EXIT_CODE.store(exit_code, Ordering::SeqCst);
    }
    Ok(())
}
fn unix_ms() -> Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock predates Unix epoch")?
        .as_millis();
    u64::try_from(millis).context("Unix timestamp does not fit u64")
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(contents)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| anyhow!(error.error))?;
    Ok(())
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

fn write_checkpoint<A: multiway::MultiwayAbstraction>(
    solver: &MultiwaySolver<HoldemGame<A>>,
    path: &Path,
    raw_config: &str,
    stop_state: &session::StopRuleState,
    evaluation_cadence: u64,
    started: &Instant,
    cumulative_before: u64,
) -> Result<()> {
    let current = solver.completed_sweeps();
    let runtime = multiway::checkpoint::CheckpointRuntimeState {
        confirmations_met: stop_state.confirmations_met,
        next_evaluation_sweep: current
            .saturating_add(session::distance_to_boundary(current, evaluation_cadence)),
        evaluation_samples: stop_state.samples,
        evaluation_sequence: stop_state.eval_index,
        cumulative_solve_millis: cumulative_before
            .saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
    };
    multiway::checkpoint::MultiwayCheckpoint::capture(solver)
        .with_runtime_metadata(raw_config, runtime)
        .write_atomic(path)
        .with_context(|| format!("writing {}", path.display()))
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn publish_observation(
    session: &session::MultiwaySession,
    metrics: &MultiwayMetricsRow,
    observer: &mut Option<&mut dyn FnMut(MultiwayRunObservation)>,
) {
    let Some(observer) = observer.as_deref_mut() else {
        return;
    };
    let root_strategy = session
        .solver
        .strategies_at_with_mass(HistoryKey::ROOT)
        .into_iter()
        .map(
            |(key, actions, probabilities, weight)| LiveRootStrategyEntry {
                key,
                actions,
                probabilities,
                weight,
            },
        )
        .collect();
    observer(MultiwayRunObservation {
        metrics: metrics.clone(),
        root_strategy,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiway_example_parses_the_public_contract() {
        let raw = include_str!("../../../examples/preflop_multiway_9max.toml");
        let parsed: SolveConfig = toml::from_str(raw).unwrap();
        assert!(matches!(parsed.game, GameSection::PreflopMultiway(_)));
    }

    /// Splices `stop_dev_gain`/`stop_confirmations`/`stop_eval_period_secs`
    /// into the 3-max smoke config's `[run]` table (right after its last
    /// key) and overrides `sweeps` to `cap`, so the stop rule's cap acts as
    /// a safety net rather than the actual target.
    fn smoke_with_stop_rule(cap: u64, extra_stop_keys: &str) -> String {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let with_cap = raw.replacen("sweeps = 2\n", &format!("sweeps = {cap}\n"), 1);
        assert_ne!(with_cap, raw, "the sweeps anchor must have matched");
        let spliced = with_cap.replacen(
            "evaluation_cadence = 1\n",
            &format!("evaluation_cadence = 1\n{extra_stop_keys}"),
            1,
        );
        assert_ne!(
            spliced, with_cap,
            "the run-section anchor must have matched"
        );
        spliced
    }

    fn run_to_json(raw: &str, config: SolveConfig) -> serde_json::Value {
        let config_hash = formats::config_hash(raw.as_bytes());
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("result.json");
        run(
            raw,
            config,
            Some(&output),
            None,
            None,
            config_hash,
            None,
            None,
            false,
        )
        .expect("multiway run should succeed");
        serde_json::from_str(&std::fs::read_to_string(&output).unwrap())
            .expect("result output must be valid JSON")
    }

    /// A very lax threshold (1000 bb, far above anything a 2bb-effective-stack
    /// smoke table could ever produce) with a single required confirmation
    /// and a near-zero wall-clock evaluation period must converge on the
    /// very first stop-rule evaluation, stopping well short of the (high)
    /// sweep cap with status `"converged"`.
    #[test]
    fn stop_dev_gain_converges_before_the_sweep_cap_with_a_lax_threshold() {
        // 50 best-response training traversals per seat is small enough to keep
        // the test fast while genuinely exercising the training +
        // evaluate_profile path.
        let raw = smoke_with_stop_rule(
            100_000,
            "stop_dev_gain = 1000.0\nstop_confirmations = 1\nstop_eval_period_secs = 0.01\n\
             stop_br_traversals = 50\n",
        );
        let config: SolveConfig = toml::from_str(&raw).expect("parse spliced config");
        let result = run_to_json(&raw, config);
        assert_eq!(result["status"], "converged");
        let sweeps = result["sweeps"].as_u64().unwrap();
        assert!(
            sweeps < 100_000,
            "expected an early stop, got {sweeps} sweeps"
        );
    }

    /// The packaged 3-max smoke fixture, unmodified, is a degenerate
    /// fold/shove-only preflop tree (2bb stacks, `bet_sizes`/`raise_sizes`
    /// emptied out on every street): its regret-greedy deviation collapses
    /// onto the average strategy almost immediately, so its real
    /// deviation-gain estimate reaches even an astronomically tight
    /// threshold within a handful of sweeps -- too easy a target to exercise
    /// "the threshold is unreachable". This restores real preflop bet/raise
    /// sizing (a bigger, 20bb stack and the crate's normal default preflop
    /// sizes) while keeping every postflop street exactly as
    /// check-down-only as the smoke fixture (so the tree, and therefore the
    /// test, stays fast): a real held-out deviation-gain upper bound on this
    /// richer preflop tree stays in the several-bb range for many sweeps
    /// (measured 5-16 bb at 20 sweeps across 8-128 evaluation samples).
    fn smoke_with_richer_preflop_and_stop_rule(
        cap: u64,
        stop_dev_gain: f64,
        stop_confirmations: u32,
    ) -> String {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let raw = raw.replace("stack_bb = 2.0", "stack_bb = 20.0");
        let raw = raw.replacen(
            "[game.betting.preflop]\nbet_sizes = []\nraise_sizes = []\nmax_aggressive_actions = 1\ninclude_allin = true\n",
            "[game.betting.preflop]\nbet_sizes = [{ kind = \"to-bb\", value = 2.5 }]\nraise_sizes = [{ kind = \"previous-bet-multiple\", factor = 3.0 }]\nmax_aggressive_actions = 4\ninclude_allin = true\n",
            1,
        );
        let with_cap = raw.replacen("sweeps = 2\n", &format!("sweeps = {cap}\n"), 1);
        assert_ne!(with_cap, raw, "the sweeps anchor must have matched");
        let spliced = with_cap.replacen(
            "evaluation_cadence = 1\n",
            &format!(
                "evaluation_cadence = 1\nstop_dev_gain = {stop_dev_gain}\nstop_confirmations = {stop_confirmations}\nstop_eval_period_secs = 0.01\n"
            ),
            1,
        );
        assert_ne!(
            spliced, with_cap,
            "the run-section anchor must have matched"
        );
        spliced
    }

    /// Validation forbids a literal zero threshold, and an astronomically
    /// tight one (e.g. `1e-9`) is unreachable for the *wrong* reason in an
    /// automated test: the adaptive sample-doubling rule (see the drive
    /// loop in `run_inner`) keeps doubling the evaluation sample count
    /// whenever the CI is too wide to ever settle below the threshold,
    /// which for `1e-9` runs all the way to the `65_536`-sample cap and
    /// repeats at that cost every period -- correct behavior, but far too
    /// slow for a unit test.
    ///
    /// Rather than lean on a fragile numeric threshold (a 20-sweep MCCFR
    /// run's exact deviation-gain estimate depends on sampling noise that
    /// can occasionally dip under even a several-bb threshold), this makes
    /// "unreachable" *structural*: `stop_confirmations` is set higher than
    /// the sweep cap itself, so no sequence of per-sweep stop-rule
    /// evaluations (at most one per sweep, since `evaluation_cadence = 1`)
    /// could possibly accumulate enough *consecutive* passes to reach it,
    /// regardless of what any individual evaluation reports. The run must
    /// therefore exhaust its (small) sweep cap with status `"completed"`.
    #[test]
    fn stop_dev_gain_runs_to_the_cap_when_the_threshold_is_unreachable() {
        let cap = 20;
        let raw = smoke_with_richer_preflop_and_stop_rule(cap, 2.0, cap as u32 + 5);
        let config: SolveConfig = toml::from_str(&raw).expect("parse spliced config");
        let result = run_to_json(&raw, config);
        assert_eq!(result["status"], "completed");
        assert_eq!(result["sweeps"].as_u64().unwrap(), cap);
    }

    #[test]
    fn three_player_smoke_contract_parses() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let parsed: SolveConfig = toml::from_str(raw).unwrap();
        assert!(matches!(parsed.game, GameSection::PreflopMultiway(_)));
        assert_eq!(parsed.run.sweeps, Some(2));
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
        assert!(final_checkpoint_path(CompletionStatus::Completed, None, Some(&output)).is_none());
        assert!(final_checkpoint_path(CompletionStatus::Cancelled, None, Some(&output)).is_none());
        let explicit = dir.path().join("explicit.mwckpt");
        assert_eq!(
            final_checkpoint_path(CompletionStatus::Completed, Some(&explicit), Some(&output))
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
}
