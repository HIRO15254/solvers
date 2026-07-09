//! Combinatorics of the 169 preflop hand classes: combo membership, class
//! masses, and the card-disjoint pair counts that turn class-level reach
//! vectors into exact combo-level aggregates (see the crate docs).
//!
//! Everything here is build-time-only and small enough to recompute on
//! demand; nothing is cached to disk.

use cards::{NUM_CLASSES, NUM_COMBOS, Range, class_index, combo_cards};

const RANK_CHARS: [char; 13] = [
    '2', '3', '4', '5', '6', '7', '8', '9', 'T', 'J', 'Q', 'K', 'A',
];

/// Class of a combo: `cards::class_index` on the combo's ranks and
/// suitedness (`combo_cards` returns the higher card first).
pub(crate) fn class_of_combo(combo: usize) -> usize {
    let (hi, lo) = combo_cards(combo);
    class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit())
}

/// Combo indices (`cards::combo_index` space) belonging to `class`.
///
/// Pairs have 6 members, suited classes 4, offsuit classes 12.
pub fn class_combos(class: usize) -> Vec<usize> {
    debug_assert!(class < NUM_CLASSES);
    (0..NUM_COMBOS)
        .filter(|&combo| class_of_combo(combo) == class)
        .collect()
}

/// Number of combos in each class: 6 for pairs, 4 suited, 12 offsuit.
pub fn class_combo_counts() -> [u32; NUM_CLASSES] {
    let mut counts = [0u32; NUM_CLASSES];
    for combo in 0..NUM_COMBOS {
        counts[class_of_combo(combo)] += 1;
    }
    counts
}

/// `N(h, o)`: for every ordered class pair, the number of ordered combo
/// pairs `(c_h, c_o)` with `c_h` in class `h`, `c_o` in class `o`, and no
/// shared card. Row-major `h * NUM_CLASSES + o`, length `169 * 169`.
///
/// Invariant: `Σ_{h,o} N(h, o) == 1326 * 1225` (see
/// [`total_disjoint_pairs`]).
pub fn compat_counts() -> Vec<u32> {
    let classes: Vec<usize> = (0..NUM_COMBOS).map(class_of_combo).collect();
    let cards: Vec<cards::CardSet> = (0..NUM_COMBOS)
        .map(|combo| {
            let (a, b) = combo_cards(combo);
            [a, b].into_iter().collect()
        })
        .collect();
    let mut counts = vec![0u32; NUM_CLASSES * NUM_CLASSES];
    for c_h in 0..NUM_COMBOS {
        for c_o in 0..NUM_COMBOS {
            if c_h != c_o && cards[c_h].is_disjoint(cards[c_o]) {
                counts[classes[c_h] * NUM_CLASSES + classes[c_o]] += 1;
            }
        }
    }
    counts
}

/// `1326 * 1225`: the number of ordered card-disjoint combo pairs, which is
/// both the sum of [`compat_counts`] and the normalizer of a full-range
/// trunk.
pub fn total_disjoint_pairs() -> u64 {
    1326 * 1225
}

/// Class mass of a combo-weighted range: `R(h) = Σ_{c in h} w(c)`.
///
/// This is the reach vector entry the trunk uses for class `h`. Note that
/// non-uniform weights *within* a class are summed into a single mass —
/// exact for the uniform (0/1 or per-class-weighted) ranges the parser
/// produces, and the standard lossless-preflop assumption.
pub fn class_mass(range: &Range) -> [f32; NUM_CLASSES] {
    let mut mass = [0f32; NUM_CLASSES];
    for combo in 0..NUM_COMBOS {
        mass[class_of_combo(combo)] += range.weight(combo);
    }
    mass
}

/// Human-readable label of a class: `"AA"`, `"AKs"`, `"AKo"`, ...
///
/// Follows the standard 13x13 grid layout of `cards::class_index` (pairs on
/// the diagonal, suited above, offsuit below).
pub fn class_label(class: usize) -> String {
    debug_assert!(class < NUM_CLASSES);
    let row = class / 13;
    let col = class % 13;
    let (hi, lo) = (row.min(col), row.max(col));
    let hi_char = RANK_CHARS[12 - hi];
    let lo_char = RANK_CHARS[12 - lo];
    if row == col {
        format!("{hi_char}{lo_char}")
    } else if row < col {
        format!("{hi_char}{lo_char}s")
    } else {
        format!("{hi_char}{lo_char}o")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn combo_counts_are_6_4_12() {
        let counts = class_combo_counts();
        assert_eq!(counts.iter().map(|&c| c as u64).sum::<u64>(), 1326);
        for class in 0..NUM_CLASSES {
            let row = class / 13;
            let col = class % 13;
            let expected = if row == col {
                6
            } else if row < col {
                4
            } else {
                12
            };
            assert_eq!(counts[class], expected, "class {}", class_label(class));
        }
    }

    #[test]
    fn labels_match_class_index_goldens() {
        // Goldens pinned in cards::range tests.
        assert_eq!(class_label(0), "AA");
        assert_eq!(class_label(1), "AKs");
        assert_eq!(class_label(13), "AKo");
        assert_eq!(class_label(168), "22");
        // Round trip through the parser for every class.
        for class in 0..NUM_CLASSES {
            let range = Range::from_str(&class_label(class)).unwrap();
            let mass = class_mass(&range);
            for (c, &m) in mass.iter().enumerate() {
                let expected = if c == class {
                    class_combo_counts()[class] as f32
                } else {
                    0.0
                };
                assert_eq!(m, expected, "class {} vs {}", class_label(class), c);
            }
        }
    }

    #[test]
    fn compat_count_goldens() {
        let counts = compat_counts();
        let n = |a: &str, b: &str| {
            let h = Range::from_str(a).unwrap();
            let o = Range::from_str(b).unwrap();
            let hc = (0..NUM_CLASSES).find(|&c| class_mass(&h)[c] > 0.0).unwrap();
            let oc = (0..NUM_CLASSES).find(|&c| class_mass(&o)[c] > 0.0).unwrap();
            counts[hc * NUM_CLASSES + oc]
        };
        // AA vs AA: each of the 6 combos leaves exactly one disjoint AA combo.
        assert_eq!(n("AA", "AA"), 6);
        // AA vs KK: no shared ranks, all 36 ordered pairs are disjoint.
        assert_eq!(n("AA", "KK"), 36);
        // AKs vs AA: 4 AKs combos x 3 AA combos avoiding that ace.
        assert_eq!(n("AKs", "AA"), 12);
        // Total ordered disjoint pairs.
        let total: u64 = counts.iter().map(|&c| c as u64).sum();
        assert_eq!(total, total_disjoint_pairs());
        // Symmetry.
        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                assert_eq!(counts[h * NUM_CLASSES + o], counts[o * NUM_CLASSES + h]);
            }
        }
    }

    #[test]
    fn full_range_mass_is_combo_counts() {
        let mass = class_mass(&Range::full());
        let counts = class_combo_counts();
        for class in 0..NUM_CLASSES {
            assert_eq!(mass[class], counts[class] as f32);
        }
    }
}
