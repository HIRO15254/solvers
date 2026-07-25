//! Generates the fixed research-only representative-envelope transfer configs
//! for one selected Tournament finalist and one selected Cash finalist.
//!
//! The generator emits a canonical v1 source config and an operational
//! compatibility twin (with a content-addressed abstraction-cache path) for
//! every scenario. It verifies that both twins lower to the same game, tree,
//! and abstraction specification before writing either file.
//!
//! Usage:
//! `cargo run --release -p cli --features research \
//!   --example generate_abstraction_transfer_configs -- \
//!   MANIFEST OUTPUT_DIR CACHE_DIR`

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use cli::config::{
    AlgorithmSection, GameSection, RakeSection, SolveConfig, StorageKind, UtilitySection,
};
use multiway::config::{
    AbstractionKind, AnteConfig, FieldPlayerConfig, RakeAllocation, RakeConfig, RakeRounding,
    RecallMode, UtilityConfig,
};
use multiway::{ExternalSamplingGame, FeatureHashAbstraction, HoldemGame};
use serde::{Deserialize, Serialize};
use serde_json::json;
use toml::Value;

const MANIFEST_SCHEMA: &str = "solvers.abstraction-transfer-validation/v1";
const OUTPUT_SCHEMA: &str = "solvers.abstraction-transfer-config-set/v1";
const MAX_PROCESS_MEMORY_BYTES: u64 = 8 * 1024 * 1024 * 1024;

// These are intentionally fixed independently of the input finalist. The
// card abstraction is excluded from game identity, so any finalist for a
// scenario must reproduce the same value.
const TOURNAMENT_6MAX_50BB_GAME_FINGERPRINT: &str =
    "a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512";
const CASH_6MAX_100BB_GAME_FINGERPRINT: &str =
    "c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b";

// Filled from the lowered canonical benchmark contracts. Keeping these
// constants separate from the source files makes accidental tree drift fail
// even if both a source and its generated twin are edited together.
const TOURNAMENT_TREE_CONTRACT_FINGERPRINT: &str =
    "97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604";
const CASH_TREE_CONTRACT_FINGERPRINT: &str =
    "bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum Case {
    Tournament,
    Cash,
}

impl Case {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tournament => "tournament",
            Self::Cash => "cash",
        }
    }
}

#[derive(Clone, Copy)]
struct Scenario {
    id: &'static str,
    case: Case,
    seats: u8,
    stack_bb: f64,
    expected_game_fingerprint: &'static str,
}

