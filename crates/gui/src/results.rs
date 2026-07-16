//! Results tab: browse a completed (or loaded) `MultiwaySolution` -- public
//! history breadcrumb/children, per-seat action-frequency matrices
//! (13x13 preflop, bucket grid postflop), and per-node/per-seat diagnostics.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use egui::{Rect, Ui, Vec2};
use formats::{MultiwayHistoryNode, MultiwaySolution, MultiwayStrategyKey};
use multiway::solver::UNREACHED_BUCKET;

use crate::matrix::{self, CellData};
use crate::theme;

const ROOT: [u8; 16] = [0; 16];

pub struct ResultsState {
    pub solution: MultiwaySolution,
    pub seat_names: Vec<String>,
    /// Where this solution came from (a just-finished solve's output path,
    /// or a file opened via "Open .mwsol..."), shown for orientation only.
    pub source_path: Option<PathBuf>,
    pub current_history: [u8; 16],
    pub selected_actor: Option<u8>,
    /// `(street, active_opponents)`, when a node has more than one variant.
    pub selected_variant: Option<(u8, u8)>,
    /// Full strategy keys (not just class/bucket indexes): postflop keys
    /// carry earlier-street recall buckets and `UNREACHED_BUCKET` sentinels
    /// that cannot be reconstructed from a cell index alone.
    pub hovered_key: Option<MultiwayStrategyKey>,
    pub pinned_key: Option<MultiwayStrategyKey>,
    children_index: HashMap<[u8; 16], Vec<usize>>,
    by_key: HashMap<[u8; 16], usize>,
}

impl ResultsState {
    pub fn new(solution: MultiwaySolution, source_path: Option<PathBuf>) -> Self {
        let seat_names = seat_names_from_config(&solution.config_toml);
        let mut children_index: HashMap<[u8; 16], Vec<usize>> = HashMap::new();
        let mut by_key: HashMap<[u8; 16], usize> = HashMap::new();
        for (index, node) in solution.histories.iter().enumerate() {
            children_index.entry(node.parent).or_default().push(index);
            by_key.insert(node.key, index);
        }
        Self {
            solution,
            seat_names,
            source_path,
            current_history: ROOT,
            selected_actor: None,
            selected_variant: None,
            hovered_key: None,
            pinned_key: None,
            children_index,
            by_key,
        }
    }

    /// Owned copies (not references) so callers can mutate `self` (e.g. on
    /// click) while iterating the result.
    fn children(&self) -> Vec<MultiwayHistoryNode> {
        self.children_index
            .get(&self.current_history)
            .into_iter()
            .flatten()
            .map(|&index| self.solution.histories[index].clone())
            .collect()
    }

    fn seat_label(&self, seat: u8) -> String {
        self.seat_names
            .get(seat as usize)
            .cloned()
            .unwrap_or_else(|| format!("Seat {seat}"))
    }

    /// All distinct `(actor, street, active_opponents)` triples with a
    /// strategy block at the current history node.
    fn node_triples(&self) -> Vec<(u8, u8, u8)> {
        let mut triples: Vec<(u8, u8, u8)> = self
            .solution
            .strategies
            .iter()
            .filter(|block| block.key.history == self.current_history)
            .map(|block| {
                (
                    block.key.actor,
                    block.key.street,
                    block.key.active_opponents,
                )
            })
            .collect();
        triples.sort_unstable();
        triples.dedup();
        triples
    }

    /// `true` when nobody has raised on the path from root to the current
    /// node (only folds/calls/checks so far) -- used to color a `call:`
    /// action at this preflop node as a limp rather than a facing-a-raise
    /// call (see `matrix::action_colors`).
    fn is_unopened(&self) -> bool {
        let mut key = self.current_history;
        while key != ROOT {
            let Some(&index) = self.by_key.get(&key) else {
                return true;
            };
            let node = &self.solution.histories[index];
            if node.action.starts_with("raise-to:") || node.action.starts_with("bet-to:") {
                return false;
            }
            key = node.parent;
        }
        true
    }

    /// Root-to-current path, walking `node.parent` links backward from
    /// `current_history` (each key is looked up directly, so this needs no
    /// knowledge of how history keys are derived).
    fn breadcrumb(&self) -> Vec<(String, [u8; 16])> {
        let mut chain = Vec::new();
        let mut key = self.current_history;
        while key != ROOT {
            let Some(&index) = self.by_key.get(&key) else {
                break;
            };
            let node = &self.solution.histories[index];
            chain.push((
                format!("{} {}", self.seat_label(node.actor), node.action),
                key,
            ));
            key = node.parent;
        }
        chain.push(("ROOT".to_string(), ROOT));
        chain.reverse();
        chain
    }
}

