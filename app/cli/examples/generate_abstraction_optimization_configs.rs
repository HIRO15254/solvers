//! Generates deterministic compatibility configs for the staged, research-only
//! abstraction optimization experiment.
//!
//! The experiment's canonical v1 configs remain its historical source of
//! truth; rollout/full-recall variants are not production inputs. Generated
//! compatibility configs exist only to expose the operational abstraction
//! cache while keeping the exact same game/tree fingerprint.
//!
//! Usage:
//! `cargo run --release -p cli --features research \
//!   --example generate_abstraction_optimization_configs -- \
//!   MANIFEST OUTPUT_DIR CACHE_DIR`

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use cli::config::{GameSection, RakeSection, SolveConfig, UtilitySection};
use multiway::config::{FieldPlayerConfig, RakeConfig, UtilityConfig};
use multiway::{ExternalSamplingGame, FeatureHashAbstraction, HoldemGame};
use serde::{Deserialize, Serialize};
use serde_json::json;
use toml::Value;

const SCHEMA: &str = "solvers.abstraction-optimization/v1";
const MAX_LOCAL_RSS_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const TOURNAMENT_GAME_FINGERPRINT: &str =
    "a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512";
const CASH_GAME_FINGERPRINT: &str =
    "c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    selection: Selection,
    #[serde(default)]
    seed_pair: Vec<SeedPair>,
    #[serde(default)]
    rung: Vec<Rung>,
    #[serde(default)]
    candidate: Vec<ModelSpec>,
    #[serde(default)]
    reference: Vec<ModelSpec>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    cash_noninferiority_bb: f64,
    tournament_noninferiority_prize_fraction: f64,
    bootstrap_replicates: u64,
    local_solver_memory_bytes: u64,
    local_rss_stop_bytes: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SeedPair {
    abstraction: u64,
    solver: u64,
    evaluation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Rung {
    id: String,
    sweeps: u64,
    seed_pairs: usize,
    evaluation_samples: u64,
    deviator_traversals_per_seat: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    survivors_per_case: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_survivors_per_case: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    only_if_unresolved: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelSpec {
    #[serde(default)]
    case: Option<String>,
    id: String,
    kind: String,
    flop_buckets: u32,
    turn_buckets: u32,
    river_buckets: u32,
    #[serde(default)]
    rollout_samples: Option<u32>,
    #[serde(default)]
    points_per_bucket: Option<u32>,
    #[serde(default)]
    kmeans_iterations: Option<u32>,
    #[serde(default)]
    seed: Option<u64>,
    recall: String,
    #[serde(default)]
    final_only: bool,
    #[serde(default)]
    rungs: Option<Vec<String>>,
}

struct CaseBase {
    name: &'static str,
    canonical_path: PathBuf,
    compatibility_path: PathBuf,
    source_label: &'static str,
    expected_game_fingerprint: &'static str,
}

struct GeneratedConfig {
    config_hash: String,
    game_fingerprint: String,
}

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        bail!(
            "usage: generate_abstraction_optimization_configs \
             MANIFEST OUTPUT_DIR CACHE_DIR"
        );
    }
    let manifest_path = absolute_existing_path(Path::new(&args[0]))?;
    let output_dir = absolute_directory(Path::new(&args[1]))?;
    let cache_dir = absolute_directory(Path::new(&args[2]))?;
    ensure_csv_safe_path(&output_dir, "OUTPUT_DIR")?;
    ensure_csv_safe_path(&cache_dir, "CACHE_DIR")?;
    generate(&manifest_path, &output_dir, &cache_dir, &workspace_root()?)
}

