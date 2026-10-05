//! Strict typed TOML decoding, semantic checks, and lowering to the shared rules.
use crate::error::value_error;
use crate::*;
use economics::{FieldPlayerConfig, RakeAllocation, RakeConfig, RakeRounding, UtilityConfig};
use nlh::script::Script;
use nlh::{MwChips, SeatId, SeatVec, TableSetup, position_name};
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
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTable {
    players: Option<i64>,
    stack_bb: Option<f64>,
    stacks_bb: BTreeMap<String, f64>,
    sb_bb: Option<f64>,
    ante_bb: Option<f64>,
    bb_ante_bb: Option<f64>,
    straddles_bb: Vec<f64>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawEconomics {
    kind: Option<String>,
    rake: Option<RawRake>,
    payouts: Option<Vec<f64>>,
    outside_field_bb: Option<Vec<f64>>,
    samples: Option<i64>,
    seed: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRake {
    rate: Option<f64>,
    cap_bb: Option<f64>,
    when: Option<String>,
    allocation: Option<String>,
    rounding: Option<String>,
    rounding_unit_bb: Option<f64>,
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
    include_allin: bool,
    allin_threshold: Option<f64>,
    preflop_reraise_jam_above_stack: Option<RawRatio>,
    max_aggressive_actions: RawLimits,
    params: BTreeMap<String, Value>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRatio {
    numerator: Option<i64>,
    denominator: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawLimits {
    preflop: Option<i64>,
    flop: Option<i64>,
    turn: Option<i64>,
    river: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawRun {
    threads: Option<Value>,
    memory: Option<Value>,
    max_time: Option<String>,
    checkpoint_interval: Option<String>,
}

fn type_error(key: &str, expected: &str) -> SpotError {
    SpotError::new(Code::NLH002, key, format!("expected {expected}"))
}

/// Check shapes before serde so every unknown key/type error has its complete dotted path.
/// Open maps are restricted to the documented value kind; solver/output belong to hooks.
fn shape(value: &Value, path: &str, kind: &str) -> Result<(), SpotError> {
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
            ("stack_bb", "number"),
            ("stacks_bb", "map:number"),
            ("sb_bb", "number"),
            ("ante_bb", "number"),
            ("bb_ante_bb", "number"),
            ("straddles_bb", "array:number"),
        ],
        "economics" => &[
            ("kind", "string"),
            ("rake", "rake"),
            ("payouts", "array:number"),
            ("outside_field_bb", "array:number"),
            ("samples", "integer"),
            ("seed", "integer"),
        ],
        "rake" => &[
            ("rate", "number"),
            ("cap_bb", "number"),
            ("when", "string"),
            ("allocation", "string"),
            ("rounding", "string"),
            ("rounding_unit_bb", "number"),
        ],
        "spot" => &[("line", "string"), ("board", "string")],
        "tree" => &[
            ("script", "string"),
            ("source", "string"),
            ("include_allin", "bool"),
            ("allin_threshold", "number"),
            ("preflop_reraise_jam_above_stack", "ratio"),
            ("max_aggressive_actions", "limits"),
            ("params", "map:scalar"),
        ],
        "ratio" => &[("numerator", "integer"), ("denominator", "integer")],
        "limits" => &[
            ("preflop", "integer"),
            ("flop", "integer"),
            ("turn", "integer"),
            ("river", "integer"),
        ],
        "run" => &[
            ("threads", "auto_integer"),
            ("memory", "auto_integer"),
            ("max_time", "string"),
            ("checkpoint_interval", "string"),
        ],
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

pub(crate) fn document(text: &str, config_path: &Path) -> Result<Document, SpotError> {
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
                "required schema is solvers.nlh/v1",
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
                        "format {s:?} was replaced by solvers.nlh/v1; old configs are not converted automatically"
                    )
                } else {
                    format!("unknown schema {s:?}; use solvers.nlh/v1")
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
    let economics = economics(raw.economics, table.stacks.len())?;
    let line = raw.spot.line.unwrap_or_default();
    let board = crate::replay::board(raw.spot.board.as_deref())?;
    let (start, actions) = crate::replay::line(&table, &line)?;
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
    let tree = tree(raw.tree, config_path, product)?;
    let run = run(raw.run)?;
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
            product,
        },
        solver: raw.solver,
        output: raw.output,
    })
}

/// Decimal-to-grid conversion checks digits rather than rounding floating-point money.
pub(crate) fn decimal_chips(text: &str, key: &str, zero: bool) -> Result<MwChips, SpotError> {
    let invalid = || {
        value_error(
            key,
            "must be a finite BB amount on the 0.001 grid within chip capacity",
        )
    };
    if !crate::dialect::plain_decimal(text) {
        return Err(invalid());
    }
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if fraction.bytes().skip(3).any(|b| b != b'0') {
        return Err(invalid());
    }
    let whole: u64 = whole.parse().map_err(|_| invalid())?;
    let fraction = format!("{fraction:0<3}");
    let fraction: u64 = fraction[..3].parse().map_err(|_| invalid())?;
    let amount = whole
        .checked_mul(1000)
        .and_then(|v| v.checked_add(fraction))
        .ok_or_else(invalid)?;
    if amount == 0 && !zero {
        return Err(value_error(key, "must be positive"));
    }
    Ok(MwChips(amount))
}

fn chips(number: f64, key: &str, zero: bool) -> Result<MwChips, SpotError> {
    if number == 0.0 && zero {
        return Ok(MwChips::ZERO);
    }
    decimal_chips(&number.to_string(), key, zero)
}

fn validate_bb_literals(item: &toml_edit::Item, path: &str) -> Result<(), SpotError> {
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
    let exact = toml_number_chips(&spelling, path)?;
    if chips(decoded, path, true)? != exact {
        return Err(value_error(
            path,
            "BB amount cannot be represented without changing its chip value",
        ));
    }
    Ok(())
}

/// Exact TOML decimal/exponent grid check, including underscores and an optional sign.
fn toml_number_chips(text: &str, key: &str) -> Result<MwChips, SpotError> {
    let invalid = || {
        value_error(
            key,
            "must be a finite BB amount on the 0.001 grid within chip capacity",
        )
    };
    let cleaned = text.replace('_', "");
    let negative = cleaned.starts_with('-');
    let cleaned = cleaned.strip_prefix(['+', '-']).unwrap_or(&cleaned);
    let (mantissa, exponent) = match cleaned.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i64>().map_err(|_| invalid())?),
        None => (cleaned, 0),
    };
    if !crate::dialect::plain_decimal(mantissa) {
        return Err(invalid());
    }
    let decimals = mantissa.split_once('.').map_or(0, |(_, f)| f.len());
    let digits = mantissa.replace('.', "");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(MwChips::ZERO);
    }
    if negative {
        return Err(invalid());
    }
    let shift = exponent
        .checked_add(3)
        .and_then(|e| e.checked_sub(decimals as i64))
        .ok_or_else(invalid)?;
    let integer = if shift < 0 {
        let discarded = shift.unsigned_abs();
        if discarded > digits.len() as u64 {
            return Err(invalid());
        }
        let cut = digits.len() - discarded as usize;
        if !digits[cut..].bytes().all(|b| b == b'0') {
            return Err(invalid());
        }
        digits[..cut].to_owned()
    } else {
        if shift > 20 || digits.len() + shift as usize > 20 {
            return Err(invalid());
        }
        format!("{digits}{}", "0".repeat(shift as usize))
    };
    Ok(MwChips(integer.parse().map_err(|_| invalid())?))
}

