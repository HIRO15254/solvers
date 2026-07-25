//! Deterministic paired bootstrap comparison for two common-reference profile
//! evaluation reports.
//!
//! Usage:
//! `cargo run --release -p cli --example paired_reference_bootstrap -- \
//!   LEFT.json RIGHT.json [--replicates 10000] [--seed 8097869545037133361]`

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const DEFAULT_REPLICATES: usize = 10_000;
const DEFAULT_BOOTSTRAP_SEED: u64 = 0x7061_6972_6564_7631;

const REFERENCE_FINGERPRINT_PATHS: &[&str] = &[
    "/reference/abstraction_fingerprint",
    "/reference/abstractionFingerprint",
    "/reference/fingerprint",
    "/reference_abstraction_fingerprint",
    "/referenceAbstractionFingerprint",
    "/reference_fingerprint",
];
const CANDIDATE_GAME_FINGERPRINT_PATHS: &[&str] = &[
    "/candidate/game_fingerprint",
    "/candidate/gameFingerprint",
    "/candidate_game_fingerprint",
    "/candidateGameFingerprint",
];
const REFERENCE_GAME_FINGERPRINT_PATHS: &[&str] = &[
    "/reference/game_fingerprint",
    "/reference/gameFingerprint",
    "/reference_game_fingerprint",
    "/referenceGameFingerprint",
];
const EXPERIMENT_RUNG_PATHS: &[&str] = &["/experiment/rung", "/rung"];
const CANDIDATE_SWEEPS_PATHS: &[&str] = &[
    "/candidate/sweeps",
    "/candidate_sweeps",
    "/candidateSweeps",
    "/sweeps",
];
const SAMPLE_COUNT_PATHS: &[&str] = &[
    "/samples",
    "/evaluation/evaluation/samples",
    "/reference_evaluation/evaluation/samples",
    "/referenceEvaluation/evaluation/samples",
];
const EVALUATION_SEED_PATHS: &[&str] = &["/seed", "/evaluation_seed", "/evaluationSeed"];
const BR_TRAVERSALS_PATHS: &[&str] = &["/br_traversals", "/brTraversals"];
const TRAINING_SEED_PATHS: &[&str] = &["/training_seed", "/trainingSeed"];
const PROFILE_PATHS: &[&str] = &["/profile"];
const PURIFY_THRESHOLD_PATHS: &[&str] = &["/purify_threshold", "/purifyThreshold"];
const WORLDS_PATHS: &[&str] = &[
    "/evaluation/worlds",
    "/reference_evaluation/worlds",
    "/referenceEvaluation/worlds",
    "/worlds",
];
const REPLAY_COVERAGE_PATHS: &[&str] = &[
    "/evaluation/coverage",
    "/reference_evaluation/coverage",
    "/referenceEvaluation/coverage",
    "/replay_coverage",
    "/replayCoverage",
    "/coverage",
];
const CANDIDATE_POLICY_COVERAGE_PATHS: &[&str] = &[
    "/evaluation/candidate_policy_coverage",
    "/evaluation/candidatePolicyCoverage",
    "/reference_evaluation/candidate_policy_coverage",
    "/referenceEvaluation/candidatePolicyCoverage",
    "/candidate_policy_coverage",
    "/candidatePolicyCoverage",
];
const TRAINING_COVERAGE_PATHS: &[&str] = &["/training_coverage", "/trainingCoverage"];

