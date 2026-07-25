//! `mw-eval`: a dev/measurement tool for strategy purification/thresholding
//! (Ganzfried & Sandholm, AAMAS 2012) against a multiway checkpoint.
//!
//! This restores a `.mwckpt` checkpoint's frozen average profile (no
//! further sweeps are run) and trains fresh per-seat best-response deviators
//! against that SAME profile. The legacy mode retains its one-line stdout
//! report exactly. `--deviator-config` instead keys the trained deviators by
//! a common reference abstraction and emits a machine-readable paired-world
//! report; its rollout assignment cache is persisted when configured.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use multiway::ExternalSamplingGame;
use serde::Serialize;

use crate::session;

#[allow(clippy::too_many_arguments)]
pub fn run(
    config_path: &Path,
    checkpoint_path: &Path,
    deviator_config_path: Option<&Path>,
    output_path: Option<&Path>,
    experiment_rung: Option<&str>,
    samples: u64,
    seed: u64,
    purify: &str,
    br_traversals: u64,
    use_current_strategy: bool,
) -> Result<()> {
    if let Some(deviator_config_path) = deviator_config_path {
        return run_reference(
            config_path,
            checkpoint_path,
            deviator_config_path,
            output_path,
            experiment_rung,
            samples,
            seed,
            purify,
            br_traversals,
            use_current_strategy,
        );
    }
    debug_assert!(
        output_path.is_none() && experiment_rung.is_none(),
        "clap requires --deviator-config"
    );
    run_legacy(
        config_path,
        checkpoint_path,
        samples,
        seed,
        purify,
        br_traversals,
        use_current_strategy,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_legacy(
    config_path: &Path,
    checkpoint_path: &Path,
    samples: u64,
    seed: u64,
    purify: &str,
    br_traversals: u64,
    use_current_strategy: bool,
) -> Result<()> {
    let raw_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let raw = std::str::from_utf8(&raw_bytes).context("config file is not valid UTF-8")?;
    let thresholds = parse_thresholds(purify)?;

    let mw_session = session::build_multiway_session(raw, Some(checkpoint_path))
        .context("restoring multiway session from checkpoint")?;
    let num_players = mw_session.game_config.seats.len();

    for threshold in thresholds {
        let started = Instant::now();
        let variant = multiway::ProfileVariant {
            purify_threshold: threshold,
            use_current_strategy,
        };
        let deviators = if br_traversals > 0 {
            let training_seed = seed ^ 0x7075_7269 ^ u64::from(threshold.to_bits());
            Some(
                session::train_deviators_parallel(
                    &mw_session.solver,
                    num_players,
                    mw_session.threads,
                    br_traversals,
                    training_seed,
                    variant,
                )
                .context("training purified best-response deviators")?,
            )
        } else {
            None
        };

        let evaluation = mw_session
            .solver
            .evaluate_profile(samples, seed, deviators.as_deref(), variant)
            .context("evaluating purified multiway profile")?;
        let elapsed = started.elapsed().as_secs_f64();

        let bounds = evaluation
            .deviation_gain_lower_bound
            .as_ref()
            .expect("evaluate_profile always returns deviation_gain_lower_bound");
        let dev_up: Vec<f64> = bounds.iter().map(|estimate| estimate.ci95[1]).collect();
        let max_dev_up = dev_up.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let max_dev_mean = bounds
            .iter()
            .map(|estimate| estimate.mean)
            .fold(f64::NEG_INFINITY, f64::max);
        let dev_up_str = dev_up
            .iter()
            .map(|value| format!("{value:.3}"))
            .collect::<Vec<_>>()
            .join(",");

        let profile_tag = if use_current_strategy {
            "current"
        } else {
            "average"
        };
        println!(
            "profile={profile_tag} purify={threshold:.3} maxDevUp={max_dev_up:.3} \
             maxDevMean={max_dev_mean:.3} devUp=[{dev_up_str}] elapsed={elapsed:.1}s"
        );
    }

    Ok(())
}

const REFERENCE_PROFILE_SCHEMA: &str = "solvers.reference-deviation-profile/v1";

#[derive(Serialize)]
struct CandidateIdentity {
    config_path: String,
    checkpoint_path: String,
    config_fingerprint: String,
    game_fingerprint: String,
    abstraction_fingerprint: String,
    configuration_fingerprint: String,
    sweeps: u64,
}

#[derive(Serialize)]
struct ReferenceIdentity {
    config_path: String,
    config_fingerprint: String,
    game_fingerprint: String,
    abstraction_fingerprint: String,
    recall: multiway::RecallMode,
    artifact_cache: Option<String>,
}

#[derive(Serialize)]
struct SeatTrainingCoverage {
    seat: usize,
    #[serde(flatten)]
    coverage: multiway::DeviatorTrainingCoverage,
}

#[derive(Serialize)]
struct ExperimentIdentity {
    rung: String,
}

#[derive(Serialize)]
struct ReferenceProfileReport {
    schema_version: &'static str,
    experiment: ExperimentIdentity,
    candidate: CandidateIdentity,
    reference: ReferenceIdentity,
    samples: u64,
    seed: u64,
    br_traversals: u64,
    profile: &'static str,
    purify_threshold: f32,
    training_seed: u64,
    elapsed_secs: f64,
    training_coverage: Vec<SeatTrainingCoverage>,
    evaluation: multiway::ReferenceDeviationEvaluation,
}

#[allow(clippy::too_many_arguments)]
fn run_reference(
    config_path: &Path,
    checkpoint_path: &Path,
    deviator_config_path: &Path,
    output_path: Option<&Path>,
    experiment_rung: Option<&str>,
    samples: u64,
    seed: u64,
    purify: &str,
    br_traversals: u64,
    use_current_strategy: bool,
) -> Result<()> {
    let candidate_bytes =
        std::fs::read(config_path).with_context(|| format!("reading {}", config_path.display()))?;
    let candidate_raw =
        std::str::from_utf8(&candidate_bytes).context("config file is not valid UTF-8")?;
    let reference_bytes = std::fs::read(deviator_config_path)
        .with_context(|| format!("reading {}", deviator_config_path.display()))?;
    let reference_raw =
        std::str::from_utf8(&reference_bytes).context("deviator config file is not valid UTF-8")?;
    let thresholds = parse_thresholds(purify)?;
    let [threshold] = thresholds.as_slice() else {
        return Err(anyhow!(
            "--deviator-config requires exactly one --purify threshold per JSON report"
        ));
    };

    let mw_session = session::build_multiway_session_with_deviation(
        candidate_raw,
        reference_raw,
        Some(checkpoint_path),
    )
    .context("restoring multiway session with common deviation reference")?;
    let reference = mw_session
        .deviation_reference
        .as_ref()
        .expect("the reference builder always records metadata");
    let num_players = mw_session.game_config.seats.len();
    let completed_sweeps = mw_session.solver.completed_sweeps();
    let experiment_rung = normalize_experiment_rung(experiment_rung, completed_sweeps)?;
    let started = Instant::now();
    let variant = multiway::ProfileVariant {
        purify_threshold: *threshold,
        use_current_strategy,
    };
    let training_seed = seed ^ 0x7075_7269 ^ u64::from(threshold.to_bits());
    let trained = session::train_deviators_with_reports_parallel(
        &mw_session.solver,
        num_players,
        mw_session.threads,
        br_traversals,
        training_seed,
        variant,
    )
    .context("training common-reference best-response deviators")?;
    let training_coverage = trained
        .iter()
        .enumerate()
        .map(|(seat, trained)| SeatTrainingCoverage {
            seat,
            coverage: trained.coverage,
        })
        .collect();
    let policies = trained
        .into_iter()
        .map(|trained| trained.policy)
        .collect::<Vec<_>>();
    let evaluation = mw_session
        .solver
        .evaluate_reference_deviators(samples, seed, &policies, variant)
        .context("evaluating common-reference deviators")?;

    if let Some(path) = reference.artifact_cache.as_deref()
        && let Some(rollout) = mw_session
            .solver
            .game()
            .deviation_abstraction()
            .and_then(|abstraction| abstraction.rollout())
    {
        rollout.persist_assignment_cache(path).with_context(|| {
            format!(
                "persisting common-reference rollout assignment cache {}",
                path.display()
            )
        })?;
    }

    let report = ReferenceProfileReport {
        schema_version: REFERENCE_PROFILE_SCHEMA,
        experiment: ExperimentIdentity {
            rung: experiment_rung,
        },
        candidate: CandidateIdentity {
            config_path: config_path.display().to_string(),
            checkpoint_path: checkpoint_path.display().to_string(),
            config_fingerprint: formats::config_hash_hex(&mw_session.config_hash),
            game_fingerprint: formats::config_hash_hex(
                &mw_session.solver.game().game_fingerprint(),
            ),
            abstraction_fingerprint: formats::config_hash_hex(
                &mw_session.solver.abstraction_fingerprint(),
            ),
            configuration_fingerprint: formats::config_hash_hex(
                &mw_session.solver.configuration_fingerprint(),
            ),
            sweeps: completed_sweeps,
        },
        reference: ReferenceIdentity {
            config_path: deviator_config_path.display().to_string(),
            config_fingerprint: formats::config_hash_hex(&reference.config_hash),
            game_fingerprint: formats::config_hash_hex(&reference.game_fingerprint),
            abstraction_fingerprint: formats::config_hash_hex(&reference.abstraction_fingerprint),
            recall: reference.recall,
            artifact_cache: reference
                .artifact_cache
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        samples,
        seed,
        br_traversals,
        profile: if use_current_strategy {
            "current"
        } else {
            "average"
        },
        purify_threshold: *threshold,
        training_seed,
        elapsed_secs: started.elapsed().as_secs_f64(),
        training_coverage,
        evaluation,
    };
    let mut json =
        serde_json::to_string_pretty(&report).context("serializing reference profile report")?;
    json.push('\n');
    if let Some(path) = output_path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating report directory {}", parent.display()))?;
        }
        std::fs::write(path, json)
            .with_context(|| format!("writing reference profile report {}", path.display()))?;
    } else {
        print!("{json}");
    }
    Ok(())
}

fn normalize_experiment_rung(rung: Option<&str>, completed_sweeps: u64) -> Result<String> {
    let Some(rung) = rung else {
        return Ok(format!("sweeps-{completed_sweeps}"));
    };
    let rung = rung.trim();
    if rung.is_empty()
        || !rung
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(anyhow!(
            "--experiment-rung must use only ASCII letters, digits, '.', '_', or '-'"
        ));
    }
    Ok(rung.to_owned())
}

