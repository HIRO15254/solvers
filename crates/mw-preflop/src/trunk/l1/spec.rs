//! The L1 card abstraction (docs/plans/p2-method-redesign.jp.md §3.4): range-relative
//! buckets split by flop group, the combo's own hand and the runout card, with the
//! recommended values and every part replaceable by hand.
use super::BucketSource;
use super::cards::HandFeatures;
use super::own::{FlopFeatures, OWN_FLOP_GROUPS, own_flop_groups};
use super::refine::{FlopClusters, HandSplit, Refinement, Relative, RelativeReach, Runout};
use super::tree::Subtree;
use crate::trunk::l0::{ClassProfileDocument, Model, Profile, Tree};
use anyhow::{Context, Result, ensure};
use nlh::{Card, NUM_COMBOS, Street};
use std::path::{Path, PathBuf};

/// The bundled flop groups: weighted k-medoids (K = 49) of the 1755
/// isomorphic flops by the mean 128-bucket EHS2 strength of the 169 hand
/// classes, stratified by suits and pairing
/// (experiments/p2-method-2026-10/gtow-fixed/subsets/k49-groups.txt).
pub const RECOMMENDED_FLOP_GROUPS: &str = include_str!("flop_groups_k49.txt");

impl FlopClusters {
    /// The bundled groups ([`RECOMMENDED_FLOP_GROUPS`]) on every street.
    pub fn recommended() -> Self {
        Self::parse(RECOMMENDED_FLOP_GROUPS).expect("bundled flop groups are valid")
    }

    /// `none`, `recommended` or a file of [`FlopClusters::parse`], the last
    /// two optionally followed by `:flop` to split only the flop.
    pub fn from_spec(spec: &str) -> anyhow::Result<Option<Self>> {
        if spec == "none" {
            return Ok(None);
        }
        let (name, flop_only) = match spec.strip_suffix(":flop") {
            Some(name) => (name, true),
            None => (spec, false),
        };
        let mut groups = if name == "recommended" {
            Self::recommended()
        } else {
            let text = std::fs::read_to_string(name)
                .map_err(|e| anyhow::anyhow!("reading flop groups {name}: {e}"))?;
            Self::parse(&text)?
        };
        groups.flop_only = flop_only;
        Ok(Some(groups))
    }
}

/// An L1 card abstraction: which combos share a Postflop strategy.
pub struct CardSpec {
    /// Percentile buckets of the actor's reach on the flop, turn and river.
    pub buckets: [usize; 3],
    /// Absolute strength levels crossed with each percentile bucket.
    pub levels: usize,
    /// Uniform prior mass in combos mixed into the reach percentiles.
    pub prior: f64,
    pub reach: RelativeReach,
    /// Flop groups splitting the buckets (none: one strategy for all flops).
    pub flops: Option<FlopClusters>,
    /// Own flop groups at every L1 leaf (P2D12): this many groups from the
    /// leaf's two ranges ([`own_flop_groups`]) replace `flops` there. The
    /// ranges come from a class profile, such as that of a solve with
    /// `flops` alone; leaves no seat reaches keep `flops`.
    pub leaf_flops: Option<usize>,
    pub hand: HandSplit,
    pub runout: Runout,
    /// Streets (flop, turn, river) whose strength levels are equal ranges of
    /// the showdown equity against the opponent's reach at the node.
    pub opponent_levels: [bool; 3],
    /// The cards of low-SPR L1 leaves ([`LeafCardSpec::low_spr_levels`]).
    pub low_spr_levels: Option<LowSprLevels>,
}

impl CardSpec {
    /// The recommended abstraction: 16 percentile buckets × 4 strength
    /// levels by the showdown equity against the opponent's reach on every
    /// street, 2 levels at L1 leaves whose flop SPR is at most 2.5 (P2D14),
    /// a 10-combo prior, 49 flop groups on every street (each L1 leaf's own
    /// from its ranges, the bundled ones where they are not known), and
    /// flush/open-ended straight draws and pocket pairs split on the flop
    /// and turn.
    pub fn recommended() -> Self {
        Self {
            buckets: [16; 3],
            levels: 4,
            prior: 10.0,
            reach: RelativeReach::Current,
            flops: Some(FlopClusters::recommended()),
            leaf_flops: Some(OWN_FLOP_GROUPS),
            hand: HandSplit {
                draws: true,
                pairs: true,
                ..HandSplit::default()
            },
            runout: Runout::default(),
            opponent_levels: [true; 3],
            low_spr_levels: Some(LowSprLevels {
                max_spr: 2.5,
                buckets: None,
                levels: 2,
            }),
        }
    }

