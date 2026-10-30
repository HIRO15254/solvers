//! M6 CLI acceptance. Production solves are explicit release acceptance because
//! a cold EHS² table build exceeds the ordinary workspace-test time budget.
#![cfg(feature = "p2")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn text(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Output {
    let output = cli(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn json(args: &[&str]) -> serde_json::Value {
    serde_json::from_slice(&ok(args).stdout).unwrap()
}

#[test]
fn p2_validation_effective_resources_types_and_preflight_before_directory() {
    let started = Instant::now();
    let dir = tempfile::tempdir().unwrap();
    let config =
        root().join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_production_smoke.toml");
    let effective = dir.path().join("effective.toml");
    let value = json(&[
        "validate",
        text(&config),
        "--format",
        "json",
        "--resources",
        "--show-effective",
        "--write-effective",
        text(&effective),
    ]);
    assert_eq!(value["product"], "MultiwayPreflop");
    assert_eq!(value["gameKind"], "mw-preflop");
    assert_eq!(value["table"]["firstActor"], "BTN");
    assert_eq!(value["ranges"][0]["combos"], 1326);
    assert_eq!(value["schema"], "solvers.nlh/v1");
    assert_eq!(value["resources"]["withinLimit"], true);
    assert_eq!(value["ruleHitStatus"], "complete");
    assert!(value["resources"]["solverStateBytes"].as_u64().unwrap() > 0);
    assert_eq!(value["effectiveConfig"]["solver"]["stop"]["max_sweeps"], 1);
    let effective2 = dir.path().join("effective2.toml");
    let human = ok(&[
        "validate",
        text(&effective),
        "--show-effective",
        "--write-effective",
        text(&effective2),
    ]);
    assert!(String::from_utf8_lossy(&human.stdout).contains("P2 (Multiway Preflop)"));
    assert!(String::from_utf8_lossy(&human.stdout).contains("rule hits: not-checked"));
    assert!(String::from_utf8_lossy(&human.stdout).contains("use validate --resources"));
    assert_eq!(
        std::fs::read(&effective).unwrap(),
        std::fs::read(effective2).unwrap()
    );
    let out = dir.path().join("refused");
    let failed = cli(&["solve", text(&config), "--out", text(&out), "--memory", "1"]);
    assert!(!failed.status.success());
    assert!(!out.exists());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("NLH003"));
    let input = dir.path().join("input.toml");
    for (extra, code, key) in [
        ("solver = 2", "NLH002", "solver"),
        ("output = 2", "NLH002", "output"),
        ("[solver]\nseed = '19'", "NLH002", "solver.seed"),
        (
            "[solver.stop]\ntarget = '0.05'",
            "NLH002",
            "solver.stop.target",
        ),
        (
            "[solver.abstraction.buckets]\nflop = '2'",
            "NLH002",
            "solver.abstraction.buckets.flop",
        ),
        (
            "[output]\nprobability_encoding = []",
            "NLH002",
            "output.probability_encoding",
        ),
        ("[solver]\nseed = -1", "NLH003", "solver.seed"),
        (
            "[solver]\nstorage = 'f32'",
            "NLH002",
            "this spot is solved by P2",
        ),
    ] {
        let (root_keys, sections) = if extra.starts_with('[') {
            ("", extra)
        } else {
            (extra, "")
        };
        std::fs::write(&input, format!("schema = 'solvers.nlh/v1'\n{root_keys}\n[table]\nplayers = 3\nstack = 2\n{sections}\n")).unwrap();
        let error = cli(&["validate", text(&input)]);
        let stderr = String::from_utf8_lossy(&error.stderr);
        assert!(!error.status.success());
        assert!(stderr.contains(code) && stderr.contains(key), "{stderr}");
    }
    std::fs::write(&input, "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack = 2\n[tree]\nscript = 'preflop when players == 9 { add raise [a] }'\n").unwrap();
    let plain = json(&["validate", text(&input), "--format", "json"]);
    assert_eq!(plain["ruleHitStatus"], "not-checked");
    assert_eq!(
        plain["warnings"],
        serde_json::json!([
            "unused tree rules not checked; use validate --resources to check them"
        ])
    );
    let checked = json(&["validate", text(&input), "--format", "json", "--resources"]);
    assert_eq!(checked["ruleHitStatus"], "complete");
    assert_eq!(
        checked["warnings"],
        serde_json::json!(["unmatched Preflop tree rule 1"])
    );
    let human = ok(&["validate", text(&input), "--resources"]);
    assert!(
        String::from_utf8_lossy(&human.stdout).contains("warning: unmatched Preflop tree rule 1")
    );
    let limited = std::fs::read_to_string(&input).unwrap() + "[run]\nmemory = 1\n";
    std::fs::write(&input, limited).unwrap();
    let incomplete = json(&["validate", text(&input), "--format", "json", "--resources"]);
    assert_eq!(incomplete["ruleHitStatus"], "incomplete");
    assert_eq!(incomplete["resources"]["complete"], false);
    assert_eq!(incomplete["warnings"], serde_json::json!([]));
    let human = ok(&["validate", text(&input), "--resources"]);
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("rule hits: incomplete"));
    assert!(!text.contains("unmatched"));
    eprintln!(
        "M6 validate wall time: {:.3}s",
        started.elapsed().as_secs_f64()
    );
}