#[derive(Debug, Parser)]
#[command(about = "Compare common-reference profile reports with a paired bootstrap")]
struct Args {
    /// Left (baseline) common-reference profile JSON report.
    left: PathBuf,
    /// Right (candidate) common-reference profile JSON report.
    right: PathBuf,
    /// Number of paired bootstrap replicates.
    #[arg(long, default_value_t = DEFAULT_REPLICATES)]
    replicates: usize,
    /// Seed for the deterministic paired bootstrap RNG.
    #[arg(long, default_value_t = DEFAULT_BOOTSTRAP_SEED)]
    seed: u64,
    /// Override the identifier embedded in the left report.
    #[arg(long)]
    left_id: Option<String>,
    /// Override the identifier embedded in the right report.
    #[arg(long)]
    right_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct WorldInput {
    #[serde(alias = "sampleId")]
    sample_id: u64,
    gains: Vec<f64>,
    #[serde(default, alias = "baselineUtilities")]
    baseline_utilities: Option<Vec<f64>>,
    #[serde(default, alias = "deviatingSeatUtilities")]
    deviating_seat_utilities: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
struct ParsedProfile {
    source: String,
    identifier: Value,
    reference_identifier: Value,
    reference_fingerprint: String,
    candidate_game_fingerprint: String,
    experiment_rung: String,
    candidate_sweeps: u64,
    samples: u64,
    evaluation_seed: u64,
    br_traversals: u64,
    training_seed: u64,
    profile: String,
    purify_threshold: f64,
    worlds: Vec<WorldInput>,
    candidate_policy_coverage: Value,
    replay_coverage: Value,
    training_coverage: Value,
}

#[derive(Clone, Debug, PartialEq)]
struct BootstrapResult {
    left_per_seat_raw_mean_gains: Vec<f64>,
    right_per_seat_raw_mean_gains: Vec<f64>,
    left_max_clamped_mean_gain: f64,
    right_max_clamped_mean_gain: f64,
    observed_delta_right_minus_left: f64,
    percentile_ci95: [f64; 2],
    probability_right_greater_than_left: f64,
}

#[derive(Debug, Serialize)]
struct Output<'a> {
    schema: &'static str,
    shared: SharedOutput<'a>,
    bootstrap: BootstrapOutput,
    left: ProfileOutput<'a>,
    right: ProfileOutput<'a>,
    comparison: ComparisonOutput,
}

#[derive(Debug, Serialize)]
struct SharedOutput<'a> {
    reference_abstraction_fingerprint: &'a str,
    candidate_game_fingerprint: &'a str,
    experiment_rung: &'a str,
    candidate_sweeps: u64,
    samples: u64,
    evaluation_seed: u64,
    br_traversals: u64,
    training_seed: u64,
    profile: &'a str,
    purify_threshold: f64,
    seats: usize,
    sample_ids_exactly_paired: bool,
}

#[derive(Debug, Serialize)]
struct BootstrapOutput {
    replicates: usize,
    seed: u64,
    confidence_level: f64,
    percentile_method: &'static str,
    statistic: &'static str,
}

#[derive(Debug, Serialize)]
struct ProfileOutput<'a> {
    source: &'a str,
    identifier: &'a Value,
    reference_identifier: &'a Value,
    per_seat_raw_mean_gains: &'a [f64],
    max_clamped_mean_gain: f64,
    candidate_policy_coverage: &'a Value,
    replay_coverage: &'a Value,
    training_coverage: &'a Value,
}

#[derive(Debug, Serialize)]
struct ComparisonOutput {
    direction: &'static str,
    observed_delta: f64,
    percentile_ci95: [f64; 2],
    probability_right_greater_than_left: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.replicates == 0 {
        bail!("--replicates must be positive");
    }

    let mut left = read_profile(&args.left)?;
    let mut right = read_profile(&args.right)?;
    if let Some(identifier) = args.left_id {
        left.identifier = Value::String(identifier);
    }
    if let Some(identifier) = args.right_id {
        right.identifier = Value::String(identifier);
    }

