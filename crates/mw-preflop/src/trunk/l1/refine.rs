//! Experimental refinements of a [`BucketSource`]: the strategy of a bucket is
//! shared by every board, so a refinement splits each bucket by public board
//! texture or by the hand's coarse bucket on the previous street.
use super::BucketSource;
use super::cards::{DRAW_CATEGORIES, HandFeatures, MADE_CATEGORIES};
use nlh::{Card, NUM_COMBOS, Street};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Refinement {
    /// Split by public texture: 0 none, 1 suits × paired, 2 also the high card on the flop.
    pub texture: u8,
    /// Equal ranges of the strength bucket recalled on later streets:
    /// previous street for `Refined`, original flop for `Relative` (1 = none).
    pub recall: u16,
}

impl Refinement {
    pub const NONE: Self = Self {
        texture: 0,
        recall: 1,
    };

    pub fn parse(value: &str) -> Option<Self> {
        let mut refinement = Self::NONE;
        for part in value.split(',').filter(|p| !p.is_empty() && *p != "none") {
            match part.split_once(':') {
                Some(("texture", v)) => refinement.texture = v.parse().ok().filter(|&t| t <= 2)?,
                Some(("recall", v)) => refinement.recall = v.parse().ok().filter(|&r| r >= 1)?,
                _ => return None,
            }
        }
        Some(refinement)
    }

    fn textures(self, street: Street) -> usize {
        match (self.texture, street) {
            (0, _) => 1,
            (1, Street::River) | (2, Street::River) => 4,
            (1, _) | (2, Street::Turn) => 6,
            (_, Street::Flop) => 24,
            _ => 1,
        }
    }

    fn texture_of(self, board: &[Card]) -> usize {
        if self.texture == 0 {
            return 0;
        }
        let mut suits = [0_u8; 4];
        let mut ranks = [0_u8; 13];
        for c in board {
            suits[c.suit() as usize] += 1;
            ranks[c.rank() as usize] += 1;
        }
        let paired = usize::from(ranks.iter().any(|&n| n >= 2));
        let most = *suits.iter().max().unwrap();
        let suited = suits.iter().filter(|&&n| n >= 2).count();
        if board.len() == 5 {
            return usize::from(most >= 3) * 2 + paired;
        }
        // Flop: monotone, two-tone, rainbow. Turn: flush possible, flush draw, none.
        let suit = if most >= 3 {
            0
        } else if suited >= 1 {
            1
        } else {
            2
        };
        let base = suit * 2 + paired;
        if board.len() == 3 && self.texture == 2 {
            let high = board.iter().map(|c| c.rank()).max().unwrap();
            let high = match high {
                12 => 0,
                11 => 1,
                9 | 10 => 2,
                _ => 3,
            };
            base * 4 + high
        } else {
            base
        }
    }
}

pub struct Refined<'a> {
    pub base: &'a dyn BucketSource,
    pub refinement: Refinement,
}

impl BucketSource for Refined<'_> {
    fn state_identity(&self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(b"refined");
        hash.update(&self.base.state_identity());
        hash.update(&postcard::to_allocvec(&self.refinement).unwrap());
        *hash.finalize().as_bytes()
    }

    fn count(&self, street: Street) -> usize {
        let recall = if street == Street::Flop {
            1
        } else {
            usize::from(self.refinement.recall)
        };
        self.base.count(street) * recall * self.refinement.textures(street)
    }

    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        let street = match board.len() {
            3 => Street::Flop,
            4 => Street::Turn,
            _ => Street::River,
        };
        let mut row = self.base.row(board);
        let n = self.base.count(street);
        let recall = usize::from(self.refinement.recall);
        if street != Street::Flop && recall > 1 {
            let previous = &board[..board.len() - 1];
            let before = self.base.row(previous);
            let m = self.base.count(if street == Street::Turn {
                Street::Flop
            } else {
                Street::Turn
            });
            for (b, &p) in row.iter_mut().zip(&before) {
                if *b != u16::MAX {
                    let coarse = usize::from(p) * recall / m;
                    *b = (coarse * n + usize::from(*b)) as u16;
                }
            }
        }
        let texture = self.refinement.texture_of(board);
        if texture > 0 {
            let width = n * if street == Street::Flop { 1 } else { recall };
            for b in row.iter_mut().filter(|b| **b != u16::MAX) {
                *b = (texture * width + usize::from(*b)) as u16;
            }
        }
        row
    }
}

