//! Shared per-hand/per-action EV table widget for the Solve and Results
//! tabs' "Evaluate EVs" button (see `docs/native-gui-plan.md` section F):
//! renders one `NodeActionEvaluation` as a GTO-Wizard-style table -- rows
//! sorted by `weight_share` descending, an aggregate row, and per-cell EV
//! mean plus a dimmer `±ci95` half-width.

use eframe::egui;
use egui::Ui;
use multiway::solver::{NodeActionEvaluation, ProfileEstimate};

use crate::matrix;

/// Draws `evaluation`. `is_preflop` selects whether each row's `group` (a
/// bare `BucketId`, since `NodeActionEvaluation` itself carries no street)
/// is labeled as a 13x13 hand class or a generic postflop bucket -- the
/// caller already knows which node it asked to evaluate, so it supplies
/// this rather than the table guessing.
pub fn ui(ui: &mut Ui, evaluation: &NodeActionEvaluation, is_preflop: bool, actor_label: &str) {
    ui.label(format!(
        "actor: {actor_label} | samples: {} | total deal attempts: {}",
        evaluation.samples, evaluation.total_deal_attempts
    ));
    egui::Grid::new("node-eval-grid")
        .striped(true)
        .show(ui, |ui| {
            ui.label("group");
            ui.label("weight");
            for label in &evaluation.action_labels {
                ui.label(label);
            }
            ui.end_row();

            // The evaluator always runs with `samples >= 1` (the UI's
            // sample-count field is clamped to at least 1, matching
            // `SolverError::ZeroEvaluationSamples`), so the aggregate row
            // always has real EVs -- unlike a per-group row, there is no
            // "this group never appeared" case for the pooled aggregate.
            ui.label(egui::RichText::new("aggregate").strong());
            ui.label("100.0%");
            for aggregate in &evaluation.aggregate {
                render_estimate(ui, &aggregate.ev);
            }
            ui.end_row();

            let mut groups: Vec<&_> = evaluation.groups.iter().collect();
            groups.sort_unstable_by(|a, b| {
                b.weight_share
                    .partial_cmp(&a.weight_share)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for group in groups {
                let label = if is_preflop {
                    matrix::class_label(group.group as usize)
                } else {
                    format!("bucket {}", group.group)
                };
                ui.monospace(label);
                ui.monospace(format!("{:>5.1}%", group.weight_share * 100.0));
                for action in &group.actions {
                    // The engine only exposes a per-group sample count (not
                    // per action-cell), so a zero-sample group blanks its
                    // whole row rather than guessing per cell.
                    if group.samples == 0 {
                        ui.label("\u{2014}");
                    } else {
                        render_estimate(ui, action);
                    }
                }
                ui.end_row();
            }
        });
}

/// One EV cell: `mean` (2 decimals) with a smaller, dimmer `±ci95`
/// half-width underneath.
fn render_estimate(ui: &mut Ui, estimate: &ProfileEstimate) {
    ui.vertical(|ui| {
        ui.monospace(format!("{:.2}", estimate.mean));
        let half_width = (estimate.ci95[1] - estimate.ci95[0]) / 2.0;
        ui.small(format!("\u{b1}{half_width:.2}"));
    });
}