fn generate(
    manifest_path: &Path,
    output_dir: &Path,
    cache_dir: &Path,
    workspace: &Path,
) -> Result<()> {
    let manifest_bytes =
        fs::read(manifest_path).with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest_raw =
        std::str::from_utf8(&manifest_bytes).context("optimization manifest is not valid UTF-8")?;
    let manifest: Manifest =
        toml::from_str(manifest_raw).context("parsing optimization manifest")?;
    validate_manifest(&manifest)?;

    let bases = [
        CaseBase {
            name: "tournament",
            canonical_path: workspace.join(
                "experiments/abstraction-2026-07-23/\
                 tournament-6max-50bb-benchmark-v1.toml",
            ),
            compatibility_path: workspace.join(
                "experiments/abstraction-2026-07-23/\
                 tournament-6max-50bb-one-size-postflop.toml",
            ),
            source_label: "experiments/abstraction-2026-07-23/\
                           tournament-6max-50bb-one-size-postflop.toml",
            expected_game_fingerprint: TOURNAMENT_GAME_FINGERPRINT,
        },
        CaseBase {
            name: "cash",
            canonical_path: workspace.join(
                "experiments/abstraction-2026-07-23/\
                 cash-6max-100bb-benchmark-v1.toml",
            ),
            compatibility_path: workspace.join(
                "experiments/abstraction-2026-07-23/\
                 cash-6max-100bb-one-size-postflop.toml",
            ),
            source_label: "experiments/abstraction-2026-07-23/\
                           cash-6max-100bb-one-size-postflop.toml",
            expected_game_fingerprint: CASH_GAME_FINGERPRINT,
        },
    ];
    for base in &bases {
        validate_base_contract(base)?;
    }
    let final_sweeps = manifest
        .rung
        .last()
        .expect("manifest validation requires a rung")
        .sweeps;
    let progress_every = manifest
        .rung
        .first()
        .expect("manifest validation requires a rung")
        .sweeps;

    let mut index = String::from(
        "role,case,id,abstraction_seed,solver_seed,evaluation_seed,config,cache,\
         config_hash,game_fingerprint\n",
    );
    let mut generated_paths = HashSet::new();

    for spec in &manifest.candidate {
        let case = spec
            .case
            .as_deref()
            .ok_or_else(|| anyhow!("candidate {} is missing case", spec.id))?;
        let base = base_for_case(&bases, case)?;
        for pair in &manifest.seed_pair {
            let file_stem = format!("{}-a{}-s{}", spec.id, pair.abstraction, pair.solver);
            let config_path = output_dir.join(format!("{file_stem}.toml"));
            if !generated_paths.insert(config_path.clone()) {
                bail!("duplicate generated config path {}", config_path.display());
            }
            let cache_seed = (spec.kind == "rollout-kmeans").then_some(pair.abstraction);
            let cache_path = cache_path(cache_dir, spec, cache_seed)?;
            let generated = write_config(
                base,
                &config_path,
                spec,
                Some(pair.abstraction),
                pair.solver,
                &cache_path,
                manifest.selection.local_solver_memory_bytes,
                final_sweeps,
                progress_every,
            )?;
            push_index_row(
                &mut index,
                &[
                    "candidate".into(),
                    case.into(),
                    spec.id.clone(),
                    pair.abstraction.to_string(),
                    pair.solver.to_string(),
                    pair.evaluation.to_string(),
                    path_field(&config_path)?,
                    path_field(&cache_path)?,
                    generated.config_hash,
                    generated.game_fingerprint,
                ],
            )?;
        }
    }

    for spec in &manifest.reference {
        for base in &bases {
            let file_stem = format!("{}-reference-{}", base.name, spec.id);
            let config_path = output_dir.join(format!("{file_stem}.toml"));
            if !generated_paths.insert(config_path.clone()) {
                bail!("duplicate generated config path {}", config_path.display());
            }
            let cache_path = cache_path(cache_dir, spec, spec.seed)?;
            let pair = manifest
                .seed_pair
                .first()
                .expect("manifest validation requires a seed pair");
            let generated = write_config(
                base,
                &config_path,
                spec,
                spec.seed,
                pair.solver,
                &cache_path,
                manifest.selection.local_solver_memory_bytes,
                final_sweeps,
                progress_every,
            )?;
            push_index_row(
                &mut index,
                &[
                    "reference".into(),
                    base.name.into(),
                    spec.id.clone(),
                    spec.seed
                        .map_or_else(String::new, |value| value.to_string()),
                    pair.solver.to_string(),
                    pair.evaluation.to_string(),
                    path_field(&config_path)?,
                    path_field(&cache_path)?,
                    generated.config_hash,
                    generated.game_fingerprint,
                ],
            )?;
        }
    }

    let index_path = output_dir.join("configs.csv");
    write_atomic(&index_path, index.as_bytes())?;
    let metadata_path = output_dir.join("experiment-metadata.json");
    let reference_routing = manifest
        .reference
        .iter()
        .map(|reference| {
            json!({
                "id": reference.id,
                "rungs": effective_reference_rungs(reference, &manifest),
                "finalOnly": reference.final_only,
            })
        })
        .collect::<Vec<_>>();
    let metadata = serde_json::to_vec_pretty(&json!({
        "schema": SCHEMA,
        "manifestPath": manifest_path,
        "manifestFingerprint": formats::config_hash_hex(&formats::config_hash(&manifest_bytes)),
        "selection": manifest.selection,
        "seedPairs": manifest.seed_pair,
        "rungs": manifest.rung,
        "referenceRouting": reference_routing,
        "expectedGameFingerprints": {
            "tournament": TOURNAMENT_GAME_FINGERPRINT,
            "cash": CASH_GAME_FINGERPRINT,
        },
        "candidateConfigs": manifest.candidate.len() * manifest.seed_pair.len(),
        "referenceConfigs": manifest.reference.len() * bases.len(),
    }))?;
    write_atomic(&metadata_path, &[metadata.as_slice(), b"\n"].concat())?;
    println!(
        "generated candidates={} references={} seed_pairs={} index={} metadata={}",
        manifest.candidate.len(),
        manifest.reference.len(),
        manifest.seed_pair.len(),
        index_path.display(),
        metadata_path.display(),
    );
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema != SCHEMA {
        bail!(
            "unsupported optimization manifest schema {:?}",
            manifest.schema
        );
    }
    if !manifest.selection.cash_noninferiority_bb.is_finite()
        || manifest.selection.cash_noninferiority_bb < 0.0
    {
        bail!("selection.cash_noninferiority_bb must be finite and nonnegative");
    }
    if !manifest
        .selection
        .tournament_noninferiority_prize_fraction
        .is_finite()
        || manifest.selection.tournament_noninferiority_prize_fraction < 0.0
    {
        bail!("selection.tournament_noninferiority_prize_fraction must be finite and nonnegative");
    }
    if manifest.selection.bootstrap_replicates == 0 {
        bail!("selection.bootstrap_replicates must be positive");
    }
    if manifest.selection.local_solver_memory_bytes == 0 {
        bail!("selection.local_solver_memory_bytes must be positive");
    }
    if manifest.selection.local_rss_stop_bytes <= manifest.selection.local_solver_memory_bytes {
        bail!(
            "selection.local_rss_stop_bytes must exceed local_solver_memory_bytes \
             so solver state can stop before the process ceiling"
        );
    }
    if manifest.selection.local_rss_stop_bytes > MAX_LOCAL_RSS_BYTES {
        bail!("selection.local_rss_stop_bytes exceeds the experiment's 8 GiB local process limit");
    }
    if manifest.seed_pair.is_empty() {
        bail!("manifest must contain at least one seed_pair");
    }
    let mut seeds = HashSet::new();
    for pair in &manifest.seed_pair {
        if !seeds.insert((pair.abstraction, pair.solver, pair.evaluation)) {
            bail!(
                "duplicate seed_pair ({}, {}, {})",
                pair.abstraction,
                pair.solver,
                pair.evaluation
            );
        }
    }
    if manifest.rung.is_empty() {
        bail!("manifest must contain at least one rung");
    }
    let mut rung_ids = HashSet::new();
    let mut previous_sweeps = 0;
    let first_sweeps = manifest.rung[0].sweeps;
    for rung in &manifest.rung {
        validate_identifier(&rung.id, "rung id")?;
        if !rung_ids.insert(rung.id.as_str()) {
            bail!("duplicate rung id {:?}", rung.id);
        }
        if rung.sweeps <= previous_sweeps {
            bail!("rung sweeps must be strictly increasing");
        }
        previous_sweeps = rung.sweeps;
        if rung.sweeps % first_sweeps != 0 {
            bail!(
                "rung {} sweeps must be a multiple of the first rung's {}-sweep \
                 checkpoint cadence",
                rung.id,
                first_sweeps
            );
        }
        if rung.seed_pairs == 0 || rung.seed_pairs > manifest.seed_pair.len() {
            bail!(
                "rung {} seed_pairs must be from 1 through {}",
                rung.id,
                manifest.seed_pair.len()
            );
        }
        if rung.evaluation_samples == 0 || rung.deviator_traversals_per_seat == 0 {
            bail!(
                "rung {} evaluation_samples and deviator_traversals_per_seat must be positive",
                rung.id
            );
        }
        if rung.survivors_per_case == Some(0) || rung.max_survivors_per_case == Some(0) {
            bail!("rung {} survivor counts must be positive", rung.id);
        }
        if let (Some(survivors), Some(maximum)) =
            (rung.survivors_per_case, rung.max_survivors_per_case)
            && survivors > maximum
        {
            bail!(
                "rung {} survivors_per_case exceeds max_survivors_per_case",
                rung.id
            );
        }
    }
    if manifest.candidate.is_empty() {
        bail!("manifest must contain at least one candidate");
    }
    if manifest.reference.is_empty()
        || manifest
            .reference
            .iter()
            .all(|reference| reference.final_only)
    {
        bail!("manifest must contain at least one non-final reference");
    }
    let mut model_ids = HashSet::new();
    for spec in &manifest.candidate {
        validate_model(spec, false)?;
        if !model_ids.insert(spec.id.as_str()) {
            bail!("duplicate model id {:?}", spec.id);
        }
    }
    for spec in &manifest.reference {
        validate_model(spec, true)?;
        if let Some(routed) = spec.rungs.as_deref() {
            if routed.is_empty() {
                bail!("reference {} rungs must not be empty", spec.id);
            }
            let mut seen = HashSet::new();
            for rung in routed {
                if !rung_ids.contains(rung.as_str()) {
                    bail!("reference {} names unknown rung {rung:?}", spec.id);
                }
                if !seen.insert(rung.as_str()) {
                    bail!("reference {} repeats rung {rung:?}", spec.id);
                }
            }
        }
        if !model_ids.insert(spec.id.as_str()) {
            bail!("duplicate model id {:?}", spec.id);
        }
    }
    for rung in &manifest.rung {
        if !manifest
            .reference
            .iter()
            .any(|reference| effective_reference_rungs(reference, manifest).contains(&rung.id))
        {
            bail!("rung {} has no routed reference model", rung.id);
        }
    }
    Ok(())
}

