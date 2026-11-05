//! Rake application, rounding, and allocation over shared NLH pot layers.
use crate::config::{CompiledRake, RakeAllocation};
use nlh::settlement::{PotLayer, PotRake, RakeConditionContext, SettlementError};
use nlh::{MwChips, Rounding};

impl PotRake for CompiledRake {
    fn apply(
        &self,
        pots: &mut [PotLayer],
        rake_context: RakeConditionContext,
    ) -> Result<MwChips, SettlementError> {
        let gross = pots
            .iter()
            .map(|pot| pot.gross)
            .fold(MwChips::ZERO, |sum, value| sum + value);
        let total = match *self {
            CompiledRake::None => MwChips::ZERO,
            CompiledRake::Generic {
                rate,
                cap,
                when,
                rounding,
                rounding_unit,
                ..
            } => {
                if !when.matches(rake_context) {
                    MwChips::ZERO
                } else {
                    // A coarse unit can round past the pot; the rake never exceeds it.
                    let value =
                        percentage_with_rounding(gross, rate, rounding, rounding_unit).min(gross);
                    cap.map_or(value, |cap| value.min(cap))
                }
            }
        };
        if total == MwChips::ZERO {
            return Ok(total);
        }
        if gross == MwChips::ZERO || total > gross {
            return Err(SettlementError::ChipInvariant);
        }

        if matches!(
            self,
            CompiledRake::Generic {
                allocation: RakeAllocation::MainFirst,
                ..
            }
        ) {
            let mut remaining = total;
            for pot in pots {
                let share = remaining.min(pot.gross);
                pot.rake = share;
                pot.net = pot.gross - share;
                remaining -= share;
            }
            if remaining != MwChips::ZERO {
                return Err(SettlementError::ChipInvariant);
            }
            return Ok(total);
        }

        let denominator = gross.raw() as u128;
        let mut allocated = 0u64;
        let mut remainders = Vec::with_capacity(pots.len());
        for (index, pot) in pots.iter_mut().enumerate() {
            let product = total.raw() as u128 * pot.gross.raw() as u128;
            let share = (product / denominator) as u64;
            let remainder = product % denominator;
            pot.rake = MwChips(share);
            pot.net = pot.gross - pot.rake;
            allocated = allocated
                .checked_add(share)
                .ok_or(SettlementError::ChipInvariant)?;
            remainders.push((remainder, index));
        }
        remainders.sort_unstable_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        for &(_, index) in remainders.iter().take((total.raw() - allocated) as usize) {
            pots[index].rake += MwChips(1);
            pots[index].net -= MwChips(1);
        }
        Ok(total)
    }
}

