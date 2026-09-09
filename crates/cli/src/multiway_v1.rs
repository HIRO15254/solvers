//! Dedicated parser and normalizer for `solvers.multiway-preflop/v1`.
//!
//! The public v1 schema intentionally does not become another variant of
//! the historical shared CLI schema. During the engine migration this
//! module lowers the supported v1 surface to the existing runtime structs;
//! unsupported v1 combinations fail explicitly instead of being ignored.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use cards::SizeUnit;
use multiway::config::{
    AbstractionConfig, AbstractionKind, ActiveOpponentBucketConfig, AnteConfig, BettingConfig,
    BlindConfig, ForcedBetConfig, MultiwayConfig, RakeAllocation as RuntimeRakeAllocation,
    RakeRounding as RuntimeRakeRounding, RecallMode, RuleAction, RuleEffect, RuleStreet,
    SeatConfig, SizeSpec, StackRatio, TreeRule as RuntimeTreeRule,
};
use multiway::types::{MAX_SEATS, MIN_SEATS, SeatId};
use serde::{Deserialize, Serialize};

use crate::config::{
    AlgorithmSection, GameSection, OutsidePlayerSection, RakeSection, RunSection, SolveConfig,
    StorageKind, UtilitySection,
};

pub const SCHEMA: &str = "solvers.multiway-preflop/v1";
pub const ROLLOUT_REMOVED_CODE: &str = "MWP001";
pub const FULL_RECALL_REMOVED_CODE: &str = "MWP002";
/// Deterministic production policy-arena budget that `memory = "auto"`
/// resolves to. Explicit `run.resources.memory` values pass through
/// unchanged, including values above this budget; the operator must size the
/// external process-RSS boundary with headroom above whatever arena budget
/// is configured (8 GiB for the default 6 GiB arena).
pub const PRODUCTION_POLICY_ARENA_AUTO_BYTES: u64 = 6 * 1024 * 1024 * 1024;

pub fn has_v1_schema(raw: &str) -> Result<bool> {
    let value: toml::Value = toml::from_str(raw).context("parsing TOML document")?;
    let Some(schema) = value.get("schema") else {
        return Ok(false);
    };
    let schema = schema
        .as_str()
        .ok_or_else(|| anyhow!("schema must be a string"))?;
    if schema == SCHEMA {
        return Ok(true);
    }
    // Every other family belongs to `solver_config_v1`, which reports its
    // own errors. Only a string no parser claims is an error here.
    if crate::solver_config_v1::owns(schema) {
        return Ok(false);
    }
    bail!(
        "unsupported config schema {schema:?}; expected one of {SCHEMA:?}, {}",
        crate::solver_config_v1::SCHEMAS.join(", ")
    );
}

/// Enforces the release solve/resume contract without changing the
/// historical v1 decoder used by read-only artifact compatibility and
/// research builds.
///
/// `kind` must be explicit because an omitted kind historically selected
/// `multiway-rollout`; silently interpreting the same bytes as EHS² would
/// change abstraction fingerprints and strategy semantics.
pub fn validate_production_contract(raw: &str) -> Result<()> {
    validate_production_contract_with_base(raw, None)
}

/// [`validate_production_contract`] for a config read from a file, so the
/// lowering it performs can resolve a `[game.tree] source` against the
/// config's own directory. Without this a perfectly valid
/// `kind = "script", source = "t.mwtree"` config fails `solvers validate`
/// with "script tree requires a config file path", even though the path was
/// known all along -- the contract check ran on the raw text and lowered
/// without it.
pub fn validate_production_contract_at(raw: &str, config_path: &Path) -> Result<()> {
    validate_production_contract_with_base(raw, Some(base_directory(config_path)))
}

fn validate_production_contract_with_base(raw: &str, base_dir: Option<&Path>) -> Result<()> {
    let value: toml::Value = toml::from_str(raw).context("parsing TOML document")?;
    let root = value
        .as_table()
        .ok_or_else(|| anyhow!("TOML root must be a table"))?;
    if root.get("schema").and_then(toml::Value::as_str) != Some(SCHEMA) {
        bail!("production Multiway Preflop solve requires schema = {SCHEMA:?}");
    }
    let game = root
        .get("game")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| anyhow!("[game] table is required"))?;
    let abstraction = game
        .get("abstraction")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            anyhow!(
                "{ROLLOUT_REMOVED_CODE}: production requires explicit \
                 [game.abstraction] kind = \"ehs2-percentile\"; omission historically selected \
                 the removed multiway-rollout backend"
            )
        })?;
    match abstraction.get("kind").and_then(toml::Value::as_str) {
        Some("ehs2-percentile") => {}
        Some("multiway-rollout") => bail!(
            "{ROLLOUT_REMOVED_CODE}: game.abstraction.kind = \"multiway-rollout\" was removed \
             from production: concrete state-to-bucket assignments grow during Solve, violating \
             the sweep-0 preallocation contract; the measured Tournament comparator was inferior \
             and the Cash comparator was about 4x slower and failed 0.95 river coverage"
        ),
        Some(kind) => bail!(
            "{ROLLOUT_REMOVED_CODE}: production supports only \
             game.abstraction.kind = \"ehs2-percentile\", found {kind:?}"
        ),
        None => bail!(
            "{ROLLOUT_REMOVED_CODE}: production requires explicit \
             game.abstraction.kind = \"ehs2-percentile\"; omission historically selected the \
             removed multiway-rollout backend"
        ),
    }
    for retired in ["rollouts_per_state", "seed", "training", "opponent_buckets"] {
        if abstraction.contains_key(retired) {
            bail!(
                "{ROLLOUT_REMOVED_CODE}: game.abstraction.{retired} is rollout-only and was \
                 removed from the production config; EHS2 is deterministic and fully built \
                 before sweep 0"
            );
        }
    }

    if let Some(information) = game.get("information") {
        let information = information
            .as_table()
            .ok_or_else(|| anyhow!("[game.information] must be a table"))?;
        match information.get("recall").and_then(toml::Value::as_str) {
            None | Some("current-street") => {}
            Some("bucket-history") => bail!(
                "{FULL_RECALL_REMOVED_CODE}: game.information.recall = \"bucket-history\" was \
                 removed from production: its sparse policy map grows during Solve and EHS2 K64 \
                 already hit the resource limit at 3,695/10,000 Tournament sweeps and \
                 4,267/10,000 Cash sweeps"
            ),
            Some(recall) => bail!(
                "{FULL_RECALL_REMOVED_CODE}: production uses fixed current-street recall, found \
                 {recall:?}"
            ),
        }
    }

    let lowered =
        parse_and_lower_with_base(raw, base_dir).context("validating production v1 config")?;
    let GameSection::PreflopMultiway(game) = lowered.game else {
        unreachable!("v1 always lowers to Multiway Preflop")
    };
    if !matches!(game.abstraction.kind, AbstractionKind::Ehs2Table)
        || !matches!(game.abstraction.recall, RecallMode::Street)
    {
        bail!("production contract lowering did not produce EHS2/current-street semantics");
    }
    Ok(())
}

fn base_directory(config_path: &Path) -> &Path {
    config_path.parent().unwrap_or_else(|| Path::new("."))
}

fn parse_and_lower_with_base(raw: &str, base_dir: Option<&Path>) -> Result<SolveConfig> {
    validate_decimal_chip_tokens(raw)?;
    let source: V1Config = toml::from_str(raw).context("parsing Multiway Preflop v1 config")?;
    source.lower(base_dir)
}

pub fn parse_and_lower(raw: &str) -> Result<SolveConfig> {
    parse_and_lower_with_base(raw, None)
}

pub fn parse_and_lower_at(raw: &str, config_path: &Path) -> Result<SolveConfig> {
    parse_and_lower_with_base(raw, Some(base_directory(config_path)))
}

pub fn probability_encoding(raw: &str) -> Result<ProbabilityEncoding> {
    let source: V1Config = toml::from_str(raw).context("parsing Multiway Preflop v1 config")?;
    Ok(source.output.probability_encoding)
}

fn normalized_config_with_base(raw: &str, base_dir: Option<&Path>) -> Result<serde_json::Value> {
    parse_and_lower_with_base(raw, base_dir)
        .context("validating Multiway Preflop v1 config before normalization")?;
    let mut source: V1Config = toml::from_str(raw).context("parsing Multiway Preflop v1 config")?;
    match base_dir {
        Some(base_dir) => source.materialize_effective_at(base_dir)?,
        None => source.materialize_effective()?,
    }
    serde_json::to_value(&source).context("serializing normalized Multiway Preflop v1 config")
}

pub fn normalized_config(raw: &str) -> Result<serde_json::Value> {
    normalized_config_with_base(raw, None)
}

pub fn normalized_config_at(raw: &str, config_path: &Path) -> Result<serde_json::Value> {
    normalized_config_with_base(raw, Some(base_directory(config_path)))
}

fn normalized_toml_from_json(json: serde_json::Value) -> Result<String> {
    let value = json_to_toml(json)?.ok_or_else(|| anyhow!("effective config is empty"))?;
    let normalized =
        toml::to_string_pretty(&value).context("serializing effective Multiway Preflop v1 TOML")?;
    // Same literal-string form postflop writes, from the same helper: both
    // families put their script at `[game.tree] script`, and an effective
    // config that reads differently per family would defeat the point of
    // sharing one grammar.
    let normalized = crate::config::literalize_tree_script("MWP004", &normalized)?;
    parse_and_lower(&normalized).context("reparsing normalized Multiway Preflop v1 config")?;
    Ok(normalized)
}

pub fn normalized_toml(raw: &str) -> Result<String> {
    normalized_toml_from_json(normalized_config(raw)?)
}

pub fn normalized_toml_at(raw: &str, config_path: &Path) -> Result<String> {
    normalized_toml_from_json(normalized_config_at(raw, config_path)?)
}