    /// The bucket source over `strength`, a 128-bucket EHS2 table.
    pub fn source<'a>(&'a self, strength: &'a dyn BucketSource) -> Relative<'a> {
        Relative {
            strength,
            counts: self.buckets,
            levels: self.levels,
            prior: self.prior,
            prior_fraction: 0.0,
            reach: self.reach,
            refinement: Refinement::NONE,
            flops: self.flops.as_ref(),
            hand: self.hand,
            runout: self.runout,
            opponent_levels: self.opponent_levels,
        }
    }
}

/// Experimental refinement of only low-SPR leaves. The rest of the card
/// specification (strength table, prior, hand and runout splits) is unchanged.
pub struct LowSprSpec {
    pub max_spr: f64,
    pub buckets: usize,
    pub levels: usize,
    pub flops: Option<FlopClusters>,
}

impl LowSprSpec {
    /// `MAX_SPR:BUCKETS[xLEVELS]:GROUPS`, where GROUPS follows
    /// [`FlopClusters::from_spec`], including its optional `:flop` suffix.
    pub fn parse(value: &str) -> Result<Self> {
        let mut parts = value.splitn(3, ':');
        let max_spr: f64 = parts.next().context("missing MAX_SPR")?.parse()?;
        let counts = parts.next().context("missing BUCKETS[xLEVELS]")?;
        let (buckets, levels) = counts.split_once('x').unwrap_or((counts, "1"));
        let buckets = buckets.parse()?;
        let levels = levels.parse()?;
        ensure!(
            max_spr.is_finite() && max_spr >= 0.0,
            "MAX_SPR must be finite and nonnegative"
        );
        ensure!(
            buckets > 0
                && buckets <= usize::from(u16::MAX)
                && levels > 0
                && levels <= usize::from(u16::MAX),
            "BUCKETS and LEVELS must be in 1..=65535"
        );
        let flops = FlopClusters::from_spec(parts.next().context("missing GROUPS")?)?;
        Ok(Self {
            max_spr,
            buckets,
            levels,
            flops,
        })
    }
}

/// A baseline relative source with a finer mapping for selected leaves.
/// Boards share showdown ranks, strength sorting and own-hand classification
/// between both mappings; higher-SPR leaf strategy arrays retain their size.
pub struct LowSprCards<'a> {
    base: Relative<'a>,
    finer: Relative<'a>,
    max_spr: f64,
}

impl<'a> LowSprCards<'a> {
    pub fn new(base: Relative<'a>, spec: &'a LowSprSpec) -> Result<Self> {
        ensure!(
            base.opponent_levels == [false; 3],
            "opponent levels do not support a low-SPR mapping"
        );
        let finer = Relative {
            counts: [spec.buckets; 3],
            levels: spec.levels,
            flops: spec.flops.as_ref(),
            ..base
        };
        for street in [Street::Flop, Street::Turn, Street::River] {
            ensure!(
                finer.count(street) <= usize::from(u16::MAX),
                "low-SPR {street:?} bucket count exceeds u16"
            );
        }
        Ok(Self {
            base,
            finer,
            max_spr: spec.max_spr,
        })
    }
}

impl BucketSource for LowSprCards<'_> {
    fn state_identity(&self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(b"low-spr-cards");
        hash.update(&self.max_spr.to_le_bytes());
        hash.update(&self.base.state_identity());
        hash.update(&self.finer.state_identity());
        *hash.finalize().as_bytes()
    }
    fn low_spr(&self) -> Option<(f64, &dyn BucketSource)> {
        Some((self.max_spr, &self.finer))
    }
    fn count(&self, street: Street) -> usize {
        self.base.count(street)
    }
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        self.base.row(board)
    }
    fn relative(&self) -> bool {
        true
    }
    fn relative_prior(&self) -> f64 {
        self.base.relative_prior()
    }
    fn relative_prior_fraction(&self) -> f64 {
        self.base.relative_prior_fraction()
    }
    fn relative_average(&self) -> bool {
        self.base.relative_average()
    }
    fn classes(&self, street: Street) -> usize {
        self.base.classes(street)
    }
    fn class(&self, board: &[Card], strength: u16) -> u16 {
        self.base.class(board, strength)
    }
    fn hand_classes(&self, street: Street) -> usize {
        self.base.hand_classes(street)
    }
    fn hand_class(&self, street: Street, features: &HandFeatures) -> u16 {
        self.base.hand_class(street, features)
    }
    fn hand_potential(&self, street: Street) -> bool {
        self.base.hand_potential(street)
    }
    fn hand_category(&self) -> bool {
        self.base.hand_category()
    }
    fn flop_recall_classes(&self, street: Street) -> usize {
        self.base.flop_recall_classes(street)
    }
    fn flop_recall_class(&self, flop: &[Card], hole: [Card; 2], strength: u16) -> u16 {
        self.base.flop_recall_class(flop, hole, strength)
    }
}

