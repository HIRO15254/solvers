use std::str::FromStr;

use crate::card::{Card, Rank, rank_from_char};

/// Number of two-card combos: C(52, 2).
pub const NUM_COMBOS: usize = 1326;
/// Number of preflop hand classes in the 13x13 grid.
pub const NUM_CLASSES: usize = 169;

/// Triangular combo index for an unordered card pair. `a` and `b` are card
/// indices and must differ.
pub fn combo_index(a: Card, b: Card) -> usize {
    let (hi, lo) = if a.index() > b.index() {
        (a.index(), b.index())
    } else {
        (b.index(), a.index())
    };
    hi * (hi - 1) / 2 + lo
}

/// Inverse of [`combo_index`]: returns the pair with the higher card first.
pub fn combo_cards(combo: usize) -> (Card, Card) {
    debug_assert!(combo < NUM_COMBOS);
    // hi is the largest h with h*(h-1)/2 <= combo
    let hi = (1..52).rfind(|&h| h * (h - 1) / 2 <= combo).unwrap();
    let lo = combo - hi * (hi - 1) / 2;
    (Card::from_index(hi as u8), Card::from_index(lo as u8))
}

/// 13x13 grid index of a hand class. Rows/cols run A (0) down to 2 (12);
/// pairs on the diagonal, suited above it, offsuit below it — the standard
/// range-chart layout. `hi` must be >= `lo`.
pub fn class_index(hi: Rank, lo: Rank, suited: bool) -> usize {
    debug_assert!(hi >= lo);
    let row = (12 - hi) as usize;
    let col = (12 - lo) as usize;
    if hi == lo {
        row * 13 + row
    } else if suited {
        row * 13 + col
    } else {
        col * 13 + row
    }
}

/// A weighted preflop range over all 1,326 two-card combos.
#[derive(Clone, PartialEq)]
pub struct Range {
    weights: Vec<f32>, // len == NUM_COMBOS, each in [0, 1]
}

impl Default for Range {
    fn default() -> Self {
        Range {
            weights: vec![0.0; NUM_COMBOS],
        }
    }
}

impl Range {
    /// Every combo at weight 1.
    pub fn full() -> Self {
        Range {
            weights: vec![1.0; NUM_COMBOS],
        }
    }

    pub fn weight(&self, combo: usize) -> f32 {
        self.weights[combo]
    }

    pub fn set_weight(&mut self, combo: usize, weight: f32) {
        assert!((0.0..=1.0).contains(&weight), "weight must be in [0, 1]");
        self.weights[combo] = weight;
    }

    pub fn weights(&self) -> &[f32] {
        &self.weights
    }

    /// Number of combos with non-zero weight.
    pub fn num_combos(&self) -> usize {
        self.weights.iter().filter(|&&w| w > 0.0).count()
    }

    /// Sum of all combo weights.
    pub fn total_weight(&self) -> f64 {
        self.weights.iter().map(|&w| w as f64).sum()
    }

