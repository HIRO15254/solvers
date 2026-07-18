//! Headless end-to-end drive of the GUI solve worker: build a session from
//! the 3-max smoke config, run it to completion through the worker protocol,
//! and check the written `.mwsol` plus the emitted event stream. This proves
//! the GUI's solve path without a window; `egui::Context::default()` makes
//! `request_repaint` a no-op.

use std::time::{Duration, Instant};

use gui::worker::{self, RunTarget, WorkerCmd, WorkerEvent};

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
            max_wall_time_secs: None,
        },
        eframe::egui::Context::default(),
    );

    let deadline = Instant::now() + Duration::from_secs(600);
    let mut saw_building = false;
    let mut saw_progress = false;
    let mut saw_evaluation = false;
    let mut saw_finite_rates = false;
    let mut watch_sent = false;
    let mut saw_node_strategies = false;
    let mut eval_sent = false;
    let mut saw_node_evaluation = false;
    let mut root_actions: Option<Vec<String>> = None;
    let finished = loop {
        assert!(Instant::now() < deadline, "worker did not finish in time");
        match handle.events.recv_timeout(Duration::from_secs(600)) {
            Ok(WorkerEvent::Building) => saw_building = true,
            Ok(WorkerEvent::Progress(snapshot)) => {
                assert!(snapshot.sweeps <= snapshot.target);
                assert!(snapshot.sweeps_per_sec.is_finite());
                assert!(snapshot.traversals_per_sec.is_finite());
                assert!(snapshot.hand_updates_per_sec.is_finite());
                saw_progress = true;
                saw_finite_rates = true;
                // Start watching ROOT (ties in with `App::start_solve`
                // sending this right after spawn) once the worker is past
                // the build phase, and check the live node view protocol:
                // the worker must answer with a `NodeStrategies` snapshot
                // for the watched node before the run finishes.
                if !watch_sent {
                    handle.send(WorkerCmd::WatchNode(Some([0; 16])));
                    watch_sent = true;
                }
                // Once we have seen at least one snapshot, exercise the
                // on-demand EV evaluation protocol at ROOT.
                if !eval_sent {
                    handle.send(WorkerCmd::EvaluateNode {
                        path: Vec::new(),
                        samples: 64,
                    });
                    eval_sent = true;
                }
            }
            Ok(WorkerEvent::NodeStrategies(snapshot)) => {
                assert_eq!(snapshot.history, [0; 16]);
                assert!(snapshot.path.is_empty(), "ROOT's path must be empty");
                assert!(
                    snapshot.action_path.is_empty(),
                    "ROOT's action path must be empty"
                );
                assert!(
                    !snapshot.children.is_empty() || !snapshot.blocks.is_empty(),
                    "expected ROOT to have children or strategy blocks"
                );
                for block in &snapshot.blocks {
                    let sum: f32 = block.probabilities.iter().sum();
                    assert!(
                        (sum - 1.0).abs() < 1e-2,
                        "block probabilities summed to {sum}"
                    );
                    assert_eq!(block.actions.len(), block.probabilities.len());
                }
                if root_actions.is_none()
                    && let Some(block) = snapshot.blocks.first()
                {
                    root_actions = Some(block.actions.clone());
                }
                saw_node_strategies = true;
            }
            Ok(WorkerEvent::NodeEvaluation(result)) => {
                assert!(
                    result.path.is_empty(),
                    "expected the reply for the requested ROOT path"
                );
                assert!(!result.evaluation.groups.is_empty());
                assert!(!result.evaluation.action_labels.is_empty());
                assert_eq!(
                    result.evaluation.aggregate.len(),
                    result.evaluation.action_labels.len()
                );
                if let Some(actions) = &root_actions {
                    assert_eq!(
                        &result.evaluation.action_labels, actions,
                        "NodeEvaluation's action labels must match ROOT's own strategy blocks"
                    );
                }
                saw_node_evaluation = true;
            }
            Ok(WorkerEvent::NodeEvaluationFailed { path, error }) => {
                panic!("unexpected NodeEvaluationFailed for path {path:?}: {error}")
            }
            Ok(WorkerEvent::Evaluated(_)) => saw_evaluation = true,
            Ok(WorkerEvent::Finished(finished)) => break finished,
            Ok(WorkerEvent::Cancelled) => panic!("worker cancelled unexpectedly"),
            Ok(WorkerEvent::Failed(message)) => panic!("worker failed: {message}"),
            Err(error) => panic!("worker event channel died: {error}"),
        }
    };

    assert!(saw_building && saw_progress && saw_evaluation && saw_finite_rates);
    assert!(
        saw_node_strategies,
        "expected at least one NodeStrategies event before Finished"
    );
    assert!(
        saw_node_evaluation,
        "expected a NodeEvaluation reply for the ROOT EvaluateNode request before Finished"
    );
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
