use mw_preflop::betting::{BettingMenu, HandPhase};
use mw_preflop::input::{self, P2Sections, Settings};
use mw_preflop::tree_rules::NlhContext;
use mw_preflop::{Action, BettingState, MwChips, SeatId, Street};
use nlh::script::{Value, VarSource};
use spot::{Document, TreeVar};
use std::path::Path;

fn document(text: &str) -> Document {
    Document::parse(text, Path::new("input.toml")).unwrap()
}
fn lower(text: &str) -> input::Lowered {
    let doc = document(text);
    let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
    input::lower(&doc.spot, &settings).unwrap()
}
fn config(players: usize, straddles: &str, script: &str) -> String {
    format!(
        "schema = \"solvers.nlh/v1\"\n[table]\nplayers = {players}\nstack_bb = 100\nstraddles_bb = {straddles}\n[tree]\nscript = '''{script}'''\n"
    )
}

#[test]
fn default_rake_unit_preserves_fingerprints() {
    use mw_preflop::solver::{ExternalSamplingGame, configuration_fingerprint_for_setup};
    let raw = config(3, "[]", "flop, turn, river { checkdown }")
        + "[economics.rake]\nrate = 0.05\ncap_bb = 4\n";
    let lowered = lower(&raw);
    let game = mw_preflop::HoldemGame::new(
        &lowered.game,
        &lowered.utility,
        &lowered.rake,
        mw_preflop::FeatureHashAbstraction::default(),
    )
    .unwrap();
    let configuration =
        configuration_fingerprint_for_setup(&game, &game.deal_sampler().unwrap(), lowered.solver);
    // Captured from HEAD 3a9c4ff before adding the configurable unit.
    assert_eq!(
        blake3::Hash::from(game.game_fingerprint())
            .to_hex()
            .as_str(),
        "0aa026ee09bf133b09f903c402da02df5ab9b4c6f35a5617f224d51e0194c4e4"
    );
    assert_eq!(
        blake3::Hash::from(configuration).to_hex().as_str(),
        "6e14c2051bed8b5ad0683e4ca0e5953c079f0731709561aacc9aafbf79740efe"
    );
}

#[test]
fn spec_p2_example_defaults_normalize_idempotently_and_lower() {
    let raw = config(
        6,
        "[]",
        "preflop { when unopened { replace raise [2.5bb, a] remove call } when aggressions >= 1 { replace raise [3x, a] } } flop, turn, river { checkdown }",
    ) + "[run]\nmemory = \"6GiB\"\nmax_time = \"12h\"\n";
    let doc = document(&raw);
    let normalized = doc.normalize(&P2Sections).unwrap();
    assert_eq!(
        document(&normalized).normalize(&P2Sections).unwrap(),
        normalized
    );
    let parsed = document(&normalized);
    let settings = Settings::parse(&parsed.spot, &parsed.solver, &parsed.output).unwrap();
    assert_eq!(settings.solver, input::Solver::default());
    assert_eq!(settings.output, input::Output::default());
    for ordered in [
        &[
            "kind =",
            "seed =",
            "opponent_exploration =",
            "batch_sweeps =",
            "[solver.abstraction]",
            "[solver.discount]",
            "[solver.pruning]",
            "[solver.stop]",
        ][..],
        &[
            "target =",
            "max_sweeps =",
            "check_every_sweeps =",
            "confirmations =",
            "evaluation_samples =",
            "deviator_traversals =",
        ][..],
    ] {
        let indices = ordered
            .iter()
            .map(|key| normalized.find(key).unwrap())
            .collect::<Vec<_>>();
        assert!(
            indices.windows(2).all(|pair| pair[0] < pair[1]),
            "{normalized}"
        );
    }
    let lowered = lower(&normalized);
    assert_eq!(lowered.run.memory_bytes, 6 * 1024 * 1024 * 1024);
    assert_eq!(lowered.run.max_time_seconds, Some(43_200.0));
    assert_eq!(lowered.run.checkpoint_interval_seconds, 900.0);
    assert_eq!(lowered.run.dev_gain_threshold, 0.05);
    assert_eq!(lowered.solver.prune_threshold, -6000.0);
    assert!(lowered.solver.traverser_vector && lowered.solver.prune);
    assert_eq!(
        lowered.game.abstraction.recall,
        mw_preflop::RecallMode::Street
    );
    let validated = lowered.game.validated().unwrap();
    let state = BettingState::from_config(&validated).unwrap();
    assert_eq!(state.to_act, Some(SeatId::new_unchecked(3)));
    assert_eq!(state.pot_size(), MwChips(1500));
    assert!(
        state
            .legal_actions(&validated.betting)
            .unwrap()
            .iter()
            .any(|a| matches!(
                a,
                Action::RaiseTo {
                    to: MwChips(2500),
                    ..
                }
            ))
    );
    assert!(
        !state
            .legal_actions(&validated.betting)
            .unwrap()
            .iter()
            .any(|a| matches!(a, Action::Call { .. }))
    );
}

