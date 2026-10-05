//! No-limit Hold'em betting state for two through nine fixed seats.
//!
//! Antes are posted before blinds and live straddles. Per-player antes are dead individual
//! money; a big-blind ante is common main-pot money and does not change the
//! poster's side-pot cap.  The state keeps an
//! absolute raise-reopen threshold for every player, which naturally handles
//! multiple short all-ins whose cumulative increase becomes a full raise.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{MwChips, SeatId, SeatMask, SeatVec, SizeSpec, Street};

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

/// A plain NLH move, independent of a product's tree menu. Bet and raise
/// amounts are total wagers on this street, not additional contributions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Fold,
    Check,
    Call,
    BetTo(MwChips),
    RaiseTo(MwChips),
    AllIn,
}

/// Why a plain move cannot be played, including canonical move suggestions
/// and the chip bounds needed by a hand-line parser's diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IllegalMove {
    #[error("betting state has no player to act")]
    MissingActor,
    #[error("seat {0} is not active but was selected to act")]
    InactiveActor(SeatId),
    #[error("{attempted:?} is illegal with {to_call} to call; use {correct:?}")]
    WrongMove {
        attempted: Move,
        correct: Move,
        to_call: MwChips,
    },
    #[error("target {target} is below the minimum full target {minimum}")]
    BelowMinimum { target: MwChips, minimum: MwChips },
    #[error("target {target} exceeds the maximum target {maximum}")]
    AboveMaximum { target: MwChips, maximum: MwChips },
    #[error("target {maximum} uses the entire stack; use {correct:?}")]
    MustBeAllIn { maximum: MwChips, correct: Move },
    #[error("all-in target {maximum} does not exceed the bet ({to_call} to call); use {correct:?}")]
    AllInIsCall {
        maximum: MwChips,
        to_call: MwChips,
        correct: Move,
    },
    #[error("raising is not reopened for seat {actor}: bet {bet_to_match}, reopen at {reopen_at}")]
    RaisingClosed {
        actor: SeatId,
        bet_to_match: MwChips,
        reopen_at: MwChips,
    },
    #[error("seat {actor} cannot bet or raise without another active player")]
    NoActiveOpponent { actor: SeatId },
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
    /// in [`BettingState::new`] via `post`, never through [`Self::apply_action`], so
    /// they never set this flag.
    pub preflop_voluntary_call_seen: bool,
    pub preflop_limpers: u8,
    pub preflop_flats: u8,
    pub last_preflop_aggressor: Option<SeatId>,
    /// Last voluntary bet/raise on the current street, including short all-ins.
    /// Forced blind and straddle posts never set this field.
    #[serde(default)]
    pub last_street_aggressor: Option<SeatId>,
    /// Last voluntary bet/raise on the immediately preceding street. A street
    /// checked through (or skipped by policy) clears this on the next street.
    #[serde(default)]
    pub previous_street_aggressor: Option<SeatId>,
    /// Seats that have taken a voluntary preflop call or aggressive action.
    /// Forced contributions are posted before play and never enter this mask.
    #[serde(default)]
    pub preflop_participants: SeatMask,
    /// Number of first-time, non-big-blind seats that called the open before
    /// any re-raise. Big-blind defense is deliberately outside this cap.
    #[serde(default)]
    pub preflop_open_cold_calls: u8,
}

/// Forced contributions and starting stacks for a fixed NLH table.
/// Seat vectors must have the same validated table length; the button and
/// first actor must refer to seats in that table.
#[derive(Debug, Clone)]
pub struct TableSetup {
    pub button: SeatId,
    pub starting_stacks: SeatVec<MwChips>,
    pub forced_antes: SeatVec<MwChips>,
    pub common_ante: MwChips,
    pub forced_blinds: SeatVec<MwChips>,
    /// Ordered live preflop posts, each (seat, total street wager). Posted
    /// after blinds. The caller validates seats, amounts and posting order,
    /// and selects the first actor after the last straddler.
    pub straddles: Vec<(SeatId, MwChips)>,
    pub nominal_big_blind: MwChips,
    pub preflop_first_to_act: SeatId,
}

/// Product policy gates for otherwise shared NLH street transitions.
pub trait StreetPolicy {
    fn check_down(&self, state: &BettingState, actor: SeatId) -> bool;
    fn skip_street(&self, street: Street, players: u8) -> bool;
}

