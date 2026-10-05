//! P2 action-menu and street-transition policy over the shared NLH state.
use crate::config::{BettingConfig, RuleAction, RuleEffect, SizeSpec, ValidatedMultiwayConfig};
use crate::types::{MwChips, SeatId, SeatVec, Street};
pub use nlh::betting::{Action, BettingError, BettingState, HandPhase, SeatState, SeatStatus};
use nlh::{StreetPolicy, TableSetup};

impl StreetPolicy for BettingConfig {
    fn check_down(&self, state: &BettingState, actor: SeatId) -> bool {
        self.rules.iter().any(|rule| {
            rule.effect == RuleEffect::Checkdown && crate::tree_rules::matches(rule, state, actor)
        })
    }
    fn skip_street(&self, street: Street, players: u8) -> bool {
        self.for_street(street)
            .max_betting_players
            .is_some_and(|max| players > max)
    }
}

/// P2 menu expansion and checked application of menu actions.
pub trait BettingMenu: Sized {
    fn from_config(config: &ValidatedMultiwayConfig) -> Result<Self, BettingError>;
    fn legal_actions(&self, config: &BettingConfig) -> Result<Vec<Action>, BettingError>;
    fn apply(&mut self, action: Action, config: &BettingConfig) -> Result<(), BettingError>;
    /// `actions` must be the exact menu for this state, not a stale list.
    /// The transition policy must agree with the validated table policy.
    fn apply_from_actions(
        &mut self,
        action: Action,
        actions: &[Action],
        betting: &BettingConfig,
    ) -> Result<(), BettingError>;
}

impl BettingMenu for BettingState {
    fn from_config(config: &ValidatedMultiwayConfig) -> Result<Self, BettingError> {
        // Construct once at the root, never during traversal transitions.
        let setup = TableSetup {
            button: config.button,
            starting_stacks: SeatVec::new_unchecked(
                config
                    .seats
                    .iter()
                    .map(|seat| seat.starting_stack)
                    .collect(),
            ),
            forced_antes: config.forced_antes.clone(),
            common_ante: config.common_ante,
            forced_blinds: config.forced_blinds.clone(),
            nominal_big_blind: config.nominal_big_blind,
            preflop_first_to_act: config.preflop_first_to_act,
        };
        Self::new(&setup, &config.betting)
    }
    fn legal_actions(&self, config: &BettingConfig) -> Result<Vec<Action>, BettingError> {
        if self.phase != HandPhase::Betting {
            return Ok(Vec::new());
        }
        let actor = self.actor()?;
        let street_config = config.for_street(self.street);
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
                actions.push(self.call_action(actor));
            }
        }

        let maximum = self.maximum_target(actor);
        if !self.can_raise(actor) || self.aggressive_actions >= street_config.max_aggressive_actions
        {
            return apply_tree_rules(self, config, actor, actions);
        }

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
        let mut targets = Vec::with_capacity(sizes.len() + 1);
        for size in sizes {
            let mut target = self.resolve_size(actor, size);
            if self.street == Street::Preflop
                && self.aggressive_actions > 0
                && !matches!(size, &SizeSpec::AllIn)
                && street_config
                    .reraise_jam_above_actor_starting_stack
                    .is_some_and(|ratio| {
                        u128::from(target.raw()) * u128::from(ratio.denominator)
                            > u128::from(self.seats[actor].starting_stack.raw())
                                * u128::from(ratio.numerator)
                    })
            {
                target = maximum;
            }
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
            if let Some(action) = self.action_for_target(actor, target) {
                actions.push(action);
            }
        }
        apply_tree_rules(self, config, actor, actions)
    }
    fn apply(&mut self, action: Action, config: &BettingConfig) -> Result<(), BettingError> {
        let actions = self.legal_actions(config)?;
        self.apply_from_actions(action, &actions, config)
    }
    fn apply_from_actions(
        &mut self,
        action: Action,
        actions: &[Action],
        betting: &BettingConfig,
    ) -> Result<(), BettingError> {
        let actor = self.to_act.ok_or(BettingError::MissingActor)?;
        if !actions.contains(&action) {
            return Err(BettingError::IllegalAction { actor, action });
        }
        self.apply_action(action, betting)
    }
}

