//! Headless end-to-end drive of the GUI solve worker: build a session from
//! the 3-max smoke config, run it to completion through the worker protocol,
//! and check the written `.mwsol` plus the emitted event stream. This proves
//! the GUI's solve path without a window; `egui::Context::default()` makes
//! `request_repaint` a no-op.

use std::time::{Duration, Instant};

use gui::worker::{self, RunTarget, WorkerEvent};

#[test]
#[ignore = "trains the multiway rollout artifact; CI runs it in release"]
fn worker_runs_the_smoke_config_to_a_finished_solution() {
    let directory = tempfile::tempdir().unwrap();
    let output_path = directory.path().join("run.mwsol");
    let checkpoint_path = directory.path().join("run.mwckpt");
    let config_toml = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");

    let handle = worker::spawn(
        RunTarget {
            config_toml: config_toml.to_string(),
            resume_checkpoint: None,
            output_path: output_path.clone(),
            checkpoint_path: Some(checkpoint_path.clone()),
            check_every: 1,
        },
        eframe::egui::Context::default(),
    );

    let deadline = Instant::now() + Duration::from_secs(600);
    let mut saw_building = false;
    let mut saw_progress = false;
    let mut saw_evaluation = false;
    let finished = loop {
        assert!(Instant::now() < deadline, "worker did not finish in time");
        match handle.events.recv_timeout(Duration::from_secs(600)) {
            Ok(WorkerEvent::Building) => saw_building = true,
            Ok(WorkerEvent::Progress(snapshot)) => {
                assert!(snapshot.sweeps <= snapshot.target);
                saw_progress = true;
            }
            Ok(WorkerEvent::Evaluated(_)) => saw_evaluation = true,
            Ok(WorkerEvent::Finished(finished)) => break finished,
            Ok(WorkerEvent::Cancelled) => panic!("worker cancelled unexpectedly"),
            Ok(WorkerEvent::Failed(message)) => panic!("worker failed: {message}"),
            Err(error) => panic!("worker event channel died: {error}"),
        }
    };

    assert!(saw_building && saw_progress && saw_evaluation);
    assert_eq!(finished.mwsol_path, output_path);
    assert!(checkpoint_path.is_file());
    let solution = formats::read_mwsol(&output_path).unwrap();
    assert_eq!(solution.sweeps, 2);
    assert!(!solution.strategies.is_empty());
    assert_eq!(solution.strategies, finished.solution.strategies);

    // The Results matrix reconstructs preflop keys as
    // `[class, UNREACHED_BUCKET x3]`; at least one such lookup must hit a
    // real block or the 13x13 grid would render empty.
    let preflop_hits = solution
        .strategies
        .iter()
        .filter(|block| block.key.street == 0)
        .filter(|block| {
            let class = block.key.bucket_path[0];
            let key = formats::MultiwayStrategyKey {
                bucket_path: [
                    class,
                    multiway::solver::UNREACHED_BUCKET,
                    multiway::solver::UNREACHED_BUCKET,
                    multiway::solver::UNREACHED_BUCKET,
                ],
                ..block.key
            };
            solution.strategy(key).is_some()
        })
        .count();
    assert!(
        preflop_hits > 0,
        "matrix-style preflop key lookups found no blocks"
    );
}
