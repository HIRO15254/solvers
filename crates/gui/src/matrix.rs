//! 13x13 preflop hand-class matrix and postflop bucket grid widgets, plus
//! the label/color derivations they share with the detail panel.
//!
//! Layout matches `cards::range::class_index`: row-major, `AA` at index 0
//! (top-left), suited hands in the upper triangle, offsuit in the lower
//! triangle, `22` at index 168.

use std::cmp::Ordering;

use eframe::egui;
use egui::{Color32, Rect, Sense, Ui, Vec2};

use crate::action_style;
use crate::format;
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

/// Resolves display colors for every action label at one node; see
/// [`action_style::action_colors`] for the fold/check/call/limp/raise-ramp/
/// all-in semantics. Re-exported here so existing callers (and this module's
/// own cell/aggregate-row rendering) keep a stable `matrix::action_colors`
/// path.
pub fn action_colors(labels: &[String], unopened: bool) -> Vec<Color32> {
    action_style::action_colors(labels, unopened)
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
            ui.monospace(format!("{label}: {}", format::percent(frequency)));
        }
    });
}

/// One matrix cell's data: action labels/probabilities for either a 13x13
/// hand class or a postflop bucket. `unopened` should be `true` only for a
/// preflop (street 0) node nobody has raised yet, so `call:` actions render
/// as limps (see [`action_colors`]). `combo_count` is the cell's fixed combo
/// count (see [`class_combo_count`]) for a preflop cell's hover tooltip;
/// `None` for a postflop bucket cell (no such fixed count applies).
pub struct CellData<'a> {
    pub label: String,
    pub actions: &'a [String],
    pub probabilities: &'a [f32],
    pub unopened: bool,
    pub combo_count: Option<u32>,
}

/// Fixed number of concrete combos in a `cards::range::class_index` class,
/// independent of any range weighting: 6 for a pocket pair, 4 for suited, 12
/// for offsuit.
pub fn class_combo_count(index: usize) -> u32 {
    assert!(index < 169, "class index out of range: {index}");
    let row = index / 13;
    let col = index % 13;
    match row.cmp(&col) {
        Ordering::Equal => 6,
        Ordering::Less => 4,
        Ordering::Greater => 12,
    }
}

/// Draws one hard-edged stacked horizontal bar cell plus its tiny label
/// overlay, and (on hover) a tooltip with the class/bucket label, combo
/// count (preflop only), and each action's colored probability. Returns the
/// response so callers can wire hover/click.
pub fn cell(ui: &mut Ui, rect: Rect, data: Option<&CellData<'_>>) -> egui::Response {
    let mut response = ui.interact(
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
            response = response.on_hover_ui(|ui| cell_tooltip(ui, data, &colors));
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

/// Hover-tooltip body for one matrix cell: class/bucket label, combo count
/// (if any), and each action's colored probability -- the same
/// label/color/percent styling as the live-node/results detail panels (see
/// [`action_breakdown_rows`]).
fn cell_tooltip(ui: &mut Ui, data: &CellData<'_>, colors: &[Color32]) {
    ui.strong(&data.label);
    if let Some(combo_count) = data.combo_count {
        ui.label(format!(
            "{combo_count} combo{}",
            if combo_count == 1 { "" } else { "s" }
        ));
    }
    action_breakdown_rows(ui, data.actions, data.probabilities, colors);
}

/// Draws one "colored square + `action  NN.N%`" row per action -- the detail
/// breakdown shared by a matrix cell's hover tooltip and the live-node/
/// Results tabs' detail panels. `colors` must be the same length as
/// `actions`/`probabilities` (typically `action_colors(actions, unopened)`).
pub fn action_breakdown_rows(
    ui: &mut Ui,
    actions: &[String],
    probabilities: &[f32],
    colors: &[Color32],
) {
    for ((action, &probability), color) in actions.iter().zip(probabilities).zip(colors) {
        ui.horizontal(|ui| {
            let (rect, _response) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, *color);
            ui.monospace(format!(
                "{action:<16} {}",
                format::percent(f64::from(probability))
            ));
        });
    }
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

    /// `matrix::action_colors` is a thin re-export of
    /// `action_style::action_colors`; the full fold/limp/ramp/all-in/unknown
    /// coverage lives in `action_style`'s own tests.
    #[test]
    fn action_colors_delegates_to_action_style() {
        let labels = vec!["fold".to_string(), "call:1000".to_string()];
        assert_eq!(
            action_colors(&labels, false),
            action_style::action_colors(&labels, false)
        );
    }
}