/// Buckets by percentile within the actor's own reach at each node, ordered by
/// a (finer) strength table on the flop and turn and the showdown rank on the
/// river, crossed with `levels` equal ranges of that table's bucket, with the
/// public texture and persistent coarse flop strength of `refinement`, and
/// with the group of the flop in `flops`.
#[derive(Clone, Copy)]
pub struct Relative<'a> {
    pub strength: &'a dyn BucketSource,
    /// Percentile buckets on the flop, turn and river.
    pub counts: [usize; 3],
    /// Absolute strength levels per percentile bucket (1 = percentile only).
    pub levels: usize,
    /// Uniform prior mass in combos (finite and nonnegative; 0 = reach only).
    pub prior: f64,
    /// Experimental extra prior mass as a fraction of the actor's reach total
    /// at the node (finite and nonnegative; 0 = combos only). The node's prior
    /// mass is `prior + prior_fraction * reach total`.
    pub prior_fraction: f64,
    /// Strategy reaches used for the bucket mapping during training.
    pub reach: RelativeReach,
    pub refinement: Refinement,
    /// Experimental flop groups splitting the buckets on every street.
    pub flops: Option<&'a FlopClusters>,
    /// Experimental split by the combo's own hand on the flop and turn.
    pub hand: HandSplit,
    /// Experimental split of the turn and river by the card just dealt.
    pub runout: Runout,
    /// Experimental: per street (flop, turn, river), whether a pass replaces
    /// the `levels` part of each class with equal ranges of the combo's
    /// showdown equity on the board so far against the opponent's reach at
    /// the node (`BucketSource::opponent_levels`).
    pub opponent_levels: [bool; 3],
}

/// Parses `none` or a comma list of `flop`, `turn` and `river` into
/// [`Relative::opponent_levels`].
pub fn parse_streets(spec: &str) -> anyhow::Result<[bool; 3]> {
    let mut streets = [false; 3];
    for part in spec.split(',').filter(|p| *p != "none") {
        let s = ["flop", "turn", "river"]
            .iter()
            .position(|s| *s == part)
            .ok_or_else(|| anyhow::anyhow!("unknown street {part} (flop, turn, river)"))?;
        streets[s] = true;
    }
    Ok(streets)
}

/// Experimental split of turn and river buckets by the kind of the card just
/// dealt. Each enabled kind is a class; a card takes the first enabled kind
/// it matches in the order flush, pair, straight, over, and class 0 (blank)
/// if none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Runout {
    /// Bit `k` enables kind `k` of [`Runout::KINDS`].
    kinds: u8,
}

