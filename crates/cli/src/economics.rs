//! Adapters that give the exact heads-up engine (`game::RakeModel` /
//! `game::UtilityModel`) the two economics models that today exist only in
//! the sampled Multiway Preflop engine: the generic condition-based rake
//! and tournament ICM against a fixed outside field.
//!
//! Neither type is wired into the TOML config parser here — that is a
//! separate change. This module only supplies the adapter types and their
//! own tests.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Result, anyhow};
use cards::{Chips, PerPlayer, Player};
use game::{
    ChipEv, GgPreflopRake, Icm, NoRake, PercentCapRake, RakeModel, TerminalDescriptor,
    TerminalKind, UtilityModel,
};

use crate::config::{RakeSection, UtilitySection};

/// Builds the rake model a config's `[rake]` section names.
///
/// Fallible because two of the four kinds carry things that can only be
/// checked by building them: a `when` condition has to compile, and the
/// numbers have to be in range. The contract parser calls the same
/// constructors during `validate`, so a config that validated cannot fail
/// here — but a lowered internal config read straight out of an artifact
/// never went through that check, and must not panic the solve path.
pub fn build_rake(rake: &RakeSection) -> Result<Box<dyn RakeModel>> {
    Ok(match rake {
        RakeSection::None => Box::new(NoRake),
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => Box::new(PercentCapRake {
            rate: *rate,
            cap: *cap,
            no_flop_no_drop: *no_flop_no_drop,
        }),
        RakeSection::Generic {
            rate,
            cap,
            when,
            allocation,
            rounding,
            rounding_unit,
        } => Box::new(
            GenericRake::compile(*rate, *cap, when, *allocation, *rounding, *rounding_unit)
                .map_err(|error| anyhow!("building the generic rake: {error}"))?,
        ),
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => Box::new(GgPreflopRake {
            rate: *rate,
            cap: *cap,
            exempt_pot: Chips(*exempt_pot),
        }),
    })
}

/// Builds the utility model a config's `[utility]` section names.
pub fn build_utility(utility: &UtilitySection) -> Result<Box<dyn UtilityModel>> {
    Ok(match utility {
        UtilitySection::ChipEv => Box::new(ChipEv),
        UtilitySection::Icm { payouts } => Box::new(Icm { payouts: *payouts }),
        UtilitySection::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => Box::new(
            TournamentIcm::new(
                payouts.clone(),
                outside_field.iter().map(|player| player.stack_bb).collect(),
                *samples,
                *seed,
            )
            .map_err(|error| anyhow!("building the tournament ICM utility: {error}"))?,
        ),
    })
}

/// The Multiway Preflop generic rake, applied to a heads-up postflop
/// subgame's single pot.
pub struct GenericRake {
    pub rate: f64,
    /// Cap in chips; `None` is uncapped.
    pub cap: Option<f64>,
    pub when: multiway::CompiledRakeCondition,
    /// No effect on a heads-up subgame: there is exactly one pot and never
    /// a side pot to allocate rake across. Kept so a `Generic` rake config
    /// is portable verbatim between the multiway and heads-up families.
    pub allocation: multiway::RakeAllocation,
    pub rounding: multiway::RakeRounding,
    /// Chip granularity the rake is rounded to. Positive and finite.
    pub rounding_unit: f64,
}

impl GenericRake {
    /// Compiles the `when` condition and validates the numeric fields.
    /// Returns the condition compiler's error string unchanged so callers
    /// can wrap it in their own error code.
    pub fn compile(
        rate: f64,
        cap: Option<f64>,
        when_source: &str,
        allocation: multiway::RakeAllocation,
        rounding: multiway::RakeRounding,
        rounding_unit: f64,
    ) -> Result<GenericRake, String> {
        if !(0.0..=1.0).contains(&rate) {
            return Err(format!(
                "generic rake rate must be within 0.0..=1.0, got {rate}"
            ));
        }
        if let Some(cap) = cap
            && !(cap.is_finite() && cap >= 0.0)
        {
            return Err(format!(
                "generic rake cap must be non-negative and finite, got {cap}"
            ));
        }
        if !(rounding_unit.is_finite() && rounding_unit > 0.0) {
            return Err(format!(
                "generic rake rounding unit must be positive and finite, got {rounding_unit}"
            ));
        }
        let when = multiway::rake_condition::compile(when_source)?;
        Ok(GenericRake {
            rate,
            cap,
            when,
            allocation,
            rounding,
            rounding_unit,
        })
    }
}