/// The tree node of the L1 leaf at the end of `line`, `/`-separated action
/// labels from the root.
pub fn leaf_of_line(tree: &Tree, line: &str) -> Result<usize> {
    let mut z = 0;
    for label in line.split('/').filter(|l| !l.is_empty()) {
        let n = &tree.nodes[z];
        let a = n
            .labels
            .iter()
            .position(|l| l == label)
            .with_context(|| format!("no action {label} at node {z}: {:?}", n.labels))?;
        z = n.children[a];
    }
    ensure!(
        tree.nodes[z]
            .terminal
            .as_ref()
            .is_some_and(|t| t.l1.is_some()),
        "line {line} does not end at an L1 leaf"
    );
    Ok(z)
}

/// Experimental per-leaf flop groups: `LINE GROUPS` rows, GROUPS as in
/// [`FlopClusters::from_spec`] with a file path relative to `dir`.
pub fn parse_leaf_flop_groups(
    text: &str,
    dir: &Path,
) -> Result<Vec<(String, Option<FlopClusters>)>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|row| {
            let parts: Vec<&str> = row.split_whitespace().collect();
            ensure!(
                parts.len() == 2,
                "leaf flop group row {row} is not `LINE GROUPS`"
            );
            let (name, suffix) = parts[1]
                .strip_suffix(":flop")
                .map_or((parts[1], ""), |name| (name, ":flop"));
            let spec = if name == "none" || name == "recommended" {
                parts[1].to_string()
            } else {
                format!("{}{suffix}", dir.join(name).display())
            };
            Ok((parts[0].to_string(), FlopClusters::from_spec(&spec)?))
        })
        .collect()
}

/// Flop groups by L1 leaf tree node (`None` = no groups at that leaf).
type LeafGroups = Vec<(usize, Option<FlopClusters>)>;

/// The experimental CLIs' `--l1-leaf-flop-groups`: which L1 leaves have
/// their own flop groups instead of the base mapping's.
#[derive(Clone, Debug, PartialEq)]
pub enum LeafFlopGroups {
    /// `none`: no leaf.
    None,
    /// `own[:K]:PROFILE`: every leaf that a seat reaches under the class
    /// profile PROFILE, with K groups from its ranges ([`own_flop_groups`],
    /// default [`OWN_FLOP_GROUPS`]).
    Own { groups: usize, profile: PathBuf },
    /// A file of `LINE GROUPS` rows ([`parse_leaf_flop_groups`]).
    Rows(PathBuf),
}

impl LeafFlopGroups {
    pub fn parse(value: &str) -> Result<Self> {
        if value == "none" {
            return Ok(Self::None);
        }
        let Some(rest) = value.strip_prefix("own:") else {
            return Ok(Self::Rows(value.into()));
        };
        // Only digits make a count, so `own:C:/x.json` is a profile path.
        let (groups, profile) = match rest.split_once(':') {
            Some((k, path)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => {
                (k.parse()?, path)
            }
            _ => (OWN_FLOP_GROUPS, rest),
        };
        ensure!(
            !profile.is_empty(),
            "own leaf flop groups are own[:K]:PROFILE"
        );
        Ok(Self::Own {
            groups,
            profile: profile.into(),
        })
    }

    /// The leaves' flop groups by tree node for [`LeafCardSpec::flops`],
    /// `None` for `none`. Own groups read the profile and compare flops by
    /// `strength`, the relative buckets' 128-bucket EHS2 table.
    pub fn resolve(
        &self,
        tree: &Tree,
        model: &Model<'_>,
        strength: &dyn BucketSource,
    ) -> Result<Option<LeafGroups>> {
        match self {
            Self::None => Ok(None),
            Self::Rows(path) => {
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                parse_leaf_flop_groups(&text, path.parent().unwrap_or(Path::new(".")))?
                    .into_iter()
                    .map(|(line, groups)| Ok((leaf_of_line(tree, &line)?, groups)))
                    .collect::<Result<_>>()
                    .map(Some)
            }
            Self::Own { groups, profile } => {
                let document: ClassProfileDocument = serde_json::from_slice(
                    &std::fs::read(profile)
                        .with_context(|| format!("reading {}", profile.display()))?,
                )?;
                let profile = Profile::from_json(tree, &document)?;
                drop(document);
                let features = FlopFeatures::new(strength);
                Ok(Some(
                    own_flop_groups(tree, model, &profile, &features, *groups)?
                        .into_iter()
                        .map(|(z, g)| (z, Some(g)))
                        .collect(),
                ))
            }
        }
    }
}

/// Which leaves of a [`LeafCards`] mapping change.
pub struct LeafCardSpec<'a> {
    /// Flop groups by leaf tree node (`None` = no groups) instead of the base's.
    pub flops: Vec<(usize, Option<&'a FlopClusters>)>,
    /// The runout split, applied where the leaf's strategy with it has at
    /// most this many slots: the size of the tree after the leaf decides.
    pub runout: Option<(Runout, usize)>,
    /// Leaves whose flop SPR is at most `max_spr` have these percentile
    /// buckets and strength levels instead of the base's.
    pub low_spr_levels: Option<LowSprLevels>,
}

/// `MAX_SPR:[BUCKETSx]LEVELS` ([`parse_low_spr_levels`]): L1 leaves whose
/// flop SPR is at most `max_spr` have `buckets` percentile buckets (the
/// base's when `None`) and `levels` strength levels. `levels` divides the
/// base's levels, which come from the opponent's range on every street.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LowSprLevels {
    pub max_spr: f64,
    pub buckets: Option<usize>,
    pub levels: usize,
}

