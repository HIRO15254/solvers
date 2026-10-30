//! P1 preparation, resource preflight and compatibility identity.
use crate::input::{self, NlhPayoff, P1Sections, Settings};
use anyhow::{Context, Result, bail};
use nlh::Street;
use std::path::Path;
/// Parsed Spot IR, lowered P1 game, normalized settings and resource measurements.
pub struct Prepared {
    pub document: spot::Document,
    pub settings: Settings<input::Storage>,
    /// Whether the input requested automatic backend selection.
    pub storage_auto: bool,
    pub config: crate::PostflopConfig,
    pub payoff: NlhPayoff,
    pub effective: String,
    pub estimate: crate::MemoryEstimate,
    /// Whole-tree bound retained to guard an unexpected runtime fallback.
    pub fallback_estimate: Option<crate::MemoryEstimate>,
    /// Disjoint compact storage spans when preflight proves the subtree path.
    pub subtree_spans: Option<Vec<hu_engine::StorageSpan>>,
    pub limit: u64,
    pub target: Option<f64>,
}

fn tree_error(error: crate::TreeBuildError) -> spot::SpotError {
    spot::SpotError::new(spot::Code::NLH003, "tree", error.to_string())
}

fn parse_p1(raw: &str, path: &Path) -> Result<(spot::Document, Settings<input::StorageChoice>)> {
    let document = spot::Document::parse(raw, path)?;
    if document.spot.product != spot::Product::HuPostflop {
        bail!("NLH005: this command requires a P1 (HU Postflop) spot");
    }
    let settings = Settings::parse(&document.spot, &document.solver, &document.output)?;
    Ok((document, settings))
}

/// Parse and lower a P1 input without measuring its tree.
pub fn lower_config(raw: &str, path: &Path) -> Result<crate::PostflopConfig> {
    let (document, settings) = parse_p1(raw, path)?;
    let mut config = input::lower(&document.spot, &settings)?;
    if let Some(profile) = &document.spot.profile {
        crate::profile::restrict_iso(&mut config, profile)?;
    }
    Ok(config)
}

/// A saved artifact's embedded config, parsed and lowered. The run that
/// wrote the artifact already passed preflight, so [`prepare_artifact`] does
/// not measure the tree again.
pub(crate) struct ArtifactInput {
    pub spot: spot::Spot,
    pub settings: Settings<input::Storage>,
    pub config: crate::PostflopConfig,
}

/// Prepares an embedded config. `storage = "auto"` resolves to `ran_with`,
/// the backend the artifact's metadata records.
pub(crate) fn prepare_artifact(raw: &str, path: &Path, ran_with: &str) -> Result<ArtifactInput> {
    let document = spot::Document::parse_embedded(raw, path)?;
    if document.spot.product != spot::Product::HuPostflop {
        bail!("NLH005: this command requires a P1 (HU Postflop) spot");
    }
    let settings = Settings::parse(&document.spot, &document.solver, &document.output)?;
    let storage = match settings.solver.storage {
        input::StorageChoice::F32 => input::Storage::F32,
        input::StorageChoice::I16 => input::Storage::I16,
        input::StorageChoice::I16F32Avg => input::Storage::I16F32Avg,
        input::StorageChoice::Auto => match ran_with {
            "f32" => input::Storage::F32,
            "i16" => input::Storage::I16,
            "i16-f32avg" => input::Storage::I16F32Avg,
            other => bail!("artifact metadata names unknown storage {other:?}"),
        },
    };
    let mut config = input::lower(&document.spot, &settings)?;
    if let Some(profile) = &document.spot.profile {
        crate::profile::restrict_iso(&mut config, profile)?;
    }
    Ok(ArtifactInput {
        spot: document.spot,
        settings: settings.with_storage(storage),
        config,
    })
}
// A failed or incomplete proof never changes the whole-tree budget.
fn proven_subtree_spans(
    document: &spot::Document,
    config: &crate::PostflopConfig,
    target: Option<f64>,
) -> Option<Vec<hu_engine::StorageSpan>> {
    let profile = document.spot.profile.as_ref()?;
    let freeze = profile.freeze_upstream.as_ref()?;
    if !profile.frequencies.is_empty()
        || (profile.locks.is_empty() && profile.incentives.is_empty())
    {
        return None;
    }
    let target = target?;
    let reader = crate::sol::SolReader::open(Path::new(&freeze.solution)).ok()?;
    let metadata = reader.metadata();
    if metadata.mode != crate::sol::StreetsStored::Full
        || !metadata.meta.nash_conv.is_finite()
        || metadata.meta.nash_conv / 2.0 >= target
    {
        return None;
    }
    let old_document =
        spot::Document::parse_embedded(&metadata.config_toml, Path::new(&freeze.solution)).ok()?;
    if let Some(old) = &old_document.spot.profile
        && (old.freeze_upstream.is_some()
            || !old.locks.iter().all(|entry| profile.locks.contains(entry))
            || !old
                .incentives
                .iter()
                .all(|entry| profile.incentives.contains(entry))
            || !old
                .frequencies
                .iter()
                .all(|entry| profile.frequencies.contains(entry)))
    {
        return None;
    }
    crate::postflop::selector_subtree_spans(config, profile)
}

