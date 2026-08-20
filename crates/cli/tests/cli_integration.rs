mod common;

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
fn read_mwsol_full(path: &std::path::Path) -> formats::MultiwaySolution {
    let mut reader = formats::MwSolReader::open(path).expect("open mwsol");
    let total = reader.strategy_count();
    let mut cursor = 0;
    let mut strategies = Vec::with_capacity(total);
    while cursor < total {
        let page = reader
            .read_strategy_page(cursor, formats::MWSOL_MAX_PAGE_LIMIT)
            .expect("read strategy page");
        strategies.extend(page.strategies);
        cursor = page.next_cursor.unwrap_or(total);
    }
    let metadata = reader.metadata().clone();
    formats::MultiwaySolution {
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
schema = "solvers.toy/v1"

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
    let loaded = formats::read_checkpoint(&checkpoint).expect("read checkpoint");
    assert_eq!(loaded.iteration, reported_iters);

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

/// Resuming must reproduce a straight solve exactly.
///
/// The partial run uses its own config file capped at 200 iterations, and
/// the resumed run swaps in the 400-iteration config. The checkpoint is
/// stamped with the config hash, so the two files must otherwise be
/// byte-identical -- which is what makes this a real equivalence check
/// rather than a re-run.
#[test]
fn resume_equivalence_kuhn() {
    let dir = temp_dir("resume-equiv");
    let full_config = dir.join("kuhn-400.toml");
    std::fs::write(&full_config, KUHN_NO_EARLY_STOP).unwrap();
    let partial_config = dir.join("kuhn-200.toml");
    std::fs::write(
        &partial_config,
        KUHN_NO_EARLY_STOP.replace("iterations = 400", "iterations = 200"),
    )
    .unwrap();

    // Run A: straight solve to the full 400 iterations.
    let run_a = dir.join("a");
    run_solvers_ok(&[
        "solve",
        full_config.to_str().unwrap(),
        "--out",
        run_a.to_str().unwrap(),
    ]);

    // Run B: solve to 200, then swap in the 400-iteration config and resume.
    let run_b = dir.join("b");
    run_solvers_ok(&[
        "solve",
        partial_config.to_str().unwrap(),
        "--out",
        run_b.to_str().unwrap(),
    ]);
    let checkpoint = run_b.join("checkpoint.ckpt");
    let loaded = formats::read_checkpoint(&checkpoint).unwrap();
    assert_eq!(loaded.iteration, 200);

    // The run directory carries the config the checkpoint was stamped with,
    // so continuing to 400 means rewriting both together.
    std::fs::write(run_b.join("run.toml"), KUHN_NO_EARLY_STOP).unwrap();
    let restamped = formats::Checkpoint {
        config_hash: formats::config_hash(KUHN_NO_EARLY_STOP.as_bytes()),
        ..loaded
    };
    formats::write_checkpoint(&checkpoint, restamped.config_hash, &restamped.state).unwrap();

    run_solvers_ok(&["resume", run_b.to_str().unwrap()]);

    let a = std::fs::read_to_string(run_a.join("strategy.json")).unwrap();
    let b = std::fs::read_to_string(run_b.join("strategy.json")).unwrap();
    assert_eq!(
        a, b,
        "a checkpointed-then-resumed solve must byte-for-byte match a straight solve"
    );
}

#[test]

fn legacy_multiway_solve_is_rejected_before_creating_artifacts() {
    let dir = temp_dir("multiway-legacy-rejected");
    let config = dir.join("lowered.toml");
    std::fs::write(&config, common::LOWERED_LEGACY).unwrap();
    let run = dir.join("run");
    let output = run_solvers(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);
    assert!(
        !output.status.success(),
        "legacy Multiway solve unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("MWP003"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The rejection happens before any artifact is produced. The run
    // directory itself exists (it is created to hold the manifest), so the
    // check is that it stayed empty of results.
    for name in ["run.json", "checkpoint.mwckpt", "solution.mwsol"] {
        assert!(!run.join(name).exists(), "{name} was written anyway");
    }
}

#[test]
fn production_validate_rejects_the_retired_rollout_abstraction() {
    let dir = temp_dir("multiway-rollout-rejected");
    let config = dir.join("rollout.toml");
    std::fs::write(&config, common::V1_RETIRED_ROLLOUT).unwrap();
    let output = run_solvers(&["validate", config.to_str().unwrap()]);
    assert!(
        !output.status.success(),
        "a config naming the retired rollout abstraction must not validate"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("MWP001"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "builds the full EHS2 tables; explicit release acceptance only"]
fn multiway_v1_u16_storage_writes_a_quantized_mwsol() {
    let dir = temp_dir("multiway-v1-u16");
    let config = workspace_root().join("examples/preflop_multiway_v1_production_smoke.toml");
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
        cli::multiway_v1::PRODUCTION_POLICY_ARENA_AUTO_BYTES
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
    let config = dir.join("kuhn.toml");
    std::fs::write(&config, KUHN_NO_EARLY_STOP).unwrap();
    let run = dir.join("run");

    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        run.to_str().unwrap(),
    ]);

    // Tamper with the run directory's copy of the config after the
    // checkpoint was stamped against it.
    let mut tampered = KUHN_NO_EARLY_STOP.to_string();
    tampered.push_str("\n# tampered\n");
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
/// `crates/holdem/tests/viewer.rs`'s `small_turn_config` and
/// `crates/cli/src/sol.rs`'s own unit-test fixture, duplicated here (rather
/// than shared) since this is a separate test binary with no access to
/// `cli`'s internal `sol` module.
const TINY_TURN_TOML: &str = r#"
schema = "solvers.postflop/v1"

[game]
kind = "postflop"
board = "2s 7s Ks 2h"
oop_range = "44,55"
ip_range = "33,66"
pot = 2
effective_stack = 20

[game.bets.turn]
oop = [0.75]
ip = [0.75]
max_raises = 1

[game.bets.river]
oop = [1.0]
ip = [1.0]
max_raises = 1

[run]
iterations = 32
check_every = 32
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

    // `.sol` omits river strategy blocks (NoRivers is the default mode) and
    // stores 16-bit quantized probabilities instead of full-precision
    // regrets + strategy sums, so it must be substantially smaller than the
    // checkpoint of the same solve.
    let sol_size = std::fs::metadata(&sol_path).unwrap().len();
    let ckpt_size = std::fs::metadata(&checkpoint).unwrap().len();
    assert!(
        sol_size < ckpt_size / 4,
        "sol ({sol_size} bytes) should be well under 1/4 of the checkpoint ({ckpt_size} bytes)"
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
schema = "solvers.postflop/v1"

[game]
kind = "postflop"
board = "2s 7s Ks 2h"
oop_range = "44,55"
ip_range = "33,66"
pot = 2
effective_stack = 20
iso_merging = false

[game.bets.turn]
oop = [0.75]
ip = [0.75]
max_raises = 1

[game.bets.river]
oop = [1.0]
ip = [1.0]
max_raises = 1

[run]
iterations = 200
check_every = 200
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
    std::fs::write(&config, TINY_TURN_TOML_NO_ISO).unwrap();
    let sol_path = dir.join("out.sol");

    run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--sol",
        sol_path.to_str().unwrap(),
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

#[test]
fn i16_storage_solve_converges_and_checkpoint_round_trips() {
    let dir = temp_dir("i16");
    let config = workspace_root().join("examples/kuhn_i16.toml");
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

// --- preflop (Mode B) -------------------------------------------------------

/// 10bb push/fold: SB may only jam or fold, BB may only call or fold (root
/// has exactly 2 actions). `{cache}` is filled in with a tempdir-local path
/// so the exact equity table's compute-then-cache-hit round trip is
/// self-contained and never touches a shared/repo-level cache.
const PREFLOP_PUSHFOLD_TOML_TEMPLATE: &str = r#"
schema = "solvers.preflop-hu/v1"

[game]
kind = "preflop"
effective_stack_bb = 10.0
open_sizes_bb = []
raise_factors = []
max_raises = 1
allow_limp = false
equity_cache = "{cache}"

[algorithm]
schedule = "dcfr"

[run]
iterations = 400
check_every = 100
"#;

#[test]
#[ignore = "computes the exact preflop equity table; CI runs it in release with --include-ignored"]
fn preflop_pushfold_solve_smoke() {
    let dir = temp_dir("preflop-pushfold");
    let cache = dir.join("equity.bin");
    // TOML string escaping: a Windows-style path could contain backslashes,
    // but tempdir() on the platforms this runs on never does, so a plain
    // substitution is safe here.
    let config_text = PREFLOP_PUSHFOLD_TOML_TEMPLATE.replace("{cache}", &cache.to_string_lossy());
    let config = dir.join("pushfold.toml");
    std::fs::write(&config, config_text).unwrap();
    let output_path = dir.join("out.json");

    let output = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("equity: computing"),
        "first run should compute the table: {stdout}"
    );
    assert!(stdout.contains("root:"), "stdout: {stdout}");
    assert!(stdout.contains("nash_conv"), "stdout: {stdout}");
    assert!(stdout.contains("done:"), "stdout: {stdout}");
    assert!(cache.exists(), "equity cache file must be created");

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output_path).unwrap())
            .expect("--output file must parse as JSON");
    let class_labels = json["class_labels"].as_array().expect("class_labels array");
    assert_eq!(class_labels.len(), 169);

    let entries = json["entries"].as_array().expect("entries array");
    let root = entries
        .iter()
        .find(|e| e["history"] == "")
        .expect("root entry (history == \"\")");
    let actions = root["actions"].as_array().expect("actions array");
    assert_eq!(actions.len(), 2, "push/fold root must have 2 actions");
    let strategy = root["strategy"].as_array().expect("strategy array");
    assert_eq!(strategy.len(), 2, "one strategy row per action");
    for row in strategy {
        assert_eq!(
            row.as_array().unwrap().len(),
            169,
            "each action's strategy row must cover all 169 classes"
        );
    }

    // Second run: the equity table should now load from the cache instead of
    // recomputing, and the run must still succeed.
    let output2 = run_solvers_ok(&[
        "solve",
        config.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("equity: loaded cached table"),
        "second run should hit the cache: {stdout2}"
    );
    assert!(stdout2.contains("root:"), "stdout: {stdout2}");
}

// --- [game.postflop] bucketed model: error path only ------------------------
//
// A cold bucketed solve builds a full-street EHS² abstraction (~10 minutes
// in release mode), which is far past what should run automatically here --
// see `examples/preflop_hu_100bb_bucketed.toml`'s own note; the orchestrator
// validates that config end-to-end manually. This test only exercises the
// fast, no-build error path: `model` validation runs before any equity
// table, abstraction, or artifact work starts, so it fails immediately.
#[test]
fn preflop_bucketed_unsupported_model_errors() {
    let dir = temp_dir("preflop-bucketed-bad-model");
    let config_text = r#"
schema = "solvers.preflop-hu/v1"

[game]
kind = "preflop"
effective_stack_bb = 10.0
open_sizes_bb = []
raise_factors = []
max_raises = 1
allow_limp = false

[game.postflop]
model = "unsupported-model"

[run]
iterations = 1
"#;
    let config = dir.join("bad_model.toml");
    std::fs::write(&config, config_text).unwrap();

    let output = run_solvers(&[
        "solve",
        config.to_str().unwrap(),
        "--out",
        dir.join("run").to_str().unwrap(),
    ]);
    assert!(
        !output.status.success(),
        "an unsupported game.postflop.model must fail the solve"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported-model") && stderr.contains("bucketed"),
        "expected an error naming the bad value and the supported one, got: {stderr}"
    );
}

/// Ported from the retired desktop backend: the canonical 6-max default
/// surface must build a public tree and size a policy arena without
/// allocating it. `--resources` is the CLI's only tree-preflight surface.
#[test]
fn validate_resources_sizes_the_default_surface_tree() {
    let config = workspace_root().join("examples/preflop_multiway_v1_default.toml");
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
    let config = workspace_root().join("examples/preflop_multiway_v1_full_surface.toml");
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
        "preflop_first_to_act = 4",
        "opponent_exploration = 0.125",
        "max_time = \"12h\"",
        "probability_encoding = \"f32\"",
    ] {
        assert!(
            stdout.contains(expected),
            "effective config lost {expected}:\n{stdout}"
        );
    }
    assert!(stdout.contains("resources: complete=true"));
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
        "gameKind": "preflop-multiway",
        "configSchema": "solvers.multiway-preflop/v1",
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
    let config = workspace_root().join("examples/preflop_multiway_v1_3max_smoke.toml");
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
    assert_eq!(manifest["gameKind"], "preflop-multiway");
    assert_eq!(manifest["configSchema"], "solvers.multiway-preflop/v1");
    assert!(manifest["finishedUnixMs"].as_u64().is_some());

    // `run.toml` must be the config the run actually used, so a run
    // directory can be re-solved from itself.
    let recorded = std::fs::read_to_string(run.join("run.toml")).unwrap();
    assert!(recorded.contains("solvers.multiway-preflop/v1"));

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
#[ignore = "spawns and signals a solve; explicit release acceptance only"]
fn a_canceled_heads_up_solve_closes_as_canceled_and_resumes() {
    let dir = temp_dir("hu-cancel-resume");
    let config = dir.join("long.toml");
    std::fs::write(
        &config,
        KUHN_NO_EARLY_STOP.replace("iterations = 400", "iterations = 20000000"),
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