fn json_to_toml(value: serde_json::Value) -> Result<Option<toml::Value>> {
    Ok(match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(value) => Some(toml::Value::Boolean(value)),
        serde_json::Value::String(value) => Some(toml::Value::String(value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Some(toml::Value::Integer(value))
            } else if let Some(value) = value.as_u64() {
                Some(toml::Value::Integer(i64::try_from(value).map_err(
                    |_| anyhow!("effective config integer {value} exceeds TOML's range"),
                )?))
            } else {
                let value = value
                    .as_f64()
                    .ok_or_else(|| anyhow!("invalid effective config number"))?;
                if !value.is_finite() {
                    bail!("effective config contains a non-finite number");
                }
                Some(toml::Value::Float(value))
            }
        }
        serde_json::Value::Array(values) => Some(toml::Value::Array(
            values
                .into_iter()
                .map(json_to_toml)
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect(),
        )),
        serde_json::Value::Object(values) => {
            let mut table = toml::map::Map::new();
            for (key, value) in values {
                if let Some(value) = json_to_toml(value)? {
                    table.insert(key, value);
                }
            }
            Some(toml::Value::Table(table))
        }
    })
}

pub fn checkpoint_interval_secs(raw: &str) -> Result<u64> {
    let source: V1Config = toml::from_str(raw).context("parsing Multiway Preflop v1 config")?;
    parse_duration(&source.run.checkpoint.interval)
}

pub fn max_time_secs(raw: &str) -> Result<Option<u64>> {
    let source: V1Config = toml::from_str(raw).context("parsing Multiway Preflop v1 config")?;
    source
        .run
        .max_time
        .as_deref()
        .map(parse_duration)
        .transpose()
}

pub fn apply_solve_overrides(
    raw: &str,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    config_path: Option<&Path>,
) -> Result<String> {
    // The default binary accepts only the fixed production abstraction.
    // Research builds deliberately retain the historical v1 decoder so the
    // checked-in rollout/full-recall experiments remain reproducible.

    match config_path {
        Some(config_path) => validate_production_contract_at(raw, config_path)?,
        None => validate_production_contract(raw)?,
    }
    validate_decimal_chip_tokens(raw)?;
    let mut source: V1Config = toml::from_str(raw).context("parsing Multiway Preflop v1 config")?;
    if let Some(config_path) = config_path {
        source.materialize_effective_at(base_directory(config_path))?;
    } else {
        source.materialize_effective()?;
    }
    if matches!(source.run.resources.threads, AutoOrUsize::Auto) {
        source.run.resources.threads = AutoOrUsize::Name("auto".into());
    }
    if matches!(source.run.resources.memory, AutoOrMemory::Auto) {
        source.run.resources.memory = AutoOrMemory::Name("auto".into());
    }
    if let Some(max_time) = max_time {
        source.run.max_time = Some(max_time.into());
    }
    if let Some(threads) = threads {
        source.run.resources.threads = AutoOrUsize::Count(threads);
    }
    if let Some(memory) = memory {
        source.run.resources.memory = AutoOrMemory::Name(memory.into());
    }
    let effective = toml::to_string_pretty(&source)
        .context("serializing solve-time v1 overrides and effective config")?;

    validate_production_contract(&effective).context("validating solve-time v1 overrides")?;

    parse_and_lower(&effective).context("validating research solve-time v1 overrides")?;
    Ok(effective)
}

#[allow(clippy::too_many_arguments)]
pub fn apply_resume_overrides(
    raw: &str,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    max_sweeps: Option<u64>,
    stop_target: Option<f64>,
    evaluation_samples: Option<u64>,
    evaluation_cadence: Option<u64>,
    checkpoint_interval: Option<&str>,
) -> Result<String> {
    let effective = apply_solve_overrides(raw, threads, memory, max_time, None)?;
    let mut document: toml::Value = toml::from_str(&effective)?;
    let run = document
        .get_mut("run")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("effective run section is missing"))?;
    if let Some(value) = max_sweeps {
        run.insert("max_sweeps".into(), toml::Value::Integer(value as i64));
    }
    let stop = run
        .get_mut("stop")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("effective run.stop section is missing"))?;
    if let Some(value) = stop_target {
        stop.insert("target".into(), toml::Value::Float(value));
    }
    if let Some(value) = evaluation_samples {
        stop.insert(
            "evaluation_samples".into(),
            toml::Value::Integer(value as i64),
        );
    }
    if let Some(value) = evaluation_cadence {
        stop.insert(
            "check_every_sweeps".into(),
            toml::Value::Integer(value as i64),
        );
    }
    if let Some(value) = checkpoint_interval {
        let checkpoint = run
            .get_mut("checkpoint")
            .and_then(toml::Value::as_table_mut)
            .ok_or_else(|| anyhow!("effective run.checkpoint section is missing"))?;
        checkpoint.insert("interval".into(), toml::Value::String(value.into()));
    }
    let effective = toml::to_string_pretty(&document)?;
    parse_and_lower(&effective).context("validating resume-time v1 overrides")?;
    Ok(effective)
}
fn validate_decimal_chip_tokens(raw: &str) -> Result<()> {
    use std::str::FromStr as _;
    let document = toml_edit::DocumentMut::from_str(raw)
        .context("parsing lossless TOML for decimal chip validation")?;
    for (key, item) in document.as_table().iter() {
        visit_decimal_item(key, item)?;
    }
    Ok(())
}

fn visit_decimal_item(key: &str, item: &toml_edit::Item) -> Result<()> {
    match item {
        toml_edit::Item::None => Ok(()),
        toml_edit::Item::Value(value) => visit_decimal_value(key, value),
        toml_edit::Item::Table(table) => {
            for (child_key, child) in table.iter() {
                visit_decimal_item(child_key, child)?;
            }
            Ok(())
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter() {
                for (child_key, child) in table.iter() {
                    visit_decimal_item(child_key, child)?;
                }
            }
            Ok(())
        }
    }
}