/// Parse and lower a P1 input, measuring its tree and resolving the memory/stop limits.
pub fn prepare(raw: &str, path: &Path) -> Result<Prepared> {
    let (document, settings) = parse_p1(raw, path)?;
    let effective = document.normalize(&P1Sections)?;
    let mut config = input::lower(&document.spot, &settings)?;
    if let Some(profile) = &document.spot.profile {
        crate::profile::restrict_iso(&mut config, profile)?;
    }
    let payoff = NlhPayoff::new(&document.spot)?;
    let target = settings
        .solver
        .stop
        .target
        .as_deref()
        .map(|target| input::resolve_target(&document.spot, target))
        .transpose()?;
    let mut estimate = crate::try_memory_usage(&config).map_err(tree_error)?;
    // The metadata frame holds the effective config twice (encoded and
    // compressed). Budget the longest possible concrete spelling for auto, so
    // artifacts restored with i16-f32avg never need more than preflight.
    let config_len = effective.len()
        + if settings.solver.storage == input::StorageChoice::Auto {
            "i16-f32avg".len() - "auto".len()
        } else {
            0
        };
    estimate.compression_bytes += 2 * config_len as u64;
    estimate.threads = run_threads(&document.spot.run)? as u64;
    if document.spot.profile.is_some() {
        estimate.profile_bytes = estimate.profile_slot_bytes();
        if settings.output.solution_streets == input::SolutionStreets::NoRivers {
            estimate.save_bytes = estimate
                .save_bytes
                .saturating_add(estimate.river_constraint_workspace(&config));
        }
    }
    if let Some(profile) = &document.spot.profile
        && profile.freeze_upstream.is_some()
    {
        estimate.profile_bytes = estimate
            .profile_bytes
            .saturating_add(estimate.freeze_bytes(&config, profile));
    }
    let physical = if document.spot.run.memory_bytes.is_none() {
        input::physical_memory_bytes().context("querying physical RAM")?
    } else {
        0
    };
    let limit = input::resolve_memory_limit(document.spot.run.memory_bytes, physical);
    let subtree_spans = proven_subtree_spans(&document, &config, target);
    let fallback_estimate = subtree_spans.as_ref().map(|_| estimate.clone());
    let choice = settings.solver.storage;
    let storage = resolve_storage_for_spans(choice, &estimate, subtree_spans.as_deref(), limit);
    let storage_auto = choice == input::StorageChoice::Auto;
    let settings = settings.with_storage(storage);
    // Run identity and all saved artifacts use a concrete backend. Validation
    // separately normalizes the document to preserve the user's input choice.
    let mut effective: toml_edit::DocumentMut = effective.parse()?;
    effective["solver"]["storage"] = toml_edit::value(storage.name());
    let effective = effective.to_string();
    Ok(Prepared {
        storage_auto,
        document,
        settings,
        config,
        payoff,
        effective,
        estimate,
        fallback_estimate,
        subtree_spans,
        limit,
        target,
    })
}

/// Select the fastest allowed backend. When neither fits, retain the mixed
/// requirement for resource reporting; `check_memory_limit` rejects execution.
pub fn resolve_storage(
    choice: input::StorageChoice,
    estimate: &crate::MemoryEstimate,
    limit: u64,
) -> input::Storage {
    resolve_storage_for_spans(choice, estimate, None, limit)
}

fn estimate_required(
    estimate: &crate::MemoryEstimate,
    spans: Option<&[hu_engine::StorageSpan]>,
    storage: input::Storage,
) -> u64 {
    match spans {
        Some(spans) => estimate.subtree_required_bytes(storage, spans),
        None => estimate.required_bytes(storage),
    }
}

