//! Dedicated parser and normalizer for `solvers.multiway-preflop/v1`.
//!
//! The public v1 schema intentionally does not become another variant of
//! the historical shared CLI schema. During the engine migration this
//! module lowers the supported v1 surface to the existing runtime structs;
//! unsupported v1 combinations fail explicitly instead of being ignored.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use multiway::config::{
    AbstractionConfig, AbstractionKind, ActiveOpponentBucketConfig, AnteConfig, BettingConfig,
    BlindConfig, ForcedBetConfig, MultiwayConfig, RakeAllocation as RuntimeRakeAllocation,
    RakeRounding as RuntimeRakeRounding, RecallMode, RuleAction, RuleEffect, RuleStreet,
    SeatConfig, SizeSpec, TreeRule as RuntimeTreeRule,
};
use multiway::types::{MAX_SEATS, MIN_SEATS, SeatId};
use serde::{Deserialize, Serialize};

use crate::config::{
    AlgorithmSection, GameSection, OutsidePlayerSection, RakeSection, RunSection, SolveConfig,
    StorageKind, UtilitySection,
};

pub const SCHEMA: &str = "solvers.multiway-preflop/v1";

pub fn has_v1_schema(raw: &str) -> Result<bool> {
    let value: toml::Value = toml::from_str(raw).context("parsing TOML document")?;
    let Some(schema) = value.get("schema") else {
        return Ok(false);
    };
    let schema = schema
        .as_str()
        .ok_or_else(|| anyhow!("schema must be a string"))?;
    if schema != SCHEMA {
        bail!("unsupported config schema {schema:?}; expected {SCHEMA:?}");
    }
    Ok(true)
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
    parse_and_lower(&effective).context("validating solve-time v1 overrides")?;
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
    #[serde(default)]
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
        #[serde(default)]
        rules: Vec<TreeRule>,
    },
    Script {
        source: String,
        #[serde(default)]
        params: BTreeMap<String, toml::Value>,
    },
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

fn parse_size_literal(source: &str) -> Result<SizeSpec> {
    let source = source.trim();
    if source == "min" {
        return Ok(SizeSpec::MinRaise);
    }
    if source == "allin" {
        return Ok(SizeSpec::AllIn);
    }
    if let Some(inner) = source
        .strip_prefix("geometric(allin,")
        .and_then(|value| value.strip_suffix(')'))
    {
        let inner = inner.trim();
        let inner = inner.strip_prefix("streets=").unwrap_or(inner);
        let streets = inner
            .parse::<u8>()
            .with_context(|| format!("invalid geometric street count in {source:?}"))?;
        if streets == 0 {
            bail!("geometric street count must be positive");
        }
        return Ok(SizeSpec::GeometricAllIn { streets });
    }
    let (number, kind) = ["%effective", "%stack", "%pot", "bb", "x"]
        .into_iter()
        .find_map(|suffix| source.strip_suffix(suffix).map(|number| (number, suffix)))
        .ok_or_else(|| anyhow!("invalid tree size literal {source:?}"))?;
    let value = number
        .parse::<f64>()
        .with_context(|| format!("invalid number in tree size {source:?}"))?;
    if !value.is_finite() || value <= 0.0 {
        bail!("tree size must be finite and positive: {source:?}");
    }
    Ok(match kind {
        "bb" => SizeSpec::ToBb { value },
        "%pot" => SizeSpec::PotAfterCall {
            fraction: value / 100.0,
        },
        "x" if value > 1.0 => SizeSpec::PreviousBetMultiple { factor: value },
        "x" => bail!("current-bet multiple must be greater than one: {source:?}"),
        "%effective" => SizeSpec::EffectiveStackFraction {
            fraction: value / 100.0,
        },
        "%stack" => SizeSpec::StackFraction {
            fraction: value / 100.0,
        },
        _ => unreachable!(),
    })
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
        lowered.push(RuntimeTreeRule {
            priority: rule.priority,
            source_order: source_order as u32,
            street: rule.street,
            condition: rule.condition.trim().to_owned(),
            effect: rule.effect,
            action: rule.action,
            sizes,
        });
    }
    lowered.sort_by_key(|rule| (rule.priority, rule.source_order));
    Ok(lowered)
}
fn lower_tree_script(
    source: &str,
    params: &BTreeMap<String, toml::Value>,
) -> Result<Vec<RuntimeTreeRule>> {
    let mut values = BTreeMap::<String, String>::new();
    let mut program = String::new();
    for line in source.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(declaration) = line.strip_prefix("param ") {
            let (name, value) = declaration
                .split_once('=')
                .ok_or_else(|| anyhow!("invalid mwtree param declaration {line:?}"))?;
            values.insert(name.trim().to_owned(), value.trim().to_owned());
        } else {
            program.push_str(line);
            program.push('\n');
        }
    }
    for (name, value) in params {
        let value = match value {
            toml::Value::String(value) => value.clone(),
            toml::Value::Integer(value) => value.to_string(),
            toml::Value::Float(value) if value.is_finite() => value.to_string(),
            toml::Value::Boolean(value) => value.to_string(),
            _ => bail!("mwtree param {name:?} must be a scalar"),
        };
        values.insert(name.clone(), value);
    }

    let mut rules = Vec::new();
    let mut remaining = program.as_str();
    while !remaining.trim().is_empty() {
        remaining = remaining.trim_start();
        let open = remaining
            .find('{')
            .ok_or_else(|| anyhow!("mwtree rule is missing '{{'"))?;
        let close = remaining[open + 1..]
            .find('}')
            .map(|index| open + 1 + index)
            .ok_or_else(|| anyhow!("mwtree rule is missing '}}'"))?;
        let header = remaining[..open].trim();
        let body = remaining[open + 1..close].trim();
        remaining = &remaining[close + 1..];
        let (street, condition) = header
            .split_once(" when ")
            .ok_or_else(|| anyhow!("mwtree rule header requires 'street when condition'"))?;
        let street = parse_rule_street(street.trim())?;
        let condition = substitute_params(condition.trim(), &values);

        if body == "checkdown" {
            rules.push(TreeRule {
                priority: 100,
                street,
                condition,
                effect: RuleEffect::Checkdown,
                action: None,
                sizes: Vec::new(),
            });
            continue;
        }
        let mut words = body.splitn(3, char::is_whitespace);
        let effect = parse_rule_effect(words.next().unwrap_or(""))?;
        let action = parse_rule_action(words.next().unwrap_or(""))?;
        let sizes = parse_script_sizes(words.next().unwrap_or(""), &values)?;
        rules.push(TreeRule {
            priority: 100,
            street,
            condition,
            effect,
            action: Some(action),
            sizes,
        });
    }
    lower_tree_rules(&rules)
}