    let seat_count = validate_and_pair(&mut left, &mut right)?;
    let result = paired_bootstrap(&left.worlds, &right.worlds, args.replicates, args.seed)?;
    let output = Output {
        schema: "solvers.paired-reference-bootstrap/v1",
        shared: SharedOutput {
            reference_abstraction_fingerprint: &left.reference_fingerprint,
            candidate_game_fingerprint: &left.candidate_game_fingerprint,
            experiment_rung: &left.experiment_rung,
            candidate_sweeps: left.candidate_sweeps,
            samples: left.samples,
            evaluation_seed: left.evaluation_seed,
            br_traversals: left.br_traversals,
            training_seed: left.training_seed,
            profile: &left.profile,
            purify_threshold: left.purify_threshold,
            seats: seat_count,
            sample_ids_exactly_paired: true,
        },
        bootstrap: BootstrapOutput {
            replicates: args.replicates,
            seed: args.seed,
            confidence_level: 0.95,
            percentile_method: "linear-interpolation-p*(n-1)",
            statistic: "max_seat(max(0, mean_raw_gain)); delta=right-left",
        },
        left: ProfileOutput {
            source: &left.source,
            identifier: &left.identifier,
            reference_identifier: &left.reference_identifier,
            per_seat_raw_mean_gains: &result.left_per_seat_raw_mean_gains,
            max_clamped_mean_gain: result.left_max_clamped_mean_gain,
            candidate_policy_coverage: &left.candidate_policy_coverage,
            replay_coverage: &left.replay_coverage,
            training_coverage: &left.training_coverage,
        },
        right: ProfileOutput {
            source: &right.source,
            identifier: &right.identifier,
            reference_identifier: &right.reference_identifier,
            per_seat_raw_mean_gains: &result.right_per_seat_raw_mean_gains,
            max_clamped_mean_gain: result.right_max_clamped_mean_gain,
            candidate_policy_coverage: &right.candidate_policy_coverage,
            replay_coverage: &right.replay_coverage,
            training_coverage: &right.training_coverage,
        },
        comparison: ComparisonOutput {
            direction: "right-minus-left; positive means right is more exploitable",
            observed_delta: result.observed_delta_right_minus_left,
            percentile_ci95: result.percentile_ci95,
            probability_right_greater_than_left: result.probability_right_greater_than_left,
        },
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn read_profile(path: &Path) -> Result<ParsedProfile> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root: Value =
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    parse_profile(root, path.display().to_string())
        .with_context(|| format!("validating {}", path.display()))
}

fn parse_profile(root: Value, source: String) -> Result<ParsedProfile> {
    let reference_fingerprint =
        consistent_string(&root, REFERENCE_FINGERPRINT_PATHS, "reference fingerprint")?;
    let candidate_game_fingerprint = consistent_string(
        &root,
        CANDIDATE_GAME_FINGERPRINT_PATHS,
        "candidate game fingerprint",
    )?;
    if let Some(reference_game_fingerprint) = optional_consistent_string(
        &root,
        REFERENCE_GAME_FINGERPRINT_PATHS,
        "reference game fingerprint",
    )? && reference_game_fingerprint != candidate_game_fingerprint
    {
        bail!(
            "candidate and reference game fingerprints differ within one report: \
             candidate={candidate_game_fingerprint} reference={reference_game_fingerprint}"
        );
    }
    let experiment_rung = consistent_string(&root, EXPERIMENT_RUNG_PATHS, "experiment rung")?;
    if experiment_rung.trim().is_empty() {
        bail!("experiment rung must not be empty");
    }
    let candidate_sweeps = consistent_u64(&root, CANDIDATE_SWEEPS_PATHS, "candidate sweep count")?;
    let samples = consistent_u64(&root, SAMPLE_COUNT_PATHS, "sample count")?;
    let evaluation_seed = consistent_u64(&root, EVALUATION_SEED_PATHS, "evaluation seed")?;
    let br_traversals =
        consistent_u64(&root, BR_TRAVERSALS_PATHS, "best-response traversal count")?;
    let training_seed = consistent_u64(&root, TRAINING_SEED_PATHS, "training seed")?;
    let profile = consistent_string(&root, PROFILE_PATHS, "strategy profile")?;
    if profile.trim().is_empty() {
        bail!("strategy profile must not be empty");
    }
    let purify_threshold = consistent_f64(&root, PURIFY_THRESHOLD_PATHS, "purification threshold")?;
    let worlds_value = first_value(&root, WORLDS_PATHS, "worlds")?.clone();
    let worlds: Vec<WorldInput> =
        serde_json::from_value(worlds_value).context("decoding per-world gains")?;

    let identifier = profile_identifier(&root);
    let reference_identifier = root
        .pointer("/reference")
        .cloned()
        .unwrap_or_else(|| Value::String(reference_fingerprint.clone()));
    let candidate_policy_coverage = optional_first_value(&root, CANDIDATE_POLICY_COVERAGE_PATHS)
        .cloned()
        .unwrap_or(Value::Null);
    let replay_coverage = optional_first_value(&root, REPLAY_COVERAGE_PATHS)
        .cloned()
        .unwrap_or(Value::Null);
    let training_coverage = optional_first_value(&root, TRAINING_COVERAGE_PATHS)
        .cloned()
        .unwrap_or(Value::Null);

    Ok(ParsedProfile {
        source,
        identifier,
        reference_identifier,
        reference_fingerprint,
        candidate_game_fingerprint,
        experiment_rung,
        candidate_sweeps,
        samples,
        evaluation_seed,
        br_traversals,
        training_seed,
        profile,
        purify_threshold,
        worlds,
        candidate_policy_coverage,
        replay_coverage,
        training_coverage,
    })
}

fn profile_identifier(root: &Value) -> Value {
    if let Some(candidate) = root.pointer("/candidate") {
        return candidate.clone();
    }
    for pointer in ["/candidate_id", "/candidateId", "/id"] {
        if let Some(identifier) = root.pointer(pointer) {
            return identifier.clone();
        }
    }

    let mut fields = Map::new();
    for key in [
        "candidate_config_path",
        "candidateConfigPath",
        "candidate_config_fingerprint",
        "candidateConfigFingerprint",
        "candidate_game_fingerprint",
        "candidateGameFingerprint",
        "candidate_abstraction_fingerprint",
        "candidateAbstractionFingerprint",
    ] {
        if let Some(value) = root.get(key) {
            fields.insert(key.to_owned(), value.clone());
        }
    }
    if fields.is_empty() {
        Value::Null
    } else {
        Value::Object(fields)
    }
}

fn first_value<'a>(root: &'a Value, paths: &[&str], label: &str) -> Result<&'a Value> {
    optional_first_value(root, paths)
        .ok_or_else(|| anyhow!("missing {label}; expected one of {}", paths.join(", ")))
}