// Do not make this manifest-configurable: the fixed envelope prevents
// post-selection cherry-picking.
const SCENARIOS: [Scenario; 10] = [
    Scenario {
        id: "tournament-6max-5bb",
        case: Case::Tournament,
        seats: 6,
        stack_bb: 5.0,
        expected_game_fingerprint: "02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c",
    },
    Scenario {
        id: "tournament-6max-50bb",
        case: Case::Tournament,
        seats: 6,
        stack_bb: 50.0,
        expected_game_fingerprint: TOURNAMENT_6MAX_50BB_GAME_FINGERPRINT,
    },
    Scenario {
        id: "tournament-8max-20bb",
        case: Case::Tournament,
        seats: 8,
        stack_bb: 20.0,
        expected_game_fingerprint: "c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f",
    },
    Scenario {
        id: "tournament-9max-5bb",
        case: Case::Tournament,
        seats: 9,
        stack_bb: 5.0,
        expected_game_fingerprint: "0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f",
    },
    Scenario {
        id: "tournament-9max-50bb",
        case: Case::Tournament,
        seats: 9,
        stack_bb: 50.0,
        expected_game_fingerprint: "39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282",
    },
    Scenario {
        id: "cash-6max-100bb",
        case: Case::Cash,
        seats: 6,
        stack_bb: 100.0,
        expected_game_fingerprint: CASH_6MAX_100BB_GAME_FINGERPRINT,
    },
    Scenario {
        id: "cash-6max-800bb",
        case: Case::Cash,
        seats: 6,
        stack_bb: 800.0,
        expected_game_fingerprint: "9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5",
    },
    Scenario {
        id: "cash-8max-400bb",
        case: Case::Cash,
        seats: 8,
        stack_bb: 400.0,
        expected_game_fingerprint: "b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843",
    },
    Scenario {
        id: "cash-9max-100bb",
        case: Case::Cash,
        seats: 9,
        stack_bb: 100.0,
        expected_game_fingerprint: "488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa",
    },
    Scenario {
        id: "cash-9max-800bb",
        case: Case::Cash,
        seats: 9,
        stack_bb: 800.0,
        expected_game_fingerprint: "e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20",
    },
];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    run: RunSpec,
    #[serde(default)]
    finalist: Vec<Finalist>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RunSpec {
    max_sweeps: u64,
    check_every_sweeps: u64,
    evaluation_samples: u64,
    deviator_traversals: u64,
    threads: usize,
    memory: String,
    checkpoint_interval: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ModelKind {
    RolloutKmeans,
    Ehs2Table,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Recall {
    Full,
    Street,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Finalist {
    case: Case,
    id: String,
    kind: ModelKind,
    flop_buckets: u32,
    turn_buckets: u32,
    river_buckets: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rollout_samples: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    points_per_bucket: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kmeans_iterations: Option<u32>,
    recall: Recall,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    abstraction_seed: Option<u64>,
    solver_seed: u64,
    evaluation_seed: u64,
}

struct BaseContract {
    case: Case,
    canonical_path: PathBuf,
    canonical_source_fingerprint: String,
    document: Value,
    source_tree: Value,
    tree_contract_fingerprint: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedRecord {
    case: &'static str,
    scenario_id: &'static str,
    finalist_id: String,
    seats: u8,
    stack_bb: f64,
    abstraction_seed: Option<u64>,
    solver_seed: u64,
    evaluation_seed: u64,
    canonical_config: String,
    canonical_config_fingerprint: String,
    compatibility_config: String,
    compatibility_config_fingerprint: String,
    game_fingerprint: String,
    tree_contract_fingerprint: String,
    abstraction_spec_fingerprint: String,
    cache: String,
}

#[derive(Debug)]
struct RuntimeVerification {
    game_fingerprint: String,
    tree_contract_fingerprint: String,
    abstraction_spec_fingerprint: String,
}

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        bail!(
            "usage: generate_abstraction_transfer_configs \
             MANIFEST OUTPUT_DIR CACHE_DIR"
        );
    }
    let manifest_path = absolute_existing_file(Path::new(&args[0]))?;
    let output_dir = absolute_directory(Path::new(&args[1]))?;
    let cache_dir = absolute_directory(Path::new(&args[2]))?;
    generate(&manifest_path, &output_dir, &cache_dir, &workspace_root()?)
}

fn generate(
    manifest_path: &Path,
    output_dir: &Path,
    cache_dir: &Path,
    workspace: &Path,
) -> Result<()> {
    ensure_csv_safe_path(output_dir, "OUTPUT_DIR")?;
    ensure_csv_safe_path(cache_dir, "CACHE_DIR")?;
    let manifest_bytes =
        fs::read(manifest_path).with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest_raw =
        std::str::from_utf8(&manifest_bytes).context("transfer manifest is not valid UTF-8")?;
    let manifest: Manifest = toml::from_str(manifest_raw).context("parsing transfer manifest")?;
    validate_manifest(&manifest)?;

    let bases = BTreeMap::from([
        (
            Case::Tournament,
            load_base_contract(Case::Tournament, workspace)?,
        ),
        (Case::Cash, load_base_contract(Case::Cash, workspace)?),
    ]);
    let finalists = manifest
        .finalist
        .iter()
        .map(|finalist| (finalist.case, finalist))
        .collect::<BTreeMap<_, _>>();

    let mut records = Vec::with_capacity(SCENARIOS.len());
    let mut generated_paths = BTreeSet::new();
    for scenario in SCENARIOS {
        let finalist = finalists
            .get(&scenario.case)
            .copied()
            .expect("manifest validation requires one finalist per case");
        let base = bases.get(&scenario.case).expect("both bases are loaded");
        let cache = cache_path(cache_dir, finalist)?;
        let canonical_path =
            output_dir.join(format!("{}-{}-canonical-v1.toml", scenario.id, finalist.id));
        let compatibility_path =
            output_dir.join(format!("{}-{}-compat.toml", scenario.id, finalist.id));
        for path in [&canonical_path, &compatibility_path] {
            if !generated_paths.insert(path.clone()) {
                bail!("duplicate generated path {}", path.display());
            }
        }

        let canonical_document =
            build_canonical_document(base, &scenario, finalist, &manifest.run)?;
        let canonical_raw = render_canonical(&canonical_document, &scenario, finalist)?;
        let canonical = verify_config(
            &canonical_raw,
            &canonical_path,
            base,
            &scenario,
            finalist,
            &manifest.run,
            None,
        )?;
        enforce_scenario_fingerprint(&scenario, &canonical.game_fingerprint)?;

        let compatibility_raw = render_compatibility(
            &canonical_raw,
            &canonical_path,
            &scenario,
            finalist,
            &manifest.run,
            &cache,
        )?;
        let compatibility = verify_config(
            &compatibility_raw,
            &compatibility_path,
            base,
            &scenario,
            finalist,
            &manifest.run,
            Some(&cache),
        )?;
        if canonical.game_fingerprint != compatibility.game_fingerprint
            || canonical.tree_contract_fingerprint != compatibility.tree_contract_fingerprint
            || canonical.abstraction_spec_fingerprint != compatibility.abstraction_spec_fingerprint
        {
            bail!(
                "{} canonical and compatibility configs do not describe the same \
                 game/tree/abstraction specification",
                scenario.id
            );
        }

        write_atomic(&canonical_path, canonical_raw.as_bytes())?;
        write_atomic(&compatibility_path, compatibility_raw.as_bytes())?;
        records.push(GeneratedRecord {
            case: scenario.case.as_str(),
            scenario_id: scenario.id,
            finalist_id: finalist.id.clone(),
            seats: scenario.seats,
            stack_bb: scenario.stack_bb,
            abstraction_seed: finalist.abstraction_seed,
            solver_seed: finalist.solver_seed,
            evaluation_seed: finalist.evaluation_seed,
            canonical_config: path_field(&canonical_path)?,
            canonical_config_fingerprint: hash_hex(canonical_raw.as_bytes()),
            compatibility_config: path_field(&compatibility_path)?,
            compatibility_config_fingerprint: hash_hex(compatibility_raw.as_bytes()),
            game_fingerprint: canonical.game_fingerprint,
            tree_contract_fingerprint: canonical.tree_contract_fingerprint,
            abstraction_spec_fingerprint: canonical.abstraction_spec_fingerprint,
            cache: path_field(&cache)?,
        });
    }

    let index_path = output_dir.join("transfer-configs.csv");
    let mut index = String::from(
        "case,scenario_id,finalist_id,seats,stack_bb,abstraction_seed,solver_seed,\
         evaluation_seed,canonical_config,canonical_config_fingerprint,\
         compatibility_config,compatibility_config_fingerprint,game_fingerprint,\
         tree_contract_fingerprint,abstraction_spec_fingerprint,cache\n",
    );
    for record in &records {
        push_index_row(
            &mut index,
            &[
                record.case.into(),
                record.scenario_id.into(),
                record.finalist_id.clone(),
                record.seats.to_string(),
                record.stack_bb.to_string(),
                record
                    .abstraction_seed
                    .map_or_else(String::new, |seed| seed.to_string()),
                record.solver_seed.to_string(),
                record.evaluation_seed.to_string(),
                record.canonical_config.clone(),
                record.canonical_config_fingerprint.clone(),
                record.compatibility_config.clone(),
                record.compatibility_config_fingerprint.clone(),
                record.game_fingerprint.clone(),
                record.tree_contract_fingerprint.clone(),
                record.abstraction_spec_fingerprint.clone(),
                record.cache.clone(),
            ],
        )?;
    }
    write_atomic(&index_path, index.as_bytes())?;

    let metadata_path = output_dir.join("transfer-metadata.json");
    let metadata = serde_json::to_vec_pretty(&json!({
        "schema": OUTPUT_SCHEMA,
        "manifestPath": manifest_path,
        "manifestFingerprint": hash_hex(&manifest_bytes),
        "fixedEnvelope": SCENARIOS.iter().map(|scenario| json!({
            "id": scenario.id,
            "case": scenario.case,
            "seats": scenario.seats,
            "stackBb": scenario.stack_bb,
            "expectedGameFingerprint": scenario.expected_game_fingerprint,
        })).collect::<Vec<_>>(),
        "run": manifest.run,
        "finalists": manifest.finalist,
        "canonicalSources": bases.values().map(|base| json!({
            "case": base.case,
            "path": base.canonical_path,
            "configFingerprint": base.canonical_source_fingerprint,
            "treeContractFingerprint": base.tree_contract_fingerprint,
        })).collect::<Vec<_>>(),
        "configs": records,
    }))?;
    write_atomic(&metadata_path, &[metadata.as_slice(), b"\n"].concat())?;
    println!(
        "generated transfer_scenarios={} canonical_configs={} compatibility_configs={} \
         index={} metadata={}",
        SCENARIOS.len(),
        SCENARIOS.len(),
        SCENARIOS.len(),
        index_path.display(),
        metadata_path.display()
    );
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema != MANIFEST_SCHEMA {
        bail!(
            "unsupported transfer manifest schema {:?}; expected {:?}",
            manifest.schema,
            MANIFEST_SCHEMA
        );
    }
    if manifest.run.max_sweeps == 0
        || manifest.run.check_every_sweeps == 0
        || manifest.run.evaluation_samples == 0
        || manifest.run.deviator_traversals == 0
        || manifest.run.threads == 0
    {
        bail!("all numeric run controls must be positive");
    }
    if !manifest
        .run
        .max_sweeps
        .is_multiple_of(manifest.run.check_every_sweeps)
    {
        bail!("run.max_sweeps must be a multiple of run.check_every_sweeps");
    }
    if manifest.run.memory.trim().is_empty() || manifest.run.checkpoint_interval.trim().is_empty() {
        bail!("run.memory and run.checkpoint_interval must not be empty");
    }
    if manifest.finalist.len() != 2 {
        bail!("manifest must contain exactly one Tournament and one Cash finalist");
    }

    let mut cases = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for finalist in &manifest.finalist {
        validate_identifier(&finalist.id, "finalist id")?;
        if !cases.insert(finalist.case) {
            bail!(
                "manifest contains more than one {} finalist",
                finalist.case.as_str()
            );
        }
        if !ids.insert(finalist.id.as_str()) {
            bail!("duplicate finalist id {:?}", finalist.id);
        }
        for (field, value) in [
            ("flop_buckets", finalist.flop_buckets),
            ("turn_buckets", finalist.turn_buckets),
            ("river_buckets", finalist.river_buckets),
        ] {
            if value == 0 || value > u32::from(u16::MAX) {
                bail!(
                    "finalist {} {field} must be from 1 through {}",
                    finalist.id,
                    u16::MAX
                );
            }
        }
        for (field, seed) in [
            ("abstraction_seed", finalist.abstraction_seed),
            ("solver_seed", Some(finalist.solver_seed)),
            ("evaluation_seed", Some(finalist.evaluation_seed)),
        ] {
            if seed.is_some_and(|seed| seed > i64::MAX as u64) {
                bail!(
                    "finalist {} {field} exceeds TOML's signed integer range",
                    finalist.id
                );
            }
        }
        match finalist.kind {
            ModelKind::RolloutKmeans => {
                if finalist.rollout_samples.is_none_or(|value| value == 0)
                    || finalist.points_per_bucket.is_none_or(|value| value == 0)
                    || finalist.kmeans_iterations.is_none_or(|value| value == 0)
                    || finalist.abstraction_seed.is_none()
                {
                    bail!(
                        "rollout finalist {} requires positive rollout_samples, \
                         points_per_bucket, kmeans_iterations, and abstraction_seed",
                        finalist.id
                    );
                }
            }
            ModelKind::Ehs2Table => {
                if finalist.rollout_samples.is_some()
                    || finalist.points_per_bucket.is_some()
                    || finalist.kmeans_iterations.is_some()
                    || finalist.abstraction_seed.is_some()
                {
                    bail!(
                        "EHS2 finalist {} forbids rollout training fields and abstraction_seed",
                        finalist.id
                    );
                }
            }
        }
    }
    if cases != BTreeSet::from([Case::Tournament, Case::Cash]) {
        bail!("manifest must contain exactly one Tournament and one Cash finalist");
    }
    Ok(())
}

fn load_base_contract(case: Case, workspace: &Path) -> Result<BaseContract> {
    let (relative, expected_game, expected_tree) = match case {
        Case::Tournament => (
            "experiments/abstraction-2026-07-23/\
             tournament-6max-50bb-benchmark-v1.toml",
            TOURNAMENT_6MAX_50BB_GAME_FINGERPRINT,
            TOURNAMENT_TREE_CONTRACT_FINGERPRINT,
        ),
        Case::Cash => (
            "experiments/abstraction-2026-07-23/\
             cash-6max-100bb-benchmark-v1.toml",
            CASH_6MAX_100BB_GAME_FINGERPRINT,
            CASH_TREE_CONTRACT_FINGERPRINT,
        ),
    };
    let path = workspace.join(relative);
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let document: Value =
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    let source_tree = source_tree(&document)?.clone();
    let parsed = cli::config::parse_solve_config_at(&raw, &path)
        .with_context(|| format!("lowering {}", path.display()))?;
    let tree_contract_fingerprint = runtime_tree_fingerprint(&parsed)?;
    if tree_contract_fingerprint != expected_tree {
        bail!(
            "{} canonical tree contract drifted: {} != {}",
            case.as_str(),
            tree_contract_fingerprint,
            expected_tree
        );
    }
    let canonical_scenario = SCENARIOS
        .iter()
        .find(|scenario| {
            scenario.case == case
                && ((case == Case::Tournament && scenario.seats == 6 && scenario.stack_bb == 50.0)
                    || (case == Case::Cash && scenario.seats == 6 && scenario.stack_bb == 100.0))
        })
        .expect("the fixed envelope includes both canonical anchors");
    let game = game_fingerprint(parsed)?;
    if game != expected_game || game != canonical_scenario.expected_game_fingerprint {
        bail!(
            "{} canonical benchmark game fingerprint drifted: {} != {}",
            case.as_str(),
            game,
            expected_game
        );
    }
    Ok(BaseContract {
        case,
        canonical_path: path,
        canonical_source_fingerprint: hash_hex(raw.as_bytes()),
        document,
        source_tree,
        tree_contract_fingerprint,
    })
}

fn build_canonical_document(
    base: &BaseContract,
    scenario: &Scenario,
    finalist: &Finalist,
    run_spec: &RunSpec,
) -> Result<Value> {
    let mut document = base.document.clone();
    let root = table_mut(&mut document, "config root")?;
    let game = child_table_mut(root, "game", "config root")?;
    game.insert(
        "seat_count".into(),
        Value::Integer(i64::from(scenario.seats)),
    );
    game.insert(
        "button".into(),
        Value::Integer(i64::from(scenario.seats - 3)),
    );
    let defaults = child_table_mut(game, "defaults", "game")?;
    defaults.insert("stack_bb".into(), Value::Float(scenario.stack_bb));
    defaults.insert("range".into(), Value::String("random".into()));
    match scenario.case {
        Case::Tournament => {
            game.insert(
                "players".into(),
                Value::Array(
                    (0..scenario.seats)
                        .map(|seat| {
                            Value::Table(toml::Table::from_iter([
                                ("seat".into(), Value::Integer(i64::from(seat))),
                                ("ante_bb".into(), Value::Float(0.125)),
                            ]))
                        })
                        .collect(),
                ),
            );
        }
        Case::Cash => {
            game.remove("players");
        }
    }
    if game.get("tree") != Some(&base.source_tree) {
        bail!(
            "{} source tree changed while building {}",
            base.case.as_str(),
            scenario.id
        );
    }
    replace_abstraction(game, finalist)?;

    let solver = child_table_mut(root, "solver", "config root")?;
    solver.insert(
        "seed".into(),
        Value::Integer(signed_integer(finalist.solver_seed, "solver seed")?),
    );
    let run = child_table_mut(root, "run", "config root")?;
    run.insert(
        "max_sweeps".into(),
        Value::Integer(signed_integer(run_spec.max_sweeps, "max sweeps")?),
    );
    let stop = child_table_mut(run, "stop", "run")?;
    stop.insert(
        "check_every_sweeps".into(),
        Value::Integer(signed_integer(
            run_spec.check_every_sweeps,
            "check cadence",
        )?),
    );
    stop.insert(
        "evaluation_samples".into(),
        Value::Integer(signed_integer(
            run_spec.evaluation_samples,
            "evaluation samples",
        )?),
    );
    stop.insert(
        "deviator_traversals".into(),
        Value::Integer(signed_integer(
            run_spec.deviator_traversals,
            "deviator traversals",
        )?),
    );
    let resources = child_table_mut(run, "resources", "run")?;
    resources.insert(
        "threads".into(),
        Value::Integer(
            i64::try_from(run_spec.threads).context("thread count exceeds TOML integer range")?,
        ),
    );
    resources.insert("memory".into(), Value::String(run_spec.memory.clone()));
    let checkpoint = child_table_mut(run, "checkpoint", "run")?;
    checkpoint.insert(
        "interval".into(),
        Value::String(run_spec.checkpoint_interval.clone()),
    );
    Ok(document)
}

fn replace_abstraction(game: &mut toml::Table, finalist: &Finalist) -> Result<()> {
    let mut abstraction = toml::Table::new();
    abstraction.insert(
        "kind".into(),
        Value::String(
            match finalist.kind {
                ModelKind::RolloutKmeans => "multiway-rollout",
                ModelKind::Ehs2Table => "ehs2-percentile",
            }
            .into(),
        ),
    );
    if finalist.kind == ModelKind::RolloutKmeans {
        abstraction.insert(
            "rollouts_per_state".into(),
            Value::Integer(i64::from(
                finalist
                    .rollout_samples
                    .expect("validated rollout finalist has samples"),
            )),
        );
        abstraction.insert(
            "seed".into(),
            Value::Integer(signed_integer(
                finalist
                    .abstraction_seed
                    .expect("validated rollout finalist has a seed"),
                "abstraction seed",
            )?),
        );
        abstraction.insert(
            "training".into(),
            Value::Table(toml::Table::from_iter([
                (
                    "points_per_bucket".into(),
                    Value::Integer(i64::from(
                        finalist
                            .points_per_bucket
                            .expect("validated rollout finalist has points"),
                    )),
                ),
                (
                    "kmeans_iterations".into(),
                    Value::Integer(i64::from(
                        finalist
                            .kmeans_iterations
                            .expect("validated rollout finalist has iterations"),
                    )),
                ),
            ])),
        );
    }
    abstraction.insert(
        "buckets".into(),
        Value::Table(toml::Table::from_iter([
            (
                "flop".into(),
                Value::Integer(i64::from(finalist.flop_buckets)),
            ),
            (
                "turn".into(),
                Value::Integer(i64::from(finalist.turn_buckets)),
            ),
            (
                "river".into(),
                Value::Integer(i64::from(finalist.river_buckets)),
            ),
        ])),
    );
    game.insert("abstraction".into(), Value::Table(abstraction));
    let information = child_table_mut(game, "information", "game")?;
    information.insert(
        "recall".into(),
        Value::String(
            match finalist.recall {
                Recall::Full => "bucket-history",
                Recall::Street => "current-street",
            }
            .into(),
        ),
    );
    Ok(())
}

fn render_canonical(document: &Value, scenario: &Scenario, finalist: &Finalist) -> Result<String> {
    Ok(format!(
        "# Generated by generate_abstraction_transfer_configs.\n\
         # Canonical v1 source of truth; scenario={} finalist={}.\n\n{}",
        scenario.id,
        finalist.id,
        toml::to_string_pretty(document).context("serializing canonical transfer config")?
    ))
}

fn render_compatibility(
    canonical_raw: &str,
    canonical_path: &Path,
    scenario: &Scenario,
    finalist: &Finalist,
    run_spec: &RunSpec,
    cache: &Path,
) -> Result<String> {
    let mut config = cli::config::parse_solve_config_at(canonical_raw, canonical_path)
        .context("lowering canonical config for compatibility serialization")?;
    let GameSection::PreflopMultiway(game) = &mut config.game else {
        bail!("canonical transfer config did not lower to multiway")
    };
    game.abstraction.artifact_cache = Some(cache.to_owned());
    config.run.seed = Some(finalist.solver_seed);
    config.run.checkpoint_every = Some(run_spec.check_every_sweeps);
    Ok(format!(
        "# Generated by generate_abstraction_transfer_configs.\n\
         # Operational cache twin of the canonical v1 config; scenario={} finalist={}.\n\n{}",
        scenario.id,
        finalist.id,
        toml::to_string_pretty(&config).context("serializing compatibility transfer config")?
    ))
}

#[allow(clippy::too_many_arguments)]
fn verify_config(
    raw: &str,
    path: &Path,
    base: &BaseContract,
    scenario: &Scenario,
    finalist: &Finalist,
    run_spec: &RunSpec,
    expected_cache: Option<&Path>,
) -> Result<RuntimeVerification> {
    if expected_cache.is_none() {
        let source: Value = toml::from_str(raw).context("parsing generated canonical TOML")?;
        if source.get("schema").and_then(Value::as_str) != Some(cli::multiway_v1::SCHEMA) {
            bail!("{} canonical config is not schema v1", scenario.id);
        }
        if source_tree(&source)? != &base.source_tree {
            bail!(
                "{} canonical source tree differs from its benchmark base",
                scenario.id
            );
        }
    }
    let parsed = cli::config::parse_solve_config_at(raw, path)
        .with_context(|| format!("parsing generated config {}", path.display()))?;
    let tree_contract_fingerprint = runtime_tree_fingerprint(&parsed)?;
    if tree_contract_fingerprint != base.tree_contract_fingerprint {
        bail!(
            "{} lowered tree contract differs from canonical base: {} != {}",
            scenario.id,
            tree_contract_fingerprint,
            base.tree_contract_fingerprint
        );
    }

    let SolveConfig {
        game,
        rake,
        utility,
        algorithm,
        run,
    } = parsed;
    let GameSection::PreflopMultiway(game) = game else {
        bail!("{} did not lower to a multiway game", scenario.id)
    };
    game.validate()
        .with_context(|| format!("validating lowered game for {}", scenario.id))?;
    verify_table_shape(&game, scenario)?;
    verify_abstraction(&game.abstraction, finalist, expected_cache)?;
    verify_economics(&utility, &rake, scenario)?;
    verify_algorithm(&algorithm, finalist.solver_seed)?;
    verify_run(
        &run,
        run_spec,
        finalist.solver_seed,
        expected_cache.is_some(),
    )?;

    let abstraction_spec_fingerprint = abstraction_spec_fingerprint(&game.abstraction)?;
    let utility_runtime = convert_utility(utility)?;
    let rake_runtime = convert_rake(rake);
    let holdem = HoldemGame::new(
        &game,
        &utility_runtime,
        &rake_runtime,
        FeatureHashAbstraction::default(),
    )
    .with_context(|| format!("building {} for game fingerprint", scenario.id))?;
    Ok(RuntimeVerification {
        game_fingerprint: formats::config_hash_hex(&holdem.game_fingerprint()),
        tree_contract_fingerprint,
        abstraction_spec_fingerprint,
    })
}

fn verify_table_shape(game: &multiway::MultiwayConfig, scenario: &Scenario) -> Result<()> {
    let seats = usize::from(scenario.seats);
    if game.seats.len() != seats {
        bail!(
            "{} has {} seats, expected {seats}",
            scenario.id,
            game.seats.len()
        );
    }
    if game.button.index() != seats - 3 {
        bail!(
            "{} button does not preserve UTG=seat-0 ordering",
            scenario.id
        );
    }
    if game.blinds.small_bb != 0.5 || game.blinds.big_bb != 1.0 {
        bail!("{} does not use 0.5/1.0 standard blinds", scenario.id);
    }
    if !matches!(game.ante, AnteConfig::None) {
        bail!(
            "{} must lower explicit antes through forced bets",
            scenario.id
        );
    }
    for (seat, config) in game.seats.iter().enumerate() {
        if config.stack_bb != scenario.stack_bb
            || !config.range.is_empty()
            || config.betting.is_some()
        {
            bail!(
                "{} seat {seat} stack/range/betting is inconsistent",
                scenario.id
            );
        }
    }
    let forced = game
        .forced_bets
        .as_ref()
        .ok_or_else(|| anyhow!("{} is missing normalized forced bets", scenario.id))?;
    let mut expected_blinds = vec![0.0; seats];
    expected_blinds[seats - 2] = 0.5;
    expected_blinds[seats - 1] = 1.0;
    let ante = if scenario.case == Case::Tournament {
        0.125
    } else {
        0.0
    };
    if forced.blinds_bb != expected_blinds
        || forced.antes_bb != vec![ante; seats]
        || forced.common_ante_bb != 0.0
        || forced.nominal_big_blind_bb != 1.0
        || forced.first_to_act.index() != 0
    {
        bail!(
            "{} blind/ante/first-actor contract is inconsistent",
            scenario.id
        );
    }
    Ok(())
}

fn verify_abstraction(
    abstraction: &multiway::AbstractionConfig,
    finalist: &Finalist,
    expected_cache: Option<&Path>,
) -> Result<()> {
    if abstraction.flop_buckets != finalist.flop_buckets as u16
        || abstraction.turn_buckets != finalist.turn_buckets as u16
        || abstraction.river_buckets != finalist.river_buckets as u16
        || abstraction.recall
            != match finalist.recall {
                Recall::Full => RecallMode::Full,
                Recall::Street => RecallMode::Street,
            }
        || abstraction.artifact_cache.as_deref() != expected_cache
        || !abstraction.active_opponent_buckets.is_empty()
    {
        bail!(
            "finalist {} abstraction specification did not round-trip",
            finalist.id
        );
    }
    match finalist.kind {
        ModelKind::RolloutKmeans => {
            if abstraction.kind != AbstractionKind::RolloutKmeans
                || abstraction.rollout_samples != finalist.rollout_samples.unwrap()
                || abstraction.points_per_bucket != finalist.points_per_bucket.unwrap()
                || abstraction.kmeans_iterations != finalist.kmeans_iterations.unwrap()
                || abstraction.seed != finalist.abstraction_seed.unwrap()
            {
                bail!(
                    "rollout finalist {} training specification changed",
                    finalist.id
                );
            }
        }
        ModelKind::Ehs2Table => {
            if abstraction.kind != AbstractionKind::Ehs2Table
                || abstraction.seed != 0
                || abstraction.rollout_samples != 512
            {
                bail!("EHS2 finalist {} lowering changed", finalist.id);
            }
        }
    }
    Ok(())
}

fn verify_economics(
    utility: &UtilitySection,
    rake: &RakeSection,
    scenario: &Scenario,
) -> Result<()> {
    match scenario.case {
        Case::Tournament => {
            let UtilitySection::TournamentIcm {
                outside_field,
                payouts,
                samples,
                seed,
            } = utility
            else {
                bail!("{} must use tournament ICM utility", scenario.id)
            };
            let mut expected = vec![50.0, 30.0, 20.0];
            expected.resize(usize::from(scenario.seats), 0.0);
            if !outside_field.is_empty()
                || payouts != &expected
                || *samples != 100_000
                || *seed != 0
                || !matches!(rake, RakeSection::None)
            {
                bail!("{} ICM/no-rake contract is inconsistent", scenario.id);
            }
        }
        Case::Cash => {
            if !matches!(utility, UtilitySection::ChipEv) {
                bail!("{} must use chip-EV utility", scenario.id);
            }
            let RakeSection::Generic {
                rate,
                cap,
                when,
                allocation,
                rounding,
            } = rake
            else {
                bail!("{} must use generic cash rake", scenario.id)
            };
            if *rate != 0.05
                || *cap != Some(4.0)
                || when != "flop_dealt"
                || *allocation != RakeAllocation::MainFirst
                || *rounding != RakeRounding::Down
            {
                bail!("{} cash rake contract is inconsistent", scenario.id);
            }
        }
    }
    Ok(())
}

fn verify_algorithm(algorithm: &AlgorithmSection, solver_seed: u64) -> Result<()> {
    let AlgorithmSection::ExternalSamplingMccfr {
        seed,
        exploration_epsilon,
        discount_every,
        discount_until,
        traverser_vector,
        prune,
        ..
    } = algorithm
    else {
        bail!("transfer config must use external-sampling MCCFR")
    };
    if *seed != solver_seed
        || *exploration_epsilon != 0.0
        || *discount_every != 10_000
        || *discount_until != 10_000_000
        || !*traverser_vector
        || *prune
    {
        bail!("transfer solver algorithm contract is inconsistent");
    }
    Ok(())
}

fn verify_run(
    run: &cli::config::RunSection,
    expected: &RunSpec,
    solver_seed: u64,
    compatibility: bool,
) -> Result<()> {
    if run.sweeps != Some(expected.max_sweeps)
        || run.check_every != expected.check_every_sweeps
        || run.threads != Some(expected.threads)
        || run.evaluation_samples != Some(expected.evaluation_samples)
        || run.evaluation_cadence != Some(expected.check_every_sweeps)
        || run.stop_br_traversals != Some(expected.deviator_traversals)
        || run.storage != StorageKind::F32
        || run
            .max_memory_bytes
            .is_none_or(|bytes| bytes == 0 || bytes > MAX_PROCESS_MEMORY_BYTES)
    {
        bail!("transfer run controls did not lower as requested");
    }
    if compatibility {
        if run.seed != Some(solver_seed)
            || run.checkpoint_every != Some(expected.check_every_sweeps)
        {
            bail!("compatibility run seed/checkpoint cadence is inconsistent");
        }
    } else if run.seed.is_some() || run.checkpoint_every.is_some() {
        bail!("canonical v1 run unexpectedly materialized compatibility-only fields");
    }
    Ok(())
}

fn runtime_tree_fingerprint(config: &SolveConfig) -> Result<String> {
    let GameSection::PreflopMultiway(game) = &config.game else {
        bail!("tree fingerprint requires a multiway config")
    };
    fingerprint_serialized(
        b"solvers.abstraction-transfer.tree-contract/v1",
        &game.betting,
    )
}

fn abstraction_spec_fingerprint(abstraction: &multiway::AbstractionConfig) -> Result<String> {
    let mut abstraction = abstraction.clone();
    abstraction.artifact_cache = None;
    fingerprint_serialized(
        b"solvers.abstraction-transfer.abstraction-spec/v1",
        &abstraction,
    )
}

fn fingerprint_serialized<T: Serialize>(domain: &[u8], value: &T) -> Result<String> {
    let payload = serde_json::to_vec(value).context("serializing fingerprint payload")?;
    let mut material = Vec::with_capacity(domain.len() + 1 + payload.len());
    material.extend_from_slice(domain);
    material.push(0);
    material.extend_from_slice(&payload);
    Ok(hash_hex(&material))
}

fn game_fingerprint(config: SolveConfig) -> Result<String> {
    let SolveConfig {
        game,
        rake,
        utility,
        ..
    } = config;
    let GameSection::PreflopMultiway(game) = game else {
        bail!("game fingerprint requires a multiway config")
    };
    let utility = convert_utility(utility)?;
    let rake = convert_rake(rake);
    let holdem = HoldemGame::new(&game, &utility, &rake, FeatureHashAbstraction::default())
        .context("building game for fingerprint")?;
    Ok(formats::config_hash_hex(&holdem.game_fingerprint()))
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
        UtilitySection::Icm { .. } => bail!("legacy heads-up ICM is not valid here"),
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

fn enforce_scenario_fingerprint(scenario: &Scenario, actual: &str) -> Result<()> {
    if actual != scenario.expected_game_fingerprint {
        bail!(
            "{} game fingerprint drifted: {} != {}",
            scenario.id,
            actual,
            scenario.expected_game_fingerprint
        );
    }
    Ok(())
}

fn cache_path(cache_dir: &Path, finalist: &Finalist) -> Result<PathBuf> {
    let filename = match finalist.kind {
        ModelKind::RolloutKmeans => format!(
            "rollout-kmeans-f{}-t{}-r{}-rs{}-p{}-i{}-a{}.mwab",
            finalist.flop_buckets,
            finalist.turn_buckets,
            finalist.river_buckets,
            finalist.rollout_samples.unwrap(),
            finalist.points_per_bucket.unwrap(),
            finalist.kmeans_iterations.unwrap(),
            finalist.abstraction_seed.unwrap()
        ),
        ModelKind::Ehs2Table => format!(
            "ehs2-table-f{}-t{}-r{}.postcard",
            finalist.flop_buckets, finalist.turn_buckets, finalist.river_buckets
        ),
    };
    Ok(cache_dir.join(filename))
}

fn source_tree(document: &Value) -> Result<&Value> {
    document
        .get("game")
        .and_then(Value::as_table)
        .and_then(|game| game.get("tree"))
        .ok_or_else(|| anyhow!("canonical v1 config is missing [game.tree]"))
}

fn table_mut<'a>(value: &'a mut Value, label: &str) -> Result<&'a mut toml::Table> {
    value
        .as_table_mut()
        .ok_or_else(|| anyhow!("{label} is not a TOML table"))
}

fn child_table_mut<'a>(
    parent: &'a mut toml::Table,
    key: &str,
    label: &str,
) -> Result<&'a mut toml::Table> {
    parent
        .get_mut(key)
        .and_then(Value::as_table_mut)
        .ok_or_else(|| anyhow!("{label} is missing [{key}]"))
}

