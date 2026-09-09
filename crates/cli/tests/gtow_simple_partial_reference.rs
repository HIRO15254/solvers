use std::path::{Path, PathBuf};

use cli::config::GameSection;
use multiway::{Action, BettingConfig, BettingState, SeatId};

#[derive(Debug, PartialEq, Eq)]
enum MenuAction {
    Fold,
    Call,
    RaiseTo { milli_bb: u64, all_in: bool },
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/bench_multiway/6max_100bb_nl50_partial_simple_reference.toml")
}

fn fixture() -> (multiway::config::ValidatedMultiwayConfig, BettingState) {
    let path = fixture_path();
    let raw = std::fs::read_to_string(&path).expect("reading Simple reference fixture");
    cli::multiway_v1::validate_production_contract_at(&raw, &path)
        .expect("validating the v1 production contract");
    let lowered = cli::multiway_v1::parse_and_lower_at(&raw, &path)
        .expect("lowering Simple reference fixture");
    let GameSection::PreflopMultiway(game) = lowered.game else {
        panic!("fixture must lower to the multiway game");
    };
    let validated = game.validated().expect("validating Simple fixture");
    let state = BettingState::new(&validated).expect("building root betting state");
    (validated, state)
}

fn legal(state: &BettingState, betting: &BettingConfig) -> Vec<Action> {
    state
        .legal_actions(betting)
        .expect("expanding legal actions")
}

fn menu(state: &BettingState, betting: &BettingConfig) -> Vec<MenuAction> {
    legal(state, betting)
        .into_iter()
        .map(|action| match action {
            Action::Fold => MenuAction::Fold,
            Action::Call { .. } => MenuAction::Call,
            Action::RaiseTo { to, all_in, .. } => MenuAction::RaiseTo {
                milli_bb: to.raw(),
                all_in,
            },
            unexpected => panic!("unexpected preflop action {unexpected:?}"),
        })
        .collect()
}

fn fold(state: &BettingState, betting: &BettingConfig) -> Action {
    legal(state, betting)
        .into_iter()
        .find(|action| matches!(action, Action::Fold))
        .expect("fold must be legal")
}

fn normal_raise(state: &BettingState, betting: &BettingConfig, milli_bb: u64) -> Action {
    legal(state, betting)
        .into_iter()
        .find(|action| {
            matches!(
                action,
                Action::RaiseTo { to, all_in: false, .. } if to.raw() == milli_bb
            )
        })
        .unwrap_or_else(|| panic!("raise to {}bb must be legal", milli_bb as f64 / 1_000.0))
}

