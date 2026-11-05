//! P2 browse CLI contracts, piped REPL and product-owned range queries.
#![cfg(feature = "p2")]
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
fn browse(file: &Path, node: &str, commands: &str) -> Output {
    browse_with_config(file, node, commands, None)
}
fn browse_with_config(file: &Path, node: &str, commands: &str, config: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_solvers"));
    command.arg("browse");
    if let Some(config) = config {
        command.args(["--config", config.to_str().unwrap()]);
    }
    let mut child = command
        .args([file.to_str().unwrap(), "--color", "never", "--node", node])
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
    child.wait_with_output().unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains('\x1b'));
    text
}
fn profile(file: &Path) {
    let mut root = vec![vec![0.25, 0.75]; 169];
    root[0] = vec![1.0, 0.0]; // AA folds: verify reach is seat-specific.
    std::fs::write(file,serde_json::to_vec(&serde_json::json!({"format":"p2-class-profile","version":1,"nodes":[
        {"path":[],"actor":0,"actions":["fold","raise-to:3000:all-in"],"probabilities":root},
        {"path":["raise-to:3000:all-in"],"actor":1,"actions":["fold","call:2000:all-in"],"probabilities":vec![vec![0.5,0.5];169]}
    ]})).unwrap()).unwrap();
}
#[test]
fn profile_navigation_grids_ranges_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("no-extension");
    profile(&file);
    let text = success(browse(
        &file,
        "root",
        "strat\ngrid 1\nrange\ncombos AKs\ngo nonsense\n1\npath\nrange 0\nrange 1\nup\nroot\nev\nhelp\n\nquit\n",
    ));
    for fragment in [
        "experimental",
        "0: fold",
        "1: raise-to:3000:all-in",
        "AKs (class unit)",
        "unknown action",
        "selector: 1",
        "actor: seat 1",
        "range seat 0: 990.000 known combos",
        "range seat 1: 1326.000 known combos",
        "EV: not available",
    ] {
        assert!(text.contains(fragment), "missing {fragment}: {text}");
    }
    // Five grids, exactly 13 rank-labelled rows each.
    let rows = text
        .lines()
        .filter(|line| {
            line.len() > 80
                && line
                    .chars()
                    .nth(2)
                    .is_some_and(|c| "AKQJT98765432".contains(c))
        })
        .count();
    assert_eq!(rows, 65);
    let solution = mw_preflop::browse::Solution::load(&file).unwrap();
    let node = solution.select("1").unwrap();
    assert_eq!(solution.range(node, 0).unwrap()[0], Some(0.0));
    assert_eq!(solution.range(node, 1).unwrap()[0], Some(6.0));
    let frequencies = solution.frequencies(solution.root).unwrap().0;
    assert!((frequencies[1] - 990.0 / 1326.0).abs() < 1e-10);
    for selector in ["1", "raise-to:3000:all-in"] {
        let text = success(browse(&file, selector, "path\ngo 1\npath\nup\nexit\n"));
        assert!(text.contains("selector: 1/1"));
        assert!(text.contains("actor: seat 1"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["browse", file.to_str().unwrap(), "--color", "always"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = output;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"strat\nquit\n")
        .unwrap();
    assert!(
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).contains("\x1b[48;2;")
    );
}
#[test]
fn unsupported_inputs_and_invalid_profiles_are_input_errors() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("solution.sol");
    for bytes in [
        b"SLVRSOLVgarbage".as_slice(),
        b"garbage",
        b"{\"format\":\"p2-class-profile\",\"version\":2,\"nodes\":[]}",
    ] {
        std::fs::write(&file, bytes).unwrap();
        let output = browse(&file, "root", "quit\n");
        assert_eq!(output.status.code(), Some(2));
    }
    std::fs::write(&file, b"SLVRMWSL").unwrap();
    assert_eq!(browse(&file, "root", "quit\n").status.code(), Some(3));
    profile(&file);
    assert_eq!(browse(&file, "bad", "quit\n").status.code(), Some(2));
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    document["nodes"][0]["probabilities"][0] = serde_json::json!([0.0, 0.0]);
    std::fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
    assert_eq!(browse(&file, "root", "quit\n").status.code(), Some(2));
}
#[test]
fn mwsol_saved_by_small_real_solve_browses_without_cache() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("p2.toml");
    let run = dir.path().join("run");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(cache.join("ehs2")).unwrap();
    let empty = serde_json::json!({"thresholds":[],"boards":{}});
    let table:mw_preflop::card_abstraction::Ehs2Abstraction=serde_json::from_value(serde_json::json!({
        "params":{"flop_buckets":1,"turn_buckets":1,"river_buckets":1},"flop":empty,"turn":empty,"river":empty})).unwrap();
    table
        .save(&cache.join("ehs2").join(format!(
            "v{}-f1-t1-r1.postcard",
            mw_preflop::card_abstraction::CACHE_FORMAT_VERSION
        )))
        .unwrap();
    std::fs::write(
        &config,
        r#"schema = 'solvers.nlh/v1'
[table]
players = 3
stack = 3
[ranges]
BTN = 'AA,KK,QQ'
SB = 'AA,KK,QQ'
BB = 'AA,KK,QQ'
[tree]
script = 'preflop { replace raise [2x, 2.5x] } flop, turn, river { checkdown } preflop when aggressions >= 1 { remove raise }'
[solver.abstraction.buckets]
flop = 1
turn = 1
river = 1
[solver.stop]
max_sweeps = 8
check_every_sweeps = 9
evaluation_samples = 2
deviator_traversals = 1
[run]
threads = 1
memory = '64MiB'
[output]
probability_encoding = 'f32'
"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args([
            "--cache-dir",
            cache.to_str().unwrap(),
            "solve",
            config.to_str().unwrap(),
            "--out",
            run.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let file = run.join("solution.mwsol");
    let rejected = browse_with_config(&file, "root", "quit\n", Some(&config));
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--config"));
    let disguised = dir.path().join("saved.json");
    std::fs::copy(&file, &disguised).unwrap();
    assert!(success(browse(&disguised, "root", "quit\n")).contains("P2 .mwsol"));
    let text = success(browse(
        &file,
        "root",
        "show\nstrat\ngrid 0\nrange\ncombos AA\ngo invalid\n0\npath\nup\nroot\nev\nquit\n",
    ));
    for fragment in [
        "P2 .mwsol",
        "seat 0 (BTN)",
        "pot: 1.500 BB",
        "0: fold",
        "selector: 0",
        "unknown action",
        "saved root summary",
    ] {
        assert!(text.contains(fragment), "{fragment}: {text}");
    }
    let solution = mw_preflop::browse::Solution::load(&file).unwrap();
    let root = solution.root;
    assert_eq!(
        solution
            .range(root, 0)
            .unwrap()
            .iter()
            .flatten()
            .sum::<f64>(),
        18.0
    );
    for selector in ["0", "fold", "00000000000000000000000000000000"] {
        success(browse(&file, selector, "path\nquit\n"));
    }
    // A missing saved class must remain unknown, including its path reach.
    let mut reader = mw_preflop::mwsol::MwSolReader::open(&file).unwrap();
    let mut metadata = reader.metadata().clone();
    let missing_aa = |key: mw_preflop::mwsol::MultiwayStrategyKey| {
        key.history == [0; 16] && key.actor == 0 && key.street == 0 && key.bucket_path[0] == 0
    };
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while cursor < reader.strategy_count() {
        let page = reader
            .read_strategy_page(cursor, mw_preflop::mwsol::MWSOL_MAX_PAGE_LIMIT)
            .unwrap();
        cursor += page.strategies.len();
        blocks.extend(page.strategies.into_iter().filter(|b| !missing_aa(b.key)));
    }
    metadata.strategy_weights.retain(|w| !missing_aa(w.key));
    let mut value = serde_json::to_value(metadata).unwrap();
    value["strategies"] = serde_json::to_value(blocks).unwrap();
    let incomplete: mw_preflop::mwsol::MultiwaySolution = serde_json::from_value(value).unwrap();
    let missing_file = dir.path().join("missing.mwsol");
    mw_preflop::mwsol::write_mwsol_with(
        &missing_file,
        &incomplete,
        mw_preflop::mwsol::MwsolStorage::F32,
    )
    .unwrap();
    let missing = mw_preflop::browse::Solution::load(&missing_file).unwrap();
    assert!(missing.nodes[missing.root].probabilities[0].is_none());
    let child = missing.select("0").unwrap();
    assert_eq!(missing.range(child, 0).unwrap()[0], None);
    assert_eq!(missing.range(child, 0).unwrap()[1], Some(0.0));
    assert!(
        success(browse(&missing_file, "root", "combos AA\nquit\n"))
            .contains("not available (unvisited)")
    );
    // Optional retained scratch artifact for a separate manual smoke session.
    if let Some(destination) = std::env::var_os("SOLVERS_BROWSE_TEST_ARTIFACT") {
        std::fs::copy(file, destination).unwrap();
    }
}

