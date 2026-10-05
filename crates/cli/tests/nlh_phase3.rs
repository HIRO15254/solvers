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

#[test]
fn draft_p1_validate_text_json_effective_and_resources() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("flop.toml");
    // The exact §14 P1 example is the maintained source, not a hand-copied approximation.
    let draft = include_str!("../../../docs/plans/nlh-input-v1.jp.md");
    let example = draft
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
        5
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
    for mode in ["full", "no-rivers"] {
        let result = cli(&[
            "solve",
            text(&config),
            "--out",
            text(&out),
            "--sol-streets",
            mode,
        ]);
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains("solution_streets"));
        assert!(!out.exists());
    }
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
