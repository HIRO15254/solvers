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

/// Fully decodes an `.mwsol` file through the paged `MwSolReader` API
/// (there is no longer a single-shot `read_mwsol` convenience wrapper).
fn read_mwsol_full(path: &std::path::Path) -> mw_preflop::mwsol::MultiwaySolution {
    let mut reader = mw_preflop::mwsol::MwSolReader::open(path).expect("open mwsol");
    let total = reader.strategy_count();
    let mut cursor = 0;
    let mut strategies = Vec::with_capacity(total);
    while cursor < total {
        let page = reader
            .read_strategy_page(cursor, mw_preflop::mwsol::MWSOL_MAX_PAGE_LIMIT)
            .expect("read strategy page");
        strategies.extend(page.strategies);
        cursor = page.next_cursor.unwrap_or(total);
    }
    let metadata = reader.metadata().clone();
    mw_preflop::mwsol::MultiwaySolution {
        schema_version: metadata.schema_version,
        config_toml: metadata.config_toml,
        config_fingerprint: metadata.config_fingerprint,
        game_fingerprint: metadata.game_fingerprint,
        algorithm_fingerprint: metadata.algorithm_fingerprint,
        abstraction_fingerprint: metadata.abstraction_fingerprint,
        configuration_fingerprint: metadata.configuration_fingerprint,
        stop_status: metadata.stop_status,
        chip_unit_bb: metadata.chip_unit_bb,
        sweeps: metadata.sweeps,
        approximate_profile: metadata.approximate_profile,
        seats: metadata.seats,
        histories: metadata.histories,
        public_states: metadata.public_states,
        strategy_weights: metadata.strategy_weights,
        strategies,
    }
}

#[test]
fn inspect_smoke() {
    let config = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
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
    let config = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
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
        // Postflop EV is measured from the start of the subgame, so the two
        // sides split the starting pot rather than summing to zero.
        // `crates/cli/tests/fixtures/river_small.toml` has `pot = 10` and no rake.
        assert!(
            (ev_oop + ev_ip - 10.0).abs() < 1e-2,
            "ev_oop+ev_ip should be the starting pot 10, got {ev_oop} + {ev_ip}"
        );
    }
    assert_eq!(row_count, 2);
}

/// A river-only config whose tree script gives paired and unpaired boards
/// genuinely different root menus (`when paired` / `when !paired`, each
/// replacing `bet` with a different size): the paired board's root actions
/// are `check` / `bet 10`, the unpaired board's are `check` / `bet 5`.
const REPORT_UNION_TOML: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 55
ante_bb = 4
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"
[ranges]
BB = "22+,A2s+,KTo+"
BTN = "55-22,QJs,A5s-A2s,KQo,T9s"
[tree]
include_allin = false
script = '''river {
  when paired {
    replace bet [100]
  }
  when !paired {
    replace bet [50]
  }
}
'''
[tree.max_aggressive_actions]
flop = 2
turn = 2
river = 2
[solver]
storage = "f32"
[solver.stop]
max_iterations = 200
check_every = 50
[run]
threads = 1
"#;

/// `report`'s CSV header is the union of every board's root action labels,
/// not one menu shared by all boards: a board predicate in the tree script
/// (`when paired`) legitimately gives two boards different root menus, and
/// sweeping boards is `report`'s whole job. A board missing a label the
/// union has (because its own root menu never had that action) gets an
/// empty cell there, not `0` -- "never had this action" and "had it and
/// never took it" are different facts a reader averaging the column must be
/// able to tell apart.
#[test]
fn report_csv_header_is_the_union_of_boards_root_actions() {
    let dir = temp_dir("report-union");
    let config = dir.join("report_union.toml");
    std::fs::write(&config, REPORT_UNION_TOML).unwrap();

    let output = run_solvers_ok(&[
        "report",
        config.to_str().unwrap(),
        "--boards",
        "2c 7d 9h Js Qs,2c 7d 9h Js Jd",
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let header = lines.next().unwrap();
    let header_cols: Vec<&str> = header.split(',').collect();
    assert!(
        header_cols.contains(&"freq_check"),
        "header must have freq_check: {header}"
    );
    assert!(
        header_cols.contains(&"freq_bet_5"),
        "header must have the unpaired board's bet: {header}"
    );
    assert!(
        header_cols.contains(&"freq_bet_10"),
        "header must have the paired board's bet: {header}"
    );

    let board_idx = header_cols.iter().position(|&c| c == "board").unwrap();
    let bet5_idx = header_cols.iter().position(|&c| c == "freq_bet_5").unwrap();
    let bet10_idx = header_cols
        .iter()
        .position(|&c| c == "freq_bet_10")
        .unwrap();

    let rows: Vec<Vec<&str>> = lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split(',').collect())
        .collect();
    assert_eq!(rows.len(), 2);

    let unpaired_row = rows
        .iter()
        .find(|row| row[board_idx] == "2c 7d 9h Js Qs")
        .expect("unpaired board row present");
    let paired_row = rows
        .iter()
        .find(|row| row[board_idx] == "2c 7d 9h Js Jd")
        .expect("paired board row present");

    // The unpaired board's root menu never had `bet 10`: empty, not `0`.
    assert_eq!(unpaired_row[bet10_idx], "");
    assert!(!unpaired_row[bet5_idx].is_empty());

    // Symmetric: the paired board's root menu never had `bet 5`.
    assert_eq!(paired_row[bet5_idx], "");
    assert!(!paired_row[bet10_idx].is_empty());
}

// --- dead tree-script rule warning ---------------------------------------

/// `crates/cli/tests/fixtures/river_small.toml` with a third river rule nested under a
/// contradiction (`aggressions == 0 && aggressions == 1`) -- the exact
/// "never fires" shape `docs/nlh-input-v1.jp.md` warns a nested
/// `when` can produce (see `crates/cli/tests/fixtures/trees/pio.tree`'s comment on it).
const RIVER_WITH_DEAD_RULE_TOML: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 55
ante_bb = 4
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"
[ranges]
BB = "22+,A2s+,KTo+"
BTN = "55-22,QJs,A5s-A2s,KQo,T9s"
[tree]
include_allin = false
script = '''river {
  replace bet [50]
  replace raise [50]
  when aggressions == 0 {
    when aggressions == 1 { replace raise [75] }
  }
}
'''
[tree.max_aggressive_actions]
flop = 2
turn = 2
river = 2
[solver]
storage = "f32"
[solver.stop]
max_iterations = 200
check_every = 50
[run]
threads = 1
"#;

/// `solve` must warn on stderr, after the `tree: nodes=...` preflight line,
/// about a rule whose condition never evaluated true anywhere in the build
/// -- and name it precisely enough (street, 1-based position within that
/// street, and the rendered effect/action/sizes/condition) that a reader
/// can find it in their own script. This is a warning, not a failure: the
/// run must still solve and exit successfully.
#[test]
fn solve_warns_about_a_tree_script_rule_that_matches_nothing() {
    let dir = temp_dir("dead-rule-solve");
    let config = dir.join("dead_rule.toml");
    std::fs::write(&config, RIVER_WITH_DEAD_RULE_TOML).unwrap();
    let run = dir.join("run");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stdout.starts_with("tree: nodes="),
        "stdout should still open with the tree preflight line: {stdout}"
    );
    assert!(
        stderr.contains("warning: unmatched River tree rule 3"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("unmatched River tree rule 3"),
        "stderr must name the dead rule's street, position, and rendered body: {stderr}"
    );
}

/// A config with no dead rule (`crates/cli/tests/fixtures/river_small.toml`'s two river
/// rules are both unconditional, so both always match) must print no
/// warning at all.
#[test]
fn solve_prints_no_warning_when_every_rule_matches() {
    let dir = temp_dir("no-dead-rule-solve");
    let config = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
    let run = dir.join("run");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unmatched"),
        "a config with no dead rule must print no dead-rule warning: {stderr}"
    );
}

