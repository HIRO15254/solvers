//! M8 CLI contract, including a real tiny P2 solve and a derived P1 solve.
use std::path::Path;
use std::process::{Command, Output};

const LINE: &str = "BTN c, BB x"; // SB implicitly folds.
const BOARD: &str = "Ks 7h 2d";
const P2: &str = r#"schema = 'solvers.nlh/v1'
[table]
players = 3
stack_bb = 3
[ranges]
BTN = 'AA,KK,QQ'
SB = 'AA,KK,QQ'
BB = 'AA,KK,QQ'
[tree]
script = 'preflop { replace raise [2x, 2.5x] } flop, turn, river { checkdown }'
[tree.max_aggressive_actions]
preflop = 1
[solver.abstraction.buckets]
flop = 1
turn = 1
river = 1
[solver.stop]
max_sweeps = 64
check_every_sweeps = 65
evaluation_samples = 2
deviator_traversals = 1
[run]
threads = 1
memory = '64MiB'
[output]
probability_encoding = 'f32'
"#;
const BASE: &str = r#"schema = 'solvers.nlh/v1'
[meta]
name = 'derived test'
description = 'P1 base'
[tree]
script = 'flop when unopened { force bet [a] } turn, river { checkdown }'
[solver.stop]
max_iterations = 2
[run]
threads = 1
memory = '64MiB'
"#;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Output {
    let result = cli(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}
