//! Plain-NLH hand replay contracts, independent of any product menu.
use nlh::betting::{Action, BettingState, HandPhase, IllegalMove, Move, SeatStatus};
use nlh::{MwChips, NoStreetPolicy, SeatId, SeatVec, SizeSpec, Street, TableSetup, position_name};

fn chips(bb: f64) -> MwChips {
    MwChips::try_from_bb(bb).unwrap()
}

fn setup(stacks: &[f64]) -> TableSetup {
    let n = stacks.len();
    let sb = usize::from(n != 2);
    let bb = sb + 1;
    let mut blinds = vec![MwChips::ZERO; n];
    blinds[sb] = chips(0.5);
    blinds[bb] = chips(1.0);
    TableSetup {
        button: SeatId(0),
        starting_stacks: SeatVec::try_new(stacks.iter().map(|&bb| chips(bb)).collect()).unwrap(),
        forced_antes: SeatVec::try_new(vec![MwChips::ZERO; n]).unwrap(),
        common_ante: MwChips::ZERO,
        forced_blinds: SeatVec::try_new(blinds).unwrap(),
        straddles: Vec::new(),
        nominal_big_blind: chips(1.0),
        preflop_first_to_act: SeatId(((bb + 1) % n) as u8),
    }
}

fn state(stacks: &[f64]) -> BettingState {
    BettingState::new(&setup(stacks), &NoStreetPolicy).unwrap()
}

fn play(state: &mut BettingState, mv: Move) -> Action {
    let action = state.resolve_move(mv).unwrap();
    state.apply_action(action.clone(), &NoStreetPolicy).unwrap();
    action
}

fn assert_no_voluntary_history(state: &BettingState) {
    assert_eq!(state.aggressive_actions, 0);
    assert_eq!(state.last_preflop_aggressor, None);
    assert_eq!(state.last_street_aggressor, None);
    assert_eq!(state.previous_street_aggressor, None);
    assert!(state.preflop_participants.is_empty());
    assert!(!state.preflop_voluntary_call_seen);
    assert_eq!(state.preflop_limpers, 0);
    assert_eq!(state.preflop_flats, 0);
    assert_eq!(state.preflop_open_cold_calls, 0);
}

#[test]
fn six_max_straddles_set_preflop_price_order_and_preserve_postflop_bb() {
    let mut setup = setup(&[100.0; 6]);
    setup.straddles = vec![(SeatId(3), chips(2.0)), (SeatId(4), chips(4.0))];
    setup.preflop_first_to_act = SeatId(5);
    let mut state = BettingState::new(&setup, &NoStreetPolicy).unwrap();
    assert_no_voluntary_history(&state);
    assert_eq!(state.pot_size(), chips(7.5));
    assert_eq!(state.big_blind, chips(1.0));
    assert_eq!(state.bet_to_match, chips(4.0));
    assert_eq!(state.last_full_raise, chips(4.0));
    assert_eq!(state.minimum_full_target(), chips(8.0));
    assert_eq!(
        state.resolve_size(SeatId(5), &SizeSpec::PreviousBetMultiple { factor: 3.0 }),
        chips(12.0)
    );
    assert_eq!(
        state.resolve_size(SeatId(5), &SizeSpec::ToBb { value: 10.0 }),
        chips(10.0)
    );
    assert_eq!(
        state.resolve_move(Move::RaiseTo(chips(8.0))).unwrap(),
        Action::RaiseTo {
            to: chips(8.0),
            all_in: false,
            full_raise: true,
        }
    );
    let mut order = Vec::new();
    for seat in [5, 0, 1, 2, 3] {
        assert_eq!(state.actor().unwrap(), SeatId(seat));
        order.push(position_name(SeatId(seat), SeatId(0), 6));
        let expected = chips(4.0) - state.current_wager(SeatId(seat));
        assert_eq!(
            play(&mut state, Move::Call),
            Action::Call {
                amount: expected,
                all_in: false
            }
        );
    }
    assert_eq!(state.preflop_limpers, 5);
    assert_eq!(state.preflop_flats, 0);
    assert_eq!(state.actor().unwrap(), SeatId(4));
    order.push(position_name(SeatId(4), SeatId(0), 6));
    assert_eq!(order, ["CO", "BTN", "SB", "BB", "UTG", "HJ"]);
    assert_eq!(state.amount_to_call(SeatId(4)), MwChips::ZERO);
    play(&mut state, Move::Check);
    assert_eq!(state.street, Street::Flop);
    assert_eq!(state.actor().unwrap(), SeatId(1));
    assert_eq!(state.minimum_full_target(), chips(1.0));
    assert_eq!(state.last_full_raise, chips(1.0));
    assert_eq!(state.aggressive_actions, 0);
    assert_eq!(state.previous_street_aggressor, None);
    assert_eq!(state.last_preflop_aggressor, None);
    assert_eq!(
        state.resolve_move(Move::BetTo(chips(1.0))).unwrap(),
        Action::BetTo {
            to: chips(1.0),
            all_in: false,
            full_raise: true,
        }
    );
}

