//! Internal typed game configuration and shared validation.
//!
//! Common-input lowering constructs `MultiwayConfig` with integral chip amounts;
//! solver settings, rake, and utility remain separate typed values.

use std::path::PathBuf;
use std::str::FromStr;

use nlh::Range;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{MAX_SEATS, MIN_SEATS, MwChips, SeatId, SeatVec};
pub use economics::{CompiledRake, FieldPlayerConfig, RakeAllocation, RakeConfig, UtilityConfig};
use nlh::{ChipScale, Rounding};

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
    /// Chip denomination and rounding; defaults are omitted to preserve fingerprints.
    #[serde(
        default = "default_bb_chips",
        skip_serializing_if = "is_default_bb_chips"
    )]
    pub bb_chips: u64,
    #[serde(default, skip_serializing_if = "Rounding::is_nearest")]
    pub chip_rounding: Rounding,
}

fn default_bb_chips() -> u64 {
    ChipScale::DEFAULT.chips_per_bb()
}
fn is_default_bb_chips(value: &u64) -> bool {
    *value == default_bb_chips()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub stack_chips: MwChips,
    /// Empty means the full 1,326-combo range.  Non-empty strings use the
    /// shared `nlh::Range` grammar.
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
    pub small_chips: MwChips,
    #[serde(default = "default_big_blind")]
    pub big_chips: MwChips,
}

impl Default for BlindConfig {
    fn default() -> Self {
        Self {
            small_chips: default_small_blind(),
            big_chips: default_big_blind(),
        }
    }
}

fn default_small_blind() -> MwChips {
    MwChips(500)
}

fn default_big_blind() -> MwChips {
    MwChips(1_000)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AnteConfig {
    #[default]
    None,
    Each {
        amount_chips: MwChips,
    },
    BigBlind {
        amount_chips: MwChips,
    },
}

/// Normalized v1 forced contributions. Legacy configs leave this absent and
/// derive the usual SB/BB plus table ante from `blinds` and `ante`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForcedBetConfig {
    pub blinds_chips: Vec<MwChips>,
    pub antes_chips: Vec<MwChips>,
    #[serde(default)]
    pub common_ante_chips: MwChips,
    pub nominal_big_blind_chips: MwChips,
    pub first_to_act: SeatId,
    /// Ordered live posts (seat, total wager in chips).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub straddles: Vec<(SeatId, MwChips)>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BettingConfig {
    /// Common-input rules in source order, evaluated by the P2 dialect.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nlh_rules: Vec<crate::tree_rules::NlhTreeRule>,
}

/// The same five effects a postflop `.tree` script's `add`/`remove`/
/// `replace`/`force`/`checkdown` statements name -- one enum, shared via
/// `nlh::script`, rather than a second copy of the same five spellings.
pub type RuleEffect = nlh::script::Effect;

/// The same five actions a postflop `.tree` script's `bet`/`raise` (and,
/// here, `fold`/`check`/`call`) statements name -- see [`RuleEffect`].
pub type RuleAction = nlh::script::ActionKind;

