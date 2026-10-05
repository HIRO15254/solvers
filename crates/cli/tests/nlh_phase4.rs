//! Phase 4 query acceptance, with exact BB amounts and self-contained artifacts.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const RIVER: &str = r#"schema = "solvers.nlh/v1"
[table]
players = 6
stack_bb = 12.75
[spot]
line = "BTN r2.5, BB c / BB x, BTN x / BB x, BTN x"
board = "Ks 7h 2d 3c 9s"
[ranges]
BTN = "AhAd,QhQd"
BB = "JhJd,ThTd"
[tree]
script = '''river { replace bet [3.3bb] replace raise [a] }'''
[tree.max_aggressive_actions]
river = 2
[solver.stop]
max_iterations = 8
check_every = 4
[run]
threads = 1
memory = "1GiB"
"#;

fn cli(args: &[&str], input: &str) -> Output {
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
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn ok(args: &[&str], input: &str) -> String {
    let result = cli(args, input);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn spec_p1_example_human_output_is_readable_and_pinned() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("spec.toml");
    let spec = include_str!("../../../docs/nlh-input-v1.jp.md");
    let example = spec
        .split("### P1: 6max")
        .nth(1)
        .unwrap()
        .split("```toml\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    std::fs::write(&config, example).unwrap();
    assert_eq!(
        ok(&["validate", path(&config)], ""),
        concat!(
            "valid: schema=solvers.nlh/v1 product=P1 (HU Postflop) amounts=BB utility=BB\n",
            "street: Flop\nboard: Ks 7h 2d\nstarting pot: 5.5 BB\n",
            "OOP: BB remaining stack 97.5 BB\nIP: BTN remaining stack 97.5 BB\n",
            "effective stack: 97.5 BB\nfolded: SB, UTG, HJ, CO\n",
            "line: UTG f*, HJ f*, CO f*, BTN r2.5, SB f*, BB c\n* marks an implied fold\n",
            "economics: cash with rake rate=0.05 cap=4 BB when=flop_dealt\n",
            "tree rule: flop { replace bet [33, 75] }\n",
            "tree rule: turn { replace bet [33, 75] }\n",
            "tree rule: river { replace bet [33, 75] }\n",
            "tree rule: flop { replace raise [3x] }\n",
            "tree rule: turn { replace raise [3x] }\n",
            "tree rule: river { replace raise [3x] }\n",
        )
    );
}

#[test]
fn exports_inspection_report_and_comparison_use_bb() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("river.toml");
    std::fs::write(&config, RIVER).unwrap();
    let run = temp.path().join("run");
    ok(&["solve", path(&config), "--out", path(&run)], "");
    let sol = run.join("solution.sol");
    let summary: serde_json::Value = serde_json::from_str(&ok(
        &["export", path(&sol), "summary", "--format", "json"],
        "",
    ))
    .unwrap();
    assert_eq!(summary["pot"], 5.5);
    assert_eq!(summary["effective_stack"], 10.25);
    assert_eq!(summary["min_bet"], 1);
    let csv = ok(
        &[
            "export",
            path(&sol),
            "tree",
            "--node",
            "r3.3",
            "--format",
            "csv",
        ],
        "",
    );
    assert!(
        csv.contains("r3.3,river,ip,8.8,true,fold|call|raise to 10.25"),
        "{csv}"
    );
    let by_label = ok(
        &[
            "export",
            path(&sol),
            "tree",
            "--node",
            "bet 3.3",
            "--format",
            "csv",
        ],
        "",
    );
    assert_eq!(csv, by_label);
    for view in ["actions", "strategy", "ev", "range"] {
        for format in ["json", "csv"] {
            let text = ok(
                &[
                    "export",
                    path(&sol),
                    view,
                    "--node",
                    "all",
                    "--format",
                    format,
                ],
                "",
            );
            assert!(!text.is_empty());
            if format == "json" {
                serde_json::from_str::<serde_json::Value>(&text).unwrap();
            }
        }
    }
    let inspection = ok(
        &["inspect", "--sol", path(&sol)],
        "show\ngo bet 3.3\nshow\nev\nquit\n",
    );
    assert!(inspection.contains("history: r3.3"));
    assert!(inspection.contains("raise to 10.25"));
    let live = ok(
        &["inspect", path(&config), "--iterations", "2"],
        "show\ngo bet 3.3\nshow\nev\nquit\n",
    );
    assert!(live.contains("history: r3.3"));
    assert!(live.contains("iterations=2"));
    let compare: serde_json::Value =
        serde_json::from_str(&ok(&["compare", path(&sol), path(&sol)], "")).unwrap();
    assert_eq!(compare["max_strategy_l1"], 0.0);
    assert_eq!(compare["max_ev_delta"], 0.0);
    let report = ok(
        &["report", path(&config), "--boards", "Ks7h2d3c9s,Ks7h2d3c8s"],
        "",
    );
    assert_eq!(report.lines().count(), 3);
    assert!(report.lines().next().unwrap().contains("freq_bet_3.3"));
    for row in report.lines().skip(1) {
        let columns: Vec<_> = row.split(',').collect();
        let ev = columns[4].parse::<f64>().unwrap() + columns[5].parse::<f64>().unwrap();
        assert!((ev - 5.5).abs() < 0.0001);
    }
    let bad = cli(&["report", path(&config), "--boards", "Ks7h2d"], "");
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    // Refuse units mixing even if --cross-game would otherwise permit the spot.
    let old_config = temp.path().join("old.toml");
    std::fs::write(&old_config, "schema='solvers.postflop/v1'\n[game]\nboard='Ks 7h 2d 3c 9s'\noop_range='JhJd,ThTd'\nip_range='AhAd,QhQd'\npot=10\neffective_stack=50\n[game.tree]\nkind='none'\n[run]\niterations=1\nthreads=1\n").unwrap();
    let old_run = temp.path().join("old-run");
    ok(&["solve", path(&old_config), "--out", path(&old_run)], "");
    let mixed = cli(
        &[
            "compare",
            path(&sol),
            path(&old_run.join("solution.sol")),
            "--cross-game",
        ],
        "",
    );
    assert!(!mixed.status.success());
    assert!(
        String::from_utf8_lossy(&mixed.stderr)
            .contains("cannot compare solvers.nlh/v1 and old-family")
    );
}

#[test]
fn no_rivers_artifact_can_resolve_decimal_history_with_icm_and_asymmetric_stacks() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("turn.toml");
    let raw = RIVER.replace(" / BB x, BTN x / BB x, BTN x", " / BB x, BTN x")
        .replace("Ks 7h 2d 3c 9s", "Ks 7h 2d 3c")
        .replace("[spot]", "[table.stacks_bb]\nBB=20\n[economics]\nkind='tournament'\npayouts=[1000,600,400,0,0,0,0]\noutside_field_bb=[30]\n[spot]")
        .replace("script = '''river", "script = '''turn { replace bet [3.3bb] } river")
        .replace("[solver.stop]", "[solver]\nstorage='i16'\n[solver.algorithm]\nschedule='vanilla'\n[output]\nsolution_streets='no-rivers'\n[solver.stop]");
    std::fs::write(&config, raw).unwrap();
    let run = temp.path().join("run");
    ok(&["solve", path(&config), "--out", path(&run)], "");
    let inspection = ok(
        &[
            "inspect",
            "--sol",
            path(&run.join("solution.sol")),
            "--river-iterations",
            "4",
        ],
        "go bet 3.3\ngo call\ngo 9s\nshow\nquit\n",
    );
    assert!(
        inspection.contains("re-solving river subgame at \"r3.3c[9s]\""),
        "{inspection}"
    );
    assert!(!inspection.contains("error:"), "{inspection}");
}
