//! Serde-facing game configuration and shared validation.
//!
//! `MultiwayConfig` is the payload of CLI `kind = "preflop-multiway"`; run,
//! algorithm, rake, and utility remain sibling sections.  Conversion to
//! integral [`MwChips`] happens exactly once here so the betting and
//! settlement layers never see floating-point chip amounts.

use std::path::PathBuf;
use std::str::FromStr;

use cards::Range;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rake_condition::{CompiledRakeCondition, compile as compile_rake_condition};
use crate::types::{MAX_SEATS, MIN_SEATS, MwChips, SeatId, SeatVec, Street};

const DEFAULT_RANGE: &str = "";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultiwayConfig {
    pub seats: Vec<SeatConfig>,
    pub button: SeatId,
    #[serde(default)]
    pub blinds: BlindConfig,
    #[serde(default)]
    pub ante: AnteConfig,
    #[serde(default)]
    pub betting: BettingConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced_bets: Option<ForcedBetConfig>,
    #[serde(default)]
    pub abstraction: AbstractionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub stack_bb: f64,
    /// Empty means the full 1,326-combo range.  Non-empty strings use the
    /// shared `cards::Range` grammar.
    #[serde(default = "default_range")]
    pub range: String,
    /// Optional complete betting profile for this seat. Omission uses the
    /// table-wide [`MultiwayConfig::betting`] profile.
    #[serde(default)]
    pub betting: Option<BettingConfig>,
}