fn visit_decimal_value(key: &str, value: &toml_edit::Value) -> Result<()> {
    use toml_edit::Value;
    if key.ends_with("_bb") {
        match value {
            Value::Integer(number) => validate_millibb_repr(key, &number.display_repr()),
            Value::Float(number) => validate_millibb_repr(key, &number.display_repr()),
            Value::Array(values) => {
                for value in values.iter() {
                    match value {
                        Value::Integer(number) => {
                            validate_millibb_repr(key, &number.display_repr())?
                        }
                        Value::Float(number) => validate_millibb_repr(key, &number.display_repr())?,
                        _ => bail!("{key} entries must be decimal BB amounts"),
                    }
                }
                Ok(())
            }
            _ => bail!("{key} must be a decimal BB amount"),
        }
    } else {
        match value {
            Value::Array(values) => {
                for value in values.iter() {
                    visit_decimal_value("", value)?;
                }
            }
            Value::InlineTable(table) => {
                for (child_key, value) in table.iter() {
                    visit_decimal_value(child_key, value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn validate_millibb_repr(field: &str, representation: &str) -> Result<()> {
    let compact = representation.replace('_', "");
    let compact = compact.strip_prefix('+').unwrap_or(&compact);
    if compact.starts_with('-') {
        bail!("{field} must be non-negative");
    }
    let (mantissa, exponent) = match compact.find(['e', 'E']) {
        Some(index) => {
            let exponent = compact[index + 1..]
                .parse::<i32>()
                .with_context(|| format!("invalid decimal exponent for {field}"))?;
            (&compact[..index], exponent)
        }
        None => (compact, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() && fraction.is_empty() {
        bail!("{field} is not a decimal amount");
    }
    let digits_text = format!("{whole}{fraction}");
    if !digits_text.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("{field} must be a finite decimal amount");
    }
    let digits = digits_text
        .parse::<u128>()
        .with_context(|| format!("{field} exceeds the supported decimal width"))?;
    let power = exponent - i32::try_from(fraction.len()).unwrap_or(i32::MAX) + 3;
    let units = if power >= 0 {
        digits
            .checked_mul(
                10u128
                    .checked_pow(power as u32)
                    .ok_or_else(|| anyhow!("{field} overflows .001 BB units"))?,
            )
            .ok_or_else(|| anyhow!("{field} overflows .001 BB units"))?
    } else {
        let divisor = 10u128
            .checked_pow((-power) as u32)
            .ok_or_else(|| anyhow!("{field} is not representable in .001 BB units"))?;
        if digits % divisor != 0 {
            bail!("{field} is not exactly representable in .001 BB units");
        }
        digits / divisor
    };
    u64::try_from(units)
        .map(|_| ())
        .map_err(|_| anyhow!("{field} overflows the u64 .001 BB representation"))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct V1Config {
    schema: String,
    game: Game,
    #[serde(default)]
    economics: Economics,
    #[serde(default)]
    solver: Solver,
    #[serde(default)]
    run: Run,
    #[serde(default)]
    output: Output,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Game {
    seat_count: u8,
    button: u8,
    #[serde(default = "yes")]
    standard_blinds: bool,
    #[serde(default)]
    preflop_first_to_act: FirstActor,
    #[serde(default)]
    common_ante_bb: f64,
    #[serde(default)]
    defaults: PlayerDefaults,
    #[serde(default)]
    players: Vec<Player>,
    #[serde(default)]
    tree: Tree,
    #[serde(default)]
    abstraction: CardAbstraction,
    #[serde(default, skip_serializing_if = "Information::is_current_street")]
    information: Information,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlayerDefaults {
    stack_bb: Option<f64>,
    range: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Player {
    seat: u8,
    stack_bb: Option<f64>,
    range: Option<String>,
    blind_bb: Option<f64>,
    #[serde(default)]
    ante_bb: f64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(untagged)]
enum FirstActor {
    #[default]
    Utg,
    Seat(u8),
    Name(String),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Tree {
    Standard {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_limp: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_aggressive_actions: Option<StreetAggressionCaps>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reraise_jam_above_actor_starting_stack: Option<TreeStackRatio>,
        #[serde(default)]
        rules: Vec<TreeRule>,
    },
    Script {
        /// Path to the `.mwtree` source, relative to the config file's
        /// directory. Exclusive with `script`. Resolved into `script` (and
        /// cleared) as part of normalizing a config with a known path, so
        /// nothing downstream of that ever sees a path again -- mirrors the
        /// postflop family's `[game.tree] source`/`script` (`TreeSection` in
        /// `crate::config`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        /// The script's own body. Exclusive with `source`; this is the only
        /// form an effective config carries.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        script: Option<String>,
        #[serde(default)]
        params: BTreeMap<String, toml::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allow_limp: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_aggressive_actions: Option<StreetAggressionCaps>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reraise_jam_above_actor_starting_stack: Option<TreeStackRatio>,
    },
}

/// `source`/`script` are exclusive, and at least one is required -- mirrors
/// `crate::solver_config_v1::check_tree_shape`.
fn check_tree_script_shape(tree: &Tree) -> Result<()> {
    let Tree::Script { source, script, .. } = tree else {
        return Ok(());
    };
    if source.is_some() && script.is_some() {
        bail!("[game.tree] source and script are exclusive; write only one.");
    }
    if source.is_none() && script.is_none() {
        bail!(
            "[game.tree] kind = \"script\" requires source or script; write one of them, or \
             set kind = \"standard\" for an empty rule list."
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StreetAggressionCaps {
    preflop: u8,
    flop: u8,
    turn: u8,
    river: u8,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TreeStackRatio {
    numerator: u32,
    denominator: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TreeRule {
    #[serde(default = "default_rule_priority")]
    priority: i32,
    street: RuleStreet,
    #[serde(rename = "when")]
    condition: String,
    effect: RuleEffect,
    action: Option<RuleAction>,
    #[serde(default)]
    sizes: Vec<String>,
}

fn default_rule_priority() -> i32 {
    100
}

/// Parses a Multiway Preflop bet-size literal (bb-denominated). Delegates to
/// the shared grammar in `cards::sizing`; the error is re-wrapped as
/// `anyhow::Error` but keeps the same message text this crate has always
/// produced.
fn parse_size_literal(source: &str) -> Result<SizeSpec> {
    Ok(SizeSpec::parse(source, SizeUnit::Bb)?)
}

fn lower_tree_rules(rules: &[TreeRule]) -> Result<Vec<RuntimeTreeRule>> {
    let mut lowered = Vec::with_capacity(rules.len());
    for (source_order, rule) in rules.iter().enumerate() {
        if rule.effect == RuleEffect::Checkdown {
            if rule.action.is_some() || !rule.sizes.is_empty() {
                bail!("checkdown tree rules must omit action and sizes");
            }
        } else if rule.action.is_none() {
            bail!("non-checkdown tree rules require action");
        }
        let sizes = rule
            .sizes
            .iter()
            .map(|size| parse_size_literal(size))
            .collect::<Result<Vec<_>>>()?;
        lowered.push(RuntimeTreeRule::new(
            rule.priority,
            source_order as u32,
            rule.street,
            rule.condition.trim().to_owned(),
            rule.effect,
            rule.action,
            sizes,
        ));
    }
    lowered.sort_by_key(|rule| (rule.priority, rule.source_order));
    Ok(lowered)
}
/// Compiles a `.mwtree` script against `multiway::tree_rules::MULTIWAY` --
/// the same `cards::script` front end (tokenizing, substitution, nesting,
/// `if`/`else`, `param`/`define`) postflop's `.tree` scripts compile
/// through -- and lowers the result straight to [`RuntimeTreeRule`]s. This
/// is the entire multiway-specific frontend now: it owns nothing but the
/// `toml::Value` -> `String` param-override conversion `crate::config`'s own
/// postflop `[game.tree.params]` handling already needed, reused verbatim
/// via `crate::config::param_overrides`.
fn lower_tree_script(
    source: &str,
    params: &BTreeMap<String, toml::Value>,
) -> Result<Vec<RuntimeTreeRule>> {
    let overrides = crate::config::param_overrides(params)?;
    Ok(multiway::tree_rules::compile_script(source, &overrides)?)
}

impl Default for Tree {
    fn default() -> Self {
        Self::Standard {
            allow_limp: None,
            max_aggressive_actions: None,
            reraise_jam_above_actor_starting_stack: None,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CardAbstractionKind {
    #[default]
    MultiwayRollout,
    Ehs2Percentile,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CardAbstraction {
    #[serde(default)]
    kind: CardAbstractionKind,
    rollouts_per_state: Option<u32>,
    seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    training: Option<AbstractionTraining>,
    #[serde(default)]
    buckets: BucketCounts,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    opponent_buckets: BTreeMap<String, BucketCounts>,
}

impl Default for CardAbstraction {
    fn default() -> Self {
        Self {
            kind: CardAbstractionKind::MultiwayRollout,
            rollouts_per_state: None,
            seed: None,
            training: None,
            buckets: BucketCounts::default(),
            opponent_buckets: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AbstractionTraining {
    #[serde(default = "default_points_per_bucket")]
    points_per_bucket: u32,
    #[serde(default = "default_kmeans_iterations")]
    kmeans_iterations: u32,
}

impl Default for AbstractionTraining {
    fn default() -> Self {
        Self {
            points_per_bucket: default_points_per_bucket(),
            kmeans_iterations: default_kmeans_iterations(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BucketCounts {
    #[serde(default = "default_buckets")]
    flop: u32,
    #[serde(default = "default_buckets")]
    turn: u32,
    #[serde(default = "default_buckets")]
    river: u32,
}

impl Default for BucketCounts {
    fn default() -> Self {
        Self {
            flop: default_buckets(),
            turn: default_buckets(),
            river: default_buckets(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Recall {
    #[default]
    CurrentStreet,
    BucketHistory,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Information {
    #[serde(default)]
    recall: Recall,
}

impl Information {
    fn is_current_street(&self) -> bool {
        matches!(self.recall, Recall::CurrentStreet)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Economics {
    Cash {
        #[serde(default)]
        rake: Option<Rake>,
    },
    TournamentIcm {
        payouts: Vec<f64>,
        #[serde(default)]
        outside_field_bb: Vec<f64>,
        samples: Option<u64>,
        seed: Option<u64>,
    },
}

impl Default for Economics {
    fn default() -> Self {
        Self::Cash { rake: None }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Rake {
    rate: f64,
    cap_bb: Option<f64>,
    #[serde(default = "default_rake_when")]
    when: String,
    #[serde(default)]
    allocation: RakeAllocation,
    #[serde(default = "default_rounding_unit")]
    rounding_unit_bb: f64,
    #[serde(default)]
    rounding: Rounding,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum RakeAllocation {
    #[default]
    MainFirst,
    Proportional,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Rounding {
    #[default]
    Down,
    Nearest,
    Up,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SolverKind {
    #[default]
    RangeVector,
    SingleHand,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Solver {
    #[serde(default)]
    kind: SolverKind,
    #[serde(default)]
    seed: u64,
    #[serde(default)]
    opponent_exploration: f64,
    #[serde(default = "one_u64")]
    batch_sweeps: u64,
    #[serde(default)]
    discount: Discount,
    #[serde(default)]
    pruning: Pruning,
}

impl Default for Solver {
    fn default() -> Self {
        Self {
            kind: SolverKind::RangeVector,
            seed: 0,
            opponent_exploration: 0.0,
            batch_sweeps: 1,
            discount: Discount::default(),
            pruning: Pruning::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Discount {
    Periodic {
        #[serde(default = "default_discount_every")]
        every_sweeps: u64,
        #[serde(default = "default_discount_until")]
        until_sweeps: u64,
    },
    None,
}

impl Default for Discount {
    fn default() -> Self {
        Self::Periodic {
            every_sweeps: default_discount_every(),
            until_sweeps: default_discount_until(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Pruning {
    #[default]
    RegretBased,
    None,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(untagged)]
enum AutoOrUsize {
    #[default]
    Auto,
    Count(usize),
    Name(String),
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(untagged)]
enum AutoOrMemory {
    #[default]
    Auto,
    Bytes(u64),
    Name(String),
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Resources {
    #[serde(default)]
    threads: AutoOrUsize,
    #[serde(default)]
    memory: AutoOrMemory,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    #[serde(default = "default_target")]
    target: Target,
    #[serde(default = "default_check_every")]
    check_every_sweeps: u64,
    #[serde(default = "default_confirmations")]
    confirmations: u32,
    #[serde(default = "default_evaluation_samples")]
    evaluation_samples: u64,
    #[serde(default = "default_deviator_traversals")]
    deviator_traversals: u64,
}

impl Default for Stop {
    fn default() -> Self {
        Self {
            target: default_target(),
            check_every_sweeps: default_check_every(),
            confirmations: default_confirmations(),
            evaluation_samples: default_evaluation_samples(),
            deviator_traversals: default_deviator_traversals(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum Target {
    Name(String),
    Value(f64),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    #[serde(default = "default_checkpoint_interval")]
    interval: String,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            interval: default_checkpoint_interval(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Run {
    #[serde(default = "default_max_sweeps")]
    max_sweeps: u64,
    max_time: Option<String>,
    #[serde(default)]
    resources: Resources,
    #[serde(default)]
    stop: Stop,
    #[serde(default)]
    checkpoint: Checkpoint,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            max_sweeps: default_max_sweeps(),
            max_time: None,
            resources: Resources::default(),
            stop: Stop::default(),
            checkpoint: Checkpoint::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbabilityEncoding {
    #[default]
    U16,
    F32,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Output {
    #[serde(default)]
    probability_encoding: ProbabilityEncoding,
}

impl V1Config {
    fn materialize_effective(&mut self) -> Result<()> {
        // Size literals are written in whichever accepted spelling the
        // author preferred; an effective config states the canonical one,
        // so the same tree always reads the same way. Parsing here also
        // means a bad literal fails during normalization rather than
        // surviving into a stored effective config.
        if let Tree::Standard { rules, .. } = &mut self.game.tree {
            for rule in rules {
                for size in &mut rule.sizes {
                    *size = parse_size_literal(size)?.render(SizeUnit::Bb);
                }
            }
        }
        let count = usize::from(self.game.seat_count);
        let default_stack = self.game.defaults.stack_bb.ok_or_else(|| {
            anyhow!("game.defaults.stack_bb is required unless every seat overrides it")
        })?;
        let default_range = self
            .game
            .defaults
            .range
            .clone()
            .unwrap_or_else(|| "random".into());
        let sb = if count == 2 {
            usize::from(self.game.button)
        } else {
            (usize::from(self.game.button) + 1) % count
        };
        let bb = (sb + 1) % count;
        let first_actor = match &self.game.preflop_first_to_act {
            FirstActor::Utg => (bb + 1) % count,
            FirstActor::Name(name) if name == "utg" => (bb + 1) % count,
            FirstActor::Seat(seat) => usize::from(*seat),
            FirstActor::Name(name) => bail!("invalid preflop_first_to_act {name:?}"),
        };
        let mut sparse = std::mem::take(&mut self.game.players)
            .into_iter()
            .map(|player| (player.seat, player))
            .collect::<BTreeMap<_, _>>();
        self.game.players = (0..count)
            .map(|seat| {
                let override_player = sparse.remove(&(seat as u8));
                let default_blind = if self.game.standard_blinds && seat == sb {
                    0.5
                } else if self.game.standard_blinds && seat == bb {
                    1.0
                } else {
                    0.0
                };
                Player {
                    seat: seat as u8,
                    stack_bb: Some(
                        override_player
                            .as_ref()
                            .and_then(|player| player.stack_bb)
                            .unwrap_or(default_stack),
                    ),
                    range: Some(
                        override_player
                            .as_ref()
                            .and_then(|player| player.range.clone())
                            .unwrap_or_else(|| default_range.clone()),
                    ),
                    blind_bb: Some(
                        override_player
                            .as_ref()
                            .and_then(|player| player.blind_bb)
                            .unwrap_or(default_blind),
                    ),
                    ante_bb: override_player.map_or(0.0, |player| player.ante_bb),
                }
            })
            .collect();
        self.game.defaults.stack_bb = Some(default_stack);
        self.game.defaults.range = Some(default_range);
        self.game.preflop_first_to_act = FirstActor::Seat(first_actor as u8);
        match self.game.abstraction.kind {
            CardAbstractionKind::MultiwayRollout => {
                self.game.abstraction.rollouts_per_state.get_or_insert(512);
                self.game.abstraction.seed.get_or_insert(0);
                self.game
                    .abstraction
                    .training
                    .get_or_insert_with(AbstractionTraining::default);
            }
            CardAbstractionKind::Ehs2Percentile
                if self.game.abstraction.training == Some(AbstractionTraining::default()) =>
            {
                // Explicit defaults are accepted for schema symmetry, but
                // EHS² has no training phase, so its canonical effective
                // config omits this no-op table just like a legacy config.
                self.game.abstraction.training = None;
            }
            CardAbstractionKind::Ehs2Percentile => {}
        }
        Ok(())
    }

    /// Resolves a `Tree::Script`'s `source` (a path, relative to `base_dir`)
    /// into `script` and clears `source`, so nothing downstream of this call
    /// ever sees a path again. Effective configs embedded in checkpoints and
    /// solutions therefore remain reparsable after the original `.mwtree`
    /// file moves. A no-op for `Tree::Standard`, and for a `Tree::Script`
    /// that already carries `script` directly -- mirrors the postflop
    /// family's `TreeSection::resolve_source_at` (`crate::config`).
    ///
    /// Unlike the old flat scanner this replaces, the script body is kept
    /// intact rather than expanded into `Tree::Standard { rules }`: nesting
    /// and `if`/`else` compose several conditions per rule, and expanding
    /// would throw away `params`, the variable schema a GUI edits.
    fn materialize_effective_at(&mut self, base_dir: &Path) -> Result<()> {
        self.materialize_effective()?;
        check_tree_script_shape(&self.game.tree)?;
        let Tree::Script { source, script, .. } = &mut self.game.tree else {
            return Ok(());
        };
        let Some(path) = source.take() else {
            return Ok(());
        };
        let full_path = base_dir.join(&path);
        let text = std::fs::read_to_string(&full_path)
            .with_context(|| format!("reading mwtree source {}", full_path.display()))?;
        *script = Some(text);
        Ok(())
    }

    fn lower(self, base_dir: Option<&Path>) -> Result<SolveConfig> {
        if self.schema != SCHEMA {
            bail!("unsupported config schema {:?}", self.schema);
        }
        let count = usize::from(self.game.seat_count);
        if !(MIN_SEATS..=MAX_SEATS).contains(&count) {
            bail!("game.seat_count must be from {MIN_SEATS} through {MAX_SEATS}");
        }
        if usize::from(self.game.button) >= count {
            bail!("game.button is outside the configured table");
        }
        check_grid("game.common_ante_bb", self.game.common_ante_bb, true)?;
        check_tree_script_shape(&self.game.tree)?;

        let (
            tree_rules,
            allow_limp,
            max_aggressive_actions,
            reraise_jam_above_actor_starting_stack,
        ) = match &self.game.tree {
            Tree::Standard {
                rules,
                allow_limp,
                max_aggressive_actions,
                reraise_jam_above_actor_starting_stack,
            } => (
                lower_tree_rules(rules)?,
                *allow_limp,
                *max_aggressive_actions,
                *reraise_jam_above_actor_starting_stack,
            ),
            Tree::Script {
                source,
                script,
                params,
                allow_limp,
                max_aggressive_actions,
                reraise_jam_above_actor_starting_stack,
            } => {
                // `check_tree_script_shape` already rejected both-or-neither,
                // so exactly one of `source`/`script` is `Some` here.
                let program = match (source, script) {
                    (Some(source), None) => {
                        let base = base_dir.ok_or_else(|| {
                            anyhow!(
                                "script tree requires a config file path for relative source \
                                 resolution"
                            )
                        })?;
                        let path = base.join(source);
                        std::fs::read_to_string(&path)
                            .with_context(|| format!("reading mwtree source {}", path.display()))?
                    }
                    (None, Some(script)) => script.clone(),
                    _ => unreachable!("checked by check_tree_script_shape"),
                };
                (
                    lower_tree_script(&program, params)?,
                    *allow_limp,
                    *max_aggressive_actions,
                    *reraise_jam_above_actor_starting_stack,
                )
            }
        };
        if let Some(ratio) = reraise_jam_above_actor_starting_stack
            && (ratio.numerator == 0
                || ratio.denominator == 0
                || ratio.numerator > ratio.denominator)
        {
            bail!("game.tree.reraise_jam_above_actor_starting_stack must be a ratio in (0, 1]");
        }

        let sb = if count == 2 {
            usize::from(self.game.button)
        } else {
            (usize::from(self.game.button) + 1) % count
        };
        let bb = (sb + 1) % count;
        let first_actor = match &self.game.preflop_first_to_act {
            FirstActor::Utg => (bb + 1) % count,
            FirstActor::Name(name) if name == "utg" => (bb + 1) % count,
            FirstActor::Name(name) => bail!("invalid preflop_first_to_act {name:?}"),
            FirstActor::Seat(seat) if usize::from(*seat) < count => usize::from(*seat),
            FirstActor::Seat(seat) => {
                bail!("preflop_first_to_act seat {seat} is outside the configured table")
            }
        };
        let mut forced_blinds = vec![0.0; count];
        if self.game.standard_blinds {
            forced_blinds[sb] = 0.5;
            forced_blinds[bb] = 1.0;
        }
        let mut forced_antes = vec![0.0; count];

        let mut by_seat: Vec<Option<Player>> = (0..count).map(|_| None).collect();
        let mut seen = HashSet::new();
        for player in self.game.players {
            let seat = usize::from(player.seat);
            if seat >= count {
                bail!("game.players seat {seat} is outside the configured table");
            }
            if !seen.insert(seat) {
                bail!("game.players contains duplicate seat {seat}");
            }
            check_grid(
                &format!("game.players[{seat}].ante_bb"),
                player.ante_bb,
                true,
            )?;
            forced_antes[seat] = player.ante_bb;
            if let Some(blind) = player.blind_bb {
                check_grid(&format!("game.players[{seat}].blind_bb"), blind, true)?;
                forced_blinds[seat] = blind;
            }
            by_seat[seat] = Some(player);
        }

        let default_stack = self.game.defaults.stack_bb;
        let default_range = self.game.defaults.range;
        let mut seats = Vec::with_capacity(count);
        for (seat, override_) in by_seat.into_iter().enumerate() {
            let stack = override_
                .as_ref()
                .and_then(|p| p.stack_bb)
                .or(default_stack)
                .ok_or_else(|| anyhow!("seat {seat} has no stack_bb and no game.defaults value"))?;
            check_grid(&format!("seat {seat} stack_bb"), stack, false)?;
            let range = override_
                .and_then(|p| p.range)
                .or_else(|| default_range.clone())
                .ok_or_else(|| anyhow!("seat {seat} has no range and no game.defaults value"))?;
            let range = if range == "random" {
                String::new()
            } else {
                range
            };
            seats.push(SeatConfig {
                name: None,
                stack_bb: stack,
                range,
                betting: None,
            });
        }

        let abstraction = lower_abstraction(self.game.abstraction, self.game.information, count)?;
        let (utility, rake) = lower_economics(self.economics, &seats)?;
        let (discount_every, discount_until) = match self.solver.discount {
            Discount::Periodic {
                every_sweeps,
                until_sweeps,
            } => {
                if every_sweeps == 0 {
                    bail!("solver.discount.every_sweeps must be positive");
                }
                (every_sweeps, until_sweeps)
            }
            Discount::None => (u64::MAX, 0),
        };
        if !(0.0..=1.0).contains(&self.solver.opponent_exploration)
            || !self.solver.opponent_exploration.is_finite()
        {
            bail!("solver.opponent_exploration must be finite and within [0, 1]");
        }
        if self.solver.batch_sweeps == 0 {
            bail!("solver.batch_sweeps must be positive");
        }
        let vector = matches!(self.solver.kind, SolverKind::RangeVector);
        let prune = matches!(self.solver.pruning, Pruning::RegretBased);
        if !vector && prune {
            bail!("regret-based pruning is forbidden with solver.kind = \"single-hand\"");
        }
        if prune && matches!(abstraction.recall, RecallMode::Full) {
            bail!(
                "regret-based pruning requires game.information.recall = \"current-street\"; \
                 set solver.pruning.kind = \"none\" for \"bucket-history\""
            );
        }
        if self.run.max_sweeps == 0 {
            bail!("run.max_sweeps must be positive");
        }
        if self.run.max_time.is_some() {
            parse_duration(self.run.max_time.as_deref().expect("checked above"))?;
        }
        if self.run.stop.check_every_sweeps == 0
            || self.run.stop.confirmations == 0
            || self.run.stop.evaluation_samples == 0
            || self.run.stop.deviator_traversals == 0
        {
            bail!(
                "run.stop cadence, confirmations, evaluation_samples, and deviator_traversals must be positive"
            );
        }
        let threads = Some(
            parse_threads(self.run.resources.threads)?.unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(usize::from)
                    .unwrap_or(1)
                    .min(
                        count
                            .saturating_mul(self.solver.batch_sweeps as usize)
                            .max(1),
                    )
            }),
        );
        let memory = Some(parse_memory(self.run.resources.memory)?.unwrap_or(u64::MAX));
        // The compatibility decoder preserves `auto` as the historical
        // sentinel. Production validation/session construction resolves it
        // to the 6 GiB auto budget without changing embedded historical
        // artifact configs decoded for read-only access.
        let target_scale = match &utility {
            UtilitySection::ChipEv => 1.0,
            UtilitySection::TournamentIcm { payouts, .. } => payouts.iter().sum::<f64>(),
            UtilitySection::Icm { .. } => unreachable!(),
        };
        let target = match self.run.stop.target {
            Target::Name(name) if name == "default" => match utility {
                UtilitySection::ChipEv => 0.05,
                UtilitySection::TournamentIcm { .. } => 0.0001 * target_scale,
                UtilitySection::Icm { .. } => unreachable!(),
            },
            Target::Name(name) => {
                let value = name
                    .parse::<f64>()
                    .with_context(|| format!("invalid run.stop.target {name:?}"))?;
                if !value.is_finite() || value <= 0.0 {
                    bail!("run.stop.target must be finite and positive");
                }
                value * target_scale
            }
            Target::Value(value) if value.is_finite() && value > 0.0 => value * target_scale,
            Target::Value(_) => bail!("run.stop.target must be finite and positive"),
        };
        parse_duration(&self.run.checkpoint.interval)?;
        let _encoding = self.output.probability_encoding;

        let nominal_big_blind_bb = forced_blinds.iter().copied().fold(0.0, f64::max);
        let mut betting = BettingConfig {
            rules: tree_rules,
            ..BettingConfig::default()
        };
        if let Some(allow_limp) = allow_limp {
            betting.allow_limp = allow_limp;
        }
        if let Some(caps) = max_aggressive_actions {
            betting.preflop.max_aggressive_actions = caps.preflop;
            betting.flop.max_aggressive_actions = caps.flop;
            betting.turn.max_aggressive_actions = caps.turn;
            betting.river.max_aggressive_actions = caps.river;
        }
        if let Some(ratio) = reraise_jam_above_actor_starting_stack {
            betting.preflop.reraise_jam_above_actor_starting_stack = Some(StackRatio {
                numerator: ratio.numerator,
                denominator: ratio.denominator,
            });
        }
        Ok(SolveConfig {
            schema: Some(SCHEMA.to_string()),
            game: GameSection::PreflopMultiway(MultiwayConfig {
                seats,
                button: SeatId::new_unchecked(self.game.button),
                blinds: BlindConfig::default(),
                ante: AnteConfig::None,
                betting,
                forced_bets: Some(ForcedBetConfig {
                    blinds_bb: forced_blinds,
                    antes_bb: forced_antes,
                    common_ante_bb: self.game.common_ante_bb,
                    nominal_big_blind_bb,
                    first_to_act: SeatId::new_unchecked(first_actor as u8),
                }),
                abstraction,
            }),
            rake,
            utility,
            algorithm: AlgorithmSection::ExternalSamplingMccfr {
                seed: self.solver.seed,
                exploration_epsilon: self.solver.opponent_exploration,
                discount_every,
                discount_until,
                traverser_vector: vector,
                prune,
                prune_threshold: None,
                prune_skip_probability: multiway::solver::DEFAULT_PRUNE_SKIP_PROBABILITY,
            },
            run: RunSection {
                max_time_secs: None,
                iterations: 0,
                sweeps: Some(self.run.max_sweeps),
                seed: None,
                check_every: self.run.stop.check_every_sweeps,
                storage: StorageKind::F32,
                target_nash_conv: None,
                threads,
                par_chance_depth: None,
                par_min_children: None,
                max_memory_bytes: memory,
                checkpoint_every: None,
                evaluation_samples: Some(self.run.stop.evaluation_samples),
                evaluation_cadence: Some(self.run.stop.check_every_sweeps),
                sweep_batch: Some(self.solver.batch_sweeps),
                stop_dev_gain: Some(target),
                stop_confirmations: Some(self.run.stop.confirmations),
                stop_eval_period_secs: Some(1.0),
                stop_br_traversals: Some(self.run.stop.deviator_traversals),
            },
        })
    }
}

fn lower_abstraction(
    source: CardAbstraction,
    information: Information,
    seats: usize,
) -> Result<AbstractionConfig> {
    let training = source.training.unwrap_or_default();
    if training.points_per_bucket == 0 {
        bail!("game.abstraction.training.points_per_bucket must be positive");
    }
    if training.kmeans_iterations == 0 {
        bail!("game.abstraction.training.kmeans_iterations must be positive");
    }
    let buckets = source.buckets;
    let cast = |field: &str, value: u32| -> Result<u16> {
        if value == 0 {
            bail!("{field} must be positive");
        }
        u16::try_from(value).map_err(|_| anyhow!("{field} exceeds the current runtime width"))
    };
    let mut overrides = Vec::new();
    for (key, value) in source.opponent_buckets {
        let opponents: u8 = key
            .parse()
            .with_context(|| format!("invalid opponent_buckets key {key:?}"))?;
        if opponents == 0 || usize::from(opponents) >= seats {
            bail!("opponent_buckets key {opponents} is unreachable at this table");
        }
        overrides.push(ActiveOpponentBucketConfig {
            active_opponents: opponents,
            flop_buckets: cast("opponent flop buckets", value.flop)?,
            turn_buckets: cast("opponent turn buckets", value.turn)?,
            river_buckets: cast("opponent river buckets", value.river)?,
        });
    }
    overrides.sort_by_key(|v| v.active_opponents);
    match source.kind {
        CardAbstractionKind::MultiwayRollout => Ok(AbstractionConfig {
            flop_buckets: cast("flop buckets", buckets.flop)?,
            turn_buckets: cast("turn buckets", buckets.turn)?,
            river_buckets: cast("river buckets", buckets.river)?,
            rollout_samples: source.rollouts_per_state.unwrap_or(512),
            points_per_bucket: training.points_per_bucket,
            kmeans_iterations: training.kmeans_iterations,
            seed: source.seed.unwrap_or(0),
            active_opponent_buckets: overrides,
            artifact_cache: None,
            recall: match information.recall {
                Recall::CurrentStreet => RecallMode::Street,
                Recall::BucketHistory => RecallMode::Full,
            },
            kind: AbstractionKind::RolloutKmeans,
        }),
        CardAbstractionKind::Ehs2Percentile => {
            if training != AbstractionTraining::default() {
                bail!("ehs2-percentile forbids non-default rollout training parameters");
            }
            if source.rollouts_per_state.is_some() || source.seed.is_some() || !overrides.is_empty()
            {
                bail!("ehs2-percentile forbids rollout samples, seed, and opponent overrides");
            }
            Ok(AbstractionConfig {
                flop_buckets: cast("flop buckets", buckets.flop)?,
                turn_buckets: cast("turn buckets", buckets.turn)?,
                river_buckets: cast("river buckets", buckets.river)?,
                rollout_samples: 512,
                points_per_bucket: training.points_per_bucket,
                kmeans_iterations: training.kmeans_iterations,
                seed: 0,
                active_opponent_buckets: Vec::new(),
                artifact_cache: None,
                recall: match information.recall {
                    Recall::CurrentStreet => RecallMode::Street,
                    Recall::BucketHistory => RecallMode::Full,
                },
                kind: AbstractionKind::Ehs2Table,
            })
        }
    }
}

fn lower_economics(
    source: Economics,
    seats: &[SeatConfig],
) -> Result<(UtilitySection, RakeSection)> {
    match source {
        Economics::Cash { rake: None } => Ok((UtilitySection::ChipEv, RakeSection::None)),
        Economics::Cash { rake: Some(rake) } => {
            if rake.rounding_unit_bb != 0.001 {
                bail!("economics.rake.rounding_unit_bb must be exactly 0.001 in v1");
            }
            if !(0.0..=1.0).contains(&rake.rate) || !rake.rate.is_finite() {
                bail!("economics.rake.rate must be finite and within [0, 1]");
            }
            if let Some(cap) = rake.cap_bb {
                check_grid("economics.rake.cap_bb", cap, true)?;
            }
            let allocation = match rake.allocation {
                RakeAllocation::MainFirst => RuntimeRakeAllocation::MainFirst,
                RakeAllocation::Proportional => RuntimeRakeAllocation::Proportional,
            };
            let rounding = match rake.rounding {
                Rounding::Down => RuntimeRakeRounding::Down,
                Rounding::Nearest => RuntimeRakeRounding::Nearest,
                Rounding::Up => RuntimeRakeRounding::Up,
            };
            Ok((
                UtilitySection::ChipEv,
                RakeSection::Generic {
                    rate: rake.rate,
                    cap: rake.cap_bb,
                    when: rake.when,
                    allocation,
                    rounding,
                    // Multiway chip amounts are big blinds on the fixed
                    // `.001 BB` grid; the lowered rounding unit says so.
                    rounding_unit: 0.001,
                },
            ))
        }
        Economics::TournamentIcm {
            mut payouts,
            outside_field_bb,
            samples,
            seed,
        } => {
            let field = seats.len() + outside_field_bb.len();
            if field > multiway::icm::ICM_MAX_PLAYERS {
                bail!(
                    "ICM field exceeds {} players",
                    multiway::icm::ICM_MAX_PLAYERS
                );
            }
            if field <= multiway::icm::EXACT_ICM_MAX_PLAYERS
                && (samples.is_some() || seed.is_some())
            {
                bail!("samples and seed are meaningless for exact ICM fields of 15 or fewer");
            }
            if payouts.len() > field {
                bail!("economics.payouts has more entries than the field");
            }
            payouts.resize(field, 0.0);
            let outside_field = outside_field_bb
                .into_iter()
                .enumerate()
                .map(|(index, stack_bb)| {
                    check_grid(&format!("outside_field_bb[{index}]"), stack_bb, false)?;
                    Ok(OutsidePlayerSection {
                        name: format!("outside-{index}"),
                        stack_bb,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((
                UtilitySection::TournamentIcm {
                    outside_field,
                    payouts,
                    samples: samples.unwrap_or(100_000),
                    seed: seed.unwrap_or(0),
                },
                RakeSection::None,
            ))
        }
    }
}

fn check_grid(field: &str, value: f64, zero_allowed: bool) -> Result<()> {
    if !value.is_finite() || value < 0.0 || (!zero_allowed && value == 0.0) {
        bail!(
            "{field} must be a {} finite BB amount",
            if zero_allowed {
                "non-negative"
            } else {
                "positive"
            }
        );
    }
    let scaled = value * 1000.0;
    if scaled > u64::MAX as f64 || (scaled - scaled.round()).abs() > 1e-9 {
        bail!("{field} must be exactly representable in .001 BB units");
    }
    Ok(())
}

fn parse_threads(value: AutoOrUsize) -> Result<Option<usize>> {
    match value {
        AutoOrUsize::Auto => Ok(None),
        AutoOrUsize::Count(0) => bail!("run.resources.threads must be positive"),
        AutoOrUsize::Count(value) => Ok(Some(value)),
        AutoOrUsize::Name(name) if name == "auto" => Ok(None),
        AutoOrUsize::Name(name) => bail!("invalid thread setting {name:?}"),
    }
}

fn parse_memory(value: AutoOrMemory) -> Result<Option<u64>> {
    match value {
        AutoOrMemory::Auto => Ok(None),
        AutoOrMemory::Bytes(0) => bail!("run.resources.memory must be positive"),
        AutoOrMemory::Bytes(value) => Ok(Some(value)),
        AutoOrMemory::Name(name) if name == "auto" => Ok(None),
        AutoOrMemory::Name(name) => parse_bytes(&name).map(Some),
    }
}

fn parse_bytes(value: &str) -> Result<u64> {
    for (suffix, factor) in [
        ("GiB", 1u64 << 30),
        ("MiB", 1u64 << 20),
        ("KiB", 1u64 << 10),
    ] {
        if let Some(number) = value.strip_suffix(suffix) {
            let number: u64 = number
                .parse()
                .with_context(|| format!("invalid memory value {value:?}"))?;
            return number
                .checked_mul(factor)
                .ok_or_else(|| anyhow!("memory value overflows u64"));
        }
    }
    bail!("invalid memory value {value:?}; use auto or an integer KiB/MiB/GiB value")
}

fn parse_duration(value: &str) -> Result<u64> {
    for (suffix, factor) in [("s", 1), ("m", 60), ("h", 3_600)] {
        if let Some(number) = value.strip_suffix(suffix) {
            let count = number
                .parse::<u64>()
                .with_context(|| format!("invalid checkpoint interval {value:?}"))?;
            if count == 0 {
                bail!("checkpoint interval must be positive");
            }
            return count
                .checked_mul(factor)
                .ok_or_else(|| anyhow!("checkpoint interval overflows"));
        }
    }
    bail!("invalid checkpoint interval {value:?}")
}

fn yes() -> bool {
    true
}
fn one_u64() -> u64 {
    1
}
/// Promoted from the 2026-07-25 abstraction study's Tournament 6-max/50bb
/// anchor (see `docs/validation/multiway-abstraction-optimization-2026-07-25.md`).
/// The Cash 6-max/100bb anchor measured better at 256; that stays an explicit
/// override rather than a second, utility-conditional default.
fn default_buckets() -> u32 {
    128
}
fn default_points_per_bucket() -> u32 {
    8
}
fn default_kmeans_iterations() -> u32 {
    20
}
fn default_discount_every() -> u64 {
    10_000
}
fn default_discount_until() -> u64 {
    10_000_000
}
fn default_max_sweeps() -> u64 {
    5_000_000
}
fn default_check_every() -> u64 {
    10_000
}
fn default_confirmations() -> u32 {
    3
}
fn default_evaluation_samples() -> u64 {
    4_096
}
fn default_deviator_traversals() -> u64 {
    20_000
}
fn default_rake_when() -> String {
    "flop_dealt".into()
}
fn default_rounding_unit() -> f64 {
    0.001
}
fn default_checkpoint_interval() -> String {
    "15m".into()
}
fn default_target() -> Target {
    Target::Name("default".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiway::abstraction::FeatureHashAbstraction;
    use multiway::holdem::HoldemGame;
    use multiway::solver::ExternalSamplingGame;

    const MINIMAL: &str = r#"
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"
"#;

    const PRODUCTION_MINIMAL: &str = r#"
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"

[game.abstraction]
kind = "ehs2-percentile"
"#;

    fn game_fingerprint(lowered: SolveConfig) -> [u8; 32] {
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        let utility = crate::session::convert_utility(lowered.utility).unwrap();
        let rake = crate::session::convert_rake(lowered.rake);
        HoldemGame::new(&game, &utility, &rake, FeatureHashAbstraction::default())
            .unwrap()
            .game_fingerprint()
    }

    #[test]
    fn minimal_config_materializes_v1_defaults() {
        let lowered = parse_and_lower(MINIMAL).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        assert_eq!(game.seats.len(), 6);
        assert!(
            game.seats
                .iter()
                .all(|seat| seat.name.is_none() && seat.range.is_empty())
        );
        assert_eq!(game.abstraction.flop_buckets, 128);
        assert_eq!(game.abstraction.rollout_samples, 512);
        assert_eq!(game.abstraction.points_per_bucket, 8);
        assert_eq!(game.abstraction.kmeans_iterations, 20);
        assert_eq!(game.abstraction.recall, RecallMode::Street);
        let AlgorithmSection::ExternalSamplingMccfr {
            exploration_epsilon,
            discount_every,
            traverser_vector,
            prune,
            ..
        } = lowered.algorithm
        else {
            panic!()
        };
        assert_eq!(exploration_epsilon, 0.0);
        assert_eq!(discount_every, 10_000);
        assert!(traverser_vector);
        assert!(prune);
        assert_eq!(checkpoint_interval_secs(MINIMAL).unwrap(), 900);
        assert_eq!(max_time_secs(MINIMAL).unwrap(), None);
        let timed = format!("{MINIMAL}\n[run]\nmax_time = \"1h\"\n");
        assert_eq!(max_time_secs(&timed).unwrap(), Some(3_600));
        assert!(parse_and_lower(&timed).is_ok());
    }

    #[test]
    fn production_contract_is_ehs2_current_street_only() {
        validate_production_contract(PRODUCTION_MINIMAL).unwrap();

        let omitted = validate_production_contract(MINIMAL)
            .expect_err("historical rollout default must not be silently reinterpreted");
        assert!(omitted.to_string().contains(ROLLOUT_REMOVED_CODE));
        assert!(
            omitted
                .to_string()
                .contains("omission historically selected")
        );

        let rollout = format!("{MINIMAL}\n[game.abstraction]\nkind = \"multiway-rollout\"\n");
        let rollout_error = validate_production_contract(&rollout)
            .unwrap_err()
            .to_string();
        assert!(rollout_error.contains(ROLLOUT_REMOVED_CODE));
        assert!(rollout_error.contains("preallocation contract"));

        let full = format!(
            "{PRODUCTION_MINIMAL}\n[game.information]\nrecall = \"bucket-history\"\n\n\
             [solver.pruning]\nkind = \"none\"\n"
        );
        let full_error = validate_production_contract(&full).unwrap_err().to_string();
        assert!(full_error.contains(FULL_RECALL_REMOVED_CODE));
        assert!(full_error.contains("3,695/10,000"));
    }

    #[test]
    fn production_contract_rejects_every_rollout_only_child_option() {
        for retired in [
            "rollouts_per_state = 512",
            "seed = 7",
            "training = { points_per_bucket = 8, kmeans_iterations = 20 }",
            "opponent_buckets = { 1 = { flop = 64, turn = 64, river = 64 } }",
        ] {
            let raw = PRODUCTION_MINIMAL.replace(
                "kind = \"ehs2-percentile\"",
                &format!("kind = \"ehs2-percentile\"\n{retired}"),
            );
            let error = validate_production_contract(&raw).unwrap_err().to_string();
            assert!(error.contains(ROLLOUT_REMOVED_CODE), "{error}");
            assert!(error.contains("rollout-only"), "{error}");
        }
    }

    #[test]
    fn bucket_history_requires_pruning_none() {
        let bucket_history =
            format!("{MINIMAL}\n[game.information]\nrecall = \"bucket-history\"\n");
        let error = parse_and_lower(&bucket_history)
            .expect_err("default regret-based pruning is unsupported with bucket-history");
        assert!(error.to_string().contains(
            "regret-based pruning requires game.information.recall = \"current-street\""
        ));

        let explicit_none = format!("{bucket_history}\n[solver.pruning]\nkind = \"none\"\n");
        let lowered = parse_and_lower(&explicit_none).unwrap();
        let AlgorithmSection::ExternalSamplingMccfr { prune, .. } = lowered.algorithm else {
            panic!()
        };
        assert!(!prune);
    }

    #[test]
    fn normalized_effective_toml_is_reparsable() {
        let effective = normalized_toml(MINIMAL).unwrap();
        assert!(effective.contains("rollouts_per_state = 512"));
        assert!(effective.contains("points_per_bucket = 8"));
        assert!(effective.contains("kmeans_iterations = 20"));
        assert!(effective.contains("probability_encoding = \"u16\""));
        parse_and_lower(&effective).unwrap();
    }

    #[test]
    fn abstraction_training_effective_round_trip_and_legacy_omission_are_stable() {
        let explicit_defaults = format!(
            "{MINIMAL}\n[game.abstraction.training]\n\
             points_per_bucket = 8\nkmeans_iterations = 20\n"
        );
        let omitted_effective = normalized_toml(MINIMAL).unwrap();
        let explicit_effective = normalized_toml(&explicit_defaults).unwrap();
        assert_eq!(omitted_effective, explicit_effective);
        assert_eq!(
            formats::config_hash(omitted_effective.as_bytes()),
            formats::config_hash(explicit_effective.as_bytes())
        );
        assert_eq!(
            omitted_effective,
            normalized_toml(&omitted_effective).unwrap()
        );

        let tuned = format!(
            "{MINIMAL}\n[game.abstraction.training]\n\
             points_per_bucket = 16\nkmeans_iterations = 40\n"
        );
        let lowered = parse_and_lower(&tuned).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        assert_eq!(game.abstraction.points_per_bucket, 16);
        assert_eq!(game.abstraction.kmeans_iterations, 40);
        let tuned_effective = normalized_toml(&tuned).unwrap();
        assert_ne!(
            formats::config_hash(tuned_effective.as_bytes()),
            formats::config_hash(omitted_effective.as_bytes())
        );
        assert_eq!(tuned_effective, normalized_toml(&tuned_effective).unwrap());
    }

    #[test]
    fn abstraction_training_validation_and_ehs2_contract_are_enforced() {
        for (field, value) in [("points_per_bucket", 0), ("kmeans_iterations", 0)] {
            let raw = format!("{MINIMAL}\n[game.abstraction.training]\n{field} = {value}\n");
            let error = parse_and_lower(&raw).unwrap_err().to_string();
            assert!(error.contains(field), "{error}");
            assert!(error.contains("must be positive"), "{error}");
        }

        let ehs2_default = format!(
            "{MINIMAL}\n[game.abstraction]\nkind = \"ehs2-percentile\"\n\n\
             [game.abstraction.training]\n\
             points_per_bucket = 8\nkmeans_iterations = 20\n"
        );
        assert!(parse_and_lower(&ehs2_default).is_ok());
        let ehs2_omitted = format!("{MINIMAL}\n[game.abstraction]\nkind = \"ehs2-percentile\"\n");
        assert_eq!(
            normalized_toml(&ehs2_default).unwrap(),
            normalized_toml(&ehs2_omitted).unwrap()
        );

        for training in [
            "points_per_bucket = 16\nkmeans_iterations = 20",
            "points_per_bucket = 8\nkmeans_iterations = 40",
        ] {
            let raw = format!(
                "{MINIMAL}\n[game.abstraction]\nkind = \"ehs2-percentile\"\n\n\
                 [game.abstraction.training]\n{training}\n"
            );
            let error = parse_and_lower(&raw).unwrap_err().to_string();
            assert!(
                error.contains("forbids non-default rollout training parameters"),
                "{error}"
            );
        }
    }

    #[test]
    fn benchmark_tree_settings_lower_and_keep_a_stable_effective_fingerprint() {
        let raw = format!(
            "{MINIMAL}\n[game.tree]\nkind = \"standard\"\nallow_limp = false\n\
             reraise_jam_above_actor_starting_stack = {{ numerator = 1, denominator = 3 }}\n\n\
             [game.tree.max_aggressive_actions]\npreflop = 6\nflop = 4\nturn = 4\nriver = 4\n"
        );
        let lowered = parse_and_lower(&raw).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        assert!(!game.betting.allow_limp);
        assert_eq!(game.betting.preflop.max_aggressive_actions, 6);
        assert_eq!(game.betting.flop.max_aggressive_actions, 4);
        assert_eq!(game.betting.turn.max_aggressive_actions, 4);
        assert_eq!(game.betting.river.max_aggressive_actions, 4);
        assert_eq!(
            game.betting.preflop.reraise_jam_above_actor_starting_stack,
            Some(StackRatio {
                numerator: 1,
                denominator: 3,
            })
        );
        assert!(
            game.betting
                .flop
                .reraise_jam_above_actor_starting_stack
                .is_none()
        );

        let first = normalized_toml(&raw).unwrap();
        let second = normalized_toml(&first).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            formats::config_hash(first.as_bytes()),
            formats::config_hash(second.as_bytes())
        );
    }

    #[test]
    fn omitted_tree_settings_preserve_legacy_runtime_defaults() {
        let raw = format!("{MINIMAL}\n[game.tree]\nkind = \"standard\"\n");
        let lowered = parse_and_lower(&raw).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        let defaults = BettingConfig::default();
        assert_eq!(game.betting.allow_limp, defaults.allow_limp);
        assert_eq!(
            game.betting.preflop.max_aggressive_actions,
            defaults.preflop.max_aggressive_actions
        );
        assert_eq!(
            game.betting.flop.max_aggressive_actions,
            defaults.flop.max_aggressive_actions
        );
        assert_eq!(
            game.betting.turn.max_aggressive_actions,
            defaults.turn.max_aggressive_actions
        );
        assert_eq!(
            game.betting.river.max_aggressive_actions,
            defaults.river.max_aggressive_actions
        );
        assert!(
            game.betting
                .preflop
                .reraise_jam_above_actor_starting_stack
                .is_none()
        );
        assert_eq!(
            game_fingerprint(parse_and_lower(MINIMAL).unwrap()),
            game_fingerprint(parse_and_lower(&raw).unwrap())
        );
    }

    #[test]
    fn invalid_reraise_jam_ratio_is_rejected_during_v1_parse() {
        for ratio in [
            "{ numerator = 0, denominator = 3 }",
            "{ numerator = 1, denominator = 0 }",
            "{ numerator = 4, denominator = 3 }",
        ] {
            let raw = format!(
                "{MINIMAL}\n[game.tree]\nkind = \"standard\"\n\
                 reraise_jam_above_actor_starting_stack = {ratio}\n"
            );
            assert!(
                parse_and_lower(&raw)
                    .unwrap_err()
                    .to_string()
                    .contains("must be a ratio in (0, 1]")
            );
        }
    }

    #[test]
    fn solve_time_overrides_are_normalized_and_validated() {
        let memory = "12GiB";
        let effective =
            apply_solve_overrides(PRODUCTION_MINIMAL, Some(3), Some(memory), Some("2h"), None)
                .unwrap();
        assert_eq!(max_time_secs(&effective).unwrap(), Some(7_200));
        let value: toml::Value = toml::from_str(&effective).unwrap();
        assert_eq!(value["run"]["resources"]["threads"].as_integer(), Some(3));
        assert_eq!(value["run"]["resources"]["memory"].as_str(), Some(memory));
        let resumed = apply_resume_overrides(
            PRODUCTION_MINIMAL,
            None,
            None,
            None,
            Some(42),
            Some(0.02),
            Some(128),
            Some(7),
            Some("2m"),
        )
        .unwrap();
        let resumed: toml::Value = toml::from_str(&resumed).unwrap();
        assert_eq!(resumed["run"]["max_sweeps"].as_integer(), Some(42));
        assert_eq!(
            resumed["run"]["stop"]["check_every_sweeps"].as_integer(),
            Some(7)
        );
        assert_eq!(
            resumed["run"]["checkpoint"]["interval"].as_str(),
            Some("2m")
        );
    }

    #[test]
    fn production_memory_override_accepts_values_above_the_auto_budget() {
        validate_production_contract(PRODUCTION_MINIMAL).unwrap();
        let at_limit =
            apply_solve_overrides(PRODUCTION_MINIMAL, None, Some("6GiB"), None, None).unwrap();
        validate_production_contract(&at_limit).unwrap();

        let above_limit =
            apply_solve_overrides(PRODUCTION_MINIMAL, None, Some("7GiB"), None, None).unwrap();
        validate_production_contract(&above_limit).unwrap();
        let value: toml::Value = toml::from_str(&above_limit).unwrap();
        assert_eq!(value["run"]["resources"]["memory"].as_str(), Some("7GiB"));
    }

    #[test]
    fn generic_rake_allocation_and_rounding_lower_to_runtime() {
        let raw = format!(
            "{MINIMAL}\n[economics]\nkind = \"cash\"\n\
             [economics.rake]\nrate = 0.05\nwhen = \"true\"\n\
             allocation = \"proportional\"\nrounding = \"nearest\"\n"
        );
        let lowered = parse_and_lower(&raw).unwrap();
        assert!(matches!(
            &lowered.rake,
            RakeSection::Generic {
                allocation: RuntimeRakeAllocation::Proportional,
                rounding: RuntimeRakeRounding::Nearest,
                cap: None,
                ..
            }
        ));
        crate::session::convert_rake(lowered.rake)
            .compile()
            .unwrap();
    }

    #[test]
    fn unknown_and_sub_millibb_values_are_rejected() {
        assert!(parse_and_lower(&MINIMAL.replace("button = 0", "button = 0\ntyop = 1")).is_err());
        assert!(parse_and_lower(&MINIMAL.replace("100.0", "100.0001")).is_err());
    }

    #[test]
    fn sparse_overrides_and_icm_payout_tail_are_normalized() {
        let raw = format!(
            "{MINIMAL}\n[[game.players]]\nseat = 4\nstack_bb = 43.275\nrange = \"QQ+\"\n\n[economics]\nkind = \"tournament-icm\"\npayouts = [1000.0, 600.0, 400.0]\n"
        );
        let lowered = parse_and_lower(&raw).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        assert_eq!(game.seats[4].stack_bb, 43.275);
        let UtilitySection::TournamentIcm { payouts, .. } = lowered.utility else {
            panic!()
        };
        assert_eq!(payouts, vec![1000.0, 600.0, 400.0, 0.0, 0.0, 0.0]);
    }
    #[test]
    fn arbitrary_forced_bets_and_first_actor_reach_betting_state() {
        let raw = MINIMAL.replace(
            "button = 0",
            "button = 0\ncommon_ante_bb = 0.25\npreflop_first_to_act = 4",
        ) + r#"

[[game.players]]
seat = 1
blind_bb = 0.0

[[game.players]]
seat = 3
stack_bb = 0.75
blind_bb = 2.0

[[game.players]]
seat = 4
ante_bb = 0.125
"#;
        let lowered = parse_and_lower(&raw).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        let validated = game.validated().unwrap();
        let state = multiway::BettingState::new(&validated).unwrap();
        assert_eq!(state.to_act, Some(SeatId::new_unchecked(4)));
        assert_eq!(state.big_blind.raw(), 2_000);
        assert_eq!(state.current_wager(SeatId::new_unchecked(1)).raw(), 0);
        assert_eq!(state.current_wager(SeatId::new_unchecked(3)).raw(), 750);
        assert_eq!(state.amount_to_call(SeatId::new_unchecked(4)).raw(), 2_000);
        assert_eq!(state.pot_size().raw(), 2_125);
    }

    /// Size literals are shared with the postflop family and canonicalize
    /// to PioSOLVER's spelling, so an effective config states one form
    /// whichever the author wrote.
    #[test]
    fn size_literals_normalize_to_the_pio_spelling() {
        let raw = format!(
            "{MINIMAL}\n[game.tree]\nkind = \"standard\"\n\
             [[game.tree.rules]]\nstreet = \"preflop\"\nwhen = \"unopened\"\n\
             effect = \"replace\"\naction = \"raise\"\n\
             sizes = [\"2.2x\", \"allin\", \"50%pot\", \"geometric(allin,2)\"]\n"
        );
        let effective = normalized_toml(&raw).unwrap();
        assert!(effective.contains("\"a\""), "{effective}");
        assert!(effective.contains("\"50\""), "{effective}");
        assert!(effective.contains("\"2e\""), "{effective}");
        assert!(!effective.contains("\"allin\""), "{effective}");
        assert!(!effective.contains("%pot"), "{effective}");
        assert_eq!(normalized_toml(&effective).unwrap(), effective);
    }

    #[test]
    fn standard_tree_rules_lower_sizes_and_change_legal_actions() {
        let raw = format!(
            "{MINIMAL}\n[game.tree]\nkind = \"standard\"\n\
             [[game.tree.rules]]\npriority = 100\nstreet = \"preflop\"\n\
             when = 'unopened && position == \"UTG\"'\neffect = \"replace\"\n\
             action = \"raise\"\nsizes = [\"2.2x\", \"allin\"]\n"
        );
        let lowered = parse_and_lower(&raw).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        let validated = game.validated().unwrap();
        let state = multiway::BettingState::new(&validated).unwrap();
        let actions = state.legal_actions(&validated.betting).unwrap();
        assert!(actions.iter().any(|action| matches!(
            action,
            multiway::Action::RaiseTo { to, .. } if to.raw() == 2_200
        )));
        assert!(!actions.iter().any(|action| matches!(
            action,
            multiway::Action::RaiseTo { to, .. } if to.raw() == 2_500
        )));
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, multiway::Action::RaiseTo { all_in: true, .. }))
        );
    }

    #[test]
    fn typed_rules_accept_last_preflop_aggressor_position() {
        let raw = format!(
            "{MINIMAL}\n[game.tree]\nkind = \"standard\"\n[[game.tree.rules]]\nstreet = \"preflop\"\nwhen = 'last_preflop_aggressor_position == \"UTG\"'\neffect = \"remove\"\naction = \"call\"\n"
        );
        let lowered = parse_and_lower(&raw).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        assert_eq!(
            game.betting.rules[0].condition,
            "last_preflop_aggressor_position == \"UTG\""
        );
        assert_ne!(
            game_fingerprint(parse_and_lower(MINIMAL).unwrap()),
            game_fingerprint(parse_and_lower(&raw).unwrap())
        );
    }

    #[test]
    fn script_roundtrip_preserves_last_preflop_aggressor_position() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("game.toml");
        let script_path = directory.path().join("position.mwtree");
        std::fs::write(
            &script_path,
            "preflop when last_preflop_aggressor_position == \"UTG\" { remove call }\n",
        )
        .unwrap();
        let raw =
            format!("{MINIMAL}\n[game.tree]\nkind = \"script\"\nsource = \"position.mwtree\"\n");
        let effective = normalized_toml_at(&raw, &config_path).unwrap();
        assert!(effective.contains("last_preflop_aggressor_position"));
        std::fs::remove_file(&script_path).unwrap();
        assert!(parse_and_lower(&effective).is_ok());
    }

    /// `Tree::Script` normalizes by inlining the file's contents into
    /// `script`, exactly as the postflop family's `[game.tree] source`
    /// inlines into `script` (`crate::config::TreeSection::resolve_source_at`)
    /// -- it does *not* expand into `Tree::Standard { rules }` the way it
    /// used to. Both halves of "self-contained" still have to hold: no
    /// `source =` (and therefore no path) remains in the effective config,
    /// and deleting the original `.mwtree` file and re-lowering the
    /// effective config still reaches the identical game.
    #[test]
    fn script_effective_config_is_self_contained() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("game.toml");
        let script_path = directory.path().join("tree.mwtree");
        std::fs::write(
            &script_path,
            "preflop when unopened { replace raise [2.2x, allin] }\n",
        )
        .unwrap();
        let raw = format!(
            "{MINIMAL}\n[game.tree]\nkind = \"script\"\nsource = \"tree.mwtree\"\n\
             allow_limp = false\n\
             reraise_jam_above_actor_starting_stack = {{ numerator = 1, denominator = 3 }}\n\n\
             [game.tree.max_aggressive_actions]\npreflop = 6\nflop = 4\nturn = 4\nriver = 4\n"
        );
        let script_fingerprint = game_fingerprint(parse_and_lower_at(&raw, &config_path).unwrap());
        let effective = normalized_toml_at(&raw, &config_path).unwrap();
        assert!(effective.contains("kind = \"script\""));
        assert!(effective.contains("2.2x"));
        assert!(effective.contains("allow_limp = false"));
        assert!(effective.contains("preflop = 6"));
        assert!(effective.contains("reraise_jam_above_actor_starting_stack"));
        assert_eq!(effective.matches("source =").count(), 0);
        assert!(!effective.contains("tree.mwtree"));
        // The script body is inlined verbatim, not expanded into rules: the
        // rendered condition (`unopened`) and the raw size tokens
        // (`2.2x`/`allin`) both appear as the script wrote them, and there
        // is no lowered `[[game.tree.rules]]` array.
        assert!(effective.contains("unopened"));
        assert!(effective.contains("allin"));
        assert_eq!(effective.matches("[[game.tree.rules]]").count(), 0);

        std::fs::remove_file(&script_path).unwrap();
        let lowered = parse_and_lower(&effective).unwrap();
        let GameSection::PreflopMultiway(game) = lowered.game else {
            panic!()
        };
        assert!(!game.betting.allow_limp);
        assert_eq!(game.betting.preflop.max_aggressive_actions, 6);
        assert_eq!(
            game.betting.preflop.reraise_jam_above_actor_starting_stack,
            Some(StackRatio {
                numerator: 1,
                denominator: 3,
            })
        );
        assert_eq!(
            script_fingerprint,
            game_fingerprint(parse_and_lower(&effective).unwrap())
        );

        // Normalizing twice is byte-identical -- normalizing the already-
        // inlined effective config is a no-op, not a second inlining.
        assert_eq!(normalized_toml(&effective).unwrap(), effective);
    }

    #[test]
    fn mwtree_frontend_compiles_params_and_checkdown() {
        let program = r#"
param open = 2.5x
preflop when unopened {
  replace raise [open, allin]
}
flop when players >= 4 {
  checkdown
}
"#;
        let mut params = BTreeMap::new();
        params.insert("open".into(), toml::Value::String("2.2x".into()));
        let rules = lower_tree_script(program, &params).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].effect, RuleEffect::Replace);
        assert_eq!(
            rules[0].sizes,
            vec![
                SizeSpec::PreviousBetMultiple { factor: 2.2 },
                SizeSpec::AllIn
            ]
        );
        assert_eq!(rules[1].effect, RuleEffect::Checkdown);
    }

    #[test]
    fn script_source_and_script_are_exclusive_and_one_is_required() {
        let both = format!(
            "{MINIMAL}\n[game.tree]\nkind = \"script\"\nsource = \"a.mwtree\"\n\
             script = \"flop {{ checkdown }}\"\n"
        );
        let error = parse_and_lower(&both).unwrap_err().to_string();
        assert!(error.contains("exclusive"), "{error}");

        let neither = format!("{MINIMAL}\n[game.tree]\nkind = \"script\"\n");
        let error = parse_and_lower(&neither).unwrap_err().to_string();
        assert!(error.contains("requires source or script"), "{error}");
    }

    /// End-to-end proof that multiway's `.mwtree` frontend is the same
    /// `cards::script` grammar postflop's `.tree` scripts use: nesting,
    /// `if`/`else if`/`else`, a multi-street list (`flop, turn`), and
    /// `param` all compile through `source =`, and the normalized effective
    /// config inlines the script body (not a lowered `[[game.tree.rules]]`
    /// array) while still lowering to the identical game after the file is
    /// deleted -- the same "self-contained" property
    /// `script_effective_config_is_self_contained` checks for the flat case.
    #[test]
    fn mwtree_script_with_nesting_and_if_else_is_the_same_front_end_as_postflop() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("game.toml");
        let script_path = directory.path().join("short-stack.mwtree");
        std::fs::write(
            &script_path,
            r#"
param open = 2.2x

preflop when unopened {
  replace raise [open, allin]
  when position in ["CO", "BTN"] {
    replace raise [open, 2.5x, allin]
  }
}

flop, turn when players >= 4 {
  checkdown
}

river {
  if spr <= 0.8      { force bet [1e] }
  else if unopened   { replace bet [66, a] }
  else               { remove bet }
}
"#,
        )
        .unwrap();
        let raw =
            format!("{MINIMAL}\n[game.tree]\nkind = \"script\"\nsource = \"short-stack.mwtree\"\n");

        let script_fingerprint = game_fingerprint(parse_and_lower_at(&raw, &config_path).unwrap());
        let effective = normalized_toml_at(&raw, &config_path).unwrap();

        // Inlined, not expanded: the script's own tokens survive verbatim,
        // there is no lowered rule array, and no path remains.
        assert!(effective.contains("kind = \"script\""));
        assert!(effective.contains("when unopened"));
        assert!(effective.contains("when position in"));
        assert!(effective.contains("if spr <= 0.8"));
        assert!(effective.contains("else if unopened"));
        assert_eq!(effective.matches("[[game.tree.rules]]").count(), 0);
        assert_eq!(effective.matches("source =").count(), 0);
        assert!(!effective.contains("short-stack.mwtree"));

        // Normalizing an already-inlined effective config is a no-op.
        assert_eq!(normalized_toml(&effective).unwrap(), effective);

        // Deleting the original file changes nothing: the effective config
        // is the whole story.
        std::fs::remove_file(&script_path).unwrap();
        assert_eq!(
            script_fingerprint,
            game_fingerprint(parse_and_lower(&effective).unwrap())
        );
    }
}
