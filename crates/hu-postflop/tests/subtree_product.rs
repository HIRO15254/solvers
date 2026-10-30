use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hu_engine::{F32Storage, NodeKind, NodeLock, Solver, StorageOps};
use hu_postflop::{artifact, checkpoint, prepare, run};
use nlh::{PerPlayer, Player};

const RIVER: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack = 21
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2s 7s Ks 4h 3d"
[ranges]
BB = "AK,KQ,AA"
BTN = "QQ,JJ"
[tree]
script = "river { replace bet [50%] } river when aggressions >= 1 { remove raise }"
[solver]
storage = "f32"
[solver.stop]
max_iterations = 50
check_every = 25
[run]
memory = "1GiB"
threads = 1
"#;

fn request<'a>(
    p: &'a prepare::Prepared,
    cancel: &'a AtomicBool,
    ckpt: &'a Path,
    sol: &'a Path,
    state: Option<checkpoint::CheckpointReader>,
    history: Vec<(u64, f64)>,
) -> run::RunRequest<'a> {
    run::RunRequest {
        prepared: p,
        checkpoint: ckpt,
        solution: sol,
        state,
        evaluation_history: history,
        elapsed_before: Duration::ZERO,
        cancel,
    }
}

fn execute(p: &prepare::Prepared, cancel: &AtomicBool, ckpt: &Path, sol: &Path) -> run::RunSummary {
    run::run(
        request(p, cancel, ckpt, sol, None, vec![]),
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .unwrap()
}

fn frozen_input(base: &Path, entries: &str) -> String {
    let path = base.to_string_lossy().replace('\\', "/");
    format!("{RIVER}\n[profile]\nfreeze_upstream = {{ solution = '{path}' }}\n{entries}\n")
}

#[test]
fn subtree_preserves_outside_strategy_and_full_tree_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spot.toml");
    let cancel = AtomicBool::new(false);
    let base_path = dir.path().join("base.sol");
    let base_p = prepare::prepare(RIVER, &path).unwrap();
    execute(&base_p, &cancel, &dir.path().join("base.ckpt"), &base_path);
    let input = frozen_input(
        &base_path,
        "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 1.0 }",
    );
    let p = prepare::prepare(&input, &path).unwrap();
    let ckpt = dir.path().join("sub.ckpt");
    let sol = dir.path().join("sub.sol");
    let summary = execute(&p, &cancel, &ckpt, &sol);
    let scope = summary.solve_scope.as_ref().unwrap();
    assert_eq!(scope.scope, "subtree");
    assert!(scope.subtree_roots > 0);
    assert_eq!(
        checkpoint::CheckpointReader::open(&ckpt).unwrap().mode,
        checkpoint::CheckpointMode::Subtree
    );
    let base = artifact::load_sol(&base_path, 0, None).unwrap();
    let result = artifact::load_sol(&sol, 0, None).unwrap();
    let (game, _, resolved) = run::build_profiled(&p).unwrap();
    let resolved = resolved.unwrap();
    let target = resolved
        .summary
        .resolved_nodes
        .iter()
        .find(|n| matches!(n.kind, hu_postflop::profile::ResolvedNodeKind::Lock))
        .unwrap()
        .node;
    let mut inside = vec![false; game.tree.nodes.len()];
    let mut stack = vec![target];
    while let Some(id) = stack.pop() {
        inside[id as usize] = true;
        stack.extend(game.tree.children(id));
    }
    let mut whole_profile = resolved.engine.clone();
    for id in 0..game.tree.nodes.len() as u32 {
        let node = game.tree.node(id);
        if node.kind == NodeKind::Action && !inside[id as usize] {
            let strategy = base.stored_strategy(id).unwrap().unwrap();
            assert_eq!(
                result.stored_strategy(id).unwrap().unwrap(),
                strategy,
                "outside node {id}"
            );
            whole_profile.locks[node.aux as usize] = Some(NodeLock {
                fixed: strategy,
                confidence: vec![1.0; game.tree.storage_ref(node).num_hands as usize],
            });
        }
    }
    // Whole-tree CFR with the same outside policy must produce the same subtree.
    let mut whole =
        Solver::<_, F32Storage>::new(game, run::schedule(&p.settings.solver.algorithm), Some(50));
    whole.set_cfr_precision(p.settings.solver.cfr_precision);
    whole.set_profile(whole_profile).unwrap();
    whole.run(50);
    for id in 0..whole.game().tree.nodes.len() as u32 {
        if inside[id as usize] && whole.game().tree.node(id).kind == NodeKind::Action {
            let a = whole.average_strategy_at(id);
            let b = result.stored_strategy(id).unwrap().unwrap();
            assert_eq!(a.len(), b.len());
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 1e-5, "node {id}: {x} vs {y}");
            }
        }
    }
    for id in 0..whole.game().tree.nodes.len() as u32 {
        let node = whole.game().tree.node(id);
        if node.kind == NodeKind::Action && !inside[id as usize] {
            let sref = whole.game().tree.storage_ref(node);
            let strategy = base.stored_strategy(id).unwrap().unwrap();
            whole
                .restore_stream(50, |storage| {
                    storage.seed_strategy(sref, sref.index, &strategy);
                    Ok::<_, std::convert::Infallible>(())
                })
                .unwrap();
        }
    }
    whole.set_profile(resolved.engine).unwrap();
    let (_, gain) = whole.evaluate();
    assert!((summary.nash_conv - gain[Player::P0] - gain[Player::P1]).abs() < 1e-6);
    assert!((summary.nash_conv - summary.expl_p0 - summary.expl_p1).abs() < 1e-8);
}

