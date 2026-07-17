//! Setup tab: preset browser (left), editable config form (center), live
//! validation + "Start solve" (right). See `docs/native-gui-plan.md`
//! section F.

use std::path::PathBuf;
use std::str::FromStr;

use eframe::egui;
use egui::Ui;

use crate::estimate::{AutoPreviewState, EstimatePanelState};
use crate::machine;
use crate::model::{self, Model, QualityPreset};
use crate::presets::{self, PresetEntry};
use crate::size_lexer;
use crate::status::{self, Level};

/// Setup tab view mode: a pure GUI-state filter over an always-complete
/// `Model` (see the Auto-mode phase B deliverable) -- switching modes never
/// changes the model, it only changes which sections are drawn. Auto is the
/// default for a brand-new setup; loading a preset/TOML picks whichever mode
/// matches its shape (see `model::matches_auto_shape`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupMode {
    Auto,
    Advanced,
}

pub struct SetupState {
    pub model: Model,
    pub mode: SetupMode,
    /// Auto mode's convergence-quality picker; irrelevant in Advanced (which
    /// edits `model.run.stop_dev_gain` and friends directly instead).
    pub quality: QualityPreset,
    pub user_preset_dir: PathBuf,
    pub presets: Vec<PresetEntry>,
    pub new_preset_name: String,
    /// Last preset-panel outcome (load/save/import/export/delete), if any --
    /// paired with its severity so a real failure reads as an error and a
    /// confirmation ("saved", "deleted") reads as informational, rather than
    /// both sharing one ad-hoc color (see `crate::status`).
    pub status_message: Option<(Level, String)>,
    pub confirm_delete: Option<PathBuf>,
    pub copy_source: usize,
    pub validation_errors: Vec<String>,
    /// Background dense-arena estimate for the current model (both modes;
    /// Auto embeds it in the derived-settings summary, Advanced has its own
    /// collapsible section). See `crate::estimate`.
    pub estimate: EstimatePanelState,
    /// Background `cli::auto_run::derive_auto_run` preview for Auto mode's
    /// derived-settings summary (threads/sweep_batch/buckets/estimated
    /// bytes) -- what pressing Solve will materialize into the model.
    pub auto_preview: AutoPreviewState,
}

/// Everything the worker needs to start a solve: the config TOML plus the
/// GUI-only artifact destinations (see `worker::RunTarget`).
pub struct StartRequest {
    pub config_toml: String,
    pub resume_checkpoint: Option<PathBuf>,
    pub output_path: PathBuf,
    pub checkpoint_path: Option<PathBuf>,
    pub check_every: u64,
    pub max_wall_time_secs: Option<f64>,
}

impl SetupState {
    pub fn new(user_preset_dir: PathBuf) -> Self {
        let presets = presets::list(&user_preset_dir);
        Self {
            model: Model::new_default(6),
            mode: SetupMode::Auto,
            quality: QualityPreset::Normal,
            user_preset_dir,
            presets,
            new_preset_name: String::new(),
            status_message: None,
            confirm_delete: None,
            copy_source: 0,
            validation_errors: Vec::new(),
            estimate: EstimatePanelState::new(),
            auto_preview: AutoPreviewState::new(),
        }
    }

    fn refresh_presets(&mut self) {
        self.presets = presets::list(&self.user_preset_dir);
    }

    fn set_status(&mut self, level: Level, message: impl Into<String>) {
        self.status_message = Some((level, message.into()));
    }

    fn load_toml(&mut self, text: &str) {
        match model::toml_to_model(text, &self.model.run) {
            Ok(model) => {
                self.mode = if model::matches_auto_shape(&model) {
                    SetupMode::Auto
                } else {
                    SetupMode::Advanced
                };
                self.model = model;
                self.status_message = None;
            }
            Err(error) => self.set_status(Level::Error, format!("load failed: {error}")),
        }
    }
}

/// Small, dimmed dependent-control hint (traverser_vector <-> street-recall,
/// EHS² greying, checkdown fields, ...): the same phrasing/weight everywhere
/// instead of each section picking its own plain-label style.
fn hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().color(crate::theme::HINT));
}

pub fn ui(ui: &mut Ui, state: &mut SetupState) -> Option<StartRequest> {
    state.validation_errors = model::validate(&state.model);

    egui::Panel::left("setup-presets")
        .resizable(true)
        .default_size(220.0)
        .show(ui, |ui| preset_panel(ui, state));

    let start = egui::Panel::right("setup-validation")
        .resizable(true)
        .default_size(260.0)
        .show(ui, |ui| validation_panel(ui, state))
        .inner;

    egui::CentralPanel::default().show(ui, |ui| {
        mode_toggle_ui(ui, state);
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            table_section(ui, state);
            seats_section(ui, state);
            betting_section(ui, state);
            match state.mode {
                SetupMode::Auto => {}
                SetupMode::Advanced => {
                    abstraction_section(ui, state);
                    estimate_panel_ui(ui, state);
                }
            }
            economics_section(ui, state);
            match state.mode {
                SetupMode::Auto => auto_panel_ui(ui, state),
                SetupMode::Advanced => {
                    algorithm_section(ui, state);
                    run_section(ui, state);
                }
            }
        });
    });

    start
}

