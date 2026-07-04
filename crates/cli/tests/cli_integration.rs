use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

fn workspace_root() -> std::path::PathBuf {
    // crates/cli/tests -> up to workspace root
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn inspect_smoke() {
    let config = workspace_root().join("examples/river_small.toml");
    let mut child = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("inspect")
        .arg(&config)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn solvers inspect");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"show\ngrid 1\neq\ncombos AA\nev\nquit\n")
        .unwrap();
    let output = child.wait_with_output().expect("wait for solvers inspect");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // `ev`'s output line must contain the literal substring "nash_conv".
    assert!(stdout.contains("nash_conv"));
    // `show` must print the root action node with its two labeled actions.
    assert!(stdout.contains("kind: action (oop to act)"));
    assert!(stdout.contains("check:"));
    // `combos AA` must print every AA combo with per-action probabilities.
    assert!(stdout.contains("combos AA:"));
    assert!(stdout.contains("AsAh check="));
    // `ev` prints all five expected fields on one line.
    assert!(stdout.contains("ev_oop="));
    assert!(stdout.contains("ev_ip="));
    assert!(stdout.contains("expl_oop="));
    assert!(stdout.contains("expl_ip="));
}

#[test]
fn report_smoke() {
    let config = workspace_root().join("examples/river_small.toml");
    let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("report")
        .arg(&config)
        .arg("--boards")
        .arg("2c 7d 9h Js Qs,2c 7d 9h Js Ks")
        .output()
        .expect("run solvers report");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let header = lines.next().unwrap();
    assert!(header.starts_with("board,"));
    let header_cols: Vec<&str> = header.split(',').collect();
    let ev_oop_idx = header_cols.iter().position(|&c| c == "ev_oop").unwrap();
    let ev_ip_idx = header_cols.iter().position(|&c| c == "ev_ip").unwrap();
    let mut row_count = 0;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        row_count += 1;
        let cols: Vec<&str> = line.split(',').collect();
        let ev_oop: f64 = cols[ev_oop_idx].parse().unwrap();
        let ev_ip: f64 = cols[ev_ip_idx].parse().unwrap();
        assert!(
            (ev_oop + ev_ip).abs() < 1e-2,
            "ev_oop+ev_ip should be ~0, got {ev_oop} + {ev_ip}"
        );
    }
    assert_eq!(row_count, 2);
}

// --- M3 research-workflow: checkpoint / metrics / resume / bench ---------

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, uniquely-named scratch directory under the OS temp dir, so
/// parallel `cargo test` runs never collide.
fn temp_dir(tag: &str) -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "solvers-cli-test-{}-{}-{}",
        std::process::id(),
        id,
        tag
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_solvers(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .expect("run solvers")
}

