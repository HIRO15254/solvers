//! Solve tab: status strip, Pause/Resume/Finish/Cancel controls, live
//! convergence charts (per-seat log10 avg-positive-regret and strategy
//! drift vs sweeps), the "Live node view" (public-history browser over the
//! in-progress average strategy), and the evaluation table.

use eframe::egui;
use egui::{Rect, Ui, Vec2};
use egui_plot::{Line, Plot, PlotPoints};
use multiway::solver::ProfileEvaluation;

use crate::matrix::{self, CellData};
use crate::worker::{NodeBlock, NodeSnapshot, ProgressSnapshot, WorkerCmd, WorkerHandle};

/// Cap plotted points per line; history beyond this is decimated (recent
/// samples are always kept, older ones are subsampled evenly).
const MAX_PLOT_POINTS: usize = 4_000;

const ROOT: [u8; 16] = [0; 16];

/// Identity of one [`NodeBlock`] within a snapshot, used for hover/pin state
/// (a snapshot has no stable indices across refreshes, so the identity is
/// the block's own key fields rather than a `Vec` position).
type BlockId = (u8, u8, u8, [u32; 4]);

/// Live public-history browser state for the Solve tab. Independent of
/// `ResultsState` (`crate::results`): that one browses a finished
/// `MultiwaySolution`, this one browses `NodeSnapshot`s streamed from the
/// running worker.
pub struct LiveNodeState {
    /// Root-to-current path: `(history key, display label)`. Index 0 is
    /// always `(ROOT, "ROOT")`. Pushed on descend, truncated on a breadcrumb
    /// click -- rebuilt from scratch (rather than re-derived from a
    /// snapshot's `path`) so a jump does not have to wait for a fresh
    /// snapshot to arrive first.
    pub breadcrumb: Vec<([u8; 16], String)>,
    pub snapshot: Option<Box<NodeSnapshot>>,
    pub selected_actor: Option<u8>,
    /// `(street, active_opponents)`, when a node has more than one variant.
    pub selected_variant: Option<(u8, u8)>,
    pub hovered: Option<BlockId>,
    pub pinned: Option<BlockId>,
}

impl Default for LiveNodeState {
    fn default() -> Self {
        Self {
            breadcrumb: vec![(ROOT, "ROOT".to_string())],
            snapshot: None,
            selected_actor: None,
            selected_variant: None,
            hovered: None,
            pinned: None,
        }
    }
}

impl LiveNodeState {
    fn current_history(&self) -> [u8; 16] {
        self.breadcrumb.last().expect("ROOT is always present").0
    }

    fn reset_selection(&mut self) {
        self.selected_actor = None;
        self.selected_variant = None;
        self.pinned = None;
    }
}

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
    pub live: LiveNodeState,
}

