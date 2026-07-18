//! Shared action-label -> color mapping, used everywhere an action (fold,
//! check, call, bet/raise, all-in) is drawn: 13x13/bucket matrix cells (see
//! `crate::matrix`), the live node view's detail panel and strategy bars, the
//! range-wide aggregate-frequency row, and the EV table's column headers
//! (`crate::eval_table`).
//!
//! Parses the exact labels `multiway::holdem::HoldemGame::write_action_label`
//! produces: `"fold"`, `"check"`, `"call:<amount>[:all-in]"`,
//! `"bet-to:<amount>[:all-in]"`, `"raise-to:<amount>[:all-in]"`.

use eframe::egui;
use egui::Color32;

use crate::theme;

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
/// order of their raise-to amount (ties share a color) -- i.e. the ramp
/// position scales with the action's rank among the node's own raise sizes,
/// not with the action's index in `labels`.
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

/// One action label's color, resolved in the context of the full `labels`
/// list at its node (needed to rank `raise-to`/`bet-to` sizes along the
/// ramp). Convenience wrapper over [`action_colors`] for a caller that only
/// wants one entry (e.g. an EV table column header).
pub fn action_color_at(labels: &[String], index: usize, unopened: bool) -> Color32 {
    action_colors(labels, unopened)
        .get(index)
        .copied()
        .unwrap_or(theme::UNKNOWN)
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn action_color_at_matches_the_full_resolution() {
        let labels = vec!["fold".to_string(), "raise-to:2500".to_string()];
        let colors = action_colors(&labels, false);
        assert_eq!(action_color_at(&labels, 0, false), colors[0]);
        assert_eq!(action_color_at(&labels, 1, false), colors[1]);
        assert_eq!(action_color_at(&labels, 5, false), theme::UNKNOWN);
    }
}
