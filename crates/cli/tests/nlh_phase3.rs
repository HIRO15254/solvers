//! Phase 3 acceptance: common-input P1 CLI and self-contained run lifecycle.
use std::path::Path;
use std::process::{Command, Output};

const RIVER: &str = r#"schema = "solvers.nlh/v1"
[table]
players = 6
stack_bb = 5
[spot]
line = "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x"
board = "Ks 7h 2d 3c 9s"
[ranges]
BTN = "AhAd,QhQd"
BB = "JhJd,ThTd"
[tree]
script = '''
preflop { replace raise [2.5bb] }
river { replace bet [1bb] }
river when !unopened { replace raise [a] }
'''
[solver.stop]
max_iterations = 16
check_every = 4
[run]
threads = 1
memory = "1GiB"
checkpoint_interval = "15m"
"#;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Output {
    let result = cli(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}
fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn normalized_solution(path: &Path) -> Vec<u8> {
    let mut payload = hu_postflop::sol::read_sol(&path.join("solution.sol")).unwrap();
    payload.meta.wall_secs = 0.0;
    let mut config: toml_edit::DocumentMut = payload.config_toml.parse().unwrap();
    config.remove("run");
    payload.config_toml = config.to_string();
    postcard::to_allocvec(&payload).unwrap()
}

#[test]
fn omitted_final_checkpoint_resumes_periodic_state_with_crash_style_progress() {
    use hu_postflop::{prepare, run};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    let temp = tempfile::tempdir().unwrap();
    for storage in ["f32", "i16", "i16-f32avg"] {
        let raw = RIVER
            .replace(
                "[solver.stop]",
                &format!("[solver]\nstorage = '{storage}'\n[solver.stop]"),
            )
            .replace(
                "checkpoint_interval = \"15m\"",
                "checkpoint_interval = '1s'\nfinal_checkpoint = false",
            );
        let p = prepare::prepare(&raw, Path::new("periodic.toml")).unwrap();
        let directory = temp.path().join(storage);
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("run.toml"), &p.effective).unwrap();
        let checkpoint = directory.join("checkpoint.ckpt");
        let solution = directory.join("solution.sol");
        let mut metrics =
            runfiles::MetricsWriter::create_or_append(&directory.join("progress.jsonl")).unwrap();
        let mut saved = Vec::new();
        let summary = run::run(
            run::RunRequest {
                prepared: &p,
                checkpoint: &checkpoint,
                solution: &solution,
                state: None,
                elapsed_before: Duration::ZERO,
                cancel: &AtomicBool::new(false),
            },
            &mut |observation| {
                match observation {
                    run::Observation::Progress(row) => {
                        let first = row.iteration == 4;
                        metrics.append(&row)?;
                        // Force exactly one periodic checkpoint in a tiny river
                        // solve, with ample room for the remaining iterations.
                        if first {
                            std::thread::sleep(Duration::from_millis(1100));
                        }
                    }
                    run::Observation::Checkpoint { iterations } => saved.push(iterations),
                    _ => {}
                }
                Ok(())
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(summary.iterations, 16);
        assert_eq!(saved, [4]);
        assert_eq!(
            hu_postflop::checkpoint::read_checkpoint(&checkpoint)
                .unwrap()
                .iteration,
            4
        );
        let omitted_solution = normalized_solution(&directory);
        let before = std::fs::read(directory.join("progress.jsonl")).unwrap();
        // Editing this operational key must be accepted by compatibility_hash.
        std::fs::write(
            directory.join("run.toml"),
            p.effective
                .replace("final_checkpoint = false", "final_checkpoint = true"),
        )
        .unwrap();
        ok(&["resume", text(&directory), "--checkpoint-interval", "1h"]);
        let after = std::fs::read(directory.join("progress.jsonl")).unwrap();
        assert!(after.starts_with(&before));
        let iterations: Vec<_> = std::str::from_utf8(&after)
            .unwrap()
            .lines()
            .map(|line| {
                serde_json::from_str::<runfiles::MetricsRow>(line)
                    .unwrap()
                    .iteration
            })
            .collect();
        assert_eq!(iterations, [4, 8, 12, 16, 8, 12, 16]);
        let config = temp.path().join(format!("straight-{storage}.toml"));
        std::fs::write(
            &config,
            raw.replace("final_checkpoint = false", "final_checkpoint = true")
                .replace("'1s'", "'1h'"),
        )
        .unwrap();
        let straight = temp.path().join(format!("straight-{storage}"));
        ok(&["solve", text(&config), "--out", text(&straight)]);
        let resumed = hu_postflop::checkpoint::read_checkpoint(&checkpoint).unwrap();
        let once =
            hu_postflop::checkpoint::read_checkpoint(&straight.join("checkpoint.ckpt")).unwrap();
        assert_eq!(resumed.iteration, 16);
        assert_eq!(resumed.state, once.state);
        assert_eq!(
            normalized_solution(&directory),
            normalized_solution(&straight)
        );
        assert_eq!(omitted_solution, normalized_solution(&straight));
    }
}

#[test]
fn false_without_periodic_checkpoint_still_exports_but_cannot_resume() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("omit.toml");
    std::fs::write(&config, format!("{RIVER}\nfinal_checkpoint = false\n")).unwrap();
    let directory = temp.path().join("run");
    ok(&["solve", text(&config), "--out", text(&directory)]);
    assert!(!directory.join("checkpoint.ckpt").exists());
    assert!(directory.join("solution.sol").exists());
    let events = std::fs::read_to_string(directory.join("events.jsonl")).unwrap();
    assert!(!events.contains("\"kind\":\"checkpoint\""));
    let error = cli(&["resume", text(&directory)]);
    assert_eq!(error.status.code(), Some(1));
    let message = String::from_utf8_lossy(&error.stderr);
    assert!(message.contains("final_checkpoint = false"));
    assert!(message.contains("re-solve from run.toml"));
}