/// Prominent Auto/Advanced selector at the top of the Setup tab. Switching
/// modes only changes which sections below are drawn -- the underlying
/// `Model` is always complete (see `SetupMode`'s doc comment).
fn mode_toggle_ui(ui: &mut Ui, state: &mut SetupState) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Mode:").strong());
        ui.selectable_value(&mut state.mode, SetupMode::Auto, "Auto");
        ui.selectable_value(&mut state.mode, SetupMode::Advanced, "Advanced");
    });
    match state.mode {
        SetupMode::Auto => hint(
            ui,
            "Auto derives the abstraction/algorithm/run settings below from your game \
             definition and this machine when you press Solve; nothing changes while you type.",
        ),
        SetupMode::Advanced => hint(
            ui,
            "Advanced exposes every abstraction/algorithm/run control directly.",
        ),
    }
}

/// Auto mode's own section: the Quality preset, an optional max-wall-time
/// cap, and the read-only derived-settings summary (item 2/3 of the
/// Auto-mode deliverable). Everything here is a preview of what pressing
/// "Start solve" will materialize into the model -- it never mutates
/// `state.model` itself (see `validation_panel`, which applies
/// `model::apply_auto_derivation` only on that click).
fn auto_panel_ui(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Auto settings", |ui| {
        ui.horizontal(|ui| {
            ui.label("Quality preset:");
            for preset in QualityPreset::ALL {
                ui.selectable_value(&mut state.quality, preset, preset.label());
            }
        });
        hint(
            ui,
            "Fast/Normal/High set the convergence stop rule's tightness (max deviation-gain \
             0.5/0.25/0.1 bb, 2 confirmations, checked every 30s).",
        );

        ui.horizontal(|ui| {
            let run = &mut state.model.run;
            let mut enabled = run.max_wall_time_minutes.is_some();
            if ui
                .checkbox(&mut enabled, "max wall time (minutes)")
                .changed()
            {
                run.max_wall_time_minutes =
                    enabled.then_some(run.max_wall_time_minutes.unwrap_or(60.0));
            }
            if let Some(minutes) = run.max_wall_time_minutes.as_mut() {
                ui.add(egui::DragValue::new(minutes).range(1.0..=100_000.0));
            }
        });
        hint(
            ui,
            "Optional: the worker finishes gracefully (solution + checkpoint written) once this \
             much wall-clock time has elapsed, same as pressing Finish & Save.",
        );

        ui.separator();
        ui.label(egui::RichText::new("Derived settings (applied on Solve)").strong());

        let threads = machine::detected_threads();
        let memory_budget = machine::half_of_total_memory_bytes();
        state
            .auto_preview
            .update(&state.model, threads, memory_budget);

        ui.monospace(format!("threads: {threads}"));
        ui.monospace(format!(
            "memory budget: {} (half of detected RAM)",
            crate::format::memory_mib(memory_budget)
        ));
        match state.auto_preview.result.as_ref() {
            Some((_, Ok(derivation))) => {
                ui.monospace(format!("sweep_batch: {}", derivation.sweep_batch));
                ui.monospace(format!(
                    "buckets (flop/turn/river): {}/{}/{}",
                    derivation.flop_buckets, derivation.turn_buckets, derivation.river_buckets
                ));
                ui.monospace(format!(
                    "estimated dense-arena memory: {} / budget {}",
                    crate::format::memory_mib(derivation.estimated_bytes),
                    crate::format::memory_mib(memory_budget)
                ));
                if crate::estimate::over_budget(derivation.estimated_bytes, memory_budget) {
                    status::show(
                        ui,
                        Level::Warning,
                        "even the smallest bucket-ladder rung (64) exceeds this budget; the \
                         derived config will still be applied, but expect a memory-limit stop.",
                    );
                }
            }
            Some((_, Err(error))) => hint(
                ui,
                &format!("derived-settings preview unavailable: {error}"),
            ),
            None => {
                ui.spinner();
                ui.label("computing derived settings...");
            }
        }
        ui.monospace(
            "model: recall=street, traverser_vector=true, abstraction=ehs2-table, \
             checkdown(flop/turn/river)=2, storage=i16",
        );
        ui.monospace(format!(
            "stop rule: max deviation-gain < {} bb, confirmations={}, checked every {}s",
            crate::format::bb(state.quality.stop_dev_gain()),
            state.quality.stop_confirmations(),
            state.quality.stop_eval_period_secs()
        ));
    });
}