/// `report` sweeps many boards, and a rule that only matches on some of
/// them (here: `REPORT_UNION_TOML`'s `when paired` / `when !paired`, each
/// matching exactly one of the two boards below) is working as intended,
/// not a bug -- so the per-board coverage must not, by itself, trigger the
/// warning. Complements `solve_warns_about_a_tree_script_rule_that_matches_nothing`,
/// which covers the single-board case.
#[test]
fn report_prints_no_warning_for_a_rule_that_matches_on_some_boards() {
    let dir = temp_dir("report-union-no-warning");
    let config = dir.join("report_union.toml");
    std::fs::write(&config, REPORT_UNION_TOML).unwrap();

    let output = run_solvers_ok(&[
        "report",
        config.to_str().unwrap(),
        "--boards",
        "2c 7d 9h Js Qs,2c 7d 9h Js Jd",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unmatched"),
        "each rule matches on at least one of the two boards, so no warning is expected: {stderr}"
    );
}

/// `report` must still accumulate across the whole sweep and warn once, at
/// the end, about a rule that matches on NO board at all -- adding a third,
/// unreachable river rule (`spr < 0` is never true) to `REPORT_UNION_TOML`'s
/// script. Printed once, not once per board: per-board would be noise for
/// the (legitimate) rules above it.
#[test]
fn report_warns_once_about_a_rule_unmatched_on_every_board() {
    let dir = temp_dir("report-union-warning");
    let config = dir.join("report_union_bad.toml");
    std::fs::write(
        &config,
        REPORT_UNION_TOML.replace(
            "  when !paired {\n    replace bet [50]\n  }\n",
            "  when !paired {\n    replace bet [50]\n  }\n  when spr < 0 { replace raise [200] }\n",
        ),
    )
    .unwrap();

    let output = run_solvers_ok(&[
        "report",
        config.to_str().unwrap(),
        "--boards",
        "2c 7d 9h Js Qs,2c 7d 9h Js Jd",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let occurrences = stderr.matches("unmatched River tree rule").count();
    assert_eq!(
        occurrences, 1,
        "the warning must be printed exactly once, after the whole sweep: {stderr}"
    );
    assert!(
        stderr.contains("unmatched River tree rule 3"),
        "stderr: {stderr}"
    );
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

/// A tiny, fast, deterministic postflop config (`crates/cli/tests/fixtures/river_small.toml`'s
/// river-only subgame) with no `target_nash_conv`: the resume-equivalence
/// test relies on the run never stopping early, so both the straight run and
/// the checkpointed-then-resumed run execute the exact same sequence of
/// steps (early stopping on `target_nash_conv` would make the two runs' step
/// counts diverge, since checking it is unconditional
/// wall-clock/loop-position-dependent, not just a function of iteration).
const RIVER_NO_EARLY_STOP: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 55
ante_bb = 4
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2c 7d 9h Js Qs"
[ranges]
BB = "22+,A2s+,KTo+"
BTN = "55-22,QJs,A5s-A2s,KQo,T9s"
[tree]
include_allin = false
script = '''river {
  replace bet [50]
  replace raise [50]
}
'''
[tree.max_aggressive_actions]
flop = 2
turn = 2
river = 2
[solver]
storage = "f32"
[solver.stop]
max_iterations = 200
check_every = 50
[run]
threads = 1
"#;

#[test]
fn solve_checkpoint_and_metrics_smoke() {
    let dir = temp_dir("ckpt-metrics");
    let config = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
    let run = dir.join("run");
    let checkpoint = run.join("checkpoint.ckpt");
    let metrics = run.join("progress.jsonl");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let reported_iters: u64 = done_line_field(&stdout, "iterations=").parse().unwrap();

    assert!(checkpoint.exists());
    let loaded = hu_postflop::checkpoint::read_checkpoint(&checkpoint).expect("read checkpoint");
    assert_eq!(loaded.iteration, reported_iters);

    let content = std::fs::read_to_string(&metrics).unwrap();
    let mut last_iter = 0u64;
    let mut row_count = 0;
    for line in content.lines() {
        let row: runfiles::MetricsRow = serde_json::from_str(line).unwrap();
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

/// Resuming must reproduce a straight solve exactly.
///
/// The partial run uses its own config file capped at 100 iterations, and
/// the resumed run swaps in the 200-iteration config. The checkpoint is
/// stamped with the config hash, so the two files must otherwise be
/// byte-identical -- which is what makes this a real equivalence check
/// rather than a re-run.
///
/// The two runs' solver states (regrets and strategy sums) are compared
/// exactly, and so is every node's average strategy as read back through
/// `export`. The `.sol` files themselves are not compared byte for byte:
/// they record the run's wall-clock time, which a split run cannot share
/// (the summary is compared without it).
#[test]
fn resume_equivalence_postflop() {
    let dir = temp_dir("resume-equiv");
    let full_config = dir.join("river-200.toml");
    std::fs::write(&full_config, RIVER_NO_EARLY_STOP).unwrap();
    let partial_config = dir.join("river-100.toml");
    std::fs::write(
        &partial_config,
        RIVER_NO_EARLY_STOP.replace("iterations = 200", "iterations = 100"),
    )
    .unwrap();

    // Run A: straight solve to the full 200 iterations.
    let run_a = dir.join("a");
    run_solvers_ok(&[
        "solve",
        full_config.to_str().unwrap(),
        "--out",
        run_a.to_str().unwrap(),
    ]);

    // Run B: solve to 100, then swap in the 200-iteration config and resume.
    let run_b = dir.join("b");
    run_solvers_ok(&[
        "solve",
        partial_config.to_str().unwrap(),
        "--out",
        run_b.to_str().unwrap(),
    ]);
    let checkpoint = run_b.join("checkpoint.ckpt");
    let loaded = hu_postflop::checkpoint::read_checkpoint(&checkpoint).unwrap();
    assert_eq!(loaded.iteration, 100);

    // The run directory carries the config the checkpoint was stamped with,
    // so continuing to 200 means rewriting both together.
    let doc = spot::Document::parse(RIVER_NO_EARLY_STOP, &full_config).unwrap();
    let effective = doc.normalize(&hu_postflop::input::P1Sections).unwrap();
    std::fs::write(run_b.join("run.toml"), &effective).unwrap();
    let restamped = hu_postflop::checkpoint::Checkpoint {
        config_hash: cli::nlh_v1::compatibility_hash(&effective).unwrap(),
        ..loaded
    };
    hu_postflop::checkpoint::write_checkpoint_with_config(
        &checkpoint,
        restamped.config_hash,
        &restamped.state,
        &effective,
        restamped.elapsed_secs.unwrap_or(0.0),
    )
    .unwrap();

    run_solvers_ok(&["resume", run_b.to_str().unwrap()]);

    let straight =
        hu_postflop::checkpoint::read_checkpoint(&run_a.join("checkpoint.ckpt")).unwrap();
    let resumed = hu_postflop::checkpoint::read_checkpoint(&checkpoint).unwrap();
    assert_eq!(straight.iteration, 200);
    assert_eq!(resumed.iteration, 200);
    assert_eq!(
        straight.state, resumed.state,
        "a checkpointed-then-resumed solve must bit-for-bit match a straight solve"
    );

    let export = |run: &std::path::Path, args: &[&str]| {
        let sol = run.join("solution.sol");
        let mut command = vec!["export", sol.to_str().unwrap()];
        command.extend_from_slice(args);
        run_solvers_ok(&command).stdout
    };
    assert_eq!(
        export(&run_a, &["strategy", "--node", "all"]),
        export(&run_b, &["strategy", "--node", "all"]),
        "a checkpointed-then-resumed solve must byte-for-byte match a straight solve"
    );

    // The summary also reports the run's wall-clock time; every other number
    // in it (root EVs, exploitability) must agree.
    let summary = |run: &std::path::Path| {
        let mut summary: serde_json::Value =
            serde_json::from_slice(&export(run, &["summary"])).expect("summary json");
        summary
            .as_object_mut()
            .unwrap()
            .remove("wall_secs")
            .expect("the summary reports its wall time");
        summary
    };
    assert_eq!(
        summary(&run_a),
        summary(&run_b),
        "a resumed solve must publish the same summary as a straight one"
    );
}

#[test]
#[ignore = "builds the full EHS2 tables; explicit release acceptance only"]
fn multiway_v1_u16_storage_writes_a_quantized_mwsol() {
    let dir = temp_dir("multiway-v1-u16");
    let config = workspace_root()
        .join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_production_smoke.toml");
    let run_dir = dir.join("run");

    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run_dir.to_str().unwrap(),
    ]);

    let result: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("run.json")).unwrap()).unwrap();
    assert_eq!(
        result["policyStorage"],
        "preallocated-all-current-street-buckets"
    );
    assert_eq!(result["preallocatedPagesCommitted"], true);
    assert!(result["preallocatedNodes"].as_u64().unwrap() > 0);
    assert!(result["preallocatedColumns"].as_u64().unwrap() > 0);
    assert!(result["preallocatedSlots"].as_u64().unwrap() > 0);
    assert!(result["preallocatedBytes"].as_u64().unwrap() > 0);
    assert_eq!(
        result["policyArenaLimitBytes"].as_u64().unwrap(),
        6 * 1024 * 1024 * 1024
    );

    let solution = read_mwsol_full(&run_dir.join("solution.mwsol"));
    assert!(!solution.strategies.is_empty());
    let grid = f64::from(u16::MAX);
    for block in &solution.strategies {
        let sum: f32 = block.probabilities.iter().sum();
        assert!((sum - 1.0).abs() <= 1e-4, "quantized sum drifted: {sum}");
        for &probability in &block.probabilities {
            let units = f64::from(probability) * grid;
            assert!(
                (units - units.round()).abs() < 1e-3,
                "probability {probability} is not on the u16 grid"
            );
        }
    }
}

#[test]
fn resume_tampered_config_errors() {
    let dir = temp_dir("resume-tamper");
    let config = dir.join("river.toml");
    std::fs::write(&config, RIVER_NO_EARLY_STOP).unwrap();
    let run = dir.join("run");

    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);

    // Tamper with the run directory's copy of the config after the
    // checkpoint was stamped against it.
    let mut tampered = RIVER_NO_EARLY_STOP.to_string();
    tampered = tampered.replace("stack_bb = 55", "stack_bb = 56");
    std::fs::write(run.join("run.toml"), tampered).unwrap();

    let output = run_solvers(&["resume", run.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("hash"),
        "expected a hash-mismatch message, got: {stderr}"
    );
}

// --- `.sol` viewer artifact: `solve --sol` / `inspect --sol` ---------------

/// Tiny turn-start config (single chance node turn->river, tiny ranges, one
/// bet size per street, one raise cap) -- same shape as
/// `crates/hu-postflop/tests/viewer.rs`'s `small_turn_config` and
/// `crates/cli/src/sol.rs`'s own unit-test fixture, duplicated here (rather
/// than shared) since this is a separate test binary with no access to
/// `cli`'s internal `sol` module.
const TINY_TURN_TOML: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 2h"
[ranges]
BB = "44,55"
BTN = "33,66"
[tree]
include_allin = false
script = '''turn { replace bet [75] }
river { replace bet [100] }
'''
[tree.max_aggressive_actions]
turn = 1
river = 1
[solver]
storage = "f32"
[solver.stop]
max_iterations = 32
check_every = 32
[run]
threads = 1
"#;

#[test]
fn sol_export_and_inspect_smoke() {
    let dir = temp_dir("sol-smoke");
    let config = dir.join("turn.toml");
    std::fs::write(&config, TINY_TURN_TOML).unwrap();
    let run = dir.join("run");
    let sol_path = run.join("solution.sol");
    let checkpoint = run.join("checkpoint.ckpt");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(sol_path.exists(), "sol file must be written");
    assert!(checkpoint.exists(), "checkpoint file must be written");

    // The two artifacts answer different questions and are sized
    // accordingly: the checkpoint carries full-precision regrets and
    // strategy sums so a run can continue, while `.sol` carries 16-bit
    // quantized strategies and values for reading. Even at the default
    // `full` mode, which stores every action node, `.sol` stays the smaller
    // of the two.
    let sol_size = std::fs::metadata(&sol_path).unwrap().len();
    let ckpt_size = std::fs::metadata(&checkpoint).unwrap().len();
    assert!(
        sol_size < ckpt_size,
        "sol ({sol_size} bytes) should be smaller than the checkpoint ({ckpt_size} bytes)"
    );

    // And `no-rivers` is the lever for a much smaller artifact: it drops
    // the river nodes, which dominate the count.
    let small_run = dir.join("run-no-rivers");
    let no_rivers = dir.join("no-rivers.toml");
    std::fs::write(
        &no_rivers,
        format!("{TINY_TURN_TOML}\n[output]\nsolution_streets = \"no-rivers\"\n"),
    )
    .unwrap();
    run_solvers_ok(&[
        "solve",
        no_rivers.to_str().unwrap(),
        "--out",
        small_run.to_str().unwrap(),
    ]);
    let small_size = std::fs::metadata(small_run.join("solution.sol"))
        .unwrap()
        .len();
    assert!(
        small_size * 4 < sol_size,
        "no-rivers ({small_size} bytes) should be far smaller than full ({sol_size} bytes)"
    );

    // `solve`'s stdout must mention the exported block count.
    let sol_line = stdout
        .lines()
        .find(|l| l.starts_with("sol:"))
        .unwrap_or_else(|| panic!("no 'sol:' summary line in stdout: {stdout:?}"));
    assert!(
        sol_line.contains("block"),
        "sol summary line should mention the block count: {sol_line:?}"
    );

    // `inspect --sol` loads the artifact back and can serve the root node.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("inspect")
        .arg("--sol")
        .arg(&sol_path)
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn solvers inspect --sol");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"show\nquit\n")
        .unwrap();
    let output = child.wait_with_output().expect("wait for inspect --sol");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("kind: action"), "stdout: {stdout:?}");
}

/// Same shape as `TINY_TURN_TOML` above, but `iso_merging = false` (so the
/// turn->river chance node's child labels are plain, unmerged cards --
/// `inspect.rs`'s `chance_child_label` only appends a representative-card
/// `*` marker for an iso-*merged* `Transition` deal) and ~200 iterations
/// (loose enough for `nash_conv` to be unremarkable -- this test is an
/// end-to-end navigation smoke, not an accuracy check, see `sol.rs`'s
/// `river_resolve_accuracy` unit test for that).
const TINY_TURN_TOML_NO_ISO: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 2h"
[ranges]
BB = "44,55"
BTN = "33,66"
[tree]
include_allin = false
script = '''turn { replace bet [75] }
river { replace bet [100] }
'''
[tree.max_aggressive_actions]
turn = 1
river = 1
[solver]
iso_merging = false
storage = "f32"
[solver.stop]
max_iterations = 200
check_every = 200
[run]
threads = 1
"#;

/// End-to-end smoke test for `inspect --sol`'s river navigation: export a
/// `NoRivers` `.sol`, then drive the REPL down to a river-entry node and
/// back, checking that the lazy re-solve actually fires and serves a
/// strategy (rather than, say, silently falling back to garbage on a
/// missing block).
///
/// The navigation script is `show` / `go check` / `go check` / `go Ah` /
/// `show` / `ev` / `quit`, not the literal `go x` shorthand one might guess
/// from the history-string convention (`x` = check in `PostflopNodeInfo`'s
/// *history*, e.g. `"xx[Ah]"`): `Repl::cmd_go`'s `resolve_action` matches an
/// action node's child by its *display label* ("check", "fold", "call",
/// "bet {to}"/"raise to {to}") or a positional index, never a raw history
/// token, so `go check` is the REPL command that actually takes the check
/// branch (confirmed by running this exact script against a debug build
/// before writing the assertions below). Two checks in a row end the turn
/// (`postflop::Builder::betting`'s `first_checked` bookkeeping) and reach
/// the turn->river chance node; `go Ah` descends into the specific river
/// card "Ah" -- live given this fixture's board "2s 7s Ks 2h" (no ace on
/// board) and ranges 44,55 / 33,66 (no ace in either range to block it
/// further) -- landing on a river-entry `Action` node with no stored block
/// (`NoRivers` mode), so the following `show` triggers `SolProvider`'s lazy
/// re-solve.
#[test]
#[ignore = "solves twice; CI runs it in release with --include-ignored"]
fn inspect_sol_river_navigation_smoke() {
    let dir = temp_dir("sol-river-nav");
    let config = dir.join("turn.toml");
    std::fs::write(
        &config,
        format!("{TINY_TURN_TOML_NO_ISO}\n[output]\nsolution_streets = \"no-rivers\"\n"),
    )
    .unwrap();
    let run = dir.join("run");
    let sol_path = run.join("solution.sol");

    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    assert!(sol_path.exists(), "sol file must be written");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("inspect")
        .arg("--sol")
        .arg(&sol_path)
        .arg("--river-iterations")
        .arg("300")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn solvers inspect --sol");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"show\ngo check\ngo check\ngo Ah\nshow\nev\nquit\n")
        .unwrap();
    let output = child.wait_with_output().expect("wait for inspect --sol");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The river-entry node's `show` must have triggered a lazy re-solve
    // (not silently served nothing) and reported it converging.
    assert!(
        stdout.contains("re-solving river subgame"),
        "stdout: {stdout:?}"
    );
    let (_before_done, after_done) = stdout.split_once("done: iterations=").unwrap_or_else(|| {
        panic!("no 'done: iterations=' re-solve summary line, stdout: {stdout:?}")
    });
    // Immediately after the re-solve's summary line comes this test's
    // second `show`, printing the served (re-solved) strategy's per-action
    // frequencies -- this fixture's river action labels are "check"/"bet 2"
    // (same bet-sizing config as the turn).
    assert!(
        after_done.contains("check:") && after_done.contains("bet 2:"),
        "stdout after the re-solve summary: {after_done:?}"
    );
    // `ev` sources from the `.sol` artifact's cached metadata
    // (`SolProvider::ev_line`), not a live solve.
    assert!(stdout.contains("at export:"), "stdout: {stdout:?}");
}

/// The quantized i16 backend on a postflop subgame (`crates/cli/tests/fixtures/river_small.toml`'s
/// river with `storage = "i16"`): it must still converge, and its
/// `StorageState::I16` checkpoint variant must round-trip.
#[test]
fn i16_storage_solve_converges_and_checkpoint_round_trips() {
    let dir = temp_dir("i16");
    let config = dir.join("river-i16.toml");
    std::fs::write(
        &config,
        RIVER_NO_EARLY_STOP.replace("storage = \"f32\"", "storage = \"i16\""),
    )
    .unwrap();
    let run = dir.join("run");
    let checkpoint = run.join("checkpoint.ckpt");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let nash_conv: f64 = done_line_field(&stdout, "nash_conv=").parse().unwrap();
    // Loose bound: the river under i16-quantized storage should still
    // converge well below the game's own scale (the pot is 10 chips, and the
    // f32 backend reaches ~2.5e-3 in the same 200 iterations).
    assert!(
        nash_conv < 0.02,
        "i16 storage should still converge on the river subgame, got nash_conv={nash_conv}"
    );

    let checkpoint_data = hu_postflop::checkpoint::read_checkpoint(&checkpoint).unwrap();
    assert!(matches!(
        checkpoint_data.state.storage,
        hu_engine::StorageState::I16 { .. }
    ));
}

/// Ported from the retired desktop backend: the canonical 6-max default
/// surface must build a public tree and size a policy arena without
/// allocating it. `--resources` is the CLI's only tree-preflight surface.
#[test]
fn validate_resources_sizes_the_default_surface_tree() {
    let config =
        workspace_root().join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_default.toml");
    let output = run_solvers(&[
        "validate",
        config.to_str().unwrap(),
        "--resources",
        "--format",
        "json",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validate --format json emits JSON");
    let resources = &summary["resources"];
    assert_eq!(resources["complete"], serde_json::json!(true));
    for field in [
        "decisionNodes",
        "terminalEdges",
        "policyColumns",
        "policySlots",
        "solverStateBytes",
    ] {
        assert!(
            resources[field].as_u64().is_some_and(|value| value > 0),
            "{field} must be a positive preflight count: {resources}"
        );
    }
}

/// Ported from the retired desktop backend: the full-option surface fixture
/// must survive normalization with its non-default choices intact.
#[test]
fn validate_accepts_the_full_surface_fixture() {
    let config = workspace_root()
        .join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_full_surface.toml");
    let output = run_solvers(&[
        "validate",
        config.to_str().unwrap(),
        "--show-effective",
        "--resources",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "opponent_exploration = 0.125",
        "max_time = \"12h\"",
        "probability_encoding = \"f32\"",
    ] {
        assert!(
            stdout.contains(expected),
            "effective config lost {expected}:\n{stdout}"
        );
    }
    assert!(stdout.contains("\"complete\":true"));
}

// --- run directory: status / watch / runs ls ------------------------------

/// Builds a run directory the way `solve --out` leaves one, without paying
/// for a real solve. `status`, `watch`, and `runs ls` read only these files,
/// so this exercises the same contract the commands promise.
fn fabricate_run(directory: &std::path::Path, state: &str, with_checkpoint: bool) {
    std::fs::create_dir_all(directory).unwrap();
    let run_id = directory.file_name().unwrap().to_string_lossy();
    let terminal = state != "running";
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "runId": run_id,
        "state": state,
        "gameKind": "mw-preflop",
        "configSchema": "solvers.nlh/v1",
        "configHash": "aa".repeat(32),
        "cliVersion": "0.1.0",
        "command": ["solve", "config.toml"],
        // A pid that owns nothing, so a `running` manifest here is the
        // abandoned case.
        "pid": 0,
        "createdUnixMs": 1_700_000_000_000u64,
        "startedUnixMs": 1_700_000_000_000u64,
        "finishedUnixMs": terminal.then_some(1_700_000_060_000u64),
        "failure": serde_json::Value::Null,
        "completion": terminal.then_some("target-reached"),
    });
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let mut events = String::from(
        "{\"seq\":0,\"unixMs\":1700000000000,\"level\":\"info\",\"kind\":\"state\",\"state\":\"running\"}\n",
    );
    if terminal {
        events.push_str(&format!(
            "{{\"seq\":1,\"unixMs\":1700000060000,\"level\":\"info\",\"kind\":\"state\",\"state\":\"{state}\"}}\n"
        ));
    }
    std::fs::write(directory.join("events.jsonl"), events).unwrap();
    std::fs::write(
        directory.join("progress.jsonl"),
        "{\"sweeps\":128,\"elapsedSecs\":12.5}\n",
    )
    .unwrap();
    if with_checkpoint {
        std::fs::write(
            directory.join("checkpoint.mwckpt"),
            b"not a real checkpoint",
        )
        .unwrap();
    }
}

#[test]
fn status_reports_a_finished_run_as_json() {
    let dir = temp_dir("run-status");
    let run = dir.join("run-a");
    fabricate_run(&run, "completed", false);

    let output = run_solvers_ok(&["status", run.to_str().unwrap(), "--format", "json"]);
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["runId"], "run-a");
    assert_eq!(status["state"], "completed");
    assert_eq!(status["sweeps"], 128);
    assert_eq!(status["completion"], "target-reached");
    assert_eq!(status["resumable"], false);
    assert!(status["eventsOffset"].as_u64().unwrap() > 0);
}

/// The case the whole run-directory contract exists for: a run whose owning
/// process is gone must not keep reporting itself as running.
#[cfg(unix)]
#[test]
fn status_reports_an_abandoned_run_as_interrupted_and_resumable() {
    let dir = temp_dir("run-status-abandoned");
    let run = dir.join("run-b");
    fabricate_run(&run, "running", true);

    let output = run_solvers_ok(&["status", run.to_str().unwrap(), "--format", "json"]);
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["state"], "interrupted");
    assert_eq!(status["recordedState"], "running");
    assert_eq!(status["resumable"], true);
}

