use nlh::{Chips, PerPlayer, Player, Street};

/// Everything a rake or utility model may depend on at a terminal. Emitted
/// by variant rules at tree-build time; never seen by the engine.
#[derive(Clone, Copy, Debug)]
pub struct TerminalDescriptor {
    pub kind: TerminalKind,
    /// Street on which the hand ended (for no-flop-no-drop rake).
    pub street: Street,
    /// Total pot, both contributions included.
    pub pot: Chips,
    /// Per-player contribution to the pot.
    pub contrib: PerPlayer<Chips>,
    /// Stacks at the start of the hand, before posting anything.
    pub stacks_before: PerPlayer<Chips>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerminalKind {
    Fold { folder: Player },
    Showdown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    WinP0,
    Tie,
    WinP1,
}

/// Rake taken from the pot at a terminal, and how the rest of a tied pot is
/// divided. Build-time only.
pub trait RakeModel: Send + Sync {
    /// Rake in fractional chips (cap rules produce fractions of a chip when
    /// pots are expressed in small units).
    fn rake(&self, t: &TerminalDescriptor) -> f64;

    /// Chips each player takes from `net_pot` on a tied showdown. The
    /// default halves it exactly; table models split on their chip grid.
    fn tie_split(&self, net_pot: f64) -> PerPlayer<f64> {
        PerPlayer::new(0.5 * net_pot, 0.5 * net_pot)
    }

    /// True when this model never takes anything (enables the zero-sum
    /// fast path together with UtilityModel::is_zero_sum_affine).
    fn is_free(&self) -> bool {
        false
    }
}

pub struct NoRake;

impl RakeModel for NoRake {
    fn rake(&self, _t: &TerminalDescriptor) -> f64 {
        0.0
    }

    fn is_free(&self) -> bool {
        true
    }
}

#[cfg(any(test, feature = "test-support"))]
/// Percentage rake with a cap, optionally waived when the hand ends
/// preflop (no flop, no drop).
pub struct PercentCapRake {
    pub rate: f64,
    pub cap: f64,
    pub no_flop_no_drop: bool,
}

#[cfg(any(test, feature = "test-support"))]
impl RakeModel for PercentCapRake {
    fn rake(&self, t: &TerminalDescriptor) -> f64 {
        if self.no_flop_no_drop && t.street == Street::Preflop {
            return 0.0;
        }
        (t.pot.as_f64() * self.rate).min(self.cap)
    }
}

/// Maps final stacks to utilities. Build-time only.
pub trait UtilityModel: Send + Sync {
    fn utility(&self, stacks_after: &PerPlayer<f64>) -> PerPlayer<f64>;

    /// Utility baseline used when baking a terminal. Common-input models
    /// replace the builder's synthetic pot split with actual contributions.
    fn baseline(&self, t: &TerminalDescriptor) -> PerPlayer<f64> {
        self.utility(&t.stacks_before.map(|c| c.as_f64()))
    }

    /// True when utility is an affine function of chip counts with equal
    /// slopes (chip EV, and pure two-player ICM). Enables the zero-sum fast
    /// path and its invariant tests.
    fn is_zero_sum_affine(&self) -> bool;
}

/// Chip expected value: utility is the stack itself.
pub struct ChipEv;

impl UtilityModel for ChipEv {
    fn utility(&self, stacks_after: &PerPlayer<f64>) -> PerPlayer<f64> {
        *stacks_after
    }

    fn is_zero_sum_affine(&self) -> bool {
        true
    }
}

/// Malmuth–Harville ICM for the two remaining players of a tournament.
/// With two players ICM is affine in stacks — `$EV_i = p2 + (p1 - p2) *
/// s_i / S` — which the invariant tests exploit: a pure HU ICM solve must
/// match the chip-EV solve exactly.
pub struct Icm {
    /// Remaining prizes, best first (prize for 1st, prize for 2nd).
    pub payouts: [f64; 2],
}

impl UtilityModel for Icm {
    fn utility(&self, stacks_after: &PerPlayer<f64>) -> PerPlayer<f64> {
        let total = stacks_after.0[0] + stacks_after.0[1];
        let [p1, p2] = self.payouts;
        stacks_after.map(|s| p2 + (p1 - p2) * s / total)
    }

