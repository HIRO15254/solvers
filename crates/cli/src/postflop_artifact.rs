//! CLI rendering of typed P1 artifact views.
use crate::multiway_artifact::{ExportFormat, ExportView};
use anyhow::{Context, Result};
use hu_postflop::views::{self, *};
use serde::Serialize;
use std::path::Path;
pub fn export(
    path: &Path,
    view: ExportView,
    format: ExportFormat,
    node: &str,
    output: Option<&Path>,
) -> Result<()> {
    let view = match view {
        ExportView::Summary => views::ExportView::Summary,
        ExportView::Tree => views::ExportView::Tree,
        ExportView::Actions => views::ExportView::Actions,
        ExportView::Strategy => views::ExportView::Strategy,
        ExportView::Ev => views::ExportView::Ev,
        ExportView::Range => views::ExportView::Range,
    };
    let data = views::export(path, view, node, &mut crate::nlh_v1::print_artifact)?;
    let rendered = match format {
        ExportFormat::Json => to_json(&data)?,
        ExportFormat::Csv => match &data {
            ExportData::Summary(data) => summary_csv(data),
            ExportData::Tree(data) => tree_csv(data),
            ExportData::Actions(data) => action_csv(data),
            ExportData::Strategy(data) => strategy_csv(data),
            ExportData::Ev(data) => value_csv(data),
            ExportData::Range(data) => range_csv(data),
        },
    };
    match output {
        Some(path) => {
            std::fs::write(path, rendered)
                .with_context(|| format!("writing {}", path.display()))?;
            println!("wrote {}", path.display());
        }
        None => print!("{rendered}"),
    }
    Ok(())
}
pub fn compare(left: &Path, right: &Path, cross_game: bool) -> Result<()> {
    print!(
        "{}",
        to_json(&views::compare(
            left,
            right,
            cross_game,
            &mut crate::nlh_v1::print_artifact
        )?)?
    );
    Ok(())
}
fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)? + "\n")
}

fn summary_csv(summary: &Summary) -> String {
    let mut out = String::from(
        "board,pot,effective_stack,min_bet,iterations,ev_oop,ev_ip,expl_oop,expl_ip,\
         nash_conv,storage,wall_secs,streets_stored,nodes,stored_nodes\n",
    );
    out.push_str(&format!(
        "{},{},{},{},{},{:.6},{:.6},{:.3e},{:.3e},{:.3e},{},{:.3},{},{},{}\n",
        summary.board,
        summary.pot,
        summary.effective_stack,
        summary.min_bet,
        summary.iterations,
        summary.ev_oop,
        summary.ev_ip,
        summary.expl_oop,
        summary.expl_ip,
        summary.nash_conv,
        summary.storage,
        summary.wall_secs,
        summary.streets_stored,
        summary.nodes,
        summary.stored_nodes,
    ));
    out
}

fn tree_csv(rows: &[TreeRow]) -> String {
    let mut out = String::from("history,street,actor,pot,stored,actions\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            row.history,
            row.street,
            row.actor,
            row.pot,
            row.stored,
            row.actions.join("|"),
        ));
    }
    out
}

fn action_csv(rows: &[ActionRow]) -> String {
    let mut out = String::from("history,street,actor,action,frequency\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{:.6}\n",
            row.history, row.street, row.actor, row.action, row.frequency
        ));
    }
    out
}

fn strategy_csv(rows: &[StrategyRow]) -> String {
    let mut out = String::from("history,actor,combo,weight,probabilities\n");
    for row in rows {
        let probs: Vec<String> = row
            .probabilities
            .iter()
            .map(|p| format!("{p:.6}"))
            .collect();
        out.push_str(&format!(
            "{},{},{},{:.6},{}\n",
            row.history,
            row.actor,
            row.combo,
            row.weight,
            probs.join("|"),
        ));
    }
    out
}

fn value_csv(rows: &[ValueRow]) -> String {
    let mut out = String::from("history,seat,combo,weight,ev\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{:.6},{:.6}\n",
            row.history, row.seat, row.combo, row.weight, row.ev
        ));
    }
    out
}

fn range_csv(rows: &[RangeRow]) -> String {
    let mut out = String::from("seat,combo,weight\n");
    for row in rows {
        out.push_str(&format!("{},{},{:.6}\n", row.seat, row.combo, row.weight));
    }
    out
}