fn validate_model(spec: &ModelSpec, reference: bool) -> Result<()> {
    validate_identifier(&spec.id, "model id")?;
    if reference {
        if spec.case.is_some() {
            bail!("reference {} must not set case", spec.id);
        }
    } else {
        if spec.rungs.is_some() {
            bail!("candidate {} must not set reference-only rungs", spec.id);
        }
        if !matches!(spec.case.as_deref(), Some("tournament" | "cash")) {
            bail!(
                "candidate {} case must be \"tournament\" or \"cash\"",
                spec.id
            );
        }
        if spec.final_only {
            bail!("candidate {} cannot be final_only", spec.id);
        }
        if spec.seed.is_some() {
            bail!(
                "candidate {} must use seed_pair abstraction seeds, not a fixed seed",
                spec.id
            );
        }
    }
    for (name, value) in [
        ("flop_buckets", spec.flop_buckets),
        ("turn_buckets", spec.turn_buckets),
        ("river_buckets", spec.river_buckets),
    ] {
        if value == 0 || value > u32::from(u16::MAX) {
            bail!("{} {} must be from 1 through {}", spec.id, name, u16::MAX);
        }
    }
    if !matches!(spec.recall.as_str(), "full" | "street") {
        bail!("{} recall must be \"full\" or \"street\"", spec.id);
    }
    match spec.kind.as_str() {
        "rollout-kmeans" => {
            if spec.rollout_samples == Some(0)
                || spec.points_per_bucket == Some(0)
                || spec.kmeans_iterations == Some(0)
                || spec.rollout_samples.is_none()
                || spec.points_per_bucket.is_none()
                || spec.kmeans_iterations.is_none()
            {
                bail!(
                    "{} rollout_samples, points_per_bucket, and kmeans_iterations \
                     must all be positive",
                    spec.id
                );
            }
            if reference && spec.seed.is_none() {
                bail!("rollout reference {} must set seed", spec.id);
            }
        }
        "ehs2-table" => {
            if spec.rollout_samples.is_some()
                || spec.points_per_bucket.is_some()
                || spec.kmeans_iterations.is_some()
                || spec.seed.is_some()
            {
                bail!(
                    "ehs2-table model {} forbids rollout training parameters and seed",
                    spec.id
                );
            }
        }
        other => bail!("unsupported abstraction kind {other:?}"),
    }
    Ok(())
}

