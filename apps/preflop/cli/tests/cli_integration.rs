use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn workspace_root() -> std::path::PathBuf {
    // apps/preflop/cli/tests -> up to workspace root
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, uniquely-named scratch directory under the OS temp dir, so
/// parallel `cargo test` runs never collide.
fn temp_dir(tag: &str) -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "solvers-preflop-cli-test-{}-{}-{}",
        std::process::id(),
        id,
        tag
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_solver(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_preflop-solver"))
        .args(args)
        .output()
        .expect("run preflop-solver")
}

fn run_solver_ok(args: &[&str]) -> std::process::Output {
    let output = run_solver(args);
    assert!(
        output.status.success(),
        "preflop-solver {:?} failed, stderr: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

// --- app gating -------------------------------------------------------------

#[test]
fn postflop_config_is_redirected_to_the_postflop_app() {
    let config = workspace_root().join("examples/river_small.toml");
    let output = run_solver(&["solve", config.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("postflop-solver"),
        "expected a pointer to the postflop app, got: {stderr}"
    );
}

// --- multiway ---------------------------------------------------------------

#[test]
#[ignore = "trains the multiway rollout artifact three times; CI runs it in release"]
fn multiway_resume_is_bit_identical_to_a_straight_run() {
    let dir = temp_dir("multiway-resume-equiv");
    let base_config =
        std::fs::read_to_string(workspace_root().join("examples/preflop_multiway_3max_smoke.toml"))
            .unwrap();
    let config_one = dir.join("one-thread.toml");
    let config_four = dir.join("four-threads.toml");
    let smaller_memory =
        base_config.replace("max_memory_bytes = 67108864", "max_memory_bytes = 33554432");
    assert_ne!(smaller_memory, base_config, "smoke memory fixture changed");
    std::fs::write(&config_one, format!("{smaller_memory}\nthreads = 1\n")).unwrap();
    std::fs::write(&config_four, format!("{base_config}\nthreads = 4\n")).unwrap();
    let straight_checkpoint = dir.join("straight.mwckpt");
    let resumed_checkpoint = dir.join("resumed.mwckpt");
    let straight_result = dir.join("straight.json");
    let resumed_result = dir.join("resumed.json");

    run_solver_ok(&[
        "solve",
        config_four.to_str().unwrap(),
        "--checkpoint",
        straight_checkpoint.to_str().unwrap(),
        "--output",
        straight_result.to_str().unwrap(),
    ]);
    run_solver_ok(&[
        "solve",
        config_one.to_str().unwrap(),
        "--iterations",
        "1",
        "--checkpoint",
        resumed_checkpoint.to_str().unwrap(),
    ]);
    run_solver_ok(&[
        "resume",
        config_four.to_str().unwrap(),
        "--checkpoint",
        resumed_checkpoint.to_str().unwrap(),
        "--output",
        resumed_result.to_str().unwrap(),
    ]);

    let straight = multiway::MultiwayCheckpoint::load_unchecked(&straight_checkpoint).unwrap();
    let resumed = multiway::MultiwayCheckpoint::load_unchecked(&resumed_checkpoint).unwrap();
    assert_eq!(straight.header.next_sample_id, 6);
    assert_eq!(straight.state, resumed.state);
    assert_eq!(
        std::fs::read(&straight_checkpoint).unwrap(),
        std::fs::read(&resumed_checkpoint).unwrap()
    );
    assert_eq!(
        straight.header.configuration_fingerprint,
        resumed.header.configuration_fingerprint
    );
    assert_eq!(
        straight.header.abstraction_fingerprint,
        resumed.header.abstraction_fingerprint
    );

    let straight_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(straight_result).unwrap()).unwrap();
    let resumed_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(resumed_result).unwrap()).unwrap();
    for field in [
        "sweeps",
        "traversals",
        "infosets",
        "memoryBytes",
        "totalDealAttempts",
        "meanDealAttempts",
        "seats",
        "strategyBlocks",
        "configHash",
    ] {
        assert_eq!(straight_json[field], resumed_json[field], "field {field}");
    }
}

#[test]
fn multiway_resource_limit_writes_an_implicit_checkpoint() {
    let dir = temp_dir("multiway-resource-limit");
    let raw =
        std::fs::read_to_string(workspace_root().join("examples/preflop_multiway_3max_smoke.toml"))
            .unwrap();
    let limited = raw.replace("max_memory_bytes = 67108864", "max_memory_bytes = 1");
    assert_ne!(raw, limited, "smoke config memory limit fixture changed");
    let config = dir.join("limited.toml");
    std::fs::write(&config, limited).unwrap();
    let output_path = dir.join("result.json");
    let checkpoint_path = dir.join("result.mwckpt");

    let command = run_solver_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&command.stdout);
    assert!(
        stdout.contains(&checkpoint_path.display().to_string()),
        "stdout must identify the recovery checkpoint: {stdout}"
    );
    assert!(checkpoint_path.is_file());
    let checkpoint = multiway::MultiwayCheckpoint::load_unchecked(&checkpoint_path).unwrap();
    assert_eq!(checkpoint.header.next_sample_id, 0);
    assert_eq!(checkpoint.state.completed_sweeps, 0);

    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
    assert_eq!(result["status"], "resource_limit");
}

#[test]
#[ignore = "trains the multiway rollout artifact; CI runs it in release"]
fn multiway_i16_storage_writes_a_quantized_mwsol() {
    let dir = temp_dir("multiway-i16");
    let raw =
        std::fs::read_to_string(workspace_root().join("examples/preflop_multiway_3max_smoke.toml"))
            .unwrap();
    let quantized = raw.replace("storage = \"f32\"", "storage = \"i16\"");
    assert_ne!(raw, quantized, "smoke config storage fixture changed");
    let config = dir.join("i16.toml");
    std::fs::write(&config, quantized).unwrap();
    let solution_path = dir.join("result.mwsol");

    run_solver_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--sol",
        solution_path.to_str().unwrap(),
    ]);

    let solution = formats::read_mwsol(&solution_path).unwrap();
    assert!(!solution.strategies.is_empty());
    let grid = f64::from(i16::MAX);
    for block in &solution.strategies {
        let sum: f32 = block.probabilities.iter().sum();
        assert!((sum - 1.0).abs() <= 1e-4, "quantized sum drifted: {sum}");
        for &probability in &block.probabilities {
            let units = f64::from(probability) * grid;
            assert!(
                (units - units.round()).abs() < 1e-3,
                "probability {probability} is not on the i16 grid"
            );
        }
    }
}

