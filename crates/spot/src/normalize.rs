//! Documentation-ordered, self-contained effective TOML without global preserve_order.
use crate::*;
use economics::{RakeConfig, UtilityConfig};
use nlh::{MwChips, SeatId};
use toml::Value;
use toml_edit::{InlineTable, Item, Table};

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

fn scalar(v: Value) -> toml_edit::Value {
    match v {
        Value::String(s) => s.into(),
        Value::Integer(i) => i.into(),
        Value::Float(f) => f.into(),
        Value::Boolean(b) => b.into(),
        Value::Datetime(d) => d.to_string().parse().expect("TOML datetime"),
        Value::Array(a) => toml_edit::Value::Array(a.into_iter().map(scalar).collect()),
        Value::Table(t) => {
            toml_edit::Value::InlineTable(t.into_iter().map(|(k, v)| (k, scalar(v))).collect())
        }
    }
}

fn insert(table: &mut Table, key: &str, v: Value) {
    table.insert(key, Item::Value(scalar(v)));
}

fn fields(items: impl IntoIterator<Item = (&'static str, Value)>) -> Table {
    let mut table = Table::new();
    for (k, v) in items {
        insert(&mut table, k, v);
    }
    table
}

fn inline(table: Table) -> toml_edit::Value {
    let mut t = InlineTable::new();
    for (k, v) in table {
        t.insert(&k, v.into_value().expect("inline scalar fields"));
    }
    t.fmt();
    toml_edit::Value::InlineTable(t)
}

fn write_table(out: &mut String, path: &str, fields: &Table) {
    if fields.is_empty() {
        return;
    }
    out.push_str(&format!("\n[{path}]\n"));
    for (key, item) in fields {
        if let Some(v) = item.as_value() {
            let name = toml_edit::Key::new(key).display_repr().into_owned();
            if path == "tree" && key == "script" {
                out.push_str("script = '''\n");
                out.push_str(v.as_str().expect("script string"));
                out.push_str("'''\n");
            } else {
                out.push_str(&format!("{name} = {v}\n"));
            }
        }
    }
    for (key, item) in fields {
        let name = toml_edit::Key::new(key).display_repr().into_owned();
        if let Some(t) = item.as_table() {
            write_table(out, &format!("{path}.{name}"), t);
        }
    }
}

fn write_product(out: &mut String, path: &str, fields: Table) {
    if !fields.is_empty() {
        let mut doc = toml_edit::DocumentMut::new();
        doc.insert(path, Item::Table(fields));
        out.push('\n');
        out.push_str(&doc.to_string());
    }
}

/// Documentation order is preflop without straddles, independent of posting order.
pub(crate) fn positions(table: &crate::Table) -> Vec<SeatId> {
    let n = table.stacks.len();
    let first = if n <= 3 { 0 } else { 3 };
    (0..n).map(|i| SeatId(first).advance(i, n)).collect()
}

fn duration(seconds: f64) -> String {
    for (scale, suffix) in [(3600.0, "h"), (60.0, "m"), (1.0, "s")] {
        if (seconds / scale).fract() == 0.0 {
            return format!("{}{suffix}", seconds / scale);
        }
    }
    format!("{seconds}s")
}

fn memory(bytes: Option<u64>) -> Value {
    let Some(n) = bytes else {
        return Value::String("auto".into());
    };
    for (scale, suffix) in [
        (1024u64.pow(3), "GiB"),
        (1024u64.pow(2), "MiB"),
        (1024, "KiB"),
    ] {
        if n.is_multiple_of(scale) {
            return Value::String(format!("{}{suffix}", n / scale));
        }
    }
    Value::Integer(n as i64)
}

pub(crate) fn document(doc: &Document, hook: &impl ProductSections) -> Result<String, SpotError> {
    let spot = &doc.spot;
    let (solver, output) = hook.normalize(spot, &doc.solver, &doc.output)?;
    let mut text = "schema = \"solvers.nlh/v1\"\n".to_owned();
    let mut meta = Table::new();
    for (key, value) in [
        ("name", &spot.meta.name),
        ("description", &spot.meta.description),
    ] {
        if let Some(value) = value {
            insert(&mut meta, key, Value::String(value.clone()));
        }
    }
    if let Some(derived) = &spot.meta.derived_from {
        let mut table = Table::new();
        for (key, value) in [
            ("run_id", &derived.run_id),
            ("solution_hash", &derived.solution_hash),
            ("line", &derived.line),
            ("board", &derived.board),
        ] {
            if let Some(value) = value {
                insert(&mut table, key, Value::String(value.clone()));
            }
        }
        meta.insert("derived_from", Item::Value(inline(table)));
    }
    write_table(&mut text, "meta", &meta);
    let t = &spot.table;
    let mut table = fields([("players", Value::Integer(t.stacks.len() as i64))]);
    let same_stack = t.stacks.iter().all(|s| *s == t.stacks[SeatId(0)]);
    if same_stack {
        insert(&mut table, "stack_bb", bb(t.stacks[SeatId(0)]));
    }
    for (key, v) in [
        ("sb_bb", t.sb),
        ("ante_bb", t.ante),
        ("bb_ante_bb", t.bb_ante),
    ] {
        insert(&mut table, key, bb(v));
    }
    insert(
        &mut table,
        "straddles_bb",
        Value::Array(t.setup.straddles.iter().map(|(_, v)| bb(*v)).collect()),
    );
    if !same_stack {
        let mut stacks = Table::new();
        for seat in positions(t) {
            insert(&mut stacks, &t.positions[seat], bb(t.stacks[seat]));
        }
        table.insert("stacks_bb", Item::Table(stacks));
    }
    write_table(&mut text, "table", &table);
    let mut economics = Table::new();
    match &spot.economics.utility {
        UtilityConfig::ChipEv => insert(&mut economics, "kind", Value::String("cash".into())),
        UtilityConfig::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => {
            insert(&mut economics, "kind", Value::String("tournament".into()));
            insert(
                &mut economics,
                "payouts",
                Value::Array(payouts.iter().map(|v| numeric(*v)).collect()),
            );
            insert(
                &mut economics,
                "outside_field_bb",
                Value::Array(outside_field.iter().map(|v| numeric(v.stack_bb)).collect()),
            );
            if t.stacks.len() + outside_field.len() > economics::EXACT_ICM_MAX_PLAYERS {
                insert(&mut economics, "samples", Value::Integer(*samples as i64));
                insert(&mut economics, "seed", Value::Integer(*seed as i64));
            }
        }
    }
    if let RakeConfig::Generic {
        rate,
        cap_bb,
        when,
        allocation,
        rounding,
        rounding_unit,
    } = &spot.economics.rake
    {
        let mut rake = fields([("rate", numeric(*rate))]);
        if let Some(cap) = cap_bb {
            insert(&mut rake, "cap_bb", numeric(*cap));
        }
        for (key, v) in [
            ("when", Value::String(when.clone())),
            ("allocation", Value::try_from(allocation).unwrap()),
            ("rounding", Value::try_from(rounding).unwrap()),
            ("rounding_unit_bb", numeric(rounding_unit.as_bb())),
        ] {
            insert(&mut rake, key, v);
        }
        economics.insert("rake", Item::Table(rake));
    }
    write_table(&mut text, "economics", &economics);
    let mut start = fields([("line", Value::String(spot.line.clone()))]);
    if let Some(board) = &spot.board_text {
        insert(&mut start, "board", Value::String(board.clone()));
    }
    write_table(&mut text, "spot", &start);
    let mut ranges = Table::new();
    for seat in positions(t) {
        if spot.product == Product::MultiwayPreflop || spot.start.non_folded_mask().contains(seat) {
            insert(
                &mut ranges,
                &t.positions[seat],
                Value::String(spot.ranges[seat].text.clone()),
            );
        }
    }
    write_table(&mut text, "ranges", &ranges);
    let tree = &spot.tree;
    let mut tree_fields = fields([
        ("script", Value::String(tree.script.clone())),
        ("include_allin", Value::Boolean(tree.include_allin)),
    ]);
    if let Some(threshold) = tree.allin_threshold {
        insert(&mut tree_fields, "allin_threshold", numeric(threshold));
    }
    if let Some(r) = &tree.preflop_reraise_jam_above_stack {
        tree_fields.insert(
            "preflop_reraise_jam_above_stack",
            Item::Value(inline(fields([
                ("numerator", Value::Integer(r.numerator as i64)),
                ("denominator", Value::Integer(r.denominator as i64)),
            ]))),
        );
    }
    let l = &tree.max_aggressive_actions;
    tree_fields.insert(
        "max_aggressive_actions",
        Item::Table(fields([
            ("preflop", Value::Integer(l.preflop as i64)),
            ("flop", Value::Integer(l.flop as i64)),
            ("turn", Value::Integer(l.turn as i64)),
            ("river", Value::Integer(l.river as i64)),
        ])),
    );
    let mut params = Table::new();
    for p in &tree.compiled.params {
        if let Some(v) = tree.params.get(&p.name) {
            insert(&mut params, &p.name, v.clone());
        }
    }
    if !params.is_empty() {
        tree_fields.insert("params", Item::Table(params));
    }
    write_table(&mut text, "tree", &tree_fields);
    write_product(&mut text, "solver", solver);
    let run = &spot.run;
    let mut run_fields = fields([
        (
            "threads",
            run.threads
                .map(|v| Value::Integer(v as i64))
                .unwrap_or_else(|| Value::String("auto".into())),
        ),
        ("memory", memory(run.memory_bytes)),
    ]);
    if let Some(seconds) = run.max_time_seconds {
        insert(
            &mut run_fields,
            "max_time",
            Value::String(duration(seconds)),
        );
    }
    insert(
        &mut run_fields,
        "checkpoint_interval",
        Value::String(duration(run.checkpoint_interval_seconds)),
    );
    write_table(&mut text, "run", &run_fields);
    write_product(&mut text, "output", output);
    Ok(text)
}