fn unsigned(number: i64, key: &str, zero: bool) -> Result<u64, SpotError> {
    if number < 0 || (number == 0 && !zero) {
        return Err(value_error(
            key,
            if zero {
                "must be nonnegative"
            } else {
                "must be positive"
            },
        ));
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
    check_positions(raw.stacks_bb.keys(), &positions, "table.stacks_bb")?;
    let default = raw
        .stack_bb
        .map(|v| chips(v, "table.stack_bb", false))
        .transpose()?;
    let stacks = SeatVec::new_unchecked(
        positions
            .iter()
            .map(|name| match raw.stacks_bb.get(name) {
                Some(v) => chips(*v, &format!("table.stacks_bb.{name}"), false),
                None => default.ok_or_else(|| {
                    value_error(
                        format!("table.stacks_bb.{name}"),
                        "missing stack; specify stack_bb or every position",
                    )
                }),
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    // All betting/settlement sums must fit the chip representation.
    stacks
        .iter()
        .try_fold(0u64, |a, b| a.checked_add(b.0))
        .ok_or_else(|| value_error("table.stacks_bb", "total stacks exceed chip capacity"))?;
    let sb = chips(raw.sb_bb.unwrap_or(0.5), "table.sb_bb", false)?;
    if sb > MwChips::ONE_BB {
        return Err(value_error("table.sb_bb", "must be at most 1 BB"));
    }
    let ante = chips(raw.ante_bb.unwrap_or(0.0), "table.ante_bb", true)?;
    let bb_ante = chips(raw.bb_ante_bb.unwrap_or(0.0), "table.bb_ante_bb", true)?;
    if ante.0 > 0 && bb_ante.0 > 0 {
        return Err(value_error(
            "table.bb_ante_bb",
            "cannot combine positive ante_bb and bb_ante_bb",
        ));
    }
    if raw.straddles_bb.len() > players - 2 {
        return Err(value_error(
            "table.straddles_bb",
            "at most players - 2 straddles are allowed; heads-up cannot straddle",
        ));
    }
    let first = SeatId(if players <= 3 { 0 } else { 3 });
    let mut straddles = Vec::new();
    let mut previous = MwChips::ONE_BB;
    for (i, v) in raw.straddles_bb.iter().enumerate() {
        let key = format!("table.straddles_bb.{i}");
        let amount = chips(*v, &key, false)?;
        if previous
            .0
            .checked_mul(2)
            .is_none_or(|minimum| amount.0 < minimum)
        {
            return Err(value_error(
                &key,
                "must be at least twice the previous forced bet",
            ));
        }
        let seat = first.advance(i, players);
        // Straddlers are never SB/BB, so only individual antes precede their post.
        if stacks[seat].saturating_sub(ante) <= amount {
            return Err(value_error(
                &key,
                "straddler must retain chips after the ante and straddle",
            ));
        }
        straddles.push((seat, amount));
        previous = amount;
    }
    let mut blinds = SeatVec::new_unchecked(vec![MwChips::ZERO; players]);
    blinds[SeatId(if players == 2 { 0 } else { 1 })] = sb;
    blinds[SeatId(if players == 2 { 1 } else { 2 })] = MwChips::ONE_BB;
    let setup = TableSetup {
        button: SeatId(0),
        starting_stacks: stacks.clone(),
        forced_antes: SeatVec::new_unchecked(vec![ante; players]),
        common_ante: bb_ante,
        forced_blinds: blinds,
        preflop_first_to_act: first.advance(straddles.len(), players),
        straddles,
        nominal_big_blind: MwChips::ONE_BB,
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

fn economics(raw: RawEconomics, players: usize) -> Result<Economics, SpotError> {
    let (rake, utility) = match raw.kind.as_deref().unwrap_or("cash") {
        "cash" => {
            for (name, set) in [
                ("payouts", raw.payouts.is_some()),
                ("outside_field_bb", raw.outside_field_bb.is_some()),
                ("samples", raw.samples.is_some()),
                ("seed", raw.seed.is_some()),
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
                    let cap_bb = r
                        .cap_bb
                        .map(|v| chips(v, "economics.rake.cap_bb", true).map(MwChips::as_bb))
                        .transpose()?;
                    if r.rounding_unit_bb.unwrap_or(0.001) != 0.001 {
                        return Err(value_error(
                            "economics.rake.rounding_unit_bb",
                            "must equal 0.001",
                        ));
                    }
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
                    let rounding = match r.rounding.as_deref().unwrap_or("down") {
                        "down" => RakeRounding::Down,
                        "nearest" => RakeRounding::Nearest,
                        "up" => RakeRounding::Up,
                        _ => {
                            return Err(value_error(
                                "economics.rake.rounding",
                                "expected down, nearest, or up",
                            ));
                        }
                    };
                    let when = r.when.unwrap_or_else(|| "flop_dealt".into());
                    economics::rake_condition::compile(&when)
                        .map_err(|e| value_error("economics.rake.when", e))?;
                    RakeConfig::Generic {
                        rate,
                        cap_bb,
                        when,
                        allocation,
                        rounding,
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
            let outside = raw.outside_field_bb.unwrap_or_default();
            let field = players + outside.len();
            if field > economics::ICM_MAX_PLAYERS {
                return Err(value_error(
                    "economics.outside_field_bb",
                    "field exceeds 10000 players",
                ));
            }
            if field <= economics::EXACT_ICM_MAX_PLAYERS {
                if raw.samples.is_some() {
                    return Err(value_error(
                        "economics.samples",
                        "samples apply only to fields of at least 16 players",
                    ));
                }
                if raw.seed.is_some() {
                    return Err(value_error(
                        "economics.seed",
                        "seed applies only to fields of at least 16 players",
                    ));
                }
            }
            let outside_field = outside
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    Ok(FieldPlayerConfig {
                        name: format!("outside-{i}"),
                        stack_bb: chips(v, &format!("economics.outside_field_bb.{i}"), false)?
                            .as_bb(),
                    })
                })
                .collect::<Result<Vec<_>, SpotError>>()?;
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
            let samples = unsigned(raw.samples.unwrap_or(100_000), "economics.samples", false)?;
            if field > economics::EXACT_ICM_MAX_PLAYERS && samples < 2 {
                return Err(value_error(
                    "economics.samples",
                    "sampled ICM requires at least two samples",
                ));
            }
            let seed = unsigned(raw.seed.unwrap_or(0), "economics.seed", true)?;
            let utility = UtilityConfig::TournamentIcm {
                outside_field,
                payouts,
                samples,
                seed,
            };
            utility
                .validate(players)
                .map_err(|e| value_error("economics.payouts", e.to_string()))?;
            (RakeConfig::None, utility)
        }
        _ => return Err(value_error("economics.kind", "expected cash or tournament")),
    };
    let compiled_rake = rake
        .compile()
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

fn tree(raw: RawTree, config_path: &Path, product: Product) -> Result<Tree, SpotError> {
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
    if raw
        .allin_threshold
        .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 1.0)
    {
        return Err(value_error(
            "tree.allin_threshold",
            "must satisfy 0 < threshold <= 1",
        ));
    }
    let ratio = raw
        .preflop_reraise_jam_above_stack
        .map(|r| {
            let numerator = r.numerator.ok_or_else(|| {
                type_error(
                    "tree.preflop_reraise_jam_above_stack.numerator",
                    "required integer",
                )
            })?;
            let denominator = r.denominator.ok_or_else(|| {
                type_error(
                    "tree.preflop_reraise_jam_above_stack.denominator",
                    "required integer",
                )
            })?;
            Ok(Ratio {
                numerator: unsigned(
                    numerator,
                    "tree.preflop_reraise_jam_above_stack.numerator",
                    false,
                )?,
                denominator: unsigned(
                    denominator,
                    "tree.preflop_reraise_jam_above_stack.denominator",
                    false,
                )?,
            })
        })
        .transpose()?;
    let l = raw.max_aggressive_actions;
    let max_aggressive_actions = MaxAggressiveActions {
        preflop: unsigned(
            l.preflop.unwrap_or(4),
            "tree.max_aggressive_actions.preflop",
            true,
        )?,
        flop: unsigned(
            l.flop.unwrap_or(3),
            "tree.max_aggressive_actions.flop",
            true,
        )?,
        turn: unsigned(
            l.turn.unwrap_or(3),
            "tree.max_aggressive_actions.turn",
            true,
        )?,
        river: unsigned(
            l.river.unwrap_or(3),
            "tree.max_aggressive_actions.river",
            true,
        )?,
    };
    Ok(Tree {
        script,
        compiled,
        include_allin: raw.include_allin,
        allin_threshold: raw.allin_threshold,
        preflop_reraise_jam_above_stack: ratio,
        max_aggressive_actions,
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

fn run(raw: RawRun) -> Result<Run, SpotError> {
    let threads = match raw.threads {
        None => None,
        Some(Value::String(s)) if s == "auto" => None,
        Some(Value::Integer(i)) => Some(unsigned(i, "run.threads", false)?),
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
        Some(Value::Integer(i)) => Some(unsigned(i, "run.memory", false)?),
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
    })
}
