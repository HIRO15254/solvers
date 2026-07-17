//! Setup tab: preset browser (left), editable config form (center), live
//! validation + "Start solve" (right). See `docs/native-gui-plan.md`
//! section F.

use std::path::PathBuf;
use std::str::FromStr;

use eframe::egui;
use egui::{Color32, Ui};

use crate::model::{self, Model};
use crate::presets::{self, PresetEntry};
use crate::size_lexer;

pub struct SetupState {
    pub model: Model,
    pub user_preset_dir: PathBuf,
    pub presets: Vec<PresetEntry>,
    pub new_preset_name: String,
    pub status_message: Option<String>,
    pub confirm_delete: Option<PathBuf>,
    pub copy_source: usize,
    pub validation_errors: Vec<String>,
}

/// Everything the worker needs to start a solve: the config TOML plus the
/// GUI-only artifact destinations (see `worker::RunTarget`).
pub struct StartRequest {
    pub config_toml: String,
    pub resume_checkpoint: Option<PathBuf>,
    pub output_path: PathBuf,
    pub checkpoint_path: Option<PathBuf>,
    pub check_every: u64,
}

impl SetupState {
    pub fn new(user_preset_dir: PathBuf) -> Self {
        let presets = presets::list(&user_preset_dir);
        Self {
            model: Model::new_default(6),
            user_preset_dir,
            presets,
            new_preset_name: String::new(),
            status_message: None,
            confirm_delete: None,
            copy_source: 0,
            validation_errors: Vec::new(),
        }
    }

    fn refresh_presets(&mut self) {
        self.presets = presets::list(&self.user_preset_dir);
    }

    fn load_toml(&mut self, text: &str) {
        match model::toml_to_model(text, &self.model.run) {
            Ok(model) => {
                self.model = model;
                self.status_message = None;
            }
            Err(error) => self.status_message = Some(format!("load failed: {error}")),
        }
    }
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
        egui::ScrollArea::vertical().show(ui, |ui| {
            table_section(ui, state);
            seats_section(ui, state);
            betting_section(ui, state);
            abstraction_section(ui, state);
            economics_section(ui, state);
            algorithm_section(ui, state);
            run_section(ui, state);
        });
    });

    start
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
            Err(error) => state.status_message = Some(format!("load failed: {error}")),
        }
    }
    if let Some(path) = to_delete {
        state.confirm_delete = Some(path);
    }
    if let Some(path) = state.confirm_delete.clone() {
        ui.colored_label(Color32::from_rgb(0xd9, 0x4a, 0x3a), "Delete this preset?");
        ui.horizontal(|ui| {
            if ui.button("Yes, delete").clicked() {
                match presets::delete(&path) {
                    Ok(()) => {
                        state.refresh_presets();
                        state.status_message = Some("deleted".to_string());
                    }
                    Err(error) => state.status_message = Some(format!("delete failed: {error}")),
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
                            state.status_message = Some("saved".to_string());
                        }
                        Err(error) => state.status_message = Some(format!("save failed: {error}")),
                    }
                }
                Err(error) => state.status_message = Some(format!("save failed: {error}")),
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
            Err(error) => state.status_message = Some(format!("import failed: {error}")),
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
                    state.status_message = Some(format!("export failed: {error}"));
                }
            }
            Err(error) => state.status_message = Some(format!("export failed: {error}")),
        }
    }
    if let Some(message) = state.status_message.clone() {
        ui.colored_label(Color32::YELLOW, message);
    }
}

fn validation_panel(ui: &mut Ui, state: &SetupState) -> Option<StartRequest> {
    ui.heading("Validation");
    if state.validation_errors.is_empty() {
        ui.colored_label(crate::theme::ACCENT, "OK — ready to solve");
    } else {
        for error in state.validation_errors.iter().take(20) {
            ui.colored_label(Color32::from_rgb(0xd9, 0x4a, 0x3a), error);
        }
    }
    ui.separator();
    ui.colored_label(
        crate::theme::ACCENT,
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
                        ui.colored_label(Color32::from_rgb(0xd9, 0x4a, 0x3a), error.to_string());
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
                        ui.colored_label(Color32::from_rgb(0xd9, 0x4a, 0x3a), error);
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
        });
    });
}

fn sized_field(ui: &mut Ui, label: &str, text: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(text);
    });
    if let Err(error) = size_lexer::parse_sizes(text) {
        ui.colored_label(Color32::from_rgb(0xd9, 0x4a, 0x3a), error);
    }
}

fn abstraction_section(ui: &mut Ui, state: &mut SetupState) {
    ui.collapsing("Abstraction", |ui| {
        let abstraction = &mut state.model.abstraction;
        ui.horizontal(|ui| {
            ui.label("flop buckets:");
            ui.add(egui::DragValue::new(&mut abstraction.flop_buckets).range(1..=4096));
            ui.label("turn buckets:");
            ui.add(egui::DragValue::new(&mut abstraction.turn_buckets).range(1..=4096));
            ui.label("river buckets:");
            ui.add(egui::DragValue::new(&mut abstraction.river_buckets).range(1..=4096));
        });
        ui.horizontal(|ui| {
            ui.label("rollout samples:");
            ui.add(egui::DragValue::new(&mut abstraction.rollout_samples).range(1..=1_000_000));
            ui.label("seed:");
            ui.add(egui::DragValue::new(&mut abstraction.seed));
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
                "street (dense, preallocated)",
            );
        });
        ui.label(
            "Street mode preallocates the whole tree up front and fails fast with a memory estimate if it doesn't fit.",
        );
        ui.label("Active-opponent bucket profiles:");
        let mut remove_index = None;
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
        if let Some(index) = remove_index {
            abstraction.active_opponent_buckets.remove(index);
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
                ui.colored_label(
                    Color32::from_rgb(0xd9, 0x4a, 0x3a),
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
            ui.colored_label(
                Color32::from_rgb(0xd9, 0x4a, 0x3a),
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
