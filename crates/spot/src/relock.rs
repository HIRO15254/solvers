//! Merge profile edits into a solved P1 configuration.
use crate::{Code, Document, Product, SpotError};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, Value};

/// Profile changes applied before re-solving a source configuration.
#[derive(Clone, Debug, Default)]
pub struct RelockEdits {
    pub add_profile_toml: Option<String>,
    pub replace: bool,
    pub clear_profile: bool,
    pub freeze_upstream: Option<PathBuf>,
}

fn invalid(key: &str, message: impl Into<String>) -> SpotError {
    SpotError::new(Code::NLH002, key, message)
}

fn position_appended(table: &mut Table) {
    table.set_position(usize::MAX);
    for (_, item) in table.iter_mut() {
        if let Some(child) = item.as_table_mut() {
            position_appended(child);
        } else if let Some(children) = item.as_array_of_tables_mut() {
            for child in children.iter_mut() {
                position_appended(child);
            }
        }
    }
}

/// Merge edits and validate the resulting P1 config with the normal document parser.
/// Relative paths in the config resolve against the current working directory;
/// callers should supply self-contained source configs and an absolute freeze path.
pub fn relock_config(source_config_toml: &str, edits: &RelockEdits) -> Result<String, SpotError> {
    let mut config: DocumentMut = source_config_toml
        .parse()
        .map_err(|e: toml_edit::TomlError| invalid("config", e.to_string()))?;
    if edits.clear_profile {
        config.remove("profile");
    }
    if let Some(fragment) = &edits.add_profile_toml {
        let mut addition: DocumentMut = fragment
            .parse()
            .map_err(|e: toml_edit::TomlError| invalid("profile", e.to_string()))?;
        if addition.iter().any(|(key, _)| key != "profile") {
            return Err(invalid(
                "profile",
                "fragment may contain only profile entries",
            ));
        }
        if let Some(profile) = addition.remove("profile") {
            let table = profile
                .as_table()
                .ok_or_else(|| invalid("profile", "expected table"))?;
            for (kind, item) in table.iter() {
                if !matches!(kind, "lock" | "incentive" | "frequency") {
                    return Err(invalid(
                        &format!("profile.{kind}"),
                        "fragment may contain only lock, incentive, and frequency arrays",
                    ));
                }
                let entries = item.as_array_of_tables().ok_or_else(|| {
                    invalid(&format!("profile.{kind}"), "expected array of tables")
                })?;
                for entry in entries.iter() {
                    if entry.get("node").and_then(Item::as_str).is_none() {
                        return Err(invalid(&format!("profile.{kind}.node"), "required string"));
                    }
                }
                if !config.contains_key("profile") {
                    config["profile"] = Item::Table(Table::new());
                }
                let target = config["profile"]
                    .as_table_mut()
                    .ok_or_else(|| invalid("profile", "expected table"))?;
                if !target.contains_key(kind) {
                    target.insert(kind, Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
                }
                let existing = target[kind].as_array_of_tables_mut().ok_or_else(|| {
                    invalid(&format!("profile.{kind}"), "expected array of tables")
                })?;
                if edits.replace {
                    existing.retain(|old| {
                        !entries.iter().any(|new| {
                            old.get("node").and_then(Item::as_str)
                                == new.get("node").and_then(Item::as_str)
                        })
                    });
                }
                for entry in entries.iter() {
                    let mut entry = entry.clone();
                    // Parsed fragments carry document positions from their own file;
                    // keep appended entries after all tables in the source document.
                    position_appended(&mut entry);
                    existing.push(entry);
                }
            }
        }
    }
    if let Some(path) = &edits.freeze_upstream {
        let path = path.to_str().ok_or_else(|| {
            invalid(
                "profile.freeze_upstream.solution",
                "path is not valid UTF-8",
            )
        })?;
        if !config.contains_key("profile") {
            config["profile"] = Item::Table(Table::new());
        }
        let profile = config["profile"]
            .as_table_mut()
            .ok_or_else(|| invalid("profile", "expected table"))?;
        let mut freeze = toml_edit::InlineTable::new();
        freeze.insert("solution", Value::from(path));
        profile.insert("freeze_upstream", Item::Value(Value::InlineTable(freeze)));
    }
    let result = config.to_string();
    let parsed = Document::parse(&result, Path::new("run.toml"))?;
    if parsed.spot.product != Product::HuPostflop {
        return Err(invalid(
            "spot",
            "relock requires a P1 postflop configuration",
        ));
    }
    Ok(result)
}