impl RakeModel for GenericRake {
    fn rake(&self, t: &TerminalDescriptor) -> f64 {
        // A heads-up postflop subgame always starts on a dealt flop (the
        // preflop street belongs to a different engine entirely), and both
        // seats are always dealt in and see the flop, so the multiway
        // condition context collapses to these four constants.
        let context = multiway::RakeConditionContext {
            flop_dealt: true,
            showdown: matches!(t.kind, TerminalKind::Showdown),
            players_dealt: 2,
            players_saw_flop: 2,
        };
        if !self.when.matches(context) {
            return 0.0;
        }
        // Match `multiway::settlement::percentage_with_rounding`: round the
        // exact percentage to the configured unit first, then cap. Multiway
        // applies the cap to the already-rounded value
        // (`crates/multiway/src/settlement.rs` `apply_rake`'s
        // `CompiledRake::Generic` arm), so this mirrors that order.
        let exact = t.pot.as_f64() * self.rate;
        let scaled = exact / self.rounding_unit;
        let rounded = match self.rounding {
            multiway::RakeRounding::Down => scaled.floor(),
            // f64::round rounds half-way cases away from zero, exactly the
            // semantics `percentage_with_rounding` relies on for `Nearest`.
            multiway::RakeRounding::Nearest => scaled.round(),
            multiway::RakeRounding::Up => scaled.ceil(),
        } * self.rounding_unit;
        match self.cap {
            Some(cap) => rounded.min(cap),
            None => rounded,
        }
    }

    fn is_free(&self) -> bool {
        self.rate == 0.0 || self.when.is_never()
    }
}

/// Multiway Preflop's tournament ICM (exact up to 15 players, deterministic
/// Monte Carlo above that), applied to the two seats of a heads-up subgame
/// plus a fixed outside field.
pub struct TournamentIcm {
    /// Padded to exactly `2 + outside_field.len()` entries (unpaid places
    /// trail as `0.0`), matching what `multiway::estimate_icm` requires.
    payouts: Vec<f64>,
    outside_field: Vec<f64>,
    samples: u64,
    seed: u64,
    /// `bake` calls `utility` twice per terminal across thousands of
    /// terminals; a fresh Monte Carlo ICM solve per call would be
    /// unusable, so results are cached on the two seats' quantized stacks.
    memo: Mutex<HashMap<(u64, u64), (f64, f64)>>,
}

impl TournamentIcm {
    /// Validates the payout structure and outside field up front so
    /// `utility` never needs to fail. `multiway::estimate_icm` re-validates
    /// its own inputs on every call (cheap relative to the Monte Carlo
    /// work), so this is a fast, friendlier-message pre-check, not the only
    /// line of defense.
    pub fn new(
        payouts: Vec<f64>,
        outside_field: Vec<f64>,
        samples: u64,
        seed: u64,
    ) -> Result<TournamentIcm, String> {
        if payouts.is_empty() {
            return Err("tournament ICM payouts must not be empty".to_string());
        }
        for (index, &payout) in payouts.iter().enumerate() {
            if !payout.is_finite() || payout < 0.0 {
                return Err(format!(
                    "tournament ICM payout {index} must be finite and non-negative, got {payout}"
                ));
            }
            if index > 0 && payouts[index - 1] < payout {
                return Err("tournament ICM payouts must be non-increasing".to_string());
            }
        }
        let field_size = 2 + outside_field.len();
        if payouts.len() > field_size {
            return Err(format!(
                "tournament ICM has {} payouts but only {field_size} players (2 seats + {} outside)",
                payouts.len(),
                outside_field.len()
            ));
        }
        for (index, &stack) in outside_field.iter().enumerate() {
            if !(stack.is_finite() && stack > 0.0) {
                return Err(format!(
                    "tournament ICM outside stack {index} must be positive and finite, got {stack}"
                ));
            }
        }
        if field_size > multiway::icm::EXACT_ICM_MAX_PLAYERS && samples < 2 {
            return Err(format!(
                "tournament ICM with {field_size} players requires at least 2 Monte Carlo samples, got {samples}"
            ));
        }
        let mut padded_payouts = payouts;
        padded_payouts.resize(field_size, 0.0);
        Ok(TournamentIcm {
            payouts: padded_payouts,
            outside_field,
            samples,
            seed,
            memo: Mutex::new(HashMap::new()),
        })
    }

