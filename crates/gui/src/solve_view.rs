//! Solve tab: status strip, Pause/Resume/Finish/Cancel controls, live
//! convergence charts (per-seat log10 avg-positive-regret and strategy
//! drift vs sweeps), the "Live node view" (public-history browser over the
//! in-progress average strategy, its range-wide action-frequency aggregate,
//! and on-demand per-hand/per-action EVs), and the evaluation table.

use eframe::egui;
use egui::{Rect, Ui, Vec2};
use egui_plot::{Line, Plot, PlotPoints};
use multiway::solver::{NodeActionEvaluation, ProfileEvaluation};

use crate::format;
use crate::frequency::{self, FrequencyBlock};
use crate::matrix::{self, CellData};
use crate::status::Level;
use crate::theme;
use crate::worker::{
    FinishedOutcome, NodeBlock, NodeSnapshot, ProgressSnapshot, StopRuleClock, StopRuleProgress,
    WorkerCmd, WorkerHandle,
};

/// Cap plotted points per line; history beyond this is decimated (recent
/// samples are always kept, older ones are subsampled evenly).
const MAX_PLOT_POINTS: usize = 4_000;

const ROOT: [u8; 16] = [0; 16];

/// Identity of one [`NodeBlock`] within a snapshot, used for hover/pin state
/// (a snapshot has no stable indices across refreshes, so the identity is
/// the block's own key fields rather than a `Vec` position).
type BlockId = (u8, u8, u8, [u32; 4]);

/// State of the on-demand "Evaluate EVs" request for the currently displayed
/// node. Carries its own `path` copy in `Ready`/`Failed` so the render side
/// can double-check it against `LiveNodeState::path` even though
/// `App::poll_worker` already drops a reply for a node no longer shown.
#[derive(Clone, Debug, Default)]
pub enum NodeEvalState {
    #[default]
    Idle,
    Pending,
    Ready {
        path: Vec<usize>,
        evaluation: NodeActionEvaluation,
    },
    Failed {
        path: Vec<usize>,
        error: String,
    },
}

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
    /// Root-to-current action-index path, parallel to `breadcrumb[1..]` --
    /// `WorkerCmd::EvaluateNode`'s `path` argument. Tracked alongside
    /// `breadcrumb` (pushed on descend, truncated on a breadcrumb jump)
    /// rather than derived from `snapshot`, so "Evaluate EVs" always targets
    /// the node currently on screen even before its first snapshot arrives.
    pub path: Vec<usize>,
    pub snapshot: Option<Box<NodeSnapshot>>,
    pub selected_actor: Option<u8>,
    /// `(street, active_opponents)`, when a node has more than one variant.
    pub selected_variant: Option<(u8, u8)>,
    pub hovered: Option<BlockId>,
    pub pinned: Option<BlockId>,
    /// Sample count for the next "Evaluate EVs" request; default matches the
    /// deliverable's documented default.
    pub eval_samples: u64,
    pub eval: NodeEvalState,
}

