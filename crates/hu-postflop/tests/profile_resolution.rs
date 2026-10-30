use hu_postflop::profile::{resolve_profile, restrict_iso};
use spot::{HandSelector, LockIr, LockStrategy, ProfileIr};

const RIVER: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2s 7s Ks 4h 3d"
[ranges]
BB = "AK,KQ,AA"
BTN = "QQ,JJ"
[tree]
script = "river { replace bet [50] } river when aggressions >= 1 { remove raise }"
[run]
memory = "1GiB"
"#;

const TURN: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 4s"
[ranges]
BB = "AK,KQ,AA"
BTN = "QQ,JJ"
[tree]
script = "turn { checkdown } river { replace bet [50] } river when aggressions >= 1 { remove raise }"
[run]
memory = "1GiB"
"#;

fn ir(hands: HandSelector) -> ProfileIr {
    ProfileIr {
        locks: vec![LockIr {
            node: "root".into(),
            hands,
            strategy: LockStrategy::Uniform(vec![("check".into(), 0.5)]),
            confidence: 1.0,
        }],
        incentives: vec![],
        frequencies: vec![],
        freeze_upstream: None,
    }
}

#[test]
fn category_locks_only_board_live_matching_hands() {
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let resolved = resolve_profile(
        &mut game,
        &p.config,
        &ir(HandSelector::Category("toppair".into())),
        None,
    )
    .unwrap();
    let lock = resolved.engine.locks[game.game.tree.node(0).aux as usize]
        .as_ref()
        .unwrap();
    let hands = &game.game.evaluator.hands;
    let expr = nlh::category::CategoryExpr::parse("toppair").unwrap();
    let mut matching = 0;
    let mut selected = Vec::new();
    for (h, &global) in hands
        .combos(game.game.tree.node(0).player)
        .iter()
        .enumerate()
    {
        let (a, b) = nlh::combo_cards(global as usize);
        let expected = expr.matches(&p.config.board, [a, b]);
        assert_eq!(lock.confidence[h] == 1.0, expected);
        if expected {
            assert_eq!(lock.fixed[h], 0.5);
            selected.push(h);
            matching += 1;
        }
    }
    assert!(matching > 0);
    let mut solver = hu_engine::Solver::<_, hu_engine::F32Storage>::new(
        game.game,
        Box::<hu_engine::Dcfr>::default(),
        Some(50),
    );
    solver.set_profile(resolved.engine).unwrap();
    solver.run(50);
    let avg = solver.average_strategy_at(0);
    for h in selected {
        assert_eq!(avg[h], 0.5);
    }
}

#[test]
fn suit_symmetry_is_retained_for_category_and_restricted_for_combo() {
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut config = p.config.clone();
    restrict_iso(&mut config, &ir(HandSelector::Category("toppair".into()))).unwrap();
    assert_eq!(config.profile_suit_perms.as_ref().unwrap().len(), 24);
    restrict_iso(&mut config, &ir(HandSelector::Range("AsKh".into()))).unwrap();
    assert!(config.profile_suit_perms.as_ref().unwrap().len() < 24);
    assert!(
        restrict_iso(
            &mut config,
            &ir(HandSelector::Category("not_a_category".into()))
        )
        .is_err()
    );
}

#[test]
fn symmetric_category_keeps_existing_iso_tree() {
    let p = hu_postflop::prepare::prepare(TURN, std::path::Path::new("test.toml")).unwrap();
    let original = hu_postflop::memory_usage(&p.config).nodes;
    let mut config = p.config.clone();
    restrict_iso(&mut config, &ir(HandSelector::Category("toppair".into()))).unwrap();
    assert_eq!(hu_postflop::memory_usage(&config).nodes, original);
    config.iso_merging = false;
    assert!(hu_postflop::memory_usage(&config).nodes > original);
}