#[test]
fn subtree_checkpoint_resume_and_legacy_mode_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spot.toml");
    let cancel = AtomicBool::new(false);
    let base_path = dir.path().join("base.sol");
    let base = prepare::prepare(RIVER, &path).unwrap();
    execute(&base, &cancel, &dir.path().join("base.ckpt"), &base_path);
    let p = prepare::prepare(
        &frozen_input(
            &base_path,
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 1.0 }",
        ),
        &path,
    )
    .unwrap();
    let full_ckpt = dir.path().join("full.ckpt");
    let full_sol = dir.path().join("full.sol");
    let full = execute(&p, &cancel, &full_ckpt, &full_sol);
    let ckpt = dir.path().join("partial.ckpt");
    let sol = dir.path().join("partial.sol");
    let mut history = vec![];
    let partial = run::run(
        request(&p, &cancel, &ckpt, &sol, None, vec![]),
        &mut |event| {
            if let run::Observation::Progress(row) = event {
                history.push((row.iteration, row.nash_conv / 2.0));
                if row.iteration == 25 {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            Ok(())
        },
        &mut |_| {},
    )
    .unwrap();
    assert!(partial.canceled);
    // Progress uses the subtree decomposition; the final summary performs an
    // independent full-tree evaluation of this same checkpoint state.
    assert!((history.last().unwrap().1 * 2.0 - partial.nash_conv).abs() < 1e-6);
    cancel.store(false, Ordering::SeqCst);
    let state = checkpoint::CheckpointReader::open(&ckpt).unwrap();
    assert_eq!(state.mode, checkpoint::CheckpointMode::Subtree);
    let resumed = run::run(
        request(&p, &cancel, &ckpt, &sol, Some(state), history),
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(full.iterations, resumed.iterations);
    assert_eq!(full.nash_conv.to_bits(), resumed.nash_conv.to_bits());
    let a = artifact::load_sol(&full_sol, 0, None).unwrap();
    let b = artifact::load_sol(&sol, 0, None).unwrap();
    for id in 0..a.game.tree.nodes.len() as u32 {
        if a.game.tree.node(id).kind == NodeKind::Action {
            assert_eq!(
                a.stored_strategy(id).unwrap(),
                b.stored_strategy(id).unwrap()
            );
        }
    }
    // The legacy writer deliberately emits a whole-tree checkpoint with the same
    // effective configuration, so compatibility must reject its execution mode.
    let (game, _, _) = run::build_profiled(&p).unwrap();
    let solver =
        Solver::<_, F32Storage>::new(game, run::schedule(&p.settings.solver.algorithm), Some(50));
    let old = dir.path().join("legacy.ckpt");
    checkpoint::write_storage_with_config(
        &old,
        prepare::compatibility_hash(&p.effective).unwrap(),
        0,
        solver.storage(),
        &p.effective,
        0.0,
        1,
    )
    .unwrap();
    let reader = checkpoint::CheckpointReader::open(&old).unwrap();
    assert_eq!(reader.mode, checkpoint::CheckpointMode::WholeTree);
    let err = run::run(
        request(&p, &cancel, &old, &sol, Some(reader), vec![]),
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .err()
    .expect("legacy freeze checkpoint must be rejected");
    assert!(
        err.to_string().to_lowercase().contains("checkpoint"),
        "{err:#}"
    );
}

#[test]
fn frequency_freeze_subtree_matches_whole_tree_and_resume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spot.toml");
    let cancel = AtomicBool::new(false);
    let base_path = dir.path().join("base.sol");
    let base = prepare::prepare(RIVER, &path).unwrap();
    execute(&base, &cancel, &dir.path().join("base.ckpt"), &base_path);
    let entries = "[[profile.frequency]]\nnode = 'x'\naction = 'check'\ntarget = 0.5";
    let p = prepare::prepare(&frozen_input(&base_path, entries), &path).unwrap();
    let full_ckpt = dir.path().join("full.ckpt");
    let full_sol = dir.path().join("full.sol");
    let full = execute(&p, &cancel, &full_ckpt, &full_sol);
    assert_eq!(full.solve_scope.as_ref().unwrap().scope, "subtree");
    assert_eq!(
        checkpoint::CheckpointReader::open(&full_ckpt).unwrap().mode,
        checkpoint::CheckpointMode::Subtree
    );
    assert_eq!(full.frequencies.len(), 1);

    let ckpt = dir.path().join("partial.ckpt");
    let sol = dir.path().join("resumed.sol");
    let mut history = Vec::new();
    let partial = run::run(
        request(&p, &cancel, &ckpt, &sol, None, vec![]),
        &mut |event| {
            if let run::Observation::Progress(row) = event {
                history.push((row.iteration, row.nash_conv / 2.0));
                if row.iteration == 25 {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            Ok(())
        },
        &mut |_| {},
    )
    .unwrap();
    assert!(partial.canceled);
    assert!((history.last().unwrap().1 * 2.0 - partial.nash_conv).abs() < 1e-6);
    let state = checkpoint::CheckpointReader::open(&ckpt).unwrap();
    assert_eq!(state.mode, checkpoint::CheckpointMode::Subtree);
    assert_eq!(state.frequency_lambdas.len(), 1);
    assert_eq!(state.frequency_lambdas[0], partial.frequencies[0].lambda);
    cancel.store(false, Ordering::SeqCst);
    let resumed = run::run(
        request(&p, &cancel, &ckpt, &sol, Some(state), history),
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(full.iterations, resumed.iterations);
    assert_eq!(full.nash_conv.to_bits(), resumed.nash_conv.to_bits());
    assert_eq!(full.frequencies[0].lambda, resumed.frequencies[0].lambda);
    assert_eq!(
        full.frequencies[0].achieved,
        resumed.frequencies[0].achieved
    );
    let result = artifact::load_sol(&full_sol, 0, None).unwrap();
    let resumed_artifact = artifact::load_sol(&sol, 0, None).unwrap();
    let base_artifact = artifact::load_sol(&base_path, 0, None).unwrap();

    // Run an ordinary full-tree solver with the frozen policy explicitly locked.
    // The reference controller follows the production evaluation cadence.
    let (game, _, resolved) = run::build_profiled(&p).unwrap();
    let resolved = resolved.unwrap();
    assert_eq!(resolved.targets.len(), 1);
    let target = resolved.targets[0].clone();
    let mut inside = vec![false; game.tree.nodes.len()];
    let mut stack = vec![target.node];
    while let Some(id) = stack.pop() {
        inside[id as usize] = true;
        stack.extend(game.tree.children(id));
    }
    let mut profile = resolved.engine;
    for id in 0..game.tree.nodes.len() as u32 {
        let node = game.tree.node(id);
        if node.kind != NodeKind::Action {
            continue;
        }
        assert_eq!(
            result.stored_strategy(id).unwrap(),
            resumed_artifact.stored_strategy(id).unwrap()
        );
        if !inside[id as usize] {
            let strategy = base_artifact.stored_strategy(id).unwrap().unwrap();
            assert_eq!(
                result.stored_strategy(id).unwrap().unwrap(),
                strategy,
                "outside node {id}"
            );
            profile.locks[node.aux as usize] = Some(NodeLock {
                fixed: strategy,
                confidence: vec![1.0; game.tree.storage_ref(node).num_hands as usize],
            });
        }
    }
    let mut whole =
        Solver::<_, F32Storage>::new(game, run::schedule(&p.settings.solver.algorithm), Some(50));
    whole.set_cfr_precision(p.settings.solver.cfr_precision);
    whole.set_profile(profile).unwrap();
    let mut lambda = 0.0;
    for _ in 0..2 {
        whole.run(25);
        let game = whole.game();
        let node = game.tree.node(target.node);
        let parents = hu_engine::parent_array(&game.tree);
        let path = hu_engine::path_from_root(&parents, target.node);
        let root = PerPlayer::new(
            game.root_ranges[Player::P0].as_slice(),
            game.root_ranges[Player::P1].as_slice(),
        );
        let reach = hu_engine::reach_on_path(&game.tree, root, &path, |id, _, out| {
            out.copy_from_slice(&whole.average_strategy_at(id))
        });
        let strategy = whole.current_strategy_at(target.node);
        let hands = game.tree.storage_ref(node).num_hands as usize;
        let mut numerator = 0.0;
        let mut denominator = 0.0;
        for &hand in &target.hands {
            let h = hand as usize;
            let weight = f64::from(reach[node.player][h]);
            denominator += weight;
            numerator += weight * f64::from(strategy[target.action * hands + h]);
        }
        assert!(denominator > 0.0);
        lambda += 0.1 * (target.target - numerator / denominator) * target.pot_utility;
        let incentive = whole.profile_mut().unwrap().incentives[target.storage_ref as usize]
            .as_mut()
            .unwrap();
        for &hand in &target.hands {
            incentive.bonus[target.action * hands + hand as usize] = lambda as f32;
        }
    }
    assert!((lambda - full.frequencies[0].lambda).abs() < 1e-5);
    let game = whole.game();
    let node = game.tree.node(target.node);
    let parents = hu_engine::parent_array(&game.tree);
    let path = hu_engine::path_from_root(&parents, target.node);
    let reach = hu_engine::reach_on_path(
        &game.tree,
        PerPlayer::new(
            game.root_ranges[Player::P0].as_slice(),
            game.root_ranges[Player::P1].as_slice(),
        ),
        &path,
        |id, _, out| out.copy_from_slice(&whole.average_strategy_at(id)),
    );
    let strategy = whole.average_strategy_at(target.node);
    let hands = game.tree.storage_ref(node).num_hands as usize;
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for &hand in &target.hands {
        let h = hand as usize;
        let weight = f64::from(reach[node.player][h]);
        denominator += weight;
        numerator += weight * f64::from(strategy[target.action * hands + h]);
    }
    assert!((numerator / denominator - full.frequencies[0].achieved.unwrap()).abs() < 1e-5);
    for id in 0..whole.game().tree.nodes.len() as u32 {
        if inside[id as usize] && whole.game().tree.node(id).kind == NodeKind::Action {
            let expected = whole.average_strategy_at(id);
            let actual = result.stored_strategy(id).unwrap().unwrap();
            assert_eq!(expected.len(), actual.len());
            for (x, y) in expected.iter().zip(actual) {
                assert!((x - y).abs() < 1e-5, "node {id}: {x} vs {y}");
            }
        }
    }
}

#[test]
fn root_profile_falls_back_to_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spot.toml");
    let cancel = AtomicBool::new(false);
    let base_path = dir.path().join("base.sol");
    let base = prepare::prepare(RIVER, &path).unwrap();
    execute(&base, &cancel, &dir.path().join("base.ckpt"), &base_path);
    let entries = "[[profile.lock]]\nnode = 'root'\nstrategy = { check = 1.0 }";
    let p = prepare::prepare(&frozen_input(&base_path, entries), &path).unwrap();
    let ckpt = dir.path().join("root.ckpt");
    let summary = execute(&p, &cancel, &ckpt, &dir.path().join("root.sol"));
    let scope = summary.solve_scope.unwrap();
    assert_eq!(scope.scope, "whole-tree");
    assert!(scope.fallback_reason.is_some());
    assert_eq!(
        checkpoint::CheckpointReader::open(&ckpt).unwrap().mode,
        checkpoint::CheckpointMode::WholeTree
    );
}

#[test]
fn missing_outside_base_strategy_falls_back_to_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spot.toml");
    let cancel = AtomicBool::new(false);
    let turn = RIVER
        .replace(
            "BTN c, BB x / BB x, BTN x / BB x, BTN x",
            "BTN c, BB x / BB x, BTN x",
        )
        .replace("2s 7s Ks 4h 3d", "2s 7s Ks 4h")
        .replace(
            "river { replace bet [50%] }",
            "turn { replace bet [50%] } river { replace bet [50%] }",
        )
        .replace("max_iterations = 50", "max_iterations = 2");
    let base_path = dir.path().join("base.sol");
    let base = prepare::prepare(
        &format!("{turn}\n[output]\nsolution_streets = 'no-rivers'\n"),
        &path,
    )
    .unwrap();
    execute(&base, &cancel, &dir.path().join("base.ckpt"), &base_path);
    let base_text = base_path.to_string_lossy().replace('\\', "/");
    let input = format!(
        "{turn}\n[profile]\nfreeze_upstream = {{ solution = '{base_text}' }}\n[[profile.lock]]\nnode = 'x'\nstrategy = {{ check = 1.0 }}\n"
    );
    let p = prepare::prepare(&input, &path).unwrap();
    let ckpt = dir.path().join("fallback.ckpt");
    let summary = execute(&p, &cancel, &ckpt, &dir.path().join("fallback.sol"));
    let scope = summary.solve_scope.unwrap();
    assert_eq!(scope.scope, "whole-tree");
    assert_eq!(scope.fallback_reason, Some("missing-base-strategy"));
    assert_eq!(
        checkpoint::CheckpointReader::open(&ckpt).unwrap().mode,
        checkpoint::CheckpointMode::WholeTree
    );
}