/// Advanced mode's tree/memory estimate section (item 3 of the Auto-mode
/// deliverable): `estimate_dense_arena` for the current model compared
/// against `run.max_memory_mib` (`0` resolves to the engine's own default,
/// see `crate::estimate::resolve_budget_bytes`).
fn estimate_panel_ui(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Tree / memory estimate", |ui| {
        state.estimate.update(&state.model);
        if state.model.abstraction.recall == model::RecallKind::Full {
            hint(
                ui,
                "full recall: the estimate below is a dense-equivalent estimate; full recall \
                 itself is unbounded and grows with visited infosets instead.",
            );
        }
        let budget = crate::estimate::resolve_budget_bytes(state.model.run.max_memory_mib);
        match state.estimate.result.as_ref() {
            Some((_, Ok(estimate))) => {
                ui.monospace(format!(
                    "nodes: {}",
                    crate::format::human_count(estimate.node_count as f64)
                ));
                ui.monospace(format!(
                    "columns: {}",
                    crate::format::human_count(estimate.total_columns as f64)
                ));
                ui.monospace(format!(
                    "estimated arena memory: {} / budget {}",
                    crate::format::memory_mib(estimate.estimated_bytes),
                    crate::format::memory_mib(budget)
                ));
                if crate::estimate::over_budget(estimate.estimated_bytes, budget) {
                    status::show(
                        ui,
                        Level::Warning,
                        "estimated arena memory exceeds run.max_memory.",
                    );
                }
            }
            Some((_, Err(error))) => hint(ui, &format!("estimate unavailable: {error}")),
            None => {
                ui.spinner();
                ui.label("estimating...");
            }
        }
    });
}

fn preset_panel(ui: &mut Ui, state: &mut SetupState) {
    ui.heading("Presets");
    if ui.button("Change folder...").clicked()
        && let Some(dir) = rfd::FileDialog::new().pick_folder()
    {
        state.user_preset_dir = dir;
        state.refresh_presets();
    }
    ui.label(egui::RichText::new(state.user_preset_dir.display().to_string()).small());
    ui.separator();

    let mut to_load: Option<usize> = None;
    let mut to_delete: Option<PathBuf> = None;
    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            for (index, preset) in state.presets.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.button(preset.display_name()).clicked() {
                        to_load = Some(index);
                    }
                    if preset.is_user()
                        && let PresetEntry::User { path, .. } = preset
                        && ui.small_button("x").clicked()
                    {
                        to_delete = Some(path.clone());
                    }
                });
            }
        });
    if let Some(index) = to_load {
        match state.presets[index].load() {
            Ok(text) => state.load_toml(&text),
            Err(error) => state.set_status(Level::Error, format!("load failed: {error}")),
        }
    }
    if let Some(path) = to_delete {
        state.confirm_delete = Some(path);
    }
    if let Some(path) = state.confirm_delete.clone() {
        status::show(ui, Level::Warning, "Delete this preset?");
        ui.horizontal(|ui| {
            if ui.button("Yes, delete").clicked() {
                match presets::delete(&path) {
                    Ok(()) => {
                        state.refresh_presets();
                        state.set_status(Level::Info, "deleted");
                    }
                    Err(error) => state.set_status(Level::Error, format!("delete failed: {error}")),
                }
                state.confirm_delete = None;
            }
            if ui.button("Cancel").clicked() {
                state.confirm_delete = None;
            }
        });
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut state.new_preset_name);
        if ui.button("Save as").clicked() {
            match model::model_to_toml(&state.model) {
                Ok(text) => {
                    match presets::save(&state.user_preset_dir, &state.new_preset_name, &text) {
                        Ok(_) => {
                            state.refresh_presets();
                            state.set_status(Level::Info, "saved");
                        }
                        Err(error) => {
                            state.set_status(Level::Error, format!("save failed: {error}"))
                        }
                    }
                }
                Err(error) => state.set_status(Level::Error, format!("save failed: {error}")),
            }
        }
    });
    if ui.button("Import TOML...").clicked()
        && let Some(path) = rfd::FileDialog::new()
            .add_filter("toml", &["toml"])
            .pick_file()
    {
        match std::fs::read_to_string(&path) {
            Ok(text) => state.load_toml(&text),
            Err(error) => state.set_status(Level::Error, format!("import failed: {error}")),
        }
    }
    if ui.button("Export TOML...").clicked()
        && let Some(path) = rfd::FileDialog::new()
            .add_filter("toml", &["toml"])
            .save_file()
    {
        match model::model_to_toml(&state.model) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    state.set_status(Level::Error, format!("export failed: {error}"));
                }
            }
            Err(error) => state.set_status(Level::Error, format!("export failed: {error}")),
        }
    }
    if let Some((level, message)) = state.status_message.clone() {
        status::show(ui, level, message);
    }
}