impl Runout {
    /// `flush`: three or more of the card's suit on the board. `pair`: the
    /// card's rank was on the board. `straight`: the card makes three ranks
    /// within five consecutive ranks (ace also low) possible for the first
    /// time. `over`: the card is above every earlier board card.
    pub const KINDS: [&'static str; 4] = ["flush", "pair", "straight", "over"];

    /// Parses `none` or a comma list of [`Runout::KINDS`].
    pub fn parse(spec: &str) -> anyhow::Result<Self> {
        let mut kinds = 0;
        for part in spec.split(',').filter(|p| *p != "none") {
            let k = Self::KINDS.iter().position(|k| *k == part).ok_or_else(|| {
                anyhow::anyhow!("unknown runout kind {part} (flush, pair, straight, over)")
            })?;
            kinds |= 1 << k;
        }
        Ok(Self { kinds })
    }

    fn classes(self, street: Street) -> usize {
        if street == Street::Flop {
            1
        } else {
            1 + self.kinds.count_ones() as usize
        }
    }

    /// The class of the last card of a turn or river board (0 on the flop).
    fn class_of(self, board: &[Card]) -> usize {
        if board.len() == 3 || self.kinds == 0 {
            return 0;
        }
        let (card, before) = board.split_last().unwrap();
        let suited = board.iter().filter(|c| c.suit() == card.suit()).count();
        let ranks = |cards: &[Card]| cards.iter().fold(0_u16, |m, c| m | 1 << c.rank());
        let connected = |mask: u16| {
            // Ranks 0..13 are 2..A; bit 0 of the shifted mask is the low ace.
            let mask = (mask << 1) | (mask >> 12 & 1);
            (0..10).any(|low| (mask >> low & 0x1f).count_ones() >= 3)
        };
        let matched = [
            suited >= 3,
            before.iter().any(|c| c.rank() == card.rank()),
            connected(ranks(board)) && !connected(ranks(before)),
            before.iter().all(|c| c.rank() < card.rank()),
        ];
        let mut class = 0;
        for (k, &m) in matched.iter().enumerate() {
            if self.kinds >> k & 1 == 1 {
                class += 1;
                if m {
                    return class;
                }
            }
        }
        0
    }
}

/// Experimental split of relative buckets by what makes a combo strong: its
/// made hand now, its draws or its potential on the flop and turn, or its
/// made-hand and draw categories (`cards::category`) on every street.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct HandSplit {
    /// Equal ranges of the made-hand percentile among the live combos
    /// (1 = no split).
    pub made: usize,
    /// Split by flush draw and open-ended straight draw (`cards::draws`).
    pub draws: bool,
    /// Equal ranges of the potential percentile (`BucketSource::hand_potential`:
    /// the variance of the strength bucket over the next card) among the live
    /// combos (1 = no split). Hashed separately so that the identities of
    /// earlier splits stay unchanged.
    #[serde(skip)]
    pub potential: usize,
    /// 0 = no category split; N = split by the made-hand and draw category
    /// (`cards::category`), each category into N equal ranges of the strength
    /// percentile among its live combos. Excludes the other splits. Hashed
    /// separately like `potential`.
    #[serde(skip)]
    pub category: usize,
    /// Keep whether the hand had a flush draw or OESD on the original flop,
    /// as a binary split on the turn and river.
    #[serde(skip)]
    pub flop_draws: bool,
    /// Add a pocket-pair / non-pair split on the flop and turn.
    #[serde(skip)]
    pub pairs: bool,
    /// With `draws`, split the flop's no-flush-draw combos by a backdoor flush
    /// draw (`cards::weak_draws`): three flush levels on the flop.
    #[serde(skip)]
    pub backdoor: bool,
    /// With `draws`, split the flop's no-OESD combos by a gutshot
    /// (`cards::weak_draws`): three straight levels on the flop.
    #[serde(skip)]
    pub gutshot: bool,
}

impl Default for HandSplit {
    fn default() -> Self {
        Self {
            made: 1,
            draws: false,
            potential: 1,
            category: 0,
            flop_draws: false,
            pairs: false,
            backdoor: false,
            gutshot: false,
        }
    }
}

impl HandSplit {
    /// Parses `none` or a comma list of `made:N`, `draws`, `potential:N`,
    /// `flop-draws`, `pairs`, `backdoor` and `gutshot` (the last two with
    /// `draws`), or `category` / `category:N` alone.
    pub fn parse(spec: &str) -> anyhow::Result<Self> {
        let mut split = Self::default();
        for part in spec.split(',').filter(|p| *p != "none") {
            if part == "draws" {
                split.draws = true;
            } else if part == "flop-draws" {
                split.flop_draws = true;
            } else if part == "pairs" {
                split.pairs = true;
            } else if part == "backdoor" {
                split.backdoor = true;
            } else if part == "gutshot" {
                split.gutshot = true;
            } else if let Some(n) = part.strip_prefix("made:") {
                split.made = n.parse()?;
                anyhow::ensure!(split.made >= 1, "made levels must be at least 1");
            } else if let Some(n) = part.strip_prefix("potential:") {
                split.potential = n.parse()?;
                anyhow::ensure!(split.potential >= 1, "potential levels must be at least 1");
            } else if part == "category" {
                split.category = 1;
            } else if let Some(n) = part.strip_prefix("category:") {
                split.category = n.parse()?;
                anyhow::ensure!(split.category >= 1, "category levels must be at least 1");
            } else {
                anyhow::bail!(
                    "unknown hand split {part} (made:N, draws, potential:N, flop-draws, pairs, backdoor, gutshot, category[:N])"
                );
            }
        }
        anyhow::ensure!(
            split.category == 0
                || (split.made == 1
                    && !split.draws
                    && split.potential == 1
                    && !split.flop_draws
                    && !split.pairs),
            "category excludes the made, draws, potential, flop-draws and pairs splits"
        );
        anyhow::ensure!(
            split.draws || !(split.backdoor || split.gutshot),
            "backdoor and gutshot require draws"
        );
        Ok(split)
    }