    /// Converts stacks with `MwChips::try_from_bb`, then solves (or serves
    /// from the memo cache) the two seats' ICM values against the outside
    /// field.
    fn icm_pair(&self, stack_p0: f64, stack_p1: f64) -> (f64, f64) {
        // ICM depends only on stack proportions, so treating one chip as
        // one BB-unit is a faithful, lossless-to-0.001 mapping; only the
        // proportions among all field stacks matter, not their scale.
        let chips_p0 = multiway::MwChips::try_from_bb(stack_p0).unwrap_or(multiway::MwChips::ZERO);
        let chips_p1 = multiway::MwChips::try_from_bb(stack_p1).unwrap_or(multiway::MwChips::ZERO);
        let key = (chips_p0.raw(), chips_p1.raw());
        if let Some(&cached) = self.memo.lock().unwrap().get(&key) {
            return cached;
        }
        let mut stacks = Vec::with_capacity(2 + self.outside_field.len());
        stacks.push(chips_p0);
        stacks.push(chips_p1);
        for &stack in &self.outside_field {
            stacks.push(multiway::MwChips::try_from_bb(stack).unwrap_or(multiway::MwChips::ZERO));
        }
        let result = match multiway::estimate_icm(&stacks, &self.payouts, self.samples, self.seed) {
            Ok(estimate) => (estimate.values[0], estimate.values[1]),
            // A stack vector that makes it past `new`'s validation but
            // still upsets `estimate_icm` (e.g. every stack rounding down
            // to zero) must not panic the bake pass; report no equity
            // rather than crash.
            Err(_) => (0.0, 0.0),
        };
        self.memo.lock().unwrap().insert(key, result);
        result
    }
}

impl UtilityModel for TournamentIcm {
    fn utility(&self, stacks_after: &PerPlayer<f64>) -> PerPlayer<f64> {
        // Clamp rather than fail: `new` is the only fallible point, so a
        // non-finite or negative stack (which should not occur, but must
        // not panic the bake pass if it does) is clamped to zero first.
        let stack_p0 = stacks_after[Player::P0].max(0.0);
        let stack_p1 = if stacks_after[Player::P1].is_finite() {
            stacks_after[Player::P1].max(0.0)
        } else {
            0.0
        };
        let stack_p0 = if stack_p0.is_finite() { stack_p0 } else { 0.0 };
        let (utility_p0, utility_p1) = self.icm_pair(stack_p0, stack_p1);
        PerPlayer::new(utility_p0, utility_p1)
    }

