//! Documentation-ordered, self-contained effective TOML without global preserve_order.
use crate::*;
use economics::{RakeConfig, UtilityConfig};
use nlh::{ChipScale, MwChips, SeatId};
use toml::Value;
use toml_edit::{InlineTable, Item, Table};

/// The exact BB value of `chips`, when a TOML number spells it exactly.
fn bb(scale: ChipScale, chips: MwChips) -> Option<Value> {
    let text = scale.bb_text(chips)?;
    if !text.contains('.') {
        return text.parse().ok().map(Value::Integer);
    }
    let value: f64 = text.parse().ok()?;
    (crate::parse::exact_decimal(&value.to_string()) == crate::parse::exact_decimal(&text))
        .then_some(Value::Float(value))
}

/// Spell a group of amounts as `{base}_bb` on decimal tables when every value
/// is an exact BB number, otherwise as `{base}_chips` integers.
fn amounts(scale: ChipScale, base: &str, chips: &[MwChips]) -> (String, Vec<Value>) {
    let exact = scale
        .is_decimal()
        .then(|| {
            chips
                .iter()
                .map(|c| bb(scale, *c))
                .collect::<Option<Vec<_>>>()
        })
        .flatten();
    match exact {
        Some(values) => (format!("{base}_bb"), values),
        None => (
            format!("{base}_chips"),
            chips.iter().map(|c| Value::Integer(c.0 as i64)).collect(),
        ),
    }
}

fn insert_amount(table: &mut Table, scale: ChipScale, base: &str, chips: MwChips) {
    let (key, mut values) = amounts(scale, base, &[chips]);
    insert(table, &key, values.pop().unwrap());
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
    let scale = t.setup.scale();
    let mut table = fields([
        ("players", Value::Integer(t.stacks.len() as i64)),
        ("bb_chips", Value::Integer(scale.chips_per_bb() as i64)),
    ]);
    let same_stack = t.stacks.iter().all(|s| *s == t.stacks[SeatId(0)]);
    if same_stack {
        insert_amount(&mut table, scale, "stack", t.stacks[SeatId(0)]);
    }
    for (base, v) in [("sb", t.sb), ("ante", t.ante), ("bb_ante", t.bb_ante)] {
        insert_amount(&mut table, scale, base, v);
    }
    let straddles: Vec<MwChips> = t.setup.straddles.iter().map(|(_, v)| *v).collect();
    let (key, values) = amounts(scale, "straddles", &straddles);
    insert(&mut table, &key, Value::Array(values));
    insert(
        &mut table,
        "chip_rounding",
        Value::try_from(t.setup.chip_rounding).unwrap(),
    );
    if !same_stack {
        let seats = positions(t);
        let chips: Vec<MwChips> = seats.iter().map(|seat| t.stacks[*seat]).collect();
        let (key, values) = amounts(scale, "stacks", &chips);
        let mut stacks = Table::new();
        for (seat, value) in seats.into_iter().zip(values) {
            insert(&mut stacks, &t.positions[seat], value);
        }
        table.insert(&key, Item::Table(stacks));
    }
    write_table(&mut text, "table", &table);
    let mut economics = Table::new();
    match &spot.economics.utility {
        UtilityConfig::ChipEv => insert(&mut economics, "kind", Value::String("cash".into())),
        UtilityConfig::TournamentIcm {
            outside_field,
            payouts,
        } => {
            insert(&mut economics, "kind", Value::String("tournament".into()));
            insert(
                &mut economics,
                "payouts",
                Value::Array(payouts.iter().map(|v| numeric(*v)).collect()),
            );
            let stacks: Vec<MwChips> = outside_field
                .iter()
                .map(|v| scale.from_bb(v.stack_bb).expect("parsed from whole chips"))
                .collect();
            let (key, values) = amounts(scale, "outside_field", &stacks);
            insert(&mut economics, &key, Value::Array(values));
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
            let cap = scale.from_bb(*cap).expect("parsed from whole chips");
            insert_amount(&mut rake, scale, "cap", cap);
        }
        for (key, v) in [
            ("when", Value::String(when.clone())),
            ("allocation", Value::try_from(allocation).unwrap()),
            ("rounding", Value::try_from(rounding).unwrap()),
        ] {
            insert(&mut rake, key, v);
        }
        insert_amount(&mut rake, scale, "rounding_unit", *rounding_unit);
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
    let mut tree_fields = fields([("script", Value::String(tree.script.clone()))]);
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
    if spot.product == Product::HuPostflop {
        insert(
            &mut run_fields,
            "final_checkpoint",
            Value::Boolean(run.final_checkpoint),
        );
    }
    write_table(&mut text, "run", &run_fields);
    write_product(&mut text, "output", output);
    if let Some(profile) = &spot.profile {
        text.push('\n');
        text.push_str(&crate::profile::normalize(profile));
    }
    Ok(text)
}
