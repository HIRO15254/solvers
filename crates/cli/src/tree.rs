use anyhow::{Context, Result};
use std::path::Path;

use crate::validate::ValidationFormat;

pub fn run(path: &Path, nodes: &[String], format: ValidationFormat) -> Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let rows = hu_postflop::views::preview_tree(&raw, path, nodes)?;
    match format {
        ValidationFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
        ValidationFormat::Human => {
            for row in rows {
                println!(
                    "{}: {} {} actor={} pot={} BB",
                    row.history,
                    row.kind,
                    row.street,
                    row.actor.as_deref().unwrap_or("-"),
                    row.pot.map_or_else(|| "-".into(), |pot| pot.to_string()),
                );
                for ((action, child), kind) in
                    row.actions.iter().zip(&row.children).zip(&row.child_kinds)
                {
                    println!("  {action} -> {child} ({kind})");
                }
            }
        }
    }
    Ok(())
}
