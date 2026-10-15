//! A P1-only binary keeps the CLI surface but rejects unavailable P2 work.
#![cfg(not(feature = "p2"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXCLUSION: &str =
    "this build excludes P2 (Multiway Preflop); use a build with the `p2` feature";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn rejects(args: &[&str], code: i32) -> String {
    let output = cli(args);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(code), "{args:?}: {stderr}");
    assert!(stderr.contains(EXCLUSION), "{args:?}: {stderr}");
    stderr
}

fn rejects_input(args: &[&str]) {
    let stderr = rejects(args, 2);
    assert!(stderr.contains("NLH005"), "{args:?}: {stderr}");
}

fn manifest_only_run(directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "runId": "p2-run",
        "state": "completed",
        "gameKind": "mw-preflop",
        "configSchema": "solvers.nlh/v1",
        "configHash": "aa".repeat(32),
        "cliVersion": "0.1.0",
        "command": ["solve", "p2.toml"],
        "pid": 0,
        "createdUnixMs": 1_700_000_000_000u64,
        "startedUnixMs": 1_700_000_000_000u64,
        "finishedUnixMs": 1_700_000_060_000u64,
        "failure": null,
        "completion": "target-reached",
    });
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn p2_inputs_are_typed_input_errors_before_starting_work() {
    let directory = tempfile::tempdir().unwrap();
    let config = root().join("examples/mw-preflop/3max_smoke.toml");
    let run = directory.path().join("run");
    for args in [
        vec!["validate", text(&config)],
        vec!["validate", text(&config), "--format", "json"],
        vec!["solve", text(&config), "--out", text(&run)],
        vec!["tree", text(&config)],
        vec!["inspect", text(&config)],
        vec!["report", text(&config), "--boards", "Ks 7h 2d"],
    ] {
        rejects_input(&args);
    }
    assert!(!run.exists(), "rejected solve must not create a run");
}

#[test]
fn config_new_defaults_to_a_valid_p1_template() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("default.toml");
    let created = cli(&["config", "new", "--out", text(&out)]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let validated = cli(&["validate", text(&out)]);
    let stdout = String::from_utf8_lossy(&validated.stdout);
    assert!(
        validated.status.success(),
        "{}",
        String::from_utf8_lossy(&validated.stderr)
    );
    assert!(stdout.contains("product=P1"), "{stdout}");
}

#[test]
fn p2_templates_are_typed_input_errors() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("p2.toml");
    for template in ["minimal", "full"] {
        rejects_input(&[
            "config",
            "new",
            "--product",
            "p2",
            "--template",
            template,
            "--out",
            text(&out),
        ]);
    }
    assert!(!out.exists());
}

#[test]
fn derive_remains_discoverable_but_rejects_valid_arguments() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("derive"));
    let help = cli(&["derive", "--help"]);
    assert!(help.status.success());
    let directory = tempfile::tempdir().unwrap();
    let run = directory.path().join("p2-run");
    manifest_only_run(&run);
    let out = directory.path().join("derived.toml");
    let stderr = rejects(
        &[
            "derive",
            text(&run),
            "--line",
            "BTN c, BB x",
            "--board",
            "Ks 7h 2d",
            "--out",
            text(&out),
        ],
        2,
    );
    assert!(stderr.contains("derive requires P2"), "{stderr}");
    assert!(!out.exists());
}

#[test]
fn p2_artifact_extensions_reject_before_decoding_fake_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let solution = directory.path().join("fake.mwsol");
    let checkpoint = directory.path().join("fake.mwckpt");
    let p1 = directory.path().join("fake.sol");
    for artifact in [&solution, &checkpoint, &p1] {
        std::fs::write(artifact, b"not a real artifact").unwrap();
    }
    for args in [
        vec!["export", text(&solution), "summary"],
        vec!["evaluate", text(&solution)],
        vec!["resume", text(&checkpoint)],
        vec!["compare", text(&solution), text(&solution)],
        vec!["compare", text(&solution), text(&p1)],
        vec!["compare", text(&p1), text(&solution)],
        vec!["inspect", "--sol", text(&solution)],
    ] {
        rejects(&args, 3);
    }
}

#[test]
fn p2_manifest_rejects_artifact_commands_without_payloads_or_config() {
    let directory = tempfile::tempdir().unwrap();
    let run = directory.path().join("p2-run");
    manifest_only_run(&run);
    for args in [
        vec!["export", text(&run), "summary"],
        vec!["evaluate", text(&run)],
        vec!["resume", text(&run)],
        vec!["compare", text(&run), text(&run)],
        vec!["inspect", "--sol", text(&run)],
    ] {
        rejects(&args, 3);
    }
}

#[test]
fn p1_example_still_validates() {
    let config = root().join("examples/hu-postflop/river_small.toml");
    let output = cli(&["validate", text(&config), "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["gameKind"], "hu-postflop");
}