#[test]
fn three_handed_button_straddle_acts_last() {
    let mut setup = setup(&[100.0; 3]);
    setup.straddles = vec![(SeatId(0), chips(2.0))];
    setup.preflop_first_to_act = SeatId(1);
    let mut state = BettingState::new(&setup, &NoStreetPolicy).unwrap();
    assert_no_voluntary_history(&state);
    assert_eq!(state.minimum_full_target(), chips(4.0));
    for seat in [1, 2] {
        assert_eq!(state.actor().unwrap(), SeatId(seat));
        play(&mut state, Move::Call);
    }
    assert_eq!(state.actor().unwrap(), SeatId(0));
    play(&mut state, Move::Check);
    assert_eq!(state.street, Street::Flop);
    assert_eq!(state.actor().unwrap(), SeatId(1));
}

#[test]
fn deep_restraddle_chains_use_the_last_level_and_clockwise_order() {
    for amounts in [&[2.0, 4.0, 8.0][..], &[2.0, 4.0, 8.0, 16.0][..]] {
        let mut setup = setup(&[100.0; 7]);
        setup.straddles = amounts
            .iter()
            .enumerate()
            .map(|(i, &bb)| (SeatId(3 + i as u8), chips(bb)))
            .collect();
        let last = setup.straddles.last().unwrap().0;
        setup.preflop_first_to_act = last.next(7);
        let mut state = BettingState::new(&setup, &NoStreetPolicy).unwrap();
        assert_no_voluntary_history(&state);
        let level = *amounts.last().unwrap();
        assert_eq!(state.minimum_full_target(), chips(2.0 * level));
        assert_eq!(
            state.resolve_size(
                state.actor().unwrap(),
                &SizeSpec::PreviousBetMultiple { factor: 3.0 }
            ),
            chips(3.0 * level)
        );
        for offset in 1..7 {
            assert_eq!(state.actor().unwrap(), last.advance(offset, 7));
            play(&mut state, Move::Call);
        }
        assert_eq!(state.actor().unwrap(), last);
        play(&mut state, Move::Check);
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.minimum_full_target(), chips(1.0));
    }
}

#[test]
fn straddle_posts_are_to_amounts_after_blinds_and_dead_money() {
    // TableSetup is a general rules API; v1 input position restrictions are
    // validated by spot. A live post must top up an existing wager, not add it.
    let mut setup = setup(&[100.0; 3]);
    setup.forced_antes = SeatVec::try_new(vec![chips(0.25); 3]).unwrap();
    setup.common_ante = chips(0.5);
    setup.straddles = vec![(SeatId(1), chips(2.0))];
    setup.preflop_first_to_act = SeatId(2);
    let state = BettingState::new(&setup, &NoStreetPolicy).unwrap();
    assert_eq!(state.current_wager(SeatId(1)), chips(2.0));
    assert_eq!(state.seats[SeatId(1)].remaining, chips(97.75));
    assert_eq!(state.pot_size(), chips(4.25));
    assert_no_voluntary_history(&state);
}

