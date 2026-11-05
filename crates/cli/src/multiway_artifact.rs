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
    )
    .map_err(|error| {
        if error
            .chain()
            .any(|cause| cause.to_string().contains("NLH003:"))
        {
            anyhow::Error::new(crate::derive::InputError(error))
        } else {
            error
        }
    })?;
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

/// Render P2 class-level data using the same row-oriented surface as P1.
pub fn export(
    path: &Path,
    view: ExportView,
    format: ExportFormat,
    nodes: &[String],
    output: Option<&Path>,
) -> Result<()> {
    let requested = match view {
        ExportView::Strategy => views::ExportView::Strategy,
        ExportView::Actions => views::ExportView::Actions,
        ExportView::Range => views::ExportView::Range,
        ExportView::Ev => views::ExportView::Ev,
        ExportView::Tree => views::ExportView::Tree,
        ExportView::Summary => views::ExportView::Summary,
        ExportView::Lock | ExportView::Decision => {
            anyhow::bail!("this view requires a postflop .sol artifact")
        }
    };
    let data = views::export(path, requested)?;
    let metadata = &data.metadata;
    let mut selected = Vec::new();
    if !matches!(view, ExportView::Summary) {
        for node in nodes {
            if node == "all" {
                selected.extend(
                    metadata
                        .public_states
                        .iter()
                        .filter(|state| matches!(view, ExportView::Tree) || state.actor.is_some()),
                );
                continue;
            }
            let key = views::parse_history(metadata, node).map_err(crate::derive::InputError)?;
            let state = metadata
                .public_states
                .iter()
                .find(|state| state.history == key)
                .ok_or_else(|| {
                    crate::derive::InputError(anyhow::anyhow!(
                        "node {node:?} is absent from the public tree"
                    ))
                })?;
            if matches!(view, ExportView::Actions | ExportView::Strategy) && state.actor.is_none() {
                return Err(crate::derive::InputError(anyhow::anyhow!(
                    "node {node:?} is not an action node"
                ))
                .into());
            }
            selected.push(state);
        }
    }
    let mut rows = Vec::new();
    let columns: &[&str] = match view {
        ExportView::Tree => {
            for state in selected {
                let children: Vec<_> = metadata
                    .histories
                    .iter()
                    .filter(|edge| edge.parent == state.history)
                    .collect();
                let kind = if state.actor.is_some() {
                    "action"
                } else {
                    "terminal"
                };
                rows.push(serde_json::json!({
                    "history": views::display_history(metadata, state.history),
                    "kind": kind, "street": street_name(state.street),
                    "actor": state.actor.map(|actor| actor.to_string()),
                    "pot": state.pot_chips as f64 / metadata.bb_chips as f64,
                    "stored": true,
                    "actions": state.legal_actions.iter().map(|action| views::display_action(action, metadata.bb_chips)).collect::<Vec<_>>(),
                    "children": children.iter().map(|edge| views::display_history(metadata, edge.key)).collect::<Vec<_>>(),
                    "child_kinds": children.iter().map(|edge| if metadata.public_states.iter().find(|child| child.history == edge.key).is_some_and(|child| child.actor.is_some()) { "action" } else { "terminal" }).collect::<Vec<_>>(),
                }));
            }
            &[
                "history",
                "kind",
                "street",
                "actor",
                "pot",
                "stored",
                "actions",
                "children",
                "child_kinds",
            ]
        }
        ExportView::Actions | ExportView::Strategy => {
            for state in selected {
                let actor = state.actor.expect("selected action node");
                let ranges = views::range_matrix(metadata, &data.strategies, state.history)?;
                let cells = ranges[actor as usize]["grid"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|row| row.as_array().into_iter().flatten());
                let mut mass = 0.0;
                let mut complete = true;
                let mut totals = vec![0.0; state.legal_actions.len()];
                for cell in cells {
                    let Some(weight) = cell["reach_weight"].as_f64() else {
                        complete = false;
                        continue;
                    };
                    let Some(bucket) = cell["bucket"].as_u64() else {
                        complete = false;
                        continue;
                    };
                    let Some(block) = data.strategies.iter().find(|block| {
                        block.key.history == state.history
                            && block.key.actor == actor
                            && block.key.street == 0
                            && block.key.bucket_path[0] == bucket as u32
                    }) else {
                        if weight > 0.0 {
                            complete = false;
                        }
                        continue;
                    };
                    mass += weight;
                    for (total, probability) in totals.iter_mut().zip(&block.probabilities) {
                        *total += weight * f64::from(*probability);
                    }
                    if matches!(view, ExportView::Strategy) {
                        rows.push(serde_json::json!({
                            "history": views::display_history(metadata, state.history),
                            "actor": actor.to_string(), "combo": cell["hand"],
                            "weight": weight, "probabilities": block.probabilities,
                        }));
                    }
                }
                if matches!(view, ExportView::Actions) {
                    for (action, total) in state.legal_actions.iter().zip(totals) {
                        rows.push(serde_json::json!({
                            "history": views::display_history(metadata, state.history),
                            "street": street_name(state.street), "actor": actor.to_string(),
                            "action": views::display_action(action, metadata.bb_chips),
                            "frequency": if complete && mass > 0.0 { Some(total / mass) } else { None },
                        }));
                    }
                }
            }
            if matches!(view, ExportView::Actions) {
                &["history", "street", "actor", "action", "frequency"]
            } else {
                &["history", "actor", "combo", "weight", "probabilities"]
            }
        }
        ExportView::Range => {
            for state in selected {
                for seat in views::range_matrix(metadata, &data.strategies, state.history)? {
                    for row in seat["grid"].as_array().into_iter().flatten() {
                        for cell in row.as_array().into_iter().flatten() {
                            rows.push(serde_json::json!({ "seat": seat["seat"], "combo": cell["hand"], "weight": cell["reach_weight"] }));
                        }
                    }
                }
            }
            &["seat", "combo", "weight"]
        }
        ExportView::Ev => {
            for state in selected {
                for seat in views::range_matrix(metadata, &data.strategies, state.history)? {
                    for row in seat["grid"].as_array().into_iter().flatten() {
                        for cell in row.as_array().into_iter().flatten() {
                            rows.push(serde_json::json!({
                                "history": views::display_history(metadata, state.history),
                                "seat": seat["seat"],
                                "combo": cell["hand"],
                                "weight": cell["reach_weight"],
                                "ev": null,
                            }));
                        }
                    }
                }
            }
            &["history", "seat", "combo", "weight", "ev"]
        }
        ExportView::Summary => {
            rows.push(serde_json::json!({
                "schema_version": metadata.schema_version,
                "sweeps": metadata.sweeps, "approximate_profile": metadata.approximate_profile,
                "visited_infosets": data.strategies.len(), "seats": metadata.seats,
            }));
            &[
                "schema_version",
                "sweeps",
                "approximate_profile",
                "visited_infosets",
            ]
        }
        ExportView::Lock | ExportView::Decision => unreachable!(),
    };
    let rendered = match format {
        ExportFormat::Json => {
            if matches!(view, ExportView::Summary) {
                serde_json::to_string_pretty(&rows[0])? + "\n"
            } else {
                serde_json::to_string_pretty(&rows)? + "\n"
            }
        }
        ExportFormat::Csv => {
            let mut result = columns.join(",") + "\n";
            for row in &rows {
                let fields: Vec<_> = columns
                    .iter()
                    .map(|column| csv_value(&row[*column]))
                    .collect();
                result.push_str(&fields.join(","));
                result.push('\n');
            }
            result
        }
    };
    if let Some(output) = output {
        std::fs::write(output, rendered)
            .with_context(|| format!("writing {}", output.display()))?;
        println!("wrote {}", output.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn street_name(street: u8) -> &'static str {
    match street {
        0 => "preflop",
        1 => "flop",
        2 => "turn",
        3 => "river",
        _ => "unknown",
    }
}

fn csv_value(value: &serde_json::Value) -> String {
    let text = match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(values) => {
            values.iter().map(csv_value).collect::<Vec<_>>().join("|")
        }
        _ => value.to_string(),
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}
