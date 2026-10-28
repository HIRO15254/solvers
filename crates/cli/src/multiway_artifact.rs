//! CLI arguments and byte-stable rendering of product-owned artifact views.
use crate::artifact_args::{ExportFormat, ExportView, InspectView};
use anyhow::{Context, Result};
use mw_preflop::views;
use std::path::Path;
impl From<InspectView> for views::InspectView {
    fn from(value: InspectView) -> Self {
        match value {
            InspectView::Node => Self::Node,
            InspectView::Summary => Self::Summary,
            InspectView::Strategy => Self::Strategy,
            InspectView::Range => Self::Range,
            InspectView::Ev => Self::Ev,
        }
    }
}

fn key_hex(key: [u8; 16]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn inspect(
    path: &Path,
    requested_history: &str,
    view: InspectView,
    actor_override: Option<u8>,
    samples: u64,
    seed: u64,
    br_traversals: u64,
) -> Result<()> {
    let data = views::inspect(
        path,
        views::InspectRequest {
            history: requested_history,
            view: view.into(),
            actor: actor_override,
            samples,
            seed,
            br_traversals,
        },
        crate::cache::root().as_deref(),
        &mut crate::multiway_solve::print_abstraction,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::to_value(data)?)?
    );
    Ok(())
}

pub fn compare(left: &Path, right: &Path, cross_game: bool) -> Result<()> {
    let data = views::compare(
        left,
        right,
        cross_game,
        crate::cache::root().as_deref(),
        &mut crate::multiway_solve::print_abstraction,
    )?;
    println!("{}", serde_json::to_string_pretty(&data)?);
    Ok(())
}

pub fn evaluate(path: &Path, samples: u64, seed: u64, br_traversals: u64) -> Result<()> {
    let data = views::evaluate(
        path,
        samples,
        seed,
        br_traversals,
        crate::cache::root().as_deref(),
        &mut crate::multiway_solve::print_abstraction,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::to_value(data)?)?
    );
    Ok(())
}

fn action_fields(
    action: &mw_preflop::mwsol::MultiwayPublicAction,
) -> (&'static str, Option<u64>, bool, bool) {
    match action {
        mw_preflop::mwsol::MultiwayPublicAction::Fold => ("fold", None, false, false),
        mw_preflop::mwsol::MultiwayPublicAction::Check => ("check", None, false, false),
        mw_preflop::mwsol::MultiwayPublicAction::Call {
            amount_millibb,
            all_in,
        } => ("call", Some(*amount_millibb), *all_in, false),
        mw_preflop::mwsol::MultiwayPublicAction::BetTo {
            amount_millibb,
            all_in,
            full_raise,
        } => ("bet-to", Some(*amount_millibb), *all_in, *full_raise),
        mw_preflop::mwsol::MultiwayPublicAction::RaiseTo {
            amount_millibb,
            all_in,
            full_raise,
        } => ("raise-to", Some(*amount_millibb), *all_in, *full_raise),
    }
}

