use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use formats::{
    Estimate, MULTIWAY_SCHEMA_VERSION, MultiwayHistoryNode, MultiwayMetricsRow,
    MultiwayMetricsWriter, MultiwaySeatMetrics, MultiwaySeatResult, MultiwaySolution,
    MultiwayStrategyBlock, MultiwayStrategyKey,
};
use multiway::abstraction::{RolloutKMeansBuilder, RolloutKMeansParams, StreetBucketCounts};
use multiway::checkpoint::MultiwayCheckpoint;
use multiway::config::{
    FieldPlayerConfig, RakeConfig as MultiwayRake, UtilityConfig as MultiwayUtility,
};
use multiway::solver::{InfoKey, PolicyEntry, ProfileEvaluation, SolverConfig, SolverError};
use multiway::{HoldemGame, MultiwaySolver};
use serde::Serialize;

use crate::config::{
    AlgorithmSection, GameSection, RakeSection, SolveConfig, StorageKind, UtilitySection,
};

const DEFAULT_MEMORY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
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
    seats: Vec<MultiwaySeatMetrics>,
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
) -> Result<()> {
    let SolveConfig {
        game,
        rake,
        utility,
        algorithm,
        run,
    } = config;
    let GameSection::PreflopMultiway(game_config) = game else {
        return Err(anyhow!(
            "multiway solve path requires kind = \"preflop-multiway\""
        ));
    };
    if run.storage != StorageKind::F32 {
        return Err(anyhow!(
            "multiway MCCFR currently requires run.storage = \"f32\""
        ));
    }
    if run.target_nash_conv.is_some() {
        return Err(anyhow!(
            "multiway profiles do not expose NashConv; remove run.target_nash_conv"
        ));
    }

    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
    game_config
        .validate_economics(&utility, &rake)
        .context("validating multiway game and utility")?;

    let (algorithm_seed, exploration_epsilon, discount_every, discount_until) = match algorithm {
        AlgorithmSection::ExternalSamplingMccfr {
            seed,
            exploration_epsilon,
            discount_every,
            discount_until,
        } => (seed, exploration_epsilon, discount_every, discount_until),
        _ => {
            return Err(anyhow!(
                "preflop-multiway requires schedule = \"external-sampling-mccfr\""
            ));
        }
    };
    let sweeps = run.sweeps.unwrap_or(run.iterations);
    if sweeps == 0 {
        return Err(anyhow!("run.sweeps must be positive for a multiway solve"));
    }
    let evaluation_cadence = run.evaluation_cadence.unwrap_or(run.check_every);
    if evaluation_cadence == 0 {
        return Err(anyhow!("run.evaluation_cadence must be positive"));
    }
    if run.checkpoint_every == Some(0) {
        return Err(anyhow!(
            "run.checkpoint_every must be positive when supplied"
        ));
    }
    if run.evaluation_samples == Some(0) {
        return Err(anyhow!(
            "run.evaluation_samples must be positive when supplied"
        ));
    }

    let evaluation_samples = run.evaluation_samples.unwrap_or(256);
    let params = RolloutKMeansParams {
        flop_buckets: u32::from(game_config.abstraction.flop_buckets),
        turn_buckets: u32::from(game_config.abstraction.turn_buckets),
        river_buckets: u32::from(game_config.abstraction.river_buckets),
        rollout_samples: game_config.abstraction.rollout_samples,
        seed: game_config.abstraction.seed,
    };
    let mut abstraction_builder = RolloutKMeansBuilder::new(params);
    for profile in &game_config.abstraction.active_opponent_buckets {
        abstraction_builder = abstraction_builder
            .active_opponent_buckets(
                profile.active_opponents,
                StreetBucketCounts {
                    flop: u32::from(profile.flop_buckets),
                    turn: u32::from(profile.turn_buckets),
                    river: u32::from(profile.river_buckets),
                },
            )
            .context("configuring active-opponent bucket budgets")?;
    }
    let abstraction = if let Some(path) = game_config.abstraction.artifact_cache.as_deref() {
        if path.is_file() {
            abstraction_builder
                .load_artifact(path)
                .with_context(|| format!("loading rollout artifact {}", path.display()))?
        } else {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating rollout artifact directory {}", parent.display())
                })?;
            }
            let abstraction = abstraction_builder
                .build()
                .context("training deterministic multiway rollout abstraction")?;
            abstraction
                .write_artifact(path)
                .with_context(|| format!("writing rollout artifact {}", path.display()))?;
            abstraction
        }
    } else {
        abstraction_builder
            .build()
            .context("training deterministic multiway rollout abstraction")?
    };
    let game = HoldemGame::new(&game_config, &utility, &rake, abstraction)
        .context("building generative multiway game")?;
    let sampler = game.deal_sampler().context("compiling table ranges")?;
    let solver_config = SolverConfig {
        seed: run.seed.unwrap_or(algorithm_seed),
        max_memory_bytes: run.max_memory_bytes.unwrap_or(DEFAULT_MEMORY_LIMIT),
        max_traversal_depth: 512,
        exploration_epsilon,
        discount_every,
        discount_until,
    };
    let evaluation_seed = solver_config.seed ^ 0x6576_616c_7561_7465;
    let mut solver = if let Some(path) = resume_checkpoint {
        let checkpoint = MultiwayCheckpoint::load_unchecked(path)
            .with_context(|| format!("reading multiway checkpoint {}", path.display()))?;
        if checkpoint.state.config != solver_config {
            return Err(anyhow!(
                "checkpoint solver configuration does not match the current config"
            ));
        }
        let solver = MultiwaySolver::from_state(game, sampler, checkpoint.state)
            .context("restoring multiway MCCFR state")?;
        if solver.configuration_fingerprint() != checkpoint.header.configuration_fingerprint {
            return Err(anyhow!(
                "checkpoint belongs to different table rules or ranges"
            ));
        }
        if solver.abstraction_fingerprint() != checkpoint.header.abstraction_fingerprint {
            return Err(anyhow!(
                "checkpoint belongs to a different card abstraction"
            ));
        }
        solver
    } else {
        MultiwaySolver::new(game, sampler, solver_config).context("initializing multiway MCCFR")?
    };
    if solver.metrics().sweeps > sweeps {
        return Err(anyhow!(
            "checkpoint already contains {} sweeps, exceeding target {}",
            solver.metrics().sweeps,
            sweeps
        ));
    }
    let mut metrics_writer = metrics_path
        .map(MultiwayMetricsWriter::create_or_append)
        .transpose()
        .context("opening multiway metrics")?;
    let started = Instant::now();
    let mut prior: HashMap<InfoKey, Vec<f32>> = solver
        .snapshot_state()
        .policies
        .into_iter()
        .map(|entry| {
            let strategy = entry.column.average_strategy();
            (entry.key, strategy)
        })
        .collect();
    let mut last_row = MultiwayMetricsRow::sampling(game_config.seats.len());
    let mut status = CompletionStatus::Completed;
    let mut has_evaluation = false;

    while solver.metrics().sweeps < sweeps {
        if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
            status = CompletionStatus::Cancelled;
            break;
        }
        let current = solver.metrics().sweeps;
        let evaluation_delta = distance_to_boundary(current, evaluation_cadence);
        let checkpoint_delta = run
            .checkpoint_every
            .map(|cadence| distance_to_boundary(current, cadence))
            .unwrap_or(u64::MAX);
        let chunk = (sweeps - current)
            .min(evaluation_delta)
            .min(checkpoint_delta)
            .max(1);
        match solver.run_sweeps(chunk) {
            Ok(()) => {}
            Err(SolverError::MemoryLimit { .. }) => {
                status = CompletionStatus::ResourceLimit;
                break;
            }
            Err(error) => return Err(error).context("running multiway MCCFR"),
        }

        let now = solver.metrics();
        if now.sweeps % evaluation_cadence == 0 || now.sweeps == sweeps {
            let snapshot = solver.snapshot_state();
            let evaluation = solver
                .evaluate_average_profile(evaluation_samples, evaluation_seed)
                .context("evaluating held-out multiway profile")?;
            let drift = strategy_drift(&snapshot.policies, &prior, game_config.seats.len());
            prior = snapshot
                .policies
                .iter()
                .map(|entry| (entry.key, entry.column.average_strategy()))
                .collect();
            last_row = metrics_row(
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
            println!(
                "sweeps={:>8} traversals={:>10} infosets={:>9} regret_proxy={:.3e} memory={}MiB",
                now.sweeps,
                now.traversals,
                now.infosets,
                mean(&now.average_positive_regret),
                now.memory_bytes / (1024 * 1024),
            );
        }
        if checkpoint_path.is_some()
            && run
                .checkpoint_every
                .is_some_and(|cadence| now.sweeps % cadence == 0)
        {
            write_checkpoint(&solver, checkpoint_path.expect("checked some"))?;
        }
    }

    if let Some(path) = checkpoint_path {
        write_checkpoint(&solver, path)?;
    }
    let final_metrics = solver.metrics();
    if !has_evaluation || last_row.sweeps != final_metrics.sweeps {
        let snapshot = solver.snapshot_state();
        let evaluation = solver
            .evaluate_average_profile(evaluation_samples, evaluation_seed)
            .context("evaluating final held-out multiway profile")?;
        let drift = strategy_drift(&snapshot.policies, &prior, game_config.seats.len());
        last_row = metrics_row(
            &final_metrics,
            drift,
            started.elapsed().as_secs_f64(),
            Some(&evaluation),
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

    let snapshot = solver.snapshot_state();
    if let Some(path) = mwsol_path {
        let solution = solution_artifact(
            raw_config,
            solver.abstraction_fingerprint(),
            &snapshot,
            &last_row,
        );
        formats::write_mwsol(path, &solution)
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
        seats: last_row.seats.clone(),
        strategy_blocks: snapshot.policies.len(),
        config_hash: formats::config_hash_hex(&config_hash),
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

fn convert_utility(utility: UtilitySection) -> Result<MultiwayUtility> {
    Ok(match utility {
        UtilitySection::ChipEv => MultiwayUtility::ChipEv,
        UtilitySection::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => MultiwayUtility::TournamentIcm {
            outside_field: outside_field
                .into_iter()
                .map(|player| FieldPlayerConfig {
                    name: player.name,
                    stack_bb: player.stack_bb,
                })
                .collect(),
            payouts,
            samples,
            seed,
        },
        UtilitySection::Icm { .. } => {
            return Err(anyhow!(
                "legacy HU utility kind = \"icm\" is not valid for preflop-multiway; use kind = \"tournament-icm\""
            ));
        }
    })
}

fn convert_rake(rake: RakeSection) -> MultiwayRake {
    match rake {
        RakeSection::None => MultiwayRake::None,
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => MultiwayRake::PercentCap {
            rate,
            cap_bb: cap / multiway::types::CHIPS_PER_BB as f64,
            no_flop_no_drop,
        },
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => MultiwayRake::GgPreflop {
            rate,
            cap_bb: cap / multiway::types::CHIPS_PER_BB as f64,
            exempt_pot_bb: exempt_pot as f64 / 1_000.0,
        },
    }
}

fn write_checkpoint<A: multiway::MultiwayAbstraction>(
    solver: &MultiwaySolver<HoldemGame<A>>,
    path: &Path,
) -> Result<()> {
    MultiwayCheckpoint::capture(solver)
        .write_atomic(path)
        .with_context(|| format!("writing {}", path.display()))
}

fn distance_to_boundary(current: u64, cadence: u64) -> u64 {
    cadence - current % cadence
}

fn strategy_drift(
    policies: &[PolicyEntry],
    prior: &HashMap<InfoKey, Vec<f32>>,
    seats: usize,
) -> Vec<f64> {
    let mut totals = vec![0.0; seats];
    let mut counts = vec![0u64; seats];
    for entry in policies {
        let current = entry.column.average_strategy();
        let value = prior.get(&entry.key).map_or(0.0, |previous| {
            current
                .iter()
                .zip(previous)
                .map(|(&left, &right)| f64::from((left - right).abs()))
                .sum::<f64>()
                * 0.5
        });
        totals[entry.key.player as usize] += value;
        counts[entry.key.player as usize] += 1;
    }
    totals
        .into_iter()
        .zip(counts)
        .map(|(total, count)| {
            if count == 0 {
                0.0
            } else {
                total / count as f64
            }
        })
        .collect()
}

fn metrics_row(
    metrics: &multiway::SolverMetrics,
    drift: Vec<f64>,
    elapsed_secs: f64,
    evaluation: Option<&ProfileEvaluation>,
) -> MultiwayMetricsRow {
    MultiwayMetricsRow {
        schema_version: MULTIWAY_SCHEMA_VERSION,
        phase: "sampling".to_string(),
        sweeps: metrics.sweeps,
        traversals: metrics.traversals,
        elapsed_secs,
        infosets: metrics.infosets,
        memory_bytes: metrics.memory_bytes,
        traversals_per_second: if elapsed_secs > 0.0 {
            metrics.traversals as f64 / elapsed_secs
        } else {
            0.0
        },
        seats: metrics
            .average_positive_regret
            .iter()
            .enumerate()
            .map(|(seat, &regret)| MultiwaySeatMetrics {
                seat: seat as u8,
                profile_ev: evaluation
                    .and_then(|value| value.seats.get(seat))
                    .map(profile_estimate),
                average_positive_regret: regret,
                strategy_drift_l1: drift[seat],
                deviation_gain_lower_bound: evaluation
                    .and_then(|value| value.deviation_gain_lower_bound.as_ref())
                    .and_then(|values| values.get(seat))
                    .map(profile_estimate),
            })
            .collect(),
    }
}

fn profile_estimate(value: &multiway::solver::ProfileEstimate) -> Estimate {
    Estimate {
        mean: value.mean,
        stderr: value.stderr,
        ci95: value.ci95,
    }
}

fn solution_artifact(
    raw_config: &str,
    abstraction_fingerprint: [u8; 32],
    state: &multiway::solver::SolverState,
    row: &MultiwayMetricsRow,
) -> MultiwaySolution {
    let strategies = state
        .policies
        .iter()
        .map(|entry| MultiwayStrategyBlock {
            key: MultiwayStrategyKey {
                history: entry.key.history.0,
                actor: entry.key.player,
                street: entry.key.street,
                active_opponents: entry.key.active_opponents,
                bucket_path: entry.key.bucket_path,
            },
            actions: entry.column.action_labels.clone(),
            probabilities: entry.column.average_strategy(),
        })
        .collect();
    MultiwaySolution {
        schema_version: MULTIWAY_SCHEMA_VERSION,
        config_toml: raw_config.to_string(),
        abstraction_fingerprint,
        sweeps: row.sweeps,
        approximate_profile: true,
        histories: state
            .histories
            .iter()
            .map(|entry| MultiwayHistoryNode {
                key: entry.key.0,
                parent: entry.parent.0,
                actor: entry.actor,
                action_index: entry.action_index,
                action: entry.action_label.clone(),
            })
            .collect(),
        seats: row
            .seats
            .iter()
            .map(|seat| MultiwaySeatResult {
                seat: seat.seat,
                profile_ev: seat.profile_ev.clone(),
                average_positive_regret: seat.average_positive_regret,
                strategy_drift_l1: seat.strategy_drift_l1,
                deviation_gain_lower_bound: seat.deviation_gain_lower_bound.clone(),
            })
            .collect(),
        strategies,
    }
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
    fn boundaries_are_positive_and_repeat() {
        assert_eq!(distance_to_boundary(0, 100), 100);
        assert_eq!(distance_to_boundary(99, 100), 1);
        assert_eq!(distance_to_boundary(100, 100), 100);
    }

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
    fn shared_rake_chip_units_convert_to_multiway_bb() {
        let rake = convert_rake(RakeSection::PercentCap {
            rate: 0.05,
            cap: 3_500.0,
            no_flop_no_drop: true,
        });
        assert!(matches!(
            rake,
            MultiwayRake::PercentCap {
                cap_bb: 3.5,
                no_flop_no_drop: true,
                ..
            }
        ));
    }

    #[test]
    fn legacy_hu_icm_is_rejected_on_multiway_path() {
        let error = convert_utility(UtilitySection::Icm {
            payouts: [1.0, 0.0],
        })
        .unwrap_err();
        assert!(error.to_string().contains("tournament-icm"));
    }
}
