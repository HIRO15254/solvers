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
fn strategy(t: &Table, path: &str, confidence: f64) -> Result<Vec<(String, f64)>, SpotError> {
    let mut result = Vec::new();
    let mut sum = 0.0;
    for (action, v) in t {
        if action.trim().is_empty() {
            return Err(value_error(path, "action must not be empty"));
        }
        let p = probability(v, &format!("{path}.{action}"))?;
        sum += p;
        result.push((action.clone(), p));
    }
    if sum > 1.0 + 1e-9 {
        return Err(value_error(
            path,
            "strategy probabilities must sum to at most 1",
        ));
    }
    if confidence < 1.0 && (sum - 1.0).abs() > 1e-9 {
        return Err(value_error(
            path,
            "confidence below 1 requires strategy probabilities to sum to 1",
        ));
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
        let node = string(row, "node", &path)?;
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
                confidence,
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
                            confidence,
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
            node: string(row, "node", &path)?,
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
            node: string(row, "node", &path)?,
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
fn map(actions: &[(String, f64)]) -> Value {
    Value::Table(
        actions
            .iter()
            .map(|(k, v)| (k.clone(), Value::Float(*v)))
            .collect(),
    )
}
fn hand(t: &mut Table, h: &HandSelector) {
    match h {
        HandSelector::All => (),
        HandSelector::Range(s) => {
            t.insert("hands".into(), Value::String(s.clone()));
        }
        HandSelector::Category(s) => {
            t.insert("category".into(), Value::String(s.clone()));
        }
    }
}
pub(crate) fn normalize(p: &ProfileIr) -> String {
    let mut root = Table::new();
    if let Some(f) = &p.freeze_upstream {
        let mut t = Table::new();
        t.insert("solution".into(), Value::String(f.solution.clone()));
        if let Some(s) = &f.sha256 {
            t.insert("sha256".into(), Value::String(s.clone()));
        }
        root.insert("freeze_upstream".into(), Value::Table(t));
    }
    let locks = p
        .locks
        .iter()
        .map(|l| {
            let mut t = Table::new();
            t.insert("node".into(), Value::String(l.node.clone()));
            hand(&mut t, &l.hands);
            t.insert("confidence".into(), Value::Float(l.confidence));
            match &l.strategy {
                LockStrategy::Uniform(s) => {
                    t.insert("strategy".into(), map(s));
                }
                LockStrategy::PerCombo(c) => {
                    t.insert(
                        "combos".into(),
                        Value::Table(c.iter().map(|(k, v)| (k.clone(), map(v))).collect()),
                    );
                }
            }
            Value::Table(t)
        })
        .collect::<Vec<_>>();
    if !locks.is_empty() {
        root.insert("lock".into(), Value::Array(locks));
    }
    let incentives = p
        .incentives
        .iter()
        .map(|v| {
            let mut t = Table::new();
            t.insert("node".into(), Value::String(v.node.clone()));
            hand(&mut t, &v.hands);
            t.insert("action".into(), Value::String(v.action.clone()));
            let (k, n) = match v.amount {
                IncentiveAmount::PotFraction(n) => ("pot", n),
                IncentiveAmount::Bb(n) => ("bb", n),
            };
            t.insert(k.into(), Value::Float(n));
            Value::Table(t)
        })
        .collect::<Vec<_>>();
    if !incentives.is_empty() {
        root.insert("incentive".into(), Value::Array(incentives));
    }
    let frequencies = p
        .frequencies
        .iter()
        .map(|v| {
            let mut t = Table::new();
            t.insert("node".into(), Value::String(v.node.clone()));
            hand(&mut t, &v.hands);
            t.insert("action".into(), Value::String(v.action.clone()));
            t.insert("target".into(), Value::Float(v.target));
            Value::Table(t)
        })
        .collect::<Vec<_>>();
    if !frequencies.is_empty() {
        root.insert("frequency".into(), Value::Array(frequencies));
    }
    let mut wrapper = Table::new();
    wrapper.insert("profile".into(), Value::Table(root));
    toml::to_string(&wrapper).expect("profile contains valid TOML")
}