#[test]
fn asymmetric_lock_agrees_with_unmerged_tree() {
    use hu_engine::{Dcfr, F32Storage, Solver};
    use nlh::Player;
    let p = hu_postflop::prepare::prepare(TURN, std::path::Path::new("test.toml")).unwrap();
    let mut profile = ir(HandSelector::Range("AhKd".into()));
    profile.locks[0].node = "xx[?]".into();
    let mut config = p.config.clone();
    restrict_iso(&mut config, &profile).unwrap();
    let solve = |config: &hu_postflop::PostflopConfig| {
        let mut game = hu_postflop::build_postflop_game(config, p.payoff.pipeline());
        let resolved = resolve_profile(&mut game, config, &profile, None).unwrap();
        let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(200));
        solver.set_profile(resolved.engine).unwrap();
        solver.run(200);
        (solver.true_expected_values(), solver.average_strategy_at(0))
    };
    let merged = solve(&config);
    config.iso_merging = false;
    let plain = solve(&config);
    for player in Player::BOTH {
        assert!((merged.0[player] - plain.0[player]).abs() < 1e-4);
    }
    assert_eq!(merged.1, plain.1);
}

#[test]
fn ancestor_freeze_and_selector_errors_are_explicit() {
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let root = game.game.tree.node(0);
    let size = root.num_children as usize * game.game.tree.storage_ref(root).num_hands as usize;
    let actions = root.num_children as usize;
    let root_ref = root.aux as usize;
    let mut profile = ir(HandSelector::All);
    profile.locks[0].node = "x".into();
    profile.freeze_upstream = Some(spot::FreezeIr {
        solution: "base.sol".into(),
        sha256: None,
    });
    let base = |id| Ok((id == 0).then(|| vec![1.0 / actions as f32; size]));
    let resolved = resolve_profile(&mut game, &p.config, &profile, Some(&base)).unwrap();
    assert!(resolved.summary.resolved_nodes.iter().any(|entry| matches!(
        entry.kind,
        hu_postflop::profile::ResolvedNodeKind::Lock
    ) && entry.entry == Some(0)
        && entry.history == "x"
        && entry.confidence == Some(1.0)
        && entry.partial == Some(false)));
    assert!(resolved.summary.resolved_nodes.iter().any(|entry| matches!(
        entry.kind,
        hu_postflop::profile::ResolvedNodeKind::Frozen
    ) && entry.node == 0
        && entry.history.is_empty()
        && entry.entry.is_none()));
    assert_eq!(resolved.frozen_nodes, vec![0]);
    assert_eq!(
        resolved.engine.locks[root_ref]
            .as_ref()
            .unwrap()
            .fixed
            .len(),
        size
    );
    profile.freeze_upstream = None;
    profile.locks[0].node = "not-a-node".into();
    assert!(resolve_profile(&mut game, &p.config, &profile, None).is_err());
    profile.locks[0].node = "root".into();
    profile.locks[0].strategy =
        LockStrategy::PerCombo(vec![("9c9d".into(), vec![("check".into(), 1.0)])]);
    assert!(resolve_profile(&mut game, &p.config, &profile, None).is_err());
    let index = hu_postflop::index::GameIndex::from_game(
        &game.game,
        &game.node_info,
        nlh::Street::River,
        p.config.scale,
    );
    let view = hu_postflop::index::TreeView {
        tree: &game.game.tree,
        index: &index,
    };
    profile.locks[0].strategy = LockStrategy::Uniform(
        view.actions(0)
            .into_iter()
            .map(|label| (label.to_owned(), 0.0))
            .collect(),
    );
    assert!(resolve_profile(&mut game, &p.config, &profile, None).is_err());
}

#[test]
fn explicit_ancestor_is_not_frozen() {
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let mut profile = ir(HandSelector::All);
    let mut child = profile.locks[0].clone();
    child.node = "x".into();
    profile.locks.push(child);
    profile.freeze_upstream = Some(spot::FreezeIr {
        solution: "base.sol".into(),
        sha256: None,
    });
    let resolved = resolve_profile(&mut game, &p.config, &profile, Some(&|_| Ok(None))).unwrap();
    assert!(resolved.frozen_nodes.is_empty());
    assert_eq!(resolved.summary.matched_nodes, 2);
}