/// `MAX_SPR:[BUCKETSx]LEVELS` of [`LeafCardSpec::low_spr_levels`], or `none`.
pub fn parse_low_spr_levels(value: &str) -> Result<Option<LowSprLevels>> {
    if value == "none" {
        return Ok(None);
    }
    let (max_spr, cards) = value
        .split_once(':')
        .context("expected MAX_SPR:[BUCKETSx]LEVELS or none")?;
    let max_spr: f64 = max_spr.parse().context("MAX_SPR")?;
    let (buckets, levels) = match cards.split_once('x') {
        Some((buckets, levels)) => (Some(buckets.parse().context("BUCKETS")?), levels),
        None => (None, cards),
    };
    let levels: usize = levels.parse().context("LEVELS")?;
    ensure!(
        max_spr.is_finite() && max_spr >= 0.0,
        "MAX_SPR must be finite and nonnegative"
    );
    ensure!(
        levels > 0 && buckets != Some(0),
        "BUCKETS and LEVELS must be positive"
    );
    Ok(Some(LowSprLevels {
        max_spr,
        buckets,
        levels,
    }))
}

/// Experimental per-leaf card mapping: a base relative mapping with other
/// flop groups, the runout split or other percentile buckets and fewer
/// strength levels at some leaves. A variant changes only the board-wide part
/// of a class (the flop group and the runout class), the number of percentile
/// buckets and, with opponent levels on every street, the number of levels,
/// so every variant shares the boards' strength order and hand classes.
pub struct LeafCards<'a> {
    base: Relative<'a>,
    variants: Vec<Relative<'a>>,
    /// Variant per tree node: 0 off the leaves and at leaves with the base's.
    by_node: Vec<usize>,
    identity: [u8; 32],
}

fn leaf_slots(subtree: &Subtree, source: &dyn BucketSource) -> usize {
    subtree
        .nodes
        .iter()
        .filter(|n| n.actor.is_some())
        .map(|n| source.count(n.street) * n.children.len())
        .sum()
}

fn same(a: &Relative<'_>, b: &Relative<'_>) -> bool {
    a.runout == b.runout
        && a.levels == b.levels
        && a.counts == b.counts
        && match (a.flops, b.flops) {
            (None, None) => true,
            (Some(x), Some(y)) => std::ptr::eq(x, y),
            _ => false,
        }
}