fn validation_panel(ui: &mut Ui, state: &mut SetupState) -> Option<StartRequest> {
    ui.heading("Validation");
    if state.validation_errors.is_empty() {
        status::show(ui, Level::Info, "OK — ready to solve");
    } else {
        for error in state.validation_errors.iter().take(20) {
            status::show(ui, Level::Error, error);
        }
    }
    ui.separator();
    status::show(
        ui,
        Level::Info,
        "approximate profile \u{2014} Nash/GTO\u{4fdd}\u{8a3c}\u{306a}\u{3057}",
    );
    ui.separator();

    let enabled = state.validation_errors.is_empty();
    let clicked = ui
        .add_enabled(enabled, egui::Button::new("Start solve"))
        .clicked();
    if !clicked {
        return None;
    }
    // Auto mode materializes its derived settings into the model right here
    // -- once, on this click -- never silently while the user is typing (see
    // `model::apply_auto_derivation`'s doc comment). Advanced mode's model is
    // already exactly what the user configured.
    if state.mode == SetupMode::Auto {
        let threads = machine::detected_threads();
        let memory_budget = machine::half_of_total_memory_bytes();
        if let Err(error) =
            model::apply_auto_derivation(&mut state.model, threads, memory_budget, state.quality)
        {
            state.set_status(Level::Error, format!("auto derivation failed: {error}"));
            return None;
        }
    }
    let config_toml = model::model_to_toml(&state.model).ok()?;
    let run = &state.model.run;
    Some(StartRequest {
        config_toml,
        resume_checkpoint: (!run.resume_from.trim().is_empty())
            .then(|| PathBuf::from(&run.resume_from)),
        output_path: PathBuf::from(&run.output_path),
        checkpoint_path: (!run.checkpoint_path.trim().is_empty())
            .then(|| PathBuf::from(&run.checkpoint_path)),
        check_every: run.check_every,
        max_wall_time_secs: run.max_wall_time_minutes.map(|minutes| minutes * 60.0),
    })
}

fn table_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Table", |ui| {
        let mut seats = state.model.seats.len();
        ui.horizontal(|ui| {
            ui.label("seats:");
            if ui
                .add(egui::DragValue::new(&mut seats).range(2..=9))
                .changed()
            {
                resize_seats(&mut state.model, seats);
            }
        });
        ui.horizontal(|ui| {
            ui.label("button:");
            let mut button = state.model.button;
            egui::ComboBox::from_id_salt("button-seat")
                .selected_text(
                    state
                        .model
                        .seats
                        .get(button)
                        .map(|seat| seat.name.clone())
                        .unwrap_or_default(),
                )
                .show_ui(ui, |ui| {
                    for (index, seat) in state.model.seats.iter().enumerate() {
                        ui.selectable_value(&mut button, index, seat.name.clone());
                    }
                });
            if button != state.model.button {
                state.model.button = button;
                model::relabel_positions(&mut state.model.seats, button);
            }
        });
        if ui.button("Relabel positions").clicked() {
            model::relabel_positions(&mut state.model.seats, state.model.button);
        }
        ui.horizontal(|ui| {
            ui.label("small blind (bb):");
            ui.add(egui::DragValue::new(&mut state.model.small_bb).speed(0.05));
        });
        ui.horizontal(|ui| {
            ui.label("big blind (bb):");
            ui.add(egui::DragValue::new(&mut state.model.big_bb).speed(0.05));
        });
        ante_editor(ui, &mut state.model.ante);
    });
}

fn resize_seats(model: &mut Model, requested: usize) {
    let n = requested.clamp(multiway::types::MIN_SEATS, multiway::types::MAX_SEATS);
    while model.seats.len() < n {
        model.seats.push(model::SeatModel {
            name: String::new(),
            stack_bb: 100.0,
            range: String::new(),
            betting_override: None,
        });
    }
    model.seats.truncate(n);
    model.button = model.button.min(n - 1);
    model::relabel_positions(&mut model.seats, model.button);
}

fn ante_editor(ui: &mut Ui, ante: &mut model::AnteModel) {
    ui.horizontal(|ui| {
        ui.label("ante:");
        ui.selectable_value(&mut ante.kind, model::AnteKind::None, "none");
        ui.selectable_value(&mut ante.kind, model::AnteKind::Each, "each");
        ui.selectable_value(&mut ante.kind, model::AnteKind::BigBlind, "big blind ante");
        if ante.kind != model::AnteKind::None {
            ui.add(egui::DragValue::new(&mut ante.amount_bb).speed(0.05));
        }
    });
}

