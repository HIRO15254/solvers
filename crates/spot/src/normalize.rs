//! Stable section ordering and self-contained effective TOML.
use crate::*;
use economics::{RakeConfig, UtilityConfig};
use nlh::MwChips;
use toml::Value;

fn bb(chips: MwChips) -> Value {
    if chips.0.is_multiple_of(1000) {
        Value::Integer((chips.0 / 1000) as i64)
    } else {
        Value::Float(chips.as_bb())
    }
}

fn numeric(n: f64) -> Value {
    if n.is_finite() && n.fract() == 0.0 && n >= i64::MIN as f64 && n < i64::MAX as f64 {
        Value::Integer(n as i64)
    } else {
        Value::Float(n)
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => toml_edit::Value::from(s.as_str()).to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => toml_edit::Value::from(*f).to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Datetime(d) => d.to_string(),
        Value::Array(a) => format!("[{}]", a.iter().map(scalar).collect::<Vec<_>>().join(", ")),
        Value::Table(t) => format!(
            "{{ {} }}",
            t.iter()
                .map(|(k, v)| format!("{} = {}", toml_edit::Key::new(k).display_repr(), scalar(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn write_table(out: &mut String, path: &str, fields: &toml::Table) {
    out.push_str(&format!("\n[{path}]\n"));
    for (key, v) in fields {
        if v.is_table() && !(path == "tree" && key == "preflop_reraise_jam_above_stack") {
            continue;
        }
        let key = toml_edit::Key::new(key).display_repr().into_owned();
        if path == "tree" && key == "script" {
            // The newline immediately after ''' is stripped by TOML, preserving script bytes.
            out.push_str("script = '''\n");
            out.push_str(v.as_str().unwrap());
            out.push_str("'''\n");
        } else {
            out.push_str(&format!("{key} = {}\n", scalar(v)));
        }
    }
    for (key, v) in fields {
        if let Value::Table(t) = v {
            if path == "tree" && key == "preflop_reraise_jam_above_stack" {
                continue;
            }
            let name = toml_edit::Key::new(key).display_repr().into_owned();
            write_table(out, &format!("{path}.{name}"), t);
        }
    }
}

fn fields(items: impl IntoIterator<Item = (&'static str, Value)>) -> toml::Table {
    items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

pub(crate) fn document(doc: &Document, hook: &impl ProductSections) -> Result<String, SpotError> {
    let spot = &doc.spot;
    let (solver, output) = hook.normalize(spot, &doc.solver, &doc.output)?;
    let mut text = "schema = \"solvers.nlh/v1\"\n".to_owned();
    let meta = Value::try_from(&spot.meta)
        .map_err(|e| SpotError::new(Code::NLH002, "meta", e.to_string()))?;
    write_table(&mut text, "meta", meta.as_table().unwrap());
    let t = &spot.table;
    // A canonical representation uses complete per-seat stacks; no redundant fallback.
    let stacks = t
        .positions
        .iter()
        .zip(t.stacks.iter())
        .map(|(p, v)| (p.clone(), bb(*v)))
        .collect();
    write_table(
        &mut text,
        "table",
        &fields([
            ("players", Value::Integer(t.stacks.len() as i64)),
            ("sb_bb", bb(t.sb)),
            ("ante_bb", bb(t.ante)),
            ("bb_ante_bb", bb(t.bb_ante)),
            (
                "straddles_bb",
                Value::Array(t.setup.straddles.iter().map(|(_, v)| bb(*v)).collect()),
            ),
            ("stacks_bb", Value::Table(stacks)),
        ]),
    );
    let mut economics = toml::Table::new();
    match &spot.economics.utility {
        UtilityConfig::ChipEv => {
            economics.insert("kind".into(), Value::String("cash".into()));
        }
        UtilityConfig::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => {
            economics = fields([
                ("kind", Value::String("tournament".into())),
                (
                    "payouts",
                    Value::Array(payouts.iter().map(|v| numeric(*v)).collect()),
                ),
                (
                    "outside_field_bb",
                    Value::Array(outside_field.iter().map(|v| numeric(v.stack_bb)).collect()),
                ),
            ]);
            if t.stacks.len() + outside_field.len() > economics::EXACT_ICM_MAX_PLAYERS {
                economics.insert("samples".into(), Value::Integer(*samples as i64));
                economics.insert("seed".into(), Value::Integer(*seed as i64));
            }
        }
    }
    if let RakeConfig::Generic {
        rate,
        cap_bb,
        when,
        allocation,
        rounding,
    } = &spot.economics.rake
    {
        let mut rake = fields([
            ("rate", numeric(*rate)),
            ("when", Value::String(when.clone())),
            ("allocation", Value::try_from(allocation).unwrap()),
            ("rounding", Value::try_from(rounding).unwrap()),
            ("rounding_unit_bb", Value::Float(0.001)),
        ]);
        if let Some(cap) = cap_bb {
            rake.insert("cap_bb".into(), numeric(*cap));
        }
        economics.insert("rake".into(), Value::Table(rake));
    }
    write_table(&mut text, "economics", &economics);
    write_table(
        &mut text,
        "spot",
        &fields([("line", Value::String(String::new()))]),
    );
    write_table(
        &mut text,
        "ranges",
        &t.positions
            .iter()
            .zip(spot.ranges.iter())
            .map(|(p, r)| (p.clone(), Value::String(r.text.clone())))
            .collect(),
    );
    let tree = &spot.tree;
    let mut tree_fields = fields([
        ("script", Value::String(tree.script.clone())),
        ("include_allin", Value::Boolean(tree.include_allin)),
        (
            "max_aggressive_actions",
            Value::try_from(&tree.max_aggressive_actions).unwrap(),
        ),
        (
            "params",
            Value::Table(tree.params.clone().into_iter().collect()),
        ),
    ]);
    if let Some(threshold) = tree.allin_threshold {
        tree_fields.insert("allin_threshold".into(), numeric(threshold));
    }
    if let Some(ratio) = &tree.preflop_reraise_jam_above_stack {
        // This setting is documented as an inline table, unlike the two tree sub-sections.
        tree_fields.insert(
            "preflop_reraise_jam_above_stack".into(),
            Value::try_from(ratio).unwrap(),
        );
    }
    write_table(&mut text, "tree", &tree_fields);
    write_table(&mut text, "solver", &solver);
    let run = &spot.run;
    let mut run_fields = fields([
        (
            "threads",
            run.threads
                .map(|n| Value::Integer(n as i64))
                .unwrap_or_else(|| Value::String("auto".into())),
        ),
        (
            "memory",
            match run.memory_bytes {
                Some(n) if n <= i64::MAX as u64 => Value::Integer(n as i64),
                Some(n) => Value::String(format!("{}KiB", n / 1024)),
                None => Value::String("auto".into()),
            },
        ),
        (
            "checkpoint_interval",
            Value::String(format!("{}s", run.checkpoint_interval_seconds)),
        ),
    ]);
    if let Some(seconds) = run.max_time_seconds {
        run_fields.insert("max_time".into(), Value::String(format!("{seconds}s")));
    }
    write_table(&mut text, "run", &run_fields);
    write_table(&mut text, "output", &output);
    Ok(text)
}