fn parse_rule_street(value: &str) -> Result<RuleStreet> {
    Ok(match value {
        "preflop" => RuleStreet::Preflop,
        "flop" => RuleStreet::Flop,
        "turn" => RuleStreet::Turn,
        "river" => RuleStreet::River,
        "postflop" => RuleStreet::Postflop,
        _ => bail!("invalid mwtree street {value:?}"),
    })
}

fn parse_rule_effect(value: &str) -> Result<RuleEffect> {
    Ok(match value {
        "add" => RuleEffect::Add,
        "remove" => RuleEffect::Remove,
        "replace" => RuleEffect::Replace,
        "force" => RuleEffect::Force,
        _ => bail!("invalid mwtree effect {value:?}"),
    })
}

fn parse_rule_action(value: &str) -> Result<RuleAction> {
    Ok(match value {
        "fold" => RuleAction::Fold,
        "check" => RuleAction::Check,
        "call" => RuleAction::Call,
        "bet" => RuleAction::Bet,
        "raise" => RuleAction::Raise,
        _ => bail!("invalid mwtree action {value:?}"),
    })
}

fn parse_script_sizes(source: &str, params: &BTreeMap<String, String>) -> Result<Vec<String>> {
    let source = source.trim();
    if source.is_empty() {
        return Ok(Vec::new());
    }
    let values = if source.starts_with('[') && source.ends_with(']') {
        source[1..source.len() - 1].split(',').collect::<Vec<_>>()
    } else {
        vec![source]
    };
    values
        .into_iter()
        .map(|value| {
            let value = value.trim();
            Ok(params
                .get(value)
                .cloned()
                .unwrap_or_else(|| value.to_owned()))
        })
        .collect()
}

fn substitute_params(source: &str, params: &BTreeMap<String, String>) -> String {
    let mut result = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let word = &source[start..index];
            result.push_str(params.get(word).map(String::as_str).unwrap_or(word));
        } else {
            result.push(bytes[index] as char);
            index += 1;
        }
    }
    result
}