fn seats_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Seats", |ui| {
        ui.horizontal(|ui| {
            ui.label("copy source seat:");
            ui.add(
                egui::DragValue::new(&mut state.copy_source)
                    .range(0..=(state.model.seats.len().saturating_sub(1))),
            );
            if ui.button("copy to all").clicked()
                && let Some(source) = state.model.seats.get(state.copy_source).cloned()
            {
                for seat in state.model.seats.iter_mut() {
                    seat.stack_bb = source.stack_bb;
                    seat.range = source.range.clone();
                    seat.betting_override = source.betting_override.clone();
                }
            }
        });
        for index in 0..state.model.seats.len() {
            ui.push_id(index, |ui| {
                ui.horizontal(|ui| {
                    let seat = &mut state.model.seats[index];
                    ui.add(egui::TextEdit::singleline(&mut seat.name).desired_width(60.0));
                    ui.add(
                        egui::DragValue::new(&mut seat.stack_bb)
                            .speed(0.5)
                            .suffix("bb"),
                    );
                    ui.add(egui::TextEdit::singleline(&mut seat.range).desired_width(220.0));
                    if !seat.range.trim().is_empty()
                        && let Err(error) = cards::Range::from_str(&seat.range)
                    {
                        status::show(ui, Level::Error, error.to_string());
                    }
                    let mut overridden = seat.betting_override.is_some();
                    if ui.checkbox(&mut overridden, "custom betting").changed() {
                        seat.betting_override = overridden.then(|| state.model.betting.clone());
                    }
                });
                if let Some(betting) = state.model.seats[index].betting_override.as_mut() {
                    ui.indent("seat-betting", |ui| betting_editor(ui, betting));
                }
            });
        }
    });
}

fn betting_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Betting (table-wide)", |ui| {
        betting_editor(ui, &mut state.model.betting);
    });
}

fn betting_editor(ui: &mut Ui, betting: &mut model::BettingModel) {
    ui.checkbox(&mut betting.allow_limp, "allow limp");
    street_editor(ui, "Preflop", &mut betting.preflop, true);
    street_editor(ui, "Flop", &mut betting.flop, false);
    street_editor(ui, "Turn", &mut betting.turn, false);
    street_editor(ui, "River", &mut betting.river, false);
}

fn street_editor(
    ui: &mut Ui,
    label: &str,
    street: &mut model::StreetBettingModel,
    show_isolate: bool,
) {
    ui.push_id(label, |ui| {
        ui.collapsing(label, |ui| {
            sized_field(ui, "bet sizes", &mut street.bet_sizes);
            if show_isolate {
                ui.horizontal(|ui| {
                    let mut enabled = street.isolate_sizes.is_some();
                    if ui
                        .checkbox(&mut enabled, "override isolate sizes")
                        .changed()
                    {
                        street.isolate_sizes = enabled.then(String::new);
                    }
                });
                if let Some(text) = street.isolate_sizes.as_mut() {
                    ui.horizontal(|ui| {
                        ui.label("isolate sizes:");
                        ui.text_edit_singleline(text);
                    });
                    if let Err(error) = size_lexer::parse_sizes(text) {
                        status::show(ui, Level::Error, error);
                    }
                }
            }
            sized_field(ui, "raise sizes", &mut street.raise_sizes);
            ui.horizontal(|ui| {
                ui.label("max aggressive actions:");
                ui.add(egui::DragValue::new(&mut street.max_aggressive_actions).range(1..=8));
            });
            ui.checkbox(&mut street.include_allin, "include all-in");
            ui.horizontal(|ui| {
                let mut enabled = street.allin_threshold.is_some();
                if ui
                    .checkbox(&mut enabled, "all-in merge threshold")
                    .changed()
                {
                    street.allin_threshold = enabled.then_some(0.85);
                }
                if let Some(value) = street.allin_threshold.as_mut() {
                    ui.add(egui::DragValue::new(value).range(0.5..=1.0).speed(0.01));
                }
            });
            if !show_isolate {
                ui.horizontal(|ui| {
                    let mut enabled = street.max_betting_players.is_some();
                    if ui
                        .checkbox(&mut enabled, "check down above a player count")
                        .changed()
                    {
                        street.max_betting_players = enabled.then_some(2);
                    }
                    if let Some(value) = street.max_betting_players.as_mut() {
                        ui.add(egui::DragValue::new(value).range(1..=9));
                    }
                });
                if street.max_betting_players.is_some() {
                    hint(ui, "max players with betting; empty = unlimited");
                }
            }
        });
    });
}

fn sized_field(ui: &mut Ui, label: &str, text: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(text);
    });
    if let Err(error) = size_lexer::parse_sizes(text) {
        status::show(ui, Level::Error, error);
    }
}

