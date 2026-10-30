//! Pre-solve tree selectors and wire rows share the artifact tree contract.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn example(product: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(product)
        .join(name)
}

fn cli(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("tree")
        .arg(config)
        .args(args)
        .args(["--format", "json"])
        .output()
        .unwrap()
}

fn rows(config: &Path, args: &[&str]) -> serde_json::Value {
    let output = cli(config, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn tree_help_describes_product_boundary() {
    let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["tree", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("heads-up postflop"), "{help}");
    assert!(help.contains("preflop configs are rejected"), "{help}");
}

#[test]
fn flop_root_and_returned_child_selectors() {
    let config = example("hu-postflop", "flop_srp.toml");
    let root = rows(&config, &[]);
    assert_eq!(root.as_array().unwrap().len(), 1);
    assert_eq!(root[0]["history"], "");
    assert_eq!(root[0]["kind"], "action");
    assert_eq!(root[0]["street"], "flop");
    assert_eq!(root[0]["actor"], "oop");
    assert_eq!(root[0]["pot"], 5.5);
    assert_eq!(
        root[0]["actions"],
        serde_json::json!(["check", "bet 1.815"])
    );
    assert_eq!(root[0].as_object().unwrap().len(), 8);
    assert!(root[0].get("stored").is_none());
    let child = root[0]["children"][0].as_str().unwrap();
    let selected = rows(&config, &["--node", child, "--node", "root"]);
    assert_eq!(selected.as_array().unwrap().len(), 2);
    assert_eq!(selected[0]["history"], child);
    assert_eq!(selected[0]["kind"], root[0]["child_kinds"][0]);
    assert_eq!(selected[0]["actor"], "ip");
    assert_eq!(selected[1], root[0]);
    assert_eq!(rows(&config, &["--node", "check"])[0], selected[0]);
}

#[test]
fn unknown_node_and_p2_are_explicit_input_errors() {
    let unknown = cli(
        &example("hu-postflop", "river_small.toml"),
        &["--node", "not-a-node"],
    );
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("not-a-node"));
    let p2 = cli(&example("mw-preflop", "3max_smoke.toml"), &[]);
    assert_eq!(p2.status.code(), Some(2));
    let message = String::from_utf8_lossy(&p2.stderr);
    assert!(message.contains("NLH005"), "{message}");
    #[cfg(feature = "p2")]
    assert!(message.contains("P1"), "{message}");
    #[cfg(not(feature = "p2"))]
    assert!(
        message.contains(
            "this build excludes P2 (Multiway Preflop); use a build with the `p2` feature"
        ),
        "{message}"
    );
}

#[test]
fn terminal_rows_all_action_nodes_and_human_listing() {
    let config = example("hu-postflop", "river_small.toml");
    let terminal = rows(&config, &["--node", "bet 1/fold"]);
    assert_eq!(terminal[0]["kind"], "terminal");
    assert_eq!(terminal[0]["street"], "river");
    assert!(terminal[0]["actor"].is_null());
    assert!(terminal[0]["pot"].is_null());
    for field in ["actions", "children", "child_kinds"] {
        assert_eq!(terminal[0][field], serde_json::json!([]));
    }
    let history = terminal[0]["history"].as_str().unwrap();
    assert_eq!(rows(&config, &["--node", history]), terminal);
    let all = rows(&config, &["--node", "all"]);
    let all = all.as_array().unwrap();
    assert!(all.len() > 1);
    assert!(all.iter().all(|row| row["kind"] == "action"));
    let human = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("tree")
        .arg(config)
        .output()
        .unwrap();
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("action river actor=oop pot=5 BB"), "{text}");
    assert!(text.contains("bet 1 -> r1 (action)"), "{text}");
}

#[test]
fn nondecimal_tree_pot_is_exact_chips_in_human_output() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("river.toml");
    let source = std::fs::read_to_string(example("hu-postflop", "river_small.toml")).unwrap();
    let source = source
        .replace("stack = 5", "stack = 5\nbb_chips = 3")
        .replace("BTN r2.5, BB c", "BTN r8c, BB c");
    std::fs::write(&config, source).unwrap();
    let human = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("tree")
        .arg(&config)
        .output()
        .unwrap();
    assert!(
        human.status.success(),
        "{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.contains("action river actor=oop pot=16c"), "{text}");
    let json = rows(&config, &[]);
    assert_eq!(json[0]["pot"], serde_json::json!(16.0 / 3.0));
    assert!(json[0].get("pot_text").is_none());
}

fn histories(rows: &serde_json::Value) -> Vec<&str> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| row["history"].as_str().unwrap())
        .collect()
}

#[test]
fn rest_of_street_follows_each_node_with_its_streets_action_nodes_in_preorder() {
    let river = example("hu-postflop", "river_small.toml");
    let street = rows(&river, &["--rest-of-street"]);
    assert_eq!(
        histories(&street),
        ["", "x", "xr1", "xr1r2.5", "r1", "r1r2.5"]
    );
    // Each selection expands on its own, in request order; terminals stay single rows.
    let selected = rows(
        &river,
        &["--node", "r1", "--node", "bet 1/fold", "--rest-of-street"],
    );
    assert_eq!(histories(&selected), ["r1", "r1r2.5", "r1f"]);

    // On the flop it stops at the turn deal: only flop action nodes, each action child included.
    let flop = example("hu-postflop", "flop_srp.toml");
    let street = rows(&flop, &["--rest-of-street"]);
    let street = street.as_array().unwrap();
    let returned: Vec<&str> = street
        .iter()
        .map(|row| row["history"].as_str().unwrap())
        .collect();
    for row in street {
        assert_eq!(
            (row["kind"].as_str(), row["street"].as_str()),
            (Some("action"), Some("flop"))
        );
        for (child, kind) in row["children"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["child_kinds"].as_array().unwrap())
        {
            assert_eq!(
                kind == "action",
                returned.contains(&child.as_str().unwrap()),
                "{child}"
            );
        }
    }
    let deal = rows(&flop, &["--node", "xx", "--rest-of-street"]);
    assert_eq!(histories(&deal), ["xx"]);
    assert_eq!(deal[0]["kind"], "chance");
}
