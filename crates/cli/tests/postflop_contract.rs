use std::path::Path;
use std::process::{Command, Output};

const CONFIG: &str = r#"
schema = "solvers.postflop/v1"
[game]
board = "2c 7d 9h Js Qs"
oop_range = "AsAh"
ip_range = "KsKh"
pot = 20
effective_stack = 80
[game.tree]
kind = "script"
script = '''river when unopened { force bet [50] }'''
[run]
iterations = 20
check_every = 5
threads = 1
"#;

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .expect("execute solvers")
}

fn ok(args: &[&str]) -> Output {
    let output = invoke(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn solve(config: &str, directory: &Path) -> std::path::PathBuf {
    let input = directory.join("config.toml");
    std::fs::write(&input, config).unwrap();
    let run = directory.join("run");
    ok(&[
        "solve",
        input.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    run
}

fn export(run: &Path, view: &str, node: &str) -> serde_json::Value {
    let output = ok(&[
        "export",
        run.join("solution.sol").to_str().unwrap(),
        view,
        "--node",
        node,
    ]);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn saved_ev_keeps_original_baseline_and_utility_units() {
    for utility in [
        "",
        "\n[utility]\nkind = \"icm\"\npayouts = [100.0, 100.0]\n",
        "\n[utility]\nkind = \"tournament-icm\"\npayouts = [100.0, 100.0, 100.0]\noutside_field = [200.0]\n",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let run = solve(&format!("{CONFIG}{utility}"), directory.path());
        let summary = export(&run, "summary", "root");
        let root = export(&run, "ev", "root");
        for row in root.as_array().unwrap() {
            let key = format!("ev_{}", row["seat"].as_str().unwrap());
            assert!((row["ev"].as_f64().unwrap() - summary[&key].as_f64().unwrap()).abs() < 0.002);
        }
        let values = export(&run, "ev", "r10");
        if !utility.is_empty() {
            assert!(
                values
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row["ev"].as_f64().unwrap().abs() < 0.002)
            );
        } else {
            let actions = export(&run, "actions", "r10");
            let call = actions
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["action"] == "call")
                .unwrap()["frequency"]
                .as_f64()
                .unwrap();
            let oop = values
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["seat"] == "oop")
                .unwrap()["ev"]
                .as_f64()
                .unwrap();
            // AA wins: fold returns the initial 20, call adds the opponent's
            // 10. OOP's own earlier bet remains a cost, not another +10.
            assert!(
                (oop - (20.0 + 10.0 * call)).abs() < 0.002,
                "{oop}, call={call}"
            );
        }
    }
}

#[test]
fn validate_and_solve_reject_the_same_invalid_inputs() {
    let directory = tempfile::tempdir().unwrap();
    for invalid in [
        CONFIG.replace("check_every = 5", "check_every = 0"),
        CONFIG.replace("iterations = 20", "iterations = 0"),
        CONFIG.replace("threads = 1", "threads = 0"),
        CONFIG.replace("[run]", "[run]\nmax_time = \"0s\""),
        CONFIG.replace("[run]", "[run]\nmax_time = \"oops\""),
        CONFIG.replace("[run]", "[run]\ntarget_nash_conv = nan"),
        CONFIG.replace("AsAh", ""),
        CONFIG.replace("AsAh", "2cAc"),
        CONFIG.replace("AsAh", "KsKh"),
    ] {
        let input = directory.path().join("invalid.toml");
        std::fs::write(&input, invalid).unwrap();
        let output = invoke(&["validate", input.to_str().unwrap(), "--show-effective"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("SLV004"));
        let run = tempfile::tempdir().unwrap();
        let output = invoke(&[
            "solve",
            input.to_str().unwrap(),
            "--out",
            run.path().to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("SLV004"));
    }
}

#[test]
fn resume_republishes_solution_and_summary_in_place_and_on_fork() {
    let directory = tempfile::tempdir().unwrap();
    let run = solve(
        &CONFIG.replace("[run]", "[run]\ntarget_nash_conv = 999.0"),
        directory.path(),
    );
    assert_eq!(
        formats::read_sol(&run.join("solution.sol"))
            .unwrap()
            .meta
            .iterations,
        5
    );
    let sol = run.join("solution.sol");
    let mut stale = formats::read_sol(&sol).unwrap();
    stale.meta.iterations = 1;
    formats::write_sol(&sol, &stale).unwrap();
    std::fs::write(run.join("run.json"), "{}").unwrap();
    let fork = directory.path().join("fork");
    ok(&[
        "resume",
        run.to_str().unwrap(),
        "--out",
        fork.to_str().unwrap(),
    ]);
    assert_eq!(
        formats::read_sol(&sol).unwrap().meta.iterations,
        1,
        "fork must preserve source"
    );
    ok(&["resume", run.to_str().unwrap()]);
    for path in [&run, &fork] {
        let artifact = formats::read_sol(&path.join("solution.sol")).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path.join("run.json")).unwrap()).unwrap();
        assert_eq!(artifact.meta.iterations, 10);
        assert_eq!(result["iterations"], 10);
        assert!((result["nashConv"].as_f64().unwrap() - artifact.meta.nash_conv).abs() < 1e-10);
        assert!(!path.join("strategy.json").exists());
    }
}

#[test]
fn convergence_claim_depends_on_economics() {
    let directory = tempfile::tempdir().unwrap();
    for (economics, general_sum) in [
        ("", false),
        (
            "\n[rake]\nkind = \"percent-cap\"\nrate = 0.05\ncap = 5.0\n",
            true,
        ),
        (
            "\n[utility]\nkind = \"tournament-icm\"\npayouts = [100.0, 60.0]\noutside_field = [200.0]\n",
            true,
        ),
    ] {
        let input = directory.path().join("config.toml");
        std::fs::write(&input, format!("{CONFIG}{economics}")).unwrap();
        let output = ok(&["validate", input.to_str().unwrap(), "--format", "json"]);
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            report["profile"].as_str().unwrap().contains("no Nash"),
            general_sum
        );
    }
}

#[test]
fn report_honors_time_budget() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("config.toml");
    std::fs::write(
        &input,
        CONFIG.replace(
            "iterations = 20",
            "iterations = 1000000\nmax_time = \"1s\"\nstorage = \"i16\"",
        ),
    )
    .unwrap();
    let output = ok(&["report", input.to_str().unwrap(), "--boards", "2c7d9hJsQs"]);
    let csv = String::from_utf8(output.stdout).unwrap();
    let iterations: u64 = csv
        .lines()
        .nth(1)
        .unwrap()
        .split(',')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!(iterations < 1000000);
    assert!(String::from_utf8_lossy(&output.stderr).contains("max_time 1s reached"));
}