/// Bet/raise size grammar, shared with every other config family that
/// lowers a bet or raise tree. Defined in `nlh::sizing`; re-exported here
/// so `mw_preflop::config::SizeSpec` keeps working for every existing user.
pub use nlh::SizeSpec;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// `Full` remains the serialized default for historical config identity;
    /// solver construction and restore reject it explicitly.
    /// `Street` switches to a bounded-memory dense arena keyed only by the
    /// current street's bucket (a Monker/Pluribus-style imperfect-recall
    /// abstraction); see `docs/mw-preflop.jp.md`. Skipped when `Full` so
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
    /// Historical bucket-history identity, retained for serialized configs
    /// and artifacts. Solver construction and restore reject this mode.
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
    128
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
    pub straddles: Vec<(SeatId, MwChips)>,
    pub abstraction: AbstractionConfig,
    pub chip_rounding: Rounding,
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
        ChipScale::new(self.bb_chips).ok_or_else(|| ConfigError::Number("bb_chips".into()))?;

        for (index, seat) in self.seats.iter().enumerate() {
            if seat.stack_chips == MwChips::ZERO {
                return Err(ConfigError::Number(format!("seats[{index}].stack_chips")));
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
            }
        }

        if self.blinds.small_chips > self.blinds.big_chips || self.blinds.big_chips == MwChips::ZERO
        {
            return Err(ConfigError::Blinds);
        }

        if let Some(forced) = &self.forced_bets {
            if forced.blinds_chips.len() != num_seats || forced.antes_chips.len() != num_seats {
                return Err(ConfigError::ForcedBetCount {
                    expected: num_seats,
                    blinds: forced.blinds_chips.len(),
                    antes: forced.antes_chips.len(),
                });
            }
            if forced.first_to_act.index() >= num_seats {
                return Err(ConfigError::FirstActor {
                    actor: forced.first_to_act.index(),
                    num_seats,
                });
            }
            let maximum = forced
                .blinds_chips
                .iter()
                .copied()
                .max()
                .unwrap_or(MwChips::ZERO);
            if maximum != forced.nominal_big_blind_chips {
                return Err(ConfigError::NominalBlind);
            }
            let mut previous = forced.nominal_big_blind_chips;
            let mut expected = if num_seats == 2 {
                self.button
            } else {
                self.button.advance(3, num_seats)
            };
            if forced.straddles.len() > num_seats - 2 {
                return Err(ConfigError::Number("forced_bets.straddles count".into()));
            }
            for &(seat, amount) in &forced.straddles {
                if seat != expected
                    || previous
                        .checked_add(previous)
                        .is_none_or(|minimum| amount < minimum)
                    || forced.antes_chips[seat.index()]
                        .checked_add(amount)
                        .is_none_or(|total| self.seats[seat.index()].stack_chips <= total)
                {
                    return Err(ConfigError::Number(
                        "forced_bets.straddles order/amount/stack".into(),
                    ));
                }
                previous = amount;
                expected = expected.next(num_seats);
            }
            if !forced.straddles.is_empty() && forced.first_to_act != expected {
                return Err(ConfigError::Number(
                    "forced_bets.straddles first actor".into(),
                ));
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
        let scale = ChipScale::new(self.bb_chips).expect("validated chip scale");
        let ranges = self.compile_ranges()?;
        let seats = self
            .seats
            .iter()
            .zip(ranges)
            .map(|(seat, range)| ValidatedSeat {
                name: seat.name.clone(),
                starting_stack: seat.stack_chips,
                range,
                betting: seat.betting.clone(),
            })
            .collect();
        let ante = match self.ante {
            AnteConfig::None => ValidatedAnte::None,
            AnteConfig::Each { amount_chips } => ValidatedAnte::Each(amount_chips),
            AnteConfig::BigBlind { amount_chips } => ValidatedAnte::BigBlind(amount_chips),
        };
        let num_seats = self.seats.len();
        let small_blind_seat = if num_seats == 2 {
            self.button
        } else {
            self.button.next(num_seats)
        };
        let big_blind_seat = small_blind_seat.next(num_seats);
        let mut forced_blinds = vec![MwChips::ZERO; num_seats];
        forced_blinds[small_blind_seat.index()] = self.blinds.small_chips;
        forced_blinds[big_blind_seat.index()] = self.blinds.big_chips;
        let mut forced_antes = vec![MwChips::ZERO; num_seats];
        let mut common_ante = MwChips::ZERO;
        match ante {
            ValidatedAnte::None => {}
            ValidatedAnte::Each(amount) => forced_antes.fill(amount),
            ValidatedAnte::BigBlind(amount) => common_ante = amount,
        }
        let mut nominal_big_blind = scale.bb();
        let mut preflop_first_to_act = big_blind_seat.next(num_seats);
        if let Some(forced) = &self.forced_bets {
            forced_blinds.clone_from(&forced.blinds_chips);
            forced_antes.clone_from(&forced.antes_chips);
            common_ante = forced.common_ante_chips;
            nominal_big_blind = forced.nominal_big_blind_chips;
            preflop_first_to_act = forced.first_to_act;
        }
        Ok(ValidatedMultiwayConfig {
            seats: SeatVec::new_unchecked(seats),
            button: self.button,
            blinds: ValidatedBlinds {
                small: self.blinds.small_chips,
                big: self.blinds.big_chips,
            },
            ante,
            betting: self.betting.clone(),
            forced_blinds: SeatVec::new_unchecked(forced_blinds),
            forced_antes: SeatVec::new_unchecked(forced_antes),
            common_ante,
            nominal_big_blind,
            preflop_first_to_act,
            straddles: self
                .forced_bets
                .as_ref()
                .map_or_else(Vec::new, |forced| forced.straddles.clone()),
            abstraction: self.abstraction.clone(),
            chip_rounding: self.chip_rounding,
        })
    }

    pub fn validate_economics(
        &self,
        utility: &UtilityConfig,
        rake: &RakeConfig,
    ) -> Result<(), ConfigError> {
        self.validate()?;
        utility.validate(self.seats.len())?;
        rake.compile(ChipScale::new(self.bb_chips).expect("validated chip scale"))?;
        if matches!(utility, UtilityConfig::TournamentIcm { .. })
            && !matches!(rake, RakeConfig::None)
        {
            return Err(ConfigError::IcmWithRake);
        }
        Ok(())
    }
}

