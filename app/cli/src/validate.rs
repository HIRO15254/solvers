use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use serde::Serialize;

use crate::config::{GameSection, parse_solve_config_at};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ValidationFormat {
    #[default]
    Human,
    Json,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationSummary {
    status: &'static str,
    schema: &'static str,
    seat_count: usize,
    chip_unit_bb: f64,
    profile: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_config: Option<serde_json::Value>,
}

pub fn run(
    config_path: &Path,
    format: ValidationFormat,
    show_effective: bool,
    write_effective: Option<&Path>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    if !crate::multiway_v1::has_v1_schema(&raw)? {
        return Err(anyhow!(
            "validate currently requires schema = {:?}",
            crate::multiway_v1::SCHEMA
        ));
    }
    let config = parse_solve_config_at(&raw, config_path)?;
    let GameSection::PreflopMultiway(game) = &config.game else {
        unreachable!("v1 routing always lowers to a multiway game")
    };
    let utility = crate::session::convert_utility(config.utility)?;
    let rake = crate::session::convert_rake(config.rake);
    game.validate_economics(&utility, &rake)
        .context("validating normalized game and economics")?;

    let effective_toml = crate::multiway_v1::normalized_toml_at(&raw, config_path)?;
    if let Some(path) = write_effective {
        std::fs::write(path, &effective_toml)
            .with_context(|| format!("writing effective config {}", path.display()))?;
    }
    let effective_config = show_effective
        .then(|| crate::multiway_v1::normalized_config_at(&raw, config_path))
        .transpose()?;

    let summary = ValidationSummary {
        status: "valid",
        schema: crate::multiway_v1::SCHEMA,
        seat_count: game.seats.len(),
        chip_unit_bb: 0.001,
        profile: if game.seats.len() >= 3 {
            "regret-minimized approximate profile; no Nash/GTO guarantee"
        } else {
            "external-sampling MCCFR average profile"
        },
        effective_config,
    };
    match format {
        ValidationFormat::Human => println!(
            "valid: schema={} seats={} chip_unit_bb={} profile={}",
            summary.schema, summary.seat_count, summary.chip_unit_bb, summary.profile
        ),
        ValidationFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
    }
    if show_effective && matches!(format, ValidationFormat::Human) {
        println!("\n{effective_toml}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_format_cli_names_are_stable() {
        use clap::ValueEnum as _;
        assert_eq!(
            ValidationFormat::Human
                .to_possible_value()
                .unwrap()
                .get_name(),
            "human"
        );
        assert_eq!(
            ValidationFormat::Json
                .to_possible_value()
                .unwrap()
                .get_name(),
            "json"
        );
    }
}