pub fn export(
    path: &Path,
    view: ExportView,
    format: ExportFormat,
    output: Option<&Path>,
) -> Result<()> {
    let view = match view {
        ExportView::Strategy => views::ExportView::Strategy,
        ExportView::Actions => views::ExportView::Actions,
        ExportView::Range => views::ExportView::Range,
        ExportView::Ev => views::ExportView::Ev,
        ExportView::Tree => views::ExportView::Tree,
        ExportView::Summary => views::ExportView::Summary,
        ExportView::Lock => {
            anyhow::bail!("export lock: the solution must be a postflop .sol artifact")
        }
        ExportView::Decision => {
            anyhow::bail!("export decision: the solution must be a postflop .sol artifact")
        }
    };
    let views::ExportData {
        metadata,
        strategies,
        ranges,
    } = views::export(path, view)?;
    let rendered = match (view, format) {
        (views::ExportView::Summary, ExportFormat::Json) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": metadata.schema_version,
                "sweeps": metadata.sweeps,
                "approximateProfile": metadata.approximate_profile,
                "visitedInfosets": strategies.len(),
                "seats": metadata.seats,
            }))?
        }
        (views::ExportView::Tree, ExportFormat::Json) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "states": metadata.public_states,
                "edges": metadata.histories,
            }))?
        }
        (views::ExportView::Strategy, ExportFormat::Json) => {
            serde_json::to_string_pretty(&strategies)?
        }
        (views::ExportView::Ev, ExportFormat::Json) => {
            serde_json::to_string_pretty(&metadata.seats)?
        }
        (views::ExportView::Range, ExportFormat::Json) => export_ranges_json(&ranges)?,
        (views::ExportView::Actions, ExportFormat::Json) => {
            let rows: Vec<_> = metadata
                .public_states
                .iter()
                .filter(|state| !state.legal_actions.is_empty())
                .map(|state| {
                    serde_json::json!({
                        "history": key_hex(state.history),
                        "actor": state.actor,
                        "street": state.street,
                        "actions": state.legal_actions,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&rows)?
        }
        (views::ExportView::Strategy, ExportFormat::Csv) => {
            let mut csv = String::from(
                "history,actor,street,active_opponents,bucket_path,action,probability\n",
            );
            for block in &strategies {
                for (action, probability) in block.actions.iter().zip(&block.probabilities) {
                    csv.push_str(&format!(
                        "{},{},{},{},\"{:?}\",\"{}\",{}\n",
                        key_hex(block.key.history),
                        block.key.actor,
                        block.key.street,
                        block.key.active_opponents,
                        block.key.bucket_path,
                        action.replace('"', "\"\""),
                        probability
                    ));
                }
            }
            csv
        }
        (views::ExportView::Actions, ExportFormat::Csv) => {
            let mut csv = String::from(
                "history,actor,street,action_index,kind,amount_millibb,all_in,full_raise,label\n",
            );
            for state in &metadata.public_states {
                for (index, action) in state.legal_actions.iter().enumerate() {
                    let (kind, amount, all_in, full_raise) = action_fields(action);
                    csv.push_str(&format!(
                        "{},{},{},{},{},{},{},{},{}\n",
                        key_hex(state.history),
                        state
                            .actor
                            .map_or_else(String::new, |actor| actor.to_string()),
                        state.street,
                        index,
                        kind,
                        amount.map_or_else(String::new, |amount| amount.to_string()),
                        all_in,
                        full_raise,
                        action.label(),
                    ));
                }
            }
            csv
        }
        (views::ExportView::Tree, ExportFormat::Csv) => {
            let mut csv = String::from("history,parent,actor,action_index,action\n");
            for node in &metadata.histories {
                csv.push_str(&format!(
                    "{},{},{},{},\"{}\"\n",
                    key_hex(node.key),
                    key_hex(node.parent),
                    node.actor,
                    node.action_index,
                    node.action.replace('"', "\"\"")
                ));
            }
            csv
        }
        (views::ExportView::Summary, ExportFormat::Csv) => {
            format!(
                "schema_version,sweeps,approximate_profile,visited_infosets\n{},{},{},{}\n",
                metadata.schema_version,
                metadata.sweeps,
                metadata.approximate_profile,
                strategies.len()
            )
        }
        (views::ExportView::Range, ExportFormat::Csv) => export_ranges_csv(&ranges)?,
        (views::ExportView::Ev, ExportFormat::Csv) => export_ev_csv(&metadata.seats),
    };
    if let Some(output) = output {
        std::fs::write(output, rendered)
            .with_context(|| format!("writing {}", output.display()))?;
    } else {
        println!("{rendered}");
    }
    Ok(())
}

fn export_ranges_json(ranges: &[views::RangeRow]) -> Result<String> {
    let rows: Vec<_> = ranges
        .iter()
        .enumerate()
        .map(|(seat, value)| serde_json::json!({ "seat": seat, "range": value.range }))
        .collect();
    Ok(serde_json::to_string_pretty(&rows)?)
}

fn export_ranges_csv(ranges: &[views::RangeRow]) -> Result<String> {
    let mut csv = String::from("seat,range\n");
    for (seat, value) in ranges.iter().enumerate() {
        csv.push_str(&format!(
            "{},\"{}\"\n",
            seat,
            value.range.replace('"', "\"\"")
        ));
    }
    Ok(csv)
}

fn export_ev_csv(seats: &[mw_preflop::mwsol::MultiwaySeatResult]) -> String {
    let mut csv = String::from(
        "seat,profile_ev,stderr,ci95_low,ci95_high,measured_deviation_mean,measured_deviation_ci95_high\n",
    );
    for seat in seats {
        let ev = seat.profile_ev.as_ref();
        let deviation = seat.deviation_gain_lower_bound.as_ref();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            seat.seat,
            ev.map(|value| value.mean).unwrap_or(f64::NAN),
            ev.map(|value| value.stderr).unwrap_or(f64::NAN),
            ev.map(|value| value.ci95[0]).unwrap_or(f64::NAN),
            ev.map(|value| value.ci95[1]).unwrap_or(f64::NAN),
            deviation.map(|value| value.mean).unwrap_or(f64::NAN),
            deviation.map(|value| value.ci95[1]).unwrap_or(f64::NAN),
        ));
    }
    csv
}