impl Default for Tree {
    fn default() -> Self {
        Self::Standard { rules: Vec::new() }
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
    #[serde(default)]
    buckets: BucketCounts,
    #[serde(default)]
    opponent_buckets: BTreeMap<String, BucketCounts>,
}

impl Default for CardAbstraction {
    fn default() -> Self {
        Self {
            kind: CardAbstractionKind::MultiwayRollout,
            rollouts_per_state: None,
            seed: None,
            buckets: BucketCounts::default(),
            opponent_buckets: BTreeMap::new(),
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
        if matches!(
            self.game.abstraction.kind,
            CardAbstractionKind::MultiwayRollout
        ) {
            self.game.abstraction.rollouts_per_state.get_or_insert(512);
            self.game.abstraction.seed.get_or_insert(0);
        }
        Ok(())
    }

    /// Expands an external deterministic tree script into the standard rule
    /// frontend. Effective configs embedded in checkpoints and solutions
    /// therefore remain reparsable after the original .mwtree file moves.
    fn materialize_effective_at(&mut self, base_dir: &Path) -> Result<()> {
        self.materialize_effective()?;
        let Tree::Script { source, params } = &self.game.tree else {
            return Ok(());
        };
        let path = base_dir.join(source);
        let program = std::fs::read_to_string(&path)
            .with_context(|| format!("reading mwtree source {}", path.display()))?;
        let rules = lower_tree_script(&program, params)?
            .into_iter()
            .map(|rule| TreeRule {
                priority: rule.priority,
                street: rule.street,
                condition: rule.condition,
                effect: rule.effect,
                action: rule.action,
                sizes: rule
                    .sizes
                    .into_iter()
                    .map(|size| match size {
                        SizeSpec::ToBb { value } => format!("{value}bb"),
                        SizeSpec::PotAfterCall { fraction } => format!("{}%pot", fraction * 100.0),
                        SizeSpec::PreviousBetMultiple { factor } => format!("{factor}x"),
                        SizeSpec::MinRaise => "min".into(),
                        SizeSpec::AllIn => "allin".into(),
                        SizeSpec::EffectiveStackFraction { fraction } => {
                            format!("{}%effective", fraction * 100.0)
                        }
                        SizeSpec::StackFraction { fraction } => {
                            format!("{}%stack", fraction * 100.0)
                        }
                        SizeSpec::GeometricAllIn { streets } => {
                            format!("geometric(allin,streets={streets})")
                        }
                    })
                    .collect(),
            })
            .collect();
        self.game.tree = Tree::Standard { rules };
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

        let tree_rules = match &self.game.tree {
            Tree::Standard { rules } => lower_tree_rules(rules)?,
            Tree::Script { source, params } => {
                let base = base_dir.ok_or_else(|| {
                    anyhow!(
                        "script tree requires a config file path for relative source resolution"
                    )
                })?;
                let path = base.join(source);
                let program = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading mwtree source {}", path.display()))?;
                lower_tree_script(&program, params)?
            }
        };

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
        // No legacy 2/4 GiB CLI cap: the runtime's allocation checks remain
        // authoritative until platform available-memory probing is installed.
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
        Ok(SolveConfig {
            game: GameSection::PreflopMultiway(MultiwayConfig {
                seats,
                button: SeatId::new_unchecked(self.game.button),
                blinds: BlindConfig::default(),
                ante: AnteConfig::None,
                betting: BettingConfig {
                    rules: tree_rules,
                    ..BettingConfig::default()
                },
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
            if source.rollouts_per_state.is_some() || source.seed.is_some() || !overrides.is_empty()
            {
                bail!("ehs2-percentile forbids rollout samples, seed, and opponent overrides");
            }
            Ok(AbstractionConfig {
                flop_buckets: cast("flop buckets", buckets.flop)?,
                turn_buckets: cast("turn buckets", buckets.turn)?,
                river_buckets: cast("river buckets", buckets.river)?,
                rollout_samples: 512,
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
fn default_buckets() -> u32 {
    64
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

    const MINIMAL: &str = r#"
schema = "solvers.multiway-preflop/v1"

[game]
seat_count = 6
button = 0

[game.defaults]
stack_bb = 100.0
range = "random"
"#;

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
        assert_eq!(game.abstraction.flop_buckets, 64);
        assert_eq!(game.abstraction.rollout_samples, 512);
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
    fn normalized_effective_toml_is_reparsable() {
        let effective = normalized_toml(MINIMAL).unwrap();
        assert!(effective.contains("rollouts_per_state = 512"));
        assert!(effective.contains("probability_encoding = \"u16\""));
        parse_and_lower(&effective).unwrap();
    }

    #[test]
    fn solve_time_overrides_are_normalized_and_validated() {
        let effective =
            apply_solve_overrides(MINIMAL, Some(3), Some("12GiB"), Some("2h"), None).unwrap();
        assert_eq!(max_time_secs(&effective).unwrap(), Some(7_200));
        let value: toml::Value = toml::from_str(&effective).unwrap();
        assert_eq!(value["run"]["resources"]["threads"].as_integer(), Some(3));
        assert_eq!(value["run"]["resources"]["memory"].as_str(), Some("12GiB"));
        let resumed = apply_resume_overrides(
            MINIMAL,
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
    fn script_effective_config_is_self_contained() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("game.toml");
        let script_path = directory.path().join("tree.mwtree");
        std::fs::write(
            &script_path,
            "preflop when unopened { replace raise [2.2x, allin] }\n",
        )
        .unwrap();
        let raw = format!("{MINIMAL}\n[game.tree]\nkind = \"script\"\nsource = \"tree.mwtree\"\n");
        let effective = normalized_toml_at(&raw, &config_path).unwrap();
        assert!(effective.contains("kind = \"standard\""));
        assert!(effective.contains("2.2x"));
        assert!(!effective.contains("tree.mwtree"));
        std::fs::remove_file(script_path).unwrap();
        parse_and_lower(&effective).unwrap();
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
}