#[test]
fn watch_replays_a_finished_run_and_resumes_from_an_offset() {
    let dir = temp_dir("run-watch");
    let run = dir.join("run-c");
    fabricate_run(&run, "completed", false);

    let all = run_solvers_ok(&["watch", run.to_str().unwrap(), "--format", "json"]);
    let lines: Vec<&str> = std::str::from_utf8(&all.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 2, "stdout: {lines:?}");
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["seq"], 0);

    // Resuming past the first line yields only what follows it.
    let first_line_len = std::fs::read_to_string(run.join("events.jsonl"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .len()
        + 1;
    let tail = run_solvers_ok(&[
        "watch",
        run.to_str().unwrap(),
        "--from",
        &first_line_len.to_string(),
        "--format",
        "json",
    ]);
    let tail_lines: Vec<&str> = std::str::from_utf8(&tail.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(tail_lines.len(), 1);
    let only: serde_json::Value = serde_json::from_str(tail_lines[0]).unwrap();
    assert_eq!(only["seq"], 1);
}

#[test]
fn watch_rejects_a_directory_that_is_not_a_run() {
    let dir = temp_dir("run-watch-invalid");
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let output = run_solvers(&["watch", dir.join("empty").to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a run directory"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn runs_ls_lists_run_directories_and_skips_everything_else() {
    let dir = temp_dir("runs-ls");
    fabricate_run(&dir.join("run-a"), "completed", false);
    fabricate_run(&dir.join("run-b"), "failed", false);
    std::fs::create_dir_all(dir.join("scratch")).unwrap();
    std::fs::write(dir.join("notes.txt"), "x").unwrap();

    let output = run_solvers_ok(&["runs", "ls", dir.to_str().unwrap(), "--format", "json"]);
    let listing: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let runs = listing["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["runId"], "run-a");
    assert_eq!(runs[1]["runId"], "run-b");
}

#[test]
#[ignore = "builds the full EHS2 tables; explicit release acceptance only"]
fn a_solve_records_a_complete_run_directory() {
    let dir = temp_dir("run-directory-contract");
    let config = workspace_root()
        .join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_3max_smoke.toml");
    let run = dir.join("run");
    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);

    for name in [
        "manifest.json",
        "events.jsonl",
        "progress.jsonl",
        "run.json",
        "run.toml",
        "checkpoint.mwckpt",
        "solution.mwsol",
    ] {
        assert!(run.join(name).is_file(), "{name} is missing");
    }

    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["state"], "completed");
    assert_eq!(manifest["gameKind"], "mw-preflop");
    assert_eq!(manifest["configSchema"], "solvers.nlh/v1");
    assert!(manifest["finishedUnixMs"].as_u64().is_some());

    // `run.toml` must be the config the run actually used, so a run
    // directory can be re-solved from itself.
    let recorded = std::fs::read_to_string(run.join("run.toml")).unwrap();
    assert!(recorded.contains("solvers.nlh/v1"));

    let events: Vec<serde_json::Value> = std::fs::read_to_string(run.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.first().unwrap()["state"], "running");
    assert_eq!(events.last().unwrap()["state"], "completed");
    assert!(events.iter().any(|event| event["kind"] == "stop"));
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], index as u64, "event sequence must be dense");
    }
}

