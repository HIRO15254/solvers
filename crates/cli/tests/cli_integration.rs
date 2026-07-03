use std::io::Write;
use std::process::{Command, Stdio};

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
