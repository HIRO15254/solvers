use cards::{Chips, PerPlayer, Player, Street};

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

/// Rake taken from the pot at a terminal. Build-time only.
pub trait RakeModel: Send + Sync {
    /// Rake in fractional chips (cap rules produce fractions of a chip when
    /// pots are expressed in small units).
    fn rake(&self, t: &TerminalDescriptor) -> f64;
}

pub struct NoRake;

impl RakeModel for NoRake {
    fn rake(&self, _t: &TerminalDescriptor) -> f64 {
        0.0
    }
}

/// Percentage rake with a cap, optionally waived when the hand ends
/// preflop (no flop, no drop).
pub struct PercentCapRake {
    pub rate: f64,
    pub cap: f64,
    pub no_flop_no_drop: bool,
}

impl RakeModel for PercentCapRake {
    fn rake(&self, t: &TerminalDescriptor) -> f64 {
        if self.no_flop_no_drop && t.street == Street::Preflop {
            return 0.0;
        }
        (t.pot.as_f64() * self.rate).min(self.cap)
    }
}

/// GG-style preflop rake: charged even when the hand ends preflop, but only
/// once the pot has been raised beyond the blinds.
pub struct GgPreflopRake {
    pub rate: f64,
    pub cap: f64,
    /// Pot size at or below which no rake is taken (e.g. a walk).
    pub exempt_pot: Chips,
}

impl RakeModel for GgPreflopRake {
    fn rake(&self, t: &TerminalDescriptor) -> f64 {
        if t.pot <= self.exempt_pot {
            return 0.0;
        }
        (t.pot.as_f64() * self.rate).min(self.cap)
    }
}

/// Maps final stacks to utilities. Build-time only.
pub trait UtilityModel: Send + Sync {
    fn utility(&self, stacks_after: &PerPlayer<f64>) -> PerPlayer<f64>;

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
    pub fn bake(&self, t: &TerminalDescriptor) -> BakedPayoffs {
        let baseline = self.utility.utility(&t.stacks_before.map(|c| c.as_f64()));
        let outcome_utility = |share: PerPlayer<f64>| -> PerPlayer<f64> {
            let net_pot = t.pot.as_f64() - self.rake.rake(t);
            let stacks_after = PerPlayer::new(
                t.stacks_before[Player::P0].as_f64() - t.contrib[Player::P0].as_f64()
                    + share[Player::P0] * net_pot,
                t.stacks_before[Player::P1].as_f64() - t.contrib[Player::P1].as_f64()
                    + share[Player::P1] * net_pot,
            );
            let u = self.utility.utility(&stacks_after);
            PerPlayer::new(
                u[Player::P0] - baseline[Player::P0],
                u[Player::P1] - baseline[Player::P1],
            )
        };
        match t.kind {
            TerminalKind::Fold { folder } => {
                let share = match folder {
                    Player::P0 => PerPlayer::new(0.0, 1.0),
                    Player::P1 => PerPlayer::new(1.0, 0.0),
                };
                let u = outcome_utility(share);
                BakedPayoffs {
                    win_p0: u,
                    tie: u,
                    win_p1: u,
                }
            }
            TerminalKind::Showdown => BakedPayoffs {
                win_p0: outcome_utility(PerPlayer::new(1.0, 0.0)),
                tie: outcome_utility(PerPlayer::new(0.5, 0.5)),
                win_p1: outcome_utility(PerPlayer::new(0.0, 1.0)),
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
}