fn default_range() -> String {
    DEFAULT_RANGE.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlindConfig {
    #[serde(default = "default_small_blind")]
    pub small_bb: f64,
    #[serde(default = "default_big_blind")]
    pub big_bb: f64,
}

impl Default for BlindConfig {
    fn default() -> Self {
        Self {
            small_bb: default_small_blind(),
            big_bb: default_big_blind(),
        }
    }
}

fn default_small_blind() -> f64 {
    0.5
}

fn default_big_blind() -> f64 {
    1.0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AnteConfig {
    #[default]
    None,
    Each {
        amount_bb: f64,
    },
    BigBlind {
        amount_bb: f64,
    },
}

/// Normalized v1 forced contributions. Legacy configs leave this absent and
/// derive the usual SB/BB plus table ante from `blinds` and `ante`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForcedBetConfig {
    pub blinds_bb: Vec<f64>,
    pub antes_bb: Vec<f64>,
    #[serde(default)]
    pub common_ante_bb: f64,
    pub nominal_big_blind_bb: f64,
    pub first_to_act: SeatId,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BettingConfig {
    #[serde(default = "default_true")]
    pub allow_limp: bool,
    #[serde(default = "default_preflop_betting")]
    pub preflop: StreetBettingConfig,
    #[serde(default = "default_postflop_betting")]
    pub flop: StreetBettingConfig,
    #[serde(default = "default_postflop_betting")]
    pub turn: StreetBettingConfig,
    #[serde(default = "default_postflop_betting")]
    pub river: StreetBettingConfig,
    #[serde(default)]
    pub rules: Vec<TreeRule>,
}

impl Default for BettingConfig {
    fn default() -> Self {
        Self {
            allow_limp: true,
            preflop: default_preflop_betting(),
            flop: default_postflop_betting(),
            turn: default_postflop_betting(),
            river: default_postflop_betting(),
            rules: Vec::new(),
        }
    }
}

impl BettingConfig {
    pub fn for_street(&self, street: Street) -> &StreetBettingConfig {
        match street {
            Street::Preflop => &self.preflop,
            Street::Flop => &self.flop,
            Street::Turn => &self.turn,
            Street::River => &self.river,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeRule {
    pub priority: i32,
    pub source_order: u32,
    pub street: RuleStreet,
    pub condition: String,
    pub effect: RuleEffect,
    pub action: Option<RuleAction>,
    pub sizes: Vec<SizeSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleStreet {
    Preflop,
    Flop,
    Turn,
    River,
    Postflop,
}

impl RuleStreet {
    pub fn matches(self, street: Street) -> bool {
        match self {
            Self::Preflop => street == Street::Preflop,
            Self::Flop => street == Street::Flop,
            Self::Turn => street == Street::Turn,
            Self::River => street == Street::River,
            Self::Postflop => street != Street::Preflop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleEffect {
    Add,
    Remove,
    Replace,
    Force,
    Checkdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleAction {
    Fold,
    Check,
    Call,
    Bet,
    Raise,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreetBettingConfig {
    /// Opening sizes. Preflop these apply before a voluntary limp; postflop
    /// they are bets into an unopened street.
    #[serde(default)]
    pub bet_sizes: Vec<SizeSpec>,
    /// Preflop raise-to sizes after one or more limps. None preserves the
    /// historical behavior of reusing bet_sizes; Some(empty) disables sized
    /// isolations while still allowing an all-in when configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolate_sizes: Option<Vec<SizeSpec>>,
    #[serde(default)]
    pub raise_sizes: Vec<SizeSpec>,
    #[serde(default = "default_max_aggressive_actions")]
    pub max_aggressive_actions: u8,
    #[serde(default = "default_true")]
    pub include_allin: bool,
    /// Raise-cap merge threshold (HRC-style). After a target is resolved
    /// (including the minimum-full-raise bump and the maximum-stack cap), a
    /// target `>= scale(maximum, allin_threshold)` is replaced by `maximum`
    /// and marked all-in. This is a *merge*, not an addition: it applies even
    /// when `include_allin` is `false`, since it collapses an
    /// already-proposed sized target into the all-in rather than adding a new
    /// action. `None` disables the merge entirely (default, unchanged
    /// behavior). Must be finite and in `(0.0, 1.0]` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allin_threshold: Option<f64>,
    /// Exact preflop re-raise jam threshold. After a normal raise target has
    /// been resolved against the minimum raise and the actor's stack cap,
    /// targets strictly above this fraction of the actor's starting stack
    /// collapse to all-in. Equality deliberately keeps both the normal size
    /// and any explicitly configured all-in. This applies only after an open
    /// (`aggressive_actions > 0`); it never changes an opening size.
    ///
    /// This exact-rational threshold and the legacy floating-point
    /// `allin_threshold` are mutually exclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reraise_jam_above_actor_starting_stack: Option<StackRatio>,
    /// HRC-style check-down threshold (flop/turn/river only). When the
    /// non-folded seat count at this street's start (all-in seats included)
    /// exceeds this value, the street has no betting at all: no decision
    /// nodes, not even checks; play proceeds straight to the next street (or
    /// showdown). `None` (default) preserves unlimited betting, so every
    /// config predating this field serializes identically and keeps its game
    /// fingerprint. See `docs/multiway-preflop-v1.jp.md`. Rejected on preflop and
    /// must be at least `1` when present; also a public-tree property, so a
    /// per-seat betting override must not disagree with the table's value
    /// for the same street (see [`MultiwayConfig::validate`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_betting_players: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackRatio {
    pub numerator: u32,
    pub denominator: u32,
}

fn default_max_aggressive_actions() -> u8 {
    4
}

fn default_preflop_betting() -> StreetBettingConfig {
    StreetBettingConfig {
        bet_sizes: vec![SizeSpec::PreviousBetMultiple { factor: 2.5 }],
        isolate_sizes: None,
        raise_sizes: vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }],
        max_aggressive_actions: 4,
        include_allin: true,
        allin_threshold: None,
        reraise_jam_above_actor_starting_stack: None,
        max_betting_players: None,
    }
}

fn default_postflop_betting() -> StreetBettingConfig {
    StreetBettingConfig {
        bet_sizes: vec![SizeSpec::PotAfterCall { fraction: 0.5 }],
        isolate_sizes: None,
        raise_sizes: vec![SizeSpec::PotAfterCall { fraction: 0.75 }],
        max_aggressive_actions: 3,
        include_allin: true,
        allin_threshold: None,
        reraise_jam_above_actor_starting_stack: None,
        max_betting_players: None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SizeSpec {
    ToBb {
        value: f64,
    },
    PotAfterCall {
        fraction: f64,
    },
    PreviousBetMultiple {
        factor: f64,
    },
    /// Always resolves to the minimum legal full raise/bet target for the
    /// current node (`BettingState::minimum_full_target`). Appended after the
    /// original three variants: both TOML (`kind` tag) and the JSON game
    /// fingerprint identify variants by name, so this ordering is purely
    /// cosmetic and does not break existing configs or fingerprints.
    MinRaise,
    /// Resolves to `fraction` of the acting seat's maximum possible target
    /// (`actor_wager + remaining stack`), i.e. a fraction of an effective
    /// all-in. Appended after the original three variants for the same
    /// name-tagged-serialization reason as `MinRaise`.
    StackFraction {
        fraction: f64,
    },
    AllIn,
    EffectiveStackFraction {
        fraction: f64,
    },
    GeometricAllIn {
        streets: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbstractionConfig {
    #[serde(default = "default_buckets")]
    pub flop_buckets: u16,
    #[serde(default = "default_buckets")]
    pub turn_buckets: u16,
    #[serde(default = "default_buckets")]
    pub river_buckets: u16,
    /// Ignored by `kind = "ehs2-table"`, which trains no rollouts (always
    /// has a default, so it need not be unset there).
    #[serde(default = "default_rollout_samples")]
    pub rollout_samples: u32,
    /// Number of deterministic rollout feature points generated per bucket
    /// while training the rollout/k-means abstraction. Skipped at the
    /// historical default so configs predating this field retain identical
    /// serialized bytes and hashes.
    #[serde(
        default = "default_points_per_bucket",
        skip_serializing_if = "is_default_points_per_bucket"
    )]
    pub points_per_bucket: u32,
    /// Maximum deterministic k-means refinement iterations. Like
    /// `points_per_bucket`, the historical default is omitted from
    /// serialization for backward compatibility.
    #[serde(
        default = "default_kmeans_iterations",
        skip_serializing_if = "is_default_kmeans_iterations"
    )]
    pub kmeans_iterations: u32,
    /// Ignored by `kind = "ehs2-table"` for the same reason as
    /// `rollout_samples`.
    #[serde(default)]
    pub seed: u64,
    /// Optional postflop budgets keyed by the number of live opponents
    /// excluding the acting player. Missing counts use the global defaults.
    /// Not supported by `kind = "ehs2-table"` (validation error).
    #[serde(default)]
    pub active_opponent_buckets: Vec<ActiveOpponentBucketConfig>,
    /// Optional operational cache location for trained centroids. This path
    /// is not part of game identity; validated artifact content is covered
    /// by the abstraction fingerprint. For `kind = "ehs2-table"` this same
    /// key holds the EHS² bucket-table cache instead of a rollout artifact.
    #[serde(default)]
    pub artifact_cache: Option<PathBuf>,
    /// Private-information recall used to key the solver's policy storage.
    /// `Full` (the default) is unchanged sparse, full-recall behavior.
    /// `Street` switches to a bounded-memory dense arena keyed only by the
    /// current street's bucket (a Monker/Pluribus-style imperfect-recall
    /// abstraction); see `docs/multiway-preflop-v1.jp.md`. Skipped when `Full` so
    /// every legacy config predating this field keeps identical serialized
    /// bytes/config hashes. Recall is independently excluded from game
    /// identity and included in abstraction identity.
    #[serde(default, skip_serializing_if = "RecallMode::is_full")]
    pub recall: RecallMode,
    /// Selects the postflop card-abstraction backend. `RolloutKmeans` (the
    /// default) is the trained rollout/k-means abstraction; `Ehs2Table` uses
    /// precomputed exact EHS^2 percentile tables (O(1) lookup, no solve-time
    /// Monte Carlo, no per-opponent-count budgets). Skipped when
    /// `RolloutKmeans` to preserve legacy serialized bytes/config hashes.
    /// Backend content is independently covered by abstraction identity.
    #[serde(default, skip_serializing_if = "AbstractionKind::is_rollout_kmeans")]
    pub kind: AbstractionKind,
}

/// Selects the postflop card-abstraction backend; see
/// [`AbstractionConfig::kind`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbstractionKind {
    /// Retired trained rollout/k-means abstraction. The implementation was
    /// removed; the variant survives so configs that still name it fail with
    /// an explicit MWP001 instead of an opaque unknown-variant error.
    RolloutKmeans,
    /// Precomputed exact EHS² percentile tables over every canonical board,
    /// O(1) lookup, no solve-time Monte Carlo.
    #[default]
    Ehs2Table,
}

impl AbstractionKind {
    pub fn is_rollout_kmeans(&self) -> bool {
        matches!(self, AbstractionKind::RolloutKmeans)
    }
}

/// Private-information recall mode; see [`AbstractionConfig::recall`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecallMode {
    /// Sparse policy storage keyed by every street's bucket visited so far
    /// (today's behavior). Memory grows with the number of visited
    /// information sets.
    #[default]
    Full,
    /// Dense, preallocated policy storage keyed only by the current
    /// street's bucket. Bounded memory, coarser (imperfect-recall) strategy
    /// conditioning.
    Street,
}

impl RecallMode {
    pub fn is_full(&self) -> bool {
        matches!(self, RecallMode::Full)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveOpponentBucketConfig {
    pub active_opponents: u8,
    pub flop_buckets: u16,
    pub turn_buckets: u16,
    pub river_buckets: u16,
}

impl Default for AbstractionConfig {
    fn default() -> Self {
        Self {
            flop_buckets: default_buckets(),
            turn_buckets: default_buckets(),
            river_buckets: default_buckets(),
            rollout_samples: default_rollout_samples(),
            points_per_bucket: default_points_per_bucket(),
            kmeans_iterations: default_kmeans_iterations(),
            seed: 0,
            active_opponent_buckets: Vec::new(),
            artifact_cache: None,
            recall: RecallMode::Full,
            kind: AbstractionKind::RolloutKmeans,
        }
    }
}

impl AbstractionConfig {
    pub fn buckets_for(&self, active_opponents: u8) -> Option<ActiveOpponentBucketConfig> {
        self.active_opponent_buckets
            .iter()
            .copied()
            .find(|profile| profile.active_opponents == active_opponents)
    }
}

fn default_buckets() -> u16 {
    32
}

fn default_rollout_samples() -> u32 {
    256
}

fn default_points_per_bucket() -> u32 {
    8
}

fn is_default_points_per_bucket(value: &u32) -> bool {
    *value == default_points_per_bucket()
}

fn default_kmeans_iterations() -> u32 {
    20
}

fn is_default_kmeans_iterations(value: &u32) -> bool {
    *value == default_kmeans_iterations()
}

/// Standalone `[utility]` payload used by both CLI and direct library users.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UtilityConfig {
    #[default]
    ChipEv,
    TournamentIcm {
        #[serde(default)]
        outside_field: Vec<FieldPlayerConfig>,
        /// One entry per remaining field player, best finish first.  Zero
        /// prizes must be present rather than omitted.
        payouts: Vec<f64>,
        #[serde(default = "default_icm_samples")]
        samples: u64,
        #[serde(default)]
        seed: u64,
    },
}

fn default_icm_samples() -> u64 {
    100_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldPlayerConfig {
    pub name: String,
    pub stack_bb: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RakeAllocation {
    #[default]
    MainFirst,
    Proportional,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RakeRounding {
    #[default]
    Down,
    Nearest,
    Up,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RakeConfig {
    #[default]
    None,
    PercentCap {
        rate: f64,
        cap_bb: f64,
        #[serde(default)]
        no_flop_no_drop: bool,
    },
    Generic {
        rate: f64,
        cap_bb: Option<f64>,
        when: String,
        allocation: RakeAllocation,
        rounding: RakeRounding,
    },
    GgPreflop {
        rate: f64,
        cap_bb: f64,
        exempt_pot_bb: f64,
    },
}

#[derive(Clone)]
pub struct ValidatedMultiwayConfig {
    pub seats: SeatVec<ValidatedSeat>,
    pub button: SeatId,
    pub blinds: ValidatedBlinds,
    pub ante: ValidatedAnte,
    pub betting: BettingConfig,
    pub forced_blinds: SeatVec<MwChips>,
    pub forced_antes: SeatVec<MwChips>,
    pub common_ante: MwChips,
    pub nominal_big_blind: MwChips,
    pub preflop_first_to_act: SeatId,
    pub abstraction: AbstractionConfig,
}

#[derive(Clone)]
pub struct ValidatedSeat {
    pub name: Option<String>,
    pub starting_stack: MwChips,
    pub range: Range,
    pub betting: Option<BettingConfig>,
}

#[derive(Debug, Clone, Copy)]
pub struct ValidatedBlinds {
    pub small: MwChips,
    pub big: MwChips,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatedAnte {
    None,
    Each(MwChips),
    BigBlind(MwChips),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompiledRake {
    None,
    PercentCap {
        rate: f64,
        cap: MwChips,
        no_flop_no_drop: bool,
    },
    Generic {
        rate: f64,
        cap: Option<MwChips>,
        when: CompiledRakeCondition,
        allocation: RakeAllocation,
        rounding: RakeRounding,
    },
    GgPreflop {
        rate: f64,
        cap: MwChips,
        exempt_pot: MwChips,
    },
}

impl MultiwayConfig {
    /// Shared CLI/bridge validation entry point.  It also parses every range
    /// so invalid or empty ranges fail before sampler/artifact work begins.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let num_seats = self.seats.len();
        if !(MIN_SEATS..=MAX_SEATS).contains(&num_seats) {
            return Err(ConfigError::SeatCount(num_seats));
        }
        if self.button.index() >= num_seats {
            return Err(ConfigError::Button {
                button: self.button.index(),
                num_seats,
            });
        }

        for (index, seat) in self.seats.iter().enumerate() {
            positive_finite(&format!("seats[{index}].stack_bb"), seat.stack_bb)?;
            let chips = MwChips::try_from_bb(seat.stack_bb)
                .map_err(|_| ConfigError::Number(format!("seats[{index}].stack_bb")))?;
            if chips == MwChips::ZERO {
                return Err(ConfigError::Number(format!("seats[{index}].stack_bb")));
            }
            if seat
                .name
                .as_ref()
                .is_some_and(|name| name.trim().is_empty())
            {
                return Err(ConfigError::EmptyName(index));
            }
            if let Some(betting) = &seat.betting {
                validate_betting(betting)?;
                // Check-down is a public-tree property: it cannot legally
                // differ by seat, so a per-seat override may only repeat the
                // table's value for the same street, never diverge from it.
                for street in [Street::Flop, Street::Turn, Street::River] {
                    let seat_value = betting.for_street(street).max_betting_players;
                    let table_value = self.betting.for_street(street).max_betting_players;
                    if seat_value != table_value {
                        return Err(ConfigError::SeatMaxBettingPlayersMismatch {
                            seat: index,
                            street,
                        });
                    }
                }
            }
        }

        finite("blinds.small_bb", self.blinds.small_bb)?;
        positive_finite("blinds.big_bb", self.blinds.big_bb)?;
        let small = MwChips::try_from_bb(self.blinds.small_bb)
            .map_err(|_| ConfigError::Number("blinds.small_bb".into()))?;
        let big = MwChips::try_from_bb(self.blinds.big_bb)
            .map_err(|_| ConfigError::Number("blinds.big_bb".into()))?;
        if small >= big || big == MwChips::ZERO {
            return Err(ConfigError::Blinds);
        }

        match self.ante {
            AnteConfig::None => {}
            AnteConfig::Each { amount_bb } => nonnegative_finite("ante.amount_bb", amount_bb)?,
            AnteConfig::BigBlind { amount_bb } => nonnegative_finite("ante.amount_bb", amount_bb)?,
        }

        if let Some(forced) = &self.forced_bets {
            if forced.blinds_bb.len() != num_seats || forced.antes_bb.len() != num_seats {
                return Err(ConfigError::ForcedBetCount {
                    expected: num_seats,
                    blinds: forced.blinds_bb.len(),
                    antes: forced.antes_bb.len(),
                });
            }
            if forced.first_to_act.index() >= num_seats {
                return Err(ConfigError::FirstActor {
                    actor: forced.first_to_act.index(),
                    num_seats,
                });
            }
            nonnegative_finite(
                "forced_bets.nominal_big_blind_bb",
                forced.nominal_big_blind_bb,
            )?;
            nonnegative_finite("forced_bets.common_ante_bb", forced.common_ante_bb)?;
            for (seat, &amount) in forced.blinds_bb.iter().enumerate() {
                nonnegative_finite(&format!("forced_bets.blinds_bb[{seat}]"), amount)?;
            }
            for (seat, &amount) in forced.antes_bb.iter().enumerate() {
                nonnegative_finite(&format!("forced_bets.antes_bb[{seat}]"), amount)?;
            }
            let maximum = forced.blinds_bb.iter().copied().fold(0.0, f64::max);
            if maximum != forced.nominal_big_blind_bb {
                return Err(ConfigError::NominalBlind);
            }
        }
        validate_betting(&self.betting)?;
        if self.abstraction.flop_buckets == 0
            || self.abstraction.turn_buckets == 0
            || self.abstraction.river_buckets == 0
        {
            return Err(ConfigError::Buckets);
        }
        if self.abstraction.rollout_samples == 0 {
            return Err(ConfigError::RolloutSamples);
        }
        if self.abstraction.points_per_bucket == 0 {
            return Err(ConfigError::TrainingPointsPerBucket);
        }
        if self.abstraction.kmeans_iterations == 0 {
            return Err(ConfigError::KMeansIterations);
        }
        if matches!(self.abstraction.kind, AbstractionKind::Ehs2Table)
            && (self.abstraction.points_per_bucket != default_points_per_bucket()
                || self.abstraction.kmeans_iterations != default_kmeans_iterations())
        {
            return Err(ConfigError::Ehs2TableTraining);
        }
        if matches!(self.abstraction.kind, AbstractionKind::Ehs2Table)
            && !self.abstraction.active_opponent_buckets.is_empty()
        {
            return Err(ConfigError::Ehs2TableOpponentBuckets);
        }
        let mut seen_opponent_counts = [false; 9];
        for profile in &self.abstraction.active_opponent_buckets {
            let opponents = usize::from(profile.active_opponents);
            if opponents == 0 || opponents >= num_seats || seen_opponent_counts[opponents] {
                return Err(ConfigError::OpponentBucketProfile(profile.active_opponents));
            }
            seen_opponent_counts[opponents] = true;
            if profile.flop_buckets == 0 || profile.turn_buckets == 0 || profile.river_buckets == 0
            {
                return Err(ConfigError::Buckets);
            }
        }

        self.compile_ranges()?;
        Ok(())
    }

    pub fn compile_ranges(&self) -> Result<SeatVec<Range>, ConfigError> {
        if !(MIN_SEATS..=MAX_SEATS).contains(&self.seats.len()) {
            return Err(ConfigError::SeatCount(self.seats.len()));
        }
        let mut ranges = Vec::with_capacity(self.seats.len());
        for (index, seat) in self.seats.iter().enumerate() {
            let range = if seat.range.trim().is_empty() {
                Range::full()
            } else {
                Range::from_str(&seat.range).map_err(|error| ConfigError::Range {
                    seat: index,
                    message: error.to_string(),
                })?
            };
            if range.total_weight() <= 0.0 {
                return Err(ConfigError::EmptyRange(index));
            }
            ranges.push(range);
        }
        Ok(SeatVec::new_unchecked(ranges))
    }

    pub fn validated(&self) -> Result<ValidatedMultiwayConfig, ConfigError> {
        self.validate()?;
        let ranges = self.compile_ranges()?;
        let seats = self
            .seats
            .iter()
            .zip(ranges)
            .map(|(seat, range)| ValidatedSeat {
                name: seat.name.clone(),
                starting_stack: MwChips::try_from_bb(seat.stack_bb)
                    .expect("validated finite stack must convert"),
                range,
                betting: seat.betting.clone(),
            })
            .collect();
        let ante = match self.ante {
            AnteConfig::None => ValidatedAnte::None,
            AnteConfig::Each { amount_bb } => ValidatedAnte::Each(
                MwChips::try_from_bb(amount_bb).expect("validated ante must convert"),
            ),
            AnteConfig::BigBlind { amount_bb } => ValidatedAnte::BigBlind(
                MwChips::try_from_bb(amount_bb).expect("validated ante must convert"),
            ),
        };
        let num_seats = self.seats.len();
        let small_blind_seat = if num_seats == 2 {
            self.button
        } else {
            self.button.next(num_seats)
        };
        let big_blind_seat = small_blind_seat.next(num_seats);
        let mut forced_blinds = vec![MwChips::ZERO; num_seats];
        forced_blinds[small_blind_seat.index()] =
            MwChips::try_from_bb(self.blinds.small_bb).expect("validated blind must convert");
        forced_blinds[big_blind_seat.index()] =
            MwChips::try_from_bb(self.blinds.big_bb).expect("validated blind must convert");
        let mut forced_antes = vec![MwChips::ZERO; num_seats];
        let mut common_ante = MwChips::ZERO;
        match ante {
            ValidatedAnte::None => {}
            ValidatedAnte::Each(amount) => forced_antes.fill(amount),
            ValidatedAnte::BigBlind(amount) => common_ante = amount,
        }
        let mut nominal_big_blind =
            MwChips::try_from_bb(self.blinds.big_bb).expect("validated blind must convert");
        let mut preflop_first_to_act = big_blind_seat.next(num_seats);
        if let Some(forced) = &self.forced_bets {
            forced_blinds = forced
                .blinds_bb
                .iter()
                .map(|&value| MwChips::try_from_bb(value).expect("validated blind must convert"))
                .collect();
            forced_antes = forced
                .antes_bb
                .iter()
                .map(|&value| MwChips::try_from_bb(value).expect("validated ante must convert"))
                .collect();
            common_ante = MwChips::try_from_bb(forced.common_ante_bb)
                .expect("validated common ante must convert");
            nominal_big_blind = MwChips::try_from_bb(forced.nominal_big_blind_bb)
                .expect("validated nominal blind must convert");
            preflop_first_to_act = forced.first_to_act;
        }
        Ok(ValidatedMultiwayConfig {
            seats: SeatVec::new_unchecked(seats),
            button: self.button,
            blinds: ValidatedBlinds {
                small: MwChips::try_from_bb(self.blinds.small_bb)
                    .expect("validated blind must convert"),
                big: MwChips::try_from_bb(self.blinds.big_bb)
                    .expect("validated blind must convert"),
            },
            ante,
            betting: self.betting.clone(),
            forced_blinds: SeatVec::new_unchecked(forced_blinds),
            forced_antes: SeatVec::new_unchecked(forced_antes),
            common_ante,
            nominal_big_blind,
            preflop_first_to_act,
            abstraction: self.abstraction.clone(),
        })
    }

    pub fn validate_economics(
        &self,
        utility: &UtilityConfig,
        rake: &RakeConfig,
    ) -> Result<(), ConfigError> {
        self.validate()?;
        utility.validate(self.seats.len())?;
        rake.compile()?;
        if matches!(utility, UtilityConfig::TournamentIcm { .. })
            && !matches!(rake, RakeConfig::None)
        {
            return Err(ConfigError::IcmWithRake);
        }
        Ok(())
    }
}

impl UtilityConfig {
    pub fn validate(&self, table_players: usize) -> Result<(), ConfigError> {
        match self {
            UtilityConfig::ChipEv => Ok(()),
            UtilityConfig::TournamentIcm {
                outside_field,
                payouts,
                samples,
                ..
            } => {
                let field = table_players + outside_field.len();
                if !(2..=crate::icm::ICM_MAX_PLAYERS).contains(&field) {
                    return Err(ConfigError::IcmField(field));
                }
                if payouts.len() != field {
                    return Err(ConfigError::PayoutCount {
                        expected: field,
                        actual: payouts.len(),
                    });
                }
                if field > crate::icm::EXACT_ICM_MAX_PLAYERS && *samples < 2 {
                    return Err(ConfigError::IcmSamples);
                }
                for (index, player) in outside_field.iter().enumerate() {
                    if player.name.trim().is_empty() {
                        return Err(ConfigError::OutsideName(index));
                    }
                    positive_finite(&format!("outside_field[{index}].stack_bb"), player.stack_bb)?;
                }
                for (index, &payout) in payouts.iter().enumerate() {
                    nonnegative_finite(&format!("payouts[{index}]"), payout)?;
                    if index > 0 && payouts[index - 1] < payout {
                        return Err(ConfigError::PayoutOrder);
                    }
                }
                if payouts.windows(2).all(|pair| pair[0] == pair[1]) {
                    return Err(ConfigError::FlatPayouts);
                }
                Ok(())
            }
        }
    }
}

impl RakeConfig {
    pub fn compile(&self) -> Result<CompiledRake, ConfigError> {
        match *self {
            RakeConfig::None => Ok(CompiledRake::None),
            RakeConfig::PercentCap {
                rate,
                cap_bb,
                no_flop_no_drop,
            } => {
                rate_value(rate)?;
                nonnegative_finite("rake.cap_bb", cap_bb)?;
                Ok(CompiledRake::PercentCap {
                    rate,
                    cap: MwChips::try_from_bb(cap_bb)
                        .map_err(|_| ConfigError::Number("rake.cap_bb".into()))?,
                    no_flop_no_drop,
                })
            }
            RakeConfig::Generic {
                rate,
                cap_bb,
                ref when,
                allocation,
                rounding,
            } => {
                rate_value(rate)?;
                let cap = cap_bb
                    .map(|cap_bb| {
                        nonnegative_finite("rake.cap_bb", cap_bb)?;
                        MwChips::try_from_bb(cap_bb)
                            .map_err(|_| ConfigError::Number("rake.cap_bb".into()))
                    })
                    .transpose()?;
                let when = compile_rake_condition(when).map_err(ConfigError::RakeCondition)?;
                Ok(CompiledRake::Generic {
                    rate,
                    cap,
                    when,
                    allocation,
                    rounding,
                })
            }
            RakeConfig::GgPreflop {
                rate,
                cap_bb,
                exempt_pot_bb,
            } => {
                rate_value(rate)?;
                nonnegative_finite("rake.cap_bb", cap_bb)?;
                nonnegative_finite("rake.exempt_pot_bb", exempt_pot_bb)?;
                Ok(CompiledRake::GgPreflop {
                    rate,
                    cap: MwChips::try_from_bb(cap_bb)
                        .map_err(|_| ConfigError::Number("rake.cap_bb".into()))?,
                    exempt_pot: MwChips::try_from_bb(exempt_pot_bb)
                        .map_err(|_| ConfigError::Number("rake.exempt_pot_bb".into()))?,
                })
            }
        }
    }
}

fn validate_size_spec(size: &SizeSpec) -> Result<(), ConfigError> {
    match *size {
        SizeSpec::ToBb { value } => positive_finite("size.to-bb", value),
        SizeSpec::PotAfterCall { fraction } => positive_finite("size.pot-after-call", fraction),
        SizeSpec::PreviousBetMultiple { factor } => {
            positive_finite("size.previous-bet-multiple", factor)?;
            if factor <= 1.0 {
                return Err(ConfigError::RaiseFactor);
            }
            Ok(())
        }
        SizeSpec::MinRaise => Ok(()),
        SizeSpec::StackFraction { fraction } => positive_finite("size.stack-fraction", fraction),
        SizeSpec::AllIn => Ok(()),
        SizeSpec::EffectiveStackFraction { fraction } => {
            positive_finite("size.effective-stack-fraction", fraction)
        }
        SizeSpec::GeometricAllIn { streets } if streets > 0 => Ok(()),
        SizeSpec::GeometricAllIn { .. } => {
            Err(ConfigError::Number("size.geometric streets".into()))
        }
    }
}

fn validate_betting(config: &BettingConfig) -> Result<(), ConfigError> {
    for street in Street::ALL {
        let section = config.for_street(street);
        if street != Street::Preflop && section.isolate_sizes.is_some() {
            return Err(ConfigError::PostflopIsolateSizes(street));
        }
        for (kind, sizes) in [
            ("bet_sizes", &section.bet_sizes),
            ("raise_sizes", &section.raise_sizes),
        ] {
            if sizes.len() > 32 {
                return Err(ConfigError::TooManySizes { street, kind });
            }
            for size in sizes {
                validate_size_spec(size)?;
            }
        }
        if let Some(sizes) = section.isolate_sizes.as_ref() {
            if sizes.len() > 32 {
                return Err(ConfigError::TooManySizes {
                    street,
                    kind: "isolate_sizes",
                });
            }
            for size in sizes {
                validate_size_spec(size)?;
            }
        }
        if let Some(threshold) = section.allin_threshold {
            finite("allin_threshold", threshold)?;
            if !(threshold > 0.0 && threshold <= 1.0) {
                return Err(ConfigError::AllinThreshold(threshold));
            }
        }
        if let Some(ratio) = section.reraise_jam_above_actor_starting_stack {
            if ratio.numerator == 0 || ratio.denominator == 0 || ratio.numerator > ratio.denominator
            {
                return Err(ConfigError::ReraiseJamRatio {
                    numerator: ratio.numerator,
                    denominator: ratio.denominator,
                });
            }
            if section.allin_threshold.is_some() {
                return Err(ConfigError::ConflictingAllinThresholds(street));
            }
            if street != Street::Preflop {
                return Err(ConfigError::PostflopReraiseJamRatio(street));
            }
        }
        if street == Street::Preflop {
            if section.max_betting_players.is_some() {
                return Err(ConfigError::PreflopMaxBettingPlayers);
            }
        } else if section.max_betting_players == Some(0) {
            return Err(ConfigError::MaxBettingPlayers);
        }
    }
    if config.rules.len() > 256 {
        return Err(ConfigError::TreeRule(
            "at most 256 tree rules are allowed".into(),
        ));
    }
    for (index, rule) in config.rules.iter().enumerate() {
        if rule.condition.trim().is_empty() {
            return Err(ConfigError::TreeRule(format!(
                "tree rule {index} has an empty condition"
            )));
        }
        crate::tree_rules::validate_condition(&rule.condition).map_err(ConfigError::TreeRule)?;
        match rule.effect {
            RuleEffect::Checkdown if rule.action.is_none() && rule.sizes.is_empty() => {}
            RuleEffect::Checkdown => {
                return Err(ConfigError::TreeRule(format!(
                    "checkdown tree rule {index} must omit action and sizes"
                )));
            }
            _ if rule.action.is_none() => {
                return Err(ConfigError::TreeRule(format!(
                    "tree rule {index} requires an action"
                )));
            }
            _ => {}
        }
        if rule.sizes.len() > 32 {
            return Err(ConfigError::TreeRule(format!(
                "tree rule {index} has more than 32 sizes"
            )));
        }
        for size in &rule.sizes {
            validate_size_spec(size)?;
        }
        if rule.effect == RuleEffect::Force
            && config.rules[..index].iter().any(|other| {
                other.effect == RuleEffect::Force
                    && other.priority == rule.priority
                    && other.street == rule.street
                    && other.condition.trim() == rule.condition.trim()
                    && (other.action != rule.action || other.sizes != rule.sizes)
            })
        {
            return Err(ConfigError::TreeRule(format!(
                "tree rule {index} conflicts with an earlier force at the same priority"
            )));
        }
    }
    Ok(())
}

fn finite(field: &str, value: f64) -> Result<(), ConfigError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ConfigError::Number(field.into()))
    }
}

fn positive_finite(field: &str, value: f64) -> Result<(), ConfigError> {
    finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(ConfigError::Number(field.into()))
    }
}

fn nonnegative_finite(field: &str, value: f64) -> Result<(), ConfigError> {
    finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(ConfigError::Number(field.into()))
    }
}

fn rate_value(value: f64) -> Result<(), ConfigError> {
    finite("rake.rate", value)?;
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(ConfigError::RakeRate)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("multiway table must contain 2 through 9 seats, got {0}")]
    SeatCount(usize),
    #[error("button seat {button} is outside a {num_seats}-seat table")]
    Button { button: usize, num_seats: usize },
    #[error("{0} must be finite and within its allowed range")]
    Number(String),
    #[error("seat {0} has an empty name")]
    EmptyName(usize),
    #[error("small blind must be non-negative and less than a positive big blind")]
    Blinds,
    #[error(
        "forced bet vectors must each contain {expected} seats (blinds={blinds}, antes={antes})"
    )]
    ForcedBetCount {
        expected: usize,
        blinds: usize,
        antes: usize,
    },
    #[error("preflop first actor {actor} is outside a {num_seats}-seat table")]
    FirstActor { actor: usize, num_seats: usize },
    #[error("nominal big blind must equal the maximum configured live blind")]
    NominalBlind,
    #[error("seat {seat} range is invalid: {message}")]
    Range { seat: usize, message: String },
    #[error("seat {0} range must contain positive combo weight")]
    EmptyRange(usize),
    #[error("{street:?}.{kind} may contain at most 32 entries")]
    TooManySizes { street: Street, kind: &'static str },
    #[error("{0:?}.isolate_sizes is valid only for preflop")]
    PostflopIsolateSizes(Street),
    #[error("previous-bet-multiple must be greater than one")]
    RaiseFactor,
    #[error("all postflop bucket counts must be positive")]
    Buckets,
    #[error("abstraction rollout_samples must be positive")]
    RolloutSamples,
    #[error("abstraction points_per_bucket must be positive")]
    TrainingPointsPerBucket,
    #[error("abstraction kmeans_iterations must be positive")]
    KMeansIterations,
    #[error("ehs2-table does not support non-default rollout training parameters")]
    Ehs2TableTraining,
    #[error("active-opponent bucket profile {0} is duplicate or outside this table")]
    OpponentBucketProfile(u8),
    #[error("ehs2-table does not support per-opponent bucket budgets")]
    Ehs2TableOpponentBuckets,
    #[error("ICM field must contain 2 through 10000 players, got {0}")]
    IcmField(usize),
    #[error("ICM payouts length must be {expected}, got {actual}")]
    PayoutCount { expected: usize, actual: usize },
    #[error("sampled ICM requires at least two samples")]
    IcmSamples,
    #[error("outside-field player {0} has an empty name")]
    OutsideName(usize),
    #[error("payouts must be ordered highest to lowest")]
    PayoutOrder,
    #[error("ICM payouts must contain at least two distinct values")]
    FlatPayouts,
    #[error("invalid rake condition: {0}")]
    RakeCondition(String),
    #[error("rake rate must be from zero through one")]
    RakeRate,
    #[error("per-hand rake and tournament ICM cannot be combined")]
    IcmWithRake,
    #[error("allin_threshold must be finite and within (0.0, 1.0], got {0}")]
    AllinThreshold(f64),
    #[error(
        "reraise_jam_above_actor_starting_stack must be an exact ratio within (0, 1], got {numerator}/{denominator}"
    )]
    ReraiseJamRatio { numerator: u32, denominator: u32 },
    #[error(
        "{0:?} cannot configure both allin_threshold and reraise_jam_above_actor_starting_stack"
    )]
    ConflictingAllinThresholds(Street),
    #[error(
        "{0:?}.reraise_jam_above_actor_starting_stack is not supported; the threshold applies only to preflop re-raises"
    )]
    PostflopReraiseJamRatio(Street),
    #[error(
        "preflop.max_betting_players is not supported; check-down thresholds apply only to flop, turn, and river"
    )]
    PreflopMaxBettingPlayers,
    #[error("max_betting_players must be at least 1")]
    MaxBettingPlayers,
    #[error(
        "seat {seat} {street:?}.max_betting_players must match the table's value for that street"
    )]
    SeatMaxBettingPlayersMismatch { seat: usize, street: Street },
    #[error("invalid tree rule: {0}")]
    TreeRule(String),
}
#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_toml() -> &'static str {
        r#"