fn percentage_with_rounding(
    amount: MwChips,
    rate: f64,
    rounding: Rounding,
    unit: MwChips,
) -> MwChips {
    let rounded =
        nlh::decimal::rounded_product(amount.raw(), rate, unit.raw(), rounding).unwrap_or(u64::MAX);
    MwChips(rounded.saturating_mul(unit.raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nlh::betting::{BettingState, HandPhase, SeatState, SeatStatus};
    use nlh::settlement::{build_rated_pots, settle_ranked};
    use nlh::{SeatId, SeatMask, SeatVec, Street};

    #[test]
    fn decimal_rake_rounds_in_exact_units() {
        assert_eq!(
            percentage_with_rounding(MwChips(5500), 0.07, Rounding::Up, MwChips(1)),
            MwChips(385)
        );
        assert_eq!(
            percentage_with_rounding(MwChips(100_000), 0.07, Rounding::Up, MwChips(1000)),
            MwChips(7000)
        );
        assert_eq!(
            percentage_with_rounding(MwChips(200), 0.145, Rounding::Down, MwChips(1)),
            MwChips(29)
        );
        assert_eq!(
            percentage_with_rounding(
                MwChips(9_007_199_254_740_993),
                1.0,
                Rounding::Down,
                MwChips(1)
            ),
            MwChips(9_007_199_254_740_993)
        );
    }
    fn manual(individual: &[u64], common: &[u64], statuses: &[SeatStatus]) -> BettingState {
        let seats = individual
            .iter()
            .zip(common)
            .zip(statuses)
            .map(|((&individual, &common), &status)| SeatState {
                starting_stack: MwChips(individual + common),
                remaining: MwChips::ZERO,
                status,
                dead_committed: MwChips::ZERO,
                common_committed: MwChips(common),
                street_committed: [
                    MwChips(individual),
                    MwChips::ZERO,
                    MwChips::ZERO,
                    MwChips::ZERO,
                ],
                raise_reopen_at: None,
            })
            .collect();
        let non_folded = statuses
            .iter()
            .filter(|status| **status != SeatStatus::Folded)
            .count() as u8;
        BettingState {
            seats: SeatVec::try_new(seats).unwrap(),
            button: SeatId(0),
            small_blind_seat: SeatId(1),
            big_blind_seat: SeatId(2.min(individual.len() - 1) as u8),
            big_blind: MwChips(1_000),
            street: Street::River,
            street_active_players: [non_folded; 4],
            to_act: None,
            bet_to_match: MwChips::ZERO,
            last_full_raise: MwChips(1_000),
            full_wager_established: false,
            pending: SeatMask::EMPTY,
            aggressive_actions: 0,
            flop_dealt: true,
            phase: HandPhase::Showdown,
            preflop_voluntary_call_seen: false,
            preflop_limpers: 0,
            preflop_flats: 0,
            last_preflop_aggressor: None,
            last_street_aggressor: None,
            previous_street_aggressor: None,
            preflop_participants: SeatMask::EMPTY,
            preflop_open_cold_calls: 0,
            chip_rounding: nlh::Rounding::Nearest,
        }
    }

    #[test]
    fn rake_is_capped_and_conserved() {
        let state = manual(&[100, 100], &[0, 0], &[SeatStatus::AllIn; 2]);
        let ranks = SeatVec::try_new(vec![Some(2), Some(1)]).unwrap();
        let settled = settle_ranked(
            &state,
            ranks,
            CompiledRake::Generic {
                rate: 0.10,
                cap: Some(MwChips(15)),
                when: crate::rake_condition::compile("true").unwrap(),
                allocation: RakeAllocation::Proportional,
                rounding: Rounding::Down,
                rounding_unit: MwChips(1),
            },
        )
        .unwrap();
        assert_eq!(settled.total_rake, MwChips(15));
        assert_eq!(settled.final_stacks[SeatId(0)], MwChips(185));
    }

    #[test]
    fn generic_rake_honors_main_first_allocation_and_rounding() {
        let state = manual(&[100, 50, 20], &[0, 0, 0], &[SeatStatus::AllIn; 3]);
        let rated = build_rated_pots(
            &state,
            CompiledRake::Generic {
                rate: 0.1,
                cap: None,
                when: crate::rake_condition::compile("true").unwrap(),
                allocation: RakeAllocation::MainFirst,
                rounding: Rounding::Down,
                rounding_unit: MwChips(1),
            },
        )
        .unwrap();
        assert_eq!(rated.pots.iter().map(|pot| pot.rake.raw()).sum::<u64>(), 12);
        assert_eq!(rated.pots[0].rake, MwChips(12));
        assert!(rated.pots[1..].iter().all(|pot| pot.rake == MwChips::ZERO));
        assert_eq!(
            percentage_with_rounding(MwChips(15), 0.1, Rounding::Nearest, MwChips(1)),
            MwChips(2)
        );
        assert_eq!(
            percentage_with_rounding(MwChips(11), 0.1, Rounding::Up, MwChips(1)),
            MwChips(2)
        );
    }

    #[test]
    fn no_flop_no_drop_skips_rake() {
        let mut state = manual(&[100, 100], &[0, 0], &[SeatStatus::AllIn; 2]);
        state.flop_dealt = false;
        let ranks = SeatVec::try_new(vec![Some(2), Some(1)]).unwrap();
        let settled = settle_ranked(
            &state,
            ranks,
            CompiledRake::Generic {
                rate: 0.10,
                cap: Some(MwChips(100)),
                when: crate::rake_condition::compile("flop_dealt").unwrap(),
                allocation: RakeAllocation::Proportional,
                rounding: Rounding::Down,
                rounding_unit: MwChips(1),
            },
        )
        .unwrap();
        assert_eq!(settled.total_rake, MwChips::ZERO);
        assert_eq!(settled.final_stacks[SeatId(0)], MwChips(200));
    }

    #[test]
    fn half_bb_units_round_then_cap_without_rounding_the_cap() {
        for (pot, expected) in [
            (6000, [500, 500, 1000]),
            (7500, [500, 1000, 1000]),
            (10000, [1000, 1000, 1000]),
        ] {
            for (mode, expected) in [Rounding::Down, Rounding::Nearest, Rounding::Up]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(
                    percentage_with_rounding(MwChips(pot), 0.1, mode, MwChips(500)),
                    MwChips(expected)
                );
            }
        }
        let state = manual(&[5000, 5000], &[0, 0], &[SeatStatus::AllIn; 2]);
        let rated = build_rated_pots(
            &state,
            CompiledRake::Generic {
                rate: 0.1,
                cap: Some(MwChips(725)),
                when: crate::rake_condition::compile("true").unwrap(),
                allocation: RakeAllocation::MainFirst,
                rounding: Rounding::Up,
                rounding_unit: MwChips(500),
            },
        )
        .unwrap();
        assert_eq!(rated.pots[0].rake, MwChips(725));
        assert_eq!(rated.pots[0].net, MwChips(9275));
    }

    #[test]
    fn a_unit_larger_than_the_pot_takes_at_most_the_pot() {
        let state = manual(&[1000, 1000], &[0, 0], &[SeatStatus::AllIn; 2]);
        let rated = build_rated_pots(
            &state,
            CompiledRake::Generic {
                rate: 0.05,
                cap: None,
                when: crate::rake_condition::compile("true").unwrap(),
                allocation: RakeAllocation::MainFirst,
                rounding: Rounding::Up,
                rounding_unit: MwChips(10_000),
            },
        )
        .unwrap();
        assert_eq!(rated.pots[0].rake, MwChips(2000));
        assert_eq!(rated.pots[0].net, MwChips::ZERO);
    }
}
