use std::path::{Path, PathBuf};

use cli::config::GameSection;
use multiway::{Action, BettingConfig, BettingState, SeatId};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/bench_multiway/6max_100bb_nl50_partial_reference.toml")
}

fn fixture() -> (multiway::config::ValidatedMultiwayConfig, BettingState) {
    let path = fixture_path();
    let raw = std::fs::read_to_string(&path).expect("reading tracked GTOW fixture");
    let lowered =
        cli::multiway_v1::parse_and_lower_at(&raw, &path).expect("lowering tracked GTOW fixture");
    let GameSection::PreflopMultiway(game) = lowered.game else {
        panic!("fixture must lower to the multiway game");
    };
    let validated = game.validated().expect("validating tracked GTOW fixture");
    let state = BettingState::new(&validated).expect("building root betting state");
    (validated, state)
}

fn legal(state: &BettingState, betting: &BettingConfig) -> Vec<Action> {
    state
        .legal_actions(betting)
        .expect("expanding legal actions")
}

fn has_call(state: &BettingState, betting: &BettingConfig) -> bool {
    legal(state, betting)
        .iter()
        .any(|action| matches!(action, Action::Call { .. }))
}

fn find_fold(state: &BettingState, betting: &BettingConfig) -> Action {
    legal(state, betting)
        .into_iter()
        .find(|action| matches!(action, Action::Fold))
        .expect("fold must be legal")
}

fn find_raise_to(state: &BettingState, betting: &BettingConfig, target_bb: f64) -> Action {
    let target = (target_bb * 1_000.0).round() as u64;
    legal(state, betting)
        .into_iter()
        .find(|action| {
            matches!(
                action,
                Action::RaiseTo { to, all_in: false, .. } if to.raw() == target
            )
        })
        .unwrap_or_else(|| panic!("raise to {target_bb}bb must be legal"))
}

fn find_jam(state: &BettingState, betting: &BettingConfig) -> Action {
    legal(state, betting)
        .into_iter()
        .find(|action| matches!(action, Action::RaiseTo { all_in: true, .. }))
        .expect("all-in raise must be legal")
}

fn apply(state: &mut BettingState, betting: &BettingConfig, action: Action) {
    state
        .apply(action, betting)
        .expect("fixture action must apply");
}

#[test]
fn tracked_fixture_keeps_four_raise_caps_and_256_postflop_buckets() {
    let (config, _) = fixture();
    assert_eq!(config.betting.preflop.max_aggressive_actions, 4);
    assert_eq!(config.betting.flop.max_aggressive_actions, 4);
    assert_eq!(config.betting.turn.max_aggressive_actions, 4);
    assert_eq!(config.betting.river.max_aggressive_actions, 4);
    assert_eq!(config.abstraction.flop_buckets, 256);
    assert_eq!(config.abstraction.turn_buckets, 256);
    assert_eq!(config.abstraction.river_buckets, 256);
}

#[test]
fn utg_open_hj_three_bet_has_the_observed_no_cold_call_menus() {
    let (config, mut state) = fixture();
    let betting = &config.betting;

    assert_eq!(state.to_act, Some(SeatId::new_unchecked(3))); // UTG
    let utg_open = find_raise_to(&state, betting, 2.0);
    apply(&mut state, betting, utg_open);

    assert_eq!(state.to_act, Some(SeatId::new_unchecked(4))); // HJ
    assert!(!has_call(&state, betting));
    let hj_three_bet = find_raise_to(&state, betting, 6.5);
    apply(&mut state, betting, hj_three_bet);

    for (seat, four_bet_bb) in [(5, 14.0), (0, 14.0), (1, 20.0), (2, 19.0)] {
        assert_eq!(state.to_act, Some(SeatId::new_unchecked(seat)));
        assert!(
            !has_call(&state, betting),
            "seat {seat} retained a cold call"
        );
        find_raise_to(&state, betting, four_bet_bb);
        let fold = find_fold(&state, betting);
        apply(&mut state, betting, fold);
    }

    assert_eq!(state.to_act, Some(SeatId::new_unchecked(3))); // original UTG opener
    assert!(has_call(&state, betting));
    find_raise_to(&state, betting, 19.0);
}

#[test]
fn jam_and_btn_blind_branches_retain_the_observed_calls() {
    let (config, root) = fixture();
    let betting = &config.betting;

    let mut utg_jam = root.clone();
    let jam = find_jam(&utg_jam, betting);
    apply(&mut utg_jam, betting, jam);
    assert_eq!(utg_jam.to_act, Some(SeatId::new_unchecked(4))); // HJ
    assert!(has_call(&utg_jam, betting));

    let mut hj_jam = root.clone();
    let open = find_raise_to(&hj_jam, betting, 2.0);
    apply(&mut hj_jam, betting, open);
    let jam = find_jam(&hj_jam, betting);
    apply(&mut hj_jam, betting, jam);
    assert_eq!(hj_jam.to_act, Some(SeatId::new_unchecked(5))); // CO
    assert!(has_call(&hj_jam, betting));

    let mut blind_three_bet = root;
    for seat in [3, 4, 5] {
        assert_eq!(blind_three_bet.to_act, Some(SeatId::new_unchecked(seat)));
        let fold = find_fold(&blind_three_bet, betting);
        apply(&mut blind_three_bet, betting, fold);
    }
    assert_eq!(blind_three_bet.to_act, Some(SeatId::new_unchecked(0))); // BTN
    let btn_open = find_raise_to(&blind_three_bet, betting, 2.5);
    apply(&mut blind_three_bet, betting, btn_open);
    assert_eq!(blind_three_bet.to_act, Some(SeatId::new_unchecked(1))); // SB
    let sb_three_bet = find_raise_to(&blind_three_bet, betting, 12.0);
    apply(&mut blind_three_bet, betting, sb_three_bet);
    assert_eq!(blind_three_bet.to_act, Some(SeatId::new_unchecked(2))); // BB
    assert!(has_call(&blind_three_bet, betting));
}
