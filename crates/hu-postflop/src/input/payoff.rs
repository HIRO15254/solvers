//! Build-time adapters for the common input. Builder amounts are milli-BB;
//! utilities are BB for cash and prize units for tournaments.
use super::invalid;
use crate::game::{PayoffPipeline, RakeModel, TerminalDescriptor, TerminalKind, UtilityModel};
use economics::{CompiledRake, UtilityConfig, estimate_icm};
use nlh::settlement::{PotKind, PotLayer, PotRake, RakeConditionContext};
use nlh::{MwChips, PerPlayer, Player, SeatMask};
use spot::{Product, Spot, SpotError};
use std::collections::HashMap;
use std::sync::Mutex;

/// Models bound to one validated P1 spot. Use with the config returned by `lower`.
pub struct NlhPayoff {
    pub rake: NlhRake,
    pub utility: NlhUtility,
}
impl NlhPayoff {
    pub fn new(spot: &Spot) -> Result<Self, SpotError> {
        if spot.product != Product::HuPostflop {
            return Err(invalid("spot", "P1 payoff requires a HU postflop spot"));
        }
        let oop = spot
            .context
            .oop
            .as_ref()
            .ok_or_else(|| invalid("spot", "missing OOP"))?;
        let ip = spot
            .context
            .ip
            .as_ref()
            .ok_or_else(|| invalid("spot", "missing IP"))?;
        let seats = PerPlayer::new(oop.seat, ip.seat);
        let remaining = seats.map(|id| spot.context.seats[id.index()].remaining_stack.0 as f64);
        let contributions =
            seats.map(|id| spot.context.seats[id.index()].total_contribution.0 as f64);
        let pot = spot.context.pot.0;
        let shares = PerPlayer::new(pot / 2, pot - pot / 2);
        let effective = spot
            .context
            .effective_stack
            .ok_or_else(|| invalid("spot", "missing effective stack"))?
            .0 as f64;
        let utility = match &spot.economics.utility {
            UtilityConfig::ChipEv => Utility::ChipEv,
            UtilityConfig::TournamentIcm {
                outside_field,
                payouts,
                samples,
                seed,
            } => {
                // OOP/IP first, then folded table seats in table order, then the
                // outside field. This also preserves legacy HU sampler ordering
                // when folded stacks are supplied as its outside field.
                let mut fixed: Vec<MwChips> = spot
                    .context
                    .seats
                    .iter()
                    .filter(|seat| seat.folded)
                    .map(|seat| seat.starting_stack - seat.total_contribution)
                    .collect();
                for player in outside_field {
                    fixed.push(
                        MwChips::try_from_bb(player.stack_bb)
                            .map_err(|e| invalid("economics.outside_field_bb", e.to_string()))?,
                    );
                }
                Utility::Icm {
                    fixed,
                    payouts: payouts.clone(),
                    samples: *samples,
                    seed: *seed,
                    memo: Mutex::new(HashMap::new()),
                }
            }
        };
        // Actual shares need not exhaust the pot: folded players supply dead
        // money. In that case even free affine utilities are general-sum.
        let zero_sum = contributions[Player::P0] + contributions[Player::P1] == pot as f64
            && match &utility {
                Utility::ChipEv => true,
                Utility::Icm { fixed, .. } => fixed.is_empty(),
            };
        let utility = NlhUtility {
            remaining,
            contributions,
            effective,
            utility,
            zero_sum,
        };
        // Validate all ICM baseline stack vectors before tree allocation.
        utility.try_values(remaining)?;
        utility.try_values(zip(remaining, contributions, |a, b| a + b))?;
        Ok(Self {
            rake: NlhRake {
                compiled: spot.economics.compiled_rake,
                shares,
                players_dealt: spot.table.positions.len() as u8,
            },
            utility,
        })
    }

    pub fn pipeline(&self) -> PayoffPipeline<'_> {
        PayoffPipeline {
            rake: &self.rake,
            utility: &self.utility,
        }
    }

    /// Add this to solver EV (including hand EV) to report from subgame start.
    pub fn ev_offset(&self) -> PerPlayer<f64> {
        let before = self.utility.values(zip(
            self.utility.remaining,
            self.utility.contributions,
            |a, b| a + b,
        ));
        let behind = self.utility.values(self.utility.remaining);
        zip(before, behind, |a, b| a - b)
    }
}