fn jam(state: &BettingState, betting: &BettingConfig) -> Action {
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

fn state_at_unopened(root: &BettingState, betting: &BettingConfig, seat: u8) -> BettingState {
    let mut state = root.clone();
    while state.to_act != Some(SeatId::new_unchecked(seat)) {
        let action = fold(&state, betting);
        apply(&mut state, betting, action);
    }
    state
}

fn expected_menu(call: bool, raises: &[(u64, bool)]) -> Vec<MenuAction> {
    let mut expected = vec![MenuAction::Fold];
    if call {
        expected.push(MenuAction::Call);
    }
    expected.extend(
        raises
            .iter()
            .map(|&(milli_bb, all_in)| MenuAction::RaiseTo { milli_bb, all_in }),
    );
    expected
}

#[test]
fn simple_fixture_has_requested_caps_buckets_and_five_unopened_menus() {
    let (config, root) = fixture();
    let betting = &config.betting;
    assert!(!betting.allow_limp);
    assert_eq!(betting.preflop.max_aggressive_actions, 4);
    assert_eq!(betting.flop.max_aggressive_actions, 1);
    assert_eq!(betting.turn.max_aggressive_actions, 1);
    assert_eq!(betting.river.max_aggressive_actions, 1);
    assert_eq!(config.abstraction.flop_buckets, 32);
    assert_eq!(config.abstraction.turn_buckets, 32);
    assert_eq!(config.abstraction.river_buckets, 32);

    let cases = [
        (3, expected_menu(false, &[(2_000, false)])),
        (4, expected_menu(false, &[(2_000, false)])),
        (5, expected_menu(false, &[(2_300, false), (100_000, true)])),
        (0, expected_menu(false, &[(2_500, false), (100_000, true)])),
        (1, expected_menu(false, &[(3_000, false), (100_000, true)])),
    ];
    for (seat, expected) in cases {
        let state = state_at_unopened(&root, betting, seat);
        assert_eq!(menu(&state, betting), expected, "unopened seat {seat}");
    }
}

#[test]
fn simple_fixture_matches_all_fifteen_observed_no_caller_responses() {
    let (config, root) = fixture();
    let betting = &config.betting;
    type ResponseCase = (u8, bool, &'static [(u64, bool)]);
    type OpenCase = (u8, u64, &'static [ResponseCase]);
    let cases: &[OpenCase] = &[
        (
            3,
            2_000,
            &[
                (4, false, &[(6_500, false)]),
                (5, false, &[(6_500, false)]),
                (0, false, &[(7_500, false)]),
                (1, false, &[(10_000, false)]),
                (2, true, &[(12_000, false)]),
            ],
        ),
        (
            4,
            2_000,
            &[
                (5, false, &[(6_500, false)]),
                (0, false, &[(7_500, false)]),
                (1, false, &[(11_000, false)]),
                (2, true, &[(12_300, false)]),
            ],
        ),
        (
            5,
            2_300,
            &[
                (0, false, &[(7_500, false), (100_000, true)]),
                (1, false, &[(11_500, false), (100_000, true)]),
                (2, true, &[(13_500, false), (100_000, true)]),
            ],
        ),
        (
            0,
            2_500,
            &[
                (1, false, &[(12_000, false)]),
                (2, true, &[(13_000, false)]),
            ],
        ),
        (1, 3_000, &[(2, true, &[(10_000, false), (100_000, true)])]),
    ];

    let mut observed = 0;
    for &(opener, open_to, responses) in cases {
        let mut state = state_at_unopened(&root, betting, opener);
        let open = normal_raise(&state, betting, open_to);
        apply(&mut state, betting, open);
        for &(actor, call, raises) in responses {
            assert_eq!(state.to_act, Some(SeatId::new_unchecked(actor)));
            assert_eq!(
                menu(&state, betting),
                expected_menu(call, raises),
                "opener {opener}, actor {actor}"
            );
            observed += 1;
            let action = fold(&state, betting);
            apply(&mut state, betting, action);
        }
    }
    assert_eq!(observed, 15);
}

#[test]
fn co_btn_and_sb_open_jams_keep_the_next_seats_call() {
    let (config, root) = fixture();
    let betting = &config.betting;
    for opener in [5, 0, 1] {
        let mut state = state_at_unopened(&root, betting, opener);
        let open_jam = jam(&state, betting);
        apply(&mut state, betting, open_jam);
        assert!(
            legal(&state, betting)
                .iter()
                .any(|action| matches!(action, Action::Call { .. })),
            "seat after opener {opener}'s jam lost its legal call"
        );
    }
}

#[test]
fn sb_open_bb_three_bet_has_the_observed_sb_four_bet_menu() {
    let (config, root) = fixture();
    let betting = &config.betting;
    let mut state = state_at_unopened(&root, betting, 1);
    let sb_open = normal_raise(&state, betting, 3_000);
    apply(&mut state, betting, sb_open);
    let bb_three_bet = normal_raise(&state, betting, 10_000);
    apply(&mut state, betting, bb_three_bet);
    assert_eq!(state.to_act, Some(SeatId::new_unchecked(1)));
    assert_eq!(
        menu(&state, betting),
        expected_menu(true, &[(21_000, false), (100_000, true)])
    );
}