#[test]
fn finalization_paths_keep_run_pool_and_prioritize_checkpoint_errors() {
    use hu_postflop::{prepare, run};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    let temp = tempfile::tempdir().unwrap();
    for parallel in [false, true] {
        let mut p = prepare::prepare(
            &RIVER.replace("threads = 1", "threads = 3"),
            Path::new("river.toml"),
        )
        .unwrap();
        let peak = p
            .estimate
            .parallel_save_bytes(p.settings.solver.storage)
            .unwrap();
        p.limit = if parallel { peak } else { peak - 1 };
        assert!(p.estimate.required_bytes(p.settings.solver.storage) <= p.limit);
        for (checkpoint_error, solution_error) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let directory = tempfile::tempdir_in(temp.path()).unwrap();
            let checkpoint = directory.path().join(if checkpoint_error {
                "missing/checkpoint.ckpt"
            } else {
                "checkpoint.ckpt"
            });
            let solution = directory.path().join(if solution_error {
                "missing/solution.sol"
            } else {
                "solution.sol"
            });
            let mut checkpoints = 0;
            let result = run::run(
                run::RunRequest {
                    prepared: &p,
                    checkpoint: &checkpoint,
                    solution: &solution,
                    state: None,
                    elapsed_before: Duration::ZERO,
                    cancel: &AtomicBool::new(false),
                },
                &mut |o| {
                    assert_eq!(rayon::current_num_threads(), 3);
                    if matches!(o, run::Observation::Checkpoint { .. }) {
                        assert!(checkpoint.is_file());
                        checkpoints += 1;
                    }
                    Ok(())
                },
                &mut |_| assert_eq!(rayon::current_num_threads(), 3),
            );
            assert_eq!(checkpoints, usize::from(!checkpoint_error));
            if checkpoint_error {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .downcast_ref::<hu_postflop::checkpoint::CheckpointError>()
                        .is_some()
                );
                assert_eq!(solution.is_file(), parallel && !solution_error);
            } else if solution_error {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok());
            }
        }
    }
    // Omission applies to every stop reason, and never emits a final checkpoint.
    for reason in [
        "max-iterations",
        "target-reached",
        "time-limit",
        "cancelled",
    ] {
        let raw = format!("{}\nfinal_checkpoint = false\n", RIVER);
        let raw = match reason {
            "target-reached" => raw.replace(
                "max_iterations = 16",
                "max_iterations = 16\ntarget = '100bb'",
            ),
            "time-limit" => format!("{raw}max_time = '0.000001s'\n"),
            _ => raw,
        };
        let p = prepare::prepare(&raw, Path::new("omit.toml")).unwrap();
        let directory = tempfile::tempdir_in(temp.path()).unwrap();
        let checkpoint = directory.path().join("checkpoint.ckpt");
        let solution = directory.path().join("solution.sol");
        let cancel = AtomicBool::new(false);
        let mut stop = None;
        run::run(
            run::RunRequest {
                prepared: &p,
                checkpoint: &checkpoint,
                solution: &solution,
                state: None,
                elapsed_before: Duration::ZERO,
                cancel: &cancel,
            },
            &mut |o| {
                match o {
                    run::Observation::Progress(_) if reason == "cancelled" => {
                        cancel.store(true, Ordering::SeqCst)
                    }
                    run::Observation::Stop { reason } => stop = Some(reason),
                    run::Observation::Checkpoint { .. } => panic!("unexpected checkpoint"),
                    _ => {}
                }
                Ok(())
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(stop, Some(reason));
        assert!(!checkpoint.exists());
        assert!(solution.is_file());
    }
}

#[test]
fn spec_p1_example_validate_text_json_effective_and_resources() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("flop.toml");
    // The exact §14 P1 example is the maintained source, not a hand-copied approximation.
    let spec = include_str!("../../../docs/nlh-input-v1.jp.md").replace("\r\n", "\n");
    let example = spec
        .split("### P1: 6max")
        .nth(1)
        .unwrap()
        .split("```toml\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    std::fs::write(&config, example).unwrap();
    let effective = temp.path().join("effective.toml");
    let result = ok(&[
        "validate",
        text(&config),
        "--format",
        "json",
        "--show-effective",
        "--write-effective",
        text(&effective),
        "--resources",
    ]);
    let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(json["product"], "HuPostflop");
    assert_eq!(
        json["effectiveConfig"]["solver"]["algorithm"]["pow4_reset"],
        false
    );
    for (key, value) in [("alpha", 1.25), ("beta", 0.5), ("gamma", 4.0)] {
        assert_eq!(json["effectiveConfig"]["solver"]["algorithm"][key], value);
    }
    assert_eq!(json["start"]["pot"], 5.5);
    assert_eq!(json["start"]["effective_stack"], 97.5);
    assert_eq!(json["start"]["oop"]["position"], "BB");
    assert_eq!(json["start"]["ip"]["position"], "BTN");
    assert_eq!(json["actions"].as_array().unwrap().len(), 6);
    assert_eq!(
        json["actions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["implicit"] == true)
            .count(),
        4
    );
    assert_eq!(
        json["effectiveConfig"]["solver"]["parallel"]["chance_depth"],
        2
    );
    assert!(json["resources"]["memoryLimitBytes"].as_u64().unwrap() > 0);
    let effective2 = temp.path().join("effective2.toml");
    let result = ok(&[
        "validate",
        text(&effective),
        "--show-effective",
        "--write-effective",
        text(&effective2),
    ]);
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("P1 (HU Postflop)"));
    assert!(stdout.contains("5.5"));
    assert_eq!(
        std::fs::read(effective).unwrap(),
        std::fs::read(effective2).unwrap()
    );
}

#[test]
fn solve_resume_fork_effective_hashes_units_and_monitoring() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("river.toml");
    std::fs::write(&config, RIVER).unwrap();
    let run = temp.path().join("run");
    ok(&[
        "solve",
        text(&config),
        "--out",
        text(&run),
        "--threads",
        "1",
        "--memory",
        "1GiB",
        "--max-time",
        "1h",
    ]);
    let effective = std::fs::read_to_string(run.join("run.toml")).unwrap();
    assert!(effective.contains("pow4_reset = false"));
    let solution = hu_postflop::sol::read_sol(&run.join("solution.sol")).unwrap();
    let checkpoint =
        hu_postflop::checkpoint::read_checkpoint(&run.join("checkpoint.ckpt")).unwrap();
    assert_eq!(solution.config_toml, effective);
    assert_eq!(checkpoint.config_toml.as_deref(), Some(effective.as_str()));
    assert_eq!(
        checkpoint.config_hash,
        cli::nlh_v1::compatibility_hash(&effective).unwrap()
    );
    assert!((solution.meta.ev.iter().sum::<f64>() - 5.5).abs() < 1e-6);
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run.json")).unwrap()).unwrap();
    assert_eq!(result["utilityUnit"], "BB");
    assert_eq!(result["gameKind"], "hu-postflop");
    assert_eq!(result["configSchema"], "solvers.nlh/v1");
    assert_eq!(
        result["configHash"],
        runfiles::config_hash_hex(&runfiles::config_hash(effective.as_bytes()))
    );
    let original = std::fs::read(run.join("checkpoint.ckpt")).unwrap();
    let fork = temp.path().join("fork");
    ok(&[
        "resume",
        text(&run),
        "--out",
        text(&fork),
        "--threads",
        "2",
        "--memory",
        "2GiB",
        "--max-time",
        "2h",
        "--checkpoint-interval",
        "1s",
    ]);
    assert_eq!(
        std::fs::read(run.join("checkpoint.ckpt")).unwrap(),
        original
    );
    let fork_raw = std::fs::read_to_string(fork.join("run.toml")).unwrap();
    assert_eq!(
        cli::nlh_v1::compatibility_hash(&fork_raw).unwrap(),
        checkpoint.config_hash
    );
    let fork_config: toml::Value = toml::from_str(&fork_raw).unwrap();
    assert_eq!(fork_config["run"]["threads"].as_integer(), Some(2));
    let fork_checkpoint =
        hu_postflop::checkpoint::read_checkpoint(&fork.join("checkpoint.ckpt")).unwrap();
    assert_eq!(checkpoint.state, fork_checkpoint.state);
    ok(&["resume", text(&fork), "--threads", "1"]);
    let status = ok(&["status", text(&fork), "--format", "json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["gameKind"], "hu-postflop");
    assert!(status["elapsedSecs"].as_f64().unwrap() > 0.0);
    ok(&["runs", "ls", text(temp.path()), "--format", "json"]);
    ok(&["watch", text(&fork), "--format", "json"]);
    // A non-run edit must fail before a fork directory is created.
    std::fs::write(
        fork.join("run.toml"),
        fork_raw.replace("max_iterations = 16", "max_iterations = 17"),
    )
    .unwrap();
    let rejected = temp.path().join("rejected");
    assert_eq!(
        cli(&["resume", text(&fork), "--out", text(&rejected)])
            .status
            .code(),
        Some(3)
    );
    assert!(!rejected.exists());
}

#[test]
fn explicit_legacy_dcfr_saved_config_resumes_and_omitted_original_is_rejected() {
    for reset in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("reset.toml");
        let algorithm = format!(
            "[solver.algorithm]\nalpha = 1.5\nbeta = 0.0\ngamma = 3.0\npow4_reset = {reset}\n[solver.stop]"
        );
        let raw = RIVER.replace("[solver.stop]", &algorithm);
        std::fs::write(&config, &raw).unwrap();
        let run = temp.path().join("reset");
        ok(&[
            "solve",
            text(&config),
            "--out",
            text(&run),
            "--max-time",
            "0.000001s",
        ]);
        let checkpoint_path = run.join("checkpoint.ckpt");
        let checkpoint = hu_postflop::checkpoint::read_checkpoint(&checkpoint_path).unwrap();
        assert!(checkpoint.iteration < 16);
        let effective = std::fs::read_to_string(run.join("run.toml")).unwrap();
        for value in ["alpha = 1.5", "beta = 0.0", "gamma = 3.0"] {
            assert!(effective.contains(value));
        }
        assert!(effective.contains(&format!("pow4_reset = {reset}")));
        assert_eq!(checkpoint.config_toml.as_deref(), Some(effective.as_str()));
        let solution = hu_postflop::sol::read_sol(&run.join("solution.sol")).unwrap();
        assert_eq!(solution.config_toml, effective);
        let resumed = temp.path().join("resumed");
        ok(&[
            "resume",
            text(&checkpoint_path),
            "--out",
            text(&resumed),
            "--max-time",
            "1h",
        ]);
        let straight = temp.path().join("straight");
        ok(&["solve", text(&config), "--out", text(&straight)]);
        let resumed_state =
            hu_postflop::checkpoint::read_checkpoint(&resumed.join("checkpoint.ckpt")).unwrap();
        let straight_state =
            hu_postflop::checkpoint::read_checkpoint(&straight.join("checkpoint.ckpt")).unwrap();
        assert_eq!(resumed_state.iteration, 16);
        assert_eq!(resumed_state.state, straight_state.state);
        // Simulate replacing an old run's expanded config with its omitted original.
        std::fs::write(run.join("run.toml"), RIVER).unwrap();
        let rejected = temp.path().join("rejected-reset");
        let result = cli(&["resume", text(&run), "--out", text(&rejected)]);
        assert_eq!(result.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&result.stderr).contains("config hash does not match"));
        assert!(!rejected.exists());
    }
}

