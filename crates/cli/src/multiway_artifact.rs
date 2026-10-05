//! CLI arguments and byte-stable rendering of product-owned artifact views.
use anyhow::{Context, Result};
use mw_preflop::views;
use std::path::Path;
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum InspectView {
    Node,
    Summary,
    Strategy,
    Range,
    Ev,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportView {
    Strategy,
    Actions,
    Range,
    Ev,
    Tree,
    Summary,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}

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
impl From<ExportView> for views::ExportView {
    fn from(value: ExportView) -> Self {
        match value {
            ExportView::Strategy => Self::Strategy,
            ExportView::Actions => Self::Actions,
            ExportView::Range => Self::Range,
            ExportView::Ev => Self::Ev,
            ExportView::Tree => Self::Tree,
            ExportView::Summary => Self::Summary,
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
    let views::ExportData {
        metadata,
        strategies,
        ranges,
    } = views::export(path, view.into())?;
    let rendered = match (view, format) {
        (ExportView::Summary, ExportFormat::Json) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": metadata.schema_version,
                "sweeps": metadata.sweeps,
                "approximateProfile": metadata.approximate_profile,
                "visitedInfosets": strategies.len(),
                "seats": metadata.seats,
            }))?
        }
        (ExportView::Tree, ExportFormat::Json) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "states": metadata.public_states,
                "edges": metadata.histories,
            }))?
        }
        (ExportView::Strategy, ExportFormat::Json) => serde_json::to_string_pretty(&strategies)?,
        (ExportView::Ev, ExportFormat::Json) => serde_json::to_string_pretty(&metadata.seats)?,
        (ExportView::Range, ExportFormat::Json) => export_ranges_json(&ranges)?,
        (ExportView::Actions, ExportFormat::Json) => {
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
        (ExportView::Strategy, ExportFormat::Csv) => {
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
        (ExportView::Actions, ExportFormat::Csv) => {
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
        (ExportView::Tree, ExportFormat::Csv) => {
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
        (ExportView::Summary, ExportFormat::Csv) => {
            format!(
                "schema_version,sweeps,approximate_profile,visited_infosets\n{},{},{},{}\n",
                metadata.schema_version,
                metadata.sweeps,
                metadata.approximate_profile,
                strategies.len()
            )
        }
        (ExportView::Range, ExportFormat::Csv) => export_ranges_csv(&ranges)?,
        (ExportView::Ev, ExportFormat::Csv) => export_ev_csv(&metadata.seats),
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