fn effective_reference_rungs(spec: &ModelSpec, manifest: &Manifest) -> Vec<String> {
    if let Some(rungs) = spec.rungs.as_ref() {
        return rungs.clone();
    }
    if spec.final_only {
        manifest
            .rung
            .last()
            .map(|rung| vec![rung.id.clone()])
            .unwrap_or_default()
    } else {
        manifest.rung.iter().map(|rung| rung.id.clone()).collect()
    }
}

fn validate_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("{label} {value:?} must use only ASCII letters, digits, '.', '_', or '-'");
    }
    Ok(())
}

fn validate_base_contract(base: &CaseBase) -> Result<()> {
    let canonical = fingerprint_file(&base.canonical_path)?;
    let compatibility = fingerprint_file(&base.compatibility_path)?;
    if canonical != compatibility {
        bail!(
            "{} canonical and compatibility configs have different game fingerprints: \
             {canonical} != {compatibility}",
            base.name
        );
    }
    if canonical != base.expected_game_fingerprint {
        bail!(
            "{} game fingerprint changed from the benchmark contract: {} != {}",
            base.name,
            canonical,
            base.expected_game_fingerprint
        );
    }
    Ok(())
}

fn base_for_case<'a>(bases: &'a [CaseBase; 2], case: &str) -> Result<&'a CaseBase> {
    bases
        .iter()
        .find(|base| base.name == case)
        .ok_or_else(|| anyhow!("unknown candidate case {case:?}"))
}