fn text(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn saved_preflop_profile_derives_reproducible_solvable_p1_and_rejects_invalid_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let p2 = dir.path().join("p2.toml");
    let run = dir.path().join("p2-run");
    let base = dir.path().join("base.toml");
    let out = dir.path().join("derived.toml");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(cache.join("ehs2")).unwrap();
    // Every postflop action is forced by unconditional checkdown, so no
    // postflop policy/abstraction lookup occurs. Empty street tables make
    // this real CLI solve independent of a minutes-long global cache build;
    // any unexpected lookup panics rather than silently approximating.
    let empty = serde_json::json!({"thresholds": [], "boards": {}});
    let table: mw_preflop::card_abstraction::Ehs2Abstraction =
        serde_json::from_value(serde_json::json!({
            "params": {"flop_buckets": 1, "turn_buckets": 1, "river_buckets": 1},
            "flop": empty, "turn": empty, "river": empty,
        }))
        .unwrap();
    table
        .save(&cache.join("ehs2").join(format!(
            "v{}-f1-t1-r1.postcard",
            mw_preflop::card_abstraction::CACHE_FORMAT_VERSION
        )))
        .unwrap();
    std::fs::write(&p2, P2).unwrap();
    std::fs::write(&base, BASE).unwrap();
    ok(&[
        "--cache-dir",
        text(&cache),
        "solve",
        text(&p2),
        "--out",
        text(&run),
    ]);
    let args = [
        "derive",
        text(&run),
        "--line",
        LINE,
        "--board",
        BOARD,
        "--base",
        text(&base),
        "--out",
        text(&out),
    ];
    let first = ok(&args);
    assert!(String::from_utf8_lossy(&first.stderr).contains("unvisited classes"));
    let bytes = std::fs::read(&out).unwrap();
    ok(&args);
    assert_eq!(bytes, std::fs::read(&out).unwrap());
    let stdout = ok(&args[..args.len() - 2]);
    assert_eq!(bytes, stdout.stdout);
    let effective = dir.path().join("effective.toml");
    ok(&[
        "validate",
        text(&out),
        "--write-effective",
        text(&effective),
    ]);
    assert_eq!(bytes, std::fs::read(&effective).unwrap());
    let doc = spot::Document::parse(std::str::from_utf8(&bytes).unwrap(), &out).unwrap();
    assert_eq!(doc.spot.product, spot::Product::HuPostflop);
    assert_eq!(doc.spot.meta.name.as_deref(), Some("derived test"));
    let provenance = doc.spot.meta.derived_from.as_ref().unwrap();
    let manifest = runfiles::RunManifest::read(&run).unwrap();
    assert_eq!(provenance.run_id.as_deref(), Some(manifest.run_id.as_str()));
    assert_eq!(provenance.line.as_deref(), Some(LINE));
    assert_eq!(provenance.board.as_deref(), Some(BOARD));
    let solution = run.join("solution.mwsol");
    assert_eq!(
        provenance.solution_hash.as_deref(),
        Some(
            blake3::hash(&std::fs::read(&solution).unwrap())
                .to_hex()
                .as_str()
        )
    );
    // Independently walk the stored edges and multiply actual saved class
    // probabilities, including BB's check after BTN call and SB fold.
    let mut reader = mw_preflop::mwsol::MwSolReader::open(&solution).unwrap();
    let metadata = reader.metadata().clone();
    let blocks = reader.read_strategy_page(0, 4096).unwrap().strategies;
    for seat in [0, 2] {
        for class in [0, 14, 28] {
            // AA, KK, QQ
            let mut probability = 1.0_f64;
            let mut history = [0; 16];
            for (actor, label) in [(0, "call:1000"), (1, "fold"), (2, "check")] {
                let edge = metadata
                    .histories
                    .iter()
                    .find(|e| e.parent == history && e.actor == actor && e.action == label)
                    .unwrap();
                if actor as usize == seat {
                    let block = blocks
                        .iter()
                        .find(|b| {
                            b.key.history == history
                                && b.key.actor == actor
                                && b.key.bucket_path[0] == class
                        })
                        .unwrap();
                    probability *= f64::from(block.probabilities[edge.action_index as usize]);
                }
                history = edge.key;
            }
            assert!(probability > 0.0);
            let label = ["AA", "KK", "QQ"][class as usize / 14];
            let starting: nlh::Range = label.parse().unwrap();
            let derived: nlh::Range = doc.spot.ranges[nlh::SeatId(seat as u8)]
                .text
                .parse()
                .unwrap();
            for combo in 0..nlh::NUM_COMBOS {
                if starting.weight(combo) > 0.0 {
                    assert_eq!(derived.weight(combo), probability as f32);
                }
            }
        }
    }
    ok(&[
        "solve",
        text(&out),
        "--out",
        text(&dir.path().join("p1-run")),
    ]);
    let inherited = ok(&["derive", text(&run), "--line", LINE, "--board", BOARD]);
    assert!(String::from_utf8_lossy(&inherited.stderr).contains("inherited postflop checkdown"));
    std::fs::write(&base, format!("{BASE}\n[spot]\nline = 'invalid replaced line'\n[ranges]\nBTN = 'invalid replaced range'\n")).unwrap();
    let replaced = ok(&args);
    let warnings = String::from_utf8_lossy(&replaced.stderr);
    assert!(
        warnings.contains("base [spot] replaced") && warnings.contains("base [ranges] replaced")
    );
    let mut manifest = manifest;
    manifest.state = runfiles::RunState::Canceled;
    manifest.write_atomic(&run).unwrap();
    assert!(String::from_utf8_lossy(&ok(&args).stderr).contains("state is canceled"));
    std::fs::write(&base, BASE).unwrap();
    let failed = dir.path().join("must-not-exist.toml");
    for (line, board, extra, message) in [
        (
            "BTN r2.2, BB c",
            BOARD,
            "",
            "BTN: requested raise-to:2200; available actions:",
        ),
        ("BTN c, SB c, BB x", BOARD, "", "3+ players remain"),
        ("BTN c, BB x / BB x, BTN x", BOARD, "", "preflop-only"),
        (LINE, "Ks Ks 2d", "", "distinct cards"),
        (
            LINE,
            BOARD,
            "[table]\nplayers = 3\nstack_bb = 4\n",
            "base [table] does not match",
        ),
        (
            LINE,
            BOARD,
            "[economics.rake]\nrate = 0.05\n",
            "base [economics] does not match",
        ),
        ("", BOARD, "", "closed preflop"),
        ("BTN c", BOARD, "", "not facing a bet"),
        (LINE, "Ks 7h 2d 3c", "", "three-card flop"),
        (
            LINE,
            BOARD,
            "[solver]\nkind = 'range-vector'\n",
            "belongs to the other product",
        ),
    ] {
        std::fs::write(&base, format!("{BASE}\n{extra}")).unwrap();
        let result = cli(&[
            "derive",
            text(&run),
            "--line",
            line,
            "--board",
            board,
            "--base",
            text(&base),
            "--out",
            text(&failed),
        ]);
        assert_eq!(
            result.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!failed.exists());
    }
    let missing = dir.path().join("missing");
    std::fs::create_dir(&missing).unwrap();
    let result = cli(&[
        "derive",
        text(&missing),
        "--line",
        LINE,
        "--board",
        BOARD,
        "--out",
        text(&failed),
    ]);
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stderr).contains("solution.mwsol"));
    assert!(!failed.exists());
    std::fs::write(&solution, "corrupt solution").unwrap();
    let result = cli(&[
        "derive",
        text(&run),
        "--line",
        LINE,
        "--board",
        BOARD,
        "--out",
        text(&failed),
    ]);
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid P2 solution"));
    assert!(!failed.exists());
}
