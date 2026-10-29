use hu_engine::NodeKind;
use hu_postflop::{artifact, prepare, run};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const TURN: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 4s"
[ranges]
BB = "AK,44"
BTN = "AK,44"
[tree]
script = "turn { checkdown } river { replace bet [50] } river when aggressions >= 1 { remove raise }"
[solver]
storage = "f32"
[solver.stop]
max_iterations = 50
check_every = 50
[run]
memory = "1GiB"
threads = 1
"#;

fn solve(p: &prepare::Prepared, ckpt: &std::path::Path, sol: &std::path::Path) {
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
    .unwrap();
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