fn abstraction_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Abstraction", |ui| {
        let abstraction = &mut state.model.abstraction;
        ui.label("Card-abstraction backend:");
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut abstraction.kind,
                model::AbstractionBackendKind::RolloutKmeans,
                "rollout k-means",
            );
            ui.selectable_value(
                &mut abstraction.kind,
                model::AbstractionBackendKind::Ehs2Table,
                "EHS\u{b2} table (recommended)",
            );
        });
        hint(
            ui,
            "rollout k-means: Monte Carlo hand-strength rollouts clustered into buckets at \
             build time -- flexible bucket counts, retrained per config.",
        );
        let ehs2_selected = abstraction.kind == model::AbstractionBackendKind::Ehs2Table;
        hint(
            ui,
            "EHS\u{b2} table (recommended): precomputed exact percentile buckets, O(1) lookup, no \
             solve-time Monte Carlo.",
        );
        if ehs2_selected {
            hint(
                ui,
                "Rollout samples, abstraction seed, and per-opponent bucket profiles below do not \
                 apply and are ignored (per-opponent profiles are also rejected).",
            );
        }
        ui.horizontal(|ui| {
            ui.label("flop buckets:");
            ui.add(egui::DragValue::new(&mut abstraction.flop_buckets).range(1..=4096));
            ui.label("turn buckets:");
            ui.add(egui::DragValue::new(&mut abstraction.turn_buckets).range(1..=4096));
            ui.label("river buckets:");
            ui.add(egui::DragValue::new(&mut abstraction.river_buckets).range(1..=4096));
        });
        hint(
            ui,
            "memory scales with nodes x buckets -- see the tree/memory estimate below.",
        );
        ui.add_enabled_ui(!ehs2_selected, |ui| {
            ui.horizontal(|ui| {
                ui.label("rollout samples:");
                ui.add(
                    egui::DragValue::new(&mut abstraction.rollout_samples).range(1..=1_000_000),
                );
                ui.label("seed:");
                ui.add(egui::DragValue::new(&mut abstraction.seed));
            });
        });
        ui.horizontal(|ui| {
            ui.label("artifact cache:");
            ui.text_edit_singleline(&mut abstraction.artifact_cache);
            if ui.button("...").clicked()
                && let Some(path) = rfd::FileDialog::new().save_file()
            {
                abstraction.artifact_cache = path.display().to_string();
            }
        });
        ui.label("Recall / memory model:");
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut abstraction.recall,
                model::RecallKind::Full,
                "full (sparse, grows)",
            );
            ui.selectable_value(
                &mut abstraction.recall,
                model::RecallKind::Street,
                "street (recommended, bounded memory)",
            );
        });
        hint(
            ui,
            "Street mode preallocates the whole tree up front and fails fast with a memory estimate if it doesn't fit. \
             Full mode never preallocates, so memory is unbounded and grows with every visited infoset instead.",
        );
        ui.label("Active-opponent bucket profiles:");
        if ehs2_selected {
            status::show(
                ui,
                Level::Warning,
                "Not supported by the EHS\u{b2} table backend; remove any profiles below before solving.",
            );
        }
        let mut remove_index = None;
        ui.add_enabled_ui(!ehs2_selected, |ui| {
            for (index, profile) in abstraction.active_opponent_buckets.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("active opp.:");
                        ui.add(egui::DragValue::new(&mut profile.active_opponents).range(1..=8));
                        ui.label("flop:");
                        ui.add(egui::DragValue::new(&mut profile.flop_buckets).range(1..=4096));
                        ui.label("turn:");
                        ui.add(egui::DragValue::new(&mut profile.turn_buckets).range(1..=4096));
                        ui.label("river:");
                        ui.add(egui::DragValue::new(&mut profile.river_buckets).range(1..=4096));
                        if ui.button("remove").clicked() {
                            remove_index = Some(index);
                        }
                    });
                });
            }
            if ui.button("add profile").clicked() {
                abstraction
                    .active_opponent_buckets
                    .push(model::ActiveOpponentBucketModel {
                        active_opponents: 1,
                        flop_buckets: 32,
                        turn_buckets: 32,
                        river_buckets: 32,
                    });
            }
        });
        if let Some(index) = remove_index {
            abstraction.active_opponent_buckets.remove(index);
        }
    });
}

