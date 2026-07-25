use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has a workspace parent")
        .parent()
        .expect("crates directory has a workspace parent")
        .to_path_buf()
}

fn run_solvers(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .expect("run solvers")
}

fn assert_error_code(output: &Output, code: &str) {
    assert!(
        !output.status.success(),
        "expected {code} rejection, stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(code),
        "expected {code} in stderr, got:\n{stderr}"
    );
}

#[test]
fn canonical_production_smoke_validates_without_building_ehs_tables() {
    let config = workspace_root()
        .join("examples")
        .join("preflop_multiway_v1_production_smoke.toml");
    let source: toml::Value = toml::from_str(
        &std::fs::read_to_string(&config).expect("read canonical production fixture"),
    )
    .expect("production fixture must be TOML");
    assert_eq!(
        source["game"]["information"]["recall"].as_str(),
        Some("current-street"),
        "the canonical source must make the production recall contract explicit"
    );
    let output = run_solvers(&[
        "validate",
        config.to_str().unwrap(),
        "--format",
        "json",
        "--show-effective",
    ]);
    assert!(
        output.status.success(),
        "production fixture failed validation\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validation output must be JSON");
    assert_eq!(summary["status"], "valid");
    assert_eq!(summary["schema"], "solvers.multiway-preflop/v1");
    assert_eq!(summary["seatCount"], 3);
    assert_eq!(
        summary["effectiveConfig"]["game"]["abstraction"]["kind"],
        "ehs2-percentile"
    );
    // current-street is the normalized default, so effective-config elides it.
    // Successful production validation above proves that lowering still
    // produced fixed current-street semantics.
    assert!(summary["effectiveConfig"]["game"]["information"].is_null());
}

#[test]
fn retired_v1_abstraction_options_have_stable_rejection_codes() {
    let root = workspace_root();
    let rollout = root.join("examples").join("preflop_multiway_v1_smoke.toml");
    let rollout_output = run_solvers(&["validate", rollout.to_str().unwrap()]);
    assert_error_code(&rollout_output, "MWP001");

    let production = root
        .join("examples")
        .join("preflop_multiway_v1_production_smoke.toml");
    let raw = std::fs::read_to_string(production).expect("read production smoke fixture");
    let full_recall = raw.replace("recall = \"current-street\"", "recall = \"bucket-history\"");
    assert_ne!(full_recall, raw, "recall fixture anchor changed");
    let directory = tempfile::tempdir().expect("create policy test tempdir");
    let config = directory.path().join("bucket-history.toml");
    std::fs::write(&config, full_recall).expect("write retired recall fixture");
    let recall_output = run_solvers(&["validate", config.to_str().unwrap()]);
    assert_error_code(&recall_output, "MWP002");
}

#[test]
#[cfg(not(feature = "research"))]
fn historical_legacy_acceptance_fixtures_are_rejected_by_release_solve() {
    for name in [
        "preflop_multiway_3max_smoke.toml",
        "preflop_multiway_9max_8b_smoke.toml",
        "preflop_multiway_6max_64b_desktop.toml",
        "preflop_multiway_hu_10bb_pushfold.toml",
    ] {
        let directory = tempfile::tempdir().expect("create rejection tempdir");
        let config = workspace_root().join("examples").join(name);
        let result = directory.path().join("result.json");
        let checkpoint = directory.path().join("result.mwckpt");
        let solution = directory.path().join("result.mwsol");
        let output = run_solvers(&[
            "solve",
            config.to_str().unwrap(),
            "--output",
            result.to_str().unwrap(),
            "--checkpoint",
            checkpoint.to_str().unwrap(),
            "--sol",
            solution.to_str().unwrap(),
        ]);
        assert_error_code(&output, "MWP003");
        assert!(!result.exists(), "{name} unexpectedly wrote a result");
        assert!(
            !checkpoint.exists(),
            "{name} unexpectedly wrote a checkpoint"
        );
        assert!(!solution.exists(), "{name} unexpectedly wrote a solution");
    }
}
