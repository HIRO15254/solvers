//! P2 rake implementation and compatibility exports for shared NLH settlement.
use crate::config::{CompiledRake, RakeAllocation, RakeRounding};
use crate::types::MwChips;
pub use nlh::settlement::*;

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
            CompiledRake::PercentCap {
                rate,
                cap,
                no_flop_no_drop,
            } => {
                if no_flop_no_drop && !rake_context.flop_dealt {
                    MwChips::ZERO
                } else {
                    percentage(gross, rate).min(cap)
                }
            }
            CompiledRake::Generic {
                rate,
                cap,
                when,
                rounding,
                ..
            } => {
                if !when.matches(rake_context) {
                    MwChips::ZERO
                } else {
                    let value = percentage_with_rounding(gross, rate, rounding);
                    cap.map_or(value, |cap| value.min(cap))
                }
            }
            CompiledRake::GgPreflop {
                rate,
                cap,
                exempt_pot,
            } => {
                if gross <= exempt_pot {
                    MwChips::ZERO
                } else {
                    percentage(gross, rate).min(cap)
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

fn percentage(amount: MwChips, rate: f64) -> MwChips {
    MwChips((amount.raw() as f64 * rate).floor() as u64)
}

fn percentage_with_rounding(amount: MwChips, rate: f64, rounding: RakeRounding) -> MwChips {
    let exact = amount.raw() as f64 * rate;
    let rounded = match rounding {
        RakeRounding::Down => exact.floor(),
        RakeRounding::Nearest => exact.round(),
        RakeRounding::Up => exact.ceil(),
    };
    MwChips(rounded as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::betting::{BettingState, HandPhase, SeatState, SeatStatus};
    use crate::types::{SeatId, SeatMask, SeatVec, Street};
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
            preflop_participants: SeatMask::EMPTY,
            preflop_open_cold_calls: 0,
        }
    }

    #[test]
    fn rake_is_capped_and_conserved() {
        let state = manual(&[100, 100], &[0, 0], &[SeatStatus::AllIn; 2]);
        let ranks = SeatVec::try_new(vec![Some(2), Some(1)]).unwrap();
        let settled = settle_ranked(
            &state,
            ranks,
            CompiledRake::PercentCap {
                rate: 0.10,
                cap: MwChips(15),
                no_flop_no_drop: false,
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
                rounding: RakeRounding::Down,
            },
        )
        .unwrap();
        assert_eq!(rated.pots.iter().map(|pot| pot.rake.raw()).sum::<u64>(), 12);
        assert_eq!(rated.pots[0].rake, MwChips(12));
        assert!(rated.pots[1..].iter().all(|pot| pot.rake == MwChips::ZERO));
        assert_eq!(
            percentage_with_rounding(MwChips(15), 0.1, RakeRounding::Nearest),
            MwChips(2)
        );
        assert_eq!(
            percentage_with_rounding(MwChips(11), 0.1, RakeRounding::Up),
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
            CompiledRake::PercentCap {
                rate: 0.10,
                cap: MwChips(100),
                no_flop_no_drop: true,
            },
        )
        .unwrap();
        assert_eq!(settled.total_rake, MwChips::ZERO);
        assert_eq!(settled.final_stacks[SeatId(0)], MwChips(200));
    }
}