#[test]
fn resume_preserves_storage_streets_and_cumulative_time() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("config.toml");
    let config = CONFIG.replace("2c 7d 9h Js Qs", "2c 7d 9h Js").replace(
        "[run]",
        "[run]\nstorage = \"i16\"\nmax_time = \"1s\"\ntarget_nash_conv = 999.0",
    );
    std::fs::write(&input, config).unwrap();
    let run = directory.path().join("run");
    ok(&[
        "solve",
        input.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
        "--sol-streets",
        "no-rivers",
    ]);
    let before = formats::read_sol(&run.join("solution.sol")).unwrap();
    assert_eq!(before.mode, formats::StreetsStored::NoRivers);
    // Simulate a durable progress mark at the cumulative limit, without
    // relying on machine-dependent solve speed or sleeping in this test.
    let progress = run.join("progress.jsonl");
    let mut rows: Vec<serde_json::Value> = std::fs::read_to_string(&progress)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows.last_mut().unwrap()["elapsed_secs"] = 1.0.into();
    let text = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    std::fs::write(&progress, text).unwrap();
    let fork = directory.path().join("fork");
    let output = ok(&[
        "resume",
        run.to_str().unwrap(),
        "--out",
        fork.to_str().unwrap(),
    ]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("max_time already reached"));
    let after = formats::read_sol(&fork.join("solution.sol")).unwrap();
    assert_eq!(after.mode, before.mode);
    assert_eq!(after.meta.storage, "i16");
    assert_eq!(after.meta.iterations, before.meta.iterations);
    assert_eq!(
        std::fs::read(fork.join("progress.jsonl")).unwrap(),
        std::fs::read(progress).unwrap()
    );
}

#[test]
fn solution_keeps_existing_format_version() {
    let directory = tempfile::tempdir().unwrap();
    let run = solve(CONFIG, directory.path());
    let path = run.join("solution.sol");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(u16::from_le_bytes(bytes[8..10].try_into().unwrap()), 1);
    assert_eq!(formats::read_sol(&path).unwrap().meta.iterations, 20);
}

#[test]
fn inspect_equity_tracks_the_current_board_and_invalidates_cached_values() {
    use std::io::Write;
    use std::process::Stdio;
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("config.toml");
    let config = CONFIG
        .replace("2c 7d 9h Js Qs", "2c 7d 9h Js")
        .replace(
            "kind = \"script\"\nscript = '''river when unopened { force bet [50] }'''",
            "kind = \"none\"",
        )
        .replace("[run]", "[run]\nstorage = \"i16\"");
    std::fs::write(&input, config).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["inspect", input.to_str().unwrap()])
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"eq\ngo check\ngo check\ngo Kc\neq\nroot\neq\nquit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("error:"), "{stdout}");
    let grids: Vec<_> = stdout
        .split("oop equity vs ip at current node (class-averaged, %)")
        .skip(1)
        .map(|part| part.lines().skip(1).take(14).collect::<Vec<_>>())
        .collect();
    assert_eq!(grids.len(), 3, "{stdout}");
    assert_ne!(
        grids[0], grids[1],
        "dealing a king must change AA's equity against KK"
    );
    assert_eq!(
        grids[0], grids[2],
        "returning to root must restore turn equity"
    );
}