fn resolve_storage_for_spans(
    choice: input::StorageChoice,
    estimate: &crate::MemoryEstimate,
    spans: Option<&[hu_engine::StorageSpan]>,
    limit: u64,
) -> input::Storage {
    use input::{Storage, StorageChoice};
    match choice {
        StorageChoice::Auto if estimate_required(estimate, spans, Storage::F32) <= limit => {
            Storage::F32
        }
        StorageChoice::Auto | StorageChoice::I16F32Avg => Storage::I16F32Avg,
        StorageChoice::F32 => Storage::F32,
        StorageChoice::I16 => Storage::I16,
    }
}

/// Apply the prepared backend's memory limit, preserving auto failure context.
pub fn check_memory_limit(p: &Prepared) -> Result<(), input::MemoryLimitError> {
    let required = required_bytes(p);
    if required <= p.limit {
        Ok(())
    } else {
        Err(input::MemoryLimitError {
            required,
            limit: p.limit,
            auto_f32_required: p.storage_auto.then(|| {
                estimate_required(&p.estimate, p.subtree_spans.as_deref(), input::Storage::F32)
            }),
        })
    }
}

/// Single-line human description of the resolved backend and selection reason.
pub fn storage_description(p: &Prepared) -> String {
    let name = p.settings.solver.storage.name();
    if !p.storage_auto {
        return name.into();
    }
    let f32 = estimate_required(&p.estimate, p.subtree_spans.as_deref(), input::Storage::F32);
    format!(
        "{name} (auto: f32 needs {f32} bytes {} limit {} bytes)",
        if f32 <= p.limit { "<=" } else { ">" },
        p.limit
    )
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
    if tree
        .compiled
        .rules
        .iter()
        .any(|rule| rule.street == Street::Preflop)
    {
        warnings.push("Preflop tree rules: have no effect in P1 (HU Postflop)".into());
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
    required_bytes_for(p, p.settings.solver.storage)
}

/// Preflight requirement for a candidate backend, including the proved scope.
pub fn required_bytes_for(p: &Prepared, storage: input::Storage) -> u64 {
    estimate_required(&p.estimate, p.subtree_spans.as_deref(), storage)
}

pub(crate) fn threads(p: &Prepared) -> Result<Option<usize>> {
    Ok(Some(run_threads(&p.document.spot.run)?))
}

/// Configured worker threads, or every available core.
fn run_threads(run: &spot::Run) -> Result<usize> {
    Ok(run
        .threads
        .map(usize::try_from)
        .transpose()?
        .unwrap_or(std::thread::available_parallelism()?.get()))
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

/// Canonical display spelling for a chip amount on the given table.
pub fn bb(amount: u64, scale: nlh::ChipScale) -> String {
    scale.amount_text(nlh::MwChips(amount))
}

/// Translates a canonical displayed history into the builder's chip spelling.
pub(crate) fn internal_history(history: &str, scale: nlh::ChipScale) -> String {
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
            let amount = if !scale.is_decimal() && chars.peek() == Some(&'c') {
                chars.next();
                nlh::MwChips(amount.parse().expect("display chip amount"))
            } else {
                scale.parse_bb(&amount).expect("display history BB amount")
            };
            result.push_str(&amount.0.to_string());
        }
    }
    result
}