fn optional_first_value<'a>(root: &'a Value, paths: &[&str]) -> Option<&'a Value> {
    paths.iter().find_map(|path| root.pointer(path))
}

fn consistent_string(root: &Value, paths: &[&str], label: &str) -> Result<String> {
    optional_consistent_string(root, paths, label)?
        .ok_or_else(|| anyhow!("missing {label}; expected one of {}", paths.join(", ")))
}

fn optional_consistent_string(root: &Value, paths: &[&str], label: &str) -> Result<Option<String>> {
    let values = paths
        .iter()
        .filter_map(|path| root.pointer(path).map(|value| (*path, value)))
        .map(|(path, value)| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("{label} at {path} must be a string"))
        })
        .collect::<Result<Vec<_>>>()?;
    let Some(first) = values.first() else {
        return Ok(None);
    };
    if values.iter().any(|value| value != first) {
        bail!("{label} fields disagree within one report");
    }
    Ok(Some(first.clone()))
}

fn consistent_u64(root: &Value, paths: &[&str], label: &str) -> Result<u64> {
    let values = paths
        .iter()
        .filter_map(|path| root.pointer(path).map(|value| (*path, value)))
        .map(|(path, value)| {
            value
                .as_u64()
                .ok_or_else(|| anyhow!("{label} at {path} must be a nonnegative integer"))
        })
        .collect::<Result<Vec<_>>>()?;
    let Some(first) = values.first() else {
        bail!("missing {label}; expected one of {}", paths.join(", "));
    };
    if values.iter().any(|value| value != first) {
        bail!("{label} fields disagree within one report");
    }
    Ok(*first)
}

