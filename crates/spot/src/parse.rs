//! Strict typed TOML decoding, semantic checks, and lowering to the shared rules.
use crate::error::value_error;
use crate::*;
use economics::{FieldPlayerConfig, RakeAllocation, RakeConfig, UtilityConfig};
use nlh::script::Script;
use nlh::{
    ChipAmountError, ChipScale, MwChips, Rounding, SeatId, SeatVec, TableSetup, position_name,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use toml::Value;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawDocument {
    schema: String,
    meta: Meta,
    table: RawTable,
    economics: RawEconomics,
    spot: RawSpot,
    ranges: BTreeMap<String, String>,
    tree: RawTree,
    solver: toml::Table,
    run: RawRun,
    output: toml::Table,
    profile: Option<toml::Table>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTable {
    players: Option<i64>,
    bb_chips: Option<i64>,
    stack_bb: Option<f64>,
    stack_chips: Option<i64>,
    stacks_bb: Option<BTreeMap<String, f64>>,
    stacks_chips: Option<BTreeMap<String, i64>>,
    sb_bb: Option<f64>,
    sb_chips: Option<i64>,
    ante_bb: Option<f64>,
    ante_chips: Option<i64>,
    bb_ante_bb: Option<f64>,
    bb_ante_chips: Option<i64>,
    straddles_bb: Option<Vec<f64>>,
    straddles_chips: Option<Vec<i64>>,
    chip_rounding: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawEconomics {
    kind: Option<String>,
    rake: Option<RawRake>,
    payouts: Option<Vec<f64>>,
    outside_field_bb: Option<Vec<f64>>,
    outside_field_chips: Option<Vec<i64>>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRake {
    rate: Option<f64>,
    cap_bb: Option<f64>,
    cap_chips: Option<i64>,
    when: Option<String>,
    allocation: Option<String>,
    rounding: Option<String>,
    rounding_unit_bb: Option<f64>,
    rounding_unit_chips: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawSpot {
    line: Option<String>,
    board: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTree {
    script: Option<String>,
    source: Option<String>,
    params: BTreeMap<String, Value>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRun {
    threads: Option<Value>,
    memory: Option<Value>,
    max_time: Option<String>,
    checkpoint_interval: Option<String>,
    final_checkpoint: Option<bool>,
}

fn type_error(key: &str, expected: &str) -> SpotError {
    SpotError::new(Code::NLH002, key, format!("expected {expected}"))
}

/// Check shapes before serde so every unknown key/type error has its complete dotted path.
/// Open maps are restricted to the documented value kind; solver/output belong to hooks.
pub(crate) fn shape(value: &Value, path: &str, kind: &str) -> Result<(), SpotError> {
    if let Some(inner) = kind.strip_prefix("array:") {
        let array = value.as_array().ok_or_else(|| type_error(path, "array"))?;
        for (i, v) in array.iter().enumerate() {
            shape(v, &format!("{path}.{i}"), inner)?;
        }
        return Ok(());
    }
    let fields: &[(&str, &str)] = match kind {
        "document" => &[
            ("schema", "string"),
            ("meta", "meta"),
            ("table", "table"),
            ("economics", "economics"),
            ("spot", "spot"),
            ("ranges", "map:string"),
            ("tree", "tree"),
            ("solver", "raw"),
            ("run", "run"),
            ("output", "raw"),
            ("profile", "profile"),
        ],
        "meta" => &[
            ("name", "string"),
            ("description", "string"),
            ("derived_from", "derived"),
        ],
        "derived" => &[
            ("run_id", "string"),
            ("solution_hash", "string"),
            ("line", "string"),
            ("board", "string"),
        ],
        "table" => &[
            ("players", "integer"),
            ("bb_chips", "integer"),
            ("stack_bb", "number"),
            ("stack_chips", "integer"),
            ("stacks_bb", "map:number"),
            ("stacks_chips", "map:integer"),
            ("sb_bb", "number"),
            ("sb_chips", "integer"),
            ("ante_bb", "number"),
            ("ante_chips", "integer"),
            ("bb_ante_bb", "number"),
            ("bb_ante_chips", "integer"),
            ("straddles_bb", "array:number"),
            ("straddles_chips", "array:integer"),
            ("chip_rounding", "string"),
        ],
        "economics" => &[
            ("kind", "string"),
            ("rake", "rake"),
            ("payouts", "array:number"),
            ("outside_field_bb", "array:number"),
            ("outside_field_chips", "array:integer"),
        ],
        "rake" => &[
            ("rate", "number"),
            ("cap_bb", "number"),
            ("cap_chips", "integer"),
            ("when", "string"),
            ("allocation", "string"),
            ("rounding", "string"),
            ("rounding_unit_bb", "number"),
            ("rounding_unit_chips", "integer"),
        ],
        "spot" => &[("line", "string"), ("board", "string")],
        "tree" => &[
            ("script", "string"),
            ("source", "string"),
            ("params", "map:scalar"),
        ],
        "run" => &[
            ("threads", "auto_integer"),
            ("memory", "auto_integer"),
            ("max_time", "string"),
            ("checkpoint_interval", "string"),
            ("final_checkpoint", "bool"),
        ],
        "profile" => &[
            ("lock", "array:profile_lock"),
            ("incentive", "array:profile_incentive"),
            ("frequency", "array:profile_frequency"),
            ("freeze_upstream", "profile_freeze"),
        ],
        "profile_lock" => &[
            ("node", "string"),
            ("hands", "string"),
            ("category", "string"),
            ("strategy", "map:number"),
            ("combos", "map:map:number"),
            ("confidence", "number"),
        ],
        "profile_incentive" => &[
            ("node", "string"),
            ("hands", "string"),
            ("category", "string"),
            ("action", "string"),
            ("pot", "number"),
            ("bb", "number"),
        ],
        "profile_frequency" => &[
            ("node", "string"),
            ("hands", "string"),
            ("category", "string"),
            ("action", "string"),
            ("target", "number"),
        ],
        "profile_freeze" => &[("solution", "string"), ("sha256", "string")],
        primitive => {
            let valid = match primitive {
                "string" => value.is_str(),
                "integer" => value.is_integer(),
                "number" => value.is_integer() || value.is_float(),
                "bool" => value.is_bool(),
                "auto_integer" => value.is_str() || value.is_integer(),
                "scalar" => {
                    value.is_str() || value.is_integer() || value.is_float() || value.is_bool()
                }
                "raw" => value.is_table(),
                map if map.starts_with("map:") => {
                    let table = value.as_table().ok_or_else(|| type_error(path, "table"))?;
                    for (key, v) in table {
                        shape(v, &format!("{path}.{key}"), &map[4..])?;
                    }
                    true
                }
                _ => unreachable!(),
            };
            return if valid {
                Ok(())
            } else {
                Err(type_error(path, primitive))
            };
        }
    };
    let table = value.as_table().ok_or_else(|| type_error(path, "table"))?;
    for (key, v) in table {
        let full = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        let (_, kind) = fields
            .iter()
            .find(|(name, _)| *name == key)
            .ok_or_else(|| SpotError::new(Code::NLH002, &full, "unknown key"))?;
        shape(v, &full, kind)?;
    }
    Ok(())
}

/// Decode only the inputs needed by the interactive line builder.
pub(crate) fn line_input(text: &str, read_line: bool) -> Result<(Table, String), SpotError> {
    // A partially edited unrelated section must not disable the builder. Prefer
    // parsing the original document (including quoted/dotted keys); if its TOML
    // is incomplete, project the requested sections and parse those instead.
    let value = match text.parse::<Value>() {
        Ok(value) => value,
        Err(_) => {
            let projected = line_sections(text, read_line);
            projected.parse::<Value>().map_err(toml_error)?
        }
    };
    let raw_table = value
        .get("table")
        .ok_or_else(|| type_error("table", "required table"))?;
    shape(raw_table, "table", "table")?;
    let lexical: toml_edit::DocumentMut = match text.parse() {
        Ok(document) => document,
        Err(_) => line_sections(text, read_line)
            .parse()
            .map_err(|e: toml_edit::TomlError| {
                SpotError::new(Code::NLH002, "table", e.to_string())
            })?,
    };
    validate_bb_literals(&lexical["table"], "table")?;
    let raw: RawTable = raw_table.clone().try_into().map_err(toml_error)?;
    let line = if read_line {
        match value.get("spot").and_then(|spot| spot.get("line")) {
            Some(Value::String(line)) => line.clone(),
            Some(_) => return Err(type_error("spot.line", "string")),
            None => String::new(),
        }
    } else {
        String::new()
    };
    Ok((table(raw)?, line))
}

fn toml_error(error: toml::de::Error) -> SpotError {
    SpotError {
        code: Code::NLH002,
        key: None,
        message: error.to_string(),
    }
}

/// Recover table sections from a form containing incomplete TOML elsewhere.
/// Valid documents take the normal parser path, so multiline strings and all
/// TOML key spellings retain their usual meaning.
fn line_sections(text: &str, read_line: bool) -> String {
    let mut result = String::new();
    let mut in_table = false;
    let mut in_spot = false;
    let mut in_line = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            if let Ok(header) = trimmed.parse::<Value>() {
                in_table = header.get("table").is_some();
                in_spot = read_line && header.get("spot").is_some();
                in_line = false;
                if in_table || in_spot {
                    result.push_str(line);
                    result.push('\n');
                }
                continue;
            }
            if !in_table {
                in_spot = false;
                in_line = false;
            }
        }
        if in_spot && !in_line {
            // Only line is relevant; an unfinished board is deliberately ignored.
            let key = trimmed.split_once('=').map(|(key, _)| key.trim());
            in_line = matches!(key, Some("line" | "\"line\"" | "'line'"));
        }
        if in_table || in_line {
            result.push_str(line);
            result.push('\n');
            if in_line && result.parse::<Value>().is_ok() {
                in_line = false;
            }
        }
    }
    result
}

pub(crate) fn document(text: &str, config_path: &Path) -> Result<Document, SpotError> {
    document_with_mode(text, config_path, false)
}

pub(crate) fn document_with_mode(
    text: &str,
    config_path: &Path,
    embedded: bool,
) -> Result<Document, SpotError> {
    let value: Value = text.parse().map_err(|e: toml::de::Error| SpotError {
        code: Code::NLH002,
        key: None,
        message: e.to_string(),
    })?;
    match value.get("schema") {
        None => {
            return Err(SpotError::new(
                Code::NLH001,
                "schema",
                "required schema is solvers.nlh/v1; see docs/nlh-input-v1.jp.md",
            ));
        }
        Some(Value::String(s)) if s == "solvers.nlh/v1" => {}
        Some(Value::String(s)) => {
            return Err(SpotError::new(
                Code::NLH001,
                "schema",
                if matches!(
                    s.as_str(),
                    "solvers.postflop/v1"
                        | "solvers.multiway-preflop/v1"
                        | "solvers.toy/v1"
                        | "solvers.preflop-hu/v1"
                ) {
                    format!(
                        "format {s:?} was replaced by solvers.nlh/v1; old configs are not converted automatically; see docs/nlh-input-v1.jp.md"
                    )
                } else {
                    format!("unknown schema {s:?}; use solvers.nlh/v1; see docs/nlh-input-v1.jp.md")
                },
            ));
        }
        Some(_) => return Err(type_error("schema", "string")),
    }
    shape(&value, "", "document")?;
    // Preserve numeric spelling for grid checks: TOML's f64 decoder can otherwise
    // erase a sub-grid tail (e.g. 0.00100000000000000001) before validation.
    let lexical: toml_edit::DocumentMut =
        text.parse().map_err(|e: toml_edit::TomlError| SpotError {
            code: Code::NLH002,
            key: None,
            message: e.to_string(),
        })?;
    validate_bb_literals(lexical.as_item(), "")?;
    if value.get("table").is_none() {
        return Err(type_error("table", "required table"));
    }
    let raw: RawDocument = value.try_into().map_err(|e: toml::de::Error| SpotError {
        code: Code::NLH002,
        key: None,
        message: e.to_string(),
    })?;
    debug_assert_eq!(raw.schema, "solvers.nlh/v1");
    let table = table(raw.table)?;
    let economics = economics(raw.economics, &table)?;
    let line = raw.spot.line.unwrap_or_default();
    let board = crate::replay::board(raw.spot.board.as_deref())?;
    let (start, actions) = crate::replay::line(&table, &line)?;
    let line = crate::cursor::line_text(&actions, table.setup.scale());
    let product = crate::replay::product(&start, &line, &board)?;
    let context = crate::replay::context(&table, &start, board.clone(), actions)?;
    let mut ranges = raw.ranges;
    check_positions(ranges.keys(), &table.positions, "ranges")?;
    if product == Product::HuPostflop {
        for seat in table.positions.seats() {
            let name = &table.positions[seat];
            if start.non_folded_mask().contains(seat) {
                if !ranges.contains_key(name) {
                    return Err(value_error(
                        format!("ranges.{name}"),
                        "both remaining P1 players require a range",
                    ));
                }
            } else if ranges.contains_key(name) {
                return Err(value_error(
                    format!("ranges.{name}"),
                    "P1 ranges may name only the two remaining players",
                ));
            }
        }
    }
    let ranges = SeatVec::new_unchecked(
        table
            .positions
            .iter()
            .map(|name| {
                let text = ranges.remove(name).unwrap_or_else(|| "random".into());
                let mut range: nlh::Range = text.parse().map_err(|e: nlh::ParseRangeError| {
                    value_error(format!("ranges.{name}"), e.to_string())
                })?;
                if product == Product::HuPostflop {
                    for combo in 0..nlh::NUM_COMBOS {
                        let (a, b) = nlh::combo_cards(combo);
                        if board.contains(&a) || board.contains(&b) {
                            range.set_weight(combo, 0.0);
                        }
                    }
                }
                Ok(SeatRange { text, range })
            })
            .collect::<Result<Vec<_>, SpotError>>()?,
    );
    if let (Some(oop), Some(ip)) = (&context.oop, &context.ip) {
        let combos = |seat| {
            (0..nlh::NUM_COMBOS)
                .filter(|c| ranges[seat].range.weight(*c) > 0.0)
                .map(nlh::combo_cards)
                .collect::<Vec<_>>()
        };
        let oop_combos = combos(oop.seat);
        let ip_combos = combos(ip.seat);
        if !oop_combos.iter().any(|(a, b)| {
            ip_combos
                .iter()
                .any(|(c, d)| a != c && a != d && b != c && b != d)
        }) {
            return Err(value_error(
                "ranges",
                "no pair of non-overlapping, board-compatible combos exists",
            ));
        }
    }
    let tree = tree(raw.tree, config_path, product, table.setup.scale())?;
    let profile = match raw.profile {
        Some(_) if product != Product::HuPostflop => {
            return Err(SpotError::new(
                Code::NLH002,
                "profile",
                format!(
                    "this spot is solved by {}; this key belongs to the other product",
                    product.name()
                ),
            ));
        }
        Some(profile) => Some(crate::profile::parse(&profile, config_path, embedded)?),
        None => None,
    };
    let run = run(raw.run, product)?;
    Ok(Document {
        spot: Spot {
            meta: raw.meta,
            table,
            economics,
            start,
            line,
            board_text: raw.spot.board,
            context,
            ranges,
            tree,
            run,
            profile,
            product,
        },
        solver: raw.solver,
        output: raw.output,
    })
}

/// Exact value of a non-negative decimal spelling (TOML underscores, sign and
/// exponent allowed) as `mantissa × 10^exponent` without trailing zeros.
pub(crate) fn exact_decimal(text: &str) -> Option<(u128, i32)> {
    let cleaned = text.replace('_', "");
    let negative = cleaned.starts_with('-');
    let cleaned = cleaned.strip_prefix(['+', '-']).unwrap_or(&cleaned);
    let (mantissa, exponent) = match cleaned.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().ok()?),
        None => (cleaned, 0),
    };
    if !crate::dialect::plain_decimal(mantissa) {
        return None;
    }
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some((0, 0));
    }
    if negative {
        return None;
    }
    let significant = digits.trim_end_matches('0');
    let exponent = exponent
        .checked_sub(i32::try_from(fraction.len()).ok()?)?
        .checked_add(i32::try_from(digits.len() - significant.len()).ok()?)?;
    Some((significant.parse().ok()?, exponent))
}

/// Explain a BB amount that does not convert to whole chips.
pub(crate) fn amount_error(error: ChipAmountError, key: &str, scale: ChipScale) -> SpotError {
    match error {
        ChipAmountError::Fractional => {
            let hint = if key.contains("_bb") {
                format!("; write {} instead", key.replacen("_bb", "_chips", 1))
            } else {
                "; write the amount in chips with a c suffix".into()
            };
            value_error(
                key,
                format!(
                    "is not a whole number of chips (table.bb_chips = {}){hint}",
                    scale.chips_per_bb()
                ),
            )
        }
        _ => value_error(
            key,
            "must be a finite non-negative BB amount within chip capacity",
        ),
    }
}

fn nonzero(amount: MwChips, key: &str, zero: bool) -> Result<MwChips, SpotError> {
    if amount == MwChips::ZERO && !zero {
        Err(value_error(key, "must be positive"))
    } else {
        Ok(amount)
    }
}

/// Whole chips of a plain decimal BB spelling (line amounts, `Nbb` sizes).
pub(crate) fn decimal_amount(
    text: &str,
    key: &str,
    zero: bool,
    scale: ChipScale,
) -> Result<MwChips, SpotError> {
    if !crate::dialect::plain_decimal(text) {
        return Err(value_error(key, "must be a plain decimal BB amount"));
    }
    let amount = scale
        .parse_bb(text)
        .map_err(|e| amount_error(e, key, scale))?;
    nonzero(amount, key, zero)
}

/// Whole chips of a decoded BB key; [`validate_bb_literals`] already proved
/// the shortest `f64` spelling is the written decimal.
fn bb_amount(number: f64, key: &str, zero: bool, scale: ChipScale) -> Result<MwChips, SpotError> {
    let (mantissa, exponent) = exact_decimal(&number.to_string()).ok_or_else(|| {
        value_error(
            key,
            "must be a finite non-negative BB amount within chip capacity",
        )
    })?;
    let amount = scale
        .from_decimal(mantissa, exponent)
        .map_err(|e| amount_error(e, key, scale))?;
    nonzero(amount, key, zero)
}

fn chip_count(number: i64, key: &str, zero: bool) -> Result<MwChips, SpotError> {
    let amount = u64::try_from(number)
        .map(MwChips)
        .map_err(|_| value_error(key, "must not be negative"))?;
    nonzero(amount, key, zero)
}

/// One amount written either in BB (`{key}_bb`) or in chips (`{key}_chips`),
/// never both.
fn either(
    scale: ChipScale,
    bb: Option<f64>,
    chips: Option<i64>,
    key: &str,
    zero: bool,
) -> Result<Option<MwChips>, SpotError> {
    let bb_key = format!("{key}_bb");
    let chips_key = format!("{key}_chips");
    match (bb, chips) {
        (Some(_), Some(_)) => Err(value_error(
            &chips_key,
            format!("cannot combine {bb_key} and {chips_key}"),
        )),
        (Some(v), None) => bb_amount(v, &bb_key, zero, scale).map(Some),
        (None, Some(n)) => chip_count(n, &chips_key, zero).map(Some),
        (None, None) => Ok(None),
    }
}

/// A list written either in BB or in chips, with the key that was used.
fn either_list(
    scale: ChipScale,
    bb: Option<Vec<f64>>,
    chips: Option<Vec<i64>>,
    key: &str,
    zero: bool,
) -> Result<Option<(String, Vec<MwChips>)>, SpotError> {
    let bb_key = format!("{key}_bb");
    let chips_key = format!("{key}_chips");
    match (bb, chips) {
        (Some(_), Some(_)) => Err(value_error(
            &chips_key,
            format!("cannot combine {bb_key} and {chips_key}"),
        )),
        (Some(values), None) => {
            let amounts = values
                .iter()
                .enumerate()
                .map(|(i, v)| bb_amount(*v, &format!("{bb_key}.{i}"), zero, scale))
                .collect::<Result<_, _>>()?;
            Ok(Some((bb_key, amounts)))
        }
        (None, Some(values)) => {
            let amounts = values
                .iter()
                .enumerate()
                .map(|(i, n)| chip_count(*n, &format!("{chips_key}.{i}"), zero))
                .collect::<Result<_, _>>()?;
            Ok(Some((chips_key, amounts)))
        }
        (None, None) => Ok(None),
    }
}

pub(crate) fn validate_bb_literals(item: &toml_edit::Item, path: &str) -> Result<(), SpotError> {
    if let Some(table) = item.as_table_like() {
        for (key, value) in table.iter() {
            let child = if path.is_empty() {
                key.to_owned()
            } else {
                format!("{path}.{key}")
            };
            validate_bb_literals(value, &child)?;
        }
    } else if let Some(value) = item.as_value() {
        validate_bb_value(value, path)?;
    }
    Ok(())
}

/// Reject BB numbers whose decoded `f64` is not exactly the written decimal,
/// so a sub-chip tail cannot be erased before the chip conversion.
fn validate_bb_value(value: &toml_edit::Value, path: &str) -> Result<(), SpotError> {
    if let Some(array) = value.as_array() {
        for (i, value) in array.iter().enumerate() {
            validate_bb_value(value, &format!("{path}.{i}"))?;
        }
        return Ok(());
    }
    let is_bb = matches!(
        path,
        "table.stack_bb"
            | "table.sb_bb"
            | "table.ante_bb"
            | "table.bb_ante_bb"
            | "economics.rake.cap_bb"
            | "economics.rake.rounding_unit_bb"
    ) || path.starts_with("table.stacks_bb.")
        || path.starts_with("table.straddles_bb.")
        || path.starts_with("economics.outside_field_bb.");
    if !is_bb {
        return Ok(());
    }
    let (spelling, decoded) = match value {
        toml_edit::Value::Integer(i) => (i.value().to_string(), *i.value() as f64),
        toml_edit::Value::Float(f) => (f.display_repr().into_owned(), *f.value()),
        _ => unreachable!("numeric shapes were checked"),
    };
    let exact = exact_decimal(&spelling).ok_or_else(|| {
        value_error(
            path,
            "must be a finite non-negative BB amount within chip capacity",
        )
    })?;
    if exact_decimal(&decoded.to_string()) != Some(exact) {
        return Err(value_error(
            path,
            "BB amount cannot be represented without changing its value",
        ));
    }
    Ok(())
}

fn positive(number: i64, key: &str) -> Result<u64, SpotError> {
    if number <= 0 {
        return Err(value_error(key, "must be positive"));
    }
    Ok(number as u64)
}

fn check_positions<'a>(
    keys: impl Iterator<Item = &'a String>,
    positions: &SeatVec<String>,
    path: &str,
) -> Result<(), SpotError> {
    for key in keys {
        if !positions.iter().any(|p| p == key) {
            return Err(value_error(
                format!("{path}.{key}"),
                "unknown position for this table size",
            ));
        }
    }
    Ok(())
}

fn rounding(text: &str, key: &str) -> Result<Rounding, SpotError> {
    match text {
        "down" => Ok(Rounding::Down),
        "nearest" => Ok(Rounding::Nearest),
        "up" => Ok(Rounding::Up),
        _ => Err(value_error(key, "expected down, nearest, or up")),
    }
}

fn chip_scale(raw: &RawTable) -> Result<ChipScale, SpotError> {
    match raw.bb_chips {
        None => Ok(ChipScale::DEFAULT),
        Some(n) => u64::try_from(n)
            .ok()
            .and_then(ChipScale::new)
            .ok_or_else(|| {
                value_error(
                    "table.bb_chips",
                    format!("must be an integer from 1 to {}", ChipScale::MAX),
                )
            }),
    }
}

fn table(raw: RawTable) -> Result<Table, SpotError> {
    let players = raw
        .players
        .ok_or_else(|| type_error("table.players", "required integer"))?;
    if !(2..=9).contains(&players) {
        return Err(value_error("table.players", "must be 2 through 9"));
    }
    let players = players as usize;
    let positions = SeatVec::new_unchecked(
        (0..players)
            .map(|s| position_name(SeatId(s as u8), SeatId(0), players).to_owned())
            .collect(),
    );
    let scale = chip_scale(&raw)?;
    let bb = scale.bb();
    let chip_rounding = rounding(
        raw.chip_rounding.as_deref().unwrap_or("nearest"),
        "table.chip_rounding",
    )?;
    if raw.stacks_bb.is_some() && raw.stacks_chips.is_some() {
        return Err(value_error(
            "table.stacks_chips",
            "cannot combine table.stacks_bb and table.stacks_chips",
        ));
    }
    let stacks_bb = raw.stacks_bb.unwrap_or_default();
    let stacks_chips = raw.stacks_chips.unwrap_or_default();
    check_positions(stacks_bb.keys(), &positions, "table.stacks_bb")?;
    check_positions(stacks_chips.keys(), &positions, "table.stacks_chips")?;
    let default = either(scale, raw.stack_bb, raw.stack_chips, "table.stack", false)?;
    let stacks = SeatVec::new_unchecked(
        positions
            .iter()
            .map(|name| {
                if let Some(v) = stacks_bb.get(name) {
                    bb_amount(*v, &format!("table.stacks_bb.{name}"), false, scale)
                } else if let Some(n) = stacks_chips.get(name) {
                    chip_count(*n, &format!("table.stacks_chips.{name}"), false)
                } else {
                    default.ok_or_else(|| {
                        value_error(
                            format!("table.stacks_bb.{name}"),
                            "missing stack; specify stack_bb, stack_chips, or every position",
                        )
                    })
                }
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    // All betting/settlement sums must fit the chip representation.
    stacks
        .iter()
        .try_fold(0u64, |a, b| a.checked_add(b.0))
        .ok_or_else(|| value_error("table.stacks_bb", "total stacks exceed chip capacity"))?;
    let sb = match either(scale, raw.sb_bb, raw.sb_chips, "table.sb", false)? {
        Some(sb) => sb,
        None if bb.raw().is_multiple_of(2) => MwChips(bb.raw() / 2),
        None => {
            return Err(value_error(
                "table.sb_bb",
                format!(
                    "the default 0.5 BB is not a whole number of chips (table.bb_chips = {}); set table.sb_bb or table.sb_chips",
                    scale.chips_per_bb()
                ),
            ));
        }
    };
    if sb > bb {
        return Err(value_error("table.sb_bb", "must be at most 1 BB"));
    }
    let ante = either(scale, raw.ante_bb, raw.ante_chips, "table.ante", true)?.unwrap_or_default();
    let bb_ante = either(
        scale,
        raw.bb_ante_bb,
        raw.bb_ante_chips,
        "table.bb_ante",
        true,
    )?
    .unwrap_or_default();
    if ante.0 > 0 && bb_ante.0 > 0 {
        return Err(value_error(
            "table.bb_ante_bb",
            "cannot combine a positive ante and bb_ante",
        ));
    }
    let (straddle_key, posts) = either_list(
        scale,
        raw.straddles_bb,
        raw.straddles_chips,
        "table.straddles",
        false,
    )?
    .unwrap_or_else(|| ("table.straddles_bb".into(), Vec::new()));
    if posts.len() > players - 2 {
        return Err(value_error(
            &straddle_key,
            "at most players - 2 straddles are allowed; heads-up cannot straddle",
        ));
    }
    let first = SeatId(if players <= 3 { 0 } else { 3 });
    let mut straddles = Vec::new();
    let mut previous = bb;
    for (i, post) in posts.into_iter().enumerate() {
        let key = format!("{straddle_key}.{i}");
        if previous
            .0
            .checked_mul(2)
            .is_none_or(|minimum| post.0 < minimum)
        {
            return Err(value_error(
                &key,
                "must be at least twice the previous forced bet",
            ));
        }
        let seat = first.advance(i, players);
        // Straddlers are never SB/BB, so only individual antes precede their post.
        if stacks[seat].saturating_sub(ante) <= post {
            return Err(value_error(
                &key,
                "straddler must retain chips after the ante and straddle",
            ));
        }
        straddles.push((seat, post));
        previous = post;
    }
    let mut blinds = SeatVec::new_unchecked(vec![MwChips::ZERO; players]);
    blinds[SeatId(if players == 2 { 0 } else { 1 })] = sb;
    blinds[SeatId(if players == 2 { 1 } else { 2 })] = bb;
    let setup = TableSetup {
        button: SeatId(0),
        starting_stacks: stacks.clone(),
        forced_antes: SeatVec::new_unchecked(vec![ante; players]),
        common_ante: bb_ante,
        forced_blinds: blinds,
        preflop_first_to_act: first.advance(straddles.len(), players),
        straddles,
        nominal_big_blind: bb,
        chip_rounding,
    };
    Ok(Table {
        positions,
        stacks,
        sb,
        ante,
        bb_ante,
        setup,
    })
}

fn economics(raw: RawEconomics, table: &Table) -> Result<Economics, SpotError> {
    let players = table.stacks.len();
    let scale = table.setup.scale();
    let (rake, utility) = match raw.kind.as_deref().unwrap_or("cash") {
        "cash" => {
            for (name, set) in [
                ("payouts", raw.payouts.is_some()),
                ("outside_field_bb", raw.outside_field_bb.is_some()),
                ("outside_field_chips", raw.outside_field_chips.is_some()),
            ] {
                if set {
                    return Err(SpotError::new(
                        Code::NLH002,
                        format!("economics.{name}"),
                        "key applies only to tournament economics",
                    ));
                }
            }
            let rake = match raw.rake {
                None => RakeConfig::None,
                Some(r) => {
                    let rate = r
                        .rate
                        .ok_or_else(|| type_error("economics.rake.rate", "required number"))?;
                    if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
                        return Err(value_error(
                            "economics.rake.rate",
                            "must be finite and between 0 and 1",
                        ));
                    }
                    let cap_bb = either(scale, r.cap_bb, r.cap_chips, "economics.rake.cap", true)?
                        .map(|cap| scale.to_bb(cap));
                    let rounding_unit = either(
                        scale,
                        r.rounding_unit_bb,
                        r.rounding_unit_chips,
                        "economics.rake.rounding_unit",
                        false,
                    )?
                    .unwrap_or(MwChips(1));
                    let allocation = match r.allocation.as_deref().unwrap_or("main-first") {
                        "main-first" => RakeAllocation::MainFirst,
                        "proportional" => RakeAllocation::Proportional,
                        _ => {
                            return Err(value_error(
                                "economics.rake.allocation",
                                "expected main-first or proportional",
                            ));
                        }
                    };
                    let rounding = rounding(
                        r.rounding.as_deref().unwrap_or("down"),
                        "economics.rake.rounding",
                    )?;
                    let when = r.when.unwrap_or_else(|| "flop_dealt".into());
                    economics::rake_condition::compile(&when)
                        .map_err(|e| value_error("economics.rake.when", e))?;
                    RakeConfig::Generic {
                        rate,
                        cap_bb,
                        when,
                        allocation,
                        rounding,
                        rounding_unit,
                    }
                }
            };
            (rake, UtilityConfig::ChipEv)
        }
        "tournament" => {
            if raw.rake.is_some() {
                return Err(value_error(
                    "economics.rake",
                    "tournament economics cannot have rake",
                ));
            }
            let (field_key, outside) = either_list(
                scale,
                raw.outside_field_bb,
                raw.outside_field_chips,
                "economics.outside_field",
                false,
            )?
            .unwrap_or_else(|| ("economics.outside_field_bb".into(), Vec::new()));
            let field = players + outside.len();
            if field > economics::ICM_MAX_PLAYERS {
                return Err(value_error(&field_key, "field exceeds 10000 players"));
            }
            let outside_field = outside
                .into_iter()
                .enumerate()
                .map(|(i, stack)| FieldPlayerConfig {
                    name: format!("outside-{i}"),
                    stack_bb: scale.to_bb(stack),
                })
                .collect();
            let mut payouts = raw
                .payouts
                .ok_or_else(|| type_error("economics.payouts", "required array"))?;
            if payouts.len() > field {
                return Err(value_error(
                    "economics.payouts",
                    "more payouts than field players",
                ));
            }
            for (i, &v) in payouts.iter().enumerate() {
                if !v.is_finite() || v < 0.0 || (i > 0 && v > payouts[i - 1]) {
                    return Err(value_error(
                        format!("economics.payouts.{i}"),
                        "payouts must be finite, nonnegative, and nonincreasing",
                    ));
                }
            }
            payouts.resize(field, 0.0);
            let utility = UtilityConfig::TournamentIcm {
                outside_field,
                payouts,
            };
            utility
                .validate(players)
                .map_err(|e| value_error("economics.payouts", e.to_string()))?;
            (RakeConfig::None, utility)
        }
        _ => return Err(value_error("economics.kind", "expected cash or tournament")),
    };
    let compiled_rake = rake
        .compile(scale)
        .map_err(|e| value_error("economics.rake", e.to_string()))?;
    utility
        .validate(players)
        .map_err(|e| value_error("economics", e.to_string()))?;
    Ok(Economics {
        rake,
        compiled_rake,
        utility,
    })
}

fn tree(
    raw: RawTree,
    config_path: &Path,
    product: Product,
    scale: ChipScale,
) -> Result<Tree, SpotError> {
    if raw.script.is_some() && raw.source.is_some() {
        return Err(value_error(
            "tree.source",
            "script and source are mutually exclusive",
        ));
    }
    let script = match raw.source {
        Some(path) => std::fs::read_to_string(
            config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(path),
        )
        .map_err(|e| value_error("tree.source", format!("cannot read source: {e}")))?,
        None => raw.script.unwrap_or_default(),
    }
    .replace("\r\n", "\n");
    if script.contains("'''") {
        return Err(value_error(
            "tree.script",
            "script cannot contain the multiline literal delimiter '''",
        ));
    }
    if script
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(value_error(
            "tree.script",
            "script contains a control character that cannot be written in a TOML multiline literal",
        ));
    }
    let mut overrides = BTreeMap::new();
    for (key, v) in &raw.params {
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Integer(i) => i.to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Float(f) if f.is_finite() => f.to_string(),
            _ => {
                return Err(value_error(
                    format!("tree.params.{key}"),
                    "parameter must be finite",
                ));
            }
        };
        overrides.insert(key.clone(), text);
    }
    let compiled = Script::compile(&script, &overrides, &NLH_V1).map_err(|e| {
        if e.line == 0 {
            for key in overrides.keys() {
                if e.message == format!("override {key:?} does not name a declared param") {
                    return value_error(
                        format!("tree.params.{key}"),
                        "override does not name a declared param",
                    );
                }
            }
        }
        value_error("tree.script", e.to_string())
    })?;
    for size in compiled.rules.iter().flat_map(|rule| &rule.sizes) {
        if let nlh::SizeSpec::ToBb { value } = *size
            && let Err(error) = scale.parse_bb(&value.to_string())
        {
            let error = amount_error(error, "tree.script", scale);
            return Err(value_error(
                "tree.script",
                format!("size {} {}", size.render(nlh::SizeUnit::Bb), error.message),
            ));
        }
    }
    let preflop_dialect = nlh::script::Dialect {
        vars: crate::dialect::PREFLOP_VARS,
        ..NLH_V1
    };
    if product == Product::MultiwayPreflop {
        Script::compile(&script, &overrides, &preflop_dialect).map_err(|e| {
            value_error(
                "tree.script",
                format!("P2 (Multiway Preflop) cannot read board variables: {e}"),
            )
        })?;
    }
    Ok(Tree {
        script,
        compiled,
        params: raw.params,
    })
}

fn duration(text: &str, key: &str) -> Result<f64, SpotError> {
    let (number, multiplier) = [('s', 1.0), ('m', 60.0), ('h', 3600.0)]
        .into_iter()
        .find_map(|(suffix, m)| text.strip_suffix(suffix).map(|n| (n, m)))
        .ok_or_else(|| value_error(key, "expected positive duration with s, m, or h suffix"))?;
    if !crate::dialect::plain_decimal(number) {
        return Err(value_error(key, "expected positive decimal duration"));
    }
    let seconds = number.parse::<f64>().unwrap_or(f64::NAN) * multiplier;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(value_error(key, "duration must be finite and positive"));
    }
    Ok(seconds)
}

