use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hu_engine::{F32Storage, NodeKind, NodeLock, Solver, Storage, StorageArrays};
use hu_postflop::{artifact, checkpoint, prepare, run};
use nlh::Player;

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
    // Independently restore the checkpoint into an ordinary full-tree solver and evaluate it.
    let (game, _, resolved) = run::build_profiled(&p).unwrap();
    let mut full =
        Solver::<_, F32Storage>::new(game, run::schedule(&p.settings.solver.algorithm), Some(50));
    full.set_cfr_precision(p.settings.solver.cfr_precision);
    run::install_profile(&mut full, resolved, None).unwrap();
    let reader = checkpoint::CheckpointReader::open(&ckpt).unwrap();
    let iteration = reader.iteration;
    full.restore_stream(iteration, |storage| reader.read_storage(storage))
        .unwrap();
    let StorageArrays::F32 {
        regrets: whole_regrets,
        strategy_sum: whole_sum,
    } = whole.storage().arrays()
    else {
        unreachable!()
    };
    let StorageArrays::F32 {
        regrets: sub_regrets,
        strategy_sum: sub_sum,
    } = full.storage().arrays()
    else {
        unreachable!()
    };
    let span = whole.game().tree.storage_spans[target as usize];
    for (a, b) in [
        (
            &whole_regrets[span.start..span.end],
            &sub_regrets[span.start..span.end],
        ),
        (
            &whole_sum[span.start..span.end],
            &sub_sum[span.start..span.end],
        ),
    ] {
        assert!(a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits()));
    }
    let (_, expl) = full.evaluate();
    let ev = full.true_expected_values();
    let offset = p.payoff.ev_offset();
    for player in Player::BOTH {
        assert!((summary.ev[player] - ev[player] - offset[player]).abs() < 1e-8);
    }
    assert!((summary.expl_p0 - expl[Player::P0]).abs() < 1e-8);
    assert!((summary.expl_p1 - expl[Player::P1]).abs() < 1e-8);
    assert!((summary.nash_conv - expl[Player::P0] - expl[Player::P1]).abs() < 1e-8);
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
fn root_and_frequency_profiles_fall_back_to_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spot.toml");
    let cancel = AtomicBool::new(false);
    let base_path = dir.path().join("base.sol");
    let base = prepare::prepare(RIVER, &path).unwrap();
    execute(&base, &cancel, &dir.path().join("base.ckpt"), &base_path);
    for (name, entries) in [
        (
            "root",
            "[[profile.lock]]\nnode = 'root'\nstrategy = { check = 1.0 }",
        ),
        (
            "frequency",
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 1.0 }\n[[profile.frequency]]\nnode = 'root'\naction = 'check'\ntarget = 0.5",
        ),
    ] {
        let p = prepare::prepare(&frozen_input(&base_path, entries), &path).unwrap();
        let ckpt = dir.path().join(format!("{name}.ckpt"));
        let summary = execute(&p, &cancel, &ckpt, &dir.path().join(format!("{name}.sol")));
        let scope = summary.solve_scope.unwrap();
        assert_eq!(scope.scope, "whole-tree", "{name}");
        assert!(scope.fallback_reason.is_some(), "{name}");
        assert_eq!(
            checkpoint::CheckpointReader::open(&ckpt).unwrap().mode,
            checkpoint::CheckpointMode::WholeTree
        );
    }
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
            "river { replace bet [50] }",
            "turn { replace bet [50] } river { replace bet [50] }",
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