#[test]
fn move_without_actor_is_rejected() {
    let state = state(&[0.5, 100.0]);
    assert_eq!(state.phase, HandPhase::Runout);
    assert_eq!(
        state.resolve_move(Move::Check),
        Err(IllegalMove::MissingActor)
    );
}

#[test]
fn inactive_actor_is_rejected() {
    let mut state = state(&[100.0; 3]);
    state.seats[SeatId(0)].status = SeatStatus::Folded;
    assert_eq!(
        state.resolve_move(Move::Call),
        Err(IllegalMove::InactiveActor(SeatId(0)))
    );
}

#[test]
fn wrong_moves_report_the_canonical_move_and_call_amount() {
    let mut state = state(&[100.0; 2]);
    for (attempted, correct) in [
        (Move::Check, Move::Call),
        (Move::BetTo(chips(3.0)), Move::RaiseTo(chips(3.0))),
    ] {
        assert_eq!(
            state.resolve_move(attempted),
            Err(IllegalMove::WrongMove {
                attempted,
                correct,
                to_call: chips(0.5)
            })
        );
    }
    play(&mut state, Move::Call);
    for attempted in [Move::Fold, Move::Call] {
        assert_eq!(
            state.resolve_move(attempted),
            Err(IllegalMove::WrongMove {
                attempted,
                correct: Move::Check,
                to_call: MwChips::ZERO
            })
        );
    }
    play(&mut state, Move::Check);
    let attempted = Move::RaiseTo(chips(2.0));
    assert_eq!(
        state.resolve_move(attempted),
        Err(IllegalMove::WrongMove {
            attempted,
            correct: Move::BetTo(chips(2.0)),
            to_call: MwChips::ZERO
        })
    );
    assert_eq!(
        state.resolve_move(Move::Fold),
        Err(IllegalMove::WrongMove {
            attempted: Move::Fold,
            correct: Move::Check,
            to_call: MwChips::ZERO
        })
    );
    play(&mut state, Move::BetTo(chips(1.0)));
    assert_eq!(
        state.resolve_move(Move::BetTo(chips(3.0))),
        Err(IllegalMove::WrongMove {
            attempted: Move::BetTo(chips(3.0)),
            correct: Move::RaiseTo(chips(3.0)),
            to_call: chips(1.0)
        })
    );
    assert_eq!(state.resolve_move(Move::Fold).unwrap(), Action::Fold);
}

#[test]
fn target_below_minimum_is_rejected_without_clamping() {
    let mut state = state(&[100.0; 2]);
    assert_eq!(
        state.resolve_move(Move::RaiseTo(chips(1.999))),
        Err(IllegalMove::BelowMinimum {
            target: chips(1.999),
            minimum: chips(2.0)
        })
    );
    play(&mut state, Move::Call);
    play(&mut state, Move::Check);
    assert_eq!(
        state.resolve_move(Move::BetTo(chips(0.999))),
        Err(IllegalMove::BelowMinimum {
            target: chips(0.999),
            minimum: chips(1.0)
        })
    );
}

#[test]
fn target_above_maximum_is_rejected_without_clamping() {
    let state = state(&[100.0; 2]);
    assert_eq!(
        state.resolve_move(Move::RaiseTo(chips(100.001))),
        Err(IllegalMove::AboveMaximum {
            target: chips(100.001),
            maximum: chips(100.0)
        })
    );
}

