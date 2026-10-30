use hu_engine::{F32Storage, I16Storage, MixedStorage, Solver, Storage};
use hu_postflop::{checkpoint, prepare, run};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const CONFIG: &str = r#"
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
[solver.stop]
max_iterations = 2
check_every = 2
[run]
memory = "1GiB"
threads = 1
"#;
fn execute(p: &prepare::Prepared, ckpt: &Path, sol: &Path) -> run::RunSummary {
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
fn compare<S: Storage>(p: &prepare::Prepared, checkpoint_path: &Path) {
    let (game, _, profile) = run::build_profiled(p).unwrap();
    let mut whole = Solver::<_, S>::new(game, run::schedule(&p.settings.solver.algorithm), Some(2));
    whole.set_cfr_precision(p.settings.solver.cfr_precision);
    run::install_profile(&mut whole, profile, None).unwrap();
    whole.run(2);
    let expected = whole.state();
    let reader = checkpoint::CheckpointReader::open(checkpoint_path).unwrap();
    let iteration = reader.iteration;
    whole
        .restore_stream(iteration, |storage| reader.read_storage(storage))
        .unwrap();
    assert_eq!(expected.storage, whole.state().storage);
}
#[test]
fn unattainable_outside_floor_falls_back_to_zero_initialized_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.toml");
    let base = dir.path().join("base.sol");
    let p = prepare::prepare(CONFIG, &path).unwrap();
    execute(&p, &dir.path().join("base.ckpt"), &base);
    for (storage, check) in [
        ("f32", compare::<F32Storage> as fn(&_, &_)),
        ("i16", compare::<I16Storage>),
        ("i16-f32avg", compare::<MixedStorage>),
    ] {
        let source = format!(
            "{}\n[solver]\nstorage = '{storage}'\n[profile]\nfreeze_upstream = {{ solution = '{}' }}\n[[profile.lock]]\nnode = 'x'\nstrategy = {{ check = 1.0 }}\n",
            CONFIG.replace(
                "max_iterations = 2",
                "max_iterations = 2\ntarget = '0.0000001%pot'"
            ),
            base.to_string_lossy().replace('\\', "/")
        );
        let p = prepare::prepare(&source, &path).unwrap();
        let ckpt = dir.path().join(format!("{storage}.ckpt"));
        let summary = execute(&p, &ckpt, &dir.path().join(format!("{storage}.sol")));
        assert_eq!(
            summary.solve_scope.unwrap().fallback_reason,
            Some("outside-residual")
        );
        assert_eq!(
            checkpoint::CheckpointReader::open(&ckpt).unwrap().mode,
            checkpoint::CheckpointMode::WholeTree
        );
        check(&p, &ckpt);
    }
}

#[test]
fn unexpected_fallback_checks_full_tree_budget_before_allocating_storage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.toml");
    let base = dir.path().join("base.sol");
    let prepared = prepare::prepare(CONFIG, &path).unwrap();
    execute(&prepared, &dir.path().join("base.ckpt"), &base);
    let source = format!(
        "{}\n[solver]\nstorage = 'f32'\n[profile]\nfreeze_upstream = {{ solution = '{}' }}\n[[profile.lock]]\nnode = 'x'\nstrategy = {{ check = 1.0 }}\n",
        CONFIG.replace(
            "max_iterations = 2",
            "max_iterations = 2\ntarget = '10000bb'"
        ),
        base.to_string_lossy().replace('\\', "/")
    );
    let mut p = prepare::prepare(&source, &path).unwrap();
    assert!(p.subtree_spans.is_some());
    // Simulate a stale proof and a limit that fits its compact budget but not
    // the fallback arena. The tight stop target makes outside-residual fire.
    p.target = Some(1e-12);
    let mut fallback = p.estimate.clone();
    fallback.f32_bytes = fallback.f32_bytes.saturating_add(1_000_000_000);
    p.fallback_estimate = Some(fallback);
    p.limit = prepare::required_bytes(&p);
    let error = run::run(
        run::RunRequest {
            prepared: &p,
            checkpoint: &dir.path().join("fallback.ckpt"),
            solution: &dir.path().join("fallback.sol"),
            state: None,
            evaluation_history: vec![],
            elapsed_before: Duration::ZERO,
            cancel: &AtomicBool::new(false),
        },
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .err()
    .expect("fallback must be rejected");
    assert!(format!("{error:#}").contains("freeze_upstream fallback outside-residual"));
}