#[test]
fn i16_saved_false_from_pf7_resumes_without_adopting_new_default() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("pf7-i16.toml");
    let raw = RIVER.replace(
        "[solver.stop]",
        "[solver]\nstorage = 'i16'\n[solver.algorithm]\nalpha = 1.25\nbeta = 0.5\ngamma = 4.0\npow4_reset = false\n[solver.stop]",
    );
    std::fs::write(&config, &raw).unwrap();
    let partial = temp.path().join("partial");
    ok(&[
        "solve",
        text(&config),
        "--out",
        text(&partial),
        "--max-time",
        "0.000001s",
    ]);
    let effective = std::fs::read_to_string(partial.join("run.toml")).unwrap();
    assert!(effective.contains("pow4_reset = false"));
    let checkpoint =
        hu_postflop::checkpoint::read_checkpoint(&partial.join("checkpoint.ckpt")).unwrap();
    assert!(checkpoint.iteration < 16);
    assert_eq!(checkpoint.config_toml.as_deref(), Some(effective.as_str()));
    assert_eq!(
        hu_postflop::sol::read_sol(&partial.join("solution.sol"))
            .unwrap()
            .config_toml,
        effective
    );
    let resumed = temp.path().join("resumed");
    ok(&[
        "resume",
        text(&partial),
        "--out",
        text(&resumed),
        "--max-time",
        "1h",
    ]);
    let straight = temp.path().join("straight");
    ok(&["solve", text(&config), "--out", text(&straight)]);
    let resumed_state =
        hu_postflop::checkpoint::read_checkpoint(&resumed.join("checkpoint.ckpt")).unwrap();
    let straight_state =
        hu_postflop::checkpoint::read_checkpoint(&straight.join("checkpoint.ckpt")).unwrap();
    assert_eq!(resumed_state.iteration, 16);
    assert_eq!(resumed_state.state, straight_state.state);
    assert!(
        std::fs::read_to_string(resumed.join("run.toml"))
            .unwrap()
            .contains("pow4_reset = false")
    );
}