fn economics_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Economics", |ui| {
        let utility = &mut state.model.utility;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut utility.kind, model::UtilityKind::ChipEv, "Chip EV");
            ui.selectable_value(
                &mut utility.kind,
                model::UtilityKind::TournamentIcm,
                "Tournament ICM",
            );
        });
        if utility.kind == model::UtilityKind::TournamentIcm {
            ui.label("payouts (comma- or newline-separated, best finish first):");
            ui.text_edit_multiline(&mut utility.payouts_text);
            ui.horizontal(|ui| {
                ui.label("samples:");
                ui.add(egui::DragValue::new(&mut utility.samples));
                ui.label("seed:");
                ui.add(egui::DragValue::new(&mut utility.seed));
            });
            ui.label("Outside field players:");
            let mut remove_index = None;
            for (index, player) in utility.outside_field.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut player.name);
                        ui.add(egui::DragValue::new(&mut player.stack_bb).speed(0.5));
                        if ui.button("remove").clicked() {
                            remove_index = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove_index {
                utility.outside_field.remove(index);
            }
            if ui.button("add outside player").clicked() {
                let name = format!("Field {}", utility.outside_field.len() + 1);
                utility.outside_field.push(model::OutsideFieldModel {
                    name,
                    stack_bb: 100.0,
                });
            }
        }

        ui.separator();
        let rake = &mut state.model.rake;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut rake.kind, model::RakeKind::None, "No rake");
            ui.selectable_value(&mut rake.kind, model::RakeKind::PercentCap, "Percent + cap");
            ui.selectable_value(&mut rake.kind, model::RakeKind::GgPreflop, "GG preflop");
        });
        if rake.kind != model::RakeKind::None {
            if utility.kind == model::UtilityKind::TournamentIcm {
                status::show(
                    ui,
                    Level::Warning,
                    "Tournament ICM and rake cannot be combined.",
                );
            }
            ui.horizontal(|ui| {
                ui.label("rate:");
                ui.add(
                    egui::DragValue::new(&mut rake.rate)
                        .speed(0.001)
                        .range(0.0..=1.0),
                );
                ui.label("cap (bb):");
                ui.add(egui::DragValue::new(&mut rake.cap_bb).speed(0.1));
            });
            match rake.kind {
                model::RakeKind::PercentCap => {
                    ui.checkbox(&mut rake.no_flop_no_drop, "no flop, no drop");
                }
                model::RakeKind::GgPreflop => {
                    ui.horizontal(|ui| {
                        ui.label("exempt pot (bb):");
                        ui.add(egui::DragValue::new(&mut rake.exempt_pot_bb).speed(0.1));
                    });
                }
                model::RakeKind::None => {}
            }
        }
    });
}