impl Default for LiveNodeState {
    fn default() -> Self {
        Self {
            breadcrumb: vec![(ROOT, "ROOT".to_string())],
            path: Vec::new(),
            snapshot: None,
            selected_actor: None,
            selected_variant: None,
            hovered: None,
            pinned: None,
            eval_samples: 4_096,
            eval: NodeEvalState::Idle,
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

    /// Truncates the breadcrumb/path to the node at breadcrumb index
    /// `index` (a breadcrumb click), dropping deeper entries; drops any
    /// in-flight/completed EV evaluation, since it answers a node this view
    /// no longer shows.
    fn jump_to(&mut self, index: usize) {
        self.breadcrumb.truncate(index + 1);
        self.path.truncate(index);
        self.reset_selection();
        self.eval = NodeEvalState::Idle;
    }

    /// Pushes one more edge (a descend into a child node); same
    /// invalidation as [`Self::jump_to`].
    fn descend(&mut self, key: [u8; 16], label: String, action_index: usize) {
        self.breadcrumb.push((key, label));
        self.path.push(action_index);
        self.reset_selection();
        self.eval = NodeEvalState::Idle;
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

    /// Status-line severity: a worker failure is an error, a cancel is a
    /// (user-requested, non-fatal) warning, everything else is informational
    /// -- see `crate::status`.
    fn level(&self) -> crate::status::Level {
        match self {
            RunStatus::Failed(_) => crate::status::Level::Error,
            RunStatus::Cancelled => crate::status::Level::Warning,
            _ => crate::status::Level::Info,
        }
    }
}

/// Live view of the convergence stop rule (`run.stop_dev_gain`), merged from
/// two worker event streams: [`StopRuleClock`] (sent on every
/// `WorkerEvent::Progress`, drives the between-evaluations countdown) and
/// [`StopRuleProgress`] (sent only on the rarer `WorkerEvent::Evaluated`
/// where the evaluation was itself a stop-rule check, drives the actual
/// pass/fail readout). `None` on `SolveTabState` until the first `Progress`
/// snapshot confirms a stop rule is configured for this run.
#[derive(Clone, Debug)]
pub struct StopRuleView {
    pub threshold: f64,
    pub confirmations_required: u32,
    pub next_eval_in_secs: f64,
    pub confirmations: u32,
    pub sample_count: u64,
    /// `None` until the first stop-rule evaluation has run.
    pub max_ci_upper: Option<f64>,
}

pub struct SolveTabState {
    pub seat_names: Vec<String>,
    pub history: Vec<ProgressSnapshot>,
    pub latest_evaluation: Option<ProfileEvaluation>,
    pub status: RunStatus,
    pub live: LiveNodeState,
    pub stop_rule: Option<StopRuleView>,
    /// How the run stopped, once `WorkerEvent::Finished` arrives; kept
    /// around (not cleared by a tab switch) so re-opening the Solve tab
    /// after `App` jumps to Results still shows why the run ended.
    pub finished_outcome: Option<FinishedOutcome>,
}

impl SolveTabState {
    pub fn new(seat_names: Vec<String>) -> Self {
        Self {
            seat_names,
            history: Vec::new(),
            latest_evaluation: None,
            status: RunStatus::Idle,
            live: LiveNodeState::default(),
            stop_rule: None,
            finished_outcome: None,
        }
    }

    /// Applies a `WorkerEvent::Progress` snapshot's stop-rule clock: `None`
    /// clears the panel (no rule configured), `Some` updates the threshold/
    /// countdown fields while leaving any already-known confirmations/
    /// sample-count/CI readout untouched.
    pub fn on_progress_stop_rule(&mut self, clock: Option<StopRuleClock>) {
        let Some(clock) = clock else {
            self.stop_rule = None;
            return;
        };
        let view = self.stop_rule.get_or_insert(StopRuleView {
            threshold: clock.threshold,
            confirmations_required: clock.confirmations_required,
            next_eval_in_secs: clock.next_eval_in_secs,
            confirmations: 0,
            sample_count: 0,
            max_ci_upper: None,
        });
        view.threshold = clock.threshold;
        view.confirmations_required = clock.confirmations_required;
        view.next_eval_in_secs = clock.next_eval_in_secs;
    }

    /// Applies a `WorkerEvent::Evaluated` reply's stop-rule progress, if the
    /// evaluation that produced it was a stop-rule check.
    pub fn on_evaluated_stop_rule(&mut self, progress: Option<StopRuleProgress>) {
        let Some(progress) = progress else {
            return;
        };
        let view = self.stop_rule.get_or_insert(StopRuleView {
            threshold: progress.threshold,
            confirmations_required: progress.confirmations_required,
            next_eval_in_secs: 0.0,
            confirmations: 0,
            sample_count: 0,
            max_ci_upper: None,
        });
        view.threshold = progress.threshold;
        view.confirmations_required = progress.confirmations_required;
        view.confirmations = progress.confirmations;
        view.sample_count = progress.sample_count;
        view.max_ci_upper = Some(progress.max_ci_upper);
    }
}

/// Status-line text/level for a finished run's outcome (see
/// `worker::FinishedOutcome`), shown on the Solve tab once
/// `WorkerEvent::Finished` arrives.
fn outcome_message(outcome: FinishedOutcome) -> (Level, String) {
    match outcome {
        FinishedOutcome::TargetReached => {
            (Level::Info, "finished: sweep target reached".to_string())
        }
        FinishedOutcome::UserRequested => (
            Level::Info,
            "finished: stopped and saved by request".to_string(),
        ),
        FinishedOutcome::Converged {
            max_ci_upper,
            threshold,
            confirmations,
        } => (
            Level::Info,
            format!(
                "converged: max devGainLB CI upper {} < {} bb, confirmed {confirmations}x",
                format::bb(max_ci_upper),
                format::bb(threshold)
            ),
        ),
        FinishedOutcome::WallTimeCap => (Level::Warning, "wall-time cap reached".to_string()),
    }
}

/// Renders the convergence-stop-rule panel: max CI upper vs threshold
/// (colored by whether it currently passes), confirmations so far, the
/// current adaptive sample count, and a countdown to the next evaluation.
/// Shown whenever a stop rule is active (`state.stop_rule.is_some()`).
fn stop_rule_ui(ui: &mut Ui, stop_rule: &StopRuleView) {
    ui.separator();
    ui.label(egui::RichText::new("Convergence stop rule").strong());
    let level = match stop_rule.max_ci_upper {
        Some(upper) if upper < stop_rule.threshold => Level::Info,
        Some(_) => Level::Warning,
        None => Level::Info,
    };
    let upper_text = stop_rule
        .max_ci_upper
        .map(format::bb)
        .unwrap_or_else(|| "-".to_string());
    crate::status::show(
        ui,
        level,
        format!(
            "max devGainLB CI upper {upper_text} vs threshold {}",
            format::bb(stop_rule.threshold)
        ),
    );
    ui.monospace(format!(
        "confirmations {}/{} | samples {} | next check in {}",
        stop_rule.confirmations,
        stop_rule.confirmations_required,
        format::human_count(stop_rule.sample_count as f64),
        format::elapsed(stop_rule.next_eval_in_secs)
    ));
}

pub fn ui(ui: &mut Ui, state: &mut SolveTabState, worker: Option<&WorkerHandle>) {
    crate::status::show(
        ui,
        state.status.level(),
        format!("status: {}", state.status.label()),
    );
    if let Some(latest) = state.history.last() {
        ui.add(
            egui::ProgressBar::new((latest.sweeps as f32 / latest.target.max(1) as f32).min(1.0))
                .text(format!("{}/{}", latest.sweeps, latest.target)),
        );
        // Aligned label-over-value grid (monospace numbers) so the
        // throughput strip reads as a scannable dashboard rather than a run
        // of loosely separated labels.
        egui::Grid::new("solve-metrics-grid")
            .striped(false)
            .show(ui, |ui| {
                ui.label("sweeps/s");
                ui.label("traversals/s");
                // Hand-updates/s is the headline throughput metric (the
                // number to compare against a range-based solver's
                // "hands/s"), so its header stays bold.
                ui.label(egui::RichText::new("hands/s").strong());
                ui.label("infosets");
                ui.label("mem");
                ui.label("elapsed");
                ui.end_row();
                ui.monospace(format::human_rate(latest.sweeps_per_sec));
                ui.monospace(format::human_rate(latest.traversals_per_sec));
                ui.monospace(
                    egui::RichText::new(format::human_rate(latest.hand_updates_per_sec)).strong(),
                );
                ui.monospace(format::human_count(latest.infosets as f64));
                ui.monospace(format::memory_mib(latest.memory_bytes));
                ui.monospace(format::elapsed(latest.elapsed_secs));
                ui.end_row();
            });
    }
    if let Some(stop_rule) = &state.stop_rule {
        stop_rule_ui(ui, stop_rule);
    }
    if let Some(outcome) = state.finished_outcome {
        let (level, message) = outcome_message(outcome);
        crate::status::show(ui, level, message);
    }

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

    // Both plots use the same per-seat color (see `theme::seat_color`), so a
    // seat's regret and drift lines are visually the same series everywhere,
    // and `egui_plot`'s built-in legend swatch already matches it exactly.
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
                plot_ui
                    .line(Line::new(seat_name(state, seat), points).color(theme::seat_color(seat)));
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
                plot_ui
                    .line(Line::new(seat_name(state, seat), points).color(theme::seat_color(seat)));
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
                    ui.monospace(format::ev_bb(estimate.mean));
                    ui.monospace(format!(
                        "[{}, {}]",
                        format::ev_bb(estimate.ci95[0]),
                        format::ev_bb(estimate.ci95[1])
                    ));
                    let gain = evaluation
                        .deviation_gain_lower_bound
                        .as_ref()
                        .and_then(|values| values.get(seat))
                        .map(|value| format::ev_bb(value.mean))
                        .unwrap_or_else(|| "-".to_string());
                    ui.monospace(gain);
                    ui.end_row();
                }
            });
    } else {
        ui.label("no evaluation yet");
    }

    ui.separator();
    crate::status::show(
        ui,
        crate::status::Level::Info,
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

/// Range-wide action-frequency aggregate for one `(actor, street,
/// active_opponents)` node variant: `Σ_b mass_b · σ̄_b(action) / Σ_b mass_b`
/// over that variant's blocks, using each block's linear-CFR strategy mass.
/// `None` when the node has no blocks yet or none has accumulated any mass.
fn node_aggregate(
    snapshot: &NodeSnapshot,
    actor: u8,
    street: u8,
    active_opponents: u8,
) -> Option<(Vec<String>, Vec<f64>)> {
    let blocks: Vec<&NodeBlock> = snapshot
        .blocks
        .iter()
        .filter(|block| {
            block.actor == actor
                && block.street == street
                && block.active_opponents == active_opponents
        })
        .collect();
    let action_labels = blocks.first()?.actions.clone();
    let frequency_blocks: Vec<FrequencyBlock<'_>> = blocks
        .iter()
        .map(|block| FrequencyBlock {
            action_labels: &block.actions,
            probabilities: &block.probabilities,
            mass: block.mass,
        })
        .collect();
    let frequencies = frequency::aggregate_action_frequencies(&action_labels, &frequency_blocks)?;
    Some((action_labels, frequencies))
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
            live.jump_to(index);
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
                descend = Some((child.key, label, child.action_index));
            }
        }
        if let Some((key, label, action_index)) = descend {
            live.descend(key, label, action_index);
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

            let unopened = street == 0 && snapshot_is_unopened(&snapshot.path);
            let aggregate = node_aggregate(snapshot, actor, street, active_opponents);
            matrix::aggregate_row(ui, aggregate.as_ref(), unopened);
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

            ui.separator();
            eval_controls_ui(ui, live, worker, seat_names, street, unopened);
        }
    }

    if let (Some(worker), Some(key)) = (worker, watch) {
        worker.send(WorkerCmd::WatchNode(Some(key)));
    }
}

