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
    resources: Option<ResourceSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_config: Option<serde_json::Value>,
}

/// Byte-bounded tree preflight reported by `--resources`. Every field is
/// derived without allocating the policy arena or building EHS2 tables.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSummary {
    complete: bool,
    recall: String,
    decision_nodes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_edges: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_columns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_slots: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    solver_state_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icm: Option<IcmSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IcmSummary {
    field_players: u64,
    paid_places: u64,
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    samples: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prepared_bytes: Option<u64>,
}

impl From<crate::session::MultiwayResourcePreflight> for ResourceSummary {
    fn from(preflight: crate::session::MultiwayResourcePreflight) -> Self {
        Self {
            complete: preflight.complete,
            recall: format!("{:?}", preflight.recall).to_lowercase(),
            decision_nodes: preflight.decision_nodes,
            terminal_edges: preflight.terminal_edges,
            policy_columns: preflight.policy_columns,
            policy_slots: preflight.policy_slots,
            solver_state_bytes: preflight.solver_state_bytes,
            icm: preflight.icm.map(|icm| IcmSummary {
                field_players: icm.field_players,
                paid_places: icm.paid_places,
                mode: match icm.mode {
                    crate::session::IcmPreflightMode::Exact => "exact",
                    crate::session::IcmPreflightMode::Sampled => "sampled",
                },
                samples: icm.samples,
                prepared_bytes: icm.prepared_bytes,
            }),
        }
    }
}

pub fn run(
    config_path: &Path,
    format: ValidationFormat,
    show_effective: bool,
    write_effective: Option<&Path>,
    resources: bool,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    if !crate::multiway_v1::has_v1_schema(&raw)? {
        return validate_solver_config(&raw, format, show_effective, write_effective, resources);
    }
    crate::multiway_v1::validate_production_contract(&raw)?;
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
    let resource_summary = resources
        .then(|| crate::session::preflight_multiway_config(&raw))
        .transpose()
        .context("running the resource preflight")?
        .map(ResourceSummary::from);

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
        resources: resource_summary,
        effective_config,
    };
    match format {
        ValidationFormat::Human => println!(
            "valid: schema={} seats={} chip_unit_bb={} profile={}",
            summary.schema, summary.seat_count, summary.chip_unit_bb, summary.profile
        ),
        ValidationFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
    }
    if let Some(resources) = &summary.resources
        && matches!(format, ValidationFormat::Human)
    {
        println!(
            "resources: complete={} recall={} decision_nodes={} policy_slots={} solver_state_bytes={}",
            resources.complete,
            resources.recall,
            resources.decision_nodes,
            resources
                .policy_slots
                .map_or_else(|| "n/a".to_string(), |slots| slots.to_string()),
            resources
                .solver_state_bytes
                .map_or_else(|| "n/a".to_string(), |bytes| bytes.to_string()),
        );
    }
    if show_effective && matches!(format, ValidationFormat::Human) {
        println!("\n{effective_toml}");
    }
    Ok(())
}

/// `validate` for the toy, postflop, and heads-up preflop contracts.
///
/// Their effective config is the parsed form serialized back, so the same
/// call both checks the file and produces the self-contained form a daemon
/// wants (R10).
fn validate_solver_config(
    raw: &str,
    format: ValidationFormat,
    show_effective: bool,
    write_effective: Option<&Path>,
    resources: bool,
) -> Result<()> {
    if resources {
        return Err(anyhow!(
            "--resources reports the multiway policy arena; the exact engine \
             prints its memory estimate when the solve builds its tree"
        ));
    }
    let effective_toml = crate::solver_config_v1::normalized_toml(raw)?;
    let kind = crate::solver_config_v1::game_kind(raw)?;
    let schema = crate::solver_config_v1::declared(raw)?;
    if let Some(path) = write_effective {
        std::fs::write(path, &effective_toml)
            .with_context(|| format!("writing effective config {}", path.display()))?;
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SolverConfigSummary {
        status: &'static str,
        schema: String,
        game_kind: &'static str,
        profile: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        effective_config: Option<serde_json::Value>,
    }

    let summary = SolverConfigSummary {
        status: "valid",
        schema,
        game_kind: kind,
        // Two-player zero-sum, so the average strategy converges to Nash --
        // unlike the multiway families, which say the opposite.
        profile: "vector CFR average profile; converges to Nash for two players",
        effective_config: show_effective
            .then(|| crate::solver_config_v1::normalized_json(raw))
            .transpose()?,
    };
    match format {
        ValidationFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
        ValidationFormat::Human => println!(
            "valid: schema={} game_kind={} profile={}",
            summary.schema, summary.game_kind, summary.profile
        ),
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