    fn is_zero_sum_affine(&self) -> bool {
        true
    }
}

/// Per-outcome utilities at one terminal, relative to the players' utility
/// at their starting stacks so that unraked chip-EV games are exactly
/// zero-sum. This is what tree builders bake into their evaluators; the
/// solve loop multiplies these constants and nothing else.
#[derive(Clone, Copy, Debug)]
pub struct BakedPayoffs {
    pub win_p0: PerPlayer<f64>,
    pub tie: PerPlayer<f64>,
    pub win_p1: PerPlayer<f64>,
}

impl BakedPayoffs {
    pub fn for_outcome(&self, outcome: Outcome) -> PerPlayer<f64> {
        match outcome {
            Outcome::WinP0 => self.win_p0,
            Outcome::Tie => self.tie,
            Outcome::WinP1 => self.win_p1,
        }
    }
}

/// The build-time pipeline: variant rules emit a [`TerminalDescriptor`],
/// the rake model shrinks the pot, the utility model maps final stacks to
/// utilities, and the result is baked to constants. Rake and ICM therefore
/// never touch the engine.
pub struct PayoffPipeline<'a> {
    pub rake: &'a dyn RakeModel,
    pub utility: &'a dyn UtilityModel,
}

impl PayoffPipeline<'_> {
    /// True when baked terminal utilities are exactly zero-sum: a free rake
    /// model and an affine (equal-slope) utility.
    pub fn is_zero_sum(&self) -> bool {
        self.rake.is_free() && self.utility.is_zero_sum_affine()
    }

    pub fn bake(&self, t: &TerminalDescriptor) -> BakedPayoffs {
        let baseline = self.utility.baseline(t);
        let net_pot = t.pot.as_f64() - self.rake.rake(t);
        let outcome_utility = |won: PerPlayer<f64>| -> PerPlayer<f64> {
            let stacks_after = PerPlayer::new(
                t.stacks_before[Player::P0].as_f64() - t.contrib[Player::P0].as_f64()
                    + won[Player::P0],
                t.stacks_before[Player::P1].as_f64() - t.contrib[Player::P1].as_f64()
                    + won[Player::P1],
            );
            let u = self.utility.utility(&stacks_after);
            PerPlayer::new(
                u[Player::P0] - baseline[Player::P0],
                u[Player::P1] - baseline[Player::P1],
            )
        };
        let p0_wins = PerPlayer::new(net_pot, 0.0);
        let p1_wins = PerPlayer::new(0.0, net_pot);
        match t.kind {
            TerminalKind::Fold { folder } => {
                let u = outcome_utility(match folder {
                    Player::P0 => p1_wins,
                    Player::P1 => p0_wins,
                });
                BakedPayoffs {
                    win_p0: u,
                    tie: u,
                    win_p1: u,
                }
            }
            TerminalKind::Showdown => BakedPayoffs {
                win_p0: outcome_utility(p0_wins),
                tie: outcome_utility(self.rake.tie_split(net_pot)),
                win_p1: outcome_utility(p1_wins),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn showdown_terminal(pot: u32, contrib: (u32, u32)) -> TerminalDescriptor {
        TerminalDescriptor {
            kind: TerminalKind::Showdown,
            street: Street::River,
            pot: Chips(pot),
            contrib: PerPlayer::new(Chips(contrib.0), Chips(contrib.1)),
            stacks_before: PerPlayer::new(Chips(100), Chips(100)),
        }
    }

    #[test]
    fn showdown_computes_rake_once_without_changing_payoff_bits() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountingRake(AtomicUsize);
        impl RakeModel for CountingRake {
            fn rake(&self, _t: &TerminalDescriptor) -> f64 {
                self.0.fetch_add(1, Ordering::Relaxed);
                0.7
            }
        }

        let rake = CountingRake(AtomicUsize::new(0));
        let terminal = showdown_terminal(21, (10, 11));
        let pipeline = PayoffPipeline {
            rake: &rake,
            utility: &ChipEv,
        };
        let baked = pipeline.bake(&terminal);
        assert_eq!(rake.0.load(Ordering::Relaxed), 1);
        for (outcome, shares) in [
            (Outcome::WinP0, PerPlayer::new(1.0, 0.0)),
            (Outcome::Tie, PerPlayer::new(0.5, 0.5)),
            (Outcome::WinP1, PerPlayer::new(0.0, 1.0)),
        ] {
            for player in Player::BOTH {
                let expected = (terminal.stacks_before[player].as_f64()
                    - terminal.contrib[player].as_f64()
                    + shares[player] * (terminal.pot.as_f64() - 0.7))
                    - terminal.stacks_before[player].as_f64();
                assert_eq!(
                    baked.for_outcome(outcome)[player].to_bits(),
                    expected.to_bits()
                );
            }
        }
    }

    #[test]
    fn chip_ev_is_zero_sum() {
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let baked = pipeline.bake(&showdown_terminal(20, (10, 10)));
        for u in [baked.win_p0, baked.tie, baked.win_p1] {
            assert!((u[Player::P0] + u[Player::P1]).abs() < 1e-12);
        }
        assert_eq!(baked.win_p0[Player::P0], 10.0);
        assert_eq!(baked.tie[Player::P0], 0.0);
    }

    #[test]
    fn rake_breaks_zero_sum() {
        let pipeline = PayoffPipeline {
            rake: &PercentCapRake {
                rate: 0.05,
                cap: 3.0,
                no_flop_no_drop: true,
            },
            utility: &ChipEv,
        };
        let baked = pipeline.bake(&showdown_terminal(20, (10, 10)));
        // Winner nets pot/2 - rake share; total leaks exactly the rake.
        assert!((baked.win_p0[Player::P0] + baked.win_p0[Player::P1] + 1.0).abs() < 1e-12);
        // No flop, no drop.
        let mut preflop = showdown_terminal(20, (10, 10));
        preflop.street = Street::Preflop;
        let baked = pipeline.bake(&preflop);
        assert_eq!(baked.win_p0[Player::P0], 10.0);
    }

    #[test]
    fn hu_icm_is_affine_in_chip_ev() {
        // With two players, ICM utilities are an affine map of chip EV, so
        // baked payoff *differences* are proportional across models.
        let icm = Icm {
            payouts: [100.0, 60.0],
        };
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &icm,
        };
        let baked = pipeline.bake(&showdown_terminal(20, (10, 10)));
        let slope = (100.0 - 60.0) / 200.0;
        assert!((baked.win_p0[Player::P0] - 10.0 * slope).abs() < 1e-12);
        assert!((baked.win_p1[Player::P0] + 10.0 * slope).abs() < 1e-12);
    }

    #[test]
    fn is_zero_sum_matches_free_rake_and_affine_utility() {
        let icm = Icm {
            payouts: [100.0, 60.0],
        };
        let percent_cap = PercentCapRake {
            rate: 0.05,
            cap: 3.0,
            no_flop_no_drop: false,
        };

        assert!(
            PayoffPipeline {
                rake: &NoRake,
                utility: &ChipEv,
            }
            .is_zero_sum(),
            "no rake + chip EV must be zero-sum"
        );
        assert!(
            !PayoffPipeline {
                rake: &percent_cap,
                utility: &ChipEv,
            }
            .is_zero_sum(),
            "a rake that takes chips breaks zero-sum"
        );
        assert!(
            PayoffPipeline {
                rake: &NoRake,
                utility: &icm,
            }
            .is_zero_sum(),
            "HU ICM is affine, so an unraked ICM game is still zero-sum"
        );
    }

    #[test]
    fn icm_conserves_prize_pool_across_stack_splits() {
        // For any stack split, the two ICM utilities must sum to
        // payouts[0] + payouts[1]: the prize pool is fully distributed
        // between exactly two remaining players.
        let icm = Icm {
            payouts: [100.0, 60.0],
        };
        for (s0, s1) in [
            (1.0, 199.0),
            (50.0, 150.0),
            (100.0, 100.0),
            (150.0, 50.0),
            (199.0, 1.0),
        ] {
            let u = icm.utility(&PerPlayer::new(s0, s1));
            let total = u[Player::P0] + u[Player::P1];
            assert!(
                (total - 160.0).abs() < 1e-9,
                "stacks ({s0}, {s1}): utilities summed to {total}, want 160.0"
            );
        }
    }
}
