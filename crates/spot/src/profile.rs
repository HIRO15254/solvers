use crate::{error::value_error, *};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use toml::{Table, Value};

fn string(t: &Table, key: &str, path: &str) -> Result<String, SpotError> {
    let s = t
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| value_error(format!("{path}.{key}"), "required string"))?;
    if s.trim().is_empty() {
        return Err(value_error(format!("{path}.{key}"), "must not be empty"));
    }
    Ok(s.to_owned())
}
fn node(t: &Table, path: &str) -> Result<String, SpotError> {
    let value = t
        .get("node")
        .and_then(Value::as_str)
        .ok_or_else(|| value_error(format!("{path}.node"), "required string"))?;
    if !value.is_empty() && value.trim().is_empty() {
        return Err(value_error(format!("{path}.node"), "must not be empty"));
    }
    Ok(value.to_owned())
}
fn number(v: &Value) -> f64 {
    v.as_float()
        .unwrap_or_else(|| v.as_integer().unwrap() as f64)
}
fn selector(t: &Table, path: &str) -> Result<HandSelector, SpotError> {
    match (t.get("hands"), t.get("category")) {
        (Some(_), Some(_)) => Err(value_error(
            path,
            "hands and category are mutually exclusive",
        )),
        (Some(v), None) => {
            let s = v.as_str().unwrap();
            if s.split(',')
                .filter_map(|entry| entry.split_once(':'))
                .any(|(_, weight)| weight.trim().parse::<f64>().ok() != Some(1.0))
            {
                return Err(value_error(
                    format!("{path}.hands"),
                    "range weights must be exactly 1",
                ));
            }
            let range: nlh::Range = s.parse().map_err(|e: nlh::ParseRangeError| {
                value_error(format!("{path}.hands"), e.to_string())
            })?;
            if (0..nlh::NUM_COMBOS).any(|i| {
                let w = range.weight(i);
                w != 0.0 && w != 1.0
            }) {
                return Err(value_error(
                    format!("{path}.hands"),
                    "range weights must be exactly 1",
                ));
            }
            Ok(HandSelector::Range(s.to_owned()))
        }
        (None, Some(v)) => Ok(HandSelector::Category(v.as_str().unwrap().to_owned())),
        _ => Ok(HandSelector::All),
    }
}
fn probability(v: &Value, path: &str) -> Result<f64, SpotError> {
    let n = number(v);
    if !n.is_finite() || !(0.0..=1.0).contains(&n) {
        return Err(value_error(path, "must be finite and in [0,1]"));
    }
    Ok(n)
}
fn strategy(t: &Table, path: &str) -> Result<Vec<(String, f64)>, SpotError> {
    let mut result = Vec::new();
    for (action, v) in t {
        if action.trim().is_empty() {
            return Err(value_error(path, "action must not be empty"));
        }
        let p = probability(v, &format!("{path}.{action}"))?;
        result.push((action.clone(), p));
    }
    Ok(result)
}
fn entries<'a>(t: &'a Table, key: &str) -> impl Iterator<Item = &'a Table> {
    t.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| v.as_table().unwrap())
}
pub(crate) fn parse(t: &Table, config_path: &Path, embedded: bool) -> Result<ProfileIr, SpotError> {
    let mut profile = ProfileIr {
        locks: vec![],
        incentives: vec![],
        frequencies: vec![],
        freeze_upstream: None,
    };
    for (i, row) in entries(t, "lock").enumerate() {
        let path = format!("profile.lock.{i}");
        let node = node(row, &path)?;
        let confidence = match row.get("confidence") {
            Some(v) => probability(v, &format!("{path}.confidence"))?,
            None => 1.0,
        };
        if confidence == 0.0 {
            return Err(value_error(
                format!("{path}.confidence"),
                "must be in (0,1]",
            ));
        }
        let hands = selector(row, &path)?;
        let strategy = match (row.get("strategy"), row.get("combos")) {
            (Some(v), None) => LockStrategy::Uniform(strategy(
                v.as_table().unwrap(),
                &format!("{path}.strategy"),
            )?),
            (None, Some(v)) => {
                if hands != HandSelector::All {
                    return Err(value_error(
                        &path,
                        "combos is mutually exclusive with hands and category",
                    ));
                }
                let mut combos = Vec::new();
                for (combo, actions) in v.as_table().unwrap() {
                    let valid = combo.len() == 4
                        && combo.is_ascii()
                        && combo[..2]
                            .parse::<nlh::Card>()
                            .ok()
                            .zip(combo[2..].parse::<nlh::Card>().ok())
                            .is_some_and(|(a, b)| a != b);
                    if !valid {
                        return Err(value_error(
                            format!("{path}.combos.{combo}"),
                            "expected two distinct cards",
                        ));
                    }
                    combos.push((
                        combo.clone(),
                        strategy(
                            actions.as_table().unwrap(),
                            &format!("{path}.combos.{combo}"),
                        )?,
                    ));
                }
                LockStrategy::PerCombo(combos)
            }
            _ => {
                return Err(value_error(
                    &path,
                    "exactly one of strategy and combos is required",
                ));
            }
        };
        profile.locks.push(LockIr {
            node,
            hands,
            strategy,
            confidence,
        });
    }
    for (i, row) in entries(t, "incentive").enumerate() {
        let path = format!("profile.incentive.{i}");
        let amount = match (row.get("pot"), row.get("bb")) {
            (Some(v), None) => IncentiveAmount::PotFraction(number(v)),
            (None, Some(v)) => IncentiveAmount::Bb(number(v)),
            _ => return Err(value_error(&path, "exactly one of pot and bb is required")),
        };
        let n = match amount {
            IncentiveAmount::PotFraction(n) | IncentiveAmount::Bb(n) => n,
        };
        if !n.is_finite() {
            return Err(value_error(&path, "amount must be finite"));
        }
        profile.incentives.push(IncentiveIr {
            node: node(row, &path)?,
            hands: selector(row, &path)?,
            action: string(row, "action", &path)?,
            amount,
        });
    }
    for (i, row) in entries(t, "frequency").enumerate() {
        let path = format!("profile.frequency.{i}");
        let target = probability(
            row.get("target")
                .ok_or_else(|| value_error(format!("{path}.target"), "required number"))?,
            &format!("{path}.target"),
        )?;
        profile.frequencies.push(FrequencyIr {
            node: node(row, &path)?,
            hands: selector(row, &path)?,
            action: string(row, "action", &path)?,
            target,
        });
    }
    if let Some(freeze) = t.get("freeze_upstream") {
        let freeze = freeze.as_table().unwrap();
        let solution = string(freeze, "solution", "profile.freeze_upstream")?;
        if embedded {
            profile.freeze_upstream = Some(FreezeIr {
                solution,
                sha256: freeze
                    .get("sha256")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase),
            });
            return Ok(profile);
        }
        let path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&solution);
        let path = std::fs::canonicalize(&path).map_err(|e| {
            value_error(
                "profile.freeze_upstream.solution",
                format!("cannot read solution: {e}"),
            )
        })?;
        let mut file = std::fs::File::open(&path).map_err(|e| {
            value_error(
                "profile.freeze_upstream.solution",
                format!("cannot read solution: {e}"),
            )
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|e| {
                value_error(
                    "profile.freeze_upstream.solution",
                    format!("cannot read solution: {e}"),
                )
            })?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        let hash = format!("{:x}", hasher.finalize());
        if freeze
            .get("sha256")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.eq_ignore_ascii_case(&hash))
        {
            return Err(value_error(
                "profile.freeze_upstream.sha256",
                "solution hash does not match",
            ));
        }
        profile.freeze_upstream = Some(FreezeIr {
            solution: path.to_string_lossy().into_owned(),
            sha256: Some(hash),
        });
    }
    Ok(profile)
}
fn map(actions: &[(String, f64)]) -> toml_edit::InlineTable {
    let mut table = toml_edit::InlineTable::new();
    for (action, probability) in actions {
        table.insert(action, toml_edit::Value::from(*probability));
    }
    table.fmt();
    table
}