fn seat_names_from_config(config_toml: &str) -> Vec<String> {
    let Ok(config) = toml::from_str::<cli::config::SolveConfig>(config_toml) else {
        return Vec::new();
    };
    let cli::config::GameSection::PreflopMultiway(game) = config.game else {
        return Vec::new();
    };
    game.seats
        .iter()
        .enumerate()
        .map(|(index, seat)| seat.name.clone().unwrap_or_else(|| format!("Seat {index}")))
        .collect()
}

pub fn ui(ui: &mut Ui, state: &mut ResultsState) {
    if let Some(path) = &state.source_path {
        ui.label(egui::RichText::new(format!("source: {}", path.display())).small());
    }
    ui.horizontal(|ui| {
        for (label, key) in state.breadcrumb() {
            if ui.button(label).clicked() {
                state.current_history = key;
                state.selected_actor = None;
                state.selected_variant = None;
                state.pinned_key = None;
            }
            ui.label(">");
        }
    });

    ui.horizontal(|ui| {
        ui.label("Children:");
        for child in state.children() {
            let label = format!("{} {}", state.seat_label(child.actor), child.action);
            if ui.button(label).clicked() {
                state.current_history = child.key;
                state.selected_actor = None;
                state.selected_variant = None;
                state.pinned_key = None;
            }
        }
    });
    ui.separator();

    let triples = state.node_triples();
    let actors: Vec<u8> = {
        let mut actors: Vec<u8> = triples.iter().map(|&(actor, _, _)| actor).collect();
        actors.sort_unstable();
        actors.dedup();
        actors
    };
    if actors.is_empty() {
        ui.label("No strategy blocks at this node.");
        return;
    }
    if state
        .selected_actor
        .is_none_or(|actor| !actors.contains(&actor))
    {
        state.selected_actor = Some(actors[0]);
    }
    ui.horizontal(|ui| {
        ui.label("Actor:");
        for &actor in &actors {
            let label = state.seat_label(actor);
            let selected = state.selected_actor == Some(actor);
            if ui.selectable_label(selected, label).clicked() {
                state.selected_actor = Some(actor);
                state.selected_variant = None;
                state.pinned_key = None;
            }
        }
    });
    let actor = state.selected_actor.expect("set above");

    let variants: Vec<(u8, u8)> = {
        let mut variants: Vec<(u8, u8)> = triples
            .iter()
            .filter(|&&(candidate, _, _)| candidate == actor)
            .map(|&(_, street, active_opponents)| (street, active_opponents))
            .collect();
        variants.sort_unstable();
        variants.dedup();
        variants
    };
    if variants.is_empty() {
        ui.label("No strategy blocks for this actor here.");
        return;
    }
    if state
        .selected_variant
        .is_none_or(|variant| !variants.contains(&variant))
    {
        state.selected_variant = Some(variants[0]);
    }
    if variants.len() > 1 {
        ui.horizontal(|ui| {
            ui.label("Node variant:");
            for &(street, active_opponents) in &variants {
                let label = format!("street {street} / {active_opponents} active opp.");
                let selected = state.selected_variant == Some((street, active_opponents));
                if ui.selectable_label(selected, label).clicked() {
                    state.selected_variant = Some((street, active_opponents));
                    state.pinned_key = None;
                }
            }
        });
    }
    let (street, active_opponents) = state.selected_variant.expect("set above");

    ui.separator();
    ui.columns(2, |columns| {
        let matrix_ui = &mut columns[0];
        if street == 0 {
            preflop_matrix(matrix_ui, state, actor, active_opponents);
        } else {
            bucket_grid(matrix_ui, state, actor, street, active_opponents);
        }
        detail_panel(&mut columns[1], state, actor, street, active_opponents);
    });
}

fn preflop_matrix(ui: &mut Ui, state: &mut ResultsState, actor: u8, active_opponents: u8) {
    let unopened = state.is_unopened();
    let cell_size = Vec2::new(30.0, 20.0);
    let (response, painter) = ui.allocate_painter(
        Vec2::new(cell_size.x * 13.0, cell_size.y * 13.0),
        egui::Sense::hover(),
    );
    let origin = response.rect.min;
    let pointer = ui.ctx().pointer_hover_pos();
    let mut hovered = None;
    for class in 0..169usize {
        let row = class / 13;
        let col = class % 13;
        let rect = Rect::from_min_size(
            origin + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y),
            cell_size,
        );
        // Preflop keys carry the class in slot 0 and the solver's
        // `UNREACHED_BUCKET` sentinel in every later street slot.
        let key = MultiwayStrategyKey {
            history: state.current_history,
            actor,
            street: 0,
            active_opponents,
            bucket_path: [
                class as u32,
                UNREACHED_BUCKET,
                UNREACHED_BUCKET,
                UNREACHED_BUCKET,
            ],
        };
        let block = state.solution.strategy(key);
        let data = block.map(|block| CellData {
            label: matrix::class_label(class),
            actions: &block.actions,
            probabilities: &block.probabilities,
            unopened,
        });
        let cell_response = matrix::cell(ui, rect, data.as_ref());
        if let Some(pointer) = pointer
            && rect.contains(pointer)
            && block.is_some()
        {
            hovered = Some(key);
        }
        if cell_response.clicked() && block.is_some() {
            state.pinned_key = Some(key);
        }
        let _ = painter;
    }
    state.hovered_key = hovered;
}

