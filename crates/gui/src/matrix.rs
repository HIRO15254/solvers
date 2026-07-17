//! 13x13 preflop hand-class matrix and postflop bucket grid widgets, plus
//! the label/color derivations they share with the detail panel.
//!
//! Layout matches `cards::range::class_index`: row-major, `AA` at index 0
//! (top-left), suited hands in the upper triangle, offsuit in the lower
//! triangle, `22` at index 168.

use std::cmp::Ordering;

use eframe::egui;
use egui::{Color32, Rect, Sense, Ui, Vec2};

use crate::theme;

pub const RANK_LETTERS: [char; 13] = [
    'A', 'K', 'Q', 'J', 'T', '9', '8', '7', '6', '5', '4', '3', '2',
];

/// Standard 13x13 hand-class label (`"AA"`, `"AKs"`, `"AKo"`, ...) for a
/// `cards::range::class_index` grid position.
pub fn class_label(index: usize) -> String {
    assert!(index < 169, "class index out of range: {index}");
    let row = index / 13;
    let col = index % 13;
    match row.cmp(&col) {
        Ordering::Equal => format!("{0}{0}", RANK_LETTERS[row]),
        Ordering::Less => format!("{}{}s", RANK_LETTERS[row], RANK_LETTERS[col]),
        Ordering::Greater => format!("{}{}o", RANK_LETTERS[col], RANK_LETTERS[row]),
    }
}

/// Semantic bucket an action label falls into, before ramp-position
/// resolution (which needs every action at the same node).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionKind {
    Fold,
    Check,
    Call,
    AllIn,
    Aggressive(u64),
    Unknown,
}

/// Parses the exact labels `HoldemGame::action_label` produces: `"fold"`,
/// `"check"`, `"call:<amount>[:all-in]"`, `"bet-to:<amount>[:all-in]"`,
/// `"raise-to:<amount>[:all-in]"`.
fn classify(label: &str) -> ActionKind {
    if label == "fold" {
        return ActionKind::Fold;
    }
    if label == "check" {
        return ActionKind::Check;
    }
    let all_in = label.ends_with(":all-in");
    if label.starts_with("call:") {
        return if all_in {
            ActionKind::AllIn
        } else {
            ActionKind::Call
        };
    }
    for prefix in ["bet-to:", "raise-to:"] {
        if let Some(rest) = label.strip_prefix(prefix) {
            if all_in {
                return ActionKind::AllIn;
            }
            return match leading_amount(rest) {
                Some(amount) => ActionKind::Aggressive(amount),
                None => ActionKind::Unknown,
            };
        }
    }
    ActionKind::Unknown
}

fn leading_amount(rest: &str) -> Option<u64> {
    rest.split(':').next()?.parse::<u64>().ok()
}

/// Resolves display colors for every action label at one node, coloring
/// `bet-to`/`raise-to` actions along the amber-to-red ramp in ascending
/// order of their raise-to amount (ties share a color).
///
/// `unopened` marks a preflop node nobody has raised yet (blinds/limps
/// only): a `call:` action there is a limp (colored distinctly from a call
/// facing a raise), matching the semantic mapping in
/// `docs/native-gui-plan.md` section F. Pass `false` postflop or once a
/// raise has occurred in this node's history.
pub fn action_colors(labels: &[String], unopened: bool) -> Vec<Color32> {
    let kinds: Vec<ActionKind> = labels.iter().map(|label| classify(label)).collect();
    let mut amounts: Vec<u64> = kinds
        .iter()
        .filter_map(|kind| match kind {
            ActionKind::Aggressive(amount) => Some(*amount),
            _ => None,
        })
        .collect();
    amounts.sort_unstable();
    amounts.dedup();

    kinds
        .into_iter()
        .map(|kind| match kind {
            ActionKind::Fold => theme::FOLD,
            ActionKind::Check => theme::CALL,
            ActionKind::Call => {
                if unopened {
                    theme::LIMP
                } else {
                    theme::CALL
                }
            }
            ActionKind::AllIn => theme::ALL_IN,
            ActionKind::Aggressive(amount) => {
                let rank = amounts
                    .iter()
                    .position(|&value| value == amount)
                    .unwrap_or(0);
                ramp_color(rank, amounts.len())
            }
            ActionKind::Unknown => theme::UNKNOWN,
        })
        .collect()
}

fn ramp_color(rank: usize, total: usize) -> Color32 {
    let stops = theme::RAISE_RAMP;
    if total <= 1 {
        return stops[0];
    }
    let segments = stops.len() - 1;
    let t = rank as f32 / (total - 1) as f32;
    let scaled = t * segments as f32;
    let seg = (scaled.floor() as usize).min(segments - 1);
    let local_t = scaled - seg as f32;
    lerp_color(stops[seg], stops[seg + 1], local_t)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp_channel = |from: u8, to: u8| -> u8 {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * t).round() as u8
    };
    Color32::from_rgb(
        lerp_channel(a.r(), b.r()),
        lerp_channel(a.g(), b.g()),
        lerp_channel(a.b(), b.b()),
    )
}

