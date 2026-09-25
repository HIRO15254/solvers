use std::collections::BTreeSet;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use multiway::{
    AverageSamplingDiagnosticsConfig, AverageSamplingDiagnosticsResult,
    AverageSamplingResearchConfig, AverageSamplingResearchResult, AverageSamplingResearchVariant,
    EndpointDeviationConfig, HistoryKey, MultiwaySolver,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum VariantArg {
    UniformOne,
    EnumerateFirstOpponent,
    PostflopContinuation,
}

impl From<VariantArg> for AverageSamplingResearchVariant {
    fn from(value: VariantArg) -> Self {
        match value {
            VariantArg::UniformOne => Self::UniformOne,
            VariantArg::EnumerateFirstOpponent => Self::EnumerateFirstOpponent,
            VariantArg::PostflopContinuation => Self::PostflopContinuation,
        }
    }
}

/// One-shot research runner. It cannot resume or write solver artifacts.
#[derive(Parser)]
#[command(name = "mw_average_sampling_research")]
struct Args {
    /// Production v1 config used to construct a fresh solver.
    #[arg(long)]
    config: PathBuf,

    /// Average-only opponent sampling proposal.
    #[arg(long, value_enum)]
    variant: VariantArg,

    /// Complete sweeps to run from a fresh solver.
    #[arg(long, default_value_t = 4_096)]
    sweeps: u64,

    /// Override worker and held-out evaluation threads.
    #[arg(long)]
    threads: Option<usize>,

    /// Override the policy-arena budget, for example `48GiB`.
    #[arg(long)]
    memory: Option<String>,

    /// Override the machine-local EHS2 cache directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Independent held-out seeds.
    #[arg(long, value_delimiter = ',', default_value = "101,202")]
    evaluation_seeds: Vec<u64>,

    /// Held-out worlds per seed.
    #[arg(long, default_value_t = 2_048)]
    evaluation_samples: u64,

    /// Omit held-out evaluation entirely.
    #[arg(long)]
    skip_evaluation: bool,

    /// Public node: `root`, a 32-digit history key, or an action-label path.
    #[arg(long, default_value = "root")]
    node: Vec<String>,

    /// Decision prefixes for a separate baseline-only coverage pass (repeatable, max 64).
    #[arg(long, conflicts_with = "skip_evaluation")]
    coverage_prefix: Vec<String>,

    /// Baseline worlds per evaluation seed; defaults to --evaluation-samples.
    #[arg(long, requires = "coverage_prefix", value_parser = clap::value_parser!(u64).range(2..))]
    coverage_samples: Option<u64>,

    /// Full raw regret / normalized average support at up to 64 decision nodes.
    #[arg(long)]
    support_node: Vec<String>,

    /// Preflop or postflop one-step deviation endpoints, including root (at most eight).
    #[arg(long, requires_all = ["endpoint_fit_samples", "endpoint_fit_seed", "endpoint_samples", "endpoint_seeds"])]
    endpoint_prefix: Vec<String>,

    #[arg(long, requires = "endpoint_prefix", value_parser = clap::value_parser!(u64).range(2..))]
    endpoint_fit_samples: Option<u64>,
    #[arg(long, requires = "endpoint_prefix")]
    endpoint_fit_seed: Option<u64>,
    #[arg(long, requires = "endpoint_prefix", value_parser = clap::value_parser!(u64).range(2..))]
    endpoint_samples: Option<u64>,
    #[arg(long, requires = "endpoint_prefix", value_delimiter = ',')]
    endpoint_seeds: Option<Vec<u64>>,
    #[arg(long, requires = "endpoint_prefix")]
    endpoint_min_fit_ess: Option<f64>,

    /// Separate root-world reach pass on the same endpoint paths.
    #[arg(long, requires_all = ["endpoint_prefix", "root_seeds"], value_parser = clap::value_parser!(u64).range(2..))]
    root_samples: Option<u64>,
    #[arg(long, requires_all = ["endpoint_prefix", "root_samples"], value_delimiter = ',')]
    root_seeds: Option<Vec<u64>>,

    /// Exact source revision or immutable source-package identifier.
    #[arg(long)]
    source_revision: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    schema_version: &'static str,
    source_revision: String,
    executable_blake3: String,
    effective_config_blake3: String,
    config: String,
    configuration_fingerprint: String,
    abstraction_fingerprint: String,
    solver_state_version: u16,
    threads: usize,
    elapsed_secs: f64,
    construction_elapsed_secs: f64,
    nodes: Vec<NodeContext>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    coverage_prefixes: Vec<NodeContext>,
    result: AverageSamplingResearchResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<AverageSamplingDiagnosticsResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    support_nodes: Vec<NodeContext>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    endpoint_prefixes: Vec<NodeContext>,
    interpretation: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeContext {
    requested: String,
    history: String,
    actor: u8,
    street: multiway::Street,
    active_opponents: u8,
}

fn node_context<G: multiway::ExternalSamplingGame>(
    solver: &MultiwaySolver<G>,
    requested: &str,
    history: HistoryKey,
) -> Result<NodeContext> {
    let node = solver
        .public_node_view(history)
        .context("expected a public decision node")?;
    Ok(NodeContext {
        requested: requested.to_owned(),
        history: history.0.iter().map(|byte| format!("{byte:02x}")).collect(),
        actor: node.actor,
        street: node.street,
        active_opponents: node.active_opponents,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.support_node.len() > 64 || args.endpoint_prefix.len() > 8 {
        bail!("at most 64 support nodes and eight endpoint prefixes are supported");
    }
    if args.coverage_prefix.len() > 64 {
        bail!("at most 64 --coverage-prefix values are supported");
    }
    if args.source_revision.trim().is_empty() {
        bail!("--source-revision must not be empty");
    }
    if args.sweeps == 0 {
        bail!("--sweeps must be at least one");
    }
    if args.threads == Some(0) {
        bail!("--threads must be at least one");
    }
    if !args.skip_evaluation {
        if args.evaluation_samples < 2 {
            bail!("--evaluation-samples must be at least two");
        }
        if args.evaluation_seeds.is_empty() {
            bail!("--evaluation-seeds must not be empty unless --skip-evaluation is used");
        }
        if args
            .evaluation_seeds
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != args.evaluation_seeds.len()
        {
            bail!("--evaluation-seeds must not contain duplicates");
        }
    }
    if let Some(cache_dir) = args.cache_dir.as_deref() {
        cli::cache::set_root_override(cache_dir);
    }
    let raw_config = fs::read_to_string(&args.config)
        .with_context(|| format!("reading {}", args.config.display()))?;
    let effective_config = cli::multiway_v1::apply_solve_overrides(
        &raw_config,
        args.threads,
        args.memory.as_deref(),
        None,
        Some(&args.config),
    )?;
    let construction_started = Instant::now();
    let session = cli::session::build_production_multiway_session(&effective_config, None)
        .context("constructing a fresh production Holdem solver")?;
    let construction_elapsed_secs = construction_started.elapsed().as_secs_f64();
    let coverage_prefixes = resolve_prefixes(&session.solver, &args.coverage_prefix)?;
    let histories = args
        .node
        .iter()
        .map(|node| resolve_history(&session.solver, node))
        .collect::<Result<Vec<_>>>()?;
    let nodes = args
        .node
        .iter()
        .zip(&histories)
        .map(|(path, &history)| node_context(&session.solver, path, history))
        .collect::<Result<Vec<_>>>()?;
    let coverage_contexts = args
        .coverage_prefix
        .iter()
        .zip(&coverage_prefixes)
        .map(|(path, &history)| node_context(&session.solver, path, history))
        .collect::<Result<Vec<_>>>()?;
    let support_histories = resolve_prefixes(&session.solver, &args.support_node)?;
    let endpoint_histories = resolve_prefixes(&session.solver, &args.endpoint_prefix)?;
    let support_nodes = args
        .support_node
        .iter()
        .zip(&support_histories)
        .map(|(path, &history)| node_context(&session.solver, path, history))
        .collect::<Result<Vec<_>>>()?;
    let endpoint_contexts = args
        .endpoint_prefix
        .iter()
        .zip(&endpoint_histories)
        .map(|(path, &history)| node_context(&session.solver, path, history))
        .collect::<Result<Vec<_>>>()?;
    let diagnostics_config = if support_histories.is_empty() && endpoint_histories.is_empty() {
        None
    } else {
        Some(AverageSamplingDiagnosticsConfig {
            support_paths: support_histories
                .iter()
                .map(|&h| path_for_history(&session.solver, h))
                .collect::<Result<_>>()?,
            endpoint_paths: endpoint_histories
                .iter()
                .map(|&h| path_for_history(&session.solver, h))
                .collect::<Result<_>>()?,
            endpoint: args
                .endpoint_fit_samples
                .map(|fit_samples| EndpointDeviationConfig {
                    fit_samples,
                    fit_seed: args.endpoint_fit_seed.expect("clap requires fit seed"),
                    held_out_samples: args
                        .endpoint_samples
                        .expect("clap requires held-out samples"),
                    held_out_seeds: args
                        .endpoint_seeds
                        .clone()
                        .expect("clap requires held-out seeds"),
                    min_fit_ess: args.endpoint_min_fit_ess.unwrap_or(64.0),
                }),
            root_samples: args.root_samples.unwrap_or(0),
            root_seeds: args.root_seeds.clone().unwrap_or_default(),
        })
    };
    let configuration_fingerprint = hex(session.solver.configuration_fingerprint());
    let abstraction_fingerprint = hex(session.solver.abstraction_fingerprint());
    let threads = session.threads;
    let executable_blake3 = hash_file(&std::env::current_exe()?)?;
    let effective_config_blake3 = blake3::hash(effective_config.as_bytes())
        .to_hex()
        .to_string();

    let started = Instant::now();
    let (evaluation_samples, evaluation_seeds) = if args.skip_evaluation {
        (0, Vec::new())
    } else {
        (args.evaluation_samples, args.evaluation_seeds)
    };
    let training_config = AverageSamplingResearchConfig {
        variant: args.variant.into(),
        sweeps: args.sweeps,
        threads,
        histories,
        evaluation_samples,
        evaluation_seeds,
        coverage_samples: if coverage_prefixes.is_empty() {
            0
        } else {
            args.coverage_samples.unwrap_or(args.evaluation_samples)
        },
        coverage_prefixes,
    };
    let (result, diagnostics) = if let Some(config) = diagnostics_config {
        let output = session
            .solver
            .run_average_sampling_research_with_diagnostics(training_config, config)?;
        (output.result, Some(output.diagnostics))
    } else {
        (
            session
                .solver
                .run_average_sampling_research(training_config)?,
            None,
        )
    };
    let output = Output {
        schema_version: "solvers.multiway-average-sampling-research/v1",
        source_revision: args.source_revision,
        executable_blake3,
        effective_config_blake3,
        config: args.config.display().to_string(),
        configuration_fingerprint,
        abstraction_fingerprint,
        solver_state_version: multiway::solver::SOLVER_STATE_VERSION,
        threads,
        elapsed_secs: started.elapsed().as_secs_f64(),
        construction_elapsed_secs,
        nodes,
        coverage_prefixes: coverage_contexts,
        result,
        diagnostics,
        support_nodes,
        endpoint_prefixes: endpoint_contexts,
        interpretation: "one-shot research output; positive-mass rows contain normalized average strategies and zero-average-mass-omitted rows have null actions; no checkpoint or solution artifact was created; ordinary result.evaluations gains use the fixed regret-greedy candidate and are not exploitability; prefix coverage uses separate baseline-only worlds, counts decisions from the prefix onward, and has no deviation gain; prefix trajectory counts overlap and positive average mass does not certify convergence; optional endpoint diagnostics fit one own-information action table before signed held-out evaluation with all prefix weight retained; root reach is separate and proposals from different profiles are not paired by matching seeds",
    };
    serde_json::to_writer_pretty(std::io::stdout().lock(), &output)?;
    println!();
    Ok(())
}

fn resolve_history<G: multiway::ExternalSamplingGame>(
    solver: &MultiwaySolver<G>,
    requested: &str,
) -> Result<HistoryKey> {
    if requested == "root" {
        return Ok(HistoryKey::ROOT);
    }
    if requested.len() == 32 && requested.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let history = parse_history_hex(requested)?;
        if solver.public_node_view(history).is_none() {
            bail!("{requested}: history is not a public decision node");
        }
        return Ok(history);
    }
    let mut history = HistoryKey::ROOT;
    for selector in requested.split('/') {
        let node = solver
            .public_node_view(history)
            .with_context(|| format!("{requested}: path left the public decision tree"))?;
        let action = selector
            .parse::<usize>()
            .ok()
            .filter(|&index| index < node.actions.len())
            .or_else(|| {
                node.actions
                    .iter()
                    .position(|action| action.label == selector)
            })
            .with_context(|| format!("{requested}: no action {selector:?} at the current node"))?;
        history = history.child(node.actor as usize, action);
        if solver.public_node_view(history).is_none() {
            bail!("{requested}: action {selector:?} does not lead to a decision node");
        }
    }
    Ok(history)
}

fn resolve_prefixes<G: multiway::ExternalSamplingGame>(
    solver: &MultiwaySolver<G>,
    requested: &[String],
) -> Result<Vec<HistoryKey>> {
    let mut histories = Vec::with_capacity(requested.len());
    for path in requested {
        let history = resolve_history(solver, path)?;
        if histories.contains(&history) {
            bail!("coverage prefixes resolve to the same history: {path:?}");
        }
        histories.push(history);
    }
    Ok(histories)
}

fn path_for_history<G: multiway::ExternalSamplingGame>(
    solver: &MultiwaySolver<G>,
    mut history: HistoryKey,
) -> Result<Vec<usize>> {
    let mut path = Vec::new();
    while history != HistoryKey::ROOT {
        if path.len() >= solver.config().max_traversal_depth as usize {
            bail!("research path exceeds maximum traversal depth");
        }
        let entry = solver
            .history_entry(history)
            .context("unknown research path ancestry")?;
        path.push(entry.action_index as usize);
        history = entry.parent;
    }
    path.reverse();
    Ok(path)
}

fn parse_history_hex(value: &str) -> Result<HistoryKey> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("history keys must contain exactly 32 hexadecimal digits");
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .with_context(|| format!("invalid history key {value:?}"))?;
    }
    Ok(HistoryKey(bytes))
}

fn hash_file(path: &Path) -> Result<String> {
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_hex_round_trips() {
        let value = "00112233445566778899aabbccddeeff";
        assert_eq!(hex_16(parse_history_hex(value).unwrap().0), value);
        assert!(parse_history_hex("not-a-history-key").is_err());
    }

    #[test]
    fn endpoint_research_arguments_require_explicit_independent_budgets() {
        let base = [
            "research",
            "--config",
            "unused",
            "--variant",
            "postflop-continuation",
            "--source-revision",
            "test",
        ];
        for extra in [
            vec!["--endpoint-prefix", "root"],
            vec!["--endpoint-samples", "32"],
            vec!["--endpoint-min-fit-ess", "64"],
            vec!["--root-seeds", "801,802"],
        ] {
            assert!(Args::try_parse_from(base.into_iter().chain(extra)).is_err());
        }
        let full = [
            "--endpoint-prefix",
            "call:1000/check",
            "--endpoint-fit-samples",
            "32",
            "--endpoint-fit-seed",
            "602",
            "--endpoint-samples",
            "64",
            "--endpoint-seeds",
            "702,703",
            "--root-samples",
            "128",
            "--root-seeds",
            "801,802",
            "--support-node",
            "root",
            "--skip-evaluation",
        ];
        let parsed = Args::try_parse_from(base.into_iter().chain(full)).unwrap();
        assert_eq!(parsed.endpoint_fit_samples, Some(32));
        assert_eq!(parsed.endpoint_seeds, Some(vec![702, 703]));
        assert_eq!(parsed.root_samples, Some(128));
        assert_eq!(parsed.support_node, vec!["root"]);
        assert!(matches!(parsed.variant, VariantArg::PostflopContinuation));
    }

    #[test]
    fn coverage_arguments_require_evaluation_and_valid_sample_budget() {
        let base = [
            "research",
            "--config",
            "unused",
            "--variant",
            "uniform-one",
            "--source-revision",
            "test",
        ];
        assert!(
            Args::try_parse_from(base.into_iter().chain(["--coverage-samples", "32"])).is_err()
        );
        assert!(
            Args::try_parse_from(base.into_iter().chain([
                "--coverage-prefix",
                "root",
                "--coverage-samples",
                "1"
            ]))
            .is_err()
        );
        assert!(
            Args::try_parse_from(base.into_iter().chain([
                "--coverage-prefix",
                "root",
                "--skip-evaluation"
            ]))
            .is_err()
        );
        let args = Args::try_parse_from(base.into_iter().chain([
            "--coverage-prefix",
            "root",
            "--coverage-samples",
            "32",
        ]))
        .unwrap();
        assert_eq!(args.coverage_prefix, vec!["root"]);
        assert_eq!(args.coverage_samples, Some(32));
    }

    fn hex_16(bytes: [u8; 16]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