impl<'a> LeafCards<'a> {
    pub fn new(tree: &Tree, base: Relative<'a>, spec: &LeafCardSpec<'a>) -> Result<Self> {
        if let Some((runout, _)) = spec.runout {
            ensure!(
                runout != Runout::default(),
                "the per-leaf runout split needs kinds"
            );
            ensure!(
                base.runout == Runout::default(),
                "a per-leaf runout split needs a base mapping without one"
            );
        }
        if let Some(rule) = spec.low_spr_levels {
            ensure!(
                rule.levels > 0 && base.levels.is_multiple_of(rule.levels),
                "the low-SPR levels must divide the base mapping's {} levels",
                base.levels
            );
            ensure!(
                base.opponent_levels == [true; 3],
                "per-leaf levels need opponent levels on the flop, turn and river"
            );
        }
        let mut flops = std::collections::HashMap::new();
        for &(z, groups) in &spec.flops {
            ensure!(
                tree.nodes
                    .get(z)
                    .and_then(|n| n.terminal.as_ref())
                    .is_some_and(|t| t.l1.is_some()),
                "node {z} is not an L1 leaf"
            );
            ensure!(
                flops.insert(z, groups).is_none(),
                "leaf {z} has flop groups twice"
            );
        }
        let mut variants: Vec<Relative<'a>> = Vec::new();
        let mut by_node = vec![0; tree.nodes.len()];
        let mut hash = blake3::Hasher::new();
        hash.update(b"leaf-cards");
        hash.update(&base.state_identity());
        for (z, n) in tree.nodes.iter().enumerate() {
            let Some(subtree) = n.terminal.as_ref().and_then(|t| t.l1.as_ref()) else {
                continue;
            };
            let mut mapping = Relative {
                flops: flops.get(&z).copied().unwrap_or(base.flops),
                ..base
            };
            if let Some(rule) = spec.low_spr_levels
                && subtree.flop_spr <= rule.max_spr
            {
                mapping.levels = rule.levels;
                if let Some(buckets) = rule.buckets {
                    mapping.counts = [buckets; 3];
                }
            }
            if let Some((runout, max_slots)) = spec.runout {
                let split = Relative { runout, ..mapping };
                if leaf_slots(subtree, &split) <= max_slots {
                    mapping = split;
                }
            }
            if same(&mapping, &base) {
                continue;
            }
            let v = match variants.iter().position(|m| same(m, &mapping)) {
                Some(i) => i + 1,
                None => {
                    for street in [Street::Flop, Street::Turn, Street::River] {
                        ensure!(
                            mapping.count(street) <= usize::from(u16::MAX),
                            "leaf {z} {street:?} bucket count exceeds u16"
                        );
                    }
                    hash.update(&mapping.state_identity());
                    variants.push(mapping);
                    variants.len()
                }
            };
            by_node[z] = v;
            hash.update(&(z as u64).to_le_bytes());
            hash.update(&(v as u64).to_le_bytes());
        }
        Ok(Self {
            base,
            variants,
            by_node,
            identity: *hash.finalize().as_bytes(),
        })
    }

    /// Leaves with their own flop groups, leaves with the runout split and
    /// leaves with other percentile buckets or strength levels.
    pub fn changed_leaves(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for &v in self.by_node.iter().filter(|&&v| v > 0) {
            let m = &self.variants[v - 1];
            let flops = Relative {
                runout: self.base.runout,
                levels: self.base.levels,
                counts: self.base.counts,
                ..*m
            };
            counts.0 += usize::from(!same(&flops, &self.base));
            counts.1 += usize::from(m.runout != self.base.runout);
            counts.2 += usize::from(m.levels != self.base.levels || m.counts != self.base.counts);
        }
        counts
    }
}

impl BucketSource for LeafCards<'_> {
    fn state_identity(&self) -> [u8; 32] {
        self.identity
    }
    fn leaf_variant(&self, terminal: usize) -> usize {
        self.by_node[terminal]
    }
    fn variants(&self) -> &[Relative<'_>] {
        &self.variants
    }
    fn count(&self, street: Street) -> usize {
        self.base.count(street)
    }
    fn row(&self, board: &[Card]) -> [u16; NUM_COMBOS] {
        self.base.row(board)
    }
    fn relative(&self) -> bool {
        true
    }
    fn relative_prior(&self) -> f64 {
        self.base.relative_prior()
    }
    fn relative_prior_fraction(&self) -> f64 {
        self.base.relative_prior_fraction()
    }
    fn relative_average(&self) -> bool {
        self.base.relative_average()
    }
    fn levels(&self) -> usize {
        self.base.levels()
    }
    fn opponent_levels(&self, street: Street) -> bool {
        self.base.opponent_levels(street)
    }
    fn classes(&self, street: Street) -> usize {
        self.base.classes(street)
    }
    fn class(&self, board: &[Card], strength: u16) -> u16 {
        self.base.class(board, strength)
    }
    fn hand_classes(&self, street: Street) -> usize {
        self.base.hand_classes(street)
    }
    fn hand_class(&self, street: Street, features: &HandFeatures) -> u16 {
        self.base.hand_class(street, features)
    }
    fn hand_potential(&self, street: Street) -> bool {
        self.base.hand_potential(street)
    }
    fn hand_category(&self) -> bool {
        self.base.hand_category()
    }
    fn flop_recall_classes(&self, street: Street) -> usize {
        self.base.flop_recall_classes(street)
    }
    fn flop_recall_class(&self, flop: &[Card], hole: [Card; 2], strength: u16) -> u16 {
        self.base.flop_recall_class(flop, hole, strength)
    }
}
