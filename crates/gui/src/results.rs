//! Results tab: browse a completed (or loaded) `MultiwaySolution` -- public
//! history breadcrumb/children, per-seat action-frequency matrices
//! (13x13 preflop, bucket grid postflop), and per-node/per-seat diagnostics.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use cards::Range;
use eframe::egui;
use egui::{Rect, Ui, Vec2};
use formats::{MultiwayHistoryNode, MultiwaySolution, MultiwayStrategyBlock, MultiwayStrategyKey};
use multiway::solver::{NodeActionEvaluation, UNREACHED_BUCKET};

use crate::frequency::{self, FrequencyBlock};
use crate::matrix::{self, CellData};
use crate::theme;

const ROOT: [u8; 16] = [0; 16];

/// State of the on-demand "Evaluate EVs" request for the currently displayed
/// node, run on a background thread (`evaluate_mwsol_node_actions` rebuilds
/// the generative game, which can be slow on an abstraction-cache miss --
/// see `cli::node_eval`'s doc comment -- so it must never block the UI
/// thread). Carries its own `path` so the render side can drop a reply that
/// answers a node this view no longer shows.
pub enum ResultsEvalState {
    Idle,
    Pending {
        path: Vec<usize>,
        receiver: Receiver<Result<NodeActionEvaluation, String>>,
    },
    Ready {
        path: Vec<usize>,
        evaluation: NodeActionEvaluation,
    },
    Failed {
        path: Vec<usize>,
        error: String,
    },
}

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
    /// Sample count for the next "Evaluate EVs" request.
    pub eval_samples: u64,
    pub eval: ResultsEvalState,
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
            eval_samples: 4_096,
            eval: ResultsEvalState::Idle,
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

    /// Root-to-current action-index path -- the order
    /// `evaluate_mwsol_node_actions` expects. Walks the same parent chain as
    /// [`Self::breadcrumb`].
    fn action_path(&self) -> Vec<usize> {
        let mut path = Vec::new();
        let mut key = self.current_history;
        while key != ROOT {
            let Some(&index) = self.by_key.get(&key) else {
                break;
            };
            let node = &self.solution.histories[index];
            path.push(node.action_index as usize);
            key = node.parent;
        }
        path.reverse();
        path
    }

    /// `active_opponents` of `actor`'s own (preflop) strategy block at
    /// `history`, if any -- a history node has at most one `active_opponents`
    /// value for a given acting seat, so the first match is the answer.
    fn active_opponents_at(&self, history: [u8; 16], actor: u8) -> Option<u8> {
        self.solution
            .strategies
            .iter()
            .find(|block| {
                block.key.history == history && block.key.actor == actor && block.key.street == 0
            })
            .map(|block| block.key.active_opponents)
    }

    /// `actor`'s configured preflop range, parsed the same way the solve
    /// itself compiled it (`MultiwayConfig::compile_ranges`): empty means
    /// "all hands" (`Range::full()`), otherwise `cards::Range`'s grammar.
    fn seat_range(&self, actor: u8) -> Option<Range> {
        let config: cli::config::SolveConfig = toml::from_str(&self.solution.config_toml).ok()?;
        let cli::config::GameSection::PreflopMultiway(game) = config.game else {
            return None;
        };
        let ranges = game.compile_ranges().ok()?;
        ranges.as_slice().get(actor as usize).cloned()
    }

    /// Per-class reach weight for `actor` at the currently displayed node:
    /// `range_weight(class) * Π σ̄(action taken | class)` over `actor`'s own
    /// earlier decisions along the root-to-current path (preflop only).
    fn class_reach_weights(&self, actor: u8) -> Option<[f64; cards::NUM_CLASSES]> {
        let range = self.seat_range(actor)?;
        let mut reach = frequency::class_weights_from_range(&range);

        let mut ancestors: Vec<(usize, [u8; 16])> = Vec::new();
        let mut key = self.current_history;
        while key != ROOT {
            let Some(&index) = self.by_key.get(&key) else {
                break;
            };
            let node = &self.solution.histories[index];
            if node.actor == actor {
                ancestors.push((node.action_index as usize, node.parent));
            }
            key = node.parent;
        }

        for (action_index, decision_history) in ancestors {
            let Some(active_opponents) = self.active_opponents_at(decision_history, actor) else {
                continue;
            };
            for (class, weight) in reach.iter_mut().enumerate() {
                let key = MultiwayStrategyKey {
                    history: decision_history,
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
                let probability = self
                    .solution
                    .strategy(key)
                    .and_then(|block| block.probabilities.get(action_index).copied())
                    .unwrap_or(0.0);
                *weight *= f64::from(probability);
            }
        }
        Some(reach)
    }

    /// Range-wide action-frequency aggregate for the currently displayed
    /// preflop node: `actor`'s reach-weighted per-class mass (see
    /// [`Self::class_reach_weights`]) fed through
    /// `frequency::aggregate_action_frequencies`. `None` at a postflop node,
    /// when `actor`'s range can't be resolved, or when the node has no
    /// blocks at all.
    fn range_wide_aggregate(
        &self,
        actor: u8,
        street: u8,
        active_opponents: u8,
    ) -> Option<(Vec<String>, Vec<f64>)> {
        if street != 0 {
            return None;
        }
        let reach = self.class_reach_weights(actor)?;
        let mut blocks: Vec<(&MultiwayStrategyBlock, f64)> = Vec::new();
        for (class, &weight) in reach.iter().enumerate() {
            let key = MultiwayStrategyKey {
                history: self.current_history,
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
            if let Some(block) = self.solution.strategy(key) {
                blocks.push((block, weight));
            }
        }
        let action_labels = blocks.first()?.0.actions.clone();
        let frequency_blocks: Vec<FrequencyBlock<'_>> = blocks
            .iter()
            .map(|(block, mass)| FrequencyBlock {
                action_labels: &block.actions,
                probabilities: &block.probabilities,
                mass: *mass,
            })
            .collect();
        let frequencies =
            frequency::aggregate_action_frequencies(&action_labels, &frequency_blocks)?;
        Some((action_labels, frequencies))
    }

    /// Spawns the background thread servicing "Evaluate EVs" for the node
    /// currently on screen; see [`ResultsEvalState`]'s doc comment for why
    /// this must not run on the UI thread.
    fn start_eval(&mut self) {
        let path = self.action_path();
        let solution = self.solution.clone();
        let samples = self.eval_samples;
        let seed = crate::worker::node_eval_seed(algorithm_seed(&self.solution.config_toml), &path);
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread_path = path.clone();
        std::thread::spawn(move || {
            let result =
                cli::node_eval::evaluate_mwsol_node_actions(&solution, &thread_path, samples, seed)
                    .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.eval = ResultsEvalState::Pending { path, receiver };
    }

    /// Non-blocking poll of an in-flight background evaluation; call once
    /// per frame before rendering.
    fn poll_eval(&mut self) {
        let ResultsEvalState::Pending { .. } = &self.eval else {
            return;
        };
        let ResultsEvalState::Pending { path, receiver } =
            std::mem::replace(&mut self.eval, ResultsEvalState::Idle)
        else {
            unreachable!("just matched Pending above");
        };
        self.eval = match receiver.try_recv() {
            Ok(Ok(evaluation)) => ResultsEvalState::Ready { path, evaluation },
            Ok(Err(error)) => ResultsEvalState::Failed { path, error },
            Err(TryRecvError::Empty) => ResultsEvalState::Pending { path, receiver },
            Err(TryRecvError::Disconnected) => ResultsEvalState::Failed {
                path,
                error: "evaluation thread disconnected without a reply".to_string(),
            },
        };
    }
}

/// `[algorithm] seed` from a solution's embedded config, or `0` on a config
/// that (unexpectedly) isn't `schedule = "external-sampling-mccfr"` -- the
/// only schedule the multiway GUI ever writes (see
/// `model::solve_config_to_model`).
fn algorithm_seed(config_toml: &str) -> u64 {
    toml::from_str::<cli::config::SolveConfig>(config_toml)
        .ok()
        .map(|config| match config.algorithm {
            cli::config::AlgorithmSection::ExternalSamplingMccfr { seed, .. } => seed,
            _ => 0,
        })
        .unwrap_or(0)
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
    state.poll_eval();

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
                state.eval = ResultsEvalState::Idle;
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
                state.eval = ResultsEvalState::Idle;
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

    let unopened = street == 0 && state.is_unopened();
    let aggregate = state.range_wide_aggregate(actor, street, active_opponents);
    matrix::aggregate_row(ui, aggregate.as_ref(), unopened);

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

    ui.separator();
    eval_controls_ui(ui, state, street);
}

/// "Evaluate EVs" button, sample-count field, and (once a reply arrives for
/// the node currently shown) the shared per-hand/per-action EV table.
fn eval_controls_ui(ui: &mut Ui, state: &mut ResultsState, street: u8) {
    ui.horizontal(|ui| {
        ui.label("EV samples:");
        ui.add(egui::DragValue::new(&mut state.eval_samples).range(1..=1_000_000));
        if ui.button("Evaluate EVs").clicked() {
            state.start_eval();
        }
        match &state.eval {
            ResultsEvalState::Idle | ResultsEvalState::Ready { .. } => {}
            ResultsEvalState::Pending { .. } => {
                ui.spinner();
                ui.label("evaluating (may retrain a rollout abstraction on a cold cache)...");
            }
            ResultsEvalState::Failed { error, .. } => {
                ui.colored_label(theme::ACCENT, format!("evaluation failed: {error}"));
            }
        }
    });
    if let ResultsEvalState::Ready { path, evaluation } = &state.eval
        && *path == state.action_path()
    {
        let actor_label = state.seat_label(evaluation.actor as u8);
        crate::eval_table::ui(ui, evaluation, street == 0, &actor_label);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const H1: [u8; 16] = [1; 16];
    const H2: [u8; 16] = [2; 16];

    /// A hand-built (never solved) two-seat `MultiwaySolution`: seat 0's
    /// range is only `AA`/`72o`, seat 0 opens `raise-to:250` at ROOT (always
    /// with `AA`, half the time with `72o`), seat 1 3bets to `raise-to:800`
    /// (`H1`), and seat 0 faces that 3bet at `H2` with different per-class
    /// strategies. Exercises `ResultsState::class_reach_weights`/
    /// `range_wide_aggregate`'s reach-weight product without needing an
    /// actual solve.
    fn synthetic_solution() -> MultiwaySolution {
        let mut model = crate::model::Model::new_default(2);
        model.seats[0].range = "AA,72o".to_string();
        let config_toml = crate::model::model_to_toml(&model).unwrap();

        let histories = vec![
            MultiwayHistoryNode {
                key: H1,
                parent: ROOT,
                actor: 0,
                action_index: 1,
                action: "raise-to:250".to_string(),
            },
            MultiwayHistoryNode {
                key: H2,
                parent: H1,
                actor: 1,
                action_index: 0,
                action: "raise-to:800".to_string(),
            },
        ];

        let aa = cards::class_index(12, 12, false) as u32;
        let seven_two_o = cards::class_index(5, 0, false) as u32;
        let unreached = [UNREACHED_BUCKET; 3];

        let mut strategies = vec![
            // Seat 0's own opening decision at ROOT.
            MultiwayStrategyBlock {
                key: MultiwayStrategyKey {
                    history: ROOT,
                    actor: 0,
                    street: 0,
                    active_opponents: 1,
                    bucket_path: [aa, unreached[0], unreached[1], unreached[2]],
                },
                actions: vec!["fold".to_string(), "raise-to:250".to_string()],
                probabilities: vec![0.0, 1.0],
            },
            MultiwayStrategyBlock {
                key: MultiwayStrategyKey {
                    history: ROOT,
                    actor: 0,
                    street: 0,
                    active_opponents: 1,
                    bucket_path: [seven_two_o, unreached[0], unreached[1], unreached[2]],
                },
                actions: vec!["fold".to_string(), "raise-to:250".to_string()],
                probabilities: vec![0.5, 0.5],
            },
            // Seat 0 facing the 3bet at H2.
            MultiwayStrategyBlock {
                key: MultiwayStrategyKey {
                    history: H2,
                    actor: 0,
                    street: 0,
                    active_opponents: 1,
                    bucket_path: [aa, unreached[0], unreached[1], unreached[2]],
                },
                actions: vec![
                    "fold".to_string(),
                    "call:800".to_string(),
                    "raise-to:2000".to_string(),
                ],
                probabilities: vec![0.0, 0.2, 0.8],
            },
            MultiwayStrategyBlock {
                key: MultiwayStrategyKey {
                    history: H2,
                    actor: 0,
                    street: 0,
                    active_opponents: 1,
                    bucket_path: [seven_two_o, unreached[0], unreached[1], unreached[2]],
                },
                actions: vec![
                    "fold".to_string(),
                    "call:800".to_string(),
                    "raise-to:2000".to_string(),
                ],
                probabilities: vec![0.9, 0.1, 0.0],
            },
        ];
        strategies.sort_unstable_by_key(|block| block.key);

        MultiwaySolution {
            schema_version: formats::MULTIWAY_SCHEMA_VERSION,
            config_toml,
            abstraction_fingerprint: [0; 32],
            sweeps: 1,
            approximate_profile: true,
            seats: Vec::new(),
            histories,
            strategies,
        }
    }

    #[test]
    fn class_reach_weights_multiply_range_weight_by_the_seats_own_earlier_action_probability() {
        let solution = synthetic_solution();
        let mut state = ResultsState::new(solution, None);
        state.current_history = H2;

        let aa = cards::class_index(12, 12, false);
        let seven_two_o = cards::class_index(5, 0, false);
        let reach = state
            .class_reach_weights(0)
            .expect("seat 0 has a configured range");

        // range_weight(AA) = 6 combos * P(raise-to:250 | AA) = 1.0 -> 6.0
        assert!((reach[aa] - 6.0).abs() < 1e-6);
        // range_weight(72o) = 12 combos * P(raise-to:250 | 72o) = 0.5 -> 6.0
        assert!((reach[seven_two_o] - 6.0).abs() < 1e-6);
        // Every other class has zero range weight (seat 0's range is only
        // AA/72o), so its reach is zero regardless of the product.
        assert_eq!(reach.iter().filter(|&&weight| weight > 0.0).count(), 2);
    }

    #[test]
    fn range_wide_aggregate_weights_groups_by_reach_not_raw_combo_count() {
        let solution = synthetic_solution();
        let mut state = ResultsState::new(solution, None);
        state.current_history = H2;

        let (action_labels, frequencies) = state
            .range_wide_aggregate(0, 0, 1)
            .expect("H2 has preflop blocks for both classes and a resolvable range");
        assert_eq!(
            action_labels,
            vec![
                "fold".to_string(),
                "call:800".to_string(),
                "raise-to:2000".to_string()
            ]
        );
        // AA and 72o both reach with weight 6.0 (see the reach-weights test
        // above), so despite 72o having twice as many combos as AA, they
        // contribute equally to the aggregate:
        // fold = (0.0+0.9)/2, call = (0.2+0.1)/2, raise = (0.8+0.0)/2.
        assert!((frequencies[0] - 0.45).abs() < 1e-6);
        assert!((frequencies[1] - 0.15).abs() < 1e-6);
        assert!((frequencies[2] - 0.4).abs() < 1e-6);
        let total: f64 = frequencies.iter().sum();
        assert!((total - 1.0).abs() < 1e-6);
    }

    #[test]
    fn range_wide_aggregate_is_none_postflop() {
        let solution = synthetic_solution();
        let mut state = ResultsState::new(solution, None);
        state.current_history = H2;
        assert!(state.range_wide_aggregate(0, 1, 1).is_none());
    }

    #[test]
    fn action_path_walks_the_root_to_current_action_indices() {
        let solution = synthetic_solution();
        let mut state = ResultsState::new(solution, None);
        assert!(state.action_path().is_empty());
        state.current_history = H1;
        assert_eq!(state.action_path(), vec![1]);
        state.current_history = H2;
        assert_eq!(state.action_path(), vec![1, 0]);
    }
}
