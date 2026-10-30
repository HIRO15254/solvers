//! Build-time adapters for the common input. Builder amounts are chips;
//! utilities are BB for cash and prize units for tournaments.
use super::invalid;
use crate::game::{PayoffPipeline, RakeModel, TerminalDescriptor, TerminalKind, UtilityModel};
use economics::{CompiledRake, PreparedIcm, UtilityConfig};
use nlh::settlement::{PotKind, PotLayer, PotRake, RakeConditionContext};
use nlh::{ChipScale, MwChips, PerPlayer, Player, SeatMask, SeatVec, split_pot};
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
        let scale = spot.table.setup.scale();
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
            UtilityConfig::ChipEv => Utility::ChipEv(scale),
            UtilityConfig::TournamentIcm {
                outside_field,
                payouts,
            } => {
                // OOP/IP form the table; folded table seats keep their final
                // stacks alongside the outside field.
                let starting = SeatVec::new_unchecked(
                    [seats[Player::P0], seats[Player::P1]]
                        .iter()
                        .map(|id| spot.context.seats[id.index()].starting_stack)
                        .collect(),
                );
                let mut fixed: Vec<MwChips> = spot
                    .context
                    .seats
                    .iter()
                    .filter(|seat| seat.folded)
                    .map(|seat| seat.starting_stack - seat.total_contribution)
                    .collect();
                fixed.extend(
                    outside_field
                        .iter()
                        .map(|player| MwChips(player.stack_chips)),
                );
                let zero_sum_field = fixed.is_empty();
                Utility::Icm {
                    prepared: Box::new(
                        PreparedIcm::new(starting, fixed, payouts.clone())
                            .map_err(|e| invalid("economics", e.to_string()))?,
                    ),
                    zero_sum_field,
                    memo: Mutex::new(HashMap::new()),
                }
            }
        };
        // Actual shares need not exhaust the pot: folded players supply dead
        // money. In that case even free affine utilities are general-sum.
        let zero_sum = contributions[Player::P0] + contributions[Player::P1] == pot as f64
            && match &utility {
                Utility::ChipEv(_) => true,
                Utility::Icm { zero_sum_field, .. } => *zero_sum_field,
            };
        // Validate and cache the ICM baseline before tree allocation.
        utility.try_values(remaining)?;
        let baseline = utility.try_values(zip(remaining, contributions, |a, b| a + b))?;
        let utility = NlhUtility {
            remaining,
            baseline,
            effective,
            utility,
            zero_sum,
        };
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
        let before = self.utility.baseline;
        let behind = self.utility.values(self.utility.remaining);
        zip(before, behind, |a, b| a - b)
    }
}

/// Shared economics rake on the matched pot, rounded on its own unit; tied
/// pots split into whole chips.
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
    /// OOP (P0) is the first player clockwise from the button, so an odd
    /// unit goes to OOP.
    fn tie_split(&self, net_pot: f64) -> PerPlayer<f64> {
        let split = split_pot(MwChips(net_pot as u64), 2);
        PerPlayer::new(split.award(0).0 as f64, split.award(1).0 as f64)
    }
    fn is_free(&self) -> bool {
        matches!(
            self.compiled,
            CompiledRake::None | CompiledRake::Generic { rate: 0.0, .. }
        )
    }
}

enum Utility {
    ChipEv(ChipScale),
    Icm {
        prepared: Box<PreparedIcm>,
        /// No folded or off-table players: heads-up ICM is affine in chips.
        zero_sum_field: bool,
        memo: Mutex<HashMap<(u64, u64), PerPlayer<f64>>>,
    },
}

/// Translates the effective-stack builder's stacks into the actual stacks.
/// Its baseline credits each actor's actual contribution, leaving folded
/// players at their final stacks in every utility evaluation.
pub struct NlhUtility {
    remaining: PerPlayer<f64>,
    baseline: PerPlayer<f64>,
    effective: f64,
    utility: Utility,
    zero_sum: bool,
}
impl Utility {
    fn try_values(&self, actual: PerPlayer<f64>) -> Result<PerPlayer<f64>, SpotError> {
        match self {
            Utility::ChipEv(scale) => Ok(actual.map(|v| v / scale.chips_per_bb() as f64)),
            Utility::Icm { prepared, memo, .. } => {
                // Stacks are whole chips: rake and tied-pot awards are integral.
                let pair = actual.map(|v| {
                    if !v.is_finite() || v < 0.0 || v.fract() != 0.0 || v >= 2.0_f64.powi(64) {
                        Err(invalid(
                            "economics",
                            "terminal stack must be a whole number of chips",
                        ))
                    } else {
                        Ok(MwChips(v as u64))
                    }
                });
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
                let values = prepared
                    .terminal_values(&SeatVec::new_unchecked(vec![*a, *b]))
                    .map_err(|e| invalid("economics", e.to_string()))?;
                let value = PerPlayer::new(values.as_slice()[0], values.as_slice()[1]);
                memo.lock().expect("ICM cache").insert(key, value);
                Ok(value)
            }
        }
    }
}
impl NlhUtility {
    fn values(&self, actual: PerPlayer<f64>) -> PerPlayer<f64> {
        self.utility
            .try_values(actual)
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
        self.baseline
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