fn cache_path(cache_dir: &Path, spec: &ModelSpec, seed: Option<u64>) -> Result<PathBuf> {
    let name = match spec.kind.as_str() {
        "rollout-kmeans" => format!(
            "rollout-kmeans-f{}-t{}-r{}-rs{}-p{}-i{}-a{}.mwab",
            spec.flop_buckets,
            spec.turn_buckets,
            spec.river_buckets,
            spec.rollout_samples
                .ok_or_else(|| anyhow!("{} is missing rollout_samples", spec.id))?,
            spec.points_per_bucket
                .ok_or_else(|| anyhow!("{} is missing points_per_bucket", spec.id))?,
            spec.kmeans_iterations
                .ok_or_else(|| anyhow!("{} is missing kmeans_iterations", spec.id))?,
            seed.ok_or_else(|| anyhow!("{} is missing an abstraction seed", spec.id))?,
        ),
        "ehs2-table" => format!(
            "ehs2-table-f{}-t{}-r{}.postcard",
            spec.flop_buckets, spec.turn_buckets, spec.river_buckets
        ),
        other => bail!("unsupported abstraction kind {other:?}"),
    };
    Ok(cache_dir.join(name))
}

#[allow(clippy::too_many_arguments)]
fn write_config(
    base: &CaseBase,
    output_path: &Path,
    spec: &ModelSpec,
    abstraction_seed: Option<u64>,
    solver_seed: u64,
    cache_path: &Path,
    memory_bytes: u64,
    final_sweeps: u64,
    progress_every: u64,
) -> Result<GeneratedConfig> {
    let raw = fs::read_to_string(&base.compatibility_path)
        .with_context(|| format!("reading {}", base.compatibility_path.display()))?;
    let mut value: Value = toml::from_str(&raw)
        .with_context(|| format!("parsing {}", base.compatibility_path.display()))?;
    let root = value
        .as_table_mut()
        .ok_or_else(|| anyhow!("base config root is not a TOML table"))?;
    let game = root
        .get_mut("game")
        .and_then(Value::as_table_mut)
        .ok_or_else(|| anyhow!("base config is missing [game]"))?;
    let abstraction = game
        .get_mut("abstraction")
        .and_then(Value::as_table_mut)
        .ok_or_else(|| anyhow!("base config is missing [game.abstraction]"))?;
    abstraction.insert("kind".into(), Value::String(spec.kind.clone()));
    abstraction.insert(
        "flop_buckets".into(),
        Value::Integer(i64::from(spec.flop_buckets)),
    );
    abstraction.insert(
        "turn_buckets".into(),
        Value::Integer(i64::from(spec.turn_buckets)),
    );
    abstraction.insert(
        "river_buckets".into(),
        Value::Integer(i64::from(spec.river_buckets)),
    );
    abstraction.insert("recall".into(), Value::String(spec.recall.clone()));
    abstraction.insert(
        "artifact_cache".into(),
        Value::String(cache_path.to_string_lossy().into_owned()),
    );
    abstraction.remove("active_opponent_buckets");
    match spec.kind.as_str() {
        "rollout-kmeans" => {
            let rollout_samples = spec
                .rollout_samples
                .ok_or_else(|| anyhow!("{} is missing rollout_samples", spec.id))?;
            let points = spec
                .points_per_bucket
                .ok_or_else(|| anyhow!("{} is missing points_per_bucket", spec.id))?;
            let iterations = spec
                .kmeans_iterations
                .ok_or_else(|| anyhow!("{} is missing kmeans_iterations", spec.id))?;
            let seed = abstraction_seed
                .or(spec.seed)
                .ok_or_else(|| anyhow!("{} is missing an abstraction seed", spec.id))?;
            abstraction.insert(
                "rollout_samples".into(),
                Value::Integer(i64::from(rollout_samples)),
            );
            abstraction.insert(
                "points_per_bucket".into(),
                Value::Integer(i64::from(points)),
            );
            abstraction.insert(
                "kmeans_iterations".into(),
                Value::Integer(i64::from(iterations)),
            );
            abstraction.insert(
                "seed".into(),
                Value::Integer(
                    i64::try_from(seed).context("abstraction seed exceeds TOML integer range")?,
                ),
            );
        }
        "ehs2-table" => {
            abstraction.remove("rollout_samples");
            abstraction.remove("points_per_bucket");
            abstraction.remove("kmeans_iterations");
            abstraction.remove("seed");
        }
        other => bail!("unsupported abstraction kind {other:?}"),
    }

    let algorithm = root
        .get_mut("algorithm")
        .and_then(Value::as_table_mut)
        .ok_or_else(|| anyhow!("base config is missing [algorithm]"))?;
    algorithm.insert(
        "seed".into(),
        Value::Integer(
            i64::try_from(solver_seed).context("solver seed exceeds TOML integer range")?,
        ),
    );

    let run = root
        .get_mut("run")
        .and_then(Value::as_table_mut)
        .ok_or_else(|| anyhow!("base config is missing [run]"))?;
    run.insert(
        "sweeps".into(),
        Value::Integer(
            i64::try_from(final_sweeps).context("final rung sweeps exceed TOML integer range")?,
        ),
    );
    run.insert(
        "seed".into(),
        Value::Integer(
            i64::try_from(solver_seed).context("solver seed exceeds TOML integer range")?,
        ),
    );
    run.insert(
        "check_every".into(),
        Value::Integer(
            i64::try_from(progress_every).context("progress cadence exceeds TOML integer range")?,
        ),
    );
    run.insert(
        "evaluation_cadence".into(),
        Value::Integer(
            i64::try_from(progress_every)
                .context("evaluation cadence exceeds TOML integer range")?,
        ),
    );
    run.insert(
        "checkpoint_every".into(),
        Value::Integer(
            i64::try_from(progress_every)
                .context("checkpoint cadence exceeds TOML integer range")?,
        ),
    );
    run.insert(
        "max_memory_bytes".into(),
        Value::Integer(
            i64::try_from(memory_bytes)
                .context("solver memory limit exceeds TOML integer range")?,
        ),
    );

    let rendered = format!(
        "# Generated from {} by generate_abstraction_optimization_configs.\n\
         # Source of truth: experiments/abstraction-optimization-2026-07-25/manifest.toml\n\
         # final_only_reference = {}\n\n{}",
        base.source_label,
        spec.final_only,
        toml::to_string_pretty(&value)?
    );
    let parsed = cli::config::parse_solve_config_at(&rendered, output_path)
        .with_context(|| format!("validating generated config {}", output_path.display()))?;
    let game_fingerprint = game_fingerprint(parsed)?;
    if game_fingerprint != base.expected_game_fingerprint {
        bail!(
            "generated config {} changed the benchmark game fingerprint: {} != {}",
            output_path.display(),
            game_fingerprint,
            base.expected_game_fingerprint
        );
    }
    write_atomic(output_path, rendered.as_bytes())?;
    Ok(GeneratedConfig {
        config_hash: formats::config_hash_hex(&formats::config_hash(rendered.as_bytes())),
        game_fingerprint,
    })
}

