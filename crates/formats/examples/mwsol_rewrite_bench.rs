//! Small, dependency-free `.mwsol` rewrite benchmark.
//!
//! Usage:
//!
//! ```text
//! cargo run --release -p formats --example mwsol_rewrite_bench -- <input.mwsol> <output.mwsol> [f32|u16]
//! cargo run --release -p formats --example mwsol_rewrite_bench -- --compare-profiles <left.mwsol> <right.mwsol>
//! ```
//!
//! The input is decoded before the write timer starts.  The output is then
//! decoded again and checked for matching metadata, keys, actions, and
//! probabilities (within U16 quantization error when requested).
//! `--compare-profiles` is read-only: it compares the solver-state identity
//! and strategy profile while deliberately ignoring evaluation-only seat
//! results and runtime metadata.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use formats::{
    MWSOL_MAX_PAGE_LIMIT, MultiwaySolution, MwSolError, MwSolReader, MwsolStorage, write_mwsol_with,
};

fn decode_solution(path: &Path) -> Result<MultiwaySolution, MwSolError> {
    let mut reader = MwSolReader::open(path)?;
    let total = reader.strategy_count();
    let mut strategies = Vec::with_capacity(total);
    let mut cursor = 0;
    while cursor < total {
        let page = reader.read_strategy_page(cursor, MWSOL_MAX_PAGE_LIMIT)?;
        strategies.extend(page.strategies);
        cursor = page.next_cursor.unwrap_or(total);
    }
    let metadata = reader.metadata().clone();
    Ok(MultiwaySolution {
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
    })
}

fn verify_round_trip(
    expected: &MultiwaySolution,
    actual: &MultiwaySolution,
    storage: MwsolStorage,
) -> Result<(), String> {
    if expected.schema_version != actual.schema_version
        || expected.config_toml != actual.config_toml
        || expected.config_fingerprint != actual.config_fingerprint
        || expected.game_fingerprint != actual.game_fingerprint
        || expected.algorithm_fingerprint != actual.algorithm_fingerprint
        || expected.abstraction_fingerprint != actual.abstraction_fingerprint
        || expected.configuration_fingerprint != actual.configuration_fingerprint
        || expected.stop_status != actual.stop_status
        || expected.chip_unit_bb != actual.chip_unit_bb
        || expected.sweeps != actual.sweeps
        || expected.approximate_profile != actual.approximate_profile
        || expected.seats != actual.seats
        || expected.histories != actual.histories
        || expected.public_states != actual.public_states
        || expected.strategy_weights != actual.strategy_weights
        || expected.strategies.len() != actual.strategies.len()
    {
        return Err("metadata or strategy shape changed during rewrite".into());
    }
    let tolerance = match storage {
        MwsolStorage::F32 => 0.0,
        MwsolStorage::U16 => 1.0 / f32::from(u16::MAX),
        MwsolStorage::I16 => return Err("I16 is not supported by v4".into()),
    };
    for (index, (want, got)) in expected
        .strategies
        .iter()
        .zip(&actual.strategies)
        .enumerate()
    {
        if want.key != got.key || want.actions != got.actions {
            return Err(format!("strategy {index} key or actions changed"));
        }
        if want.probabilities.len() != got.probabilities.len()
            || want
                .probabilities
                .iter()
                .zip(&got.probabilities)
                .any(|(want, got)| (want - got).abs() > tolerance)
        {
            return Err(format!("strategy {index} probabilities changed"));
        }
    }
    Ok(())
}

fn compare_profiles(left_path: &Path, right_path: &Path) -> Result<(), String> {
    let left =
        decode_solution(left_path).map_err(|error| format!("left profile read failed: {error}"))?;
    let right = decode_solution(right_path)
        .map_err(|error| format!("right profile read failed: {error}"))?;

    if left.config_fingerprint != right.config_fingerprint
        || left.game_fingerprint != right.game_fingerprint
        || left.abstraction_fingerprint != right.abstraction_fingerprint
        || left.algorithm_fingerprint != right.algorithm_fingerprint
        || left.configuration_fingerprint != right.configuration_fingerprint
        || left.sweeps != right.sweeps
        || left.histories != right.histories
        || left.public_states != right.public_states
        || left.strategy_weights != right.strategy_weights
        || left.strategies.len() != right.strategies.len()
    {
        return Err("profile identity or strategy shape differs".into());
    }
    for (index, (left_block, right_block)) in
        left.strategies.iter().zip(&right.strategies).enumerate()
    {
        if left_block.key != right_block.key
            || left_block.actions != right_block.actions
            || left_block.probabilities != right_block.probabilities
        {
            return Err(format!("strategy {index} differs"));
        }
    }
    println!(
        "profiles_equal left={} right={} strategies={}",
        left_path.display(),
        right_path.display(),
        left.strategies.len(),
    );
    Ok(())
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let first = args
        .next()
        .ok_or_else(|| "missing input path".to_string())?;
    if first == std::ffi::OsStr::new("--compare-profiles") {
        let left = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "missing left profile path".to_string())?;
        let right = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "missing right profile path".to_string())?;
        if args.next().is_some() {
            return Err("too many arguments".into());
        }
        return compare_profiles(&left, &right);
    }
    let input = PathBuf::from(first);
    let output = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "missing output path".to_string())?;
    let storage = match args.next().as_deref() {
        None => MwsolStorage::F32,
        Some(value) if value == std::ffi::OsStr::new("f32") => MwsolStorage::F32,
        Some(value) if value == std::ffi::OsStr::new("u16") => MwsolStorage::U16,
        Some(other) => return Err(format!("unknown storage {:?}; expected f32 or u16", other)),
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }

    let solution = decode_solution(&input).map_err(|error| format!("read failed: {error}"))?;
    let started = Instant::now();
    write_mwsol_with(&output, &solution, storage)
        .map_err(|error| format!("write failed: {error}"))?;
    let write_elapsed = started.elapsed();
    let round_trip =
        decode_solution(&output).map_err(|error| format!("verify read failed: {error}"))?;
    verify_round_trip(&solution, &round_trip, storage)?;
    println!(
        "rewrite_ok input={} output={} strategies={} storage={storage:?} write_ms={:.3}",
        input.display(),
        output.display(),
        solution.strategies.len(),
        write_elapsed.as_secs_f64() * 1_000.0,
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("usage: mwsol_rewrite_bench <input.mwsol> <output.mwsol> [f32|u16]");
            eprintln!("   or: mwsol_rewrite_bench --compare-profiles <left.mwsol> <right.mwsol>");
            ExitCode::FAILURE
        }
    }
}