#[test]
fn periodic_checkpoint_and_cumulative_time_limit() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("river.toml");
    std::fs::write(&config, RIVER.replace("15m", "0.000001s")).unwrap();
    let run = temp.path().join("periodic");
    ok(&["solve", text(&config), "--out", text(&run)]);
    let events = std::fs::read_to_string(run.join("events.jsonl")).unwrap();
    assert_eq!(
        events
            .lines()
            .filter(|line| line.contains("\"kind\":\"checkpoint\""))
            .count(),
        4
    );
    let limited = temp.path().join("limited");
    ok(&[
        "solve",
        text(&config),
        "--out",
        text(&limited),
        "--max-time",
        "0.000001s",
    ]);
    let before =
        hu_postflop::checkpoint::read_checkpoint(&limited.join("checkpoint.ckpt")).unwrap();
    assert!(before.iteration < 16);
    ok(&["resume", text(&limited)]);
    let after = hu_postflop::checkpoint::read_checkpoint(&limited.join("checkpoint.ckpt")).unwrap();
    assert_eq!(before.iteration, after.iteration);
    assert!(after.elapsed_secs.unwrap() >= before.elapsed_secs.unwrap());
    ok(&["resume", text(&limited), "--max-time", "1h"]);
    let resumed =
        hu_postflop::checkpoint::read_checkpoint(&limited.join("checkpoint.ckpt")).unwrap();
    let straight = hu_postflop::checkpoint::read_checkpoint(&run.join("checkpoint.ckpt")).unwrap();
    assert_eq!(resumed.state, straight.state);
}

