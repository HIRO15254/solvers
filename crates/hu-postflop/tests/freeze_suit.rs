use hu_engine::NodeKind;
use hu_postflop::{artifact, prepare, run};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const TURN: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 4s"
[ranges]
BB = "AK,44"
BTN = "AK,44"
[tree]
script = "turn { checkdown } river { replace bet [50%] } river when aggressions >= 1 { remove raise }"
[solver]
storage = "f32"
[solver.stop]
max_iterations = 50
check_every = 50
[run]
memory = "1GiB"
threads = 1
"#;

fn solve(p: &prepare::Prepared, ckpt: &std::path::Path, sol: &std::path::Path) -> run::RunSummary {
    run::run(
        run::RunRequest {
            prepared: p,
            checkpoint: ckpt,
            solution: sol,
            state: None,
            evaluation_history: vec![],
            elapsed_before: Duration::ZERO,
            cancel: &AtomicBool::new(false),
        },
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .unwrap()
}

#[test]
fn asymmetrical_frozen_river_against_merged_base() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.toml");
    let base_sol = dir.path().join("base.sol");
    let p = prepare::prepare(TURN, &path).unwrap();
    solve(&p, &dir.path().join("base.ckpt"), &base_sol);
    let base = artifact::load_sol(&base_sol, 0, None).unwrap();
    for selector in ["xx[?]x", "xx[5h]x"] {
        let input = format!(
            "{TURN}\n[profile]\nfreeze_upstream = {{ solution = '{}' }}\n[[profile.lock]]\nnode = '{selector}'\nhands = '4c4h'\nstrategy = {{ check = 1.0 }}\n",
            base_sol.display()
        );
        let profiled = prepare::prepare(&input, &path).unwrap();
        let (game, _, profile) = run::build_profiled(&profiled).unwrap();
        assert!(game.tree.nodes.len() > base.game.tree.nodes.len());
        let profile = profile.unwrap();
        assert!(profile.summary.iso_restricted);
        assert!(
            profile
                .frozen_nodes
                .iter()
                .any(|&id| id != 0 && game.tree.node(id).kind == NodeKind::Action)
        );
        solve(
            &profiled,
            &dir.path().join("freeze.ckpt"),
            &dir.path().join("freeze.sol"),
        );
    }
}

#[test]
fn frozen_relock_agrees_with_equivalently_restricted_base() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.toml");
    let plain = prepare::prepare(TURN, &path).unwrap();
    let merged_sol = dir.path().join("merged.sol");
    let merged_base = solve(&plain, &dir.path().join("merged.ckpt"), &merged_sol);
    // A zero incentive restricts the base grouping without changing its payoff.
    let restricted_input = format!(
        "{TURN}\n[profile]\n[[profile.incentive]]\nnode = 'xx[?]x'\nhands = '4c4h'\naction = 'check'\npot = 0.0\n"
    );
    let restricted = prepare::prepare(&restricted_input, &path).unwrap();
    let restricted_sol = dir.path().join("restricted.sol");
    let restricted_base = solve(
        &restricted,
        &dir.path().join("restricted.ckpt"),
        &restricted_sol,
    );
    let mut results = Vec::new();
    for (i, base) in [&merged_sol, &restricted_sol].into_iter().enumerate() {
        let input = format!(
            "{TURN}\n[profile]\nfreeze_upstream = {{ solution = '{}' }}\n[[profile.lock]]\nnode = 'xx[?]x'\nhands = '4c4h'\nstrategy = {{ check = 1.0 }}\n",
            base.display()
        );
        let p = prepare::prepare(&input, &path).unwrap();
        let output = dir.path().join(format!("result{i}.sol"));
        let summary = solve(&p, &dir.path().join(format!("result{i}.ckpt")), &output);
        results.push((summary, artifact::load_sol(&output, 0, None).unwrap()));
    }
    // Each base is only approximately converged; use their combined NashConv
    // as a conservative tolerance on the resulting root EV.
    let tolerance = merged_base.nash_conv + restricted_base.nash_conv + 1e-5;
    for seat in nlh::Player::BOTH {
        assert!((results[0].0.ev[seat] - results[1].0.ev[seat]).abs() <= tolerance);
    }
    let a = results[0].1.stored_strategy(0).unwrap().unwrap();
    let b = results[1].1.stored_strategy(0).unwrap().unwrap();
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= tolerance as f32);
    }
}