/// Draws the range-wide action-frequency aggregate row shared by the Solve
/// tab's live node view and the Results tab (see
/// `crate::frequency::aggregate_action_frequencies`): a small stacked bar
/// plus one "label: NN.N%" entry per action, colored the same way a matrix
/// cell would be. Shows a placeholder instead when `aggregate` is `None`
/// (e.g. zero total mass, or a node the caller does not aggregate at all).
pub fn aggregate_row(ui: &mut Ui, aggregate: Option<&(Vec<String>, Vec<f64>)>, unopened: bool) {
    ui.label("Range-wide action frequency:");
    let Some((action_labels, frequencies)) = aggregate else {
        ui.label("(no strategy mass yet)");
        return;
    };
    let colors = action_colors(action_labels, unopened);
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(240.0, 14.0), Sense::hover());
    let painter = ui.painter();
    let mut x = rect.min.x;
    for (&frequency, color) in frequencies.iter().zip(&colors) {
        let width = rect.width() * frequency as f32;
        let segment =
            Rect::from_min_size(egui::pos2(x, rect.min.y), Vec2::new(width, rect.height()));
        painter.rect_filled(segment, 0.0, *color);
        x += width;
    }
    ui.horizontal_wrapped(|ui| {
        for (label, &frequency) in action_labels.iter().zip(frequencies) {
            ui.monospace(format!("{label}: {:>5.1}%", frequency * 100.0));
        }
    });
}

/// One matrix cell's data: action labels/probabilities for either a 13x13
/// hand class or a postflop bucket. `unopened` should be `true` only for a
/// preflop (street 0) node nobody has raised yet, so `call:` actions render
/// as limps (see [`action_colors`]).
pub struct CellData<'a> {
    pub label: String,
    pub actions: &'a [String],
    pub probabilities: &'a [f32],
    pub unopened: bool,
}

/// Draws one hard-edged stacked horizontal bar cell plus its tiny label
/// overlay. Returns the response so callers can wire hover/click.
pub fn cell(ui: &mut Ui, rect: Rect, data: Option<&CellData<'_>>) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id()
            .with(("mw-cell", rect.min.x as i32, rect.min.y as i32)),
        Sense::click(),
    );
    let painter = ui.painter();
    match data {
        None => {
            painter.rect_filled(rect, 0.0, theme::BG);
        }
        Some(data) => {
            let colors = action_colors(data.actions, data.unopened);
            let mut x = rect.min.x;
            let total: f32 = data.probabilities.iter().sum::<f32>().max(1e-6);
            for (probability, color) in data.probabilities.iter().zip(&colors) {
                let width = rect.width() * (probability / total);
                let segment =
                    Rect::from_min_size(egui::pos2(x, rect.min.y), Vec2::new(width, rect.height()));
                painter.rect_filled(segment, 0.0, *color);
                x += width;
            }
            painter.text(
                rect.left_top() + Vec2::new(1.0, 0.0),
                egui::Align2::LEFT_TOP,
                &data.label,
                theme::matrix_label_font(),
                Color32::from_black_alpha(200),
            );
        }
    }
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(0.5, theme::STROKE),
        egui::StrokeKind::Inside,
    );
    if response.hovered() {
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_labels_match_the_documented_layout() {
        assert_eq!(class_label(0), "AA");
        assert_eq!(class_label(1), "AKs");
        assert_eq!(class_label(13), "AKo");
        assert_eq!(class_label(168), "22");
    }

    #[test]
    fn every_class_index_produces_a_three_character_label_or_pair() {
        for index in 0..169 {
            let label = class_label(index);
            assert!(
                label.len() == 2 || label.len() == 3,
                "bad label for {index}: {label}"
            );
        }
    }

    #[test]
    fn fold_and_call_take_fixed_colors() {
        let labels = vec!["fold".to_string(), "call:1000".to_string()];
        let colors = action_colors(&labels, false);
        assert_eq!(colors[0], theme::FOLD);
        assert_eq!(colors[1], theme::CALL);
    }

    #[test]
    fn an_unopened_call_is_a_limp() {
        let labels = vec!["fold".to_string(), "call:1000".to_string()];
        let colors = action_colors(&labels, true);
        assert_eq!(colors[1], theme::LIMP);
    }

    #[test]
    fn raises_are_ordered_by_ascending_amount_along_the_ramp() {
        let labels = vec![
            "fold".to_string(),
            "call:1000".to_string(),
            "raise-to:2500".to_string(),
            "raise-to:5000".to_string(),
            "raise-to:10000:all-in".to_string(),
        ];
        let colors = action_colors(&labels, false);
        assert_eq!(colors[0], theme::FOLD);
        assert_eq!(colors[1], theme::CALL);
        assert_eq!(colors[2], theme::RAISE_RAMP[0]);
        assert_eq!(colors[3], theme::RAISE_RAMP[2]);
        assert_eq!(colors[4], theme::ALL_IN);
    }

    #[test]
    fn unknown_labels_fall_back_to_grey() {
        assert_eq!(
            action_colors(&["mystery".to_string()], false)[0],
            theme::UNKNOWN
        );
    }
}