fn consistent_f64(root: &Value, paths: &[&str], label: &str) -> Result<f64> {
    let values = paths
        .iter()
        .filter_map(|path| root.pointer(path).map(|value| (*path, value)))
        .map(|(path, value)| {
            let value = value
                .as_f64()
                .ok_or_else(|| anyhow!("{label} at {path} must be a finite number"))?;
            if !value.is_finite() {
                bail!("{label} at {path} must be a finite number");
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()?;
    let Some(first) = values.first() else {
        bail!("missing {label}; expected one of {}", paths.join(", "));
    };
    if values
        .iter()
        .any(|value| value.to_bits() != first.to_bits())
    {
        bail!("{label} fields disagree within one report");
    }
    Ok(*first)
}

fn validate_and_pair(left: &mut ParsedProfile, right: &mut ParsedProfile) -> Result<usize> {
    if left.reference_fingerprint != right.reference_fingerprint {
        bail!(
            "reference abstraction fingerprints differ: left={} right={}",
            left.reference_fingerprint,
            right.reference_fingerprint
        );
    }
    if left.candidate_game_fingerprint != right.candidate_game_fingerprint {
        bail!(
            "candidate game fingerprints differ: left={} right={}",
            left.candidate_game_fingerprint,
            right.candidate_game_fingerprint
        );
    }
    if left.experiment_rung != right.experiment_rung {
        bail!(
            "experiment rungs differ: left={} right={}",
            left.experiment_rung,
            right.experiment_rung
        );
    }
    if left.candidate_sweeps != right.candidate_sweeps {
        bail!(
            "candidate sweep counts differ: left={} right={}",
            left.candidate_sweeps,
            right.candidate_sweeps
        );
    }
    if left.br_traversals != right.br_traversals {
        bail!(
            "best-response traversal counts differ: left={} right={}",
            left.br_traversals,
            right.br_traversals
        );
    }
    if left.training_seed != right.training_seed {
        bail!(
            "training seeds differ: left={} right={}",
            left.training_seed,
            right.training_seed
        );
    }
    if left.profile != right.profile {
        bail!(
            "strategy profiles differ: left={} right={}",
            left.profile,
            right.profile
        );
    }
    if left.purify_threshold.to_bits() != right.purify_threshold.to_bits() {
        bail!(
            "purification thresholds differ: left={} right={}",
            left.purify_threshold,
            right.purify_threshold
        );
    }
    if left.samples != right.samples {
        bail!(
            "evaluation sample counts differ: left={} right={}",
            left.samples,
            right.samples
        );
    }
    if left.evaluation_seed != right.evaluation_seed {
        bail!(
            "evaluation seeds differ: left={} right={}",
            left.evaluation_seed,
            right.evaluation_seed
        );
    }
    if left.samples == 0 {
        bail!("evaluation sample count must be positive");
    }
    let expected_samples = usize::try_from(left.samples)
        .context("evaluation sample count does not fit this platform")?;
    if left.worlds.len() != expected_samples || right.worlds.len() != expected_samples {
        bail!(
            "world count must equal samples={expected_samples}: left={} right={}",
            left.worlds.len(),
            right.worlds.len()
        );
    }

    left.worlds.sort_by_key(|world| world.sample_id);
    right.worlds.sort_by_key(|world| world.sample_id);
    let left_ids = left
        .worlds
        .iter()
        .map(|world| world.sample_id)
        .collect::<Vec<_>>();
    let right_ids = right
        .worlds
        .iter()
        .map(|world| world.sample_id)
        .collect::<Vec<_>>();
    if left_ids.windows(2).any(|pair| pair[0] == pair[1])
        || right_ids.windows(2).any(|pair| pair[0] == pair[1])
    {
        bail!("sample IDs must be unique within each report");
    }
    if left_ids != right_ids {
        bail!("sample IDs differ between reports");
    }

    let seat_count = left
        .worlds
        .first()
        .map(|world| world.gains.len())
        .unwrap_or(0);
    if seat_count == 0 {
        bail!("each world must contain at least one seat gain");
    }
    validate_world_shape("left", &left.worlds, seat_count)?;
    validate_world_shape("right", &right.worlds, seat_count)?;
    validate_coverage_shape(
        "left candidate-policy coverage",
        &left.candidate_policy_coverage,
        seat_count,
    )?;
    validate_coverage_shape(
        "right candidate-policy coverage",
        &right.candidate_policy_coverage,
        seat_count,
    )?;
    validate_coverage_shape("left replay coverage", &left.replay_coverage, seat_count)?;
    validate_coverage_shape("right replay coverage", &right.replay_coverage, seat_count)?;
    validate_coverage_shape(
        "left training coverage",
        &left.training_coverage,
        seat_count,
    )?;
    validate_coverage_shape(
        "right training coverage",
        &right.training_coverage,
        seat_count,
    )?;
    Ok(seat_count)
}

fn validate_world_shape(label: &str, worlds: &[WorldInput], seat_count: usize) -> Result<()> {
    for world in worlds {
        if world.gains.len() != seat_count {
            bail!(
                "{label} sample {} has {} gains, expected {seat_count}",
                world.sample_id,
                world.gains.len()
            );
        }
        for (field, values) in [
            ("baseline_utilities", world.baseline_utilities.as_ref()),
            (
                "deviating_seat_utilities",
                world.deviating_seat_utilities.as_ref(),
            ),
        ] {
            if let Some(values) = values
                && values.len() != seat_count
            {
                bail!(
                    "{label} sample {} has {} {field}, expected {seat_count}",
                    world.sample_id,
                    values.len()
                );
            }
        }
    }
    Ok(())
}

fn validate_coverage_shape(label: &str, coverage: &Value, seat_count: usize) -> Result<()> {
    if coverage.is_null() {
        return Ok(());
    }
    let values = coverage
        .as_array()
        .ok_or_else(|| anyhow!("{label} must be a seat-indexed array or null"))?;
    if values.len() != seat_count {
        bail!("{label} has {} seats, expected {seat_count}", values.len());
    }
    Ok(())
}

fn paired_bootstrap(
    left: &[WorldInput],
    right: &[WorldInput],
    replicates: usize,
    seed: u64,
) -> Result<BootstrapResult> {
    if replicates == 0 {
        bail!("bootstrap replicates must be positive");
    }
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        bail!("paired bootstrap requires equally sized nonempty reports");
    }
    let seat_count = left[0].gains.len();
    if seat_count == 0
        || left.iter().any(|world| world.gains.len() != seat_count)
        || right.iter().any(|world| world.gains.len() != seat_count)
    {
        bail!("paired bootstrap requires a common nonzero seat shape");
    }

    let left_means = per_seat_means(left, seat_count);
    let right_means = per_seat_means(right, seat_count);
    let left_observed = max_clamped_mean(&left_means);
    let right_observed = max_clamped_mean(&right_means);

    let mut rng = StdRng::seed_from_u64(seed);
    let mut deltas = Vec::with_capacity(replicates);
    let mut right_greater = 0usize;
    let mut left_sums = vec![0.0; seat_count];
    let mut right_sums = vec![0.0; seat_count];
    let divisor = left.len() as f64;
    for _ in 0..replicates {
        left_sums.fill(0.0);
        right_sums.fill(0.0);
        for _ in 0..left.len() {
            let index = rng.gen_range(0..left.len());
            for seat in 0..seat_count {
                left_sums[seat] += left[index].gains[seat];
                right_sums[seat] += right[index].gains[seat];
            }
        }
        for value in &mut left_sums {
            *value /= divisor;
        }
        for value in &mut right_sums {
            *value /= divisor;
        }
        let delta = max_clamped_mean(&right_sums) - max_clamped_mean(&left_sums);
        if delta > 0.0 {
            right_greater += 1;
        }
        deltas.push(delta);
    }
    deltas.sort_by(f64::total_cmp);

    Ok(BootstrapResult {
        left_per_seat_raw_mean_gains: left_means,
        right_per_seat_raw_mean_gains: right_means,
        left_max_clamped_mean_gain: left_observed,
        right_max_clamped_mean_gain: right_observed,
        observed_delta_right_minus_left: right_observed - left_observed,
        percentile_ci95: [quantile(&deltas, 0.025), quantile(&deltas, 0.975)],
        probability_right_greater_than_left: right_greater as f64 / replicates as f64,
    })
}

fn per_seat_means(worlds: &[WorldInput], seat_count: usize) -> Vec<f64> {
    let mut means = vec![0.0; seat_count];
    for world in worlds {
        for (mean, gain) in means.iter_mut().zip(&world.gains) {
            *mean += gain;
        }
    }
    let divisor = worlds.len() as f64;
    for mean in &mut means {
        *mean /= divisor;
    }
    means
}

fn max_clamped_mean(means: &[f64]) -> f64 {
    means
        .iter()
        .copied()
        .map(|mean| mean.max(0.0))
        .fold(0.0, f64::max)
}

fn quantile(sorted: &[f64], probability: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    debug_assert!((0.0..=1.0).contains(&probability));
    let position = probability * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = position - lower as f64;
        sorted[lower] + fraction * (sorted[upper] - sorted[lower])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_BOOTSTRAP_SEED, ParsedProfile, WorldInput, paired_bootstrap, parse_profile,
        quantile, validate_and_pair,
    };
    use serde_json::{Value, json};

    fn world(sample_id: u64, gains: &[f64]) -> WorldInput {
        WorldInput {
            sample_id,
            gains: gains.to_vec(),
            baseline_utilities: None,
            deviating_seat_utilities: None,
        }
    }

    fn profile(fingerprint: &str, sample_ids: &[u64], gains: &[&[f64]]) -> ParsedProfile {
        ParsedProfile {
            source: "test".to_owned(),
            identifier: Value::String("candidate".to_owned()),
            reference_identifier: Value::String(fingerprint.to_owned()),
            reference_fingerprint: fingerprint.to_owned(),
            candidate_game_fingerprint: "game".to_owned(),
            experiment_rung: "s1".to_owned(),
            candidate_sweeps: 500,
            samples: sample_ids.len() as u64,
            evaluation_seed: 42,
            br_traversals: 1_000,
            training_seed: 123,
            profile: "average".to_owned(),
            purify_threshold: 0.0,
            worlds: sample_ids
                .iter()
                .copied()
                .zip(gains)
                .map(|(sample_id, gains)| world(sample_id, gains))
                .collect(),
            candidate_policy_coverage: Value::Null,
            replay_coverage: Value::Null,
            training_coverage: Value::Null,
        }
    }

    #[test]
    fn parses_common_reference_report_and_checks_duplicate_metadata() {
        let parsed = parse_profile(
            json!({
                "experiment": {"rung": "s1"},
                "candidate": {
                    "id": "T-R256",
                    "game_fingerprint": "game",
                    "sweeps": 500
                },
                "reference": {
                    "abstraction_fingerprint": "abc",
                    "game_fingerprint": "game",
                    "recall": "bucket-history"
                },
                "reference_abstraction_fingerprint": "abc",
                "samples": 2,
                "seed": 99,
                "br_traversals": 1000,
                "training_seed": 123,
                "profile": "average",
                "purify_threshold": 0.0,
                "training_coverage": [{}, {}],
                "evaluation": {
                    "evaluation": {"samples": 2},
                    "candidate_policy_coverage": [{}, {}],
                    "coverage": [{}, {}],
                    "worlds": [
                        {"sample_id": 0, "gains": [1.0, -1.0]},
                        {"sample_id": 1, "gains": [3.0, 1.0]}
                    ]
                }
            }),
            "input.json".to_owned(),
        )
        .unwrap();
        assert_eq!(parsed.reference_fingerprint, "abc");
        assert_eq!(parsed.samples, 2);
        assert_eq!(parsed.evaluation_seed, 99);
        assert_eq!(parsed.candidate_game_fingerprint, "game");
        assert_eq!(parsed.experiment_rung, "s1");
        assert_eq!(parsed.candidate_sweeps, 500);
        assert_eq!(parsed.br_traversals, 1_000);
        assert_eq!(parsed.training_seed, 123);
        assert_eq!(parsed.profile, "average");
        assert_eq!(parsed.purify_threshold, 0.0);
        assert_eq!(parsed.worlds.len(), 2);
        assert_eq!(
            parsed.identifier,
            json!({
                "id": "T-R256",
                "game_fingerprint": "game",
                "sweeps": 500
            })
        );
    }

    #[test]
    fn bootstrap_is_deterministic_and_clamps_each_seat_before_maximum() {
        let left = vec![
            world(0, &[-2.0, -1.0]),
            world(1, &[-4.0, -3.0]),
            world(2, &[-6.0, -5.0]),
        ];
        let right = vec![
            world(0, &[-1.0, 2.0]),
            world(1, &[-3.0, 4.0]),
            world(2, &[-5.0, 6.0]),
        ];
        let first = paired_bootstrap(&left, &right, 1_000, DEFAULT_BOOTSTRAP_SEED).unwrap();
        let second = paired_bootstrap(&left, &right, 1_000, DEFAULT_BOOTSTRAP_SEED).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.left_per_seat_raw_mean_gains, vec![-4.0, -3.0]);
        assert_eq!(first.right_per_seat_raw_mean_gains, vec![-3.0, 4.0]);
        assert_eq!(first.left_max_clamped_mean_gain, 0.0);
        assert_eq!(first.right_max_clamped_mean_gain, 4.0);
        assert_eq!(first.observed_delta_right_minus_left, 4.0);
    }

    #[test]
    fn pairing_sorts_ids_but_rejects_different_ids_and_reference() {
        let mut left = profile("same", &[1, 0], &[&[1.0], &[2.0]]);
        let mut right = profile("same", &[0, 1], &[&[3.0], &[4.0]]);
        assert_eq!(validate_and_pair(&mut left, &mut right).unwrap(), 1);
        assert_eq!(
            left.worlds
                .iter()
                .map(|world| world.sample_id)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );

        let mut wrong_ids = profile("same", &[0, 2], &[&[3.0], &[4.0]]);
        assert!(
            validate_and_pair(&mut left.clone(), &mut wrong_ids)
                .unwrap_err()
                .to_string()
                .contains("sample IDs differ")
        );

        let mut wrong_reference = profile("other", &[0, 1], &[&[3.0], &[4.0]]);
        assert!(
            validate_and_pair(&mut left, &mut wrong_reference)
                .unwrap_err()
                .to_string()
                .contains("fingerprints differ")
        );
    }

    #[test]
    fn pairing_rejects_inconsistent_seat_shapes_and_coverage() {
        let mut left = profile("same", &[0, 1], &[&[1.0, 2.0], &[3.0, 4.0]]);
        let mut wrong_world = profile("same", &[0, 1], &[&[1.0, 2.0], &[3.0]]);
        assert!(
            validate_and_pair(&mut left.clone(), &mut wrong_world)
                .unwrap_err()
                .to_string()
                .contains("gains")
        );

        left.replay_coverage = json!([{}]);
        let mut right = profile("same", &[0, 1], &[&[1.0, 2.0], &[3.0, 4.0]]);
        assert!(
            validate_and_pair(&mut left, &mut right)
                .unwrap_err()
                .to_string()
                .contains("coverage has 1 seats")
        );
    }

    #[test]
    fn pairing_rejects_every_noncomparable_experiment_condition() {
        fn assert_rejected(left: &ParsedProfile, mut right: ParsedProfile, expected: &str) {
            assert!(
                validate_and_pair(&mut left.clone(), &mut right)
                    .unwrap_err()
                    .to_string()
                    .contains(expected),
                "expected mismatch mentioning {expected}"
            );
        }

        let left = profile("same", &[0, 1], &[&[1.0], &[2.0]]);

        let mut right = left.clone();
        right.candidate_game_fingerprint = "other-game".to_owned();
        assert_rejected(&left, right, "candidate game fingerprints differ");

        let mut right = left.clone();
        right.experiment_rung = "s3".to_owned();
        assert_rejected(&left, right, "experiment rungs differ");

        let mut right = left.clone();
        right.candidate_sweeps += 1;
        assert_rejected(&left, right, "candidate sweep counts differ");

        let mut right = left.clone();
        right.br_traversals += 1;
        assert_rejected(&left, right, "best-response traversal counts differ");

        let mut right = left.clone();
        right.training_seed += 1;
        assert_rejected(&left, right, "training seeds differ");

        let mut right = left.clone();
        right.profile = "current".to_owned();
        assert_rejected(&left, right, "strategy profiles differ");

        let mut right = left.clone();
        right.purify_threshold = 0.01;
        assert_rejected(&left, right, "purification thresholds differ");
    }

    #[test]
    fn percentile_uses_linear_interpolation() {
        assert_eq!(quantile(&[0.0, 10.0, 20.0, 30.0, 40.0], 0.25), 10.0);
        assert_eq!(quantile(&[0.0, 10.0, 20.0, 30.0], 0.5), 15.0);
    }
}
