//! Line-builder CLI contract: table-only inspection without solver validation.
use std::path::Path;
use std::process::{Command, Output};

const TABLE: &str = r#"schema = "solvers.nlh/v1"
[table]
players = 6
stack_bb = 100
"#;

fn cli(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("line")
        .arg(config)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn json(config: &Path, args: &[&str]) -> serde_json::Value {
    let mut arguments = args.to_vec();
    arguments.extend(["--format", "json"]);
    let output = cli(config, &arguments);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("line --format json emits the complete cursor")
}

fn option<'a>(cursor: &'a serde_json::Value, kind: &str) -> &'a serde_json::Value {
    cursor["next"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["kind"] == kind)
        .unwrap_or_else(|| panic!("missing {kind} option: {cursor}"))
}

#[test]
fn root_reports_cursor_and_legal_options_without_moves() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("root.toml");
    std::fs::write(&config, TABLE).unwrap();
    let cursor = json(&config, &[]);
    assert_eq!(cursor["moves"], serde_json::json!([]));
    assert_eq!(cursor["line"], "");
    assert_eq!(cursor["actions"], serde_json::json!([]));
    assert_eq!(cursor["street"], "preflop");
    assert_eq!(cursor["pot"], 1.5);
    assert_eq!(cursor["players"], 6);
    assert_eq!(cursor["button"], 0);
    let seats = cursor["seats"].as_array().unwrap();
    assert_eq!(seats.len(), 6);
    for (index, seat) in seats.iter().enumerate() {
        assert_eq!(seat["seat"], index);
        assert_eq!(seat["status"], "active");
    }
    assert_eq!(seats[0]["position"], "BTN");
    assert_eq!(seats[1]["wager"], 0.5);
    assert_eq!(seats[1]["stack"], 99.5);
    assert_eq!(seats[2]["wager"], 1.0);
    assert_eq!(seats[2]["stack"], 99.0);
    assert_eq!(
        seats
            .iter()
            .map(|seat| seat["committed"].as_f64().unwrap())
            .sum::<f64>(),
        cursor["pot"].as_f64().unwrap()
    );
    assert_eq!(cursor["terminal"], false);
    assert_eq!(cursor["complete"], false);
    assert!(cursor.as_object().unwrap().contains_key("boardCards"));
    assert!(cursor["boardCards"].is_null());
    assert!(cursor["next"]["seat"].is_number());
    assert_eq!(cursor["next"]["position"], "UTG");
    assert_eq!(cursor["next"]["street"], "preflop");
    assert_eq!(cursor["next"]["toCall"], 1.0);
    assert_eq!(cursor["next"]["stack"], 100.0);
    assert_eq!(cursor["next"]["wager"], 0.0);
    assert_eq!(option(&cursor, "fold")["token"], "f");
    assert_eq!(option(&cursor, "call")["token"], "c");
    assert_eq!(option(&cursor, "call")["amount"], 1.0);
    assert_eq!(option(&cursor, "raise")["token"], "r");
    assert_eq!(option(&cursor, "raise")["min"], 2.0);
    assert_eq!(option(&cursor, "allIn")["token"], "a");
}

#[test]
fn explicit_moves_build_partial_cursor_then_close_preflop() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("moves.toml");
    std::fs::write(&config, TABLE).unwrap();
    let partial = json(&config, &["--moves", "f,f,f,r2.5"]);
    assert_eq!(partial["moves"], serde_json::json!(["f", "f", "f", "r2.5"]));
    assert_eq!(partial["complete"], false);
    assert_eq!(partial["street"], "preflop");
    assert_eq!(partial["next"]["position"], "SB");
    assert_eq!(partial["next"]["toCall"], 2.0);
    assert_eq!(partial["next"]["wager"], 0.5);
    assert_eq!(partial["actions"].as_array().unwrap().len(), 4);
    assert_eq!(partial["actions"][3]["action"]["raise-to"]["to"], 2.5);

    let closed = json(&config, &["--moves", "f,f,f,r2.5,f,c"]);
    assert_eq!(closed["line"], "BTN r2.5, BB c");
    assert_eq!(closed["complete"], true);
    assert_eq!(closed["street"], "flop");
    assert_eq!(closed["pot"], 5.5);
    assert_eq!(closed["players"], 2);
    assert_eq!(closed["boardCards"], 3);
    assert_eq!(closed["terminal"], false);
    assert_eq!(closed["next"]["position"], "BB");
    assert_eq!(closed["next"]["toCall"], 0.0);
    assert_eq!(closed["next"]["stack"], 97.5);
    assert_eq!(option(&closed, "check")["token"], "x");
    assert_eq!(option(&closed, "bet")["token"], "b");
    assert_eq!(option(&closed, "bet")["min"], 1.0);
    assert_eq!(closed["actions"][5]["action"]["call"]["amount"], 1.5);
    assert_eq!(closed["button"], 0);
    let seats = closed["seats"].as_array().unwrap();
    assert!(seats.iter().all(|seat| seat["wager"] == 0.0));
    assert_eq!(seats[0]["stack"], 97.5);
    assert_eq!(seats[2]["stack"], 97.5);
    for (index, seat) in seats.iter().enumerate() {
        assert_eq!(
            seat["status"],
            if index == 0 || index == 2 {
                "active"
            } else {
                "folded"
            }
        );
    }
    assert_eq!(
        seats
            .iter()
            .map(|seat| seat["committed"].as_f64().unwrap())
            .sum::<f64>(),
        closed["pot"].as_f64().unwrap()
    );
}

