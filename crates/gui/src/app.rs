//! Top-level eframe app: tab bar (Setup / Solve / Results), worker
//! lifecycle, and the glue between the three tab modules.

use eframe::egui;

use crate::worker::{self, WorkerCmd, WorkerEvent, WorkerHandle};
use crate::{presets, results, setup, solve_view, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Setup,
    Solve,
    Results,
}

const PRESET_DIR_KEY: &str = "gui.user_preset_dir";

pub struct App {
    tab: Tab,
    setup: setup::SetupState,
    worker: Option<WorkerHandle>,
    solve: solve_view::SolveTabState,
    results: Option<results::ResultsState>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let user_dir = cc
            .storage
            .and_then(|storage| storage.get_string(PRESET_DIR_KEY))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(presets::default_user_dir);
        Self {
            tab: Tab::Setup,
            setup: setup::SetupState::new(user_dir),
            worker: None,
            solve: solve_view::SolveTabState::new(Vec::new()),
            results: None,
        }
    }

    fn poll_worker(&mut self, ctx: &egui::Context) {
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        let mut drop_worker = false;
        while let Ok(event) = worker.events.try_recv() {
            match event {
                WorkerEvent::Building => self.solve.status = solve_view::RunStatus::Building,
                WorkerEvent::Progress(snapshot) => {
                    self.solve.status = solve_view::RunStatus::Running;
                    self.solve.history.push(snapshot);
                }
                WorkerEvent::Evaluated(evaluation) => {
                    self.solve.latest_evaluation = Some(evaluation);
                }
                WorkerEvent::NodeStrategies(snapshot) => {
                    self.solve.live.snapshot = Some(snapshot);
                }
                WorkerEvent::NodeEvaluation(result) => {
                    // Stale-guard: only keep a reply for the node the Live
                    // node view is still showing (the user may have
                    // navigated away while the evaluation was in flight).
                    if result.path == self.solve.live.path {
                        self.solve.live.eval = solve_view::NodeEvalState::Ready {
                            path: result.path,
                            evaluation: result.evaluation,
                        };
                    }
                }
                WorkerEvent::NodeEvaluationFailed { path, error } => {
                    if path == self.solve.live.path {
                        self.solve.live.eval = solve_view::NodeEvalState::Failed { path, error };
                    }
                }
                WorkerEvent::Finished(finished) => {
                    self.solve.status = solve_view::RunStatus::Finished;
                    self.solve.live = solve_view::LiveNodeState::default();
                    self.results = Some(results::ResultsState::new(
                        finished.solution,
                        Some(finished.mwsol_path),
                    ));
                    self.tab = Tab::Results;
                    drop_worker = true;
                }
                WorkerEvent::Cancelled => {
                    self.solve.status = solve_view::RunStatus::Cancelled;
                    self.solve.live = solve_view::LiveNodeState::default();
                    drop_worker = true;
                }
                WorkerEvent::Failed(error) => {
                    self.solve.status = solve_view::RunStatus::Failed(error);
                    self.solve.live = solve_view::LiveNodeState::default();
                    drop_worker = true;
                }
            }
        }
        if drop_worker {
            self.worker = None;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_worker(&ctx);

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Setup, "Setup");
                ui.selectable_value(&mut self.tab, Tab::Solve, "Solve");
                ui.add_enabled_ui(self.results.is_some(), |ui| {
                    ui.selectable_value(&mut self.tab, Tab::Results, "Results");
                });
                ui.separator();
                if self.worker.is_some() {
                    ui.colored_label(theme::ACCENT, "\u{25cf} solving");
                } else {
                    ui.label("idle");
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Setup => {
                if let Some(request) = setup::ui(ui, &mut self.setup) {
                    self.start_solve(request, ctx.clone());
                }
            }
            Tab::Solve => solve_view::ui(ui, &mut self.solve, self.worker.as_ref()),
            Tab::Results => {
                if let Some(results) = self.results.as_mut() {
                    results::ui(ui, results);
                } else {
                    ui.label("No results yet. Finish a solve, or open a .mwsol file.");
                }
                if ui.button("Open .mwsol...").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("mwsol", &["mwsol"])
                        .pick_file()
                {
                    match formats::read_mwsol(&path) {
                        Ok(solution) => {
                            self.results = Some(results::ResultsState::new(solution, Some(path)))
                        }
                        Err(error) => {
                            self.solve.status = solve_view::RunStatus::Failed(error.to_string())
                        }
                    }
                }
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            PRESET_DIR_KEY,
            self.setup.user_preset_dir.display().to_string(),
        );
    }
}

impl App {
    fn start_solve(&mut self, request: setup::StartRequest, ctx: egui::Context) {
        let seat_names: Vec<String> = self
            .setup
            .model
            .seats
            .iter()
            .map(|seat| seat.name.clone())
            .collect();
        self.solve = solve_view::SolveTabState::new(seat_names);
        self.solve.status = solve_view::RunStatus::Building;
        self.tab = Tab::Solve;
        let target = worker::RunTarget {
            config_toml: request.config_toml,
            resume_checkpoint: request.resume_checkpoint,
            output_path: request.output_path,
            checkpoint_path: request.checkpoint_path,
            check_every: request.check_every,
        };
        let worker = worker::spawn(target, ctx);
        // Live node view starts at ROOT; the worker answers with a fresh
        // `NodeSnapshot` after the current chunk (or immediately, if it is
        // still building or paused).
        worker.send(WorkerCmd::WatchNode(Some([0; 16])));
        self.worker = Some(worker);
    }
}