#[test]
fn full_and_short_stack_targets_must_be_written_as_all_in() {
    for stack in [100.0, 1.5] {
        let state = state(&[stack, 100.0]);
        assert_eq!(
            state.resolve_move(Move::RaiseTo(chips(stack))),
            Err(IllegalMove::MustBeAllIn {
                maximum: chips(stack),
                correct: Move::AllIn
            })
        );
        assert_eq!(
            state.resolve_move(Move::AllIn).unwrap(),
            Action::RaiseTo {
                to: chips(stack),
                all_in: true,
                full_raise: stack >= 2.0
            }
        );
    }
    let mut state = state(&[100.0; 2]);
    play(&mut state, Move::Call);
    play(&mut state, Move::Check);
    assert_eq!(
        state.resolve_move(Move::BetTo(chips(99.0))),
        Err(IllegalMove::MustBeAllIn {
            maximum: chips(99.0),
            correct: Move::AllIn
        })
    );
    assert_eq!(
        state.resolve_move(Move::AllIn).unwrap(),
        Action::BetTo {
            to: chips(99.0),
            all_in: true,
            full_raise: true
        }
    );
}

#[test]
fn all_in_calls_for_less_or_exactly_the_bet_are_calls() {
    for stack in [0.75, 1.0] {
        let mut state = state(&[stack, 100.0]);
        assert_eq!(
            state.resolve_move(Move::AllIn),
            Err(IllegalMove::AllInIsCall {
                maximum: chips(stack),
                to_call: chips(0.5),
                correct: Move::Call
            })
        );
        assert_eq!(
            play(&mut state, Move::Call),
            Action::Call {
                amount: chips(stack - 0.5),
                all_in: true
            }
        );
        assert_eq!(state.seats[SeatId(0)].status, SeatStatus::AllIn);
        assert_eq!(state.phase, HandPhase::Runout);
    }
}

fn short_raise_state() -> BettingState {
    let mut state = state(&[100.0, 4.0, 100.0, 100.0]);
    play(&mut state, Move::RaiseTo(chips(3.0))); // CO
    play(&mut state, Move::Call); // BTN
    assert_eq!(
        play(&mut state, Move::AllIn),
        Action::RaiseTo {
            to: chips(4.0),
            all_in: true,
            full_raise: false
        }
    ); // SB
    assert_eq!(state.last_full_raise, chips(2.0));
    assert_eq!(state.last_street_aggressor, Some(SeatId(1)));
    state
}

#[test]
fn short_all_in_does_not_reopen_an_actor_who_already_acted() {
    let mut state = short_raise_state();
    play(&mut state, Move::Call); // BB
    assert_eq!(state.actor().unwrap(), SeatId(3));
    for mv in [Move::RaiseTo(chips(6.0)), Move::AllIn] {
        assert_eq!(
            state.resolve_move(mv),
            Err(IllegalMove::RaisingClosed {
                actor: SeatId(3),
                bet_to_match: chips(4.0),
                reopen_at: chips(5.0)
            })
        );
    }
    play(&mut state, Move::Call);
    assert!(matches!(
        state.resolve_move(Move::AllIn),
        Err(IllegalMove::RaisingClosed {
            actor: SeatId(0),
            ..
        })
    ));
}

#[test]
fn player_yet_to_act_can_raise_over_a_non_reopening_short_all_in() {
    let mut state = short_raise_state();
    assert_eq!(state.actor().unwrap(), SeatId(2));
    assert_eq!(
        play(&mut state, Move::RaiseTo(chips(6.0))),
        Action::RaiseTo {
            to: chips(6.0),
            all_in: false,
            full_raise: true
        }
    );
    assert_eq!(state.actor().unwrap(), SeatId(3));
    assert!(state.resolve_move(Move::RaiseTo(chips(8.0))).is_ok());
}

#[test]
fn cumulative_short_all_ins_reopen_the_original_raiser() {
    let mut state = state(&[100.0, 4.0, 5.0, 100.0]);
    play(&mut state, Move::RaiseTo(chips(3.0)));
    play(&mut state, Move::Call);
    play(&mut state, Move::AllIn);
    play(&mut state, Move::AllIn);
    assert_eq!(state.actor().unwrap(), SeatId(3));
    assert_eq!(state.minimum_full_target(), chips(7.0));
    play(&mut state, Move::RaiseTo(chips(7.0)));
}