fn hand(t: &mut toml_edit::Table, h: &HandSelector) {
    match h {
        HandSelector::All => (),
        HandSelector::Range(s) => {
            t.insert("hands", toml_edit::value(s.as_str()));
        }
        HandSelector::Category(s) => {
            t.insert("category", toml_edit::value(s.as_str()));
        }
    }
}

pub(crate) fn normalize(p: &ProfileIr) -> String {
    use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value};

    let mut root = Table::new();
    let mut locks = ArrayOfTables::new();
    for lock in &p.locks {
        let mut t = Table::new();
        t.insert("node", toml_edit::value(lock.node.as_str()));
        hand(&mut t, &lock.hands);
        match &lock.strategy {
            LockStrategy::Uniform(strategy) => {
                t.insert("strategy", Item::Value(Value::InlineTable(map(strategy))));
            }
            LockStrategy::PerCombo(combos) => {
                let mut values = toml_edit::InlineTable::new();
                for (combo, strategy) in combos {
                    values.insert(combo, Value::InlineTable(map(strategy)));
                }
                values.fmt();
                t.insert("combos", Item::Value(Value::InlineTable(values)));
            }
        }
        t.insert("confidence", toml_edit::value(lock.confidence));
        locks.push(t);
    }
    if !locks.is_empty() {
        root.insert("lock", Item::ArrayOfTables(locks));
    }
    let mut incentives = ArrayOfTables::new();
    for incentive in &p.incentives {
        let mut t = Table::new();
        t.insert("node", toml_edit::value(incentive.node.as_str()));
        hand(&mut t, &incentive.hands);
        t.insert("action", toml_edit::value(incentive.action.as_str()));
        let (key, amount) = match incentive.amount {
            IncentiveAmount::PotFraction(n) => ("pot", n),
            IncentiveAmount::Bb(n) => ("bb", n),
        };
        t.insert(key, toml_edit::value(amount));
        incentives.push(t);
    }
    if !incentives.is_empty() {
        root.insert("incentive", Item::ArrayOfTables(incentives));
    }
    let mut frequencies = ArrayOfTables::new();
    for frequency in &p.frequencies {
        let mut t = Table::new();
        t.insert("node", toml_edit::value(frequency.node.as_str()));
        hand(&mut t, &frequency.hands);
        t.insert("action", toml_edit::value(frequency.action.as_str()));
        t.insert("target", toml_edit::value(frequency.target));
        frequencies.push(t);
    }
    if !frequencies.is_empty() {
        root.insert("frequency", Item::ArrayOfTables(frequencies));
    }
    if let Some(freeze) = &p.freeze_upstream {
        let mut t = Table::new();
        t.insert("solution", toml_edit::value(freeze.solution.as_str()));
        if let Some(hash) = &freeze.sha256 {
            t.insert("sha256", toml_edit::value(hash.as_str()));
        }
        root.insert("freeze_upstream", Item::Table(t));
    }
    let mut document = DocumentMut::new();
    document.insert("profile", Item::Table(root));
    document.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_node_selects_root_for_all_profile_entries() {
        let table: Table = r#"
            [[lock]]
            node = ""
            strategy = { check = 1 }
            [[incentive]]
            node = ""
            action = "check"
            bb = 1
            [[frequency]]
            node = ""
            action = "check"
            target = 0.5
        "#
        .parse()
        .unwrap();
        let profile = parse(&table, Path::new("config.toml"), false).unwrap();
        assert_eq!(profile.locks[0].node, "");
        assert_eq!(profile.incentives[0].node, "");
        assert_eq!(profile.frequencies[0].node, "");
    }
}
