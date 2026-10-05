//! Shared rake and utility configuration with byte-compatible serde shapes.

use nlh::MwChips;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rake_condition::{CompiledRakeCondition, compile as compile_rake_condition};

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
        #[serde(
            default = "default_rounding_unit",
            skip_serializing_if = "is_default_rounding_unit"
        )]
        rounding_unit: MwChips,
    },
    GgPreflop {
        rate: f64,
        cap_bb: f64,
        exempt_pot_bb: f64,
    },
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
        rounding_unit: MwChips,
    },
    GgPreflop {
        rate: f64,
        cap: MwChips,
        exempt_pot: MwChips,
    },
}

fn default_rounding_unit() -> MwChips {
    MwChips(1)
}

fn is_default_rounding_unit(unit: &MwChips) -> bool {
    *unit == default_rounding_unit()
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
                rounding_unit,
            } => {
                rate_value(rate)?;
                if rounding_unit == MwChips::ZERO {
                    return Err(ConfigError::Number("rake.rounding_unit".into()));
                }
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
                    rounding_unit,
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
    #[error("{0} must be finite and within its allowed range")]
    Number(String),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icm_sample_minimum_applies_only_to_sampled_fields() {
        let table_players = 3;
        let exact_players = table_players;
        let exact = UtilityConfig::TournamentIcm {
            outside_field: Vec::new(),
            payouts: (0..exact_players).rev().map(|place| place as f64).collect(),
            samples: 0,
            seed: 1,
        };
        exact.validate(table_players).unwrap();

        let outside_count = 16 - table_players;
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
            sampled.validate(table_players),
            Err(ConfigError::IcmSamples)
        ));
    }
}