#[test]
fn scriptless_tree_has_only_passive_actions_and_sb_one_is_supported() {
    for players in 2..=9 {
        let raw = config(players, "[]", "").replace("stack_bb = 100", "stack_bb = 100\nsb_bb = 1");
        let lowered = lower(&raw);
        let state = BettingState::from_config(&lowered.game.validated().unwrap()).unwrap();
        let actions = state.legal_actions(&lowered.game.betting).unwrap();
        assert!(
            actions
                .iter()
                .all(|a| matches!(a, Action::Fold | Action::Check | Action::Call { .. }))
        );
        let small = if players == 2 { 0 } else { 1 };
        assert_eq!(
            state.current_wager(SeatId::new_unchecked(small)),
            MwChips(1000)
        );
    }
}

#[test]
fn product_owned_keys_and_values_have_nlh_diagnostics() {
    let base = config(6, "[]", "");
    for suffix in [
        "[solver]\niso_merging = true",
        "[output]\nsolution_streets = 'full'",
        "[solver.stop]\nmax_iterations = 1",
    ] {
        let doc = document(&(base.clone() + suffix));
        let error = doc.normalize(&P2Sections).unwrap_err().to_string();
        assert!(
            error.contains("NLH002") && error.contains("this spot is solved by P2"),
            "{error}"
        );
    }
    for suffix in [
        "[solver]\ntypo = true",
        "[solver.abstraction.buckets]\nunknown = 1",
        "[solver.pruning]\nthreshold = -1",
        "[solver.discount]\nkind = 'none'\nevery_sweeps = 1",
    ] {
        let doc = document(&(base.clone() + suffix));
        assert!(
            doc.normalize(&P2Sections)
                .unwrap_err()
                .to_string()
                .contains("NLH002")
        );
    }
    for suffix in [
        "[solver]\nkind = 'single-hand'",
        "[solver]\nbatch_sweeps = 0",
        "[solver]\nopponent_exploration = nan",
        "[solver]\nseed = -1",
        "[solver.abstraction]\nkind = 'multiway-rollout'",
        "[solver.stop]\ntarget = 0",
        "[solver.stop]\ntarget = '0.05bb'",
        "[solver.stop]\nmax_sweeps = 0",
        "[solver.abstraction.buckets]\nflop = 0",
        "[solver.discount]\nevery_sweeps = 0",
        "[output]\nprobability_encoding = 'i16'",
    ] {
        let doc = document(&(base.clone() + suffix));
        let error = doc.normalize(&P2Sections).unwrap_err().to_string();
        assert!(error.contains("NLH003"), "{suffix}: {error}");
    }
    let doc = document(&(base.clone() + "[solver.discount]\n[solver.pruning]\n"));
    assert!(doc.normalize(&P2Sections).is_ok());
    let disabled = lower(&(base + "[solver.discount]\nkind = 'periodic'\nuntil_sweeps = 0\n"));
    assert_eq!(disabled.solver.discount_until, 0);
}

