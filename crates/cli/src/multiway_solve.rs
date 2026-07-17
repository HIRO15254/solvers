use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use formats::{MULTIWAY_SCHEMA_VERSION, MultiwayMetricsRow, MultiwayMetricsWriter};
use multiway::solver::InfoKey;
use multiway::{HoldemGame, MultiwaySolver};
use serde::Serialize;

use crate::config::{GameSection, SolveConfig, StorageKind};
use crate::session;

const APPROXIMATION_NOTICE: &str = "3人以上は多人数・一般和ゲームのregret-minimized approximationです。Nash/GTO保証やexploitability指標ではありません。";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompletionStatus {
    Completed,
    ResourceLimit,
    Cancelled,
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
        emit_progress,
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
        emit_progress,
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
    emit_progress: bool,
) -> Result<()> {
    validate_artifact_paths(output, metrics_path, checkpoint_path, mwsol_path)?;
    if !matches!(config.game, GameSection::PreflopMultiway(_)) {
        return Err(anyhow!(
            "multiway solve path requires kind = \"preflop-multiway\""
        ));
    }
    // `config` may carry a CLI `--iterations` override baked into
    // `run.sweeps` (see `solve::run`), which never appears in `raw_config`'s
    // own bytes. Re-serializing it (config.rs's schema round-trips through
    // TOML) lets `build_multiway_session` validate and build against these
    // *effective* run parameters, while the artifact-facing
    // `config_toml`/`config_hash` below are restored to the original file
    // bytes so checkpoints/solutions are stamped exactly as before this
    // refactor -- the override never changes what a checkpoint is stamped
    // with.
    let effective_toml =
        toml::to_string(&config).context("re-serializing the effective multiway config")?;
    let mut mw_session = session::build_multiway_session(&effective_toml, resume_checkpoint)?;
    mw_session.config_toml = raw_config.to_string();
    mw_session.config_hash = config_hash;

    let mut metrics_writer = metrics_path
        .map(MultiwayMetricsWriter::create_or_append)
        .transpose()
        .context("opening multiway metrics")?;
    let started = Instant::now();
    // Seeds `prior` with the current averages (the drift result is
    // discarded), so a resumed run's first drift row measures movement since
    // the checkpoint rather than since an empty profile.
    let mut prior: HashMap<InfoKey, Vec<f32>> = HashMap::new();
    mw_session.solver.strategy_drift_refresh(&mut prior);
    let mut last_row = MultiwayMetricsRow::sampling(mw_session.game_config.seats.len());
    let mut status = CompletionStatus::Completed;
    let mut has_evaluation = false;

    while mw_session.solver.completed_sweeps() < mw_session.sweeps_target {
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
                println!(
                    "sweeps={:>8} traversals={:>10} infosets={:>9} regret_proxy={:.3e} memory={}MiB",
                    now.sweeps,
                    now.traversals,
                    now.infosets,
                    mean(&now.average_positive_regret),
                    now.memory_bytes / (1024 * 1024),
                );
            }
        }
        if let Some(path) = checkpoint_path
            && mw_session
                .checkpoint_every
                .is_some_and(|cadence| sweeps_now % cadence == 0)
        {
            write_checkpoint(&mw_session.solver, path)?;
        }
    }

    let final_checkpoint = final_checkpoint_path(status, checkpoint_path, output);
    if let Some(path) = final_checkpoint.as_deref() {
        write_checkpoint(&mw_session.solver, path)?;
        if emit_progress && status == CompletionStatus::ResourceLimit {
            println!("resource_limit checkpoint: {}", path.display());
        }
    }
    // The rollout abstraction's assignment cache is pure memoization
    // (deterministic f(centroids, key)), so it only grows as the solve
    // visits more concrete rollout keys. Re-save it here so a later run
    // against the same `artifact_cache` path starts warm instead of
    // re-paying every cache miss this run already resolved.
    if let Some(path) = mw_session.game_config.abstraction.artifact_cache.as_deref() {
        mw_session
            .solver
            .game()
            .abstraction()
            .persist_assignment_cache(path)
            .with_context(|| format!("persisting rollout assignment cache {}", path.display()))?;
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
        CompletionStatus::Cancelled => "cancelled",
    }
    .to_string();
    if let Some(writer) = metrics_writer.as_mut() {
        writer
            .append(&last_row)
            .context("writing final multiway metrics")?;
    }

    let snapshot = mw_session.solver.snapshot_state();
    if let Some(path) = mwsol_path {
        let solution = session::make_solution(
            &mw_session.config_toml,
            mw_session.solver.abstraction_fingerprint(),
            &snapshot,
            &last_row,
        );
        // `run.storage` selects the artifact encoding only; the live MCCFR
        // state and `.mwckpt` checkpoints stay f32 regardless.
        let artifact_storage = match mw_session.storage {
            StorageKind::F32 => formats::MwsolStorage::F32,
            StorageKind::I16 => formats::MwsolStorage::I16,
        };
        formats::write_mwsol_with(path, &solution, artifact_storage)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    let elapsed = started.elapsed().as_secs_f64();
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
        seats: last_row.seats.clone(),
        strategy_blocks: snapshot.policies.len(),
        config_hash: formats::config_hash_hex(&mw_session.config_hash),
    };
    let json = serde_json::to_string_pretty(&result)?;
    if let Some(path) = output {
        std::fs::write(path, format!("{json}\n"))
            .with_context(|| format!("writing {}", path.display()))?;
    } else {
        println!("{json}");
    }
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
) -> Result<()> {
    multiway::checkpoint::MultiwayCheckpoint::capture(solver)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiway_example_parses_the_public_contract() {
        let raw = include_str!("../../../examples/preflop_multiway_9max.toml");
        let parsed: SolveConfig = toml::from_str(raw).unwrap();
        assert!(matches!(parsed.game, GameSection::PreflopMultiway(_)));
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