button = 0

[[seats]]
stack_bb = 40

[[seats]]
stack_bb = 25.5
range = "22+,A2s+"

[[seats]]
stack_bb = 12
"#
    }

    #[test]
    fn minimal_three_seat_config_applies_defaults_and_round_trips() {
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.validate().unwrap();
        assert_eq!(config.blinds.small_bb, 0.5);
        assert_eq!(config.abstraction.flop_buckets, 32);
        assert!(config.abstraction.active_opponent_buckets.is_empty());
        assert!(config.betting.allow_limp);
        assert_eq!(config.compile_ranges().unwrap().len(), 3);
        let encoded = toml::to_string(&config).unwrap();
        let decoded: MultiwayConfig = toml::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
    }

    #[test]
    fn validated_config_uses_thousand_chips_per_bb() {
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        let compiled = config.validated().unwrap();
        assert_eq!(compiled.seats[SeatId(0)].starting_stack, MwChips(40_000));
        assert_eq!(compiled.seats[SeatId(1)].starting_stack, MwChips(25_500));
        assert_eq!(compiled.blinds.big, MwChips(1_000));
    }

    #[test]
    fn rejects_bad_button_empty_range_and_unknown_fields() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.button = SeatId(3);
        assert!(matches!(config.validate(), Err(ConfigError::Button { .. })));

        config.button = SeatId(0);
        config.seats[0].range = "AA:0".to_string();
        assert!(matches!(config.validate(), Err(ConfigError::EmptyRange(0))));

        let bad = format!("{}\nunknown = 1", minimal_toml());
        assert!(toml::from_str::<MultiwayConfig>(&bad).is_err());
    }

    #[test]
    fn validates_seat_betting_and_opponent_bucket_overrides() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.seats[0].betting = Some(BettingConfig::default());
        config.abstraction.active_opponent_buckets = vec![ActiveOpponentBucketConfig {
            active_opponents: 2,
            flop_buckets: 24,
            turn_buckets: 20,
            river_buckets: 16,
        }];
        config.validate().unwrap();
        let validated = config.validated().unwrap();
        assert!(validated.seats[SeatId(0)].betting.is_some());
        assert_eq!(config.abstraction.buckets_for(2).unwrap().river_buckets, 16);

        config
            .abstraction
            .active_opponent_buckets
            .push(config.abstraction.active_opponent_buckets[0]);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::OpponentBucketProfile(2))
        ));
    }

    #[test]
    fn tournament_field_and_rake_are_validated_together() {
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        let utility = UtilityConfig::TournamentIcm {
            outside_field: vec![FieldPlayerConfig {
                name: "off-table".into(),
                stack_bb: 100.0,
            }],
            payouts: vec![100.0, 60.0, 40.0, 0.0],
            samples: 10_000,
            seed: 7,
        };
        config
            .validate_economics(&utility, &RakeConfig::None)
            .unwrap();
        assert!(matches!(
            config.validate_economics(
                &utility,
                &RakeConfig::PercentCap {
                    rate: 0.05,
                    cap_bb: 3.0,
                    no_flop_no_drop: true,
                }
            ),
            Err(ConfigError::IcmWithRake)
        ));
    }

    #[test]
    fn icm_sample_minimum_applies_only_to_sampled_fields() {
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        let exact_players = config.seats.len();
        let exact = UtilityConfig::TournamentIcm {
            outside_field: Vec::new(),
            payouts: (0..exact_players).rev().map(|place| place as f64).collect(),
            samples: 0,
            seed: 1,
        };
        exact.validate(config.seats.len()).unwrap();

        let outside_count = 16 - config.seats.len();
        let sampled = UtilityConfig::TournamentIcm {
            outside_field: (0..outside_count)
                .map(|index| FieldPlayerConfig {
                    name: format!("outside-{index}"),
                    stack_bb: 100.0,
                })
                .collect(),
            payouts: (0..16).rev().map(|place| place as f64).collect(),
            samples: 1,
            seed: 1,
        };
        assert!(matches!(
            sampled.validate(config.seats.len()),
            Err(ConfigError::IcmSamples)
        ));
    }

    #[test]
    fn min_raise_and_stack_fraction_sizes_validate_and_round_trip() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.betting.preflop.bet_sizes = vec![SizeSpec::MinRaise];
        config.betting.preflop.raise_sizes = vec![
            SizeSpec::MinRaise,
            SizeSpec::StackFraction { fraction: 0.25 },
            SizeSpec::StackFraction { fraction: 0.5 },
        ];
        config.betting.preflop.allin_threshold = Some(0.85);
        config.validate().unwrap();

        let encoded = toml::to_string(&config).unwrap();
        assert!(encoded.contains("min-raise"));
        assert!(encoded.contains("stack-fraction"));
        assert!(encoded.contains("allin_threshold"));
        let decoded: MultiwayConfig = toml::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.betting.preflop.bet_sizes, vec![SizeSpec::MinRaise]);
        assert_eq!(
            decoded.betting.preflop.allin_threshold,
            config.betting.preflop.allin_threshold
        );
    }

    #[test]
    fn stack_fraction_rejects_nonpositive_or_nonfinite_values() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            config.betting.preflop.bet_sizes = vec![SizeSpec::StackFraction { fraction: bad }];
            assert!(
                matches!(config.validate(), Err(ConfigError::Number(_))),
                "fraction {bad} should be rejected"
            );
        }
    }

    #[test]
    fn allin_threshold_validation_rejects_zero_above_one_and_nan() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        for bad in [0.0, 1.5, f64::NAN] {
            config.betting.preflop.allin_threshold = Some(bad);
            let result = config.validate();
            assert!(result.is_err(), "threshold {bad} should be rejected");
        }
        config.betting.preflop.allin_threshold = Some(1.0);
        config.validate().unwrap();
        config.betting.preflop.allin_threshold = Some(0.85);
        config.validate().unwrap();
    }

    #[test]
    fn exact_reraise_jam_ratio_validates_round_trips_and_conflicts_are_rejected() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config
            .betting
            .preflop
            .reraise_jam_above_actor_starting_stack = Some(StackRatio {
            numerator: 1,
            denominator: 3,
        });
        config.validate().unwrap();

        let encoded = toml::to_string(&config).unwrap();
        assert!(encoded.contains("reraise_jam_above_actor_starting_stack"));
        let decoded: MultiwayConfig = toml::from_str(&encoded).unwrap();
        assert_eq!(
            decoded
                .betting
                .preflop
                .reraise_jam_above_actor_starting_stack,
            Some(StackRatio {
                numerator: 1,
                denominator: 3,
            })
        );
        decoded.validate().unwrap();

        for ratio in [
            StackRatio {
                numerator: 0,
                denominator: 3,
            },
            StackRatio {
                numerator: 1,
                denominator: 0,
            },
            StackRatio {
                numerator: 4,
                denominator: 3,
            },
        ] {
            let mut invalid: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
            invalid
                .betting
                .preflop
                .reraise_jam_above_actor_starting_stack = Some(ratio);
            assert!(matches!(
                invalid.validate(),
                Err(ConfigError::ReraiseJamRatio { .. })
            ));
        }

        config.betting.preflop.allin_threshold = Some(0.85);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ConflictingAllinThresholds(Street::Preflop))
        ));

        let mut postflop: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        postflop.betting.flop.reraise_jam_above_actor_starting_stack = Some(StackRatio {
            numerator: 1,
            denominator: 3,
        });
        assert!(matches!(
            postflop.validate(),
            Err(ConfigError::PostflopReraiseJamRatio(Street::Flop))
        ));
    }

    #[test]
    fn default_recall_mode_is_full_and_is_omitted_from_serialization() {
        // Regression guard: `recall` must not appear in a default-mode
        // config's serialized form (JSON or TOML), so every config that
        // predates this field keeps unchanged bytes/config hashes and
        // round-trips unchanged.
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        assert_eq!(config.abstraction.recall, RecallMode::Full);
        let json = serde_json::to_string(&config.abstraction).unwrap();
        assert!(!json.contains("recall"));
        let toml_text = toml::to_string(&config).unwrap();
        assert!(!toml_text.contains("recall"));

        let mut street = config;
        street.abstraction.recall = RecallMode::Street;
        let street_json = serde_json::to_string(&street.abstraction).unwrap();
        assert!(street_json.contains("\"recall\":\"street\""));
        let decoded: AbstractionConfig = serde_json::from_str(&street_json).unwrap();
        assert_eq!(decoded.recall, RecallMode::Street);
    }

    #[test]
    fn default_abstraction_kind_is_rollout_kmeans_and_is_omitted_from_serialization() {
        // Regression guard: `kind` must not appear in a default-mode
        // config's serialized form (JSON or TOML), so every config that
        // predates this field keeps unchanged bytes/config hashes and
        // round-trips unchanged. The whole-config
        // TOML already contains the word "kind" from unrelated tagged enums
        // (`ante`'s `kind = "none"`, betting size `kind` tags), so the TOML
        // check below looks for the serialized variant name specifically
        // rather than the field name.
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        assert_eq!(config.abstraction.kind, AbstractionKind::RolloutKmeans);
        let json = serde_json::to_string(&config.abstraction).unwrap();
        assert!(!json.contains("\"kind\""));
        let toml_text = toml::to_string(&config).unwrap();
        assert!(!toml_text.contains("rollout-kmeans"));

        let mut ehs2 = config;
        ehs2.abstraction.kind = AbstractionKind::Ehs2Table;
        let ehs2_json = serde_json::to_string(&ehs2.abstraction).unwrap();
        assert!(ehs2_json.contains("\"kind\":\"ehs2-table\""));
        let decoded: AbstractionConfig = serde_json::from_str(&ehs2_json).unwrap();
        assert_eq!(decoded.kind, AbstractionKind::Ehs2Table);
    }

    #[test]
    fn rollout_training_defaults_are_omitted_and_nondefaults_round_trip() {
        // These values were hard-coded in RolloutTrainingParams before they
        // became configurable. Omitting them must therefore preserve the
        // serialized form of every existing core config.
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        assert_eq!(config.abstraction.points_per_bucket, 8);
        assert_eq!(config.abstraction.kmeans_iterations, 20);
        let json = serde_json::to_string(&config.abstraction).unwrap();
        assert!(!json.contains("points_per_bucket"));
        assert!(!json.contains("kmeans_iterations"));
        let toml_text = toml::to_string(&config).unwrap();
        assert!(!toml_text.contains("points_per_bucket"));
        assert!(!toml_text.contains("kmeans_iterations"));

        let mut tuned = config;
        tuned.abstraction.points_per_bucket = 16;
        tuned.abstraction.kmeans_iterations = 40;
        let tuned_json = serde_json::to_string(&tuned.abstraction).unwrap();
        assert!(tuned_json.contains("\"points_per_bucket\":16"));
        assert!(tuned_json.contains("\"kmeans_iterations\":40"));
        let decoded: AbstractionConfig = serde_json::from_str(&tuned_json).unwrap();
        assert_eq!(decoded.points_per_bucket, 16);
        assert_eq!(decoded.kmeans_iterations, 40);
    }

    #[test]
    fn rollout_training_parameters_are_validated_and_ehs2_rejects_nondefaults() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.abstraction.points_per_bucket = 0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::TrainingPointsPerBucket)
        ));

        config.abstraction.points_per_bucket = 8;
        config.abstraction.kmeans_iterations = 0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::KMeansIterations)
        ));

        config.abstraction.kmeans_iterations = 20;
        config.abstraction.kind = AbstractionKind::Ehs2Table;
        config.validate().unwrap();

        config.abstraction.points_per_bucket = 16;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Ehs2TableTraining)
        ));
        config.abstraction.points_per_bucket = 8;
        config.abstraction.kmeans_iterations = 40;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Ehs2TableTraining)
        ));
    }

    #[test]
    fn ehs2_table_rejects_active_opponent_bucket_overrides() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.abstraction.kind = AbstractionKind::Ehs2Table;
        config.abstraction.active_opponent_buckets = vec![ActiveOpponentBucketConfig {
            active_opponents: 1,
            flop_buckets: 16,
            turn_buckets: 16,
            river_buckets: 16,
        }];
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Ehs2TableOpponentBuckets)
        ));

        config.abstraction.active_opponent_buckets.clear();
        config.validate().unwrap();
    }

    #[test]
    fn default_config_toml_and_behavior_are_unchanged_by_new_size_variants() {
        // Regression guard: a config predating MinRaise/StackFraction/
        // allin_threshold must still validate, and its serialized form must
        // not gain the new field since it is None by default.
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.validate().unwrap();
        assert!(config.betting.preflop.allin_threshold.is_none());
        assert!(
            config
                .betting
                .preflop
                .reraise_jam_above_actor_starting_stack
                .is_none()
        );
        let encoded = toml::to_string(&config).unwrap();
        assert!(!encoded.contains("allin_threshold"));
        assert!(!encoded.contains("reraise_jam_above_actor_starting_stack"));
        assert!(!encoded.contains("min-raise"));
        assert!(!encoded.contains("stack-fraction"));
    }

    #[test]
    fn max_betting_players_absent_field_serializes_byte_identically() {
        // Regression guard: fingerprint stability. A config that never sets
        // max_betting_players must serialize (JSON, used for the game
        // fingerprint, and TOML) exactly as it did before this field existed.
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        assert!(config.betting.flop.max_betting_players.is_none());
        let json = serde_json::to_string(&config.betting.flop).unwrap();
        assert!(!json.contains("max_betting_players"));
        let toml_text = toml::to_string(&config).unwrap();
        assert!(!toml_text.contains("max_betting_players"));

        let mut capped = config;
        capped.betting.flop.max_betting_players = Some(2);
        let capped_json = serde_json::to_string(&capped.betting.flop).unwrap();
        assert!(capped_json.contains("\"max_betting_players\":2"));
        let decoded: StreetBettingConfig = serde_json::from_str(&capped_json).unwrap();
        assert_eq!(decoded.max_betting_players, Some(2));

        let encoded = toml::to_string(&capped).unwrap();
        let redecoded: MultiwayConfig = toml::from_str(&encoded).unwrap();
        redecoded.validate().unwrap();
        assert_eq!(redecoded.betting.flop.max_betting_players, Some(2));
    }

    #[test]
    fn max_betting_players_rejects_zero_and_preflop() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.betting.flop.max_betting_players = Some(0);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::MaxBettingPlayers)
        ));

        config.betting.flop.max_betting_players = Some(1);
        config.validate().unwrap();

        config.betting.flop.max_betting_players = None;
        config.betting.preflop.max_betting_players = Some(2);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::PreflopMaxBettingPlayers)
        ));
    }

    #[test]
    fn max_betting_players_seat_override_must_match_the_table() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.betting.turn.max_betting_players = Some(2);

        let mut mismatched_seat_betting = BettingConfig::default();
        mismatched_seat_betting.turn.max_betting_players = Some(3);
        config.seats[0].betting = Some(mismatched_seat_betting);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::SeatMaxBettingPlayersMismatch { seat: 0, street }) if street == Street::Turn
        ));

        let mut matched_seat_betting = BettingConfig::default();
        matched_seat_betting.turn.max_betting_players = Some(2);
        config.seats[0].betting = Some(matched_seat_betting);
        config.validate().unwrap();
    }
}
