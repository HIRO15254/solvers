//! Range-level aggregation: mapping combos to their 13x13 preflop class
//! (see [`cards::class_index`]) and summarizing per-combo values (weights,
//! weight-averaged values) by class. Used for reporting/display, not by any
//! solve path.

use cards::{NUM_CLASSES, NUM_COMBOS, class_index, combo_cards};

/// The 13x13 class index of a combo: same rank pair, suited-ness, as
/// [`cards::class_index`]. `combo_cards` always returns the higher-rank card
/// first (see its doc comment), so `hi.rank() >= lo.rank()` holds here.
pub fn class_of_combo(combo: usize) -> usize {
    let (hi, lo) = combo_cards(combo);
    class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit())
}

/// Sum of combo weights per class. `weights` must have length
/// [`cards::NUM_COMBOS`].
pub fn class_weights(weights: &[f32]) -> [f64; NUM_CLASSES] {
    assert_eq!(weights.len(), NUM_COMBOS, "weights must cover every combo");
    let mut out = [0.0f64; NUM_CLASSES];
    for (combo, &w) in weights.iter().enumerate() {
        out[class_of_combo(combo)] += w as f64;
    }
    out
}

/// Weight-averaged per-class value: `sum(w * v) / sum(w)` per class, `0.0`
/// where the class has zero total weight. `weights` and `per_combo` must
/// both have length [`cards::NUM_COMBOS`].
pub fn class_average(weights: &[f32], per_combo: &[f32]) -> [f64; NUM_CLASSES] {
    assert_eq!(weights.len(), NUM_COMBOS, "weights must cover every combo");
    assert_eq!(
        per_combo.len(),
        NUM_COMBOS,
        "per_combo must cover every combo"
    );
    let mut weight_sum = [0.0f64; NUM_CLASSES];
    let mut value_sum = [0.0f64; NUM_CLASSES];
    for combo in 0..NUM_COMBOS {
        let class = class_of_combo(combo);
        let w = weights[combo] as f64;
        weight_sum[class] += w;
        value_sum[class] += w * per_combo[combo] as f64;
    }
    let mut out = [0.0f64; NUM_CLASSES];
    for class in 0..NUM_CLASSES {
        if weight_sum[class] != 0.0 {
            out[class] = value_sum[class] / weight_sum[class];
        }
    }
    out
}