#[test]
fn p2_rounding_warning_reaches_both_validation_formats_once() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.toml");
    let effective = dir.path().join("effective.toml");
    std::fs::write(&input, "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack = 2.0004\n[tree]\nscript = 'preflop when players == 9 { add raise [a] }'\n").unwrap();
    for resources in [false, true] {
        let mut args = vec![
            "validate",
            text(&input),
            "--format",
            "json",
            "--write-effective",
            text(&effective),
        ];
        if resources {
            args.push("--resources");
        }
        let value = json(&args);
        let warnings = value["warnings"].as_array().unwrap();
        let rounded: Vec<_> = warnings
            .iter()
            .filter_map(|w| w.as_str())
            .filter(|w| w.contains("table.stack"))
            .collect();
        assert_eq!(rounded.len(), 1, "{warnings:?}");
        assert!(rounded[0].contains("2.0004"), "{}", rounded[0]);
        assert!(
            rounded[0].contains("2000") && rounded[0].contains("nearest"),
            "{}",
            rounded[0]
        );
        let expected = if resources {
            "unmatched Preflop tree rule 1"
        } else {
            "unused tree rules not checked"
        };
        assert!(
            warnings
                .iter()
                .any(|w| w.as_str().unwrap().contains(expected))
        );
        let mut human_args = vec!["validate", text(&input)];
        if resources {
            human_args.push("--resources");
        }
        let human = ok(&human_args);
        let human = String::from_utf8_lossy(&human.stdout);
        assert_eq!(
            human.matches(&format!("warning: {}", rounded[0])).count(),
            1
        );
    }
    let effective_value: toml::Value =
        toml::from_str(&std::fs::read_to_string(&effective).unwrap()).unwrap();
    assert_eq!(effective_value["table"]["stack"].as_str(), Some("2000c"));
    assert_eq!(effective_value["table"]["sb"].as_str(), Some("500c"));
    let normalized = json(&["validate", text(&effective), "--format", "json"]);
    assert!(
        !normalized["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("table.stack"))
    );
}

#[test]
#[ignore = "production EHS² cache build and repeated artifact evaluation; run explicitly with --release --ignored"]
fn fixed_seed_smoke_resume_and_inspect_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let config =
        root().join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_3max_smoke.toml");
    let run = dir.path().join("partial");
    ok(&[
        "solve",
        text(&config),
        "--out",
        text(&run),
        "--threads",
        "1",
    ]);
    let resumed = dir.path().join("resumed");
    ok(&[
        "resume",
        text(&run),
        "--out",
        text(&resumed),
        "--max-sweeps",
        "3",
        "--threads",
        "1",
    ]);
    // `solve` takes the sweep ceiling from the config; only `resume` overrides it.
    let direct_config = dir.path().join("direct.toml");
    let fixture = std::fs::read_to_string(&config).unwrap();
    assert!(fixture.contains("max_sweeps = 2"));
    std::fs::write(
        &direct_config,
        fixture.replace("max_sweeps = 2", "max_sweeps = 3"),
    )
    .unwrap();
    let direct = dir.path().join("direct");
    ok(&[
        "solve",
        text(&direct_config),
        "--out",
        text(&direct),
        "--threads",
        "1",
    ]);
    let sol = resumed.join("solution.mwsol");
    let direct_sol = direct.join("solution.mwsol");
    for view in ["summary", "strategy", "range", "ev", "tree", "actions"] {
        assert_eq!(
            ok(&["export", text(&sol), view]).stdout,
            ok(&["export", text(&direct_sol), view]).stdout,
            "{view}"
        );
    }
    let compare = json(&["compare", text(&sol), text(&sol)]);
    assert!(compare.is_object());
    let evaluated = json(&[
        "evaluate",
        text(&sol),
        "--samples",
        "8",
        "--seed",
        "7",
        "--br-traversals",
        "1",
    ]);
    assert_eq!(evaluated["samples"], 8);
    let ev = json(&[
        "inspect",
        text(&sol),
        "--node",
        "1",
        "--view",
        "ev",
        "--samples",
        "8",
        "--seed",
        "9",
    ]);
    assert!(ev["evaluation"].is_object());
    let manifest = json(&["status", text(&resumed), "--format", "json"]);
    assert!(manifest.to_string().contains("mw-preflop"));
    ok(&["runs", "ls", text(dir.path()), "--format", "json"]);
    let watched = ok(&[
        "watch",
        text(&resumed),
        "--format",
        "json",
        "--poll-secs",
        "0.05",
    ]);
    assert!(String::from_utf8_lossy(&watched.stdout).contains("completed"));
    let effective = std::fs::read_to_string(resumed.join("run.toml")).unwrap();
    let artifact = mw_preflop::mwsol::MwSolReader::open(&sol).unwrap();
    assert_eq!(artifact.metadata().config_toml, effective);
    let checkpoint = mw_preflop::checkpoint::MultiwayCheckpoint::load_unchecked(
        &resumed.join("checkpoint.mwckpt"),
    )
    .unwrap();
    assert_eq!(checkpoint.config_toml.as_deref(), Some(effective.as_str()));
}