/// Shared economics rake on the matched pot, rounded on the 0.001 BB grid.
pub struct NlhRake {
    compiled: CompiledRake,
    shares: PerPlayer<u64>,
    players_dealt: u8,
}
impl RakeModel for NlhRake {
    fn rake(&self, t: &TerminalDescriptor) -> f64 {
        let bets = PerPlayer::new(
            t.contrib[Player::P0].0 as u64 - self.shares[Player::P0],
            t.contrib[Player::P1].0 as u64 - self.shares[Player::P1],
        );
        let refund = match t.kind {
            TerminalKind::Fold { .. } => bets[Player::P0].abs_diff(bets[Player::P1]),
            TerminalKind::Showdown => 0,
        };
        let gross = MwChips(t.pot.0 as u64 - refund);
        let mut pots = [PotLayer {
            kind: PotKind::Common,
            gross,
            rake: MwChips::ZERO,
            net: gross,
            contributors: SeatMask::EMPTY,
            eligible: SeatMask::EMPTY,
        }];
        self.compiled
            .apply(
                &mut pots,
                RakeConditionContext {
                    flop_dealt: true,
                    showdown: matches!(t.kind, TerminalKind::Showdown),
                    players_dealt: self.players_dealt,
                    players_saw_flop: 2,
                },
            )
            .expect("validated rake on the matched P1 pot")
            .0 as f64
    }
    fn is_free(&self) -> bool {
        matches!(
            self.compiled,
            CompiledRake::None | CompiledRake::Generic { rate: 0.0, .. }
        )
    }
}

enum Utility {
    ChipEv,
    Icm {
        fixed: Vec<MwChips>,
        payouts: Vec<f64>,
        samples: u64,
        seed: u64,
        memo: Mutex<HashMap<(u64, u64), PerPlayer<f64>>>,
    },
}

/// Translates the effective-stack builder's stacks into the actual stacks.
/// Its baseline credits each actor's actual contribution, leaving folded
/// players at their final stacks in every utility evaluation.
pub struct NlhUtility {
    remaining: PerPlayer<f64>,
    contributions: PerPlayer<f64>,
    effective: f64,
    utility: Utility,
    zero_sum: bool,
}
impl NlhUtility {
    fn try_values(&self, actual: PerPlayer<f64>) -> Result<PerPlayer<f64>, SpotError> {
        match &self.utility {
            Utility::ChipEv => Ok(actual.map(|v| v / 1000.0)),
            Utility::Icm {
                fixed,
                payouts,
                samples,
                seed,
                memo,
            } => {
                // Showdown ties can leave half a milli-BB. As in the legacy
                // tournament adapter, round stacks to the shared ICM grid.
                let pair = actual.map(|v| MwChips::try_from_bb(v / 1000.0));
                let a = pair[Player::P0]
                    .as_ref()
                    .map_err(|e| invalid("economics", e.to_string()))?;
                let b = pair[Player::P1]
                    .as_ref()
                    .map_err(|e| invalid("economics", e.to_string()))?;
                let key = (a.0, b.0);
                if let Some(value) = memo.lock().expect("ICM cache").get(&key) {
                    return Ok(*value);
                }
                let mut stacks = vec![*a, *b];
                stacks.extend_from_slice(fixed);
                let estimate = estimate_icm(&stacks, payouts, *samples, *seed)
                    .map_err(|e| invalid("economics", e.to_string()))?;
                let value = PerPlayer::new(estimate.values[0], estimate.values[1]);
                memo.lock().expect("ICM cache").insert(key, value);
                Ok(value)
            }
        }
    }
    fn values(&self, actual: PerPlayer<f64>) -> PerPlayer<f64> {
        self.try_values(actual)
            .expect("validated P1 terminal stacks")
    }
}
impl UtilityModel for NlhUtility {
    fn utility(&self, synthetic: &PerPlayer<f64>) -> PerPlayer<f64> {
        self.values(zip(*synthetic, self.remaining, |s, remaining| {
            remaining + s - self.effective
        }))
    }
    fn baseline(&self, _t: &TerminalDescriptor) -> PerPlayer<f64> {
        self.values(zip(self.remaining, self.contributions, |a, b| a + b))
    }
    fn is_zero_sum_affine(&self) -> bool {
        self.zero_sum
    }
}

fn zip(a: PerPlayer<f64>, b: PerPlayer<f64>, f: impl Fn(f64, f64) -> f64) -> PerPlayer<f64> {
    PerPlayer::new(
        f(a[Player::P0], b[Player::P0]),
        f(a[Player::P1], b[Player::P1]),
    )
}