    fn set_class(&mut self, hi: Rank, lo: Rank, suited: Option<bool>, weight: f32) {
        for a in 0..4u8 {
            for b in 0..4u8 {
                if hi == lo && a >= b {
                    continue; // pairs: unordered suit pairs, a < b
                }
                if hi != lo {
                    match suited {
                        Some(true) if a != b => continue,
                        Some(false) if a == b => continue,
                        _ => {}
                    }
                }
                let c1 = Card::new(hi, a);
                let c2 = Card::new(lo, b);
                self.set_weight(combo_index(c1, c2), weight);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid range entry: {0:?}")]
pub struct ParseRangeError(pub String);

/// Parses a range string.
///
/// Supported entry forms, comma-separated, later entries overwriting earlier
/// ones for the combos they cover:
///
/// - pairs: `AA`, `22+`, `TT-77`
/// - unpaired classes: `AKs`, `AKo`, `AK` (both), `A2s+`, `KTo+`, `ATs-A5s`
/// - explicit combos: `AhKh`
/// - per-entry weight suffix: `AA:0.5`, `A2s+:0.25`
impl FromStr for Range {
    type Err = ParseRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut range = Range::default();
        for raw in s.split(',') {
            let entry = raw.trim();
            if entry.is_empty() {
                continue;
            }
            let (spec, weight) = match entry.split_once(':') {
                Some((spec, w)) => {
                    let weight: f32 = w
                        .trim()
                        .parse()
                        .map_err(|_| ParseRangeError(entry.to_string()))?;
                    if !(0.0..=1.0).contains(&weight) {
                        return Err(ParseRangeError(entry.to_string()));
                    }
                    (spec.trim(), weight)
                }
                None => (entry, 1.0),
            };
            apply_spec(&mut range, spec, weight)
                .ok_or_else(|| ParseRangeError(entry.to_string()))?;
        }
        Ok(range)
    }
}

fn apply_spec(range: &mut Range, spec: &str, weight: f32) -> Option<()> {
    // Explicit combo like "AhKs".
    if spec.len() == 4 {
        let (a, b) = (spec[..2].parse::<Card>(), spec[2..].parse::<Card>());
        if let (Ok(a), Ok(b)) = (a, b) {
            if a == b {
                return None;
            }
            range.set_weight(combo_index(a, b), weight);
            return Some(());
        }
    }
    if let Some((lo_spec, hi_spec)) = split_dash(spec) {
        return apply_dash_range(range, hi_spec, lo_spec, weight);
    }
    let (class, plus) = match spec.strip_suffix('+') {
        Some(class) => (class, true),
        None => (spec, false),
    };
    let (hi, lo, suited) = parse_class(class)?;
    if plus {
        if hi == lo {
            // Pairs upward: 22+ => 22..AA
            for r in lo..=12 {
                range.set_class(r, r, None, weight);
            }
        } else {
            // Kicker upward: A2s+ => A2s..AKs
            for r in lo..hi {
                range.set_class(hi, r, suited, weight);
            }
        }
    } else {
        range.set_class(hi, lo, suited, weight);
    }
    Some(())
}

/// Splits `"TT-77"` into `("77", "TT")`, tolerating either order.
fn split_dash(spec: &str) -> Option<(&str, &str)> {
    let (a, b) = spec.split_once('-')?;
    Some((a.trim(), b.trim()))
}

fn apply_dash_range(range: &mut Range, first: &str, second: &str, weight: f32) -> Option<()> {
    let (hi1, lo1, s1) = parse_class(first)?;
    let (hi2, lo2, s2) = parse_class(second)?;
    if s1 != s2 {
        return None;
    }
    if hi1 == lo1 && hi2 == lo2 {
        // Pair range, e.g. TT-77.
        let (top, bottom) = (hi1.max(hi2), hi1.min(hi2));
        for r in bottom..=top {
            range.set_class(r, r, None, weight);
        }
        return Some(());
    }
    // Same-high-card kicker range, e.g. ATs-A5s.
    if hi1 != hi2 || hi1 == lo1 || hi2 == lo2 {
        return None;
    }
    let (top, bottom) = (lo1.max(lo2), lo1.min(lo2));
    for r in bottom..=top {
        range.set_class(hi1, r, s1, weight);
    }
    Some(())
}

/// Parses `"AA"`, `"AKs"`, `"AKo"`, `"AK"` into (hi, lo, suitedness).
fn parse_class(class: &str) -> Option<(Rank, Rank, Option<bool>)> {
    let chars: Vec<char> = class.chars().collect();
    let (r1, r2, suited) = match chars.as_slice() {
        [r1, r2] => (*r1, *r2, None),
        [r1, r2, 's'] | [r1, r2, 'S'] => (*r1, *r2, Some(true)),
        [r1, r2, 'o'] | [r1, r2, 'O'] => (*r1, *r2, Some(false)),
        _ => return None,
    };
    let r1 = rank_from_char(r1)?;
    let r2 = rank_from_char(r2)?;
    let (hi, lo) = (r1.max(r2), r1.min(r2));
    if hi == lo && suited.is_some() {
        return None; // "AAs" is invalid
    }
    Some((hi, lo, suited))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combos(s: &str) -> usize {
        s.parse::<Range>().unwrap().num_combos()
    }

    #[test]
    fn combo_index_bijection() {
        let mut seen = vec![false; NUM_COMBOS];
        for a in 0..52u8 {
            for b in 0..a {
                let idx = combo_index(Card::from_index(a), Card::from_index(b));
                assert!(!seen[idx]);
                seen[idx] = true;
                let (hi, lo) = combo_cards(idx);
                assert_eq!((hi.index(), lo.index()), (a as usize, b as usize));
            }
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn class_counts() {
        assert_eq!(combos("AA"), 6);
        assert_eq!(combos("AKs"), 4);
        assert_eq!(combos("AKo"), 12);
        assert_eq!(combos("AK"), 16);
        assert_eq!(combos("22+"), 13 * 6);
        assert_eq!(combos("A2s+"), 12 * 4);
        assert_eq!(combos("KTo+"), 3 * 12);
        assert_eq!(combos("TT-77"), 4 * 6);
        assert_eq!(combos("ATs-A5s"), 6 * 4);
        assert_eq!(combos("AhKh"), 1);
    }

    #[test]
    fn full_range() {
        assert_eq!(Range::full().num_combos(), NUM_COMBOS);
        // A "any two" written out: pairs + all suited + all offsuit.
        let r: Range =
            "22+, 32s+, 42s+, 52s+, 62s+, 72s+, 82s+, 92s+, T2s+, J2s+, Q2s+, K2s+, A2s+, \
                        32o+, 42o+, 52o+, 62o+, 72o+, 82o+, 92o+, T2o+, J2o+, Q2o+, K2o+, A2o+"
                .parse()
                .unwrap();
        assert_eq!(r.num_combos(), NUM_COMBOS);
    }

    #[test]
    fn weights() {
        let r: Range = "AA:0.5, KK".parse().unwrap();
        assert!((r.total_weight() - (6.0 * 0.5 + 6.0)).abs() < 1e-9);
        // Later entries overwrite earlier ones.
        let r: Range = "22+, AA:0.25".parse().unwrap();
        assert!((r.total_weight() - (12.0 * 6.0 + 6.0 * 0.25)).abs() < 1e-9);
    }

    #[test]
    fn class_index_layout() {
        // AA top-left, A2 offsuit bottom-left column-wise mirror.
        assert_eq!(class_index(12, 12, false), 0);
        assert_eq!(class_index(12, 11, true), 1); // AKs
        assert_eq!(class_index(12, 11, false), 13); // AKo
        assert_eq!(class_index(0, 0, false), 168); // 22
    }

    #[test]
    fn rejects_garbage() {
        assert!("XX".parse::<Range>().is_err());
        assert!("AAs".parse::<Range>().is_err());
        assert!("AA:1.5".parse::<Range>().is_err());
        assert!("AKs-QJs".parse::<Range>().is_err());
    }
}
