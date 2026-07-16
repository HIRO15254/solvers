//! No-limit Hold'em betting state for two through nine fixed seats.
//!
//! Antes are posted before blinds.  Per-player antes are dead individual
//! money; a big-blind ante is common main-pot money and does not change the
//! poster's side-pot cap.  The state keeps an
//! absolute raise-reopen threshold for every player, which naturally handles
//! multiple short all-ins whose cumulative increase becomes a full raise.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{BettingConfig, SizeSpec, ValidatedAnte, ValidatedMultiwayConfig};
use crate::types::{MwChips, SeatId, SeatMask, SeatVec, Street};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SeatStatus {
    Active,
    Folded,
    AllIn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Fold,
    Check,
    Call {
        amount: MwChips,
        all_in: bool,
    },
    BetTo {
        to: MwChips,
        all_in: bool,
        full_raise: bool,
    },
    RaiseTo {
        to: MwChips,
        all_in: bool,
        full_raise: bool,
    },
}

impl Action {
    pub fn amount(&self) -> Option<MwChips> {
        match *self {
            Self::Fold | Self::Check => None,
            Self::Call { amount, .. } => Some(amount),
            Self::BetTo { to, .. } | Self::RaiseTo { to, .. } => Some(to),
        }
    }

    pub fn is_aggressive(&self) -> bool {
        matches!(self, Self::BetTo { .. } | Self::RaiseTo { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HandPhase {
    Betting,
    Runout,
    Showdown,
    Uncontested { winner: SeatId },
}

impl HandPhase {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Betting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatState {
    pub starting_stack: MwChips,
    pub remaining: MwChips,
    pub status: SeatStatus,
    /// Per-player antes and similar dead individual contributions.
    pub dead_committed: MwChips,
    /// Common contribution made by this seat, currently the big-blind ante.
    pub common_committed: MwChips,
    pub street_committed: [MwChips; 4],
    /// Absolute street wager that must be reached before this seat may raise
    /// again. `None` means the seat has not yet closed its raising rights.
    pub raise_reopen_at: Option<MwChips>,
}

impl SeatState {
    pub fn committed_on(&self, street: Street) -> MwChips {
        self.street_committed[street.index()]
    }

    pub fn individual_committed(&self) -> MwChips {
        self.street_committed
            .iter()
            .copied()
            .fold(self.dead_committed, |sum, value| sum + value)
    }

    pub fn total_committed(&self) -> MwChips {
        self.individual_committed() + self.common_committed
    }

    /// Individual contribution height this player may contest in side pots.
    /// A BBA is common dead money: it enters only the lowest main pot and
    /// does not extend the poster's side-pot eligibility.
    pub fn eligibility_cap(&self) -> MwChips {
        self.individual_committed()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BettingState {
    pub seats: SeatVec<SeatState>,
    pub button: SeatId,
    pub small_blind_seat: SeatId,
    pub big_blind_seat: SeatId,
    pub big_blind: MwChips,
    pub street: Street,
    /// Non-folded dealt-player count captured independently on each street.
    /// This is historical public state used by abstraction keys; later folds
    /// must not retroactively change an earlier street's bucket context.
    pub street_active_players: [u8; 4],
    pub to_act: Option<SeatId>,
    pub bet_to_match: MwChips,
    pub last_full_raise: MwChips,
    pub full_wager_established: bool,
    pub pending: SeatMask,
    pub aggressive_actions: u8,
    pub flop_dealt: bool,
    pub phase: HandPhase,
    /// Whether any seat has voluntarily called (as opposed to a forced blind
    /// post) while `street == Preflop`. This is the only thing betting
    /// history is read for: it decides whether preflop `isolate_sizes`
    /// (rather than `bet_sizes`) govern a raise. Blinds are posted directly
    /// in [`BettingState::new`] via `post`, never through [`Self::apply`], so
    /// they never set this flag.
    pub preflop_voluntary_call_seen: bool,
}

impl BettingState {
    pub fn new(config: &ValidatedMultiwayConfig) -> Result<Self, BettingError> {
        let num_seats = config.seats.len();
        let small_blind_seat = if num_seats == 2 {
            config.button
        } else {
            config.button.next(num_seats)
        };
        let big_blind_seat = small_blind_seat.next(num_seats);
        let mut seats = SeatVec::new_unchecked(
            config
                .seats
                .iter()
                .map(|seat| SeatState {
                    starting_stack: seat.starting_stack,
                    remaining: seat.starting_stack,
                    status: SeatStatus::Active,
                    dead_committed: MwChips::ZERO,
                    common_committed: MwChips::ZERO,
                    street_committed: [MwChips::ZERO; 4],
                    raise_reopen_at: None,
                })
                .collect(),
        );

        // Tournament and cash-game conventions both post antes before blinds.
        match config.ante {
            ValidatedAnte::None => {}
            ValidatedAnte::Each(amount) => {
                for seat in seats.seats().collect::<Vec<_>>() {
                    post(&mut seats[seat], amount, Contribution::Dead);
                }
            }
            ValidatedAnte::BigBlind(amount) => {
                post(&mut seats[big_blind_seat], amount, Contribution::Common);
            }
        }
        post(
            &mut seats[small_blind_seat],
            config.blinds.small,
            Contribution::Street(Street::Preflop),
        );
        post(
            &mut seats[big_blind_seat],
            config.blinds.big,
            Contribution::Street(Street::Preflop),
        );

        let mut state = Self {
            seats,
            button: config.button,
            small_blind_seat,
            big_blind_seat,
            big_blind: config.blinds.big,
            street: Street::Preflop,
            street_active_players: [num_seats as u8, 0, 0, 0],
            to_act: None,
            // A short forced big blind does not lower the nominal call price.
            bet_to_match: config.blinds.big,
            last_full_raise: config.blinds.big,
            full_wager_established: true,
            pending: SeatMask::EMPTY,
            aggressive_actions: 0,
            flop_dealt: false,
            phase: HandPhase::Betting,
            preflop_voluntary_call_seen: false,
        };
        state.pending = state.active_mask();
        state.finish_or_select(big_blind_seat);
        Ok(state)
    }

    pub fn num_seats(&self) -> usize {
        self.seats.len()
    }

    pub fn players_on_street(&self, street: Street) -> u8 {
        self.street_active_players[street.index()]
    }

    pub fn pot_size(&self) -> MwChips {
        self.seats
            .iter()
            .map(SeatState::total_committed)
            .fold(MwChips::ZERO, |sum, value| sum + value)
    }

    pub fn non_folded_mask(&self) -> SeatMask {
        let mut mask = SeatMask::EMPTY;
        for seat in self.seats.seats() {
            if self.seats[seat].status != SeatStatus::Folded {
                mask.insert(seat);
            }
        }
        mask
    }

    pub fn active_mask(&self) -> SeatMask {
        let mut mask = SeatMask::EMPTY;
        for seat in self.seats.seats() {
            if self.seats[seat].status == SeatStatus::Active {
                mask.insert(seat);
            }
        }
        mask
    }

    pub fn current_wager(&self, seat: SeatId) -> MwChips {
        self.seats[seat].committed_on(self.street)
    }

    pub fn amount_to_call(&self, seat: SeatId) -> MwChips {
        self.bet_to_match.saturating_sub(self.current_wager(seat))
    }

    pub fn legal_actions(&self, config: &BettingConfig) -> Result<Vec<Action>, BettingError> {
        if self.phase != HandPhase::Betting {
            return Ok(Vec::new());
        }
        let actor = self.to_act.ok_or(BettingError::MissingActor)?;
        if self.seats[actor].status != SeatStatus::Active {
            return Err(BettingError::InactiveActor(actor));
        }
        let street_config = config.for_street(self.street);
        let stack = self.seats[actor].remaining;
        let actor_wager = self.current_wager(actor);
        let to_call = self.amount_to_call(actor);
        let mut actions = Vec::new();

        if to_call == MwChips::ZERO {
            actions.push(Action::Check);
        } else {
            actions.push(Action::Fold);
            let unopened_limp = self.street == Street::Preflop
                && self.aggressive_actions == 0
                && self.bet_to_match == self.big_blind;
            if config.allow_limp || !unopened_limp {
                let amount = to_call.min(stack);
                actions.push(Action::Call {
                    amount,
                    all_in: amount == stack,
                });
            }
        }

        let maximum = actor_wager + stack;
        let raising_open = self.seats[actor]
            .raise_reopen_at
            .is_none_or(|threshold| self.bet_to_match >= threshold);
        let another_active = !self
            .active_mask()
            .difference(SeatMask::from_seat(actor))
            .is_empty();
        if maximum <= self.bet_to_match
            || !raising_open
            || !another_active
            || self.aggressive_actions >= street_config.max_aggressive_actions
        {
            return Ok(actions);
        }

        let minimum = self.minimum_full_target();
        let sizes = if self.aggressive_actions != 0 {
            &street_config.raise_sizes
        } else if self.street == Street::Preflop && self.preflop_voluntary_call_seen {
            street_config
                .isolate_sizes
                .as_ref()
                .unwrap_or(&street_config.bet_sizes)
        } else {
            &street_config.bet_sizes
        };
        let pot = self.pot_size();
        let pot_after_call = pot + to_call.min(stack);
        let called_to = actor_wager + to_call.min(stack);
        let mut targets = Vec::with_capacity(sizes.len() + 1);
        for size in sizes {
            let proposed = match *size {
                SizeSpec::ToBb { value } => scale(self.big_blind, value),
                SizeSpec::PotAfterCall { fraction } => called_to + scale(pot_after_call, fraction),
                SizeSpec::PreviousBetMultiple { factor } => scale(self.bet_to_match, factor),
                SizeSpec::MinRaise => minimum,
                SizeSpec::StackFraction { fraction } => scale(maximum, fraction),
            };
            let mut target = if proposed < minimum && maximum >= minimum {
                minimum
            } else {
                proposed.min(maximum)
            };
            // Raise-cap merge (HRC-style): a target that already reaches the
            // configured fraction of the effective stack collapses into the
            // all-in target instead of standing as its own sized action. This
            // merge applies even when `include_allin` is false, since it is
            // folding an already-proposed size into all-in rather than adding
            // a brand-new all-in action.
            if street_config
                .allin_threshold
                .is_some_and(|threshold| target >= scale(maximum, threshold))
            {
                target = maximum;
            }
            if target > self.bet_to_match {
                targets.push(target);
            }
        }
        if street_config.include_allin {
            targets.push(maximum);
        }
        targets.sort_unstable();
        targets.dedup();

        for target in targets {
            if target <= self.bet_to_match {
                continue;
            }
            let all_in = target == maximum;
            let full_raise = target >= minimum;
            // Sub-minimum voluntary raises are illegal; only a stack cap may
            // create a short raise.
            if !full_raise && !all_in {
                continue;
            }
            let action = if self.bet_to_match == MwChips::ZERO {
                Action::BetTo {
                    to: target,
                    all_in,
                    full_raise,
                }
            } else {
                Action::RaiseTo {
                    to: target,
                    all_in,
                    full_raise,
                }
            };
            actions.push(action);
        }
        Ok(actions)
    }

    pub fn apply(&mut self, action: Action, config: &BettingConfig) -> Result<(), BettingError> {
        let actions = self.legal_actions(config)?;
        self.apply_from_actions(action, &actions)
    }

    /// Applies `action`, checking legality against an already-expanded
    /// action list instead of recomputing [`Self::legal_actions`].
    ///
    /// `actions` must be exactly what `legal_actions` would return for this
    /// state (e.g. the value the MCCFR traverser already expanded once per
    /// node); passing a stale or unrelated list can accept an action that
    /// would otherwise be rejected.
    pub fn apply_from_actions(
        &mut self,
        action: Action,
        actions: &[Action],
    ) -> Result<(), BettingError> {
        let actor = self.to_act.ok_or(BettingError::MissingActor)?;
        if !actions.contains(&action) {
            return Err(BettingError::IllegalAction { actor, action });
        }
        match action {
            Action::Fold => {
                self.seats[actor].status = SeatStatus::Folded;
                self.pending.remove(actor);
            }
            Action::Check => {
                // A checker retains raising rights against a later short bet.
                self.seats[actor].raise_reopen_at = None;
                self.pending.remove(actor);
            }
            Action::Call { amount, .. } => {
                if self.street == Street::Preflop {
                    self.preflop_voluntary_call_seen = true;
                }
                self.pay_street(actor, amount)?;
                self.seats[actor].raise_reopen_at =
                    Some(saturating_add(self.bet_to_match, self.last_full_raise));
                self.pending.remove(actor);
            }
            Action::BetTo { to, full_raise, .. } | Action::RaiseTo { to, full_raise, .. } => {
                let previous = self.bet_to_match;
                let delta = to
                    .checked_sub(self.current_wager(actor))
                    .ok_or(BettingError::ChipInvariant)?;
                self.pay_street(actor, delta)?;
                self.bet_to_match = to;
                if full_raise {
                    self.last_full_raise = if self.full_wager_established {
                        to.checked_sub(previous)
                            .ok_or(BettingError::ChipInvariant)?
                    } else {
                        to
                    };
                    self.full_wager_established = true;
                }
                self.aggressive_actions = self.aggressive_actions.saturating_add(1);
                self.seats[actor].raise_reopen_at = Some(saturating_add(to, self.last_full_raise));
                self.pending = self.active_mask();
                self.pending.remove(actor);
            }
        }
        self.street_active_players[self.street.index()] = self.non_folded_mask().len() as u8;
        self.finish_or_select(actor);
        Ok(())
    }

    fn minimum_full_target(&self) -> MwChips {
        if self.full_wager_established {
            saturating_add(self.bet_to_match, self.last_full_raise)
        } else {
            self.big_blind
        }
    }

    fn pay_street(&mut self, seat: SeatId, amount: MwChips) -> Result<(), BettingError> {
        if amount > self.seats[seat].remaining {
            return Err(BettingError::ChipInvariant);
        }
        self.seats[seat].remaining -= amount;
        self.seats[seat].street_committed[self.street.index()] += amount;
        if self.seats[seat].remaining == MwChips::ZERO {
            self.seats[seat].status = SeatStatus::AllIn;
            self.pending.remove(seat);
        }
        Ok(())
    }

    fn finish_or_select(&mut self, after: SeatId) {
        let non_folded = self.non_folded_mask();
        if non_folded.len() == 1 {
            self.phase = HandPhase::Uncontested {
                winner: non_folded.iter().next().expect("one seat exists"),
            };
            self.to_act = None;
            self.pending = SeatMask::EMPTY;
            return;
        }
        self.pending = self.pending.intersection(self.active_mask());
        if self.active_mask().len() == 1 {
            let sole_active = self
                .active_mask()
                .iter()
                .next()
                .expect("one active seat exists");
            if self.amount_to_call(sole_active) == MwChips::ZERO {
                self.pending = SeatMask::EMPTY;
                self.end_betting_round();
                return;
            }
        }
        if self.pending.is_empty() {
            self.end_betting_round();
            return;
        }
        self.to_act = self.next_in_mask(after, self.pending);
    }

    fn end_betting_round(&mut self) {
        self.to_act = None;
        if self.street == Street::River {
            self.phase = HandPhase::Showdown;
            return;
        }
        if self.active_mask().len() <= 1 {
            self.phase = HandPhase::Runout;
            self.flop_dealt = true;
            return;
        }
        let players_entering_next_street = self.street_active_players[self.street.index()];
        self.street = self.street.next().expect("river handled above");
        self.street_active_players[self.street.index()] = players_entering_next_street;
        if self.street == Street::Flop {
            self.flop_dealt = true;
        }
        self.bet_to_match = MwChips::ZERO;
        self.last_full_raise = self.big_blind;
        self.full_wager_established = false;
        self.aggressive_actions = 0;
        for seat in self.seats.seats().collect::<Vec<_>>() {
            self.seats[seat].raise_reopen_at = None;
        }
        self.pending = self.active_mask();
        self.phase = HandPhase::Betting;
        self.to_act = self.next_in_mask(self.button, self.pending);
    }

    fn next_in_mask(&self, after: SeatId, mask: SeatMask) -> Option<SeatId> {
        (1..=self.num_seats())
            .map(|step| after.advance(step, self.num_seats()))
            .find(|seat| mask.contains(*seat))
    }
}

#[derive(Debug, Clone, Copy)]
enum Contribution {
    Dead,
    Common,
    Street(Street),
}

fn post(seat: &mut SeatState, requested: MwChips, contribution: Contribution) {
    let amount = requested.min(seat.remaining);
    seat.remaining -= amount;
    match contribution {
        Contribution::Dead => seat.dead_committed += amount,
        Contribution::Common => seat.common_committed += amount,
        Contribution::Street(street) => seat.street_committed[street.index()] += amount,
    }
    if seat.remaining == MwChips::ZERO {
        seat.status = SeatStatus::AllIn;
    }
}

fn saturating_add(left: MwChips, right: MwChips) -> MwChips {
    MwChips(left.raw().saturating_add(right.raw()))
}

fn scale(amount: MwChips, factor: f64) -> MwChips {
    let scaled = (amount.raw() as f64 * factor).round();
    MwChips(scaled.clamp(0.0, u64::MAX as f64) as u64)
}

#[derive(Debug, Error)]
pub enum BettingError {
    #[error("betting state has no player to act")]
    MissingActor,
    #[error("seat {0} is not active but was selected to act")]
    InactiveActor(SeatId),
    #[error("illegal action by seat {actor}: {action:?}")]
    IllegalAction { actor: SeatId, action: Action },
    #[error("betting chip invariant was violated")]
    ChipInvariant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AbstractionConfig, AnteConfig, BlindConfig, MultiwayConfig, SeatConfig};

    fn state(stacks: &[f64], button: u8, ante: AnteConfig) -> (BettingState, BettingConfig) {
        let config = MultiwayConfig {
            seats: stacks
                .iter()
                .map(|&stack_bb| SeatConfig {
                    name: None,
                    stack_bb,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(button),
            blinds: BlindConfig::default(),
            ante,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        }
        .validated()
        .unwrap();
        let betting = config.betting.clone();
        (BettingState::new(&config).unwrap(), betting)
    }

    #[test]
    fn forced_posts_and_action_order_cover_heads_up_and_multiway() {
        let (heads_up, _) = state(&[20.0, 20.0], 0, AnteConfig::None);
        assert_eq!(heads_up.small_blind_seat, SeatId(0));
        assert_eq!(heads_up.big_blind_seat, SeatId(1));
        assert_eq!(heads_up.to_act, Some(SeatId(0)));

        let (nine, _) = state(&[20.0; 9], 4, AnteConfig::None);
        assert_eq!(nine.small_blind_seat, SeatId(5));
        assert_eq!(nine.big_blind_seat, SeatId(6));
        assert_eq!(nine.to_act, Some(SeatId(7)));
    }

    #[test]
    fn bba_is_posted_before_a_short_big_blind() {
        let (state, _) = state(
            &[20.0, 20.0, 1.5],
            0,
            AnteConfig::BigBlind { amount_bb: 1.0 },
        );
        assert_eq!(state.big_blind_seat, SeatId(2));
        assert_eq!(state.seats[SeatId(2)].common_committed, MwChips(1_000));
        assert_eq!(
            state.seats[SeatId(2)].committed_on(Street::Preflop),
            MwChips(500)
        );
        assert_eq!(state.seats[SeatId(2)].status, SeatStatus::AllIn);
        assert_eq!(state.bet_to_match, MwChips(1_000));
    }

    #[test]
    fn cumulative_short_raises_reopen_at_the_absolute_threshold() {
        let (mut state, mut betting) = state(&[20.0, 4.0, 5.0, 20.0], 0, AnteConfig::None);
        betting.preflop.bet_sizes = vec![crate::config::SizeSpec::ToBb { value: 3.0 }];
        // UTG (seat 3) raises 3bb, button calls; SB shoves 4bb, BB shoves 5bb.
        let raise = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|action| {
                matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(3_000),
                        ..
                    }
                )
            })
            .unwrap();
        state.apply(raise, &betting).unwrap();
        let call = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|action| matches!(action, Action::Call { .. }))
            .unwrap();
        state.apply(call, &betting).unwrap();
        let shove_four = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|action| {
                matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(4_000),
                        ..
                    }
                )
            })
            .unwrap();
        state.apply(shove_four, &betting).unwrap();
        let shove_five = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|action| {
                matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(5_000),
                        ..
                    }
                )
            })
            .unwrap();
        state.apply(shove_five, &betting).unwrap();
        assert_eq!(state.to_act, Some(SeatId(3)));
        assert!(
            state
                .legal_actions(&betting)
                .unwrap()
                .iter()
                .any(Action::is_aggressive)
        );
    }

    #[test]
    fn preflop_open_and_isolation_sizes_are_distinct() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        betting.preflop.bet_sizes = vec![crate::config::SizeSpec::ToBb { value: 2.5 }];
        betting.preflop.isolate_sizes = Some(vec![crate::config::SizeSpec::ToBb { value: 4.0 }]);
        betting.preflop.include_allin = false;

        let opening_actions = state.legal_actions(&betting).unwrap();
        assert!(opening_actions.iter().any(|action| {
            matches!(
                action,
                Action::RaiseTo {
                    to: MwChips(2_500),
                    ..
                }
            )
        }));
        assert!(!opening_actions.iter().any(|action| {
            matches!(
                action,
                Action::RaiseTo {
                    to: MwChips(4_000),
                    ..
                }
            )
        }));

        let limp = opening_actions
            .into_iter()
            .find(|action| matches!(action, Action::Call { .. }))
            .unwrap();
        state.apply(limp, &betting).unwrap();
        let isolation_actions = state.legal_actions(&betting).unwrap();
        assert!(isolation_actions.iter().any(|action| {
            matches!(
                action,
                Action::RaiseTo {
                    to: MwChips(4_000),
                    ..
                }
            )
        }));
    }

    #[test]
    fn limp_around_preserves_big_blind_option_and_all_streets_advance() {
        let (mut state, betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        assert_eq!(state.players_on_street(Street::Preflop), 3);
        for expected_actor in [SeatId(0), SeatId(1)] {
            assert_eq!(state.to_act, Some(expected_actor));
            let call = state
                .legal_actions(&betting)
                .unwrap()
                .into_iter()
                .find(|action| matches!(action, Action::Call { .. }))
                .unwrap();
            state.apply(call, &betting).unwrap();
        }
        assert_eq!(state.to_act, Some(SeatId(2)));
        state.apply(Action::Check, &betting).unwrap();
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.players_on_street(Street::Flop), 3);
        assert_eq!(state.to_act, Some(SeatId(1)));

        for expected_next in [Street::Turn, Street::River] {
            for _ in 0..3 {
                state.apply(Action::Check, &betting).unwrap();
            }
            assert_eq!(state.street, expected_next);
            assert_eq!(state.players_on_street(expected_next), 3);
            assert_eq!(state.to_act, Some(SeatId(1)));
        }
        for _ in 0..3 {
            state.apply(Action::Check, &betting).unwrap();
        }
        assert_eq!(state.phase, HandPhase::Showdown);
        assert_eq!(state.to_act, None);
    }

    #[test]
    fn lone_active_big_blind_runs_out_without_a_check_node() {
        let (state, _) = state(&[0.5, 20.0], 0, AnteConfig::None);
        assert_eq!(state.seats[SeatId(0)].status, SeatStatus::AllIn);
        assert_eq!(state.seats[SeatId(1)].status, SeatStatus::Active);
        assert_eq!(state.phase, HandPhase::Runout);
        assert_eq!(state.to_act, None);
        assert!(state.flop_dealt);
    }

    /// Builds a 3-seat, 100bb table with button=0 (so button is preflop UTG),
    /// then advances to a node where seat 2 (the big blind) faces a raise:
    /// button opens to 3bb, SB calls, BB is to act with `aggressive_actions
    /// == 1` so `raise_sizes` (not `bet_sizes`) governs its menu.
    fn state_with_bb_facing_a_raise(betting: &mut BettingConfig) -> BettingState {
        betting.preflop.bet_sizes = vec![crate::config::SizeSpec::ToBb { value: 3.0 }];
        let (mut bb_state, _) = state(&[100.0, 100.0, 100.0], 0, AnteConfig::None);
        let open_raise = bb_state
            .legal_actions(betting)
            .unwrap()
            .into_iter()
            .find(|action| {
                matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(3_000),
                        ..
                    }
                )
            })
            .unwrap();
        bb_state.apply(open_raise, betting).unwrap();
        let call = bb_state
            .legal_actions(betting)
            .unwrap()
            .into_iter()
            .find(|action| matches!(action, Action::Call { .. }))
            .unwrap();
        bb_state.apply(call, betting).unwrap();
        assert_eq!(bb_state.to_act, Some(SeatId(2)));
        bb_state
    }

    #[test]
    fn min_raise_size_emits_exactly_the_minimum_full_raise_target() {
        let (_, mut betting) = state(&[100.0, 100.0, 100.0], 0, AnteConfig::None);
        betting.preflop.raise_sizes = vec![
            crate::config::SizeSpec::MinRaise,
            crate::config::SizeSpec::PreviousBetMultiple { factor: 3.0 },
        ];
        betting.preflop.include_allin = false;
        let bb_state = state_with_bb_facing_a_raise(&mut betting);

        // bet_to_match = 3bb, last_full_raise = 2bb, so the minimum full
        // raise target is 5bb; the 3x-previous-bet size is 9bb, distinct
        // from the minimum.
        let actions = bb_state.legal_actions(&betting).unwrap();
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(5_000),
                full_raise: true,
                all_in: false,
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(9_000),
                ..
            }
        )));
    }

    #[test]
    fn stack_fraction_ladder_scales_off_the_actors_maximum() {
        let (_, mut betting) = state(&[100.0, 100.0, 100.0], 0, AnteConfig::None);
        betting.preflop.raise_sizes = vec![
            crate::config::SizeSpec::StackFraction { fraction: 0.25 },
            crate::config::SizeSpec::StackFraction { fraction: 0.5 },
        ];
        betting.preflop.include_allin = false;
        let bb_state = state_with_bb_facing_a_raise(&mut betting);

        // BB's maximum = actor_wager (1bb already posted) + remaining stack
        // (99bb) = 100bb = 100_000 chips. Neither fraction is clipped by the
        // minimum (5bb) or the maximum (100bb).
        let actions = bb_state.legal_actions(&betting).unwrap();
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(25_000),
                ..
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(50_000),
                ..
            }
        )));
    }

    #[test]
    fn allin_threshold_merges_a_qualifying_size_into_a_deduplicated_all_in() {
        let (_, mut betting) = state(&[100.0, 100.0, 100.0], 0, AnteConfig::None);
        betting.preflop.raise_sizes =
            vec![crate::config::SizeSpec::StackFraction { fraction: 0.9 }];
        betting.preflop.include_allin = true;
        betting.preflop.allin_threshold = Some(0.85);
        let bb_state = state_with_bb_facing_a_raise(&mut betting);

        // 0.9 * maximum (100_000) = 90_000 >= 0.85 * 100_000 = 85_000, so the
        // stack-fraction size merges into the all-in target instead of
        // standing on its own; `include_allin` would also propose the same
        // all-in target, so the merged and native all-in entries must dedup
        // to exactly one action.
        let actions = bb_state.legal_actions(&betting).unwrap();
        let all_in_raises: Vec<_> = actions
            .iter()
            .filter(|action| {
                matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(100_000),
                        all_in: true,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(all_in_raises.len(), 1);
        assert!(!actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(90_000),
                ..
            }
        )));
    }

    #[test]
    fn allin_threshold_absent_leaves_default_legal_actions_unchanged() {
        // Regression guard: an unmodified default preflop config (no
        // MinRaise/StackFraction sizes, no allin_threshold) must still
        // produce exactly the historical action set at the opening node.
        let (state, betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        assert!(betting.preflop.allin_threshold.is_none());
        let mut actions = state.legal_actions(&betting).unwrap();
        actions.sort_by_key(|action| action.amount().map(MwChips::raw));
        assert_eq!(
            actions,
            vec![
                Action::Fold,
                Action::Call {
                    amount: MwChips(1_000),
                    all_in: false,
                },
                Action::RaiseTo {
                    to: MwChips(2_500),
                    all_in: false,
                    full_raise: true,
                },
                Action::RaiseTo {
                    to: MwChips(20_000),
                    all_in: true,
                    full_raise: true,
                },
            ]
        );
    }
}
