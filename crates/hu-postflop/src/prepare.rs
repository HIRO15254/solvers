//! P1 preparation, resource preflight and compatibility identity.
use crate::input::{self, NlhPayoff, P1Sections, Settings};
use anyhow::{Context, Result, bail};
use nlh::Street;
use std::path::Path;
/// Parsed Spot IR, lowered P1 game, normalized settings and resource measurements.
pub struct Prepared {
    pub document: spot::Document,
    pub settings: Settings,
    pub config: crate::PostflopConfig,
    pub payoff: NlhPayoff,
    pub effective: String,
    pub estimate: crate::MemoryEstimate,
    pub limit: u64,
    pub target: Option<f64>,
}

fn tree_error(error: crate::TreeBuildError) -> spot::SpotError {
    spot::SpotError::new(spot::Code::NLH003, "tree", error.to_string())
}

/// Parse and lower a P1 input, measuring its tree and resolving the memory/stop limits.
pub fn prepare(raw: &str, path: &Path) -> Result<Prepared> {
    let document = spot::Document::parse(raw, path)?;
    if document.spot.product != spot::Product::HuPostflop {
        bail!("NLH005: this command requires a P1 (HU Postflop) spot");
    }
    let settings = Settings::parse(&document.spot, &document.solver, &document.output)?;
    let effective = document.normalize(&P1Sections)?;
    let config = input::lower(&document.spot, &settings)?;
    let payoff = NlhPayoff::new(&document.spot)?;
    let target = settings
        .solver
        .stop
        .target
        .as_deref()
        .map(|target| input::resolve_target(&document.spot, target))
        .transpose()?;
    let mut estimate = crate::try_memory_usage(&config).map_err(tree_error)?;
    let threads = document
        .spot
        .run
        .threads
        .unwrap_or(std::thread::available_parallelism()?.get() as u64);
    estimate.compression_bytes = compression_workspace(&estimate, threads, effective.len());
    let physical = if document.spot.run.memory_bytes.is_none() {
        input::physical_memory_bytes().context("querying physical RAM")?
    } else {
        0
    };
    let limit = input::resolve_memory_limit(document.spot.run.memory_bytes, physical);
    Ok(Prepared {
        document,
        settings,
        config,
        payoff,
        effective,
        estimate,
        limit,
        target,
    })
}

fn compression_workspace(estimate: &crate::MemoryEstimate, threads: u64, config_len: usize) -> u64 {
    // zstd uses a 1 MiB window and jobs of at least 2 MiB. Tiny payloads
    // cannot occupy every requested worker. The streamed strategies are
    // still in the payload, though no longer in save_bytes. Slot overhead
    // conservatively covers both lists' per-block postcard headers. The
    // parallel strategy batch is already fully budgeted in save_bytes,
    // independent of threads; do not add per-worker strategy buffers here.
    let solution = estimate.save_bytes.saturating_add(estimate.f32_bytes / 4);
    let payload = estimate.f32_bytes.max(solution);
    let jobs = threads.min(payload.div_ceil(2 * 1024 * 1024).max(1));
    jobs.saturating_mul(16 * 1024 * 1024)
        .saturating_add(8 * 1024 * 1024)
        .saturating_add(config_len as u64 * 3)
}

/// Return warnings from the prepared tree and stop settings.
pub fn warnings(p: &Prepared) -> Vec<String> {
    warnings_for_hits(p, &p.estimate.rule_hits)
}

/// Return warnings using rule hits aggregated across one or more boards.
pub fn warnings_for_hits(p: &Prepared, hits: &crate::RuleHits) -> Vec<String> {
    let mut warnings = Vec::new();
    if p.target.is_none() {
        warnings.push("no stop target: runs until max_iterations or max_time".into());
    }
    let tree = &p.document.spot.tree;
    let mut ignored = Vec::new();
    if tree.preflop_reraise_jam_above_stack.is_some() {
        ignored.push("tree.preflop_reraise_jam_above_stack");
    }
    if tree.max_aggressive_actions.preflop != spot::MaxAggressiveActions::default().preflop {
        ignored.push("tree.max_aggressive_actions.preflop");
    }
    if tree
        .compiled
        .rules
        .iter()
        .any(|rule| rule.street == Street::Preflop)
    {
        ignored.push("Preflop tree rules");
    }
    if !ignored.is_empty() {
        warnings.push(format!(
            "{}: have no effect in P1 (HU Postflop)",
            ignored.join(", ")
        ));
    }
    for street in [Street::Flop, Street::Turn, Street::River] {
        for (index, &hit) in hits[street].iter().enumerate() {
            if !hit {
                warnings.push(format!("unmatched {street:?} tree rule {}", index + 1));
            }
        }
    }
    warnings
}

/// Required storage and save workspace bytes for the selected P1 backend.
pub fn required_bytes(p: &Prepared) -> u64 {
    match p.settings.solver.storage {
        input::Storage::F32 => p.estimate.f32_bytes,
        input::Storage::I16 => p.estimate.i16_bytes,
    }
    .saturating_add(p.estimate.save_bytes)
    .saturating_add(p.estimate.compression_bytes)
}

pub(crate) fn threads(p: &Prepared) -> Result<Option<usize>> {
    Ok(Some(
        p.document
            .spot
            .run
            .threads
            .map(usize::try_from)
            .transpose()?
            .unwrap_or(std::thread::available_parallelism()?.get()),
    ))
}

/// Reporting unit of the prepared cash or tournament utility.
pub fn utility_unit(p: &Prepared) -> &'static str {
    // The normalized economics kind is authoritative and avoids duplicating the economics enum.
    if p.effective.parse::<toml::Value>().expect("normalized TOML")["economics"]["kind"].as_str()
        == Some("cash")
    {
        "BB"
    } else {
        "prizes"
    }
}

