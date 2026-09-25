//! Bet-size literal grammar shared by every config family that lowers bet
//! and raise trees.
//!
//! The spelling follows PioSOLVER's, so a size string copied out of Pio
//! reads the same here: a bare number is a percentage of the pot (`50`),
//! `c` is an absolute chip count (`20c`), `x` is a multiple of the wager
//! being faced (`3x`, and `2x` is the minimum legal raise), `a` is all-in,
//! and `e` is the geometric size that splits the remaining stack into equal
//! pot-fraction bets — bare `e` over the streets that are left, `3e` over
//! exactly three.
//!
//! Three literals have no Pio counterpart and keep an explicit spelling:
//! `min`, `80%effective`, `60%stack`. One is family-specific: `2.5bb`, for
//! the big-blind-denominated Multiway Preflop family, where Pio's chip
//! `c` does not apply.
//!
//! [`SizeSpec`] is the serialized/config representation of one sizing rule;
//! [`SizeSpec::parse`] and [`SizeSpec::render`] convert it to and from that
//! literal syntax. Pre-Pio spellings (`"allin"`, `"50%pot"`,
//! `"geometric(allin,streets=2)"`) are still accepted as input and
//! normalize to the Pio form. [`geometric_allin_target`] is the unit-free
//! formula shared by every engine that resolves a geometric size against a
//! concrete chip stack.

use serde::{Deserialize, Serialize};

/// A single configured bet/raise size, before it is resolved against a
/// concrete betting state.
///
/// Both TOML (`kind` tag) and the JSON game fingerprint identify variants by
/// name, not by declaration order, so appending a new variant at the end is
/// always compatible with existing configs and fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
    /// Absolute target expressed in the config's chip unit (postflop family).
    ToChips {
        value: f64,
    },
    /// Pio's bare `e`: geometric over however many betting streets are left
    /// from the one being resolved (three on the flop, two on the turn, one
    /// on the river). A separate variant rather than
    /// `GeometricAllIn { streets: Option<u8> }` so the existing variant's
    /// serialized shape — which multiway game fingerprints hash — is
    /// untouched.
    GeometricAllInRemaining,
}

/// Which absolute-chip literal (`bb` or `c`) a config family accepts.
/// `SizeSpec::parse`/`render` reject the other family's literal so a
/// misplaced `"2.5bb"` in a chip-denominated config fails loudly instead of
/// silently misinterpreting the number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeUnit {
    Bb,
    Chips,
}

/// A bet-size literal that does not match the grammar, or whose number is
/// out of range for its suffix (non-finite, non-positive, or a raise
/// multiple that is not strictly greater than one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSizeError(pub String);

impl std::fmt::Display for ParseSizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseSizeError {}