/// Parses a comma-separated list of purification thresholds, e.g.
/// `"0.0,0.02,0.05"`. Blank tokens (a trailing comma, or an all-whitespace
/// list) are skipped.
fn parse_thresholds(raw: &str) -> Result<Vec<f32>> {
    let thresholds: Vec<f32> = raw
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            token
                .parse::<f32>()
                .with_context(|| format!("invalid --purify threshold {token:?}"))
        })
        .collect::<Result<_>>()?;
    if thresholds.is_empty() {
        return Err(anyhow!("--purify must list at least one threshold"));
    }
    Ok(thresholds)
}

#[cfg(test)]
mod tests {
    use super::{REFERENCE_PROFILE_SCHEMA, normalize_experiment_rung, parse_thresholds, run};

    #[test]
    fn parses_comma_separated_thresholds() {
        assert_eq!(
            parse_thresholds("0.0,0.02,0.05").unwrap(),
            vec![0.0, 0.02, 0.05]
        );
    }

    #[test]
    fn trims_whitespace_and_skips_blank_tokens() {
        assert_eq!(
            parse_thresholds(" 0.0 , 0.1 ,,0.2").unwrap(),
            vec![0.0, 0.1, 0.2]
        );
    }

    #[test]
    fn rejects_unparseable_tokens() {
        assert!(parse_thresholds("0.0,not-a-number").is_err());
    }