impl SolveTabState {
    pub fn new(seat_names: Vec<String>) -> Self {
        Self {
            seat_names,
            history: Vec::new(),
            latest_evaluation: None,
            status: RunStatus::Idle,
            live: LiveNodeState::default(),
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

    let seat_names = state.seat_names.clone();
    egui::CollapsingHeader::new("Live node view")
        .default_open(true)
        .show(ui, |ui| {
            live_node_ui(ui, &seat_names, &mut state.live, worker)
        });
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

fn seat_label_at(seat_names: &[String], seat: u8) -> String {
    seat_names
        .get(seat as usize)
        .cloned()
        .unwrap_or_else(|| format!("Seat {seat}"))
}

/// `true` when no `raise-to:`/`bet-to:` action appears anywhere on the
/// root-to-node path, matching `results::ResultsState::is_unopened`'s
/// semantics (a `call:` at such a node is a limp, not a call facing a
/// raise) -- but reading it straight off the snapshot's already-resolved
/// "SEAT action" path instead of walking history links.
fn snapshot_is_unopened(path: &[String]) -> bool {
    !path
        .iter()
        .any(|entry| entry.contains("raise-to:") || entry.contains("bet-to:"))
}

fn live_node_ui(
    ui: &mut Ui,
    seat_names: &[String],
    live: &mut LiveNodeState,
    worker: Option<&WorkerHandle>,
) {
    let mut watch: Option<[u8; 16]> = None;

    ui.horizontal_wrapped(|ui| {
        let mut jump_to = None;
        for (index, (_, label)) in live.breadcrumb.iter().enumerate() {
            if ui.button(label).clicked() {
                jump_to = Some(index);
            }
            ui.label(">");
        }
        if let Some(index) = jump_to {
            live.breadcrumb.truncate(index + 1);
            live.reset_selection();
            watch = Some(live.current_history());
        }
    });

    // Cloned so the rest of this function can mutate `live` (selection,
    // hover/pin) without holding a borrow of `live.snapshot` alive.
    let Some(snapshot) = live.snapshot.clone() else {
        ui.label("Waiting for the first live snapshot...");
        if let (Some(worker), Some(key)) = (worker, watch) {
            worker.send(WorkerCmd::WatchNode(Some(key)));
        }
        return;
    };
    let snapshot = snapshot.as_ref();

    ui.label(format!(
        "平均戦略(暫定)@ {} sweeps — approximate profile",
        snapshot.sweeps
    ));

    ui.horizontal_wrapped(|ui| {
        ui.label("Children:");
        let mut descend = None;
        for child in &snapshot.children {
            let label = format!(
                "{} {}",
                seat_label_at(seat_names, child.actor),
                child.action
            );
            if ui.button(&label).clicked() {
                descend = Some((child.key, label));
            }
        }
        if let Some((key, label)) = descend {
            live.breadcrumb.push((key, label));
            live.reset_selection();
            watch = Some(key);
        }
    });
    ui.separator();

    let mut actors: Vec<u8> = snapshot.blocks.iter().map(|block| block.actor).collect();
    actors.sort_unstable();
    actors.dedup();
    if actors.is_empty() {
        ui.label("No strategy blocks at this node yet.");
    } else {
        if live
            .selected_actor
            .is_none_or(|actor| !actors.contains(&actor))
        {
            live.selected_actor = Some(actors[0]);
        }
        if actors.len() > 1 {
            ui.horizontal(|ui| {
                ui.label("Actor:");
                for &actor in &actors {
                    let selected = live.selected_actor == Some(actor);
                    if ui
                        .selectable_label(selected, seat_label_at(seat_names, actor))
                        .clicked()
                    {
                        live.selected_actor = Some(actor);
                        live.selected_variant = None;
                        live.pinned = None;
                    }
                }
            });
        }
        let actor = live.selected_actor.expect("set above");

        let mut variants: Vec<(u8, u8)> = snapshot
            .blocks
            .iter()
            .filter(|block| block.actor == actor)
            .map(|block| (block.street, block.active_opponents))
            .collect();
        variants.sort_unstable();
        variants.dedup();
        if variants.is_empty() {
            ui.label("No strategy blocks for this actor here.");
        } else {
            if live
                .selected_variant
                .is_none_or(|variant| !variants.contains(&variant))
            {
                live.selected_variant = Some(variants[0]);
            }
            if variants.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label("Node variant:");
                    for &(street, active_opponents) in &variants {
                        let label = format!("street {street} / {active_opponents} active opp.");
                        let selected = live.selected_variant == Some((street, active_opponents));
                        if ui.selectable_label(selected, label).clicked() {
                            live.selected_variant = Some((street, active_opponents));
                            live.pinned = None;
                        }
                    }
                });
            }
            let (street, active_opponents) = live.selected_variant.expect("set above");

            ui.separator();
            ui.columns(2, |columns| {
                let matrix_ui = &mut columns[0];
                if street == 0 {
                    live_preflop_matrix(matrix_ui, live, snapshot, actor, active_opponents);
                } else {
                    live_bucket_grid(matrix_ui, live, snapshot, actor, street, active_opponents);
                }
                live_detail_panel(&mut columns[1], live, snapshot, street, active_opponents);
            });
        }
    }

    if let (Some(worker), Some(key)) = (worker, watch) {
        worker.send(WorkerCmd::WatchNode(Some(key)));
    }
}