impl SizeSpec {
    /// Parses a bet-size literal: `"50"`, `"20c"`, `"3x"`, `"a"`, `"e"`,
    /// `"3e"`, `"min"`, `"2.5bb"`, `"80%effective"`, or `"60%stack"`. The
    /// pre-Pio spellings `"allin"`, `"50%pot"`, and
    /// `"geometric(allin,streets=2)"` are accepted too and normalize to the
    /// Pio form.
    ///
    /// `unit` selects which absolute-chip suffix (`bb` or `c`) is valid for
    /// the caller's config family; a literal using the other suffix is
    /// rejected.
    pub fn parse(source: &str, unit: SizeUnit) -> Result<SizeSpec, ParseSizeError> {
        let source = source.trim();
        if source == "min" {
            return Ok(SizeSpec::MinRaise);
        }
        // `allin` is the pre-Pio spelling, kept so old configs still read.
        if source == "a" || source == "allin" {
            return Ok(SizeSpec::AllIn);
        }
        if source == "e" {
            return Ok(SizeSpec::GeometricAllInRemaining);
        }
        if let Some(inner) = source
            .strip_prefix("geometric(allin,")
            .and_then(|value| value.strip_suffix(')'))
        {
            let inner = inner.trim();
            let inner = inner.strip_prefix("streets=").unwrap_or(inner);
            let streets = inner.parse::<u8>().map_err(|_| {
                ParseSizeError(format!("invalid geometric street count in {source:?}"))
            })?;
            if streets == 0 {
                return Err(ParseSizeError(
                    "geometric street count must be positive".to_string(),
                ));
            }
            return Ok(SizeSpec::GeometricAllIn { streets });
        }
        // A bare number is Pio's percentage of the pot. It is matched last,
        // as the empty suffix, so every suffixed form wins first.
        let (number, kind) = ["%effective", "%stack", "%pot", "bb", "c", "x", "e", ""]
            .into_iter()
            .find_map(|suffix| source.strip_suffix(suffix).map(|number| (number, suffix)))
            .ok_or_else(|| ParseSizeError(format!("invalid tree size literal {source:?}")))?;
        let value = number.parse::<f64>().map_err(|_| {
            // With the empty suffix nothing was recognized at all, so say
            // that rather than blaming a number the author never wrote.
            if kind.is_empty() {
                ParseSizeError(format!("invalid tree size literal {source:?}"))
            } else {
                ParseSizeError(format!("invalid number in tree size {source:?}"))
            }
        })?;
        if !value.is_finite() || value <= 0.0 {
            return Err(ParseSizeError(format!(
                "tree size must be finite and positive: {source:?}"
            )));
        }
        // A bare number below one is almost always a pot *fraction* written
        // by someone used to the pre-Pio spelling, where `0.33` meant a
        // third of the pot and now means a third of a percent. Say so
        // rather than silently building a one-chip bet.
        if kind.is_empty() && value < 1.0 {
            return Err(ParseSizeError(format!(
                "bet size {source} is {source}% of the pot; sizes are percentages, so write \
                 {} for {}% of the pot, or \"{source}%pot\" if you really mean {source}%",
                value * 100.0,
                value * 100.0,
            )));
        }
        Ok(match kind {
            "bb" if unit == SizeUnit::Bb => SizeSpec::ToBb { value },
            "bb" => {
                return Err(ParseSizeError(format!(
                    "the bb unit is not valid for this family: {source:?}"
                )));
            }
            "c" if unit == SizeUnit::Chips => SizeSpec::ToChips { value },
            "c" => {
                return Err(ParseSizeError(format!(
                    "the c (chips) unit is not valid for this family: {source:?}"
                )));
            }
            "%pot" | "" => SizeSpec::PotAfterCall {
                fraction: value / 100.0,
            },
            "e" if value >= 1.0 && value.fract() == 0.0 && value <= f64::from(u8::MAX) => {
                SizeSpec::GeometricAllIn {
                    streets: value as u8,
                }
            }
            "e" => {
                return Err(ParseSizeError(format!(
                    "geometric street count must be a whole number of at least one: {source:?}"
                )));
            }
            "x" if value > 1.0 => SizeSpec::PreviousBetMultiple { factor: value },
            "x" => {
                return Err(ParseSizeError(format!(
                    "current-bet multiple must be greater than one: {source:?}"
                )));
            }
            "%effective" => SizeSpec::EffectiveStackFraction {
                fraction: value / 100.0,
            },
            "%stack" => SizeSpec::StackFraction {
                fraction: value / 100.0,
            },
            _ => unreachable!(),
        })
    }

    /// Inverse of `parse`; `parse(&spec.render(unit), unit) == Ok(spec)`.
    ///
    /// `unit` must match `self`'s family for `ToBb`/`ToChips` (debug-checked
    /// below); the other variants render the same literal regardless of
    /// `unit`.
    pub fn render(self, unit: SizeUnit) -> String {
        match self {
            SizeSpec::ToBb { value } => {
                debug_assert_eq!(unit, SizeUnit::Bb, "ToBb only renders under SizeUnit::Bb");
                format!("{value}bb")
            }
            SizeSpec::ToChips { value } => {
                debug_assert_eq!(
                    unit,
                    SizeUnit::Chips,
                    "ToChips only renders under SizeUnit::Chips"
                );
                format!("{value}c")
            }
            SizeSpec::PotAfterCall { fraction } => render_percent(fraction),
            SizeSpec::PreviousBetMultiple { factor } => format!("{factor}x"),
            SizeSpec::MinRaise => "min".into(),
            SizeSpec::AllIn => "a".into(),
            SizeSpec::EffectiveStackFraction { fraction } => {
                format!("{}%effective", render_percent(fraction))
            }
            SizeSpec::StackFraction { fraction } => {
                format!("{}%stack", render_percent(fraction))
            }
            SizeSpec::GeometricAllIn { streets } => format!("{streets}e"),
            SizeSpec::GeometricAllInRemaining => "e".into(),
        }
    }
}

