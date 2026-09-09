use std::collections::BTreeSet;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use multiway::{
    AverageSamplingResearchConfig, AverageSamplingResearchResult, AverageSamplingResearchVariant,
    HistoryKey, MultiwaySolver, PublicActionDestination,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum VariantArg {
    UniformOne,
    EnumerateFirstOpponent,
}

impl From<VariantArg> for AverageSamplingResearchVariant {
    fn from(value: VariantArg) -> Self {
        match value {
            VariantArg::UniformOne => Self::UniformOne,
            VariantArg::EnumerateFirstOpponent => Self::EnumerateFirstOpponent,
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
    result: AverageSamplingResearchResult,
    interpretation: &'static str,
}

fn main() -> Result<()> {
    let args = Args::parse();
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
    let session = cli::session::build_production_multiway_session(&effective_config, None)
        .context("constructing a fresh production Holdem solver")?;
    let histories = args
        .node
        .iter()
        .map(|node| resolve_history(&session.solver, node))
        .collect::<Result<Vec<_>>>()?;
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
    let result = session
        .solver
        .run_average_sampling_research(AverageSamplingResearchConfig {
            variant: args.variant.into(),
            sweeps: args.sweeps,
            threads,
            histories,
            evaluation_samples,
            evaluation_seeds,
        })?;
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
        result,
        interpretation: "one-shot research output; positive-mass rows contain normalized average strategies and zero-average-mass-omitted rows have null actions; no checkpoint or solution artifact was created; held-out gains use the fixed regret-greedy candidate and are not exploitability",
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
        history = match node.actions[action].destination {
            PublicActionDestination::PreflopDecision(child) => child,
            PublicActionDestination::PostflopBoundary | PublicActionDestination::Terminal => {
                bail!("{requested}: action {selector:?} does not lead to a decision node")
            }
        };
    }
    Ok(history)
}

fn parse_history_hex(value: &str) -> Result<HistoryKey> {
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

    fn hex_16(bytes: [u8; 16]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