fn apply_tree_rules(
    state: &BettingState,
    config: &BettingConfig,
    actor: SeatId,
    mut actions: Vec<Action>,
) -> Result<Vec<Action>, BettingError> {
    let mut rules = config.rules.iter().collect::<Vec<_>>();
    rules.sort_by_key(|rule| (rule.priority, rule.source_order));
    for rule in rules {
        if !crate::tree_rules::matches(rule, state, actor) {
            continue;
        }
        if rule.effect == RuleEffect::Checkdown {
            actions.retain(|action| matches!(action, Action::Check));
            continue;
        }
        let action_kind = rule
            .action
            .ok_or_else(|| BettingError::TreeRule("non-checkdown rule requires action".into()))?;
        let candidates = rule_candidates(state, config, action_kind, &rule.sizes)?;
        match rule.effect {
            RuleEffect::Add => actions.extend(candidates),
            RuleEffect::Remove => actions.retain(|action| !action_matches(action, action_kind)),
            RuleEffect::Replace => {
                actions.retain(|action| !action_matches(action, action_kind));
                actions.extend(candidates);
            }
            RuleEffect::Force => actions = candidates,
            RuleEffect::Checkdown => unreachable!(),
        }
        actions.sort_by_key(action_sort_key);
        actions.dedup();
    }
    Ok(actions)
}

fn rule_candidates(
    state: &BettingState,
    config: &BettingConfig,
    action_kind: RuleAction,
    sizes: &[SizeSpec],
) -> Result<Vec<Action>, BettingError> {
    if !matches!(action_kind, RuleAction::Bet | RuleAction::Raise) {
        let mut base = config.clone();
        base.rules.clear();
        return Ok(state
            .legal_actions(&base)?
            .into_iter()
            .filter(|action| action_matches(action, action_kind))
            .collect());
    }

    let mut scoped = config.clone();
    scoped.rules.clear();
    let street = match state.street {
        Street::Preflop => &mut scoped.preflop,
        Street::Flop => &mut scoped.flop,
        Street::Turn => &mut scoped.turn,
        Street::River => &mut scoped.river,
    };
    street.include_allin = false;
    if state.aggressive_actions == 0 {
        street.bet_sizes = sizes.to_vec();
        street.isolate_sizes = Some(sizes.to_vec());
    } else {
        street.raise_sizes = sizes.to_vec();
    }
    Ok(state
        .legal_actions(&scoped)?
        .into_iter()
        .filter(|action| action_matches(action, action_kind))
        .collect())
}

fn action_matches(action: &Action, kind: RuleAction) -> bool {
    matches!(
        (action, kind),
        (Action::Fold, RuleAction::Fold)
            | (Action::Check, RuleAction::Check)
            | (Action::Call { .. }, RuleAction::Call)
            | (Action::BetTo { .. }, RuleAction::Bet)
            | (Action::RaiseTo { .. }, RuleAction::Raise)
    )
}

fn action_sort_key(action: &Action) -> (u8, u64) {
    match action {
        Action::Fold => (0, 0),
        Action::Check => (1, 0),
        Action::Call { amount, .. } => (2, amount.raw()),
        Action::BetTo { to, .. } => (3, to.raw()),
        Action::RaiseTo { to, .. } => (4, to.raw()),
    }
}