#[test]
fn omitted_moves_strictly_replay_config_line_including_implicit_folds() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("line.toml");
    std::fs::write(
        &config,
        format!("{TABLE}\n[spot]\nline = \"BTN r2.5, BB c\"\n"),
    )
    .unwrap();
    let replayed = json(&config, &[]);
    let explicit = json(&config, &["--moves", "f,f,f,r2.5,f,c"]);
    for field in [
        "moves",
        "line",
        "street",
        "pot",
        "complete",
        "boardCards",
        "players",
        "next",
        "terminal",
    ] {
        assert_eq!(replayed[field], explicit[field], "field {field}");
    }
    let actions = replayed["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 6);
    assert!(actions[0]["implicit"].as_bool().unwrap());
    assert!(!actions[3]["implicit"].as_bool().unwrap());

    std::fs::write(&config, format!("{TABLE}\n[spot]\nline = \"BTN c\"\n")).unwrap();
    let output = cli(&config, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("NLH004"));
}

#[test]
fn board_ranges_and_solver_errors_are_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("ignored.toml");
    for ignored in [
        r#"
[spot]
line = "BTN r2.5, BB c"
board = "not a board"
[ranges]
BTN = "not a range"
BB = "also invalid"
[tree]
script = "not a script"
[solver]
unknown = true
"#,
        r#"
[spot]
line = "BTN r2.5, BB c"
board = 42
ranges = false
[ranges]
BTN = 42
BB = false
[economics]
kind = "unsupported"
[run]
memory = false
"#,
    ] {
        std::fs::write(&config, format!("{TABLE}{ignored}")).unwrap();
        let replayed = json(&config, &[]);
        assert_eq!(replayed["line"], "BTN r2.5, BB c");
        assert_eq!(replayed["boardCards"], 3);
        let root = json(&config, &["--moves", ""]);
        assert_eq!(root["moves"], serde_json::json!([]));
        assert_eq!(root["street"], "preflop");
        assert_eq!(root["next"]["position"], "UTG");
    }
    std::fs::write(&config, format!("{TABLE}\n[spot]\nline = 42\n")).unwrap();
    let root = json(&config, &["--moves", ""]);
    assert_eq!(root["line"], "");
    assert_eq!(root["next"]["position"], "UTG");
}

#[test]
fn illegal_move_reports_one_based_index_and_input_exit_code() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("illegal.toml");
    std::fs::write(&config, TABLE).unwrap();
    let output = cli(&config, &["--moves", "c,x", "--format", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("NLH004"), "{error}");
    assert!(error.contains("move 2:"), "{error}");
}

#[test]
fn terminal_cursor_has_no_next_actor() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("terminal.toml");
    std::fs::write(&config, TABLE).unwrap();
    let cursor = json(&config, &["--moves", "f,f,f,f,f"]);
    assert_eq!(cursor["terminal"], true);
    assert_eq!(cursor["complete"], false);
    assert_eq!(cursor["players"], 1);
    assert!(cursor["next"].is_null());
    assert!(cursor["boardCards"].is_null());
}

#[test]
fn human_output_explains_cursor_and_options() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("human.toml");
    std::fs::write(&config, TABLE).unwrap();
    let output = cli(&config, &["--moves", "f,f,f,r2.5,f,c", "--format", "human"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let human = String::from_utf8(output.stdout).unwrap();
    for text in [
        "line: BTN r2.5, BB c",
        "street: Flop",
        "pot: 5.5 BB",
        "players: 2",
        "complete: true",
        "terminal: false",
        "board cards: 3",
        "next: BB",
        "to call: 0 BB",
        "stack: 97.5 BB",
        "options:",
        "Check (x)",
        "Bet (b)",
        "min=1 BB",
    ] {
        assert!(human.contains(text), "missing {text:?}: {human}");
    }
}