/// Exact shortest decimal on the milli-BB grid for query labels and monetary displays.
pub fn bb(amount: u64) -> String {
    let fraction = amount % 1000;
    if fraction == 0 {
        (amount / 1000).to_string()
    } else {
        format!("{}.{fraction:03}", amount / 1000)
            .trim_end_matches('0')
            .into()
    }
}

pub(crate) fn display_game(game: &mut crate::PostflopGame) {
    for info in &mut game.node_info {
        info.history = convert_history(&info.history, false);
        for label in &mut info.actions {
            for prefix in ["bet ", "raise to "] {
                if let Some(amount) = label.strip_prefix(prefix) {
                    *label = format!("{prefix}{}", bb(amount.parse().expect("builder amount")));
                    break;
                }
            }
        }
    }
}

pub(crate) fn convert_history(history: &str, to_internal: bool) -> String {
    let mut chars = history.chars().peekable();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        result.push(c);
        if c == '[' {
            for c in chars.by_ref() {
                result.push(c);
                if c == ']' {
                    break;
                }
            }
        } else if c == 'r' {
            let mut amount = String::new();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                amount.push(chars.next().unwrap());
            }
            if to_internal {
                let amount =
                    nlh::MwChips::try_from_bb(amount.parse().expect("display history amount"))
                        .expect("BB history grid");
                result.push_str(&amount.0.to_string());
            } else {
                result.push_str(&bb(amount.parse().expect("builder history amount")));
            }
        }
    }
    result
}

/// Compatibility identity excluding the operational run settings and descriptive metadata.
pub fn compatibility_hash(effective: &str) -> Result<[u8; 32]> {
    let mut document: toml_edit::DocumentMut = effective.parse()?;
    document.remove("run");
    document.remove("meta");
    Ok(runfiles::config_hash(document.to_string().as_bytes()))
}

pub(crate) fn merge_rule_hits(acc: &mut crate::RuleHits, hits: &crate::RuleHits) {
    for street in [Street::Flop, Street::Turn, Street::River] {
        let acc_street = &mut acc[street];
        let hit_street = &hits[street];
        assert_eq!(
            acc_street.len(),
            hit_street.len(),
            "merged RuleHits must come from the same tree script"
        );
        for (a, &b) in acc_street.iter_mut().zip(hit_street) {
            *a |= b;
        }
    }
}

pub(crate) fn action_frequencies(
    avg_strategy: &[f32],
    weight: &[f32],
    num_actions: usize,
    num_hands: usize,
) -> Vec<f64> {
    let total: f64 = weight.iter().map(|&w| w as f64).sum();
    if total <= 0.0 {
        return vec![0.0; num_actions];
    }
    (0..num_actions)
        .map(|a| {
            let row = &avg_strategy[a * num_hands..(a + 1) * num_hands];
            let sum: f64 = weight
                .iter()
                .zip(row)
                .map(|(&w, &s)| w as f64 * s as f64)
                .sum();
            sum / total
        })
        .collect()
}

/// Reject removed artifact inputs before rebuilding or restoring a game.
pub(crate) fn require_artifact_config(raw: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(raw)?;
    let schema = value
        .get("schema")
        .and_then(toml::Value::as_str)
        .unwrap_or("missing schema");
    if schema != "solvers.nlh/v1" {
        bail!(
            "removed config family {schema}: re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"
        );
    }
    Ok(())
}

/// Metadata-verified reader and cumulative solve time for a continuation.
/// Full payload integrity is verified while restoring into the final arenas.
pub struct ResumeState {
    pub state: crate::checkpoint::CheckpointReader,
    pub elapsed: Option<std::time::Duration>,
}

/// Verify historical/embedded inputs and retain the open checkpoint reader.
pub fn restore(raw: &str, checkpoint_path: &Path) -> Result<ResumeState> {
    require_artifact_config(raw)?;
    let historical = prepare(raw, Path::new("run.toml"))?;
    let checkpoint = crate::checkpoint::CheckpointReader::open(checkpoint_path)?;
    if let Some(embedded) = &checkpoint.config_toml {
        require_artifact_config(embedded)?;
    }
    let expected = compatibility_hash(&historical.effective)?;
    if checkpoint.config_hash != expected {
        bail!("checkpoint config hash does not match run.toml; refusing to resume");
    }
    let embedded = checkpoint
        .config_toml
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("checkpoint has no effective config"))?;
    let embedded = prepare(embedded, Path::new("run.toml"))?;
    if compatibility_hash(&embedded.effective)? != expected {
        bail!("checkpoint embedded config hash does not match run.toml");
    }
    let elapsed = checkpoint
        .elapsed_secs
        .map(std::time::Duration::try_from_secs_f64)
        .transpose()
        .context("invalid checkpoint elapsed time")?;
    Ok(ResumeState {
        state: checkpoint,
        elapsed,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn compression_budget_counts_streamed_strategies_as_payload() {
        const MIB: u64 = 1024 * 1024;
        // Asymmetric supports can make the retained values dominate storage.
        // Values alone fit one job, but values plus strategies require two.
        let estimate = crate::MemoryEstimate {
            f32_bytes: MIB,
            save_bytes: 2 * MIB - 1,
            ..Default::default()
        };
        assert_eq!(
            super::compression_workspace(&estimate, 8, 100),
            40 * MIB + 300
        );
        assert_eq!(
            super::compression_workspace(&estimate, 1, 100),
            24 * MIB + 300
        );
    }
}