fn fingerprint_file(path: &Path) -> Result<String> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let config = cli::config::parse_solve_config_at(&raw, path)
        .with_context(|| format!("parsing {}", path.display()))?;
    game_fingerprint(config)
}

fn game_fingerprint(config: SolveConfig) -> Result<String> {
    let SolveConfig {
        game,
        rake,
        utility,
        ..
    } = config;
    let GameSection::PreflopMultiway(game) = game else {
        bail!("benchmark config is not kind = \"preflop-multiway\"");
    };
    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
    let game = HoldemGame::new(&game, &utility, &rake, FeatureHashAbstraction::default())
        .context("building benchmark game for fingerprint verification")?;
    Ok(formats::config_hash_hex(&game.game_fingerprint()))
}

fn convert_utility(utility: UtilitySection) -> Result<UtilityConfig> {
    Ok(match utility {
        UtilitySection::ChipEv => UtilityConfig::ChipEv,
        UtilitySection::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => UtilityConfig::TournamentIcm {
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
            bail!("legacy HU utility kind = \"icm\" is not valid for the multiway benchmark");
        }
    })
}

fn convert_rake(rake: RakeSection) -> RakeConfig {
    match rake {
        RakeSection::None => RakeConfig::None,
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => RakeConfig::PercentCap {
            rate,
            cap_bb: cap / multiway::types::CHIPS_PER_BB as f64,
            no_flop_no_drop,
        },
        RakeSection::Generic {
            rate,
            cap,
            when,
            allocation,
            rounding,
        } => RakeConfig::Generic {
            rate,
            cap_bb: cap,
            when,
            allocation,
            rounding,
        },
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => RakeConfig::GgPreflop {
            rate,
            cap_bb: cap / multiway::types::CHIPS_PER_BB as f64,
            exempt_pot_bb: exempt_pot as f64 / 1_000.0,
        },
    }
}