    #[test]
    fn rejects_an_all_blank_list() {
        assert!(parse_thresholds(" , ,").is_err());
    }

    #[test]
    fn experiment_rung_is_validated_and_has_a_standalone_fallback() {
        assert_eq!(normalize_experiment_rung(None, 500).unwrap(), "sweeps-500");
        assert_eq!(normalize_experiment_rung(Some(" s1 "), 500).unwrap(), "s1");
        assert!(normalize_experiment_rung(Some("../s1"), 500).is_err());
    }

    #[test]
    fn common_reference_mode_writes_one_machine_readable_report_and_cache() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml")
            .replace("stack_bb = 2.0", "stack_bb = 10.0")
            .replacen(
                "bet_sizes = []\nraise_sizes = []\nmax_aggressive_actions = 1\ninclude_allin = true",
                "bet_sizes = [{ kind = \"previous-bet-multiple\", factor = 2.0 }]\n\
                 raise_sizes = []\nmax_aggressive_actions = 1\ninclude_allin = false",
                1,
            );
        let directory = tempfile::tempdir().unwrap();
        let candidate_path = directory.path().join("candidate.toml");
        let reference_path = directory.path().join("reference.toml");
        let checkpoint_path = directory.path().join("candidate.mwckpt");
        let report_path = directory.path().join("report.json");
        let cache_path = directory.path().join("reference.mwab");
        let cache_literal = format!("{:?}", cache_path.display().to_string());
        let reference_raw = raw
            .replace("flop_buckets = 8", "flop_buckets = 4")
            .replace("turn_buckets = 8", "turn_buckets = 4")
            .replace("river_buckets = 8", "river_buckets = 4")
            .replacen(
                "seed = 17\n",
                &format!("seed = 23\nartifact_cache = {cache_literal}\n"),
                1,
            );
        std::fs::write(&candidate_path, &raw).unwrap();
        std::fs::write(&reference_path, &reference_raw).unwrap();