pub(crate) fn validate_size_spec(size: &SizeSpec) -> Result<(), ConfigError> {
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
        // Bare `e` carries no street count to validate: the engine reads it
        // off the street being resolved.
        SizeSpec::GeometricAllInRemaining => Ok(()),
        // A positive BB literal may round down to zero before the minimum
        // legal target is applied.
        SizeSpec::ToChips { .. } => Ok(()),
    }
}

fn validate_betting(config: &BettingConfig) -> Result<(), ConfigError> {
    for rule in &config.nlh_rules {
        rule.validate()?;
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

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Economics(#[from] economics::ConfigError),
    #[error("multiway table must contain 2 through 9 seats, got {0}")]
    SeatCount(usize),
    #[error("button seat {button} is outside a {num_seats}-seat table")]
    Button { button: usize, num_seats: usize },
    #[error("{0} must be finite and within its allowed range")]
    Number(String),
    #[error("seat {0} has an empty name")]
    EmptyName(usize),
    #[error("small blind must be non-negative and no greater than a positive big blind")]
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
    #[error("per-hand rake and tournament ICM cannot be combined")]
    IcmWithRake,
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
stack_chips = 40000

[[seats]]
stack_chips = 25500
range = "22+,A2s+"

[[seats]]
stack_chips = 12000
"#
    }

    #[test]
    fn minimal_three_seat_config_applies_defaults_and_round_trips() {
        let config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.validate().unwrap();
        assert_eq!(config.blinds.small_chips, MwChips(500));
        assert_eq!(config.abstraction.flop_buckets, 128);
        assert!(config.abstraction.active_opponent_buckets.is_empty());
        assert_eq!(config.compile_ranges().unwrap().len(), 3);
        let encoded = toml::to_string(&config).unwrap();
        let decoded: MultiwayConfig = toml::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
    }

    #[test]
    fn validated_config_preserves_chip_amounts() {
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
        // `nlh::Range::from_str` now rejects any spec that selects zero
        // combos (including a range whose only entry has weight 0), so this
        // now fails inside `Range::from_str` itself and surfaces as
        // `ConfigError::Range` rather than reaching the
        // `total_weight() <= 0.0` check that produces `EmptyRange`.
        config.seats[0].range = "AA:0".to_string();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Range { seat: 0, .. })
        ));

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
                stack_chips: 100_000,
            }],
            payouts: vec![100.0, 60.0, 40.0, 0.0],
        };
        config
            .validate_economics(&utility, &RakeConfig::None)
            .unwrap();
        assert!(matches!(
            config.validate_economics(
                &utility,
                &RakeConfig::Generic {
                    rate: 0.05,
                    cap_chips: Some(3_000),
                    when: "true".into(),
                    allocation: RakeAllocation::default(),
                    rounding: Rounding::default(),
                    rounding_unit: MwChips(1),
                }
            ),
            Err(ConfigError::IcmWithRake)
        ));
    }

    #[test]
    fn stack_fraction_rejects_nonpositive_or_nonfinite_values() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                validate_size_spec(&SizeSpec::StackFraction { fraction: bad }),
                Err(ConfigError::Number(_))
            ));
        }
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
    fn rejects_legacy_betting_menus_and_fractional_chips() {
        assert!(toml::from_str::<BettingConfig>("allow_limp = true").is_err());
        assert!(toml::from_str::<BettingConfig>("[preflop]\nbet_sizes = []").is_err());
        let fractional = minimal_toml().replace("stack_chips = 40000", "stack_chips = 40.5");
        assert!(toml::from_str::<MultiwayConfig>(&fractional).is_err());
    }

    #[test]
    fn forced_bets_preserve_chips_and_validate_straddles() {
        let mut config: MultiwayConfig = toml::from_str(minimal_toml()).unwrap();
        config.forced_bets = Some(ForcedBetConfig {
            blinds_chips: vec![MwChips(0), MwChips(500), MwChips(1_000)],
            antes_chips: vec![MwChips(10); 3],
            common_ante_chips: MwChips(7),
            nominal_big_blind_chips: MwChips(1_000),
            first_to_act: SeatId(1),
            straddles: vec![(SeatId(0), MwChips(2_000))],
        });
        let validated = config.validated().unwrap();
        assert_eq!(validated.common_ante, MwChips(7));
        assert_eq!(validated.nominal_big_blind, MwChips(1_000));
        assert_eq!(validated.straddles, vec![(SeatId(0), MwChips(2_000))]);
        config.forced_bets.as_mut().unwrap().straddles[0].1 = MwChips(1_999);
        assert!(config.validate().is_err());
    }
}