    fn is_zero_sum_affine(&self) -> bool {
        // Two-player ICM is affine in stacks (see `game::Icm`'s doc
        // comment); adding a nonzero outside field breaks that, since a
        // seat's ICM value then also depends on how it compares to the
        // outside stacks, not just the other seat's stack.
        self.outside_field.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::Chips;
    use game::{ChipEv, Icm, PayoffPipeline};

    fn showdown_terminal(pot: u32, contrib: (u32, u32)) -> TerminalDescriptor {
        TerminalDescriptor {
            kind: TerminalKind::Showdown,
            street: cards::Street::River,
            pot: Chips(pot),
            contrib: PerPlayer::new(Chips(contrib.0), Chips(contrib.1)),
            stacks_before: PerPlayer::new(Chips(100), Chips(100)),
        }
    }

    fn fold_terminal(pot: u32, contrib: (u32, u32), folder: Player) -> TerminalDescriptor {
        TerminalDescriptor {
            kind: TerminalKind::Fold { folder },
            street: cards::Street::Flop,
            pot: Chips(pot),
            contrib: PerPlayer::new(Chips(contrib.0), Chips(contrib.1)),
            stacks_before: PerPlayer::new(Chips(100), Chips(100)),
        }
    }

    // --- GenericRake -------------------------------------------------

    #[test]
    fn flop_dealt_condition_rakes_a_showdown_terminal() {
        let rake = GenericRake::compile(
            0.05,
            None,
            "flop_dealt",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        assert_eq!(rake.rake(&showdown_terminal(20, (10, 10))), 1.0);
    }

    #[test]
    fn showdown_condition_only_rakes_showdown_terminals() {
        let rake = GenericRake::compile(
            0.1,
            None,
            "showdown",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        assert_eq!(rake.rake(&showdown_terminal(20, (10, 10))), 2.0);
        assert_eq!(
            rake.rake(&fold_terminal(20, (10, 10), Player::P0)),
            0.0,
            "a fold terminal is not a showdown"
        );
    }

    #[test]
    fn cap_binds_above_the_rounded_percentage() {
        let rake = GenericRake::compile(
            0.5,
            Some(3.0),
            "true",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        // 0.5 * 20 = 10, well above the 3.0 cap.
        assert_eq!(rake.rake(&showdown_terminal(20, (10, 10))), 3.0);
    }

    #[test]
    fn rounding_modes_match_multiway_semantics() {
        // rate * pot = 0.13 * 25 = 3.25, unit = 1.0: down -> 3, nearest ->
        // 3 (round-half-away-from-zero only matters exactly at .5), up -> 4.
        let down = GenericRake::compile(
            0.13,
            None,
            "true",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        let nearest = GenericRake::compile(
            0.13,
            None,
            "true",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Nearest,
            1.0,
        )
        .unwrap();
        let up = GenericRake::compile(
            0.13,
            None,
            "true",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Up,
            1.0,
        )
        .unwrap();
        let terminal = showdown_terminal(25, (12, 13));
        assert_eq!(down.rake(&terminal), 3.0);
        assert_eq!(nearest.rake(&terminal), 3.0);
        assert_eq!(up.rake(&terminal), 4.0);

        // Exactly half-way (0.5 * 5 = 2.5 with a 1.0 unit) rounds away from
        // zero under `Nearest`, matching `f64::round`.
        let half = GenericRake::compile(
            0.5,
            None,
            "true",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Nearest,
            1.0,
        )
        .unwrap();
        assert_eq!(half.rake(&showdown_terminal(5, (2, 3))), 3.0);
    }

    #[test]
    fn zero_rate_is_free() {
        let rake = GenericRake::compile(
            0.0,
            None,
            "true",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        assert!(rake.is_free());
    }

    #[test]
    fn unsatisfiable_condition_is_free() {
        let rake = GenericRake::compile(
            0.1,
            None,
            "players_dealt > 20",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        assert!(rake.is_free());
        assert_eq!(rake.rake(&showdown_terminal(20, (10, 10))), 0.0);
    }

    #[test]
    fn unparsable_condition_reports_an_error() {
        assert!(
            GenericRake::compile(
                0.1,
                None,
                "cards_seen > 3",
                multiway::RakeAllocation::MainFirst,
                multiway::RakeRounding::Down,
                1.0,
            )
            .is_err()
        );
    }

    #[test]
    fn generic_rake_pipeline_breaks_zero_sum() {
        let rake = GenericRake::compile(
            0.05,
            None,
            "showdown",
            multiway::RakeAllocation::MainFirst,
            multiway::RakeRounding::Down,
            1.0,
        )
        .unwrap();
        let pipeline = PayoffPipeline {
            rake: &rake,
            utility: &ChipEv,
        };
        assert!(!pipeline.is_zero_sum());
        let baked = pipeline.bake(&showdown_terminal(20, (10, 10)));
        assert!((baked.win_p0[Player::P0] + baked.win_p0[Player::P1] + 1.0).abs() < 1e-12);
    }

    // --- TournamentIcm -------------------------------------------------

    #[test]
    fn empty_outside_field_agrees_with_hu_icm() {
        let payouts = vec![100.0, 60.0];
        let icm = TournamentIcm::new(payouts.clone(), vec![], 1, 0).unwrap();
        let reference = Icm {
            payouts: [payouts[0], payouts[1]],
        };
        assert!(icm.is_zero_sum_affine());
        for (s0, s1) in [(1.0, 199.0), (50.0, 150.0), (100.0, 100.0), (199.0, 1.0)] {
            let stacks = PerPlayer::new(s0, s1);
            let got = icm.utility(&stacks);
            let want = reference.utility(&stacks);
            assert!((got[Player::P0] - want[Player::P0]).abs() < 1e-9);
            assert!((got[Player::P1] - want[Player::P1]).abs() < 1e-9);
        }
    }

    #[test]
    fn nonempty_outside_field_is_not_affine() {
        let icm = TournamentIcm::new(vec![100.0, 60.0, 30.0], vec![200.0], 1, 0).unwrap();
        assert!(!icm.is_zero_sum_affine());

        // A fixed 10-chip swing at a short stack must value differently
        // from the same swing at a deep stack once an outside field is
        // present (affine-in-stacks would make the two swings equal).
        let short_low = icm.utility(&PerPlayer::new(10.0, 100.0));
        let short_high = icm.utility(&PerPlayer::new(20.0, 100.0));
        let short_swing = short_high[Player::P0] - short_low[Player::P0];

        let deep_low = icm.utility(&PerPlayer::new(490.0, 100.0));
        let deep_high = icm.utility(&PerPlayer::new(500.0, 100.0));
        let deep_swing = deep_high[Player::P0] - deep_low[Player::P0];

        assert!(
            (short_swing - deep_swing).abs() > 1e-6,
            "short-stack swing {short_swing} should differ from deep-stack swing {deep_swing}"
        );
    }

    #[test]
    fn prize_pool_is_conserved_with_an_outside_field() {
        // `utility` only reports the two table seats, so exercise the same
        // stack vector `icm_pair` builds directly through
        // `multiway::estimate_icm` and check the *full* field's ICM values
        // (table seats plus outside field) sum to the total prize pool.
        let payouts = vec![100.0, 60.0, 30.0, 0.0];
        for (s0, s1) in [(10.0, 40.0), (100.0, 100.0), (300.0, 5.0)] {
            let stacks = [
                multiway::MwChips::try_from_bb(s0).unwrap(),
                multiway::MwChips::try_from_bb(s1).unwrap(),
                multiway::MwChips::try_from_bb(200.0).unwrap(),
                multiway::MwChips::try_from_bb(150.0).unwrap(),
            ];
            let estimate = multiway::estimate_icm(&stacks, &payouts, 1, 0).unwrap();
            let total: f64 = estimate.values.iter().sum();
            assert!(
                (total - 190.0).abs() < 1e-9,
                "stacks ({s0}, {s1}): field ICM values summed to {total}, want 190.0"
            );
        }
    }

    #[test]
    fn memo_cache_returns_identical_values_on_repeat_calls() {
        let icm = TournamentIcm::new(vec![100.0, 60.0, 30.0], vec![200.0], 2_000, 3).unwrap();
        let stacks = PerPlayer::new(123.456, 78.9);
        let first = icm.utility(&stacks);
        let second = icm.utility(&stacks);
        assert_eq!(first, second);
    }

    #[test]
    fn constructor_rejects_invalid_inputs() {
        assert!(
            TournamentIcm::new(vec![], vec![], 1, 0).is_err(),
            "empty payouts"
        );
        assert!(
            TournamentIcm::new(vec![50.0, 100.0], vec![], 1, 0).is_err(),
            "increasing payouts"
        );
        assert!(
            TournamentIcm::new(vec![100.0, -10.0], vec![], 1, 0).is_err(),
            "negative payout"
        );
        assert!(
            TournamentIcm::new(vec![100.0, 60.0, 30.0], vec![], 1, 0).is_err(),
            "more payouts than players"
        );
        let outside_field = vec![100.0; 14]; // 2 + 14 = 16 > EXACT_ICM_MAX_PLAYERS
        assert!(
            TournamentIcm::new(vec![100.0], outside_field, 1, 0).is_err(),
            "too few samples for a 16-player field"
        );
    }

    #[test]
    fn tournament_icm_pipeline_is_not_zero_sum_with_outside_field() {
        let icm = TournamentIcm::new(vec![100.0, 60.0, 30.0], vec![200.0], 1, 0).unwrap();
        let pipeline = PayoffPipeline {
            rake: &game::NoRake,
            utility: &icm,
        };
        assert!(!pipeline.is_zero_sum());
        let baked = pipeline.bake(&showdown_terminal(20, (10, 10)));
        // Baked payoffs need not be exactly zero-sum once an outside field
        // is present; just confirm the pipeline runs end to end.
        assert!(baked.win_p0[Player::P0].is_finite());
        assert!(baked.win_p0[Player::P1].is_finite());
    }
}