// --- preflop (Mode B) -------------------------------------------------------

/// 10bb push/fold: SB may only jam or fold, BB may only call or fold (root
/// has exactly 2 actions). `{cache}` is filled in with a tempdir-local path
/// so the exact equity table's compute-then-cache-hit round trip is
/// self-contained and never touches a shared/repo-level cache.
const PREFLOP_PUSHFOLD_TOML_TEMPLATE: &str = r#"
[game]
kind = "preflop"
effective_stack_bb = 10.0
open_sizes_bb = []
raise_factors = []
max_raises = 1
allow_limp = false
equity_cache = "{cache}"

[algorithm]
schedule = "dcfr"

[run]
iterations = 400
check_every = 100
"#;

#[test]
#[ignore = "computes the exact preflop equity table; CI runs it in release with --include-ignored"]
fn preflop_pushfold_solve_smoke() {
    let dir = temp_dir("preflop-pushfold");
    let cache = dir.join("equity.bin");
    // TOML string escaping: a Windows-style path could contain backslashes,
    // but tempdir() on the platforms this runs on never does, so a plain
    // substitution is safe here.
    let config_text = PREFLOP_PUSHFOLD_TOML_TEMPLATE.replace("{cache}", &cache.to_string_lossy());
    let config = dir.join("pushfold.toml");
    std::fs::write(&config, config_text).unwrap();
    let output_path = dir.join("out.json");

    let output = run_solver_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("equity: computing"),
        "first run should compute the table: {stdout}"
    );
    assert!(stdout.contains("root:"), "stdout: {stdout}");
    assert!(stdout.contains("nash_conv"), "stdout: {stdout}");
    assert!(stdout.contains("done:"), "stdout: {stdout}");
    assert!(cache.exists(), "equity cache file must be created");

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output_path).unwrap())
            .expect("--output file must parse as JSON");
    let class_labels = json["class_labels"].as_array().expect("class_labels array");
    assert_eq!(class_labels.len(), 169);

    let entries = json["entries"].as_array().expect("entries array");
    let root = entries
        .iter()
        .find(|e| e["history"] == "")
        .expect("root entry (history == \"\")");
    let actions = root["actions"].as_array().expect("actions array");
    assert_eq!(actions.len(), 2, "push/fold root must have 2 actions");
    let strategy = root["strategy"].as_array().expect("strategy array");
    assert_eq!(strategy.len(), 2, "one strategy row per action");
    for row in strategy {
        assert_eq!(
            row.as_array().unwrap().len(),
            169,
            "each action's strategy row must cover all 169 classes"
        );
    }

    // Second run: the equity table should now load from the cache instead of
    // recomputing, and the run must still succeed.
    let output2 = run_solver_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("equity: loaded cached table"),
        "second run should hit the cache: {stdout2}"
    );
    assert!(stdout2.contains("root:"), "stdout: {stdout2}");
}

// --- [game.postflop] bucketed model: error path only ------------------------
//
// A cold bucketed solve builds a full-street EHS² abstraction (~10 minutes
// in release mode), which is far past what should run automatically here --
// see `examples/preflop_hu_100bb_bucketed.toml`'s own note; the orchestrator
// validates that config end-to-end manually. This test only exercises the
// fast, no-build error path: `model` validation runs before any equity
// table, abstraction, or artifact work starts, so it fails immediately.
#[test]
fn preflop_bucketed_unsupported_model_errors() {
    let dir = temp_dir("preflop-bucketed-bad-model");
    let config_text = r#"
[game]
kind = "preflop"
effective_stack_bb = 10.0
open_sizes_bb = []
raise_factors = []
max_raises = 1
allow_limp = false

[game.postflop]
model = "unsupported-model"

[run]
iterations = 1
"#;
    let config = dir.join("bad_model.toml");
    std::fs::write(&config, config_text).unwrap();

    let output = run_solver(&["solve", config.to_str().unwrap()]);
    assert!(
        !output.status.success(),
        "an unsupported game.postflop.model must fail the solve"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported-model") && stderr.contains("bucketed"),
        "expected an error naming the bad value and the supported one, got: {stderr}"
    );
}