        let mut candidate = crate::session::build_multiway_session(&raw, None).unwrap();
        candidate.solver.run_sweeps(1).unwrap();
        multiway::MultiwayCheckpoint::capture(&candidate.solver)
            .write_atomic(&checkpoint_path)
            .unwrap();

        run(
            &candidate_path,
            &checkpoint_path,
            Some(&reference_path),
            Some(&report_path),
            Some("s1"),
            8,
            4242,
            "0.0",
            8,
            false,
        )
        .unwrap();

        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(report["schema_version"], REFERENCE_PROFILE_SCHEMA);
        assert_eq!(report["experiment"]["rung"], "s1");
        assert_eq!(report["candidate"]["sweeps"], 1);
        assert_eq!(report["samples"], 8);
        assert_eq!(report["seed"], 4242);
        assert_eq!(report["training_coverage"].as_array().unwrap().len(), 3);
        assert_eq!(
            report["evaluation"]["candidate_policy_coverage"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            report["evaluation"]["coverage"].as_array().unwrap().len(),
            3
        );
        assert_eq!(report["evaluation"]["worlds"].as_array().unwrap().len(), 8);
        for coverage in report["evaluation"]["candidate_policy_coverage"]
            .as_array()
            .unwrap()
        {
            assert_eq!(
                coverage["decision_visits"].as_u64().unwrap(),
                coverage["stored_strategy_visits"].as_u64().unwrap()
                    + coverage["uniform_fallback_visits"].as_u64().unwrap()
            );
        }
        let cached = multiway::RolloutKMeansAbstraction::read_artifact(&cache_path).unwrap();
        assert!(
            cached.assignment_cache_len() > 0,
            "reference evaluation assignments must be persisted"
        );
    }
}
