//! Product-neutral replay and document assembly for the P2 → P1 bridge.
use crate::*;
use nlh::{Street, script::Effect};
use std::collections::BTreeMap;
use std::path::Path;
use toml::Value;

/// Replay a preflop-only line, requiring a live heads-up flop start.
pub fn replay(source: &Document, line: &str, board: &str) -> Result<StartState, SpotError> {
    if line.contains('/') {
        return Err(SpotError::new(
            Code::NLH004,
            "spot.line",
            "derive requires a preflop-only line; postflop actions are unsupported",
        ));
    }
    let cards = crate::replay::board(Some(board))?;
    let (state, actions) = crate::replay::line(&source.spot.table, line)?;
    if state.street != Street::Flop || cards.len() != 3 {
        return Err(SpotError::new(
            Code::NLH004,
            "spot.line",
            "derive requires closed preflop and a three-card flop board",
        ));
    }
    crate::replay::product(&state, line, &cards)?;
    crate::replay::context(&source.spot.table, &state, cards, actions)
}

struct CommonSections;
impl ProductSections for CommonSections {
    fn normalize(
        &self,
        _: &Spot,
        _: &toml::Table,
        _: &toml::Table,
    ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError> {
        Ok((toml_edit::Table::new(), toml_edit::Table::new()))
    }
}

fn value(text: &str) -> Result<Value, SpotError> {
    text.parse()
        .map_err(|e: toml::de::Error| SpotError::new(Code::NLH002, "derive", e.to_string()))
}
fn serialize(v: &Value) -> String {
    toml::to_string(v).expect("TOML document")
}

/// Assemble common input; the caller must validate/normalize it with P1 settings.
/// Base paths resolve relative to the base file, including tree.source.
pub fn assemble(
    source: &str,
    base: Option<(&str, &Path)>,
    line: &str,
    board: &str,
    ranges: &BTreeMap<String, String>,
    provenance: DerivedFrom,
) -> Result<(Document, Vec<String>), SpotError> {
    let source_doc = Document::parse(source, Path::new("embedded.toml"))?;
    replay(&source_doc, line, board)?;
    let mut result = value(&source_doc.normalize(&CommonSections)?)?;
    for key in ["meta", "run", "solver", "output"] {
        result.as_table_mut().expect("document").remove(key);
    }
    let mut warnings = Vec::new();
    let mut path = Path::new("derived.toml");
    let mut inherited = true;
    if let Some((raw, base_path)) = base {
        path = base_path;
        let base = value(raw)?;
        if base.get("schema").and_then(Value::as_str) != Some("solvers.nlh/v1") {
            return Err(SpotError::new(
                Code::NLH001,
                "schema",
                "derive base requires schema solvers.nlh/v1",
            ));
        }
        crate::parse::shape(&base, "", "document")?;
        let lexical: toml_edit::DocumentMut = raw.parse().map_err(|e: toml_edit::TomlError| {
            SpotError::new(Code::NLH002, "derive.base", e.to_string())
        })?;
        crate::parse::validate_bb_literals(lexical.as_item(), "")?;
        for key in ["table", "economics"] {
            if let Some(section) = base.get(key) {
                let mut probe = value(source)?;
                probe
                    .as_table_mut()
                    .expect("document")
                    .insert(key.into(), section.clone());
                let normalized =
                    Document::parse(&serialize(&probe), path)?.normalize(&CommonSections)?;
                let expected = value(&source_doc.normalize(&CommonSections)?)?;
                if value(&normalized)?.get(key) != expected.get(key) {
                    return Err(SpotError::new(
                        Code::NLH003,
                        key,
                        format!("base [{key}] does not match the P2 solution after normalization"),
                    ));
                }
            }
        }
        for key in ["spot", "ranges"] {
            if base.get(key).is_some() {
                warnings.push(format!("base [{key}] replaced with derive values"));
            }
        }
        for key in ["tree", "solver", "output", "run", "meta"] {
            if let Some(section) = base.get(key) {
                result
                    .as_table_mut()
                    .expect("document")
                    .insert(key.into(), section.clone());
                if key == "tree" {
                    inherited = false;
                }
            }
        }
    }
    let meta = result
        .as_table_mut()
        .expect("document")
        .entry("meta")
        .or_insert_with(|| Value::Table(toml::Table::new()));
    meta.as_table_mut().expect("meta").insert(
        "derived_from".into(),
        Value::try_from(provenance).expect("provenance"),
    );
    result.as_table_mut().expect("document").insert(
        "spot".into(),
        Value::Table(toml::Table::from_iter([
            ("line".into(), Value::String(line.into())),
            ("board".into(), Value::String(board.into())),
        ])),
    );
    result
        .as_table_mut()
        .expect("document")
        .insert("ranges".into(), Value::try_from(ranges).expect("ranges"));
    let document = Document::parse(&serialize(&result), path)?;
    if inherited
        && document
            .spot
            .tree
            .compiled
            .rules
            .iter()
            .any(|r| r.street != Street::Preflop && r.effect == Effect::Checkdown)
    {
        warnings.push(
            "inherited postflop checkdown rules: matching P1 actors can only check or fold".into(),
        );
    }
    Ok((document, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack_bb = 10\n[tree]\nscript = 'flop, turn, river { checkdown }'\n";
    fn ranges() -> BTreeMap<String, String> {
        BTreeMap::from([("BTN".into(), "AA".into()), ("BB".into(), "KK".into())])
    }
    #[test]
    fn base_table_equality_uses_normalized_values_and_tree_source_is_relative() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("small.tree"),
            "flop when unopened { force bet [a] }",
        )
        .unwrap();
        let raw = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\n[table.stacks_bb]\nBTN = 10.0\nSB = 10\nBB = 10\n[economics]\nkind = 'cash'\n[tree]\nsource = 'small.tree'\n[meta]\nname = 'kept'\n[run]\nthreads = 2\n";
        let (doc, warnings) = assemble(
            SOURCE,
            Some((raw, &dir.path().join("base.toml"))),
            "BTN c, BB x",
            "Ks 7h 2d",
            &ranges(),
            DerivedFrom::default(),
        )
        .unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.spot.run.threads, Some(2));
        assert_eq!(doc.spot.meta.name.as_deref(), Some("kept"));
        assert!(doc.spot.tree.script.contains("force bet [a]"));
    }
    #[test]
    fn base_shape_schema_and_subgrid_amounts_remain_strict() {
        for raw in [
            "[solver]\nseed = 1",
            "schema = 'solvers.nlh/v1'\nunknown = 1",
            "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack_bb = 10.0000000000000001",
        ] {
            assert!(
                assemble(
                    SOURCE,
                    Some((raw, Path::new("base.toml"))),
                    "BTN c, BB x",
                    "Ks 7h 2d",
                    &ranges(),
                    DerivedFrom::default()
                )
                .is_err()
            );
        }
    }
}