fn run(raw: RawRun, product: Product) -> Result<Run, SpotError> {
    if raw.final_checkpoint.is_some() && product != Product::HuPostflop {
        return Err(SpotError::new(
            Code::NLH002,
            "run.final_checkpoint",
            format!(
                "this spot is solved by {}; this key belongs to the other product",
                product.name()
            ),
        ));
    }
    let threads = match raw.threads {
        None => None,
        Some(Value::String(s)) if s == "auto" => None,
        Some(Value::Integer(i)) => Some(positive(i, "run.threads")?),
        _ => {
            return Err(value_error(
                "run.threads",
                "expected auto or positive integer",
            ));
        }
    };
    let memory_bytes = match raw.memory {
        None => None,
        Some(Value::String(s)) if s == "auto" => None,
        Some(Value::Integer(i)) => Some(positive(i, "run.memory")?),
        Some(Value::String(s)) => {
            let (number, scale) = [
                ("KiB", 1024u64),
                ("MiB", 1024 * 1024),
                ("GiB", 1024 * 1024 * 1024),
            ]
            .into_iter()
            .find_map(|(suffix, m)| s.strip_suffix(suffix).map(|n| (n, m)))
            .ok_or_else(|| {
                value_error(
                    "run.memory",
                    "expected integer with KiB, MiB, or GiB suffix",
                )
            })?;
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return Err(value_error(
                    "run.memory",
                    "expected positive integer memory size",
                ));
            }
            let bytes = number
                .parse::<u64>()
                .ok()
                .and_then(|n| n.checked_mul(scale))
                .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                .ok_or_else(|| {
                    value_error(
                        "run.memory",
                        "memory size must be positive and at most i64::MAX bytes",
                    )
                })?;
            Some(bytes)
        }
        _ => unreachable!("shape checked memory"),
    };
    Ok(Run {
        threads,
        memory_bytes,
        max_time_seconds: raw
            .max_time
            .map(|s| duration(&s, "run.max_time"))
            .transpose()?,
        checkpoint_interval_seconds: duration(
            raw.checkpoint_interval.as_deref().unwrap_or("15m"),
            "run.checkpoint_interval",
        )?,
        final_checkpoint: raw.final_checkpoint.unwrap_or(true),
    })
}