fn signed_integer(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{label} exceeds TOML's signed integer range"))
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

fn hash_hex(bytes: &[u8]) -> String {
    formats::config_hash_hex(&formats::config_hash(bytes))
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("resolving source workspace")
}

fn absolute_existing_file(path: &Path) -> Result<PathBuf> {
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

    fn example_manifest_path(workspace: &Path) -> PathBuf {
        workspace.join("experiments/abstraction-transfer-2026-07-25/manifest.example.toml")
    }

    #[test]
    fn tracked_example_generates_the_exact_fixed_envelope() {
        let workspace = workspace_root().unwrap();
        let manifest_path = example_manifest_path(&workspace);
        let temporary = tempfile::tempdir().unwrap();
        let output = temporary.path().join("out");
        let cache = temporary.path().join("cache");
        fs::create_dir_all(&output).unwrap();
        fs::create_dir_all(&cache).unwrap();

        generate(&manifest_path, &output, &cache, &workspace).unwrap();

        let index = fs::read_to_string(output.join("transfer-configs.csv")).unwrap();
        let rows = index.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 1 + SCENARIOS.len());
        assert_eq!(
            rows.iter()
                .skip(1)
                .map(|row| row.split(',').nth(1).unwrap())
                .collect::<Vec<_>>(),
            SCENARIOS
                .iter()
                .map(|scenario| scenario.id)
                .collect::<Vec<_>>()
        );
        let mut games = BTreeSet::new();
        let mut tree_by_case = BTreeMap::<&str, String>::new();
        let mut abstraction_by_case = BTreeMap::<&str, String>::new();
        for row in rows.iter().skip(1) {
            let fields = row.split(',').collect::<Vec<_>>();
            assert_eq!(fields.len(), 16);
            let canonical_raw = fs::read_to_string(fields[8]).unwrap();
            let compatibility_raw = fs::read_to_string(fields[10]).unwrap();
            assert!(canonical_raw.contains("schema = \"solvers.multiway-preflop/v1\""));
            assert!(!compatibility_raw.contains("schema = "));
            assert_eq!(fields[9], hash_hex(canonical_raw.as_bytes()));
            assert_eq!(fields[11], hash_hex(compatibility_raw.as_bytes()));
            assert!(Path::new(fields[8]).is_absolute());
            assert!(Path::new(fields[10]).is_absolute());
            assert!(Path::new(fields[15]).is_absolute());
            games.insert(fields[12].to_owned());
            match tree_by_case.insert(fields[0], fields[13].to_owned()) {
                Some(previous) => assert_eq!(previous, fields[13]),
                None => {}
            }
            match abstraction_by_case.insert(fields[0], fields[14].to_owned()) {
                Some(previous) => assert_eq!(previous, fields[14]),
                None => {}
            }
        }
        assert_eq!(games.len(), SCENARIOS.len());
        assert_eq!(tree_by_case.len(), 2);
        assert_eq!(abstraction_by_case.len(), 2);

        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("transfer-metadata.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["schema"], OUTPUT_SCHEMA);
        assert_eq!(
            metadata["fixedEnvelope"].as_array().unwrap().len(),
            SCENARIOS.len()
        );
        assert_eq!(
            metadata["configs"].as_array().unwrap().len(),
            SCENARIOS.len()
        );
    }

    #[test]
    fn canonical_bytes_are_output_directory_independent() {
        let workspace = workspace_root().unwrap();
        let manifest_path = example_manifest_path(&workspace);
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for root in [first.path(), second.path()] {
            fs::create_dir_all(root.join("out")).unwrap();
            fs::create_dir_all(root.join("cache")).unwrap();
            generate(
                &manifest_path,
                &root.join("out"),
                &root.join("cache"),
                &workspace,
            )
            .unwrap();
        }
        for scenario in SCENARIOS {
            let finalist = if scenario.case == Case::Tournament {
                "T-R256-example"
            } else {
                "C-R256-example"
            };
            let filename = format!("{}-{}-canonical-v1.toml", scenario.id, finalist);
            assert_eq!(
                fs::read(first.path().join("out").join(&filename)).unwrap(),
                fs::read(second.path().join("out").join(&filename)).unwrap()
            );
        }
    }

    #[test]
    fn finalist_manifest_is_fail_closed() {
        let workspace = workspace_root().unwrap();
        let raw = fs::read_to_string(example_manifest_path(&workspace)).unwrap();
        let mut missing: Manifest = toml::from_str(&raw).unwrap();
        missing.finalist.pop();
        assert!(
            validate_manifest(&missing)
                .unwrap_err()
                .to_string()
                .contains("exactly one")
        );

        let mut duplicate_case: Manifest = toml::from_str(&raw).unwrap();
        duplicate_case.finalist[1].case = Case::Tournament;
        assert!(
            validate_manifest(&duplicate_case)
                .unwrap_err()
                .to_string()
                .contains("more than one")
        );

        let mut invalid_rollout: Manifest = toml::from_str(&raw).unwrap();
        invalid_rollout.finalist[0].abstraction_seed = None;
        assert!(
            validate_manifest(&invalid_rollout)
                .unwrap_err()
                .to_string()
                .contains("requires positive")
        );

        let mut invalid_ehs: Manifest = toml::from_str(&raw).unwrap();
        invalid_ehs.finalist[0].kind = ModelKind::Ehs2Table;
        assert!(
            validate_manifest(&invalid_ehs)
                .unwrap_err()
                .to_string()
                .contains("forbids")
        );
    }

    #[test]
    fn ehs2_street_finalist_generates_without_an_abstraction_seed() {
        let workspace = workspace_root().unwrap();
        let raw = fs::read_to_string(example_manifest_path(&workspace)).unwrap();
        let mut manifest: Manifest = toml::from_str(&raw).unwrap();
        let tournament = manifest
            .finalist
            .iter_mut()
            .find(|finalist| finalist.case == Case::Tournament)
            .unwrap();
        tournament.id = "T-E256-street-test".into();
        tournament.kind = ModelKind::Ehs2Table;
        tournament.rollout_samples = None;
        tournament.points_per_bucket = None;
        tournament.kmeans_iterations = None;
        tournament.abstraction_seed = None;
        tournament.recall = Recall::Street;
        validate_manifest(&manifest).unwrap();

        let temporary = tempfile::tempdir().unwrap();
        let manifest_path = temporary.path().join("manifest.toml");
        write_atomic(
            &manifest_path,
            toml::to_string_pretty(&manifest).unwrap().as_bytes(),
        )
        .unwrap();
        let output = temporary.path().join("out");
        let cache = temporary.path().join("cache");
        fs::create_dir_all(&output).unwrap();
        fs::create_dir_all(&cache).unwrap();
        generate(&manifest_path, &output, &cache, &workspace).unwrap();

        let canonical = fs::read_to_string(
            output.join("tournament-6max-5bb-T-E256-street-test-canonical-v1.toml"),
        )
        .unwrap();
        assert!(canonical.contains("kind = \"ehs2-percentile\""));
        assert!(canonical.contains("recall = \"current-street\""));
        assert!(!canonical.contains("abstraction_seed"));
        let metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("transfer-metadata.json")).unwrap())
                .unwrap();
        let finalist = metadata["finalists"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finalist| finalist["case"] == "tournament")
            .unwrap();
        assert!(finalist["abstraction_seed"].is_null());
    }

    #[test]
    fn verification_rejects_tree_and_rake_drift() {
        let workspace = workspace_root().unwrap();
        let raw = fs::read_to_string(example_manifest_path(&workspace)).unwrap();
        let manifest: Manifest = toml::from_str(&raw).unwrap();
        validate_manifest(&manifest).unwrap();
        let base = load_base_contract(Case::Cash, &workspace).unwrap();
        let scenario = SCENARIOS
            .iter()
            .find(|scenario| scenario.id == "cash-6max-100bb")
            .unwrap();
        let finalist = manifest
            .finalist
            .iter()
            .find(|finalist| finalist.case == Case::Cash)
            .unwrap();
        let mut document =
            build_canonical_document(&base, scenario, finalist, &manifest.run).unwrap();

        let tree = document
            .get_mut("game")
            .and_then(Value::as_table_mut)
            .and_then(|game| game.get_mut("tree"))
            .and_then(Value::as_table_mut)
            .unwrap();
        tree.insert("allow_limp".into(), Value::Boolean(true));
        let drifted = render_canonical(&document, scenario, finalist).unwrap();
        assert!(
            verify_config(
                &drifted,
                Path::new("tree-drift.toml"),
                &base,
                scenario,
                finalist,
                &manifest.run,
                None,
            )
            .unwrap_err()
            .to_string()
            .contains("source tree differs")
        );

        let mut document =
            build_canonical_document(&base, scenario, finalist, &manifest.run).unwrap();
        let rake = document
            .get_mut("economics")
            .and_then(Value::as_table_mut)
            .and_then(|economics| economics.get_mut("rake"))
            .and_then(Value::as_table_mut)
            .unwrap();
        rake.insert("rate".into(), Value::Float(0.06));
        let drifted = render_canonical(&document, scenario, finalist).unwrap();
        assert!(
            verify_config(
                &drifted,
                Path::new("rake-drift.toml"),
                &base,
                scenario,
                finalist,
                &manifest.run,
                None,
            )
            .unwrap_err()
            .to_string()
            .contains("cash rake contract")
        );
    }

    #[test]
    fn verification_rejects_stack_blind_and_icm_drift() {
        let workspace = workspace_root().unwrap();
        let raw = fs::read_to_string(example_manifest_path(&workspace)).unwrap();
        let manifest: Manifest = toml::from_str(&raw).unwrap();
        let cash_base = load_base_contract(Case::Cash, &workspace).unwrap();
        let cash_scenario = SCENARIOS
            .iter()
            .find(|scenario| scenario.id == "cash-6max-100bb")
            .unwrap();
        let cash_finalist = manifest
            .finalist
            .iter()
            .find(|finalist| finalist.case == Case::Cash)
            .unwrap();

        let mut stack_drift =
            build_canonical_document(&cash_base, cash_scenario, cash_finalist, &manifest.run)
                .unwrap();
        stack_drift["game"]["defaults"]["stack_bb"] = Value::Float(101.0);
        let raw = render_canonical(&stack_drift, cash_scenario, cash_finalist).unwrap();
        assert!(
            verify_config(
                &raw,
                Path::new("stack-drift.toml"),
                &cash_base,
                cash_scenario,
                cash_finalist,
                &manifest.run,
                None,
            )
            .unwrap_err()
            .to_string()
            .contains("stack/range/betting")
        );

        let mut blind_drift =
            build_canonical_document(&cash_base, cash_scenario, cash_finalist, &manifest.run)
                .unwrap();
        blind_drift
            .get_mut("game")
            .and_then(Value::as_table_mut)
            .unwrap()
            .insert("standard_blinds".into(), Value::Boolean(false));
        let raw = render_canonical(&blind_drift, cash_scenario, cash_finalist).unwrap();
        assert!(
            verify_config(
                &raw,
                Path::new("blind-drift.toml"),
                &cash_base,
                cash_scenario,
                cash_finalist,
                &manifest.run,
                None,
            )
            .unwrap_err()
            .to_string()
            .contains("blind/ante/first-actor")
        );

        let tournament_base = load_base_contract(Case::Tournament, &workspace).unwrap();
        let tournament_scenario = SCENARIOS
            .iter()
            .find(|scenario| scenario.id == "tournament-6max-50bb")
            .unwrap();
        let tournament_finalist = manifest
            .finalist
            .iter()
            .find(|finalist| finalist.case == Case::Tournament)
            .unwrap();
        let mut icm_drift = build_canonical_document(
            &tournament_base,
            tournament_scenario,
            tournament_finalist,
            &manifest.run,
        )
        .unwrap();
        icm_drift["economics"]["payouts"] = Value::Array(vec![
            Value::Float(60.0),
            Value::Float(25.0),
            Value::Float(15.0),
        ]);
        let raw = render_canonical(&icm_drift, tournament_scenario, tournament_finalist).unwrap();
        assert!(
            verify_config(
                &raw,
                Path::new("icm-drift.toml"),
                &tournament_base,
                tournament_scenario,
                tournament_finalist,
                &manifest.run,
                None,
            )
            .unwrap_err()
            .to_string()
            .contains("ICM/no-rake contract")
        );
    }
}