/// The iteration count on a run directory's last progress row.
#[cfg(unix)]
fn last_progress_iteration(run: &std::path::Path) -> u64 {
    let progress = std::fs::read_to_string(run.join("progress.jsonl")).unwrap();
    let last = progress.lines().last().expect("at least one progress row");
    serde_json::from_str::<serde_json::Value>(last).unwrap()["iteration"]
        .as_u64()
        .unwrap()
}

/// A stopped heads-up run is resumable off its own checkpoint name, which
/// differs from the multiway one.
#[test]
fn status_reports_a_canceled_heads_up_run_as_resumable() {
    let dir = temp_dir("run-status-hu");
    let run = dir.join("run-hu");
    fabricate_run(&run, "canceled", false);
    std::fs::write(run.join("checkpoint.ckpt"), b"not a real checkpoint").unwrap();

    let output = run_solvers_ok(&["status", run.to_str().unwrap(), "--format", "json"]);
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["state"], "canceled");
    assert_eq!(status["resumable"], true);
}

/// Ctrl-C during a heads-up solve must stop at a checkpoint boundary and
/// leave the run resumable, the same way it does for a multiway solve.
#[test]
#[cfg(unix)]
#[ignore = "spawns and signals a solve; explicit release acceptance only"]
fn a_canceled_heads_up_solve_closes_as_canceled_and_resumes() {
    let dir = temp_dir("hu-cancel-resume");
    let config = dir.join("long.toml");
    std::fs::write(
        &config,
        RIVER_NO_EARLY_STOP.replace("iterations = 200", "iterations = 20000000"),
    )
    .unwrap();
    let run = dir.join("run");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args([
            "solve",
            config.to_str().unwrap(),
            "--out",
            run.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("spawn solve");
    while !run.join("progress.jsonl").exists() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) };
    assert!(child.wait().expect("wait for solve").success());

    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["state"], "canceled");
    assert_eq!(manifest["completion"], "cancelled");

    let before = last_progress_iteration(&run);

    let mut resumed = std::process::Command::new(env!("CARGO_BIN_EXE_solvers"))
        .args(["resume", run.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("spawn resume");
    std::thread::sleep(std::time::Duration::from_millis(500));
    unsafe { libc::kill(resumed.id() as libc::pid_t, libc::SIGINT) };
    assert!(resumed.wait().expect("wait for resume").success());

    let after = last_progress_iteration(&run);
    assert!(
        after > before,
        "resume must continue past {before}, got {after}"
    );

    // The resumed segment continues the same event sequence.
    let seqs: Vec<u64> = std::fs::read_to_string(run.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
}

/// A cold cache costs minutes; a warm one must cost nothing. The run also
/// has to say which of the two happened, since a watcher otherwise sees an
/// unexplained silence before the first progress row.
#[test]
#[ignore = "builds the full EHS2 tables once; explicit release acceptance only"]
fn the_abstraction_cache_is_shared_across_runs() {
    let dir = temp_dir("ehs2-cache");
    let cache = dir.join("cache");
    let config = workspace_root()
        .join("crates/mw-preflop/tests/fixtures/preflop_multiway_v1_3max_smoke.toml");

    let cold = run_solvers_ok(&[
        "--cache-dir",
        cache.to_str().unwrap(),
        "solve",
        config.to_str().unwrap(),
        "--out",
        dir.join("cold").to_str().unwrap(),
    ]);
    assert!(
        String::from_utf8_lossy(&cold.stderr).contains("ehs2 tables: built"),
        "the first run must build the tables"
    );

    // The table's file name carries the bucket counts, so a second run with
    // different counts would not reuse this one.
    let cached: Vec<_> = std::fs::read_dir(cache.join("ehs2"))
        .expect("cache directory")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(cached.len(), 1, "{cached:?}");
    assert!(cached[0].contains("-f2-t2-r2."), "{cached:?}");

    let warm_run = dir.join("warm");
    let warm = run_solvers_ok(&[
        "--cache-dir",
        cache.to_str().unwrap(),
        "solve",
        config.to_str().unwrap(),
        "--out",
        warm_run.to_str().unwrap(),
    ]);
    assert!(
        String::from_utf8_lossy(&warm.stderr).contains("ehs2 tables: loaded"),
        "the second run must load the cached tables"
    );

    let events = std::fs::read_to_string(warm_run.join("events.jsonl")).unwrap();
    assert!(
        events.contains("ehs2 tables loaded in"),
        "the run event log must record the cache hit: {events}"
    );
    let evaluated = run_solvers_ok(&[
        "--cache-dir",
        cache.to_str().unwrap(),
        "evaluate",
        warm_run.join("solution.mwsol").to_str().unwrap(),
        "--samples",
        "2",
        "--br-traversals",
        "1",
    ]);
    let report: serde_json::Value =
        serde_json::from_slice(&evaluated.stdout).expect("evaluate stdout must be pure JSON");
    assert_eq!(report["samples"], 2);
    let artifact = read_mwsol_full(&warm_run.join("solution.mwsol"));
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(warm_run.join("run.json")).unwrap()).unwrap();
    assert_eq!(
        summary["algorithmFingerprint"],
        runfiles::config_hash_hex(&artifact.algorithm_fingerprint),
        "run summary and solution must identify the same update semantics"
    );
}

/// Postflop publishes through `solution.sol`, which `export` reads. There
/// is deliberately no second JSON of the same thing.
#[test]
fn postflop_publishes_through_the_artifact() {
    let dir = temp_dir("postflop-export");
    let config = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
    let run = dir.join("run");
    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);

    assert!(
        run.join("solution.sol").exists(),
        "artifact must be written"
    );

    // Every view renders, and the per-node views accept `all`.
    let sol = run.join("solution.sol");
    for (view, node) in [
        ("summary", "root"),
        ("range", "root"),
        ("tree", "all"),
        ("actions", "all"),
        ("strategy", "root"),
        ("ev", "root"),
    ] {
        for format in ["json", "csv"] {
            let output = run_solvers_ok(&[
                "export",
                sol.to_str().unwrap(),
                view,
                "--node",
                node,
                "--format",
                format,
            ]);
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                !stdout.trim().is_empty(),
                "export {view} --format {format} produced nothing"
            );
            // Progress notices belong on stderr: a CSV a caller pipes into
            // a file must be nothing but the table.
            assert!(
                !stdout.contains("rebuilding tree"),
                "export {view} leaked a progress line into stdout: {stdout}"
            );
        }
    }

    // The EV view reports the same root numbers the summary does.
    let summary = run_solvers_ok(&["export", sol.to_str().unwrap(), "summary"]);
    let summary: serde_json::Value = serde_json::from_slice(&summary.stdout).expect("summary json");
    let ev_oop = summary["ev_oop"].as_f64().expect("ev_oop");
    assert!(
        ev_oop > 0.0 && ev_oop < 10.0,
        "ev_oop {ev_oop} outside the pot"
    );
}