fn push_index_row(index: &mut String, fields: &[String]) -> Result<()> {
    for field in fields {
        if field.contains([',', '\r', '\n']) {
            bail!("CSV index field contains a comma or newline: {field:?}");
        }
    }
    index.push_str(&fields.join(","));
    index.push('\n');
    Ok(())
}

fn path_field(path: &Path) -> Result<String> {
    let field = path.to_string_lossy().into_owned();
    if field.contains([',', '\r', '\n']) {
        bail!(
            "experiment paths containing commas or newlines are unsupported: {}",
            path.display()
        );
    }
    Ok(field)
}

fn ensure_csv_safe_path(path: &Path, label: &str) -> Result<()> {
    if path.to_string_lossy().contains([',', '\r', '\n']) {
        bail!("{label} may not contain commas or newlines");
    }
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("resolving the source workspace")
}

fn absolute_existing_path(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("resolving {}", path.display()))
}

fn absolute_directory(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    path.canonicalize()
        .with_context(|| format!("resolving {}", path.display()))
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary file beside {}", path.display()))?;
    temporary.write_all(contents)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| anyhow!(error.error))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_manifest_generates_valid_content_addressed_configs() {
        let workspace = workspace_root().unwrap();
        let manifest =
            workspace.join("experiments/abstraction-optimization-2026-07-25/manifest.toml");
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("configs");
        let cache = temporary.path().join("cache");
        fs::create_dir_all(&output).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let tracked: Manifest = toml::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();

        generate(&manifest, &output, &cache, &workspace).unwrap();

        let index = fs::read_to_string(output.join("configs.csv")).unwrap();
        let rows = index.lines().collect::<Vec<_>>();
        assert_eq!(
            rows.len(),
            1 + tracked.candidate.len() * tracked.seed_pair.len() + tracked.reference.len() * 2
        );
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("experiment-metadata.json")).unwrap())
                .unwrap();
        let routing = metadata["referenceRouting"].as_array().unwrap();
        assert_eq!(routing.len(), tracked.reference.len());
        assert!(
            routing
                .iter()
                .all(|route| !route["rungs"].as_array().unwrap().is_empty())
        );

        let candidate_cache = |id: &str, solver_seed: &str| {
            rows.iter()
                .skip(1)
                .map(|row| row.split(',').collect::<Vec<_>>())
                .find(|fields| {
                    fields[0] == "candidate" && fields[2] == id && fields[4] == solver_seed
                })
                .map(|fields| fields[7].to_string())
                .unwrap()
        };
        assert_eq!(
            candidate_cache("T-E64", "1011"),
            candidate_cache("C-E64", "1011"),
            "equivalent EHS models should share one content-addressed cache"
        );
        assert_ne!(
            candidate_cache("T-E64", "1011"),
            candidate_cache("T-E256", "1011"),
            "incompatible bucket counts must not collide"
        );

        for row in rows.iter().skip(1) {
            let fields = row.split(',').collect::<Vec<_>>();
            assert_eq!(fields.len(), 10);
            let raw = fs::read_to_string(fields[6]).unwrap();
            let config = cli::config::parse_solve_config_at(&raw, Path::new(fields[6])).unwrap();
            let GameSection::PreflopMultiway(game) = config.game else {
                panic!("generated a non-multiway config")
            };
            game.validate().unwrap();
            assert!(Path::new(fields[6]).is_absolute());
            assert!(Path::new(fields[7]).is_absolute());
            assert_eq!(
                fields[8],
                formats::config_hash_hex(&formats::config_hash(raw.as_bytes()))
            );
        }
    }

    #[test]
    fn reference_routing_rejects_duplicate_or_unknown_rungs() {
        let workspace = workspace_root().unwrap();
        let raw = fs::read_to_string(
            workspace.join("experiments/abstraction-optimization-2026-07-25/manifest.toml"),
        )
        .unwrap();
        let mut duplicate: Manifest = toml::from_str(&raw).unwrap();
        duplicate.reference[0].rungs = Some(vec!["s1".into(), "s1".into()]);
        assert!(
            validate_manifest(&duplicate)
                .unwrap_err()
                .to_string()
                .contains("repeats rung")
        );

        let mut unknown: Manifest = toml::from_str(&raw).unwrap();
        unknown.reference[0].rungs = Some(vec!["missing".into()]);
        assert!(
            validate_manifest(&unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown rung")
        );

        let mut candidate: Manifest = toml::from_str(&raw).unwrap();
        candidate.candidate[0].rungs = Some(vec!["s1".into()]);
        assert!(
            validate_manifest(&candidate)
                .unwrap_err()
                .to_string()
                .contains("reference-only rungs")
        );
    }
}