/// "Evaluate EVs" button, sample-count field, and (once a reply arrives for
/// the node currently shown) the shared per-hand/per-action EV table.
/// `unopened` is threaded through to [`crate::eval_table::ui`] so its action
/// column headers use the same fold/limp/raise-ramp colors as the matrix.
fn eval_controls_ui(
    ui: &mut Ui,
    live: &mut LiveNodeState,
    worker: Option<&WorkerHandle>,
    seat_names: &[String],
    street: u8,
    unopened: bool,
) {
    ui.horizontal(|ui| {
        ui.label("EV samples:");
        ui.add(egui::DragValue::new(&mut live.eval_samples).range(1..=1_000_000));
        if ui.button("Evaluate EVs").clicked()
            && let Some(worker) = worker
        {
            worker.send(WorkerCmd::EvaluateNode {
                path: live.path.clone(),
                samples: live.eval_samples,
            });
            live.eval = NodeEvalState::Pending;
        }
        match &live.eval {
            NodeEvalState::Idle | NodeEvalState::Ready { .. } => {}
            NodeEvalState::Pending => {
                ui.spinner();
                ui.label("evaluating...");
            }
            NodeEvalState::Failed { error, .. } => {
                crate::status::show(
                    ui,
                    crate::status::Level::Error,
                    format!("evaluation failed: {error}"),
                );
            }
        }
    });
    if let NodeEvalState::Ready { path, evaluation } = &live.eval
        && *path == live.path
    {
        let actor_label = seat_label_at(seat_names, evaluation.actor as u8);
        crate::eval_table::ui(ui, evaluation, street == 0, &actor_label, unopened);
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
            combo_count: Some(matrix::class_combo_count(class as usize)),
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
            combo_count: None,
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
            matrix::action_breakdown_rows(ui, &block.actions, &block.probabilities, &colors);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_stop_rule_clears_the_panel_when_the_rule_is_not_configured() {
        let mut state = SolveTabState::new(vec!["BTN".to_string()]);
        state.on_progress_stop_rule(Some(StopRuleClock {
            threshold: 0.25,
            confirmations_required: 2,
            next_eval_in_secs: 10.0,
        }));
        assert!(state.stop_rule.is_some());
        state.on_progress_stop_rule(None);
        assert!(state.stop_rule.is_none());
    }

    #[test]
    fn progress_stop_rule_updates_the_countdown_without_disturbing_known_progress() {
        let mut state = SolveTabState::new(Vec::new());
        state.on_evaluated_stop_rule(Some(StopRuleProgress {
            max_ci_upper: 0.5,
            threshold: 0.25,
            confirmations: 1,
            confirmations_required: 2,
            sample_count: 512,
        }));
        state.on_progress_stop_rule(Some(StopRuleClock {
            threshold: 0.25,
            confirmations_required: 2,
            next_eval_in_secs: 12.0,
        }));
        let view = state.stop_rule.expect("stop rule must be tracked by now");
        assert_eq!(view.next_eval_in_secs, 12.0);
        // The countdown update must not clobber the confirmations/sample
        // count/CI readout the last `Evaluated` event reported.
        assert_eq!(view.confirmations, 1);
        assert_eq!(view.sample_count, 512);
        assert_eq!(view.max_ci_upper, Some(0.5));
    }

    #[test]
    fn evaluated_stop_rule_is_a_no_op_for_an_ordinary_cadence_evaluation() {
        let mut state = SolveTabState::new(Vec::new());
        state.on_evaluated_stop_rule(None);
        assert!(state.stop_rule.is_none());
    }

    #[test]
    fn outcome_messages_cover_every_variant_with_a_sensible_level() {
        assert_eq!(
            outcome_message(FinishedOutcome::TargetReached).0,
            Level::Info
        );
        assert_eq!(
            outcome_message(FinishedOutcome::UserRequested).0,
            Level::Info
        );
        assert_eq!(
            outcome_message(FinishedOutcome::WallTimeCap).0,
            Level::Warning
        );
        let (level, message) = outcome_message(FinishedOutcome::Converged {
            max_ci_upper: 0.12,
            threshold: 0.25,
            confirmations: 2,
        });
        assert_eq!(level, Level::Info);
        assert!(message.contains("converged"));
        assert!(message.contains("confirmed 2x"));
    }
}
