//! P2 action-menu and street-transition policy over the shared NLH state.
use crate::config::{BettingConfig, RuleAction, RuleEffect, SizeSpec, ValidatedMultiwayConfig};
use crate::types::{MwChips, SeatId, SeatVec};
pub use nlh::betting::{Action, BettingError, BettingState, HandPhase, SeatState, SeatStatus};
use nlh::{StreetPolicy, TableSetup};

impl StreetPolicy for BettingConfig {
    fn forced_action(&self, state: &BettingState, actor: SeatId) -> Option<Action> {
        // Scan once, evaluating conditions only for checkdown rules until one
        // matches. A later matching edit requires the ordinary final menu.
        let mut checkdown = false;
        let mut later_edit = false;
        for rule in &self.nlh_rules {
            if (checkdown || rule.effect == RuleEffect::Checkdown) && rule.matches(state, actor) {
                if rule.effect == RuleEffect::Checkdown {
                    checkdown = true;
                    later_edit = false;
                } else {
                    later_edit = true;
                }
            }
        }
        if !checkdown {
            return None;
        }
        let action = check_or_fold(state, actor);
        if !later_edit
            || state
                .legal_actions(self)
                .is_ok_and(|menu| menu == [action.clone()])
        {
            Some(action)
        } else {
            None
        }
    }
    fn skip_street(&self, _street: crate::types::Street, _players: u8) -> bool {
        false
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
        from_config_with_policy(config, &config.betting)
    }
    fn legal_actions(&self, config: &BettingConfig) -> Result<Vec<Action>, BettingError> {
        if self.phase != HandPhase::Betting {
            return Ok(Vec::new());
        }
        let actor = self.actor()?;
        let to_call = self.amount_to_call(actor);
        let mut actions = Vec::new();

        if to_call == MwChips::ZERO {
            actions.push(Action::Check);
        } else {
            actions.push(Action::Fold);
            actions.push(self.call_action(actor));
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

pub(crate) fn from_config_with_policy<P: StreetPolicy>(
    config: &ValidatedMultiwayConfig,
    policy: &P,
) -> Result<BettingState, BettingError> {
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
        straddles: config.straddles.clone(),
        nominal_big_blind: config.nominal_big_blind,
        preflop_first_to_act: config.preflop_first_to_act,
        chip_rounding: config.chip_rounding,
    };
    BettingState::new(&setup, policy)
}

fn check_or_fold(state: &BettingState, actor: SeatId) -> Action {
    if state.amount_to_call(actor) == MwChips::ZERO {
        Action::Check
    } else {
        Action::Fold
    }
}

fn apply_tree_rules(
    state: &BettingState,
    config: &BettingConfig,
    actor: SeatId,
    mut actions: Vec<Action>,
) -> Result<Vec<Action>, BettingError> {
    let matched = config
        .nlh_rules
        .iter()
        .filter(|rule| rule.matches(state, actor))
        .map(|rule| (rule.effect, rule.action, rule.sizes.as_slice()));
    for (effect, action, sizes) in matched {
        if effect == RuleEffect::Checkdown {
            actions = vec![check_or_fold(state, actor)];
            continue;
        }
        let action_kind = action
            .ok_or_else(|| BettingError::TreeRule("non-checkdown rule requires action".into()))?;
        match effect {
            RuleEffect::Jam(bound) => jam(state, actor, &mut actions, action_kind, bound, sizes)?,
            RuleEffect::Merge => merge(state, actor, &mut actions, action_kind, sizes)?,
            _ => {
                let candidates = rule_candidates(state, actor, action_kind, sizes);
                match effect {
                    RuleEffect::Add => actions.extend(candidates),
                    RuleEffect::Remove => {
                        actions.retain(|action| !action_matches(action, action_kind))
                    }
                    RuleEffect::Replace => {
                        actions.retain(|action| !action_matches(action, action_kind));
                        actions.extend(candidates);
                    }
                    RuleEffect::Force => actions = candidates,
                    RuleEffect::Checkdown | RuleEffect::Jam(_) | RuleEffect::Merge => {
                        unreachable!()
                    }
                }
            }
        }
        actions.sort_by_key(action_sort_key);
        actions.dedup();
    }
    Ok(actions)
}

/// The `jam` effect: every current candidate of `action_kind` whose target
/// passes `bound` against the threshold becomes the all-in target.
fn jam(
    state: &BettingState,
    actor: SeatId,
    actions: &mut [Action],
    action_kind: RuleAction,
    bound: nlh::script::JamBound,
    sizes: &[SizeSpec],
) -> Result<(), BettingError> {
    let [threshold] = sizes else {
        return Err(BettingError::TreeRule(
            "jam requires exactly one threshold".into(),
        ));
    };
    let threshold = state.jam_threshold(actor, threshold);
    let all_in = state.maximum_target(actor);
    for action in actions.iter_mut() {
        if action_matches(action, action_kind)
            && let Some(to) = action.amount()
            && bound.holds(to, threshold)
            && let Some(jammed) = state.action_for_target(actor, all_in)
        {
            *action = jammed;
        }
    }
    Ok(())
}

/// The `merge` effect: walking the current `action_kind` candidates from the
/// largest target down, drop each one within the width of the last kept one.
fn merge(
    state: &BettingState,
    actor: SeatId,
    actions: &mut Vec<Action>,
    action_kind: RuleAction,
    sizes: &[SizeSpec],
) -> Result<(), BettingError> {
    let [width] = sizes else {
        return Err(BettingError::TreeRule(
            "merge requires exactly one width".into(),
        ));
    };
    let width = state.merge_width(actor, width).raw();
    let mut kept: Vec<u64> = actions
        .iter()
        .filter(|action| action_matches(action, action_kind))
        .filter_map(|action| action.amount().map(MwChips::raw))
        .collect();
    nlh::merge_targets(&mut kept, width);
    actions.retain(|action| {
        !action_matches(action, action_kind)
            || action.amount().is_none_or(|to| kept.contains(&to.raw()))
    });
    Ok(())
}

fn rule_candidates(
    state: &BettingState,
    actor: SeatId,
    action_kind: RuleAction,
    sizes: &[SizeSpec],
) -> Vec<Action> {
    match action_kind {
        RuleAction::Fold if state.amount_to_call(actor) != MwChips::ZERO => vec![Action::Fold],
        RuleAction::Call if state.amount_to_call(actor) != MwChips::ZERO => {
            vec![state.call_action(actor)]
        }
        RuleAction::Check if state.amount_to_call(actor) == MwChips::ZERO => vec![Action::Check],
        RuleAction::Bet | RuleAction::Raise if state.can_raise(actor) => sizes
            .iter()
            .filter_map(|size| {
                let target = state.resolve_size(actor, size);
                (target > state.bet_to_match)
                    .then(|| state.action_for_target(actor, target))
                    .flatten()
            })
            .filter(|action| action_matches(action, action_kind))
            .collect(),
        _ => Vec::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AbstractionConfig, AnteConfig, BlindConfig, MultiwayConfig, SeatConfig};
    use crate::types::Street;

    fn state(stacks: &[f64], button: u8, ante: AnteConfig) -> (BettingState, BettingConfig) {
        let config = MultiwayConfig {
            seats: stacks
                .iter()
                .map(|&stack_bb| SeatConfig {
                    name: None,
                    stack_chips: nlh::ChipScale::DEFAULT.from_bb(stack_bb).unwrap(),
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
            bb_chips: 1000,
            chip_rounding: nlh::Rounding::Nearest,
        }
        .validated()
        .unwrap();
        let betting = config.betting.clone();
        (BettingState::from_config(&config).unwrap(), betting)
    }

    fn with_script(betting: &mut BettingConfig, script: &str) {
        betting.nlh_rules =
            nlh::script::Script::compile(script, &Default::default(), &spot::NLH_V1)
                .unwrap()
                .rules
                .into_iter()
                .map(crate::tree_rules::NlhTreeRule::from_compiled)
                .collect();
        for rule in &betting.nlh_rules {
            rule.validate().unwrap();
        }
    }

    #[test]
    fn baseline_is_passive_and_script_adds_wagers() {
        let (state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        assert!(
            !state
                .legal_actions(&betting)
                .unwrap()
                .iter()
                .any(Action::is_aggressive)
        );
        with_script(&mut betting, "preflop { add raise [2.5bb] }");

        let opening_actions = state.legal_actions(&betting).unwrap();
        assert!(opening_actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(2_500),
                ..
            }
        )));
    }

    #[test]
    fn passive_rule_candidates_can_restore_removed_actions() {
        let (state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        with_script(&mut betting, "preflop { remove call add call [] }");
        assert!(
            state
                .legal_actions(&betting)
                .unwrap()
                .iter()
                .any(|action| matches!(action, Action::Call { .. }))
        );
    }

    #[test]
    fn preflop_open_sizes_are_scripted() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        with_script(&mut betting, "preflop { add raise [2.5bb, 4bb] }");
        let opening_actions = state.legal_actions(&betting).unwrap();
        assert!(opening_actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(2_500),
                ..
            }
        )));
        let limp = opening_actions
            .into_iter()
            .find(|action| matches!(action, Action::Call { .. }))
            .unwrap();
        state.apply(limp, &betting).unwrap();
        let isolation_actions = state.legal_actions(&betting).unwrap();
        assert!(isolation_actions.iter().any(|action| matches!(
            action,
            Action::RaiseTo {
                to: MwChips(4_000),
                ..
            }
        )));
    }

    fn state_with_bb_facing_a_raise(betting: &mut BettingConfig) -> BettingState {
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
        with_script(
            &mut betting,
            "preflop { add raise [3bb] } preflop when aggressions >= 1 { replace raise [min, 3x] }",
        );
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
        with_script(
            &mut betting,
            "preflop { add raise [3bb] } preflop when aggressions >= 1 { replace raise [25%stack, 50%stack] }",
        );
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
    fn jam_raise_above_a_third_of_the_stack_is_strict_and_deduplicates_all_in() {
        let actions_for = |raise_to_bb: f64, script: &str| {
            let (mut state, mut betting) = state(&[99.0, 99.0, 99.0], 0, AnteConfig::None);
            with_script(
                &mut betting,
                &format!(
                    "preflop {{ add raise [3bb, a] }} preflop when aggressions >= 1 {{ replace raise [{raise_to_bb}bb, a] }} {script}"
                ),
            );

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
        let raise_to = |actions: &[Action], to: MwChips, all_in: bool| {
            actions
                .iter()
                .filter(|action| {
                    matches!(
                        action,
                        Action::RaiseTo { to: found, all_in: found_all_in, .. }
                            if *found == to && *found_all_in == all_in
                    )
                })
                .count()
        };
        // BB's M is 99 BB, so `33.333%stack` resolves to exactly 33 BB.
        let above = "preflop when aggressions >= 1 { jam raise > 33.333%stack }";
        for retained in [MwChips(32_999), MwChips(33_000)] {
            let actions = actions_for(nlh::ChipScale::DEFAULT.to_bb(retained), above);
            assert_eq!(raise_to(&actions, retained, false), 1);
            assert_eq!(raise_to(&actions, MwChips(99_000), true), 1);
        }
        let jammed = actions_for(33.001, above);
        assert_eq!(raise_to(&jammed, MwChips(33_001), false), 0);
        assert_eq!(raise_to(&jammed, MwChips(99_000), true), 1);

        let at_least = "preflop when aggressions >= 1 { jam raise >= 33.333%stack }";
        let jammed = actions_for(33.0, at_least);
        assert_eq!(raise_to(&jammed, MwChips(33_000), false), 0);
        assert_eq!(raise_to(&jammed, MwChips(99_000), true), 1);

        // The condition keeps the threshold away from an opening size, even
        // when that open is above one third of the stack.
        let (opening_state, mut opening_betting) = state(&[99.0, 99.0, 99.0], 0, AnteConfig::None);
        with_script(
            &mut opening_betting,
            &format!("preflop {{ add raise [40bb] }} {above}"),
        );
        let opening = opening_state.legal_actions(&opening_betting).unwrap();
        assert_eq!(raise_to(&opening, MwChips(40_000), false), 1);
    }

    #[test]
    fn merge_raise_keeps_the_larger_target_with_a_pot_or_bb_width() {
        let raises_for = |script: &str| {
            let (state, mut betting) = state(&[99.0, 99.0, 99.0], 0, AnteConfig::None);
            with_script(
                &mut betting,
                &format!("preflop {{ add raise [2bb, 2.5bb, 3bb, a] }} {script}"),
            );
            let actions = state.legal_actions(&betting).unwrap();
            assert!(actions.contains(&Action::Fold));
            actions
                .iter()
                .filter_map(|action| match action {
                    Action::RaiseTo { to, .. } => Some(to.raw()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(raises_for(""), [2_000, 2_500, 3_000, 99_000]);
        // 2.5 is within 0.5 BB of the kept 3 (inclusive); 2 is 1 BB below it.
        assert_eq!(
            raises_for("preflop { merge raise within 0.5bb }"),
            [2_000, 3_000, 99_000]
        );
        // The pot after the 1 BB call is 2.5 BB, so 20% of it is 0.5 BB.
        assert_eq!(
            raises_for("preflop { merge raise within 20% }"),
            [2_000, 3_000, 99_000]
        );
        assert_eq!(
            raises_for("preflop { merge raise within 100%stack }"),
            [99_000]
        );
        assert_eq!(
            raises_for("preflop { merge bet within 100%stack }"),
            [2_000, 2_500, 3_000, 99_000]
        );
    }

    #[test]
    fn tree_checkdown_skips_a_matching_street_without_decision_nodes() {
        let (mut state, mut betting) = state(&[20.0, 20.0, 20.0], 0, AnteConfig::None);
        betting.nlh_rules = nlh::script::Script::compile(
            "flop when players >= 3 { checkdown }",
            &Default::default(),
            &spot::NLH_V1,
        )
        .unwrap()
        .rules
        .into_iter()
        .map(crate::tree_rules::NlhTreeRule::from_compiled)
        .collect();
        for rule in &betting.nlh_rules {
            rule.validate().unwrap();
        }
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

    fn flop(script: &str, players: usize) -> (BettingState, BettingConfig) {
        let (mut state, mut betting) = state(&vec![20.0; players], 0, AnteConfig::None);
        betting.nlh_rules =
            nlh::script::Script::compile(script, &Default::default(), &spot::NLH_V1)
                .unwrap()
                .rules
                .into_iter()
                .map(crate::tree_rules::NlhTreeRule::from_compiled)
                .collect();
        while state.street == Street::Preflop {
            let action = state
                .legal_actions(&betting)
                .unwrap()
                .into_iter()
                .find(|a| matches!(a, Action::Call { .. } | Action::Check))
                .unwrap();
            state.apply(action, &betting).unwrap();
        }
        (state, betting)
    }

    #[test]
    fn conditional_checkdown_checks_only_button_and_folds_facing_a_bet() {
        let (mut state, betting) = flop(
            "flop { add bet [1bb] } flop when position == BTN { checkdown }",
            3,
        );
        assert_eq!(state.to_act, Some(SeatId(1)));
        assert!(state.legal_actions(&betting).unwrap().len() > 1);
        state.apply(Action::Check, &betting).unwrap();
        assert_eq!(state.to_act, Some(SeatId(2)));
        assert!(state.legal_actions(&betting).unwrap().len() > 1);
        state.apply(Action::Check, &betting).unwrap();
        assert_eq!(state.street, Street::Turn); // BTN's check was elided.
        assert_eq!(state.seats[SeatId(0)].status, SeatStatus::Active);

        let (mut state, betting) = flop(
            "flop { add bet [1bb] } flop when position == BTN { checkdown }",
            3,
        );
        let bet = state
            .legal_actions(&betting)
            .unwrap()
            .into_iter()
            .find(|a| matches!(a, Action::BetTo { .. }))
            .unwrap();
        state.apply(bet, &betting).unwrap();
        assert_eq!(state.to_act, Some(SeatId(2)));
        state.apply(Action::Fold, &betting).unwrap();
        assert_eq!(state.seats[SeatId(0)].status, SeatStatus::Folded);
        assert_eq!(state.phase, HandPhase::Uncontested { winner: SeatId(1) });
        assert_eq!(state.to_act, None);
        assert!(state.pending.is_empty());
        assert_eq!(state.players_on_street(Street::Flop), 1);
    }

    #[test]
    fn later_matching_edits_decide_whether_checkdown_turn_is_elided() {
        let (mut state, betting) = flop("flop when position == BTN { checkdown add bet [1bb] }", 2);
        state.apply(Action::Check, &betting).unwrap();
        assert_eq!(state.street, Street::Flop);
        assert_eq!(state.to_act, Some(SeatId(0)));
        let menu = state.legal_actions(&betting).unwrap();
        assert!(matches!(
            menu.as_slice(),
            [
                Action::Check,
                Action::BetTo {
                    to: MwChips(1000),
                    ..
                }
            ]
        ));

        // Matching but inapplicable raise adds nothing: final check is forced.
        let (mut state, betting) =
            flop("flop when position == BTN { checkdown add raise [1bb] }", 2);
        state.apply(Action::Check, &betting).unwrap();
        assert_eq!(state.street, Street::Turn);
    }
}