fn live_preflop_matrix(
    ui: &mut Ui,
    live: &mut LiveNodeState,
    snapshot: &NodeSnapshot,
    actor: u8,
    active_opponents: u8,
) {
    let unopened = snapshot_is_unopened(&snapshot.path);
    let blocks_by_class: std::collections::HashMap<u32, &NodeBlock> = snapshot
        .blocks
        .iter()
        .filter(|block| {
            block.actor == actor && block.street == 0 && block.active_opponents == active_opponents
        })
        .map(|block| (block.bucket_path[0], block))
        .collect();

    let cell_size = Vec2::new(30.0, 20.0);
    let (response, _painter) = ui.allocate_painter(
        Vec2::new(cell_size.x * 13.0, cell_size.y * 13.0),
        egui::Sense::hover(),
    );
    let origin = response.rect.min;
    let pointer = ui.ctx().pointer_hover_pos();
    let mut hovered = None;
    for class in 0..169u32 {
        let row = (class / 13) as usize;
        let col = (class % 13) as usize;
        let rect = Rect::from_min_size(
            origin + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y),
            cell_size,
        );
        let block = blocks_by_class.get(&class).copied();
        let data = block.map(|block| CellData {
            label: matrix::class_label(class as usize),
            actions: &block.actions,
            probabilities: &block.probabilities,
            unopened,
        });
        let cell_response = matrix::cell(ui, rect, data.as_ref());
        if let Some(block) = block {
            let id = (actor, 0u8, active_opponents, block.bucket_path);
            if let Some(pointer) = pointer
                && rect.contains(pointer)
            {
                hovered = Some(id);
            }
            if cell_response.clicked() {
                live.pinned = Some(id);
            }
        }
    }
    live.hovered = hovered;
}

fn live_bucket_grid(
    ui: &mut Ui,
    live: &mut LiveNodeState,
    snapshot: &NodeSnapshot,
    actor: u8,
    street: u8,
    active_opponents: u8,
) {
    let mut blocks: Vec<&NodeBlock> = snapshot
        .blocks
        .iter()
        .filter(|block| {
            block.actor == actor
                && block.street == street
                && block.active_opponents == active_opponents
        })
        .collect();
    blocks.sort_unstable_by_key(|block| block.bucket_path);
    if blocks.is_empty() {
        ui.label("No buckets at this node.");
        return;
    }
    let cell_size = Vec2::new(56.0, 20.0);
    let columns = 12usize;
    let rows = blocks.len().div_ceil(columns);
    let (response, _painter) = ui.allocate_painter(
        Vec2::new(cell_size.x * columns as f32, cell_size.y * rows as f32),
        egui::Sense::hover(),
    );
    let origin = response.rect.min;
    let pointer = ui.ctx().pointer_hover_pos();
    let mut hovered = None;
    for (index, block) in blocks.iter().enumerate() {
        let row = index / columns;
        let col = index % columns;
        let rect = Rect::from_min_size(
            origin + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y),
            cell_size,
        );
        let bucket = block.bucket_path[street as usize];
        let data = CellData {
            label: format!("B{bucket}"),
            actions: &block.actions,
            probabilities: &block.probabilities,
            unopened: false,
        };
        let cell_response = matrix::cell(ui, rect, Some(&data));
        let id = (actor, street, active_opponents, block.bucket_path);
        if let Some(pointer) = pointer
            && rect.contains(pointer)
        {
            hovered = Some(id);
        }
        if cell_response.clicked() {
            live.pinned = Some(id);
        }
    }
    live.hovered = hovered;
}

fn live_detail_panel(
    ui: &mut Ui,
    live: &LiveNodeState,
    snapshot: &NodeSnapshot,
    street: u8,
    active_opponents: u8,
) {
    ui.heading("Detail");
    ui.label(format!(
        "history: {} | active opponents: {active_opponents} | street: {street}",
        hex(&snapshot.history)
    ));
    let selected = live.pinned.or(live.hovered);
    if let Some((actor, street, active_opponents, bucket_path)) = selected {
        let block = snapshot.blocks.iter().find(|block| {
            block.actor == actor
                && block.street == street
                && block.active_opponents == active_opponents
                && block.bucket_path == bucket_path
        });
        if let Some(block) = block {
            let label = if street == 0 {
                matrix::class_label(bucket_path[0] as usize)
            } else {
                format!("B{}", bucket_path[street as usize])
            };
            ui.label(egui::RichText::new(label).monospace().strong());
            let unopened = street == 0 && snapshot_is_unopened(&snapshot.path);
            let colors = matrix::action_colors(&block.actions, unopened);
            for ((action, probability), color) in
                block.actions.iter().zip(&block.probabilities).zip(&colors)
            {
                ui.horizontal(|ui| {
                    let (rect, _response) =
                        ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, *color);
                    ui.monospace(format!("{action:<16} {:>5.1}%", probability * 100.0));
                });
            }
        } else {
            ui.label("(no strategy block for this cell)");
        }
    } else {
        ui.label("Hover or click a cell to see its action breakdown.");
    }
}

fn hex(bytes: &[u8; 16]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