fn bucket_grid(ui: &mut Ui, state: &mut ResultsState, actor: u8, street: u8, active_opponents: u8) {
    // Postflop keys embed the full private-recall path, so cells must come
    // from the actual blocks at this node -- a key synthesized from the
    // current-street bucket alone would never match.
    let mut keys: Vec<MultiwayStrategyKey> = state
        .solution
        .strategies
        .iter()
        .filter(|block| {
            block.key.history == state.current_history
                && block.key.actor == actor
                && block.key.street == street
                && block.key.active_opponents == active_opponents
        })
        .map(|block| block.key)
        .collect();
    keys.sort_unstable_by_key(|key| key.bucket_path);
    if keys.is_empty() {
        ui.label("No buckets at this node.");
        return;
    }
    let cell_size = Vec2::new(56.0, 20.0);
    let columns = 12usize;
    let rows = keys.len().div_ceil(columns);
    let (response, _painter) = ui.allocate_painter(
        Vec2::new(cell_size.x * columns as f32, cell_size.y * rows as f32),
        egui::Sense::hover(),
    );
    let origin = response.rect.min;
    let pointer = ui.ctx().pointer_hover_pos();
    let mut hovered = None;
    for (index, &key) in keys.iter().enumerate() {
        let row = index / columns;
        let col = index % columns;
        let rect = Rect::from_min_size(
            origin + Vec2::new(col as f32 * cell_size.x, row as f32 * cell_size.y),
            cell_size,
        );
        let bucket = key.bucket_path[street as usize];
        let block = state.solution.strategy(key);
        let data = block.map(|block| CellData {
            label: format!("B{bucket}"),
            actions: &block.actions,
            probabilities: &block.probabilities,
            unopened: false,
        });
        let cell_response = matrix::cell(ui, rect, data.as_ref());
        if let Some(pointer) = pointer
            && rect.contains(pointer)
            && block.is_some()
        {
            hovered = Some(key);
        }
        if cell_response.clicked() && block.is_some() {
            state.pinned_key = Some(key);
        }
    }
    state.hovered_key = hovered;
}

fn detail_panel(ui: &mut Ui, state: &ResultsState, _actor: u8, street: u8, active_opponents: u8) {
    ui.heading("Detail");
    ui.label(format!(
        "history: {} | active opponents: {active_opponents} | street: {street}",
        hex(&state.current_history)
    ));
    let selected_key = state.pinned_key.or(state.hovered_key);
    if let Some(key) = selected_key {
        let label = if key.street == 0 {
            matrix::class_label(key.bucket_path[0] as usize)
        } else {
            format!("B{}", key.bucket_path[key.street as usize])
        };
        if let Some(block) = state.solution.strategy(key) {
            ui.label(egui::RichText::new(label).monospace().strong());
            let unopened = key.street == 0 && state.is_unopened();
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

    ui.separator();
    ui.label("Seat diagnostics:");
    for seat in &state.solution.seats {
        let name = state
            .seat_names
            .get(seat.seat as usize)
            .cloned()
            .unwrap_or_else(|| format!("Seat {}", seat.seat));
        ui.monospace(format!(
            "{name:<8} regret={:.3e} drift={:.3e}",
            seat.average_positive_regret, seat.strategy_drift_l1
        ));
        if let Some(ev) = &seat.profile_ev {
            ui.monospace(format!(
                "  EV {:.3} [{:.3}, {:.3}]",
                ev.mean, ev.ci95[0], ev.ci95[1]
            ));
        }
        if let Some(gain) = &seat.deviation_gain_lower_bound {
            ui.monospace(format!("  deviation-gain>= {:.4}", gain.mean));
        }
    }
    ui.separator();
    ui.colored_label(theme::ACCENT, "approximate profile — Nash/GTO保証なし");
}

fn hex(bytes: &[u8; 16]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