#[test]
fn empty_hand_selections_are_errors_for_each_kind() {
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let empty = HandSelector::Range("22".into());
    let mut profile = ir(empty.clone());
    assert!(
        resolve_profile(&mut game, &p.config, &profile, None)
            .err()
            .unwrap()
            .to_string()
            .contains("selects no hands")
    );
    profile.locks.clear();
    profile.incentives.push(spot::IncentiveIr {
        node: "root".into(),
        hands: empty.clone(),
        action: "check".into(),
        amount: spot::IncentiveAmount::Bb(1.0),
    });
    assert!(
        resolve_profile(&mut game, &p.config, &profile, None)
            .err()
            .unwrap()
            .to_string()
            .contains("selects no hands")
    );
    profile.incentives.clear();
    profile.frequencies.push(spot::FrequencyIr {
        node: "root".into(),
        hands: empty,
        action: "check".into(),
        target: 0.5,
    });
    assert!(
        resolve_profile(&mut game, &p.config, &profile, None)
            .err()
            .unwrap()
            .to_string()
            .contains("selects no hands")
    );
}

#[test]
fn unmatched_profile_node_selector_is_an_error() {
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let mut profile = ir(HandSelector::All);
    profile.locks[0].node = "not-a-node".into();
    let error = resolve_profile(&mut game, &p.config, &profile, None)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("unknown or non-action node selector"),
        "{error}"
    );
    assert!(error.contains("not-a-node"), "{error}");
}

#[test]
fn profile_history_accepts_equivalent_bb_and_chip_amounts() {
    for (scale, bb, chips, canonical) in [(1000, "r1", "r1000c", "r1"), (6, "r1", "r6c", "r6c")] {
        let input = RIVER.replace("players = 2", &format!("players = 2\nbb_chips = {scale}"));
        let p = hu_postflop::prepare::prepare(&input, std::path::Path::new("test.toml")).unwrap();
        let mut ids = Vec::new();
        for selector in [bb, chips] {
            let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
            let mut profile = ir(HandSelector::All);
            profile.locks[0].node = selector.into();
            profile.locks[0].strategy = LockStrategy::Uniform(vec![("call".into(), 1.0)]);
            let resolved = resolve_profile(&mut game, &p.config, &profile, None).unwrap();
            let entry = &resolved.summary.resolved_nodes[0];
            assert_eq!(entry.history, canonical);
            ids.push(entry.node);
        }
        assert_eq!(ids[0], ids[1]);
    }
}

#[test]
fn label_path_accepts_iso_marker() {
    let p = hu_postflop::prepare::prepare(TURN, std::path::Path::new("test.toml")).unwrap();
    let game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let index = hu_postflop::index::GameIndex::from_game(
        &game.game,
        &game.node_info,
        nlh::Street::Turn,
        p.config.scale,
    );
    let view = hu_postflop::index::TreeView {
        tree: &game.game.tree,
        index: &index,
    };
    let plain = view.profile_nodes("check/check/?").unwrap();
    let board = view.profile_board(plain[0], &p.config.board);
    let card = board.last().unwrap();
    let marked = view.profile_nodes(&format!("check/check/{card}*")).unwrap();
    assert_eq!(
        marked,
        view.profile_nodes(&format!("check/check/{card}")).unwrap()
    );
}

#[test]
fn evaluator_compatibility_matches_brute_force() {
    use hu_engine::TerminalEvaluator;
    use nlh::Player;
    let p = hu_postflop::prepare::prepare(RIVER, std::path::Path::new("test.toml")).unwrap();
    let mut game = hu_postflop::build_postflop_game(&p.config, p.payoff.pipeline());
    let space = game.game.evaluator.register_profile_board(&p.config.board);
    for player in Player::BOTH {
        let evaluator = &game.game.evaluator;
        let opp = evaluator.hands.combos(player.opponent());
        let reach: Vec<_> = (0..opp.len()).map(|i| (i + 1) as f32 / 100.0).collect();
        let mut out = vec![f32::NAN; evaluator.hands.len(player)];
        evaluator.compat_mass(space, player, &reach, &mut out);
        for (i, &own) in evaluator.hands.combos(player).iter().enumerate() {
            let (a, b) = nlh::combo_cards(own as usize);
            let expected: f32 = opp
                .iter()
                .enumerate()
                .filter_map(|(j, &other)| {
                    let (c, d) = nlh::combo_cards(other as usize);
                    (a != c && a != d && b != c && b != d).then_some(reach[j])
                })
                .sum();
            assert!((out[i] - expected).abs() < 1e-5);
        }
    }
}
