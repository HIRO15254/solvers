//! Exact chip settlement, including refunds, side pots, rake, and odd chips.

use cards::{Card, CardSet, rank_of};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::betting::{BettingState, HandPhase, SeatStatus};
use crate::config::CompiledRake;
use crate::types::{MwChips, SeatId, SeatMask, SeatVec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PotKind {
    Individual { lower: MwChips, upper: MwChips },
    Common,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PotLayer {
    pub kind: PotKind,
    pub gross: MwChips,
    pub rake: MwChips,
    pub net: MwChips,
    pub contributors: SeatMask,
    pub eligible: SeatMask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PotConstruction {
    pub pots: Vec<PotLayer>,
    pub refunds: SeatVec<MwChips>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    pub pots: Vec<PotLayer>,
    pub refunds: SeatVec<MwChips>,
    pub awards: SeatVec<MwChips>,
    pub final_stacks: SeatVec<MwChips>,
    pub total_rake: MwChips,
    /// Comparable exact seven-card ranks; folded seats are `None`.
    pub ranks: SeatVec<Option<u16>>,
}

/// Builds contribution layers before rake.  Eligibility-cap breakpoints are
/// included in addition to contribution breakpoints because a BBA can make a
/// short big blind eligible for part of a one-contributor layer.
pub fn build_pots(state: &BettingState) -> Result<PotConstruction, SettlementError> {
    let mut boundaries = Vec::with_capacity(state.num_seats() * 2);
    for seat in state.seats.iter() {
        let individual = seat.individual_committed();
        let eligibility = seat.eligibility_cap();
        if individual > MwChips::ZERO {
            boundaries.push(individual);
        }
        if eligibility > MwChips::ZERO {
            boundaries.push(eligibility);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut pots = Vec::new();
    let mut refunds = SeatVec::new_unchecked(vec![MwChips::ZERO; state.num_seats()]);
    let mut lower = MwChips::ZERO;
    for upper in boundaries {
        if upper <= lower {
            continue;
        }
        let mut contributors = SeatMask::EMPTY;
        let mut eligible = SeatMask::EMPTY;
        for seat in state.seats.seats() {
            if state.seats[seat].individual_committed() >= upper {
                contributors.insert(seat);
            }
            if state.seats[seat].status != SeatStatus::Folded
                && state.seats[seat].eligibility_cap() >= upper
            {
                eligible.insert(seat);
            }
        }
        if contributors.is_empty() {
            lower = upper;
            continue;
        }
        let width = upper
            .checked_sub(lower)
            .ok_or(SettlementError::ChipInvariant)?;
        let gross = multiply(width, contributors.len())?;
        if contributors.len() == 1 {
            let contributor = contributors.iter().next().expect("one contributor");
            let opponents = eligible.difference(SeatMask::from_seat(contributor));
            if opponents.is_empty() {
                refunds[contributor] = refunds[contributor]
                    .checked_add(gross)
                    .ok_or(SettlementError::ChipInvariant)?;
                lower = upper;
                continue;
            }
        }
        if eligible.is_empty() {
            return Err(SettlementError::NoEligiblePlayer);
        }
        pots.push(PotLayer {
            kind: PotKind::Individual { lower, upper },
            gross,
            rake: MwChips::ZERO,
            net: gross,
            contributors,
            eligible,
        });
        lower = upper;
    }

    let common = state
        .seats
        .iter()
        .map(|seat| seat.common_committed)
        .fold(MwChips::ZERO, |sum, value| sum + value);
    if common > MwChips::ZERO {
        let eligible = state.non_folded_mask();
        if eligible.is_empty() {
            return Err(SettlementError::NoEligiblePlayer);
        }
        let contributors = state.seats.seats().fold(SeatMask::EMPTY, |mut mask, seat| {
            if state.seats[seat].common_committed > MwChips::ZERO {
                mask.insert(seat);
            }
            mask
        });
        pots.insert(
            0,
            PotLayer {
                kind: PotKind::Common,
                gross: common,
                rake: MwChips::ZERO,
                net: common,
                contributors,
                eligible,
            },
        );
    }
    Ok(PotConstruction { pots, refunds })
}

/// Builds pot layers with rake applied, but computes no winners or awards.
///
/// This is the structural half of [`settle_with_winners`] -- the half that
/// depends only on the betting line (contributions, eligibility caps, rake)
/// and never on any hole card. Callers that need to settle many hypothetical
/// hands sharing one betting line (the vector-traverser terminal fast path in
/// `crate::holdem`) call this once and reuse the resulting pot amounts and
/// eligible-seat masks across every hypothetical hand, instead of paying for
/// this construction again on every call to [`settle_ranked`].
pub(crate) fn build_rated_pots(
    state: &BettingState,
    rake: CompiledRake,
) -> Result<PotConstruction, SettlementError> {
    let mut construction = build_pots(state)?;
    apply_rake(&mut construction.pots, rake, state.flop_dealt)?;
    Ok(construction)
}

pub fn settle_uncontested(
    state: &BettingState,
    rake: CompiledRake,
) -> Result<Settlement, SettlementError> {
    let HandPhase::Uncontested { winner } = state.phase else {
        return Err(SettlementError::WrongPhase);
    };
    let ranks = SeatVec::new_unchecked(vec![None; state.num_seats()]);
    settle_with_winners(state, rake, ranks, |pot, _| {
        if !pot.eligible.contains(winner) {
            return Err(SettlementError::WinnerIneligible(winner));
        }
        Ok(SeatMask::from_seat(winner))
    })
}

/// Evaluates every live seven-card hand exactly and settles every side pot
/// independently.  All supplied cards are checked for duplication.
pub fn settle_showdown(
    state: &BettingState,
    board: [Card; 5],
    holes: &SeatVec<Option<[Card; 2]>>,
    rake: CompiledRake,
) -> Result<Settlement, SettlementError> {
    if !matches!(state.phase, HandPhase::Showdown | HandPhase::Runout) {
        return Err(SettlementError::WrongPhase);
    }
    if holes.len() != state.num_seats() {
        return Err(SettlementError::SeatCount);
    }
    let mut used = CardSet::EMPTY;
    for card in board {
        insert_unique(&mut used, card)?;
    }
    for cards in holes.iter().flatten() {
        insert_unique(&mut used, cards[0])?;
        insert_unique(&mut used, cards[1])?;
    }
    let mut ranks = Vec::with_capacity(state.num_seats());
    for seat in state.seats.seats() {
        if state.seats[seat].status == SeatStatus::Folded {
            ranks.push(None);
            continue;
        }
        let [first, second] = holes[seat].ok_or(SettlementError::MissingHoleCards(seat))?;
        ranks.push(Some(rank_of(board.into_iter().chain([first, second])).0));
    }
    settle_ranked(state, SeatVec::new_unchecked(ranks), rake)
}

/// Settles already-computed exact ranks.  This is useful to callers that
/// cache river rank tables and avoids evaluating cards twice.
pub fn settle_ranked(
    state: &BettingState,
    ranks: SeatVec<Option<u16>>,
    rake: CompiledRake,
) -> Result<Settlement, SettlementError> {
    if !matches!(state.phase, HandPhase::Showdown | HandPhase::Runout) {
        return Err(SettlementError::WrongPhase);
    }
    if ranks.len() != state.num_seats() {
        return Err(SettlementError::SeatCount);
    }
    for seat in state.non_folded_mask() {
        if ranks[seat].is_none() {
            return Err(SettlementError::MissingRank(seat));
        }
    }
    let winner_ranks = ranks.clone();
    settle_with_winners(state, rake, ranks, move |pot, _| {
        let best = pot
            .eligible
            .iter()
            .filter_map(|seat| winner_ranks[seat])
            .max()
            .ok_or(SettlementError::NoEligiblePlayer)?;
        let mut winners = SeatMask::EMPTY;
        for seat in pot.eligible {
            if winner_ranks[seat] == Some(best) {
                winners.insert(seat);
            }
        }
        Ok(winners)
    })
}

fn settle_with_winners(
    state: &BettingState,
    rake: CompiledRake,
    ranks: SeatVec<Option<u16>>,
    mut winners_for: impl FnMut(&PotLayer, usize) -> Result<SeatMask, SettlementError>,
) -> Result<Settlement, SettlementError> {
    let PotConstruction { mut pots, refunds } = build_pots(state)?;
    let total_rake = apply_rake(&mut pots, rake, state.flop_dealt)?;
    let mut awards = SeatVec::new_unchecked(vec![MwChips::ZERO; state.num_seats()]);
    for (index, pot) in pots.iter().enumerate() {
        let winners = winners_for(pot, index)?;
        if winners.is_empty() || !winners.difference(pot.eligible).is_empty() {
            return Err(SettlementError::NoEligiblePlayer);
        }
        award_split(&mut awards, pot.net, winners, state.button)?;
    }
    let final_stacks = SeatVec::new_unchecked(
        state
            .seats
            .seats()
            .map(|seat| state.seats[seat].remaining + refunds[seat] + awards[seat])
            .collect(),
    );
    let initial = state
        .seats
        .iter()
        .map(|seat| seat.starting_stack)
        .fold(MwChips::ZERO, |sum, value| sum + value);
    let final_total = final_stacks
        .iter()
        .copied()
        .fold(total_rake, |sum, value| sum + value);
    if initial != final_total {
        return Err(SettlementError::Conservation {
            initial,
            final_total,
        });
    }
    Ok(Settlement {
        pots,
        refunds,
        awards,
        final_stacks,
        total_rake,
        ranks,
    })
}

fn apply_rake(
    pots: &mut [PotLayer],
    rake: CompiledRake,
    flop_dealt: bool,
) -> Result<MwChips, SettlementError> {
    let gross = pots
        .iter()
        .map(|pot| pot.gross)
        .fold(MwChips::ZERO, |sum, value| sum + value);
    let total = match rake {
        CompiledRake::None => MwChips::ZERO,
        CompiledRake::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => {
            if no_flop_no_drop && !flop_dealt {
                MwChips::ZERO
            } else {
                percentage(gross, rate).min(cap)
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

fn award_split(
    awards: &mut SeatVec<MwChips>,
    amount: MwChips,
    winners: SeatMask,
    button: SeatId,
) -> Result<(), SettlementError> {
    let count = winners.len() as u64;
    if count == 0 {
        return Err(SettlementError::NoEligiblePlayer);
    }
    let share = amount.raw() / count;
    let odd = (amount.raw() % count) as usize;
    for seat in winners {
        awards[seat] = awards[seat]
            .checked_add(MwChips(share))
            .ok_or(SettlementError::ChipInvariant)?;
    }
    let num_seats = awards.len();
    let ordered = (1..=num_seats)
        .map(|step| button.advance(step, num_seats))
        .filter(|seat| winners.contains(*seat));
    for seat in ordered.take(odd) {
        awards[seat] += MwChips(1);
    }
    Ok(())
}

fn percentage(amount: MwChips, rate: f64) -> MwChips {
    MwChips((amount.raw() as f64 * rate).floor() as u64)
}

fn multiply(amount: MwChips, count: usize) -> Result<MwChips, SettlementError> {
    amount
        .raw()
        .checked_mul(count as u64)
        .map(MwChips)
        .ok_or(SettlementError::ChipInvariant)
}

fn insert_unique(set: &mut CardSet, card: Card) -> Result<(), SettlementError> {
    if set.contains(card) {
        return Err(SettlementError::DuplicateCard(card.to_string()));
    }
    set.insert(card);
    Ok(())
}

#[derive(Debug, Error)]
pub enum SettlementError {
    #[error("settlement called in the wrong hand phase")]
    WrongPhase,
    #[error("seat-vector length does not match the betting state")]
    SeatCount,
    #[error("no eligible player exists for a contested pot")]
    NoEligiblePlayer,
    #[error("uncontested winner seat {0} is not eligible for a pot")]
    WinnerIneligible(SeatId),
    #[error("missing hole cards for live seat {0}")]
    MissingHoleCards(SeatId),
    #[error("missing hand rank for live seat {0}")]
    MissingRank(SeatId),
    #[error("duplicate card {0}")]
    DuplicateCard(String),
    #[error("settlement chip invariant was violated")]
    ChipInvariant,
    #[error("chip conservation failed: initial {initial}, final plus rake {final_total}")]
    Conservation {
        initial: MwChips,
        final_total: MwChips,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::betting::SeatState;
    use crate::types::Street;

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
        }
    }

    #[test]
    fn side_pots_and_unmatched_top_are_exact() {
        let state = manual(&[100, 60, 20], &[0, 0, 0], &[SeatStatus::AllIn; 3]);
        let built = build_pots(&state).unwrap();
        assert_eq!(
            built
                .pots
                .iter()
                .map(|pot| pot.gross.raw())
                .collect::<Vec<_>>(),
            vec![60, 80]
        );
        assert_eq!(built.refunds[SeatId(0)], MwChips(40));
    }

    #[test]
    fn bba_is_common_main_pot_money_without_side_pot_credit() {
        let state = manual(&[2_000, 500], &[0, 1_000], &[SeatStatus::AllIn; 2]);
        let built = build_pots(&state).unwrap();
        assert_eq!(
            built
                .pots
                .iter()
                .map(|pot| pot.gross.raw())
                .collect::<Vec<_>>(),
            vec![1_000, 1_000]
        );
        assert_eq!(built.refunds[SeatId(0)], MwChips(1_500));
        assert_eq!(built.pots[0].kind, PotKind::Common);
        assert_eq!(built.pots[0].eligible, SeatMask::all(2).unwrap());
    }

    #[test]
    fn ties_split_each_side_pot_and_odd_chip_starts_after_button() {
        let state = manual(&[101, 101, 21], &[0, 0, 0], &[SeatStatus::AllIn; 3]);
        let ranks = SeatVec::try_new(vec![Some(10), Some(10), Some(9)]).unwrap();
        let settled = settle_ranked(&state, ranks, CompiledRake::None).unwrap();
        assert_eq!(settled.awards[SeatId(0)], MwChips(111));
        assert_eq!(settled.awards[SeatId(1)], MwChips(112));
        assert_eq!(settled.awards[SeatId(2)], MwChips::ZERO);
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
    fn exact_cards_reject_duplicates() {
        let state = manual(&[100, 100], &[0, 0], &[SeatStatus::AllIn; 2]);
        let card = |text: &str| text.parse::<Card>().unwrap();
        let board = [card("As"), card("Ks"), card("Qs"), card("Js"), card("Ts")];
        let holes = SeatVec::try_new(vec![
            Some([card("2c"), card("2d")]),
            Some([card("As"), card("3d")]),
        ])
        .unwrap();
        assert!(matches!(
            settle_showdown(&state, board, &holes, CompiledRake::None),
            Err(SettlementError::DuplicateCard(_))
        ));
    }

    #[test]
    fn folded_money_remains_in_main_and_side_pots() {
        let state = manual(
            &[100, 100, 20],
            &[0, 0, 0],
            &[SeatStatus::Folded, SeatStatus::AllIn, SeatStatus::AllIn],
        );
        let ranks = SeatVec::try_new(vec![None, Some(5), Some(10)]).unwrap();
        let settled = settle_ranked(&state, ranks, CompiledRake::None).unwrap();
        assert_eq!(settled.awards[SeatId(0)], MwChips::ZERO);
        assert_eq!(settled.awards[SeatId(1)], MwChips(160));
        assert_eq!(settled.awards[SeatId(2)], MwChips(60));
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

    #[test]
    fn exact_showdown_ranks_live_hands() {
        let state = manual(&[100, 100], &[0, 0], &[SeatStatus::AllIn; 2]);
        let card = |text: &str| text.parse::<Card>().unwrap();
        let board = [card("2c"), card("3d"), card("4h"), card("9s"), card("Kc")];
        let holes = SeatVec::try_new(vec![
            Some([card("Ac"), card("Ad")]),
            Some([card("Qc"), card("Qd")]),
        ])
        .unwrap();
        let settled = settle_showdown(&state, board, &holes, CompiledRake::None).unwrap();
        assert!(settled.ranks[SeatId(0)] > settled.ranks[SeatId(1)]);
        assert_eq!(settled.final_stacks[SeatId(0)], MwChips(200));
        assert_eq!(settled.final_stacks[SeatId(1)], MwChips::ZERO);
    }
}
