use anyhow::{Context, Result};
use clap::ValueEnum;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ValidationFormat {
    #[default]
    Human,
    Json,
}

pub fn run(
    path: &Path,
    format: ValidationFormat,
    show: bool,
    write: Option<&Path>,
    resources: bool,
) -> Result<()> {
    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    crate::nlh_v1::validate(&raw, path, format, show, write, resources)
}
pub(super) fn diagnostic_bb(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "pot"
                        | "effective_stack"
                        | "starting_stack"
                        | "remaining_stack"
                        | "total_contribution"
                        | "refund"
                        | "amount"
                        | "to"
                ) {
                    if let Some(amount) = value.as_u64() {
                        *value = if amount % 1000 == 0 {
                            json!(amount / 1000)
                        } else {
                            json!(amount as f64 / 1000.0)
                        };
                    }
                } else {
                    diagnostic_bb(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(diagnostic_bb),
        _ => {}
    }
}