#[test]
fn economics_resources_and_algorithm_lower_with_legacy_meanings() {
    let raw = config(3, "[]", "")
        + r#"
[economics]
kind = "tournament"
payouts = [1000, 600, 400]
outside_field_bb = [18, 26]
[solver]
kind = "single-hand"
seed = 19
opponent_exploration = 0.125
batch_sweeps = 3
[solver.discount]
kind = "none"
[solver.pruning]
kind = "none"
[solver.stop]
target = 0.05
[run]
threads = 2
memory = "7GiB"
max_time = "2m"
checkpoint_interval = "5m"
[output]
probability_encoding = "f32"
"#;
    let lowered = lower(&raw);
    let economics::UtilityConfig::TournamentIcm {
        outside_field,
        payouts,
        ..
    } = &lowered.utility
    else {
        panic!()
    };
    assert_eq!(outside_field[0].name, "outside-0");
    assert_eq!(payouts, &[1000.0, 600.0, 400.0, 0.0, 0.0]);
    assert_eq!(lowered.run.dev_gain_threshold, 100.0);
    assert_eq!(
        input::resolve_target(&lowered.utility, &input::Target::default()).unwrap(),
        0.2
    );
    assert_eq!(lowered.run.threads, 2);
    assert_eq!(lowered.run.memory_bytes, 7 * 1024 * 1024 * 1024);
    assert_eq!(lowered.run.max_time_seconds, Some(120.0));
    assert_eq!(lowered.run.checkpoint_interval_seconds, 300.0);
    assert_eq!(lowered.solver.seed, 19);
    assert_eq!(lowered.solver.exploration_epsilon, 0.125);
    assert_eq!(lowered.solver.discount_every, u64::MAX);
    assert!(!lowered.solver.traverser_vector && !lowered.solver.prune);
    let antes =
        lower(&config(3, "[]", "").replace("stack_bb = 100", "stack_bb = 100\nante_bb = 0.125"));
    assert_eq!(antes.game.forced_bets.unwrap().antes_bb, vec![0.125; 3]);
    let bb_ante =
        lower(&config(3, "[]", "").replace("stack_bb = 100", "stack_bb = 100\nbb_ante_bb = 0.5"));
    assert_eq!(bb_ante.game.forced_bets.unwrap().common_ante_bb, 0.5);
    let huge_prizes = economics::UtilityConfig::TournamentIcm {
        outside_field: Vec::new(),
        payouts: vec![1e308, 9e307, 0.0],
        samples: 100_000,
        seed: 0,
    };
    assert!(input::resolve_target(&huge_prizes, &input::Target::default()).is_err());
}

fn assert_straddles(
    players: usize,
    posts: &str,
    order: &[&str],
    minimum: u64,
    three_x: u64,
    call: u64,
) {
    let lowered = lower(&config(
        players,
        posts,
        "preflop { replace raise [min, 3x] }",
    ));
    let validated = lowered.game.validated().unwrap();
    let mut state = BettingState::from_config(&validated).unwrap();
    let actions = state.legal_actions(&validated.betting).unwrap();
    for amount in [minimum, three_x] {
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::RaiseTo { to, .. } if to.raw() == amount)),
            "{actions:?}"
        );
    }
    for position in order {
        let actor = state.to_act.unwrap();
        assert_eq!(nlh::position_name(actor, state.button, players), *position);
        assert_eq!(state.aggressive_actions, 0);
        let actions = state.legal_actions(&validated.betting).unwrap();
        let action = actions
            .iter()
            .find(|a| matches!(a, Action::Call { .. } | Action::Check))
            .unwrap()
            .clone();
        if *position == order[0] {
            assert_eq!(state.amount_to_call(actor).raw(), call);
        }
        if *position == *order.last().unwrap() {
            assert_eq!(action, Action::Check);
        }
        state.apply(action, &validated.betting).unwrap();
    }
    assert_eq!(state.street, Street::Flop);
    assert_eq!(state.big_blind.raw(), 1000);
    assert_eq!(state.minimum_full_target().raw(), 1000);
    let roundtrip: mw_preflop::MultiwayConfig =
        serde_json::from_str(&serde_json::to_string(&lowered.game).unwrap()).unwrap();
    assert_eq!(
        BettingState::from_config(&roundtrip.validated().unwrap()).unwrap(),
        BettingState::from_config(&validated).unwrap()
    );
}

#[test]
fn live_straddles_preserve_order_raise_limp_and_postflop_bb() {
    assert_straddles(
        6,
        "[2, 4]",
        &["CO", "BTN", "SB", "BB", "UTG", "HJ"],
        8000,
        12000,
        4000,
    );
    assert_straddles(3, "[2]", &["SB", "BB", "BTN"], 4000, 6000, 1500);
    assert_straddles(
        6,
        "[2, 4, 8]",
        &["BTN", "SB", "BB", "UTG", "HJ", "CO"],
        16000,
        24000,
        8000,
    );
}