#[test]
fn rejects_memory_p2_sol_streets_and_empty_menus_before_creating_a_run() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("river.toml");
    std::fs::write(&config, RIVER).unwrap();
    let out = temp.path().join("out");
    let result = cli(&["solve", text(&config), "--out", text(&out), "--memory", "1"]);
    assert_eq!(result.status.code(), Some(75));
    assert!(!out.exists());
    std::fs::write(
        &config,
        RIVER.replace("[table]", "run = 1\n[table]").replace(
            "[run]\nthreads = 1\nmemory = \"1GiB\"\ncheckpoint_interval = \"15m\"",
            "",
        ),
    )
    .unwrap();
    let result = cli(&[
        "solve",
        text(&config),
        "--out",
        text(&out),
        "--threads",
        "1",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("NLH002"));
    assert!(!out.exists());
    std::fs::write(&config, RIVER).unwrap();
    // The removed command-line flag is a usage error; `[output] solution_streets` replaces it.
    let result = cli(&[
        "solve",
        text(&config),
        "--out",
        text(&out),
        "--sol-streets",
        "full",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--sol-streets"));
    assert!(!out.exists());
    std::fs::write(
        &config,
        "schema = \"solvers.nlh/v1\"\n[table]\nplayers = 2\nstack_bb = 5\n",
    )
    .unwrap();
    let result = ok(&["validate", text(&config)]);
    assert!(String::from_utf8_lossy(&result.stdout).contains("P2 (Multiway Preflop)"));
    std::fs::write(&config, RIVER.replace("replace bet [1bb]", "remove check")).unwrap();
    for args in [
        vec!["validate", text(&config)],
        vec!["solve", text(&config), "--out", text(&out)],
    ] {
        let result = cli(&args);
        assert_eq!(result.status.code(), Some(2));
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("NLH003") && error.contains("River"),
            "{error}"
        );
        assert!(!out.exists());
    }
}

#[test]
fn compatibility_hash_ignores_meta_and_run_only() {
    let base = r#"schema = "solvers.nlh/v1"

[meta]
name = "a"

[table]
players = 2
stack_bb = 100

[run]
threads = 1
"#;
    let hash = |text: &str| cli::nlh_v1::compatibility_hash(text).unwrap();
    assert_eq!(
        hash(base),
        hash(&base.replace("name = \"a\"", "name = \"b\""))
    );
    assert_eq!(
        hash(base),
        hash(&base.replace("threads = 1", "threads = 2"))
    );
    assert_ne!(
        hash(base),
        hash(&base.replace("stack_bb = 100", "stack_bb = 50"))
    );
}

fn p1_ignored_warning(text: &str) -> usize {
    text.lines()
        .filter(|line| line.contains("have no effect in P1"))
        .count()
}

#[test]
fn preflop_only_settings_warn_once_and_normalized_defaults_do_not_warn() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("input.toml");
    let base = RIVER.replace("preflop { replace raise [2.5bb] }\n", "");
    for (raw, expected) in [
        (RIVER.to_owned(), "Preflop tree rules"),
        (
            base.replace(
                "script =",
                "preflop_reraise_jam_above_stack = { numerator = 1, denominator = 2 }\nscript =",
            ),
            "tree.preflop_reraise_jam_above_stack",
        ),
        (
            base.clone() + "[tree.max_aggressive_actions]\npreflop = 5\n",
            "tree.max_aggressive_actions.preflop",
        ),
    ] {
        std::fs::write(&config, raw).unwrap();
        let result = ok(&["validate", text(&config), "--format", "json"]);
        let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let warnings: Vec<_> = json["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|w| w.as_str())
            .filter(|w| w.contains("have no effect in P1"))
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains(expected));
        let result = ok(&["validate", text(&config)]);
        assert_eq!(
            p1_ignored_warning(&String::from_utf8_lossy(&result.stdout)),
            1
        );
    }
    for template in ["minimal", "full"] {
        let result = ok(&["config", "new", "--product", "p1", "--template", template]);
        std::fs::write(&config, result.stdout).unwrap();
        let result = ok(&["validate", text(&config), "--format", "json"]);
        assert_eq!(
            p1_ignored_warning(&String::from_utf8_lossy(&result.stdout)),
            0
        );
    }
    std::fs::write(&config, &base).unwrap();
    let effective = temp.path().join("effective.toml");
    ok(&[
        "validate",
        text(&config),
        "--write-effective",
        text(&effective),
    ]);
    let result = ok(&["validate", text(&effective), "--format", "json"]);
    assert_eq!(
        p1_ignored_warning(&String::from_utf8_lossy(&result.stdout)),
        0
    );

    // All three triggers share one warning through solve, resume and report.
    let raw = RIVER.replace(
        "script =",
        "preflop_reraise_jam_above_stack = { numerator = 1, denominator = 2 }\nscript =",
    ) + "[tree.max_aggressive_actions]\npreflop = 5\n";
    std::fs::write(&config, raw).unwrap();
    let run = temp.path().join("run");
    for args in [
        vec!["solve", text(&config), "--out", text(&run)],
        vec!["resume", text(&run)],
    ] {
        let result = ok(&args);
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert_eq!(p1_ignored_warning(&stderr), 1, "{stderr}");
        for name in [
            "Preflop tree rules",
            "tree.preflop_reraise_jam_above_stack",
            "tree.max_aggressive_actions.preflop",
        ] {
            assert!(stderr.contains(name));
        }
    }
    let report = temp.path().join("report.csv");
    let result = ok(&[
        "report",
        text(&config),
        "--boards",
        "Ks7h2d3c9s,Ks7h2d3c8s",
        "--output",
        text(&report),
    ]);
    assert_eq!(
        p1_ignored_warning(&String::from_utf8_lossy(&result.stderr)),
        1
    );
}

