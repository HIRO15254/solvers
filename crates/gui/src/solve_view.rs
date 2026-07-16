//! Solve tab: status strip, Pause/Resume/Finish/Cancel controls, live
//! convergence charts (per-seat log10 avg-positive-regret and strategy
//! drift vs sweeps), and the evaluation table.

use eframe::egui;
use egui::Ui;
use egui_plot::{Line, Plot, PlotPoints};
use multiway::solver::ProfileEvaluation;

use crate::worker::{ProgressSnapshot, WorkerCmd, WorkerHandle};

/// Cap plotted points per line; history beyond this is decimated (recent
/// samples are always kept, older ones are subsampled evenly).
const MAX_PLOT_POINTS: usize = 4_000;

#[derive(Clone, Debug, PartialEq)]
pub enum RunStatus {
    Idle,
    Building,
    Running,
    Paused,
    Finished,
    Cancelled,
    Failed(String),
}

impl RunStatus {
    fn label(&self) -> String {
        match self {
            RunStatus::Idle => "idle".to_string(),
            RunStatus::Building => "building game/abstraction...".to_string(),
            RunStatus::Running => "running".to_string(),
            RunStatus::Paused => "paused".to_string(),
            RunStatus::Finished => "finished".to_string(),
            RunStatus::Cancelled => "cancelled".to_string(),
            RunStatus::Failed(error) => format!("failed: {error}"),
        }
    }
}

pub struct SolveTabState {
    pub seat_names: Vec<String>,
    pub history: Vec<ProgressSnapshot>,
    pub latest_evaluation: Option<ProfileEvaluation>,
    pub status: RunStatus,
}

impl SolveTabState {
    pub fn new(seat_names: Vec<String>) -> Self {
        Self {
            seat_names,
            history: Vec::new(),
            latest_evaluation: None,
            status: RunStatus::Idle,
        }
    }
}

pub fn ui(ui: &mut Ui, state: &mut SolveTabState, worker: Option<&WorkerHandle>) {
    ui.horizontal(|ui| {
        ui.label(format!("status: {}", state.status.label()));
        if let Some(latest) = state.history.last() {
            ui.add(
                egui::ProgressBar::new(
                    (latest.sweeps as f32 / latest.target.max(1) as f32).min(1.0),
                )
                .text(format!("{}/{}", latest.sweeps, latest.target)),
            );
            ui.label(format!("{:.1} sweeps/s", latest.sweeps_per_sec));
            ui.label(format!("infosets {}", latest.infosets));
            ui.label(format!("mem {} MiB", latest.memory_bytes / (1024 * 1024)));
            ui.label(format!("elapsed {:.0}s", latest.elapsed_secs));
        }
    });

    if let Some(worker) = worker {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    state.status == RunStatus::Running,
                    egui::Button::new("Pause"),
                )
                .clicked()
            {
                worker.send(WorkerCmd::Pause);
                state.status = RunStatus::Paused;
            }
            if ui
                .add_enabled(
                    state.status == RunStatus::Paused,
                    egui::Button::new("Resume"),
                )
                .clicked()
            {
                worker.send(WorkerCmd::Resume);
                state.status = RunStatus::Running;
            }
            if ui.button("Finish && Save").clicked() {
                worker.send(WorkerCmd::Finish);
            }
            if ui.button("Cancel").clicked() {
                worker.send(WorkerCmd::Cancel);
            }
        });
    }
    ui.separator();

    let seats = state.seat_names.len();
    Plot::new("regret-plot")
        .height(200.0)
        .legend(egui_plot::Legend::default())
        .show(ui, |plot_ui| {
            for seat in 0..seats {
                let points: PlotPoints = decimate(&state.history)
                    .map(|snap| {
                        let regret = snap.seat_avg_pos_regret.get(seat).copied().unwrap_or(0.0);
                        [snap.sweeps as f64, regret.max(1e-12).log10()]
                    })
                    .collect();
                plot_ui.line(Line::new(seat_name(state, seat), points));
            }
        });
    ui.label("per-seat log10(avg positive regret) vs sweeps");

    Plot::new("drift-plot")
        .height(200.0)
        .legend(egui_plot::Legend::default())
        .show(ui, |plot_ui| {
            for seat in 0..seats {
                let points: PlotPoints = decimate(&state.history)
                    .map(|snap| {
                        let drift = snap.seat_drift_l1.get(seat).copied().unwrap_or(0.0);
                        [snap.sweeps as f64, drift]
                    })
                    .collect();
                plot_ui.line(Line::new(seat_name(state, seat), points));
            }
        });
    ui.label("per-seat strategy drift (L1) vs sweeps");
    ui.separator();

    ui.heading("Evaluation");
    if let Some(evaluation) = &state.latest_evaluation {
        egui::Grid::new("evaluation-grid")
            .striped(true)
            .show(ui, |ui| {
                ui.label("seat");
                ui.label("EV mean");
                ui.label("95% CI");
                ui.label("deviation-gain LB");
                ui.end_row();
                for (seat, estimate) in evaluation.seats.iter().enumerate() {
                    ui.monospace(seat_name(state, seat));
                    ui.monospace(format!("{:.4}", estimate.mean));
                    ui.monospace(format!(
                        "[{:.4}, {:.4}]",
                        estimate.ci95[0], estimate.ci95[1]
                    ));
                    let gain = evaluation
                        .deviation_gain_lower_bound
                        .as_ref()
                        .and_then(|values| values.get(seat))
                        .map(|value| format!("{:.4}", value.mean))
                        .unwrap_or_else(|| "-".to_string());
                    ui.monospace(gain);
                    ui.end_row();
                }
            });
    } else {
        ui.label("no evaluation yet");
    }

    ui.separator();
    ui.colored_label(
        crate::theme::ACCENT,
        "approximate profile — Nash/GTO保証なし",
    );
}

fn seat_name(state: &SolveTabState, seat: usize) -> String {
    state
        .seat_names
        .get(seat)
        .cloned()
        .unwrap_or_else(|| format!("seat {seat}"))
}

fn decimate(history: &[ProgressSnapshot]) -> impl Iterator<Item = &ProgressSnapshot> {
    let step = (history.len() / MAX_PLOT_POINTS).max(1);
    history.iter().step_by(step)
}