/// `--history` picked a node out of a second JSON export that no longer
/// exists; `export --node` is the only path now. The flag is gone from both
/// commands, so clap refuses it (usage error, exit code 2) rather than
/// ignoring it, and a refused `solve` leaves no run directory behind.
#[test]
fn the_retired_history_flag_is_refused() {
    let dir = temp_dir("history-flag");
    let config = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
    let run = dir.join("run");

    let rejected = run_solvers(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
        "--history",
        "r5",
    ]);
    assert_eq!(rejected.status.code(), Some(2), "solve --history");
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("--history"), "stderr: {stderr}");
    assert!(
        !run.exists(),
        "a refused flag must not create the run directory"
    );

    let rejected = run_solvers(&["resume", run.to_str().unwrap(), "--history", "r5"]);
    assert_eq!(rejected.status.code(), Some(2), "resume --history");
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("--history"), "stderr: {stderr}");
}

/// Missing, retired and unknown schemas fail NLH001 on every config command.
/// Refusal happens before a solve creates its run directory.
#[test]
fn removed_family_schemas_are_refused_as_unsupported() {
    let dir = temp_dir("removed-schemas");
    for (name, schema, game) in [
        ("postflop", "solvers.postflop/v1", ""),
        ("multiway", "solvers.multiway-preflop/v1", ""),
        ("toy", "solvers.toy/v1", "kind = \"kuhn\""),
        (
            "preflop-hu",
            "solvers.preflop-hu/v1",
            "effective_stack_bb = 10.0",
        ),
    ] {
        let text = format!("schema = \"{schema}\"\n\n[game]\n{game}\n\n[run]\niterations = 10\n");
        let config = dir.join(format!("{name}.toml"));
        std::fs::write(&config, &text).unwrap();
        let config = config.to_str().unwrap();
        let run = dir.join(format!("{name}-run"));

        let commands = [
            vec!["validate", config],
            vec!["solve", config, "--out", run.to_str().unwrap()],
            vec!["inspect", config],
            vec!["report", config, "--boards", "2c7d9h"],
        ];
        for args in commands {
            let output = run_solvers(&args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
            assert!(
                stderr.contains("NLH001") && stderr.contains(schema),
                "{args:?} must name the removed schema as unsupported: {stderr}"
            );
            assert!(
                stderr.contains("solvers.nlh/v1") && stderr.contains("docs/nlh-input-v1.jp.md"),
                "{args:?} must list the schemas that remain: {stderr}"
            );
        }
        assert!(
            !run.exists(),
            "a refused solve must not create its run directory"
        );
    }
}

/// `compare` lines two artifacts up by node, so an artifact compared with
/// itself must report exactly zero, and two solves of the same config that
/// ran for different lengths must differ a little and agree on the spot.
#[test]
fn postflop_compare_reports_zero_against_itself() {
    let dir = temp_dir("postflop-compare");
    let short = workspace_root().join("crates/cli/tests/fixtures/river_small.toml");
    let long = dir.join("long.toml");
    std::fs::write(
        &long,
        std::fs::read_to_string(&short)
            .unwrap()
            .replace("iterations = 200", "iterations = 2000"),
    )
    .unwrap();

    let run_short = dir.join("short");
    let run_long = dir.join("long");
    run_solvers_ok(&[
        "solve",
        short.to_str().unwrap(),
        "--out",
        run_short.to_str().unwrap(),
    ]);
    run_solvers_ok(&[
        "solve",
        long.to_str().unwrap(),
        "--out",
        run_long.to_str().unwrap(),
    ]);
    let a = run_short.join("solution.sol");
    let b = run_long.join("solution.sol");

    let same = run_solvers_ok(&["compare", a.to_str().unwrap(), a.to_str().unwrap()]);
    let same: serde_json::Value = serde_json::from_slice(&same.stdout).expect("compare json");
    assert_eq!(same["mean_strategy_l1"].as_f64(), Some(0.0));
    assert_eq!(same["max_ev_delta"].as_f64(), Some(0.0));
    assert!(same["nodes"].as_u64().unwrap() > 0);

    let differ = run_solvers_ok(&["compare", a.to_str().unwrap(), b.to_str().unwrap()]);
    let differ: serde_json::Value = serde_json::from_slice(&differ.stdout).expect("compare json");
    assert!(
        differ["mean_strategy_l1"].as_f64().unwrap() > 0.0,
        "two different solve lengths should not play identically"
    );
    // A longer solve of the same spot converges toward the same root EV.
    let ev = differ["ev_oop"].as_array().unwrap();
    let (left, right) = (ev[0].as_f64().unwrap(), ev[1].as_f64().unwrap());
    assert!(
        (left - right).abs() < 0.1,
        "root EV drifted: {left} vs {right}"
    );
}

/// Read-only monitoring remains available even when the embedded config is retired.
#[test]
fn old_run_monitoring_preserves_the_recorded_schema_and_game_kind() {
    let dir = tempfile::tempdir().unwrap();
    for (name, schema, kind) in [
        ("postflop", "solvers.postflop/v1", "postflop"),
        (
            "multiway",
            "solvers.multiway-preflop/v1",
            "preflop-multiway",
        ),
    ] {
        let run = dir.path().join(name);
        fabricate_run(&run, "completed", false);
        let manifest_path = run.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["configSchema"] = schema.into();
        manifest["gameKind"] = kind.into();
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        std::fs::write(run.join("run.toml"), format!("schema = '{schema}'\n")).unwrap();
        let output = run_solvers_ok(&["status", run.to_str().unwrap(), "--format", "json"]);
        let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(status.to_string().contains(schema), "{status}");
        assert!(status.to_string().contains(kind), "{status}");
        run_solvers_ok(&["watch", run.to_str().unwrap(), "--format", "json"]);
    }
    let listed = run_solvers_ok(&[
        "runs",
        "ls",
        dir.path().to_str().unwrap(),
        "--format",
        "json",
    ]);
    let rows: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(rows["runs"].as_array().unwrap().len(), 2);
    assert!(rows.to_string().contains("solvers.postflop/v1"));
    assert!(rows.to_string().contains("solvers.multiway-preflop/v1"));
}
