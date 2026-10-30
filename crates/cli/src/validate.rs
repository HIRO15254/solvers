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
pub(super) fn diagnostic_bb(value: &mut Value, scale: spot::ChipScale) {
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
                        *value = if amount % scale.chips_per_bb() == 0 {
                            json!(amount / scale.chips_per_bb())
                        } else {
                            json!(amount as f64 / scale.chips_per_bb() as f64)
                        };
                    }
                } else {
                    diagnostic_bb(value, scale);
                }
            }
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| diagnostic_bb(value, scale)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_amounts_use_table_scale_recursively() {
        let mut value = json!({
            "pot": 15,
            "seats": [{"starting_stack": 30, "remaining_stack": 25}],
            "actions": [{"amount": 5, "to": 10}],
            "players": 3
        });
        diagnostic_bb(&mut value, spot::ChipScale::new(10).unwrap());
        assert_eq!(value["pot"], json!(1.5));
        assert_eq!(value["seats"][0]["starting_stack"], json!(3));
        assert_eq!(value["seats"][0]["remaining_stack"], json!(2.5));
        assert_eq!(value["actions"][0]["amount"], json!(0.5));
        assert_eq!(value["actions"][0]["to"], json!(1));
        assert_eq!(value["players"], json!(3));
    }
}