    /// Flush levels times straight levels: 2 × 2 with `draws`, and on the flop
    /// one more flush level with `backdoor` and straight level with `gutshot`.
    fn draw_levels(self, street: Street) -> (usize, usize) {
        if !self.draws {
            return (1, 1);
        }
        let flop = street == Street::Flop;
        (
            2 + usize::from(flop && self.backdoor),
            2 + usize::from(flop && self.gutshot),
        )
    }

    fn draw_classes(self, street: Street) -> usize {
        let (flush, straight) = self.draw_levels(street);
        flush * straight
    }

    fn pair_classes(self) -> usize {
        if self.pairs { 2 } else { 1 }
    }

    fn classes(self, street: Street) -> usize {
        if self.category > 0 {
            let draws = if street == Street::River {
                1
            } else {
                DRAW_CATEGORIES
            };
            MADE_CATEGORIES * draws * self.category
        } else if street == Street::River {
            1
        } else {
            self.made * self.draw_classes(street) * self.potential * self.pair_classes()
        }
    }
}

/// Experimental groups of flops: a strategy is shared only by the flops of one
/// group (on the flop and on every later street of those flops).
/// Suit-isomorphic flops always share a group.
pub struct FlopClusters {
    /// Group by raw flop `(a * 52 + b) * 52 + c` of ascending card indices.
    groups: Vec<u16>,
    count: usize,
    identity: [u8; 32],
    /// Split only the flop; the turn and river share across groups.
    pub flop_only: bool,
}

impl FlopClusters {
    /// Parses `card card card group` lines (`Ks 7h 2d 3`) naming every one of
    /// the 1755 suit-isomorphism classes of flops once; groups are 0..count
    /// and each is used.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let mut by_class = std::collections::BTreeMap::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            anyhow::ensure!(
                parts.len() == 4,
                "flop group line {line} is not `card card card group`"
            );
            let cards: Vec<Card> = parts[..3]
                .iter()
                .map(|c| c.parse().map_err(|e| anyhow::anyhow!("card {c}: {e:?}")))
                .collect::<anyhow::Result<_>>()?;
            let flop: [Card; 3] = cards.try_into().unwrap();
            anyhow::ensure!(
                flop[0] != flop[1] && flop[0] != flop[2] && flop[1] != flop[2],
                "flop {line} repeats a card"
            );
            let group: u16 = parts[3].parse()?;
            anyhow::ensure!(
                by_class.insert(canonical_flop(flop), group).is_none(),
                "flop {line} repeats an isomorphic flop"
            );
        }
        Self::from_classes(&by_class)
    }

    /// Groups by suit-isomorphism class of flop, naming every one of the 1755
    /// classes; groups are 0..count and each is used.
    pub(super) fn from_classes(
        by_class: &std::collections::BTreeMap<FlopClass, u16>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(by_class.len() == 1755, "{} of 1755 flops", by_class.len());
        let count = usize::from(*by_class.values().max().unwrap()) + 1;
        let mut used = vec![false; count];
        for &g in by_class.values() {
            used[usize::from(g)] = true;
        }
        anyhow::ensure!(used.iter().all(|&u| u), "flop groups are not 0..{count}");
        let mut groups = vec![u16::MAX; 52 * 52 * 52];
        for a in 0..52_u8 {
            for b in a + 1..52 {
                for c in b + 1..52 {
                    let flop = [a, b, c].map(Card::from_index);
                    groups[(usize::from(a) * 52 + usize::from(b)) * 52 + usize::from(c)] =
                        by_class[&canonical_flop(flop)];
                }
            }
        }
        let mut hash = blake3::Hasher::new();
        for g in by_class.values() {
            hash.update(&g.to_le_bytes());
        }
        Ok(Self {
            groups,
            count,
            identity: *hash.finalize().as_bytes(),
            flop_only: false,
        })
    }

    pub fn count(&self) -> usize {
        self.count
    }

    fn groups(&self, street: Street) -> usize {
        if self.flop_only && street != Street::Flop {
            1
        } else {
            self.count
        }
    }

    pub(super) fn group(&self, board: &[Card]) -> usize {
        if self.flop_only && board.len() > 3 {
            return 0;
        }
        let mut f = [board[0].index(), board[1].index(), board[2].index()].map(usize::from);
        f.sort_unstable();
        usize::from(self.groups[(f[0] * 52 + f[1]) * 52 + f[2]])
    }
}