#[test]
fn common_cbet_and_donk_follow_the_preceding_street() {
    use nlh::betting::Move;
    let lowered = lower(&config(3, "[]", ""));
    let mut state = BettingState::from_config(&lowered.game.validated().unwrap()).unwrap();
    for action in [Move::RaiseTo(MwChips(3000)), Move::Fold, Move::Call] {
        state
            .apply_action(state.resolve_move(action).unwrap(), &nlh::NoStreetPolicy)
            .unwrap();
    }
    let value = |state: &BettingState, actor, var| NlhContext { state, actor }.value(var);
    assert_eq!(
        value(&state, SeatId::new_unchecked(0), TreeVar::Cbet),
        Value::Bool(true)
    );
    assert_eq!(
        value(&state, SeatId::new_unchecked(2), TreeVar::Donk),
        Value::Bool(true)
    );
    state
        .apply_action(
            state.resolve_move(Move::BetTo(MwChips(2000))).unwrap(),
            &nlh::NoStreetPolicy,
        )
        .unwrap();
    state
        .apply_action(
            state.resolve_move(Move::Call).unwrap(),
            &nlh::NoStreetPolicy,
        )
        .unwrap();
    assert_eq!(state.street, Street::Turn);
    assert_eq!(
        value(&state, SeatId::new_unchecked(2), TreeVar::Cbet),
        Value::Bool(true)
    );
    assert_eq!(
        value(&state, SeatId::new_unchecked(0), TreeVar::Donk),
        Value::Bool(true)
    );
    for _ in 0..2 {
        state
            .apply_action(
                state.resolve_move(Move::Check).unwrap(),
                &nlh::NoStreetPolicy,
            )
            .unwrap();
    }
    assert_eq!(state.street, Street::River);
    for actor in [0, 2] {
        for var in [TreeVar::Cbet, TreeVar::Donk] {
            assert_eq!(
                value(&state, SeatId::new_unchecked(actor), var),
                Value::Bool(false)
            );
        }
    }
    // A river cbet rule is unused after a checked turn: preceding-street
    // aggression, rather than the original preflop raiser, controls cbet.
    let raw = config(3, "[]", "river when cbet { remove check }");
    let betting = lower(&raw).game.betting;
    state.apply(Action::Check, &betting).unwrap();
    assert_eq!(state.legal_actions(&betting).unwrap(), vec![Action::Check]);
}

#[test]
fn random_preflop_lines_agree_with_spot_facts_and_replay() {
    use rand::{Rng, SeedableRng};
    let raw = config(6, "[]", "preflop { replace raise [min, 3x, a] }");
    let lowered = lower(&raw);
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(61);
    let mut compared = 0;
    for _ in 0..2000 {
        let mut state = BettingState::from_config(&lowered.game.validated().unwrap()).unwrap();
        let mut tokens = Vec::new();
        while state.street == Street::Preflop && state.phase == HandPhase::Betting {
            let actor = state.to_act.unwrap();
            let ctx = NlhContext {
                state: &state,
                actor,
            };
            assert_eq!(ctx.value(TreeVar::Aggressions), ctx.value(TreeVar::Raises));
            assert_eq!(ctx.value(TreeVar::Cbet), Value::Bool(false));
            assert_eq!(ctx.value(TreeVar::Donk), Value::Bool(false));
            assert_eq!(
                ctx.value(TreeVar::Pot),
                Value::Number(state.pot_size().as_bb())
            );
            assert_eq!(
                ctx.value(TreeVar::ToCall),
                Value::Number(state.amount_to_call(actor).as_bb())
            );
            let actions = state.legal_actions(&lowered.game.betting).unwrap();
            let action = actions[rng.gen_range(0..actions.len())].clone();
            let spelling = match action {
                Action::Fold => None,
                Action::Check => Some("x".into()),
                Action::Call { .. } => Some("c".into()),
                Action::RaiseTo { to, .. } => Some(format!("r{}", to.as_bb())),
                _ => unreachable!(),
            };
            if let Some(spelling) = spelling {
                tokens.push(format!(
                    "{} {spelling}",
                    nlh::position_name(actor, state.button, 6)
                ));
            }
            state.apply(action, &lowered.game.betting).unwrap();
        }
        if state.street != Street::Flop
            || state.active_mask().len() != 2
            || state.non_folded_mask().len() != 2
        {
            continue;
        }
        let line = tokens.join(", ");
        let parsed = document(
            &(raw.clone()
                + &format!("[spot]\nline = '{line}'\nboard = 'Ks 7h 2d'\n[ranges]\n")
                + &state
                    .non_folded_mask()
                    .iter()
                    .map(|seat| {
                        format!("{} = 'random'\n", nlh::position_name(seat, state.button, 6))
                    })
                    .collect::<String>()),
        );
        assert_eq!(parsed.spot.start, state);
        for player in [
            parsed.spot.context.oop.as_ref().unwrap(),
            parsed.spot.context.ip.as_ref().unwrap(),
        ] {
            let ctx = NlhContext {
                state: &state,
                actor: player.seat,
            };
            let facts = &player.preflop;
            for (var, expected) in [
                (TreeVar::Limpers, Value::Number(facts.limpers.into())),
                (TreeVar::Flats, Value::Number(facts.flats.into())),
                (
                    TreeVar::OpenColdCalls,
                    Value::Number(facts.open_cold_calls.into()),
                ),
                (TreeVar::Squeeze, Value::Bool(facts.squeeze)),
                (
                    TreeVar::PreflopParticipant,
                    Value::Bool(facts.preflop_participant),
                ),
                (
                    TreeVar::InPositionToLastAggressor,
                    Value::Bool(facts.in_position_to_last_aggressor),
                ),
            ] {
                assert_eq!(ctx.value(var), expected, "{line}");
            }
            assert_eq!(
                ctx.value(TreeVar::LastPreflopAggressorPosition),
                Value::Text(
                    state
                        .last_preflop_aggressor
                        .map(|s| nlh::position_name(s, state.button, 6))
                        .unwrap_or("")
                )
            );
            assert_eq!(
                facts.last_preflop_aggressor_position,
                state
                    .last_preflop_aggressor
                    .map(|s| nlh::position_name(s, state.button, 6))
                    .unwrap_or("")
            );
        }
        compared += 1;
    }
    assert!(compared > 20, "only {compared} eligible HU lines");
}