/// Plain NLH: no policy-driven check-down or skipped betting streets.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStreetPolicy;

impl StreetPolicy for NoStreetPolicy {
    fn check_down(&self, _state: &BettingState, _actor: SeatId) -> bool {
        false
    }
    fn skip_street(&self, _street: Street, _players: u8) -> bool {
        false
    }
}

impl BettingState {
    /// Post the setup's forced contributions and select the first actor.
    pub fn new<P: StreetPolicy>(config: &TableSetup, policy: &P) -> Result<Self, BettingError> {
        let num_seats = config.starting_stacks.len();
        let small_blind_seat = if num_seats == 2 {
            config.button
        } else {
            config.button.next(num_seats)
        };
        let big_blind_seat = small_blind_seat.next(num_seats);
        let mut seats = SeatVec::new_unchecked(
            config
                .starting_stacks
                .iter()
                .map(|seat| SeatState {
                    starting_stack: *seat,
                    remaining: *seat,
                    status: SeatStatus::Active,
                    dead_committed: MwChips::ZERO,
                    common_committed: MwChips::ZERO,
                    street_committed: [MwChips::ZERO; 4],
                    raise_reopen_at: None,
                })
                .collect(),
        );

        // Forced contributions are applied in the v1 normative order:
        // individual antes, table-common dead money, blinds, then straddles.
        for seat in config.starting_stacks.seats() {
            post(
                &mut seats[seat],
                config.forced_antes[seat],
                Contribution::Dead,
            );
        }
        post(
            &mut seats[big_blind_seat],
            config.common_ante,
            Contribution::Common,
        );
        for seat in config.starting_stacks.seats() {
            post(
                &mut seats[seat],
                config.forced_blinds[seat],
                Contribution::Street(Street::Preflop),
            );
        }
        for &(seat, to) in &config.straddles {
            let additional = to.saturating_sub(seats[seat].committed_on(Street::Preflop));
            post(
                &mut seats[seat],
                additional,
                Contribution::Street(Street::Preflop),
            );
        }
        let preflop_level = config
            .straddles
            .last()
            .map_or(config.nominal_big_blind, |&(_, to)| to);

        let mut state = Self {
            seats,
            button: config.button,
            small_blind_seat,
            big_blind_seat,
            big_blind: config.nominal_big_blind,
            street: Street::Preflop,
            street_active_players: [num_seats as u8, 0, 0, 0],
            to_act: None,
            // A short forced big blind does not lower the nominal call price.
            bet_to_match: preflop_level,
            last_full_raise: preflop_level,
            full_wager_established: true,
            pending: SeatMask::EMPTY,
            aggressive_actions: 0,
            flop_dealt: false,
            phase: HandPhase::Betting,
            preflop_voluntary_call_seen: false,
            preflop_limpers: 0,
            preflop_flats: 0,
            last_preflop_aggressor: None,
            last_street_aggressor: None,
            previous_street_aggressor: None,
            preflop_participants: SeatMask::EMPTY,
            preflop_open_cold_calls: 0,
        };
        state.pending = state.active_mask();
        let before_first = config
            .preflop_first_to_act
            .advance(num_seats - 1, num_seats);
        state.finish_or_select(before_first, policy);
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

    /// The selected active player, with the same errors used by menu expansion.
    pub fn actor(&self) -> Result<SeatId, BettingError> {
        let actor = self.to_act.ok_or(BettingError::MissingActor)?;
        if self.seats[actor].status != SeatStatus::Active {
            return Err(BettingError::InactiveActor(actor));
        }
        Ok(actor)
    }

    /// Stack-capped call for an active actor (possibly a zero call).
    pub fn call_action(&self, actor: SeatId) -> Action {
        let stack = self.seats[actor].remaining;
        let amount = self.amount_to_call(actor).min(stack);
        Action::Call {
            amount,
            all_in: amount == stack,
        }
    }

    /// Largest street wager this player can reach.
    pub fn maximum_target(&self, actor: SeatId) -> MwChips {
        self.current_wager(actor) + self.seats[actor].remaining
    }

    /// Whether NLH permits an aggressive action, independent of menu caps.
    pub fn can_raise(&self, actor: SeatId) -> bool {
        let raising_open = self.seats[actor]
            .raise_reopen_at
            .is_none_or(|threshold| self.bet_to_match >= threshold);
        let another_active = !self
            .active_mask()
            .difference(SeatMask::from_seat(actor))
            .is_empty();
        self.maximum_target(actor) > self.bet_to_match && raising_open && another_active
    }

    /// Resolve a strict plain-NLH move without clamping targets or consulting
    /// a product menu. Apply the returned action with [`Self::apply_action`]
    /// and [`NoStreetPolicy`] when replaying a hand line.
    pub fn resolve_move(&self, mv: Move) -> Result<Action, IllegalMove> {
        let actor = self.to_act.ok_or(IllegalMove::MissingActor)?;
        if self.seats[actor].status != SeatStatus::Active {
            return Err(IllegalMove::InactiveActor(actor));
        }
        let to_call = self.amount_to_call(actor);
        let wrong = |correct| IllegalMove::WrongMove {
            attempted: mv,
            correct,
            to_call,
        };
        match mv {
            Move::Fold | Move::Call if to_call == MwChips::ZERO => {
                return Err(wrong(Move::Check));
            }
            Move::Check if to_call > MwChips::ZERO => return Err(wrong(Move::Call)),
            Move::Fold => return Ok(Action::Fold),
            Move::Check => return Ok(Action::Check),
            Move::Call => return Ok(self.call_action(actor)),
            Move::BetTo(to) if self.bet_to_match > MwChips::ZERO => {
                return Err(wrong(Move::RaiseTo(to)));
            }
            Move::RaiseTo(to) if self.bet_to_match == MwChips::ZERO => {
                return Err(wrong(Move::BetTo(to)));
            }
            _ => {}
        }
        let maximum = self.maximum_target(actor);
        let target = match mv {
            Move::AllIn => {
                if maximum <= self.bet_to_match {
                    return Err(IllegalMove::AllInIsCall {
                        maximum,
                        to_call,
                        correct: Move::Call,
                    });
                }
                maximum
            }
            Move::BetTo(target) | Move::RaiseTo(target) => {
                if target > maximum {
                    return Err(IllegalMove::AboveMaximum { target, maximum });
                }
                if target == maximum {
                    return Err(IllegalMove::MustBeAllIn {
                        maximum,
                        correct: Move::AllIn,
                    });
                }
                let minimum = self.minimum_full_target();
                if target < minimum {
                    return Err(IllegalMove::BelowMinimum { target, minimum });
                }
                target
            }
            Move::Fold | Move::Check | Move::Call => unreachable!("passive moves handled above"),
        };
        if let Some(reopen_at) = self.seats[actor].raise_reopen_at
            && self.bet_to_match < reopen_at
        {
            return Err(IllegalMove::RaisingClosed {
                actor,
                bet_to_match: self.bet_to_match,
                reopen_at,
            });
        }
        if !self.can_raise(actor) {
            return Err(IllegalMove::NoActiveOpponent { actor });
        }
        Ok(self
            .action_for_target(actor, target)
            .expect("validated target and raising rights"))
    }

    /// Resolve one size literal, clamping to the minimum full wager or stack cap.
    pub fn resolve_size(&self, actor: SeatId, size: &SizeSpec) -> MwChips {
        let stack = self.seats[actor].remaining;
        let actor_wager = self.current_wager(actor);
        let to_call = self.amount_to_call(actor);
        let maximum = self.maximum_target(actor);
        let minimum = self.minimum_full_target();
        let pot_after_call = self.pot_size() + to_call.min(stack);
        let called_to = actor_wager + to_call.min(stack);
        let proposed = match *size {
            SizeSpec::ToBb { value } => scale(self.big_blind, value),
            SizeSpec::PotAfterCall { fraction } => called_to + scale(pot_after_call, fraction),
            SizeSpec::PreviousBetMultiple { factor } => scale(self.bet_to_match, factor),
            SizeSpec::MinRaise => minimum,
            SizeSpec::AllIn => maximum,
            SizeSpec::EffectiveStackFraction { fraction } => {
                let effective = self
                    .seats
                    .seats()
                    .filter(|seat| *seat != actor && self.seats[*seat].status != SeatStatus::Folded)
                    .map(|seat| self.current_wager(seat) + self.seats[seat].remaining)
                    .max()
                    .unwrap_or(maximum)
                    .min(maximum);
                scale(effective, fraction)
            }
            SizeSpec::GeometricAllIn { streets } => {
                geometric_allin_target(called_to, pot_after_call, maximum, streets)
            }
            // Pio's bare `e`: divide the remaining stack across the
            // streets still to be played, this one included.
            SizeSpec::GeometricAllInRemaining => geometric_allin_target(
                called_to,
                pot_after_call,
                maximum,
                streets_remaining(self.street),
            ),
            SizeSpec::StackFraction { fraction } => scale(maximum, fraction),
            SizeSpec::ToChips { .. } => unreachable!(
                "ToChips is the postflop-family chip literal; the multiway config \
                     parser (SizeUnit::Bb) never produces it"
            ),
        };
        if proposed < minimum && maximum >= minimum {
            minimum
        } else {
            proposed.min(maximum)
        }
    }

    /// Build the aggressive action for a target no greater than the stack cap.
    /// The caller checks raising rights; sub-minimum wagers are accepted only
    /// at the stack cap.
    pub fn action_for_target(&self, actor: SeatId, target: MwChips) -> Option<Action> {
        if target <= self.bet_to_match {
            return None;
        }
        let all_in = target == self.maximum_target(actor);
        let full_raise = target >= self.minimum_full_target();
        if !full_raise && !all_in {
            return None;
        }
        Some(if self.bet_to_match == MwChips::ZERO {
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
        })
    }

    /// Mutate the state after an action. The caller must have checked legality.
    pub fn apply_action<P: StreetPolicy>(
        &mut self,
        action: Action,
        betting: &P,
    ) -> Result<(), BettingError> {
        let actor = self.to_act.ok_or(BettingError::MissingActor)?;
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
                    let first_voluntary_action = !self.preflop_participants.contains(actor);
                    if self.aggressive_actions == 0 {
                        self.preflop_limpers = self.preflop_limpers.saturating_add(1);
                    } else {
                        self.preflop_flats = self.preflop_flats.saturating_add(1);
                    }
                    if self.aggressive_actions == 1
                        && actor != self.big_blind_seat
                        && first_voluntary_action
                    {
                        self.preflop_open_cold_calls =
                            self.preflop_open_cold_calls.saturating_add(1);
                    }
                    self.preflop_participants.insert(actor);
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
                if self.street == Street::Preflop {
                    self.preflop_participants.insert(actor);
                    self.last_preflop_aggressor = Some(actor);
                    self.preflop_flats = 0;
                }
                self.aggressive_actions = self.aggressive_actions.saturating_add(1);
                self.last_street_aggressor = Some(actor);
                self.seats[actor].raise_reopen_at = Some(saturating_add(to, self.last_full_raise));
                self.pending = self.active_mask();
                self.pending.remove(actor);
            }
        }
        self.street_active_players[self.street.index()] = self.non_folded_mask().len() as u8;
        self.finish_or_select(actor, betting);
        Ok(())
    }

    /// Minimum target for a full bet or raise.
    pub fn minimum_full_target(&self) -> MwChips {
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

    fn finish_or_select<P: StreetPolicy>(&mut self, after: SeatId, betting: &P) {
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
                self.end_betting_round(betting);
                return;
            }
        }
        if self.pending.is_empty() {
            self.end_betting_round(betting);
            return;
        }
        self.to_act = self.next_in_mask(after, self.pending);
        if self
            .to_act
            .is_some_and(|actor| betting.check_down(self, actor))
        {
            self.pending = SeatMask::EMPTY;
            self.end_betting_round(betting);
        }
    }

    /// Closes the current street's betting round and opens the next one, or
    /// settles the hand if the river just closed.
    ///
    /// Before putting a postflop street into `Betting`, this checks that
    /// street's `max_betting_players` (HRC-style check-down) against the
    /// non-folded seat count carried forward from the street that just
    /// closed. A street whose count exceeds its threshold gets no decision
    /// nodes at all: the loop below advances straight past it (folds are
    /// impossible without betting, so the count is unchanged) and re-checks
    /// the next street's own threshold, exactly as if every remaining actor
    /// had nothing to do. This reuses the same actionless
    /// [`HandPhase::Runout`] fast-forward already used when every seat still
    /// in the hand is all-in, rather than emitting check actions.
    fn end_betting_round<P: StreetPolicy>(&mut self, betting: &P) {
        self.to_act = None;
        if self.street == Street::River {
            self.phase = HandPhase::Showdown;
            return;
        }
        if self.active_mask().len() <= 1 {
            self.phase = HandPhase::Runout;
            self.flop_dealt = true;
            if self.street == Street::Preflop {
                self.street_active_players[Street::Flop.index()] =
                    self.non_folded_mask().len() as u8;
            }
            return;
        }
        let players_entering_next_street = self.street_active_players[self.street.index()];
        loop {
            self.previous_street_aggressor = self.last_street_aggressor.take();
            self.street = self.street.next().expect("river handled above");
            self.street_active_players[self.street.index()] = players_entering_next_street;
            if self.street == Street::Flop {
                self.flop_dealt = true;
            }
            let checked_down = betting.skip_street(self.street, players_entering_next_street);
            if !checked_down {
                break;
            }
            if self.street == Street::River {
                self.phase = HandPhase::Runout;
                return;
            }
        }
        self.bet_to_match = MwChips::ZERO;
        self.last_full_raise = self.big_blind;
        self.full_wager_established = false;
        self.aggressive_actions = 0;
        for index in 0..self.num_seats() {
            self.seats[SeatId::new_unchecked(index as u8)].raise_reopen_at = None;
        }
        self.pending = self.active_mask();
        self.phase = HandPhase::Betting;
        self.finish_or_select(self.button, betting);
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
/// Betting streets left to play from `street`, inclusive — what a bare `e`
/// geometric size divides the remaining stack across.
fn streets_remaining(street: Street) -> u8 {
    match street {
        Street::Preflop => 4,
        Street::Flop => 3,
        Street::Turn => 2,
        Street::River => 1,
    }
}

fn geometric_allin_target(
    called_to: MwChips,
    pot_after_call: MwChips,
    maximum: MwChips,
    streets: u8,
) -> MwChips {
    MwChips(crate::geometric_allin_target(
        called_to.raw(),
        pot_after_call.raw(),
        maximum.raw(),
        streets,
    ))
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
    #[error("invalid tree rule: {0}")]
    TreeRule(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    enum AnteConfig {
        None,
        BigBlind { amount_bb: f64 },
    }

    fn state(stacks: &[f64], button: u8, ante: AnteConfig) -> (BettingState, NoStreetPolicy) {
        let num_seats = stacks.len();
        let button = SeatId(button);
        let small_blind = if num_seats == 2 {
            button
        } else {
            button.next(num_seats)
        };
        let big_blind = small_blind.next(num_seats);
        let mut forced_blinds = vec![MwChips::ZERO; num_seats];
        forced_blinds[small_blind.index()] = MwChips(500);
        forced_blinds[big_blind.index()] = MwChips(1000);
        let setup = TableSetup {
            button,
            starting_stacks: SeatVec::try_new(
                stacks
                    .iter()
                    .map(|&bb| MwChips::try_from_bb(bb).unwrap())
                    .collect(),
            )
            .unwrap(),
            forced_antes: SeatVec::try_new(vec![MwChips::ZERO; num_seats]).unwrap(),
            common_ante: match ante {
                AnteConfig::None => MwChips::ZERO,
                AnteConfig::BigBlind { amount_bb } => MwChips::try_from_bb(amount_bb).unwrap(),
            },
            forced_blinds: SeatVec::try_new(forced_blinds).unwrap(),
            straddles: Vec::new(),
            nominal_big_blind: MwChips(1000),
            preflop_first_to_act: big_blind.next(num_seats),
        };
        (
            BettingState::new(&setup, &NoStreetPolicy).unwrap(),
            NoStreetPolicy,
        )
    }

    fn raise_to(state: &BettingState, to: MwChips) -> Action {
        let actor = state.actor().unwrap();
        assert!(state.can_raise(actor));
        let target = state.resolve_size(actor, &SizeSpec::ToBb { value: to.as_bb() });
        assert_eq!(target, to);
        state.action_for_target(actor, target).unwrap()
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
        let (mut state, betting) = state(&[20.0, 4.0, 5.0, 20.0], 0, AnteConfig::None);
        // UTG (seat 3) raises 3bb, button calls; SB shoves 4bb, BB shoves 5bb.
        let raise = raise_to(&state, MwChips(3_000));
        state.apply_action(raise, &betting).unwrap();
        let call = state.call_action(state.actor().unwrap());
        state.apply_action(call, &betting).unwrap();
        let shove_four = raise_to(&state, MwChips(4_000));
        state.apply_action(shove_four, &betting).unwrap();
        let shove_five = raise_to(&state, MwChips(5_000));
        state.apply_action(shove_five, &betting).unwrap();
        assert_eq!(state.to_act, Some(SeatId(3)));
        assert!(
            state.can_raise(state.actor().unwrap())
                && state
                    .action_for_target(state.actor().unwrap(), state.minimum_full_target())
                    .is_some()
        );
    }

    #[test]
    fn limp_around_preserves_big_blind_option_and_all_streets_advance() {
        let (mut state, betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        assert_eq!(state.players_on_street(Street::Preflop), 3);
        for expected_actor in [SeatId(0), SeatId(1)] {
            assert_eq!(state.to_act, Some(expected_actor));
            let call = state.call_action(state.actor().unwrap());
            state.apply_action(call, &betting).unwrap();
        }
        assert_eq!(state.to_act, Some(SeatId(2)));
        state.apply_action(Action::Check, &betting).unwrap();
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.players_on_street(Street::Flop), 3);
        assert_eq!(state.to_act, Some(SeatId(1)));

        for expected_next in [Street::Turn, Street::River] {
            for _ in 0..3 {
                state.apply_action(Action::Check, &betting).unwrap();
            }
            assert_eq!(state.street, expected_next);
            assert_eq!(state.players_on_street(expected_next), 3);
            assert_eq!(state.to_act, Some(SeatId(1)));
        }
        for _ in 0..3 {
            state.apply_action(Action::Check, &betting).unwrap();
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

    #[test]
    fn preflop_participants_and_non_bb_open_cold_calls_exclude_forced_posts() {
        let (mut state, betting) = state(&[100.0; 6], 0, AnteConfig::None);
        assert!(state.preflop_participants.is_empty());
        assert_eq!(state.preflop_open_cold_calls, 0);

        let open = raise_to(&state, MwChips(2_500));
        assert_eq!(state.to_act, Some(SeatId(3)));
        state.apply_action(open, &betting).unwrap();
        assert!(state.preflop_participants.contains(SeatId(3)));

        for (actor, expected_calls) in [(SeatId(4), 1), (SeatId(5), 2), (SeatId(0), 3)] {
            assert_eq!(state.to_act, Some(actor));
            let call = state.call_action(state.actor().unwrap());
            state.apply_action(call, &betting).unwrap();
            assert!(state.preflop_participants.contains(actor));
            assert_eq!(state.preflop_open_cold_calls, expected_calls);
        }

        assert_eq!(state.to_act, Some(SeatId(1)));
        state.apply_action(Action::Fold, &betting).unwrap();
        assert_eq!(state.to_act, Some(SeatId(2)));
        let bb_call = state.call_action(state.actor().unwrap());
        state.apply_action(bb_call, &betting).unwrap();
        assert!(state.preflop_participants.contains(SeatId(2)));
        assert_eq!(state.preflop_open_cold_calls, 3);
        assert!(!state.preflop_participants.contains(SeatId(1)));
    }

    #[test]
    fn participant_fields_default_when_deserializing_older_betting_state() {
        let (state, _) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        let mut encoded = serde_json::to_value(state).unwrap();
        let object = encoded.as_object_mut().unwrap();
        object.remove("preflop_participants");
        object.remove("preflop_open_cold_calls");
        object.remove("last_street_aggressor");
        object.remove("previous_street_aggressor");
        let decoded: BettingState = serde_json::from_value(encoded).unwrap();
        assert!(decoded.preflop_participants.is_empty());
        assert_eq!(decoded.preflop_open_cold_calls, 0);
        assert_eq!(decoded.last_street_aggressor, None);
        assert_eq!(decoded.previous_street_aggressor, None);
    }
}
