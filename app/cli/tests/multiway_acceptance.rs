use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

struct FixtureRun {
    _directory: tempfile::TempDir,
    result: serde_json::Value,
    checkpoint: multiway::MultiwayCheckpoint,
    solution: formats::MultiwaySolution,
    elapsed: Duration,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has a workspace parent")
        .parent()
        .expect("crates directory has a workspace parent")
        .to_path_buf()
}

/// Fully decodes an `.mwsol` file through the paged `MwSolReader` API
/// (there is no longer a single-shot `read_mwsol` convenience wrapper).
fn read_mwsol_full(path: &Path) -> formats::MultiwaySolution {
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

fn run_fixture(name: &str) -> FixtureRun {
    let directory = tempfile::tempdir().expect("create acceptance tempdir");
    let result_path = directory.path().join("result.json");
    let checkpoint_path = directory.path().join("result.mwckpt");
    let solution_path = directory.path().join("result.mwsol");
    let config = workspace_root().join("examples").join(name);
    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("solve")
        .arg(&config)
        .arg("--output")
        .arg(&result_path)
        .arg("--checkpoint")
        .arg(&checkpoint_path)
        .arg("--sol")
        .arg(&solution_path)
        .output()
        .expect("run multiway acceptance fixture");
    let elapsed = started.elapsed();
    assert!(
        output.status.success(),
        "fixture {} failed\nstdout:\n{}\nstderr:\n{}",
        config.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result =
        serde_json::from_slice(&std::fs::read(&result_path).expect("acceptance result must exist"))
            .expect("acceptance result must be JSON");
    let checkpoint = multiway::MultiwayCheckpoint::load_unchecked(&checkpoint_path)
        .expect("acceptance checkpoint must load");
    let solution = read_mwsol_full(&solution_path);
    FixtureRun {
        _directory: directory,
        result,
        checkpoint,
        solution,
        elapsed,
    }
}

#[test]
#[ignore = "9-max all-street smoke is reserved for release --include-ignored acceptance runs"]
fn nine_max_eight_bucket_all_street_smoke() {
    let run = run_fixture("preflop_multiway_9max_8b_smoke.toml");
    assert_eq!(run.result["status"], "completed");
    assert_eq!(run.result["sweeps"], 2);
    assert_eq!(run.result["seats"].as_array().unwrap().len(), 9);
    assert_eq!(run.checkpoint.state.completed_sweeps, 2);
    assert_eq!(run.solution.seats.len(), 9);
    assert!(!run.solution.strategies.is_empty());
    assert!(
        run.solution
            .strategies
            .iter()
            .any(|block| block.key.street == 3),
        "the all-street fixture must visit at least one river information set"
    );
}

#[test]
#[ignore = "64/64/64 desktop benchmark is reserved for release --include-ignored runs"]
fn desktop_sixty_four_bucket_benchmark() {
    let run = run_fixture("preflop_multiway_6max_64b_desktop.toml");
    assert_eq!(run.result["status"], "completed");
    assert_eq!(run.result["sweeps"], 4);
    assert_eq!(run.result["seats"].as_array().unwrap().len(), 6);
    assert_eq!(run.checkpoint.state.completed_sweeps, 4);
    assert_eq!(run.solution.seats.len(), 6);
    assert!(!run.solution.strategies.is_empty());
    eprintln!(
        "multiway desktop 64/64/64: elapsed={:.3}s infosets={} memory={}MiB traversals/s={:.1}",
        run.elapsed.as_secs_f64(),
        run.result["infosets"],
        run.result["memoryBytes"].as_u64().unwrap_or_default() / (1024 * 1024),
        run.result["traversalsPerSecond"]
            .as_f64()
            .unwrap_or_default()
    );
}

#[test]
#[ignore = "computes the exact HU equity table and trains 200k sampled sweeps; release acceptance only"]
fn two_player_push_fold_tracks_the_existing_hu_exact_solver() {
    let directory = tempfile::tempdir().expect("create HU comparison tempdir");
    let equity_cache = directory.path().join("equity.bin");
    let hu_config_path = directory.path().join("hu.toml");
    let hu_output_path = directory.path().join("hu.json");
    let multiway_output_path = directory.path().join("multiway.json");
    let multiway_solution_path = directory.path().join("multiway.mwsol");

    let hu_template = std::fs::read_to_string(workspace_root().join("examples/pushfold_10bb.toml"))
        .expect("read existing HU push/fold fixture");
    let escaped_cache = equity_cache.to_string_lossy().replace('\\', "\\\\");
    let hu_config = hu_template.replace(".cache/preflop_equity.bin", &escaped_cache);
    std::fs::write(&hu_config_path, hu_config).expect("write isolated HU fixture");
    let hu_process = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("solve")
        .arg(&hu_config_path)
        .arg("--output")
        .arg(&hu_output_path)
        .output()
        .expect("run exact HU push/fold fixture");
    assert!(
        hu_process.status.success(),
        "HU fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&hu_process.stdout),
        String::from_utf8_lossy(&hu_process.stderr)
    );

    let multiway_config = workspace_root()
        .join("examples")
        .join("preflop_multiway_hu_10bb_pushfold.toml");
    let multiway_process = Command::new(env!("CARGO_BIN_EXE_solvers"))
        .arg("solve")
        .arg(&multiway_config)
        .arg("--output")
        .arg(&multiway_output_path)
        .arg("--sol")
        .arg(&multiway_solution_path)
        .output()
        .expect("run sampled two-player push/fold fixture");
    assert!(
        multiway_process.status.success(),
        "multiway fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&multiway_process.stdout),
        String::from_utf8_lossy(&multiway_process.stderr)
    );

    let hu: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&hu_output_path).unwrap()).unwrap();
    let root = hu["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["history"] == "")
        .expect("HU root strategy");
    let jam_action = root["actions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|action| {
            action
                .as_str()
                .is_some_and(|label| label.to_ascii_lowercase().contains("all-in"))
        })
        .expect("HU root jam action");
    let hu_jam: Vec<f64> = root["strategy"][jam_action]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_f64().unwrap())
        .collect();
    assert_eq!(hu_jam.len(), 169);

    let solution = read_mwsol_full(&multiway_solution_path);
    let mut multiway_jam = vec![None; 169];
    for block in solution.strategies.iter().filter(|block| {
        block.key.history == [0; 16]
            && block.key.actor == 0
            && block.key.street == 0
            && block.key.active_opponents == 1
    }) {
        let class = block.key.bucket_path[0] as usize;
        let jam_action = block
            .actions
            .iter()
            .position(|action| action.ends_with(":all-in"))
            .expect("multiway root jam action");
        assert!(multiway_jam[class].is_none());
        multiway_jam[class] = Some(f64::from(block.probabilities[jam_action]));
    }
    let multiway_jam: Vec<f64> = multiway_jam
        .into_iter()
        .map(|value| value.expect("200k sweeps must visit every preflop class"))
        .collect();
    let mean_absolute_error = hu_jam
        .iter()
        .zip(&multiway_jam)
        .map(|(&exact, &sampled)| (exact - sampled).abs())
        .sum::<f64>()
        / 169.0;
    let exact_frequency = hu_jam.iter().sum::<f64>() / 169.0;
    let sampled_frequency = multiway_jam.iter().sum::<f64>() / 169.0;
    assert!(mean_absolute_error < 0.25, "MAE={mean_absolute_error}");
    assert!(
        (exact_frequency - sampled_frequency).abs() < 0.10,
        "HU jam frequency={exact_frequency}, multiway={sampled_frequency}"
    );
}