#[test]
fn typed_session_retains_effective_config_and_checks_resume_fingerprints() {
    use mw_preflop::{FeatureHashAbstraction, MultiwayCheckpoint};
    let raw = config(
        3,
        "[]",
        "preflop { replace raise [a] } flop, turn, river { checkdown }",
    );
    let effective = document(&raw).normalize(&P2Sections).unwrap();
    let session = input::build_session(
        lower(&effective),
        FeatureHashAbstraction::default(),
        effective.clone(),
        None,
    )
    .unwrap();
    assert_eq!(
        session.config_hash,
        runfiles::config_hash(effective.as_bytes())
    );
    assert_eq!(session.config_toml, effective);
    assert!(
        session
            .solver
            .policy_arena_allocation()
            .unwrap()
            .pages_committed
    );
    let temp = tempfile::tempdir().unwrap();
    let checkpoint_path = temp.path().join("resume.mwckpt");
    MultiwayCheckpoint::capture(&session.solver)
        .with_runtime_metadata(&effective, Default::default())
        .write_atomic(&checkpoint_path)
        .unwrap();
    let restored = input::build_session(
        lower(&effective),
        FeatureHashAbstraction::default(),
        effective.clone(),
        Some(&checkpoint_path),
    )
    .unwrap();
    assert!(restored.checkpoint_runtime.is_some());
    assert_eq!(
        session.solver.configuration_fingerprint(),
        restored.solver.configuration_fingerprint()
    );
    let different = effective.replace("seed = 0", "seed = 1");
    assert!(
        input::build_session(
            lower(&different),
            FeatureHashAbstraction::default(),
            different,
            Some(&checkpoint_path)
        )
        .is_err()
    );
}