/// A flop's suit-isomorphism class (`canonical_flop`).
pub type FlopClass = [(u8, u8); 3];

/// The suit-isomorphism class of a flop: its (rank, suit) pairs under the
/// suit relabelling that gives the smallest descending list.
pub fn canonical_flop(flop: [Card; 3]) -> FlopClass {
    let mut best = [(u8::MAX, u8::MAX); 3];
    for a in 0..4_u8 {
        for b in 0..4 {
            for c in 0..4 {
                if a == b || a == c || b == c {
                    continue;
                }
                let p = [a, b, c, 6 - a - b - c];
                let mut key = flop.map(|x| (x.rank(), p[x.suit() as usize]));
                key.sort_unstable_by(|x, y| y.cmp(x));
                best = best.min(key);
            }
        }
    }
    best
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RelativeReach {
    #[default]
    Current,
    Average,
}

impl BucketSource for Relative<'_> {
    fn state_identity(&self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(b"relative");
        hash.update(&self.strength.state_identity());
        hash.update(&postcard::to_allocvec(&(self.counts, self.levels, self.refinement)).unwrap());
        // Preserve the identity of existing states at the experimental defaults.
        if self.prior != 0.0 {
            hash.update(b"relative-prior");
            hash.update(&self.prior.to_le_bytes());
        }
        if self.prior_fraction != 0.0 {
            hash.update(b"relative-prior-fraction");
            hash.update(&self.prior_fraction.to_le_bytes());
        }
        if self.reach == RelativeReach::Average {
            hash.update(b"relative-reach-average");
        }
        if let Some(flops) = self.flops {
            hash.update(b"relative-flop-groups");
            hash.update(&flops.identity);
            if flops.flop_only {
                hash.update(b"flop-only");
            }
        }
        if self.hand != HandSplit::default() {
            hash.update(b"relative-hand");
            hash.update(&postcard::to_allocvec(&self.hand).unwrap());
        }
        if self.hand.potential != 1 {
            hash.update(b"relative-potential");
            hash.update(&(self.hand.potential as u64).to_le_bytes());
        }
        if self.hand.category != 0 {
            hash.update(b"relative-category");
            hash.update(&(self.hand.category as u64).to_le_bytes());
        }
        if self.hand.flop_draws {
            hash.update(b"relative-flop-draws");
        }
        if self.hand.pairs {
            hash.update(b"relative-pairs");
        }
        if self.hand.backdoor {
            hash.update(b"relative-backdoor");
        }
        if self.hand.gutshot {
            hash.update(b"relative-gutshot");
        }
        if self.refinement.recall > 1 {
            // Older Relative sources accepted recall but did not apply it.
            hash.update(b"relative-flop-recall");
        }
        if self.runout != Runout::default() {
            hash.update(b"relative-runout");
            hash.update(&[self.runout.kinds]);
        }
        if self.opponent_levels != [false; 3] {
            hash.update(b"relative-opponent-levels");
            hash.update(&self.opponent_levels.map(u8::from));
        }
        *hash.finalize().as_bytes()
    }

    fn count(&self, street: Street) -> usize {
        self.counts[street.index() - 1] * self.classes(street)
    }

    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        self.strength.row(board)
    }

    fn relative(&self) -> bool {
        true
    }

    fn relative_prior(&self) -> f64 {
        self.prior
    }

    fn relative_prior_fraction(&self) -> f64 {
        self.prior_fraction
    }

    fn relative_average(&self) -> bool {
        self.reach == RelativeReach::Average
    }

    fn levels(&self) -> usize {
        self.levels
    }

    fn opponent_levels(&self, street: Street) -> bool {
        self.opponent_levels[street.index() - 1]
    }

    fn classes(&self, street: Street) -> usize {
        self.levels
            * self.refinement.textures(street)
            * self.flops.map_or(1, |f| f.groups(street))
            * self.hand.classes(street)
            * self.runout.classes(street)
            * self.flop_recall_classes(street)
    }

    fn hand_classes(&self, street: Street) -> usize {
        self.hand.classes(street)
    }

    fn hand_class(&self, street: Street, f: &HandFeatures) -> u16 {
        let level = |p: f64, n: usize| ((p * n as f64) as usize).min(n - 1);
        if self.hand.category > 0 {
            return (f.category * self.hand.category + level(f.within, self.hand.category)) as u16;
        }
        let draws = match self.hand.draw_levels(street) {
            (1, 1) => 0,
            // The original encoding: bit 0 the flush draw, bit 1 the OESD.
            (2, 2) => usize::from(f.draws),
            (flush_levels, straight_levels) => {
                let weak = |bit: u8, levels: usize| levels == 3 && f.weak_draws & bit != 0;
                let flush = if f.draws & 1 != 0 {
                    flush_levels - 1
                } else {
                    usize::from(weak(1, flush_levels))
                };
                let straight = if f.draws & 2 != 0 {
                    straight_levels - 1
                } else {
                    usize::from(weak(2, straight_levels))
                };
                flush * straight_levels + straight
            }
        };
        let class = level(f.made, self.hand.made) * self.hand.draw_classes(street) + draws;
        let class = class * self.hand.potential + level(f.potential, self.hand.potential);
        (class * self.hand.pair_classes() + usize::from(self.hand.pairs && f.pocket_pair)) as u16
    }

    fn hand_potential(&self, street: Street) -> bool {
        self.hand.potential > 1 && street != Street::River
    }

    fn hand_category(&self) -> bool {
        self.hand.category > 0
    }
    fn flop_recall_classes(&self, street: Street) -> usize {
        if street == Street::Flop {
            1
        } else {
            usize::from(self.refinement.recall) * if self.hand.flop_draws { 2 } else { 1 }
        }
    }

    fn flop_recall_class(&self, flop: &[Card], hole: [Card; 2], strength: u16) -> u16 {
        // Reuse Refined's coarse bucket scaling, but keep the original flop
        // rather than replacing it with the turn when classifying the river.
        let level = usize::from(strength) * usize::from(self.refinement.recall)
            / self.strength.count(Street::Flop);
        let draw_classes = if self.hand.flop_draws { 2 } else { 1 };
        let draw = usize::from(self.hand.flop_draws && super::cards::draws(flop, hole) != 0);
        (level * draw_classes + draw) as u16
    }

    fn class(&self, board: &[Card], strength: u16) -> u16 {
        let street = [Street::Flop, Street::Turn, Street::River][board.len() - 3];
        let level = usize::from(strength) * self.levels / self.strength.count(street);
        let group = self.flops.map_or(0, |f| f.group(board));
        let texture = group * self.refinement.textures(street) + self.refinement.texture_of(board);
        let texture = texture * self.runout.classes(street) + self.runout.class_of(board);
        (texture * self.levels + level) as u16
    }
}
