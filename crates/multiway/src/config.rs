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
}

impl Default for BettingConfig {
    fn default() -> Self {
        Self {
            allow_limp: true,
            preflop: default_preflop_betting(),
            flop: default_postflop_betting(),
            turn: default_postflop_betting(),
            river: default_postflop_betting(),
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
}

fn default_max_aggressive_actions() -> u8 {
    4
}

fn default_preflop_betting() -> StreetBettingConfig {
    StreetBettingConfig {
        bet_sizes: vec![SizeSpec::ToBb { value: 2.5 }],
        isolate_sizes: None,
        raise_sizes: vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }],
        max_aggressive_actions: 4,
        include_allin: true,
    }
}

fn default_postflop_betting() -> StreetBettingConfig {
    StreetBettingConfig {
        bet_sizes: vec![SizeSpec::PotAfterCall { fraction: 0.5 }],
        isolate_sizes: None,
        raise_sizes: vec![SizeSpec::PotAfterCall { fraction: 0.75 }],
        max_aggressive_actions: 3,
        include_allin: true,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SizeSpec {
    ToBb { value: f64 },
    PotAfterCall { fraction: f64 },
    PreviousBetMultiple { factor: f64 },
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
    #[serde(default = "default_rollout_samples")]
    pub rollout_samples: u32,
    #[serde(default)]
    pub seed: u64,
    /// Optional postflop budgets keyed by the number of live opponents
    /// excluding the acting player. Missing counts use the global defaults.
    #[serde(default)]
    pub active_opponent_buckets: Vec<ActiveOpponentBucketConfig>,
    /// Optional operational cache location for trained centroids. This path
    /// is not part of game identity; validated artifact content is covered
    /// by the abstraction fingerprint.
    #[serde(default)]
    pub artifact_cache: Option<PathBuf>,
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
            seed: 0,
            active_opponent_buckets: Vec::new(),
            artifact_cache: None,
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

/// Standalone `[utility]` payload used by both CLI and direct library users.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UtilityConfig {
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

impl Default for UtilityConfig {
    fn default() -> Self {
        Self::ChipEv
    }
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
                if !(2..=100).contains(&field) {
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
                match *size {
                    SizeSpec::ToBb { value } => positive_finite("size.to-bb", value)?,
                    SizeSpec::PotAfterCall { fraction } => {
                        positive_finite("size.pot-after-call", fraction)?
                    }
                    SizeSpec::PreviousBetMultiple { factor } => {
                        positive_finite("size.previous-bet-multiple", factor)?;
                        if factor <= 1.0 {
                            return Err(ConfigError::RaiseFactor);
                        }
                    }
                }
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
                match *size {
                    SizeSpec::ToBb { value } => positive_finite("size.to-bb", value)?,
                    SizeSpec::PotAfterCall { fraction } => {
                        positive_finite("size.pot-after-call", fraction)?
                    }
                    SizeSpec::PreviousBetMultiple { factor } => {
                        positive_finite("size.previous-bet-multiple", factor)?;
                        if factor <= 1.0 {
                            return Err(ConfigError::RaiseFactor);
                        }
                    }
                }
            }
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
    #[error("active-opponent bucket profile {0} is duplicate or outside this table")]
    OpponentBucketProfile(u8),
    #[error("ICM field must contain 2 through 100 players, got {0}")]
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
    #[error("rake rate must be from zero through one")]
    RakeRate,
    #[error("per-hand rake and tournament ICM cannot be combined")]
    IcmWithRake,
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
}