#[test]
fn own_action_reach_multiplies_across_multiple_decisions() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("profile.json");
    profile(&file);
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    document["nodes"][0]["actions"][1] = serde_json::json!("raise-to:3000");
    document["nodes"][1]["path"][0] = serde_json::json!("raise-to:3000");
    document["nodes"][1]["actions"][1] = serde_json::json!("raise-to:6000");
    document["nodes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "path": ["raise-to:3000", "raise-to:6000"],
            "actor": 0, "actions": ["fold", "call:3000"], "probabilities": vec![vec![0.8,0.2];169]
        }));
    std::fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
    let solution = mw_preflop::browse::Solution::load(&file).unwrap();
    let node = solution.select("1/1/1").unwrap();
    let range = solution.range(node, 0).unwrap();
    assert_eq!(range[0], Some(0.0));
    assert!((range[1].unwrap() - 4.0 * 0.75 * 0.2).abs() < 1e-12);
    assert!((range.iter().flatten().sum::<f64>() - 198.0).abs() < 1e-10);
    assert!(
        (solution
            .range(node, 1)
            .unwrap()
            .iter()
            .flatten()
            .sum::<f64>()
            - 663.0)
            .abs()
            < 1e-10
    );
}

#[test]
fn browse_help_and_auto_color_on_pipe() {
    let help = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["browse", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    for token in ["--config", "--node", "--color", "auto", "always", "never"] {
        assert!(text.contains(token));
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("profile");
    profile(&file);
    let mut child = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["browse", file.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"strat\nquit\n")
        .unwrap();
    success(child.wait_with_output().unwrap());
}

#[test]
fn configured_class_profile_context_and_tree_validation() {
    use mw_preflop::trunk::l0::{self, FlopLeaves, Profile, Tree};
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("pushfold.toml");
    let raw = include_str!("../../../examples/bench/hu_pushfold_10bb.toml")
        .replace("BTN = \"random\"", "BTN = \"AA:0.5,AKs\"")
        .replace("BB = \"random\"", "BB = \"KK\"");
    std::fs::write(&config, &raw).unwrap();
    let game = l0::game_from_config(&raw, &config).unwrap();
    let tree = Tree::build_with(&game, FlopLeaves::L1).unwrap();
    let mut document = Profile::uniform(&tree).export(&tree);
    let file = dir.path().join("profile.json");
    std::fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
    let solution = mw_preflop::browse::Solution::load_with_config(&file, Some(&config)).unwrap();
    assert_eq!(solution.positions, ["BTN", "BB"]);
    assert_eq!(solution.nodes[0].pot_bb, Some(1.5));
    assert_eq!(solution.nodes[0].stacks_bb, Some(vec![9.5, 9.0]));
    assert_eq!(
        solution.range(0, 0).unwrap().iter().flatten().sum::<f64>(),
        7.0
    );
    let child = solution.select("1").unwrap();
    assert_eq!(solution.nodes[child].pot_bb, Some(11.0));
    assert_eq!(solution.nodes[child].stacks_bb, Some(vec![0.0, 9.0]));
    assert_eq!(
        solution
            .range(child, 0)
            .unwrap()
            .iter()
            .flatten()
            .sum::<f64>(),
        3.5
    );
    let text = success(browse_with_config(
        &file,
        "root",
        "strat\ngo 1\nrange BTN\nrange BB\nquit\n",
        Some(&config),
    ));
    for fragment in [
        "config starting ranges",
        "path: root; actor: seat 0 (BTN)",
        "path: BTN raise-to:10000:all-in",
        "actor: seat 1 (BB)",
        "range seat 0 (BTN): 3.500",
        "range seat 1 (BB): 6.000",
    ] {
        assert!(text.contains(fragment), "{fragment}: {text}");
    }
    let strat = text
        .split("path: root; actor:")
        .nth(1)
        .unwrap()
        .split("p2-class-profile")
        .next()
        .unwrap();
    assert!(!strat.contains("pot:"));
    assert!(!strat.contains("assumed full"));
    // Missing decision, wrong actor, action and path each fail as input errors.
    let original = document.clone();
    document.nodes.pop();
    std::fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
    assert_eq!(
        browse_with_config(&file, "root", "", Some(&config))
            .status
            .code(),
        Some(2)
    );
    for kind in 0..3 {
        let mut bad = original.clone();
        match kind {
            0 => bad.nodes[0].actor = 1,
            1 => bad.nodes[0].actions[0] = "check".into(),
            _ => bad.nodes[1].path[0] = "unknown".into(),
        }
        std::fs::write(&file, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert_eq!(
            browse_with_config(&file, "root", "", Some(&config))
                .status
                .code(),
            Some(2)
        );
    }
    // Legal action reordering remains accepted and is displayed in tree order.
    let mut reordered = original;
    reordered.nodes[0].actions.reverse();
    for row in &mut reordered.nodes[0].probabilities {
        row.reverse();
    }
    std::fs::write(&file, serde_json::to_vec(&reordered).unwrap()).unwrap();
    success(browse_with_config(&file, "root", "quit\n", Some(&config)));
}
