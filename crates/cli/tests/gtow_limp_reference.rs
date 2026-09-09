use std::path::{Path, PathBuf};

use cli::config::GameSection;
use multiway::{Action, BettingState, SeatId};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/bench_multiway/6max_100bb_nl50_partial_reference_limp.toml")
}

fn fixture() -> (multiway::config::ValidatedMultiwayConfig, BettingState) {
    let path = fixture_path();
    let raw = std::fs::read_to_string(&path).expect("reading limp fixture");
    let lowered = cli::multiway_v1::parse_and_lower_at(&raw, &path).expect("lowering limp fixture");
    cli::multiway_v1::validate_production_contract_at(&raw, &path)
        .expect("production contract for limp fixture");
    let GameSection::PreflopMultiway(game) = lowered.game else {
        panic!("fixture must lower to multiway game");
    };
    let validated = game.validated().expect("validating limp fixture");
    let state = BettingState::new(&validated).expect("building root state");
    (validated, state)
}

fn apply(state: &mut BettingState, config: &multiway::BettingConfig, action: Action) {
    state.apply(action, config).expect("fixture action applies");
}

fn raise(state: &BettingState, config: &multiway::BettingConfig, bb: f64) -> Action {
    let target = (bb * 1000.0).round() as u64;
    state.legal_actions(config).unwrap().into_iter().find(|action| {
        matches!(action, Action::RaiseTo { to, all_in: false, .. } if to.raw() == target)
    }).unwrap_or_else(|| panic!("raise to {bb}bb missing"))
}

fn has_raise(state: &BettingState, config: &multiway::BettingConfig, bb: f64) -> bool {
    let target = (bb * 1000.0).round() as u64;
    state.legal_actions(config).unwrap().iter().any(
        |action| matches!(action, Action::RaiseTo { to, all_in: false, .. } if to.raw() == target),
    )
}

fn has_jam(state: &BettingState, config: &multiway::BettingConfig) -> bool {
    state
        .legal_actions(config)
        .unwrap()
        .iter()
        .any(|action| matches!(action, Action::RaiseTo { all_in: true, .. }))
}

fn has_fold(state: &BettingState, config: &multiway::BettingConfig) -> bool {
    state
        .legal_actions(config)
        .unwrap()
        .iter()
        .any(|action| matches!(action, Action::Fold))
}

fn has_check(state: &BettingState, config: &multiway::BettingConfig) -> bool {
    state
        .legal_actions(config)
        .unwrap()
        .iter()
        .any(|action| matches!(action, Action::Check))
}

fn assert_bb_limp_menu(state: &BettingState, config: &multiway::BettingConfig) {
    assert_eq!(state.legal_actions(config).unwrap().len(), 4);
    assert!(has_check(state, config));
    assert!(has_raise(state, config, 3.0));
    assert!(has_raise(state, config, 5.0));
    assert!(has_jam(state, config));
    assert!(!has_raise(state, config, 4.0));
}

fn assert_sb_iso_menu(state: &BettingState, config: &multiway::BettingConfig, target: f64) {
    assert_eq!(state.legal_actions(config).unwrap().len(), 4);
    assert!(has_fold(state, config));
    assert!(
        state
            .legal_actions(config)
            .unwrap()
            .iter()
            .any(|action| matches!(action, Action::Call { .. }))
    );
    assert!(has_raise(state, config, target));
    assert!(has_jam(state, config));
    assert!(!has_raise(state, config, 4.0));
    assert!(!has_raise(state, config, 7.5));
    assert!(!has_raise(state, config, 12.5));
}

fn fold(state: &BettingState, config: &multiway::BettingConfig) -> Action {
    state
        .legal_actions(config)
        .unwrap()
        .into_iter()
        .find(|action| matches!(action, Action::Fold))
        .unwrap()
}

#[test]
fn limp_fixture_has_bb_three_and_five_iso_menus_and_sb_responses() {
    let (config, mut state) = fixture();
    let betting = &config.betting;
    for seat in [3, 4, 5, 0] {
        assert_eq!(state.to_act, Some(SeatId::new_unchecked(seat)));
        let action = fold(&state, betting);
        apply(&mut state, betting, action);
    }
    assert_eq!(state.to_act, Some(SeatId::new_unchecked(1)));
    let limp = state
        .legal_actions(betting)
        .unwrap()
        .into_iter()
        .find(|a| matches!(a, Action::Call { .. }))
        .unwrap();
    apply(&mut state, betting, limp);
    assert_eq!(state.to_act, Some(SeatId::new_unchecked(2)));
    assert_eq!(
        state.seats[SeatId::new_unchecked(1)].remaining.raw(),
        99_000
    );
    assert_eq!(
        state.seats[SeatId::new_unchecked(2)].remaining.raw(),
        99_000
    );
    assert_eq!(state.pot_size().raw(), 2_000);
    assert_bb_limp_menu(&state, betting);

    let mut three = state.clone();
    let three_bet = raise(&three, betting, 3.0);
    apply(&mut three, betting, three_bet);
    assert_eq!(three.to_act, Some(SeatId::new_unchecked(1)));
    assert_eq!(three.pot_size().raw(), 4_000);
    assert_eq!(
        three.seats[SeatId::new_unchecked(1)].remaining.raw(),
        99_000
    );
    assert_eq!(
        three.seats[SeatId::new_unchecked(2)].remaining.raw(),
        97_000
    );
    assert_sb_iso_menu(&three, betting, 14.0);

    let mut five = state;
    let five_bet = raise(&five, betting, 5.0);
    apply(&mut five, betting, five_bet);
    assert_eq!(five.to_act, Some(SeatId::new_unchecked(1)));
    assert_eq!(five.pot_size().raw(), 6_000);
    assert_eq!(five.seats[SeatId::new_unchecked(1)].remaining.raw(), 99_000);
    assert_eq!(five.seats[SeatId::new_unchecked(2)].remaining.raw(), 95_000);
    assert_sb_iso_menu(&five, betting, 18.0);
}

#[test]
fn limp_fixture_jam_has_no_nonjam_iso_and_open_branch_is_unchanged() {
    let (config, root) = fixture();
    let betting = &config.betting;
    let mut state = root.clone();
    for _seat in [3, 4, 5, 0] {
        let action = fold(&state, betting);
        apply(&mut state, betting, action);
    }
    let limp = state
        .legal_actions(betting)
        .unwrap()
        .into_iter()
        .find(|a| matches!(a, Action::Call { .. }))
        .unwrap();
    apply(&mut state, betting, limp);
    let jam = state
        .legal_actions(betting)
        .unwrap()
        .into_iter()
        .find(|a| matches!(a, Action::RaiseTo { all_in: true, .. }))
        .unwrap();
    apply(&mut state, betting, jam);
    assert_eq!(state.to_act, Some(SeatId::new_unchecked(1)));
    assert!(!has_raise(&state, betting, 14.0));
    assert!(!has_raise(&state, betting, 18.0));

    let mut open = root;
    for _seat in [3, 4, 5, 0] {
        let action = fold(&open, betting);
        apply(&mut open, betting, action);
    }
    let sb_open = raise(&open, betting, 3.0);
    apply(&mut open, betting, sb_open);
    let bb_three_bet = raise(&open, betting, 10.0);
    apply(&mut open, betting, bb_three_bet);
    assert_eq!(open.to_act, Some(SeatId::new_unchecked(1)));
    let sb_four_bet = raise(&open, betting, 21.0);
    assert!(matches!(sb_four_bet, Action::RaiseTo { .. }));
}