/// Compatibility identity excluding the operational run settings and descriptive metadata.
pub fn compatibility_hash(effective: &str) -> Result<[u8; 32]> {
    let mut document: toml_edit::DocumentMut = effective.parse()?;
    document.remove("run");
    document.remove("meta");
    // CFR precision changes neither the game definition nor the state format.
    if let Some(solver) = document
        .get_mut("solver")
        .and_then(toml_edit::Item::as_table_mut)
    {
        solver.remove("cfr_precision");
    }
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
        bail!(
            "checkpoint config hash does not match run.toml; refusing to resume. Checkpoints written by earlier builds cannot be resumed; re-solve from the config"
        );
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
mod budget_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    const CONFIG: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack = 21
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2s 7s Ks 4h 3d"
[ranges]
BB = "AK,KQ,AA"
BTN = "QQ,JJ"
[tree]
script = "river { replace bet [50%] } river when aggressions >= 1 { remove raise }"
[solver.stop]
max_iterations = 2
check_every = 2
[run]
memory = "1GiB"
threads = 1
"#;

    #[test]
    fn no_rivers_profile_budgets_constraint_stream_without_freeze() {
        let path = Path::new("input.toml");
        let profile = "\n[[profile.lock]]\nnode = 'x'\nstrategy = { check = 1.0 }\n";
        let full = prepare(&format!("{CONFIG}{profile}"), path).unwrap();
        let no_rivers = prepare(
            &format!("{CONFIG}\n[output]\nsolution_streets = 'no-rivers'\n{profile}"),
            path,
        )
        .unwrap();
        let plain = prepare(
            &format!("{CONFIG}\n[output]\nsolution_streets = 'no-rivers'\n"),
            path,
        )
        .unwrap();
        assert_eq!(
            no_rivers.estimate.save_bytes,
            full.estimate.save_bytes
                + no_rivers
                    .estimate
                    .river_constraint_workspace(&no_rivers.config)
        );
        assert_eq!(plain.estimate.save_bytes, full.estimate.save_bytes);
    }

    #[test]
    fn freeze_preflight_uses_proved_spans_and_tighter_target_uses_full_tree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.toml");
        let base_path = dir.path().join("base.sol");
        let base = prepare(CONFIG, &path).unwrap();
        crate::run::run(
            crate::run::RunRequest {
                prepared: &base,
                checkpoint: &dir.path().join("base.ckpt"),
                solution: &base_path,
                state: None,
                evaluation_history: vec![],
                elapsed_before: Duration::ZERO,
                cancel: &AtomicBool::new(false),
            },
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        let frozen = |target: &str| {
            format!(
                "{}\n[solver]\nstorage = 'auto'\n[profile]\nfreeze_upstream = {{ solution = '{}' }}\n[[profile.lock]]\nnode = 'x'\nstrategy = {{ check = 1.0 }}\n",
                CONFIG.replace(
                    "max_iterations = 2",
                    &format!("max_iterations = 2\ntarget = '{target}'")
                ),
                base_path.to_string_lossy().replace('\\', "/"),
            )
        };
        let mut proved = prepare(&frozen("10000bb"), &path).unwrap();
        let spans = proved.subtree_spans.as_ref().expect("subtree proof");
        let compact = proved
            .estimate
            .subtree_required_bytes(input::Storage::F32, spans);
        let (game, _, resolved) = crate::run::build_profiled(&proved).unwrap();
        let node = resolved
            .unwrap()
            .summary
            .resolved_nodes
            .into_iter()
            .find(|node| matches!(node.kind, crate::profile::ResolvedNodeKind::Lock))
            .unwrap()
            .node;
        let actual = game.tree.storage_spans[node as usize];
        assert_eq!(spans[0].end, actual.end - actual.start);
        assert_eq!(spans[0].sref_end, actual.sref_end - actual.sref_start);
        let partial = frozen("10000bb").replace("node = 'x'", "node = 'x'\nhands = 'QQ'");
        assert!(prepare(&partial, &path).unwrap().subtree_spans.is_some());
        let unknown = frozen("10000bb").replace("node = 'x'", "node = 'root'");
        assert!(prepare(&unknown, &path).unwrap().subtree_spans.is_none());
        let mut estimate = proved.estimate.clone();
        // Make the full arena dominate fixed overhead in this tiny fixture.
        estimate.f32_bytes = compact.saturating_add(1_000_000_000);
        estimate.f32_regret_bytes = 0;
        assert_eq!(required_bytes(&proved), compact);
        assert_eq!(
            resolve_storage_for_spans(input::StorageChoice::Auto, &estimate, Some(spans), compact),
            input::Storage::F32
        );
        assert_eq!(
            resolve_storage_for_spans(input::StorageChoice::Auto, &estimate, None, compact),
            input::Storage::I16F32Avg
        );
        proved.limit = compact;
        assert!(check_memory_limit(&proved).is_ok());
        proved.limit = compact - 1;
        assert_eq!(check_memory_limit(&proved).unwrap_err().required, compact);

        let tight = prepare(&frozen("0.0000001%pot"), &path).unwrap();
        assert!(tight.subtree_spans.is_none());
        assert!(tight.fallback_estimate.is_none());
        assert_eq!(
            required_bytes(&tight),
            tight.estimate.required_bytes(tight.settings.solver.storage)
        );
    }
}