#[test]
fn fold_only_checkdown_nodes_solve_save_export_inspect_and_report() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("input.toml");
    let raw = RIVER
        .replace("preflop { replace raise [2.5bb] }\n", "")
        .replace(
            "river { replace bet [1bb] }",
            "river when unopened { force bet [1bb] }",
        )
        .replace(
            "river when !unopened { replace raise [a] }",
            "river when !unopened { checkdown }",
        );
    std::fs::write(&config, raw).unwrap();
    let run = temp.path().join("run");
    ok(&["solve", text(&config), "--out", text(&run)]);
    let solution = run.join("solution.sol");
    let sol = hu_postflop::sol::read_sol(&solution).unwrap();
    assert!((sol.meta.ev[0] - 5.5).abs() < 1e-6);
    assert!(sol.meta.ev[1].abs() < 1e-6);
    for view in ["tree", "strategy", "ev", "summary"] {
        ok(&["export", text(&solution), view, "--format", "json"]);
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["inspect", "--sol", text(&solution)])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"go 0\nshow\nev\nquit\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("fold"));
    let report = temp.path().join("report.csv");
    ok(&[
        "report",
        text(&config),
        "--boards",
        "Ks7h2d3c9s,Ks7h2d3c8s",
        "--output",
        text(&report),
    ]);
}
