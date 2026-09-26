//! Public combo labels and per-seat value boundaries must survive compact
//! postflop storage, including asymmetric ranges and uninformative deal masks.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn config(oop: &str, ip: &str, storage: &str) -> String {
    format!(
        r#"schema = "solvers.postflop/v1"
[game]
board = "2c 7d 9h Js Qs"
oop_range = "{oop}"
ip_range = "{ip}"
pot = 20
effective_stack = 80
iso_merging = false
[game.tree]
kind = "script"
script = '''river when unopened {{ force bet [50] }}'''
[run]
iterations = 8
check_every = 8
threads = 1
storage = "{storage}"
"#
    )
}

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn solve(directory: &Path, config: &str) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(directory).unwrap();
    let input = directory.join("config.toml");
    std::fs::write(&input, config).unwrap();
    let run = directory.join("run");
    success(invoke(&[
        "solve",
        input.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
        "--sol-streets",
        "full",
    ]));
    (input, run.join("solution.sol"))
}

fn export(sol: &Path, view: &str, node: &str) -> serde_json::Value {
    serde_json::from_str(&success(invoke(&[
        "export",
        sol.to_str().unwrap(),
        view,
        "--node",
        node,
    ])))
    .unwrap()
}

fn labels(rows: &serde_json::Value, seat: &str, seat_field: &str) -> BTreeSet<String> {
    rows.as_array()
        .unwrap()
        .iter()
        .filter(|row| row[seat_field] == seat)
        .map(|row| row["combo"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn artifact_rows_keep_global_labels_and_unequal_seat_value_lengths() {
    for storage in ["f32", "i16"] {
        for (oop, ip) in [("AsAh,AdAc", "KsKh"), ("KsKh", "AsAh,AdAc")] {
            let directory = tempfile::tempdir().unwrap();
            let (_, sol) = solve(directory.path(), &config(oop, ip, storage));
            let expected_oop = oop.split(',').map(str::to_owned).collect();
            let expected_ip = ip.split(',').map(str::to_owned).collect();
            let ranges = export(&sol, "range", "root");
            assert_eq!(labels(&ranges, "oop", "seat"), expected_oop);
            assert_eq!(labels(&ranges, "ip", "seat"), expected_ip);
            let summary = export(&sol, "summary", "root");
            // A forced root bet has just one continuation. Every combo in
            // either seat has the same showdown result in this fixture, so
            // each row equals that seat's aggregate EV at both action nodes.
            for (node, actor, expected) in
                [("root", "oop", &expected_oop), ("r10", "ip", &expected_ip)]
            {
                let strategies = export(&sol, "strategy", node);
                assert_eq!(&labels(&strategies, actor, "actor"), expected);
                let values = export(&sol, "ev", node);
                assert_eq!(values.as_array().unwrap().len(), 3);
                assert_eq!(labels(&values, "oop", "seat"), expected_oop);
                assert_eq!(labels(&values, "ip", "seat"), expected_ip);
                for row in values.as_array().unwrap() {
                    let key = format!("ev_{}", row["seat"].as_str().unwrap());
                    assert!(
                        (row["ev"].as_f64().unwrap() - summary[&key].as_f64().unwrap()).abs()
                            < 0.002,
                        "{storage}, node={node}, row={row}, summary={summary}"
                    );
                }
            }
        }
    }
}

fn inspect(args: &[&str], commands: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
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
        .write_all(commands.as_bytes())
        .unwrap();
    let stdout = success(child.wait_with_output().unwrap());
    assert!(!stdout.contains("error:"), "{stdout}");
    stdout
}

#[test]
fn inspect_live_and_saved_expand_only_at_display_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let (input, sol) = solve(directory.path(), &config("AsAh,AdAc", "KsKh", "i16"));
    let commands = "range oop\nrange ip\ngrid 0\ncombos AA\neq\ngo 0\ngrid call\ncombos KK\nquit\n";
    let live = inspect(&["inspect", input.to_str().unwrap()], commands);
    let stored = inspect(&["inspect", "--sol", sol.to_str().unwrap()], commands);
    for stdout in [&live, &stored] {
        for label in ["AsAh bet 10=1.000", "AdAc bet 10=1.000", "KsKh fold="] {
            assert!(stdout.contains(label), "{stdout}");
        }
        assert!(
            !stdout.contains("AsAc bet"),
            "unsupported combos must be absent"
        );
        // AA is the first displayed class and wins this completed board.
        // Expansion must place its equity at AA, not at local combo zero.
        let equity = stdout
            .split_once("oop equity vs ip at current node (class-averaged, %)")
            .unwrap()
            .1;
        let aa = equity
            .lines()
            .find_map(|line| {
                let mut cells = line.split_whitespace();
                if cells.next()? != "A" {
                    return None;
                }
                cells.next()?.parse::<u16>().ok()
            })
            .unwrap();
        assert_eq!(aa, 100);
    }
}

#[test]
fn compact_chance_labels_cover_action_and_all_in_terminal_children() {
    for all_in in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut raw = config("AsAh,AdAc", "KsKh", "f32").replace("2c 7d 9h Js Qs", "2c 7d 9h Js");
        if all_in {
            raw = raw
                .replace("effective_stack = 80", "effective_stack = 5")
                .replace("river when unopened", "turn when unopened");
        } else {
            raw = raw.replace(
                "kind = \"script\"\nscript = '''river when unopened { force bet [50] }'''",
                "kind = \"none\"",
            );
        }
        let (input, sol) = solve(directory.path(), &raw);
        let prefix = if all_in {
            "go 0\ngo call"
        } else {
            "go check\ngo check"
        };
        // Qc removes no supported combo, as do many other cards. Its mask
        // cannot identify it. All-in branches additionally lack action tags.
        let commands = format!("{prefix}\nshow\ngo Qc\nshow\neq\nquit\n");
        for args in [
            vec!["inspect", input.to_str().unwrap()],
            vec!["inspect", "--sol", sol.to_str().unwrap()],
        ] {
            let stdout = inspect(&args, &commands);
            assert!(stdout.contains("] Qc\n"), "{stdout}");
            assert!(stdout.contains("[Qc]"), "{stdout}");
            assert!(
                stdout.contains(if all_in {
                    "kind: terminal"
                } else {
                    "kind: action (oop to act)"
                }),
                "{stdout}"
            );
        }
        if !all_in {
            assert_eq!(
                export(&sol, "strategy", "check/check/Qc"),
                export(&sol, "strategy", "xx[Qc]")
            );
        }
    }
}

#[test]
fn compare_rejects_equal_length_different_combo_support_even_cross_game() {
    let directory = tempfile::tempdir().unwrap();
    let (_, left) = solve(
        &directory.path().join("left"),
        &config("AsAh,AdAc", "KsKh", "f32"),
    );
    let identical: serde_json::Value = serde_json::from_str(&success(invoke(&[
        "compare",
        left.to_str().unwrap(),
        left.to_str().unwrap(),
    ])))
    .unwrap();
    assert_eq!(identical["max_strategy_l1"], 0.0);
    assert_eq!(identical["max_ev_delta"], 0.0);
    for (seat, oop, ip) in [("oop", "AsAd,AhAc", "KsKh"), ("ip", "AsAh,AdAc", "KdKc")] {
        let (_, right) = solve(&directory.path().join(seat), &config(oop, ip, "f32"));
        for cross_game in [false, true] {
            let mut args = vec!["compare", left.to_str().unwrap(), right.to_str().unwrap()];
            if cross_game {
                args.push("--cross-game");
            }
            let output = invoke(&args);
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains(&format!("different {seat} hand support")),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
