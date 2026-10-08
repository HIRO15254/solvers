//! The standard 169 classes and the hero-conditioned card-removal model.

use nlh::{Card, NUM_CLASSES, NUM_COMBOS, Range, class_index, combo_cards};
use std::sync::OnceLock;

/// Class of an unordered physical combo, using the standard range matrix.
pub fn class(combo: usize) -> usize {
    let (a, b) = combo_cards(combo);
    class_index(
        a.rank().max(b.rank()),
        a.rank().min(b.rank()),
        a.suit() == b.suit(),
    )
}

pub(crate) fn mask(cards: [Card; 2]) -> u64 {
    (1 << cards[0].index()) | (1 << cards[1].index())
}

/// Combo lists are ascending, so the first combo is the fixed representative.
pub struct Classes {
    combos: [Vec<usize>; NUM_CLASSES],
    cards: [[Card; 2]; NUM_COMBOS],
    masks: [u64; NUM_COMBOS],
    k: [[u8; NUM_CLASSES]; NUM_CLASSES],
}

impl Classes {
    pub fn get() -> &'static Self {
        static CLASSES: OnceLock<Classes> = OnceLock::new();
        CLASSES.get_or_init(|| {
            let mut combos: [Vec<usize>; NUM_CLASSES] = std::array::from_fn(|_| Vec::new());
            let cards = std::array::from_fn(|i| {
                let (a, b) = combo_cards(i);
                combos[class(i)].push(i);
                [a, b]
            });
            let masks = cards.map(mask);
            let k = std::array::from_fn(|c| {
                std::array::from_fn(|d| {
                    combos[d]
                        .iter()
                        .filter(|&&v| masks[v] & masks[combos[c][0]] == 0)
                        .count() as u8
                })
            });
            Self {
                combos,
                cards,
                masks,
                k,
            }
        })
    }

    pub fn combos(&self, c: usize) -> &[usize] {
        &self.combos[c]
    }
    pub fn representative(&self, c: usize) -> usize {
        self.combos[c][0]
    }
    pub fn n(&self, c: usize) -> usize {
        self.combos[c].len()
    }
    pub fn k(&self, c: usize, d: usize) -> u8 {
        self.k[c][d]
    }
    pub fn cards(&self, combo: usize) -> [Card; 2] {
        self.cards[combo]
    }
    pub(crate) fn combo_mask(&self, combo: usize) -> u64 {
        self.masks[combo]
    }

    pub fn name(&self, c: usize) -> String {
        let [a, b] = self.cards(self.representative(c));
        let hi = a.rank().max(b.rank());
        let lo = a.rank().min(b.rank());
        format!(
            "{}{}{}",
            char::from(b"23456789TJQKA"[hi as usize]),
            char::from(b"23456789TJQKA"[lo as usize]),
            if hi == lo {
                ""
            } else if a.suit() == b.suit() {
                "s"
            } else {
                "o"
            }
        )
    }

    /// Average within each class, recording every suit-asymmetric class.
    pub fn weights(&self, range: &Range) -> ClassWeights {
        let mut warnings = Vec::new();
        let weights = std::array::from_fn(|c| {
            let mut min = 1.0_f32;
            let mut max = 0.0_f32;
            let mut sum = 0.0;
            for &v in &self.combos[c] {
                let w = range.weight(v);
                min = min.min(w);
                max = max.max(w);
                sum += f64::from(w);
            }
            if min != max {
                warnings.push(SuitAsymmetry {
                    class: c,
                    name: self.name(c),
                    min,
                    max,
                });
            }
            sum / self.n(c) as f64
        });
        ClassWeights { weights, warnings }
    }
}

#[derive(Debug)]
pub struct SuitAsymmetry {
    pub class: usize,
    pub name: String,
    pub min: f32,
    pub max: f32,
}

pub struct ClassWeights {
    pub weights: [f64; NUM_CLASSES],
    pub warnings: Vec<SuitAsymmetry>,
}

/// Stable T2 payload order, from the hero's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Ordering2 {
    Win = 0,
    Tie = 1,
    Lose = 2,
}

/// Stable weak-ordering indices for (H, A, B). Larger ranks are stronger.
///
/// 0 H>A>B; 1 H>B>A; 2 A>H>B; 3 A>B>H; 4 B>H>A; 5 B>A>H;
/// 6 H=A>B; 7 H=B>A; 8 A=B>H; 9 H>A=B; 10 A>H=B; 11 B>H=A;
/// 12 H=A=B. These indices are part of the experimental cache format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ordering3(u8);

impl Ordering3 {
    pub const ALL: [Self; 13] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
        Self(8),
        Self(9),
        Self(10),
        Self(11),
        Self(12),
    ];
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
    pub fn from_index(index: usize) -> Option<Self> {
        (index < 13).then_some(Self(index as u8))
    }
    pub fn from_ranks(h: u16, a: u16, b: u16) -> Self {
        let i = if h == a && a == b {
            12
        } else if h == a {
            if h > b { 6 } else { 11 }
        } else if h == b {
            if h > a { 7 } else { 10 }
        } else if a == b {
            if h > a { 9 } else { 8 }
        } else if h > a {
            if a > b {
                0
            } else if h > b {
                1
            } else {
                4
            }
        } else if h > b {
            2
        } else if a > b {
            3
        } else {
            5
        };
        Self(i)
    }
    pub fn swap_opponents(self) -> Self {
        Self([1, 0, 4, 5, 2, 3, 7, 6, 8, 9, 11, 10, 12][self.index()])
    }
    pub fn ranks(self) -> [u16; 3] {
        [
            [3, 2, 1],
            [3, 1, 2],
            [2, 3, 1],
            [1, 3, 2],
            [2, 1, 3],
            [1, 2, 3],
            [2, 2, 1],
            [2, 1, 2],
            [1, 2, 2],
            [2, 1, 1],
            [1, 2, 1],
            [1, 1, 2],
            [1, 1, 1],
        ][self.index()]
    }
}