fn run_solvers_ok(args: &[&str]) -> std::process::Output {
    let output = run_solvers(args);
    assert!(
        output.status.success(),
        "solvers {:?} failed, stderr: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn done_line_field(stdout: &str, key: &str) -> String {
    let done_line = stdout
        .lines()
        .find(|l| l.starts_with("done:"))
        .unwrap_or_else(|| panic!("no 'done:' summary line in stdout: {stdout:?}"));
    done_line
        .split(key)
        .nth(1)
        .unwrap_or_else(|| panic!("no {key:?} field in done line: {done_line:?}"))
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// A tiny, fast, deterministic Kuhn config with no `target_nash_conv`: the
/// resume-equivalence test relies on the run never stopping early, so both
/// the straight run and the checkpointed-then-resumed run execute the exact
/// same sequence of steps (early stopping on `target_nash_conv` would make
/// the two runs' step counts diverge, since checking it is unconditional
/// wall-clock/loop-position-dependent, not just a function of iteration).
const KUHN_NO_EARLY_STOP: &str = r#"
[game]
kind = "kuhn"

[algorithm]
schedule = "dcfr"

[run]
iterations = 400
check_every = 50
"#;

#[test]
fn solve_checkpoint_and_metrics_smoke() {
    let dir = temp_dir("ckpt-metrics");
    let config = workspace_root().join("examples/kuhn.toml");
    let checkpoint = dir.join("run.ckpt");
    let metrics = dir.join("run.jsonl");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--checkpoint",
        checkpoint.to_str().unwrap(),
        "--metrics",
        metrics.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let reported_iters: u64 = done_line_field(&stdout, "iterations=").parse().unwrap();

    assert!(checkpoint.exists());
    let header = formats::peek_header(&checkpoint).expect("peek checkpoint header");
    assert_eq!(header.iteration, reported_iters);

    let content = std::fs::read_to_string(&metrics).unwrap();
    let mut last_iter = 0u64;
    let mut row_count = 0;
    for line in content.lines() {
        let row: formats::MetricsRow = serde_json::from_str(line).unwrap();
        assert!(
            row.iteration > last_iter,
            "rows must be strictly increasing in iteration"
        );
        last_iter = row.iteration;
        row_count += 1;
    }
    assert!(row_count > 0);
    assert_eq!(last_iter, reported_iters);
}

#[test]
fn resume_equivalence_kuhn() {
    let dir = temp_dir("resume-equiv");
    let config = dir.join("kuhn.toml");
    std::fs::write(&config, KUHN_NO_EARLY_STOP).unwrap();
    let checkpoint = dir.join("run.ckpt");
    let out_a = dir.join("a.json");
    let out_b = dir.join("b.json");

    // Run A: straight solve to the full 400 iterations.
    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--output",
        out_a.to_str().unwrap(),
    ]);

    // Run B: partial solve to 200 (an `--iterations` override, which does
    // NOT change the config-file hash the checkpoint is stamped with --
    // see `solve::run`), then resume using the *same* config file to 400
    // total.
    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--iterations",
        "200",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let header = formats::peek_header(&checkpoint).unwrap();
    assert_eq!(header.iteration, 200);

    run_solvers_ok(&[
        "resume",
        config.to_str().unwrap(),
        "--checkpoint",
        checkpoint.to_str().unwrap(),
        "--output",
        out_b.to_str().unwrap(),
    ]);

    let a = std::fs::read_to_string(&out_a).unwrap();
    let b = std::fs::read_to_string(&out_b).unwrap();
    assert_eq!(
        a, b,
        "a checkpointed-then-resumed solve must byte-for-byte match a straight solve"
    );
}

#[test]
fn resume_tampered_config_errors() {
    let dir = temp_dir("resume-tamper");
    let config = dir.join("kuhn.toml");
    std::fs::write(&config, KUHN_NO_EARLY_STOP).unwrap();
    let checkpoint = dir.join("run.ckpt");

    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);

    // Tamper with the config after the checkpoint was produced.
    let mut tampered = KUHN_NO_EARLY_STOP.to_string();
    tampered.push_str("\n# tampered\n");
    std::fs::write(&config, tampered).unwrap();

    let output = run_solvers(&[
        "resume",
        config.to_str().unwrap(),
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("hash"),
        "expected a hash-mismatch message, got: {stderr}"
    );
}

#[test]
fn bench_kuhn_two_schedules() {
    let dir = temp_dir("bench");
    let config = dir.join("kuhn.toml");
    std::fs::write(&config, KUHN_NO_EARLY_STOP).unwrap();
    let metrics_dir = dir.join("metrics");

    let output = run_solvers_ok(&[
        "bench",
        config.to_str().unwrap(),
        "--schedules",
        "dcfr,cfr-plus",
        "--metrics-dir",
        metrics_dir.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("schedule"));
    assert!(stdout.contains("nash_conv"));
    assert!(stdout.contains("dcfr"));
    assert!(stdout.contains("cfr-plus"));

    assert!(metrics_dir.join("dcfr.jsonl").exists());
    assert!(metrics_dir.join("cfr-plus.jsonl").exists());
    let dcfr_metrics = std::fs::read_to_string(metrics_dir.join("dcfr.jsonl")).unwrap();
    assert!(dcfr_metrics.lines().count() > 0);
}

#[test]
fn i16_storage_solve_converges_and_checkpoint_round_trips() {
    let dir = temp_dir("i16");
    let config = workspace_root().join("examples/kuhn_i16.toml");
    let checkpoint = dir.join("run.ckpt");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--checkpoint",
        checkpoint.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let nash_conv: f64 = done_line_field(&stdout, "nash_conv=").parse().unwrap();
    // Loose bound: kuhn under i16-quantized storage should still converge
    // well below the game's own scale (P0's EV is -1/18 ~= -0.0556 chips).
    assert!(
        nash_conv < 0.02,
        "i16 storage should still converge on kuhn, got nash_conv={nash_conv}"
    );

    let checkpoint_data = formats::read_checkpoint(&checkpoint).unwrap();
    assert!(matches!(
        checkpoint_data.state.storage,
        engine::StorageState::I16 { .. }
    ));
}