fn scale(amount: MwChips, factor: f64) -> MwChips {
    let scaled = (amount.raw() as f64 * factor).round();
    MwChips(scaled.clamp(0.0, u64::MAX as f64) as u64)
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
            forced_bets: None,
            abstraction: AbstractionConfig::default(),
        }
        .validated()
        .unwrap();
        let betting = config.betting.clone();
        (BettingState::from_config(&config).unwrap(), betting)
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
        let all_in_raises = actions
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
            .count();
        assert_eq!(all_in_raises, 1);
        assert!(!actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(90_000),
                ..
            }
        )));
    }

    #[test]
    fn exact_reraise_jam_ratio_is_strict_at_one_third_and_deduplicates_all_in() {
        let actions_for = |raise_to_bb: f64| {
            let (mut state, mut betting) = state(&[99.0, 99.0, 99.0], 0, AnteConfig::None);
            betting.preflop.bet_sizes = vec![crate::config::SizeSpec::ToBb { value: 3.0 }];
            betting.preflop.raise_sizes =
                vec![crate::config::SizeSpec::ToBb { value: raise_to_bb }];
            betting.preflop.include_allin = true;
            betting.preflop.reraise_jam_above_actor_starting_stack =
                Some(crate::config::StackRatio {
                    numerator: 1,
                    denominator: 3,
                });

            let open = state
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
            state.apply(open, &betting).unwrap();
            let call = state
                .legal_actions(&betting)
                .unwrap()
                .into_iter()
                .find(|action| matches!(action, Action::Call { .. }))
                .unwrap();
            state.apply(call, &betting).unwrap();
            assert_eq!(state.to_act, Some(SeatId(2)));
            state.legal_actions(&betting).unwrap()
        };

        for retained in [MwChips(32_999), MwChips(33_000)] {
            let actions = actions_for(retained.as_bb());
            assert!(actions.iter().any(|action| {
                matches!(
                    action,
                    Action::RaiseTo {
                        to,
                        all_in: false,
                        ..
                    } if *to == retained
                )
            }));
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| matches!(
                        action,
                        Action::RaiseTo {
                            to: MwChips(99_000),
                            all_in: true,
                            ..
                        }
                    ))
                    .count(),
                1
            );
        }

        let above = actions_for(33.001);
        assert!(!above.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(33_001),
                ..
            }
        )));
        assert_eq!(
            above
                .iter()
                .filter(|action| matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(99_000),
                        all_in: true,
                        ..
                    }
                ))
                .count(),
            1
        );

        // The same threshold never changes an opening size, even when that
        // open is above one third of the starting stack.
        let (opening_state, mut opening_betting) = state(&[99.0, 99.0, 99.0], 0, AnteConfig::None);
        opening_betting.preflop.bet_sizes = vec![crate::config::SizeSpec::ToBb { value: 40.0 }];
        opening_betting.preflop.include_allin = false;
        opening_betting
            .preflop
            .reraise_jam_above_actor_starting_stack = Some(crate::config::StackRatio {
            numerator: 1,
            denominator: 3,
        });
        assert!(
            opening_state
                .legal_actions(&opening_betting)
                .unwrap()
                .iter()
                .any(|action| matches!(
                    action,
                    Action::RaiseTo {
                        to: MwChips(40_000),
                        all_in: false,
                        ..
                    }
                ))
        );
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

    #[test]
    fn max_betting_players_collapses_every_postflop_street_when_persistently_exceeded() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        // All three postflop streets cap betting at two players; three
        // players see the flop and check-down can never fold anyone, so
        // every later street re-checks the same losing count and collapses
        // too, all the way to showdown.
        betting.flop.max_betting_players = Some(2);
        betting.turn.max_betting_players = Some(2);
        betting.river.max_betting_players = Some(2);

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
        // The big blind's option check is the last preflop action; it must
        // fast-forward straight to showdown with no postflop decision node
        // ever created.
        state.apply(Action::Check, &betting).unwrap();

        assert_eq!(state.phase, HandPhase::Runout);
        assert_eq!(state.to_act, None);
        assert_eq!(state.street, Street::River);
        assert_eq!(state.players_on_street(Street::Flop), 3);
        assert_eq!(state.players_on_street(Street::Turn), 3);
        assert_eq!(state.players_on_street(Street::River), 3);
        assert!(state.legal_actions(&betting).unwrap().is_empty());
    }

    #[test]
    fn folds_during_flop_betting_can_reenable_a_stricter_turn_threshold() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0, 20.0], 0, AnteConfig::None);
        // Four players is exactly the flop's cap (betting happens); the turn
        // has a stricter cap that only two folds during the flop can satisfy.
        betting.flop.max_betting_players = Some(4);
        betting.turn.max_betting_players = Some(2);

        // Preflop: everyone limps/checks to see a 4-way flop.
        for _ in 0..3 {
            let call = state
                .legal_actions(&betting)
                .unwrap()
                .into_iter()
                .find(|action| matches!(action, Action::Call { .. }))
                .unwrap();
            state.apply(call, &betting).unwrap();
        }
        state.apply(Action::Check, &betting).unwrap();
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.phase, HandPhase::Betting);
        assert_eq!(state.players_on_street(Street::Flop), 4);

        // Flop: first actor bets, the next two fold to it, the last calls,
        // leaving exactly two non-folded players.
        let bet = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(Action::is_aggressive)
            .unwrap();
        state.apply(bet, &betting).unwrap();
        state.apply(Action::Fold, &betting).unwrap();
        state.apply(Action::Fold, &betting).unwrap();
        let call = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|action| matches!(action, Action::Call { .. }))
            .unwrap();
        state.apply(call, &betting).unwrap();

        // The turn re-evaluates its own threshold against the post-fold
        // count (2), which no longer exceeds it, so the turn has betting.
        assert_eq!(state.street, Street::Turn);
        assert_eq!(state.players_on_street(Street::Turn), 2);
        assert_eq!(state.phase, HandPhase::Betting);
        assert!(state.to_act.is_some());
        assert!(
            state
                .legal_actions(&betting)
                .unwrap()
                .iter()
                .any(|action| matches!(action, Action::Check))
        );
    }

    #[test]
    fn max_betting_players_boundary_equal_count_still_gets_betting() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        betting.flop.max_betting_players = Some(2);

        // UTG folds preflop; SB calls, BB checks its option: exactly two
        // players reach the flop, exactly matching (not exceeding) the
        // threshold, so the flop still gets betting.
        state.apply(Action::Fold, &betting).unwrap();
        let call = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|action| matches!(action, Action::Call { .. }))
            .unwrap();
        state.apply(call, &betting).unwrap();
        state.apply(Action::Check, &betting).unwrap();

        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.players_on_street(Street::Flop), 2);
        assert_eq!(state.phase, HandPhase::Betting);
        assert!(state.to_act.is_some());
    }

    #[test]
    fn all_in_seat_counts_toward_the_checkdown_threshold() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 0.7], 0, AnteConfig::None);
        assert_eq!(state.seats[SeatId(2)].status, SeatStatus::AllIn);
        betting.flop.max_betting_players = Some(2);
        betting.turn.max_betting_players = Some(2);
        betting.river.max_betting_players = Some(2);

        // Both non-all-in seats call to see the flop: three seats remain in
        // the hand (two active, one all-in), so the flop's threshold of two
        // is exceeded even though only two players can still act.
        for _ in 0..2 {
            let call = state
                .legal_actions(&betting)
                .unwrap()
                .into_iter()
                .find(|action| matches!(action, Action::Call { .. }))
                .unwrap();
            state.apply(call, &betting).unwrap();
        }

        assert_eq!(state.phase, HandPhase::Runout);
        assert_eq!(state.players_on_street(Street::Flop), 3);
    }
    #[test]
    fn tree_checkdown_skips_a_matching_street_without_decision_nodes() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        betting.rules.push(crate::config::TreeRule::new(
            100,
            0,
            crate::config::RuleStreet::Flop,
            "players >= 3".into(),
            crate::config::RuleEffect::Checkdown,
            None,
            Vec::new(),
        ));
        while state.street == Street::Preflop {
            let actions = state.legal_actions(&betting).unwrap();
            let action = actions
                .into_iter()
                .find(|action| matches!(action, Action::Call { .. } | Action::Check))
                .unwrap();
            state.apply(action, &betting).unwrap();
        }
        assert_eq!(state.street, Street::Turn);
        assert_eq!(state.phase, HandPhase::Betting);
    }
}
