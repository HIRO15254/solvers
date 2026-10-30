//! Shared rake and utility configuration with byte-compatible serde shapes.

use nlh::{ChipScale, MwChips, Rounding};
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
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldPlayerConfig {
    pub name: String,
    pub stack_chips: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RakeAllocation {
    #[default]
    MainFirst,
    Proportional,
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
        cap_chips: Option<u64>,
        when: String,
        allocation: RakeAllocation,
        rounding: Rounding,
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
        rounding: Rounding,
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
                for (index, player) in outside_field.iter().enumerate() {
                    if player.name.trim().is_empty() {
                        return Err(ConfigError::OutsideName(index));
                    }
                    if player.stack_chips == 0 {
                        return Err(ConfigError::Number(format!(
                            "outside_field[{index}].stack_chips"
                        )));
                    }
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
    /// Compile BB amounts onto whole chips of `scale`.
    pub fn compile(&self, scale: ChipScale) -> Result<CompiledRake, ConfigError> {
        let chips = |field: &str, bb: f64| {
            scale
                .from_bb(bb)
                .map_err(|_| ConfigError::Number(field.into()))
        };
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
                    cap: chips("rake.cap_bb", cap_bb)?,
                    no_flop_no_drop,
                })
            }
            RakeConfig::Generic {
                rate,
                cap_chips,
                ref when,
                allocation,
                rounding,
                rounding_unit,
            } => {
                rate_value(rate)?;
                if rounding_unit == MwChips::ZERO {
                    return Err(ConfigError::Number("rake.rounding_unit".into()));
                }
                let cap = cap_chips.map(MwChips);
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
                    cap: chips("rake.cap_bb", cap_bb)?,
                    exempt_pot: chips("rake.exempt_pot_bb", exempt_pot_bb)?,
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
    fn generic_rake_cap_preserves_exact_chips() {
        let json = r#"{"kind":"generic","rate":0.05,"cap_chips":18446744073709551615,"when":"true","allocation":"main-first","rounding":"down"}"#;
        let config: RakeConfig = serde_json::from_str(json).unwrap();
        assert!(
            serde_json::to_string(&config)
                .unwrap()
                .contains(r#""cap_chips":18446744073709551615"#)
        );
        assert!(matches!(
            config.compile(ChipScale::DEFAULT).unwrap(),
            CompiledRake::Generic {
                cap: Some(MwChips(u64::MAX)),
                ..
            }
        ));
        assert!(serde_json::from_str::<RakeConfig>(
            r#"{"kind":"generic","rate":0.05,"cap_chips":1.5,"when":"true","allocation":"main-first","rounding":"down"}"#
        ).is_err());
        assert!(serde_json::from_str::<RakeConfig>(
            r#"{"kind":"generic","rate":0.05,"cap_bb":1.0,"when":"true","allocation":"main-first","rounding":"down"}"#
        ).is_err());
    }

    #[test]
    fn outside_field_chips_are_exact_and_positive() {
        let json = r#"{"kind":"tournament-icm","outside_field":[{"name":"Field","stack_chips":18446744073709551615}],"payouts":[100.0,0.0]}"#;
        let config: UtilityConfig = serde_json::from_str(json).unwrap();
        config.validate(1).unwrap();
        let encoded = serde_json::to_string(&config).unwrap();
        assert!(encoded.contains(r#""stack_chips":18446744073709551615"#));

        let zero: UtilityConfig = serde_json::from_str(
            r#"{"kind":"tournament-icm","outside_field":[{"name":"Field","stack_chips":0}],"payouts":[100.0,0.0]}"#,
        )
        .unwrap();
        assert!(matches!(
            zero.validate(1),
            Err(ConfigError::Number(field)) if field == "outside_field[0].stack_chips"
        ));
    }

    #[test]
    fn outside_field_rejects_non_integer_and_legacy_stacks() {
        for player in [
            r#"{"name":"Field","stack_chips":1.5}"#,
            r#"{"name":"Field","stack_chips":-1}"#,
            r#"{"name":"Field","stack_bb":10.0}"#,
        ] {
            assert!(serde_json::from_str::<FieldPlayerConfig>(player).is_err());
        }
    }
}