fn algorithm_section(ui: &mut Ui, state: &mut SetupState) {
    let recall_is_street = state.model.abstraction.recall == model::RecallKind::Street;
    ui.collapsing("Algorithm", |ui| {
        let algorithm = &mut state.model.algorithm;
        ui.horizontal(|ui| {
            ui.label("seed:");
            ui.add(egui::DragValue::new(&mut algorithm.seed));
        });
        ui.horizontal(|ui| {
            ui.label("exploration epsilon:");
            ui.add(
                egui::DragValue::new(&mut algorithm.exploration_epsilon)
                    .speed(0.001)
                    .range(0.0..=1.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label("discount every:");
            ui.add(egui::DragValue::new(&mut algorithm.discount_every));
        });
        ui.horizontal(|ui| {
            ui.label("discount until:");
            ui.add(egui::DragValue::new(&mut algorithm.discount_until));
        });
        ui.checkbox(
            &mut algorithm.traverser_vector,
            "vector traverser (fast, street-recall only)",
        );
        if algorithm.traverser_vector && !recall_is_street {
            status::show(
                ui,
                Level::Warning,
                "Vector traverser requires Recall / memory model = \"street\" above.",
            );
        }
    });
}

fn run_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Run", |ui| {
        let run = &mut state.model.run;
        ui.horizontal(|ui| {
            ui.label("sweeps:");
            ui.add(egui::DragValue::new(&mut run.sweeps));
        });
        ui.horizontal(|ui| {
            let mut enabled = run.seed.is_some();
            if ui.checkbox(&mut enabled, "seed override").changed() {
                run.seed = enabled.then_some(run.seed.unwrap_or(0));
            }
            if let Some(seed) = run.seed.as_mut() {
                ui.add(egui::DragValue::new(seed));
            }
        });
        ui.horizontal(|ui| {
            ui.label("check every:");
            ui.add(egui::DragValue::new(&mut run.check_every).range(1..=u64::MAX));
        });
        ui.horizontal(|ui| {
            ui.label("threads (0 = auto):");
            ui.add(egui::DragValue::new(&mut run.threads));
        });
        ui.horizontal(|ui| {
            ui.label("sweep batch (parallelism):");
            ui.add(egui::DragValue::new(&mut run.sweep_batch).range(1..=u64::MAX));
        });
        // One sweep yields `seats` parallel traversals, so total in-flight
        // parallelism is `seats x sweep_batch`; when that is below the
        // thread count the extra cores idle. Machine-independence: the
        // detected core count only feeds this HINT -- the config always
        // carries the explicit `sweep_batch` value.
        {
            let seats = state.model.seats.len().max(1) as u64;
            let threads = if run.threads > 0 {
                run.threads as u64
            } else {
                std::thread::available_parallelism()
                    .map(|value| value.get() as u64)
                    .unwrap_or(1)
            };
            let parallel = seats * run.sweep_batch;
            if parallel < threads {
                let suggested = threads.div_ceil(seats);
                let message = format!(
                    "{seats} seats x sweep_batch {} = {parallel} parallel traversals \
                     < {threads} threads; sweep_batch >= {suggested} uses every core",
                    run.sweep_batch
                );
                status::show(ui, status::Level::Warning, &message);
            } else {
                hint(
                    ui,
                    &format!(
                        "{seats} seats x sweep_batch {} = {parallel} parallel traversals \
                         on {threads} threads",
                        run.sweep_batch
                    ),
                );
            }
        }
        optional_u64(ui, "checkpoint every", &mut run.checkpoint_every, 50_000);
        optional_u64(
            ui,
            "evaluation cadence",
            &mut run.evaluation_cadence,
            50_000,
        );
        optional_u64(ui, "evaluation samples", &mut run.evaluation_samples, 256);
        ui.horizontal(|ui| {
            ui.label("max memory (MiB, 0 = default):");
            ui.add(egui::DragValue::new(&mut run.max_memory_mib));
        });
        ui.horizontal(|ui| {
            ui.label("storage:");
            ui.selectable_value(&mut run.storage, cli::config::StorageKind::F32, "f32");
            ui.selectable_value(&mut run.storage, cli::config::StorageKind::I16, "i16");
        });
        path_field(ui, "output .mwsol:", &mut run.output_path, &["mwsol"], true);
        path_field(
            ui,
            "checkpoint path:",
            &mut run.checkpoint_path,
            &["mwckpt"],
            true,
        );
        path_field(
            ui,
            "resume from checkpoint:",
            &mut run.resume_from,
            &["mwckpt"],
            false,
        );

        ui.separator();
        ui.label("Convergence stop rule (optional):");
        ui.horizontal(|ui| {
            let mut enabled = run.stop_dev_gain.is_some();
            if ui.checkbox(&mut enabled, "stop on convergence").changed() {
                run.stop_dev_gain = enabled.then_some(run.stop_dev_gain.unwrap_or(0.25));
                run.stop_confirmations = enabled.then_some(run.stop_confirmations.unwrap_or(2));
                run.stop_eval_period_secs =
                    enabled.then_some(run.stop_eval_period_secs.unwrap_or(30.0));
            }
        });
        if let Some(threshold) = run.stop_dev_gain.as_mut() {
            ui.horizontal(|ui| {
                ui.label("dev-gain threshold (bb):");
                ui.add(
                    egui::DragValue::new(threshold)
                        .speed(0.01)
                        .range(0.0001..=1000.0),
                );
            });
            let confirmations = run.stop_confirmations.get_or_insert(2);
            ui.horizontal(|ui| {
                ui.label("confirmations:");
                ui.add(egui::DragValue::new(confirmations).range(1..=u32::MAX));
            });
            let period = run.stop_eval_period_secs.get_or_insert(30.0);
            ui.horizontal(|ui| {
                ui.label("eval period (secs):");
                ui.add(egui::DragValue::new(period).range(0.001..=100_000.0));
            });
            hint(
                ui,
                "run.sweeps becomes a safety cap: the solve stops once the max per-seat \
                 deviation-gain CI upper bound stays below the threshold for this many \
                 consecutive wall-clock-spaced evaluations.",
            );
        }

        ui.separator();
        ui.horizontal(|ui| {
            let mut enabled = run.max_wall_time_minutes.is_some();
            if ui
                .checkbox(&mut enabled, "max wall time (minutes)")
                .changed()
            {
                run.max_wall_time_minutes =
                    enabled.then_some(run.max_wall_time_minutes.unwrap_or(60.0));
            }
            if let Some(minutes) = run.max_wall_time_minutes.as_mut() {
                ui.add(egui::DragValue::new(minutes).range(1.0..=100_000.0));
            }
        });
        hint(
            ui,
            "GUI-only: never written to the config TOML. The worker finishes gracefully \
             (solution + checkpoint written) once this much wall-clock time has elapsed.",
        );
    });
}

fn optional_u64(ui: &mut Ui, label: &str, value: &mut Option<u64>, default: u64) {
    ui.horizontal(|ui| {
        let mut enabled = value.is_some();
        if ui.checkbox(&mut enabled, label).changed() {
            *value = enabled.then_some(value.unwrap_or(default));
        }
        if let Some(v) = value.as_mut() {
            ui.add(egui::DragValue::new(v));
        }
    });
}

fn path_field(ui: &mut Ui, label: &str, text: &mut String, extensions: &[&str], save: bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(text);
        if ui.button("...").clicked() {
            let mut dialog = rfd::FileDialog::new();
            if let Some(extension) = extensions.first() {
                dialog = dialog.add_filter(*extension, extensions);
            }
            let picked = if save {
                dialog.save_file()
            } else {
                dialog.pick_file()
            };
            if let Some(path) = picked {
                *text = path.display().to_string();
            }
        }
    });
}