/// Renders a fraction as a percentage, shortest first.
///
/// `fraction * 100.0` is not exact in binary — `0.29 * 100.0` is
/// `28.999999999999996` — and printing that would put a 17-digit artifact
/// into every effective config that spelled a size as a bare fraction. This
/// walks precisions upward and returns the first one that reads back as the
/// same `f64`, so the rendered literal is both short and exact.
fn render_percent(fraction: f64) -> String {
    let scaled = fraction * 100.0;
    for precision in 0..=17 {
        let text = format!("{scaled:.precision$}");
        if text.parse::<f64>().map(|value| value / 100.0) == Ok(fraction) {
            return text;
        }
    }
    format!("{scaled}")
}

/// Target wager that reaches `maximum` in `streets` equal geometric raises,
/// on whatever integral chip grid the caller uses (multiway's 0.001bb grid,
/// postflop's raw chip grid, ...). `called_to` is the amount already put in
/// by the acting seat if this raise is called; `pot_after_call` is the pot
/// size once that call is made.
pub fn geometric_allin_target(
    called_to: u64,
    pot_after_call: u64,
    maximum: u64,
    streets: u8,
) -> u64 {
    let remaining = maximum.saturating_sub(called_to);
    if remaining == 0 || pot_after_call == 0 {
        return maximum;
    }
    let steps = f64::from(streets.max(1));
    let growth = 1.0 + 2.0 * remaining as f64 / pot_after_call as f64;
    let fraction = (growth.powf(1.0 / steps) - 1.0) / 2.0;
    let scaled = (pot_after_call as f64 * fraction).round();
    called_to
        .checked_add(scaled.clamp(0.0, u64::MAX as f64) as u64)
        .expect("geometric all-in target overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The serde JSON shape of every pre-existing variant must not change:
    /// multiway game fingerprints and checkpoints serialize `SizeSpec`.
    #[test]
    fn serde_json_shape_is_unchanged() {
        assert_eq!(
            serde_json::to_string(&SizeSpec::ToBb { value: 2.5 }).unwrap(),
            r#"{"kind":"to-bb","value":2.5}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::PotAfterCall { fraction: 0.5 }).unwrap(),
            r#"{"kind":"pot-after-call","fraction":0.5}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::PreviousBetMultiple { factor: 3.0 }).unwrap(),
            r#"{"kind":"previous-bet-multiple","factor":3.0}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::MinRaise).unwrap(),
            r#"{"kind":"min-raise"}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::StackFraction { fraction: 0.6 }).unwrap(),
            r#"{"kind":"stack-fraction","fraction":0.6}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::AllIn).unwrap(),
            r#"{"kind":"all-in"}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::EffectiveStackFraction { fraction: 0.8 }).unwrap(),
            r#"{"kind":"effective-stack-fraction","fraction":0.8}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::GeometricAllIn { streets: 2 }).unwrap(),
            r#"{"kind":"geometric-all-in","streets":2}"#
        );
        assert_eq!(
            serde_json::to_string(&SizeSpec::ToChips { value: 40.0 }).unwrap(),
            r#"{"kind":"to-chips","value":40.0}"#
        );
    }

    fn bb_specs() -> Vec<SizeSpec> {
        vec![
            SizeSpec::ToBb { value: 2.5 },
            SizeSpec::PotAfterCall { fraction: 0.5 },
            SizeSpec::PreviousBetMultiple { factor: 3.0 },
            SizeSpec::MinRaise,
            SizeSpec::StackFraction { fraction: 0.6 },
            SizeSpec::AllIn,
            SizeSpec::EffectiveStackFraction { fraction: 0.8 },
            SizeSpec::GeometricAllIn { streets: 2 },
            SizeSpec::GeometricAllInRemaining,
        ]
    }

    fn chips_specs() -> Vec<SizeSpec> {
        vec![
            SizeSpec::ToChips { value: 40.0 },
            SizeSpec::PotAfterCall { fraction: 0.5 },
            SizeSpec::PreviousBetMultiple { factor: 3.0 },
            SizeSpec::MinRaise,
            SizeSpec::StackFraction { fraction: 0.6 },
            SizeSpec::AllIn,
            SizeSpec::EffectiveStackFraction { fraction: 0.8 },
            SizeSpec::GeometricAllIn { streets: 2 },
            SizeSpec::GeometricAllInRemaining,
        ]
    }

    #[test]
    fn round_trips_every_variant_in_bb_unit() {
        for spec in bb_specs() {
            let rendered = spec.render(SizeUnit::Bb);
            assert_eq!(SizeSpec::parse(&rendered, SizeUnit::Bb), Ok(spec));
        }
    }

    /// Pot percentages are the common case, so they must render as Pio's
    /// bare number and must not leak binary artifacts (`0.29 * 100.0` is
    /// `28.999999999999996`).
    #[test]
    fn percentages_render_short_and_exact() {
        for (fraction, expected) in [
            (0.29, "29"),
            (0.33, "33"),
            (0.075, "7.5"),
            (1.0, "100"),
            (0.6666, "66.66"),
        ] {
            let spec = SizeSpec::PotAfterCall { fraction };
            let rendered = spec.render(SizeUnit::Chips);
            assert_eq!(rendered, expected);
            assert_eq!(SizeSpec::parse(&rendered, SizeUnit::Chips), Ok(spec));
        }
    }

    #[test]
    fn round_trips_every_variant_in_chips_unit() {
        for spec in chips_specs() {
            let rendered = spec.render(SizeUnit::Chips);
            assert_eq!(SizeSpec::parse(&rendered, SizeUnit::Chips), Ok(spec));
        }
    }

    #[test]
    fn rejects_bad_literals() {
        for source in [
            "0%pot",
            "0",
            "1x",
            "-2bb",
            "nanbb",
            "geometric(allin,0)",
            "0e",
            "2.5e",
            "not-a-size",
        ] {
            assert!(
                SizeSpec::parse(source, SizeUnit::Bb).is_err(),
                "expected {source:?} to be rejected"
            );
        }
        assert!(SizeSpec::parse("2.5bb", SizeUnit::Chips).is_err());
        assert!(SizeSpec::parse("40c", SizeUnit::Bb).is_err());
    }

    /// The literals a Pio user already has in their fingers.
    #[test]
    fn the_pio_spellings_parse() {
        for (source, expected) in [
            ("33", SizeSpec::PotAfterCall { fraction: 0.33 }),
            ("75", SizeSpec::PotAfterCall { fraction: 0.75 }),
            ("150", SizeSpec::PotAfterCall { fraction: 1.5 }),
            ("20c", SizeSpec::ToChips { value: 20.0 }),
            ("2x", SizeSpec::PreviousBetMultiple { factor: 2.0 }),
            ("a", SizeSpec::AllIn),
            ("e", SizeSpec::GeometricAllInRemaining),
            ("3e", SizeSpec::GeometricAllIn { streets: 3 }),
            ("1e", SizeSpec::GeometricAllIn { streets: 1 }),
        ] {
            assert_eq!(
                SizeSpec::parse(source, SizeUnit::Chips),
                Ok(expected),
                "{source:?}"
            );
        }
    }

    /// The spellings this grammar used before it followed Pio still read,
    /// and normalize to the Pio form.
    #[test]
    fn the_pre_pio_spellings_still_parse_and_normalize() {
        for (old, new) in [
            ("allin", "a"),
            ("50%pot", "50"),
            ("geometric(allin,2)", "2e"),
            ("geometric(allin,streets=2)", "2e"),
        ] {
            let spec = SizeSpec::parse(old, SizeUnit::Chips).expect(old);
            assert_eq!(spec.render(SizeUnit::Chips), new, "{old:?}");
        }
    }

    /// A bare number below one is the pre-Pio pot *fraction*, off by 100x.
    /// It must name the fix rather than build a one-chip bet.
    #[test]
    fn a_bare_fraction_says_sizes_are_percentages() {
        let error = SizeSpec::parse("0.33", SizeUnit::Chips).unwrap_err().0;
        assert!(error.contains("percentages"), "{error}");
        assert!(error.contains("33"), "{error}");
        // Spelled out explicitly, a sub-one-percent size is still legal.
        assert_eq!(
            SizeSpec::parse("0.33%pot", SizeUnit::Chips),
            Ok(SizeSpec::PotAfterCall { fraction: 0.0033 })
        );
    }

    #[test]
    fn geometric_all_in_matches_hand_computed_target() {
        // Doubling the pot exactly once should land on the maximum.
        let target = geometric_allin_target(1_000, 2_000, 5_000, 1);
        assert_eq!(target, 5_000);
        // Zero remaining or zero pot short-circuits to `maximum`.
        assert_eq!(geometric_allin_target(5_000, 2_000, 5_000, 3), 5_000);
        assert_eq!(geometric_allin_target(1_000, 0, 5_000, 3), 5_000);
    }
}
