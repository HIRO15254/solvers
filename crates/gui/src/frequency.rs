//! Range-wide action-frequency aggregation shared by the Solve tab's live
//! node view and the Results tab's per-node matrix: reduces a node's
//! per-hand-group strategy blocks (each carrying its own reach weight) to
//! one "Σ mass · frequency / Σ mass" row per action -- a GTO-Wizard-style
//! aggregate frequency strip drawn above/beside the per-bucket blocks.
//!
//! The two call sites disagree on what "mass" means (the live view uses each
//! column's linear-CFR strategy-sum mass from
//! `multiway::solver::MultiwaySolver::strategies_at_with_mass`; the Results
//! tab uses a range-weight times earlier-action-probability product), so
//! this module only owns the mass-agnostic reduction plus the
//! range-to-per-class-weight helper the Results tab needs to build its own
//! masses.

use cards::{NUM_CLASSES, NUM_COMBOS, Range, class_index, combo_cards};

/// One hand-group block ready for range-wide aggregation: its own action
/// labels/probabilities (matched by name against the caller's canonical
/// `action_labels`, so a block whose action set differs -- which should not
/// happen at a well-formed node, but costs nothing to tolerate -- is simply
/// skipped for the labels it does not recognize) plus its reach weight
/// ("mass").
pub struct FrequencyBlock<'a> {
    pub action_labels: &'a [String],
    pub probabilities: &'a [f32],
    pub mass: f64,
}

/// `Σ_b mass_b · σ̄_b(action) / Σ_b mass_b`, aligned with `action_labels`.
/// Returns `None` when every block has zero (or negative) mass -- nothing to
/// aggregate, e.g. a node whose blocks have not accumulated any linear-CFR
/// reach weight yet.
pub fn aggregate_action_frequencies(
    action_labels: &[String],
    blocks: &[FrequencyBlock<'_>],
) -> Option<Vec<f64>> {
    let mut totals = vec![0.0f64; action_labels.len()];
    let mut total_mass = 0.0f64;
    for block in blocks {
        if block.mass <= 0.0 {
            continue;
        }
        for (label, &probability) in block.action_labels.iter().zip(block.probabilities) {
            if let Some(index) = action_labels
                .iter()
                .position(|candidate| candidate == label)
            {
                totals[index] += block.mass * f64::from(probability);
            }
        }
        total_mass += block.mass;
    }
    if total_mass <= 0.0 {
        return None;
    }
    Some(totals.iter().map(|total| total / total_mass).collect())
}

/// Per-class total combo weight (indexed by `cards::class_index`'s 169-class
/// layout, the same one the 13x13 matrix uses) reduced from a per-combo
/// [`Range`]: each class's weight is the sum of its own combos' weights. An
/// "all hands" range (`Range::full()`) naturally reduces to each class's
/// combo count (6 for pairs, 4 for suited, 12 for offsuit), matching
/// "uniform per class by combo count".
pub fn class_weights_from_range(range: &Range) -> [f64; NUM_CLASSES] {
    let mut weights = [0.0f64; NUM_CLASSES];
    for combo in 0..NUM_COMBOS {
        let (a, b) = combo_cards(combo);
        let class = class_index(a.rank(), b.rank(), a.suit() == b.suit());
        weights[class] += f64::from(range.weight(combo));
    }
    weights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_weights_by_mass_share() {
        let labels = vec!["fold".to_string(), "call".to_string()];
        let blocks = vec![
            FrequencyBlock {
                action_labels: &labels,
                probabilities: &[1.0, 0.0],
                mass: 3.0,
            },
            FrequencyBlock {
                action_labels: &labels,
                probabilities: &[0.0, 1.0],
                mass: 1.0,
            },
        ];
        let aggregate = aggregate_action_frequencies(&labels, &blocks).unwrap();
        assert!((aggregate[0] - 0.75).abs() < 1e-9);
        assert!((aggregate[1] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn zero_total_mass_returns_none() {
        let labels = vec!["fold".to_string()];
        let blocks = vec![FrequencyBlock {
            action_labels: &labels,
            probabilities: &[1.0],
            mass: 0.0,
        }];
        assert!(aggregate_action_frequencies(&labels, &blocks).is_none());
    }

    #[test]
    fn mismatched_action_labels_are_skipped_rather_than_panicking() {
        let labels = vec!["fold".to_string(), "call".to_string()];
        let odd_labels = vec!["fold".to_string(), "raise-to:500".to_string()];
        let blocks = vec![FrequencyBlock {
            action_labels: &odd_labels,
            probabilities: &[0.4, 0.6],
            mass: 1.0,
        }];
        let aggregate = aggregate_action_frequencies(&labels, &blocks).unwrap();
        // Only "fold" is recognized; "raise-to:500" contributes nowhere, so
        // the aggregate under-sums to 0.4 rather than 1.0. Tolerance is
        // widened past 1e-9 to absorb the f32 -> f64 widening error in the
        // `0.4_f32` literal itself.
        assert!((aggregate[0] - 0.4).abs() < 1e-6);
        assert!((aggregate[1] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn full_range_class_weights_match_combo_counts() {
        let weights = class_weights_from_range(&Range::full());
        assert!((weights[class_index(12, 12, false)] - 6.0).abs() < 1e-9); // AA
        assert!((weights[class_index(12, 11, true)] - 4.0).abs() < 1e-9); // AKs
        assert!((weights[class_index(12, 11, false)] - 12.0).abs() < 1e-9); // AKo
        let total: f64 = weights.iter().sum();
        assert!((total - NUM_COMBOS as f64).abs() < 1e-9);
    }

    #[test]
    fn weighted_range_class_weights_reflect_per_entry_weight() {
        let range: Range = "AA:0.5".parse().unwrap();
        let weights = class_weights_from_range(&range);
        assert!((weights[class_index(12, 12, false)] - 3.0).abs() < 1e-9);
        assert!((weights[class_index(12, 11, true)] - 0.0).abs() < 1e-9);
    }
}