#[test]
fn tree_controls_source_order_and_integer_boundaries_are_explicit() {
    let raw = config(3, "[]", "preflop { force raise [3bb] force raise [4bb] }");
    let lowered = lower(&raw);
    let state = BettingState::from_config(&lowered.game.validated().unwrap()).unwrap();
    let actions = state.legal_actions(&lowered.game.betting).unwrap();
    assert_eq!(actions.len(), 1);
    assert!(matches!(
        actions[0],
        Action::RaiseTo {
            to: MwChips(4000),
            ..
        }
    ));

    let raw = config(3, "[]", "preflop { replace raise [5bb] }")
        .replace("stack_bb = 100", "stack_bb = 10")
        + "allin_threshold = 0.4\n";
    let lowered = lower(&raw);
    let state = BettingState::from_config(&lowered.game.validated().unwrap()).unwrap();
    assert!(
        state
            .legal_actions(&lowered.game.betting)
            .unwrap()
            .iter()
            .any(|a| matches!(
                a,
                Action::RaiseTo {
                    to: MwChips(10000),
                    all_in: true,
                    ..
                }
            ))
    );

    let raw = config(3, "[]", "preflop { when unopened { replace raise [2.5bb] } when !unopened { replace raise [10bb] } }")
        .replace("stack_bb = 100", "stack_bb = 20") + "preflop_reraise_jam_above_stack = { numerator = 1, denominator = 3 }\n";
    let lowered = lower(&raw);
    let mut state = BettingState::from_config(&lowered.game.validated().unwrap()).unwrap();
    let open = state
        .legal_actions(&lowered.game.betting)
        .unwrap()
        .into_iter()
        .find(|a| {
            matches!(
                a,
                Action::RaiseTo {
                    to: MwChips(2500),
                    ..
                }
            )
        })
        .unwrap();
    state.apply(open, &lowered.game.betting).unwrap();
    assert!(
        state
            .legal_actions(&lowered.game.betting)
            .unwrap()
            .iter()
            .any(|a| matches!(
                a,
                Action::RaiseTo {
                    to: MwChips(20000),
                    all_in: true,
                    ..
                }
            ))
    );

    for suffix in [
        "[tree.max_aggressive_actions]\npreflop = 256\n",
        "preflop_reraise_jam_above_stack = { numerator = 1, denominator = 4294967296 }\n",
    ] {
        let doc = document(&(config(3, "[]", "") + suffix));
        let settings = Settings::parse(&doc.spot, &doc.solver, &doc.output).unwrap();
        assert!(
            input::lower(&doc.spot, &settings)
                .unwrap_err()
                .to_string()
                .contains("NLH003")
        );
    }

    let roundtrip: mw_preflop::MultiwayConfig =
        serde_json::from_str(&serde_json::to_string(&lowered.game).unwrap()).unwrap();
    assert!(roundtrip.validated().is_ok());
    let mut invalid = roundtrip;
    invalid.betting.nlh_rules[0].condition = "board_cards == 3".into();
    assert!(invalid.validated().is_err());
}

#[test]
fn straddle_configuration_rejects_invalid_posts_without_panicking() {
    let game = lower(&config(6, "[2, 4]", "")).game;
    for posts in [
        vec![(3, 1.5)],
        vec![(4, 2.0)],
        vec![(3, 2.0), (3, 4.0)],
        vec![(3, 2.0001)],
        vec![(3, 100.0)],
    ] {
        let mut invalid = game.clone();
        invalid.forced_bets.as_mut().unwrap().straddles = posts
            .into_iter()
            .map(|(seat, amount)| (SeatId::new_unchecked(seat), amount))
            .collect();
        // Make first actor valid independently, so an off-grid post cannot
        // pass this test merely because it also changed the chain length.
        let forced = invalid.forced_bets.as_mut().unwrap();
        forced.first_to_act = forced.straddles.last().unwrap().0.next(6);
        assert!(invalid.validated().is_err());
    }
}

#[test]
fn p2_type_errors_keep_full_dotted_keys_and_target_is_strict() {
    let base = config(3, "[]", "");
    for (suffix, key) in [
        ("[solver]\nabstraction = 2", "solver.abstraction"),
        ("[solver]\nseed = '19'", "solver.seed"),
        (
            "[solver]\nopponent_exploration = '0.1'",
            "solver.opponent_exploration",
        ),
        (
            "[solver.abstraction]\nbuckets = 2",
            "solver.abstraction.buckets",
        ),
        (
            "[solver.abstraction.buckets]\nflop = '2'",
            "solver.abstraction.buckets.flop",
        ),
        ("[solver.stop]\nmax_sweeps = '1'", "solver.stop.max_sweeps"),
        ("[solver.stop]\ntarget = '0.05'", "solver.stop.target"),
        ("[solver.stop]\ntarget = true", "solver.stop.target"),
        (
            "[output]\nprobability_encoding = 2",
            "output.probability_encoding",
        ),
    ] {
        let doc = document(&(base.clone() + suffix));
        let error = doc.normalize(&P2Sections).unwrap_err();
        assert_eq!(error.code, spot::Code::NLH002, "{suffix}: {error}");
        assert!(error.to_string().contains(key), "{suffix}: {error}");
    }
}