#[test]
fn lone_active_player_cannot_raise_against_an_all_in_player() {
    let mut state = state(&[100.0, 1.5]);
    play(&mut state, Move::Call);
    play(&mut state, Move::Check);
    assert_eq!(
        play(&mut state, Move::AllIn),
        Action::BetTo {
            to: chips(0.5),
            all_in: true,
            full_raise: false
        }
    );
    assert_eq!(state.minimum_full_target(), chips(1.0));
    for mv in [Move::RaiseTo(chips(1.0)), Move::AllIn] {
        assert_eq!(
            state.resolve_move(mv),
            Err(IllegalMove::NoActiveOpponent { actor: SeatId(0) })
        );
    }
    play(&mut state, Move::Call);
    assert_eq!(state.phase, HandPhase::Runout);
}

#[test]
fn checking_preserves_raising_rights_against_a_short_opening_all_in() {
    let mut state = state(&[1.5, 100.0, 100.0]);
    play(&mut state, Move::Call);
    play(&mut state, Move::Call);
    play(&mut state, Move::Check);
    play(&mut state, Move::Check); // SB
    play(&mut state, Move::Check); // BB
    play(&mut state, Move::AllIn); // BTN opens for 0.5
    assert_eq!(state.minimum_full_target(), chips(1.0));
    assert_eq!(
        play(&mut state, Move::RaiseTo(chips(1.0))),
        Action::RaiseTo {
            to: chips(1.0),
            all_in: false,
            full_raise: true
        }
    );
}

#[test]
fn antes_and_big_blind_ante_never_change_minimum_raise() {
    for (ante, bba) in [(0.25, 0.0), (0.0, 2.0)] {
        let mut setup = setup(&[100.0; 3]);
        setup.forced_antes = SeatVec::try_new(vec![chips(ante); 3]).unwrap();
        setup.common_ante = chips(bba);
        let mut state = BettingState::new(&setup, &NoStreetPolicy).unwrap();
        assert_no_voluntary_history(&state);
        assert_eq!(state.pot_size(), chips(1.5 + 3.0 * ante + bba));
        assert_eq!(state.minimum_full_target(), chips(2.0));
        assert_eq!(state.amount_to_call(SeatId(0)), chips(1.0));
        play(&mut state, Move::RaiseTo(chips(2.0)));
        assert_eq!(state.minimum_full_target(), chips(3.0));
        play(&mut state, Move::Call);
        play(&mut state, Move::Call);
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.minimum_full_target(), chips(1.0));
    }
}

#[test]
fn aggressors_track_each_street_and_checked_through_street_clears_history() {
    let mut state = state(&[100.0; 2]);
    play(&mut state, Move::RaiseTo(chips(3.0))); // BTN
    assert_eq!(state.last_street_aggressor, Some(SeatId(0)));
    play(&mut state, Move::Call);
    assert_eq!(state.street, Street::Flop);
    assert_eq!(state.previous_street_aggressor, Some(SeatId(0)));
    assert_eq!(state.last_street_aggressor, None);
    play(&mut state, Move::BetTo(chips(1.0))); // BB
    play(&mut state, Move::RaiseTo(chips(3.0))); // BTN
    play(&mut state, Move::RaiseTo(chips(5.0))); // BB
    assert_eq!(state.last_street_aggressor, Some(SeatId(1)));
    assert_eq!(state.previous_street_aggressor, Some(SeatId(0)));
    let encoded = serde_json::to_value(&state).unwrap();
    assert_eq!(
        serde_json::from_value::<BettingState>(encoded).unwrap(),
        state
    );
    play(&mut state, Move::Call);
    assert_eq!(state.street, Street::Turn);
    assert_eq!(state.previous_street_aggressor, Some(SeatId(1)));
    assert_eq!(state.last_street_aggressor, None);
    play(&mut state, Move::Check);
    play(&mut state, Move::Check);
    assert_eq!(state.street, Street::River);
    assert_eq!(state.previous_street_aggressor, None);
    assert_eq!(state.last_street_aggressor, None);
    assert_eq!(state.last_preflop_aggressor, Some(SeatId(0)));
}
