//! Read-only research comparison of an interrupted HU run, its resumed fork,
//! and an uninterrupted run. Signal delivery and process exit codes must be
//! verified by the external controller; artifacts alone cannot prove lineage.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use engine::StorageState;
use formats::{Checkpoint, MetricsRow, RunEventPayload, RunManifest, RunState, SolPayload};
use serde::{Deserialize, Serialize};
use serde_json::json;

const FILES: [&str; 7] = [
    formats::RUN_CONFIG_FILE,
    formats::RUN_MANIFEST_FILE,
    formats::RUN_EVENTS_FILE,
    formats::RUN_PROGRESS_FILE,
    formats::RUN_RESULT_FILE,
    formats::RUN_HU_CHECKPOINT_FILE,
    formats::RUN_HU_SOLUTION_FILE,
];

#[derive(Parser)]
struct Args {
    #[arg(long)]
    interrupted: PathBuf,
    #[arg(long)]
    resumed: PathBuf,
    #[arg(long)]
    straight: PathBuf,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Identity {
    path: PathBuf,
    bytes: u64,
    blake3: String,
}

fn identity(path: &Path) -> Result<Identity> {
    ensure!(
        path.symlink_metadata()?.file_type().is_file(),
        "not a regular file: {}",
        path.display()
    );
    let mut input = BufReader::new(File::open(path)?);
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0u64;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes = bytes
            .checked_add(count as u64)
            .context("file length overflow")?;
    }
    Ok(Identity {
        path: path.to_owned(),
        bytes,
        blake3: hash.finalize().to_hex().to_string(),
    })
}

fn identities(directory: &Path) -> Result<Vec<Identity>> {
    FILES
        .iter()
        .map(|name| identity(&directory.join(name)))
        .collect()
}

fn same_file(left: &Path, right: &Path) -> Result<bool> {
    let mut left = BufReader::new(File::open(left)?);
    let mut right = BufReader::new(File::open(right)?);
    let mut a = [0; 65536];
    let mut b = [0; 65536];
    // Fill equal-sized buffers, so independent short reads do not affect equality.
    loop {
        let mut count = 0;
        while count < a.len() {
            let read = left.read(&mut a[count..])?;
            if read == 0 {
                break;
            }
            count += read;
        }
        right.read_exact(&mut b[..count])?;
        if a[..count] != b[..count] {
            return Ok(false);
        }
        if count < a.len() {
            return Ok(right.read(&mut b[..1])? == 0);
        }
    }
}

fn bits_equal(left: f64, right: f64) -> bool {
    left.to_bits() == right.to_bits()
}

// The JSON reader's decimal-to-f64 conversion is also applied to the expected
// value. This is exact serialization equality, not a numerical tolerance.
fn json_float(value: f64) -> Result<f64> {
    Ok(serde_json::from_str(&serde_json::to_string(&value)?)?)
}

fn metric(line: &str) -> Result<MetricsRow> {
    let value: serde_json::Value = serde_json::from_str(line)?;
    let object = value
        .as_object()
        .context("progress row must be an object")?;
    let keys = [
        "iteration",
        "elapsed_secs",
        "expl_p0",
        "expl_p1",
        "nash_conv",
    ];
    ensure!(
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)),
        "unexpected progress fields"
    );
    Ok(serde_json::from_str(line)?)
}

fn state_shape(state: &StorageState) -> Result<(&'static str, usize, usize)> {
    match state {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => {
            ensure!(
                !regrets.is_empty() && regrets.len() == strategy_sum.len(),
                "invalid F32 storage lengths"
            );
            ensure!(
                regrets.iter().all(|v| v.is_finite())
                    && strategy_sum.iter().all(|v| v.is_finite() && *v >= 0.0),
                "invalid F32 state value"
            );
            Ok(("f32", regrets.len(), 0))
        }
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => {
            ensure!(
                !regrets.is_empty()
                    && regrets.len() == strategy_sum.len()
                    && !regret_scales.is_empty()
                    && regret_scales.len() == strategy_scales.len(),
                "invalid I16 storage lengths"
            );
            ensure!(
                strategy_sum.iter().all(|v| *v >= 0)
                    && regret_scales
                        .iter()
                        .chain(strategy_scales)
                        .all(|v| v.is_finite() && *v >= 0.0),
                "invalid I16 state value"
            );
            Ok(("i16", regrets.len(), regret_scales.len()))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunResult {
    kind: String,
    iterations: u64,
    wall_secs: f64,
    expl_p0: f64,
    expl_p1: f64,
    nash_conv: f64,
}

struct Run {
    directory: PathBuf,
    files: Vec<Identity>,
    config: String,
    manifest: RunManifest,
    checkpoint: Checkpoint,
    sol: SolPayload,
    progress_bytes: Vec<u8>,
    progress: Vec<MetricsRow>,
    events: Vec<formats::RunEvent>,
    target: u64,
    cadence: u64,
}

fn read_run(directory: PathBuf) -> Result<Run> {
    let files = identities(&directory)?;
    let config = std::fs::read_to_string(directory.join(formats::RUN_CONFIG_FILE))?;
    let parsed = cli::config::parse_solve_config(&config)?;
    ensure!(
        parsed.schema.as_deref() == Some("solvers.postflop/v1"),
        "only postflop/v1 is supported"
    );
    ensure!(
        matches!(&parsed.game, cli::config::GameSection::Postflop { tree, .. } if tree.source.is_none()),
        "config must be self-contained postflop"
    );
    ensure!(
        parsed.run.iterations > 0 && parsed.run.check_every > 0 && parsed.run.threads == Some(1),
        "positive iteration/cadence and explicit threads=1 required"
    );
    ensure!(
        parsed.run.max_time_secs.is_none() && parsed.run.target_nash_conv.is_none(),
        "time/quality early stops are outside this fixed-iteration audit"
    );
    let config_hash = formats::config_hash(config.as_bytes());
    let manifest = RunManifest::read(&directory)?;
    ensure!(
        manifest.schema_version == formats::RUN_MANIFEST_VERSION
            && manifest.game_kind == "postflop"
            && manifest.config_schema == parsed.schema
            && manifest.config_hash == formats::config_hash_hex(&config_hash),
        "manifest/config identity mismatch"
    );
    ensure!(
        manifest.failure.is_none()
            && manifest.started_unix_ms.is_some()
            && manifest.finished_unix_ms.is_some()
            && !manifest.cli_version.is_empty(),
        "manifest is incomplete or failed"
    );
    let checkpoint = formats::read_checkpoint(&directory.join(formats::RUN_HU_CHECKPOINT_FILE))?;
    let sol = formats::read_sol(&directory.join(formats::RUN_HU_SOLUTION_FILE))?;
    ensure!(
        checkpoint.config_hash == config_hash && sol.config_toml == config,
        "checkpoint/SOL config identity mismatch"
    );
    ensure!(
        checkpoint.iteration == checkpoint.state.iteration
            && checkpoint.iteration == sol.meta.iterations,
        "header/state/SOL iterations differ"
    );
    let storage = match parsed.run.storage {
        cli::config::StorageKind::F32 => "f32",
        cli::config::StorageKind::I16 => "i16",
    };
    ensure!(
        state_shape(&checkpoint.state.storage)?.0 == storage && sol.meta.storage == storage,
        "storage backend differs from config"
    );
    ensure!(
        sol.mode == formats::StreetsStored::Full && sol.node_count > 0 && !sol.blocks.is_empty(),
        "nonempty Full SOL required"
    );
    ensure!(
        sol.blocks.len() == sol.values.len()
            && sol.blocks.windows(2).all(|w| w[0].sref < w[1].sref),
        "invalid strategy key ordering"
    );
    for (strategy, value) in sol.blocks.iter().zip(&sol.values) {
        ensure!(
            strategy.sref == value.sref
                && !strategy.probs.is_empty()
                && strategy.probs.len().is_multiple_of(2)
                && !value.values.is_empty()
                && value.values.len().is_multiple_of(2)
                && value.scale.is_finite()
                && value.scale >= 0.0,
            "invalid saved block shape/key/scale"
        );
    }
    ensure!(
        sol.meta
            .ev
            .iter()
            .chain(&sol.meta.expl)
            .chain([&sol.meta.nash_conv, &sol.meta.wall_secs])
            .all(|v| v.is_finite())
            && sol.meta.wall_secs >= 0.0,
        "nonfinite live metadata or invalid wall time"
    );
    let result: RunResult =
        serde_json::from_slice(&std::fs::read(directory.join(formats::RUN_RESULT_FILE))?)?;
    ensure!(
        result.kind == "postflop" && result.iterations == checkpoint.iteration,
        "run result kind/iteration differs"
    );
    for (actual, expected) in [
        (result.expl_p0, sol.meta.expl[0]),
        (result.expl_p1, sol.meta.expl[1]),
        (result.nash_conv, sol.meta.nash_conv),
        (result.wall_secs, sol.meta.wall_secs),
    ] {
        ensure!(
            bits_equal(actual, json_float(expected)?),
            "run result and SOL live metadata differ"
        );
    }
    let progress_bytes = std::fs::read(directory.join(formats::RUN_PROGRESS_FILE))?;
    ensure!(
        progress_bytes.ends_with(b"\n"),
        "partial/empty progress log"
    );
    let progress: Vec<MetricsRow> = std::str::from_utf8(&progress_bytes)?
        .lines()
        .map(metric)
        .collect::<Result<_>>()?;
    ensure!(!progress.is_empty(), "empty progress log");
    let mut previous = 0u64;
    let mut elapsed = 0.0;
    for row in &progress {
        ensure!(
            previous < parsed.run.iterations,
            "progress continues past target"
        );
        previous += parsed.run.check_every.min(parsed.run.iterations - previous);
        ensure!(
            row.iteration == previous
                && row.elapsed_secs.is_finite()
                && row.elapsed_secs >= elapsed
                && [row.expl_p0, row.expl_p1, row.nash_conv]
                    .iter()
                    .all(|v| v.is_finite()),
            "invalid progress cadence/value"
        );
        elapsed = row.elapsed_secs;
    }
    let last = progress.last().context("empty progress")?;
    ensure!(
        last.iteration == checkpoint.iteration,
        "progress/checkpoint iteration differs"
    );
    for (actual, expected) in [
        (last.expl_p0, sol.meta.expl[0]),
        (last.expl_p1, sol.meta.expl[1]),
        (last.nash_conv, sol.meta.nash_conv),
    ] {
        ensure!(
            bits_equal(actual, json_float(expected)?),
            "progress/SOL live metadata differ"
        );
    }
    let events_path = directory.join(formats::RUN_EVENTS_FILE);
    let (events, consumed) = formats::read_events(&events_path, 0)?;
    ensure!(
        consumed == events_path.metadata()?.len() && !events.is_empty(),
        "partial/empty event log"
    );
    ensure!(
        events
            .iter()
            .enumerate()
            .all(|(i, event)| event.seq == i as u64 && event.level == formats::RunEventLevel::Info),
        "event sequence/level differs"
    );
    Ok(Run {
        directory,
        files,
        config,
        manifest,
        checkpoint,
        sol,
        progress_bytes,
        progress,
        events,
        target: parsed.run.iterations,
        cadence: parsed.run.check_every,
    })
}

fn lifecycle(run: &Run, start: u64, canceled: bool, command: &str) -> Result<()> {
    let end_state = if canceled {
        RunState::Canceled
    } else {
        RunState::Completed
    };
    let completion = if canceled { "cancelled" } else { "completed" };
    ensure!(
        run.manifest.state == end_state
            && run.manifest.completion.as_deref() == Some(completion)
            && run.manifest.command.first().map(String::as_str) == Some(command),
        "unexpected manifest outcome/command"
    );
    ensure!(
        match command {
            "solve" => run.manifest.command.len() == 4 && run.manifest.command[2] == "--out",
            "resume" => run.manifest.command.len() == 2,
            _ => false,
        },
        "unexpected recorded command shape"
    );
    let mut expected = vec![RunEventPayload::State {
        state: RunState::Running,
    }];
    // Progress cadence was checked on input; iterate the retained rows rather
    // than allocating from an unchecked iteration count in an artifact header.
    for row in run.progress.iter().filter(|row| row.iteration > start) {
        expected.push(RunEventPayload::Checkpoint {
            sweeps: row.iteration,
        });
    }
    if canceled {
        expected.push(RunEventPayload::Stop {
            reason: "cancelled".to_owned(),
        });
    }
    expected.push(RunEventPayload::State { state: end_state });
    ensure!(
        run.events.len() == expected.len()
            && run.events.iter().zip(expected).all(|(a, b)| a.payload == b),
        "event lifecycle/checkpoint cadence differs"
    );
    Ok(())
}

fn same_metrics(left: &MetricsRow, right: &MetricsRow) -> bool {
    left.iteration == right.iteration
        && bits_equal(left.expl_p0, right.expl_p0)
        && bits_equal(left.expl_p1, right.expl_p1)
        && bits_equal(left.nash_conv, right.nash_conv)
}

fn same_sol(left: &SolPayload, right: &SolPayload) -> bool {
    // Exhaustively destructure both payloads and metadata: a new persisted field
    // must receive an explicit comparison policy at compile time. Only wall_secs
    // is excluded; no strategy, value, precision or utility field is ignored.
    let SolPayload {
        config_toml: lc,
        meta: lm,
        mode: lmode,
        node_count: ln,
        blocks: lb,
        values: lv,
    } = left;
    let SolPayload {
        config_toml: rc,
        meta: rm,
        mode: rmode,
        node_count: rn,
        blocks: rb,
        values: rv,
    } = right;
    let formats::SolMeta {
        iterations: li,
        expl: lx,
        ev: le,
        nash_conv: lnc,
        storage: ls,
        wall_secs: _,
    } = lm;
    let formats::SolMeta {
        iterations: ri,
        expl: rx,
        ev: re,
        nash_conv: rnc,
        storage: rs,
        wall_secs: _,
    } = rm;
    lc == rc
        && lmode == rmode
        && ln == rn
        && lb == rb
        && li == ri
        && ls == rs
        && lx
            .iter()
            .chain(le)
            .zip(rx.iter().chain(re))
            .all(|(a, b)| bits_equal(*a, *b))
        && bits_equal(*lnc, *rnc)
        && lv.len() == rv.len()
        && lv.iter().zip(rv).all(|(a, b)| {
            let formats::ValueBlock {
                sref: ar,
                scale: ascale,
                values: av,
            } = a;
            let formats::ValueBlock {
                sref: br,
                scale: bscale,
                values: bv,
            } = b;
            ar == br && ascale.to_bits() == bscale.to_bits() && av == bv
        })
}

fn audit(args: Args) -> Result<serde_json::Value> {
    let dirs = [args.interrupted, args.resumed, args.straight].map(std::fs::canonicalize);
    let [interrupted_dir, resumed_dir, straight_dir] = dirs;
    let (interrupted_dir, resumed_dir, straight_dir) =
        (interrupted_dir?, resumed_dir?, straight_dir?);
    ensure!(
        interrupted_dir != resumed_dir
            && interrupted_dir != straight_dir
            && resumed_dir != straight_dir,
        "three distinct run directories required"
    );
    let interrupted = read_run(interrupted_dir).context("interrupted run")?;
    let resumed = read_run(resumed_dir).context("resumed run")?;
    let straight = read_run(straight_dir).context("straight run")?;
    ensure!(
        interrupted.config == resumed.config && resumed.config == straight.config,
        "run configs differ; no restamping or target changes allowed"
    );
    ensure!(
        interrupted.manifest.cli_version == resumed.manifest.cli_version
            && resumed.manifest.cli_version == straight.manifest.cli_version,
        "CLI versions differ"
    );
    let m = interrupted.checkpoint.iteration;
    let n = interrupted.target;
    ensure!(
        m > 0 && m < n && m.is_multiple_of(interrupted.cadence),
        "interruption is not a completed nonfinal checkpoint"
    );
    ensure!(
        resumed.checkpoint.iteration == n && straight.checkpoint.iteration == n,
        "final runs did not reach original target"
    );
    lifecycle(&interrupted, 0, true, "solve")?;
    lifecycle(&resumed, m, false, "resume")?;
    lifecycle(&straight, 0, false, "solve")?;
    ensure!(
        resumed
            .progress_bytes
            .starts_with(&interrupted.progress_bytes)
            && resumed.progress.len() > interrupted.progress.len(),
        "resumed fork lost its exact progress prefix or performed zero steps"
    );
    ensure!(
        resumed.progress[interrupted.progress.len()].iteration > m,
        "resumed segment did not advance"
    );
    ensure!(
        resumed.progress.len() == straight.progress.len()
            && resumed
                .progress
                .iter()
                .zip(&straight.progress)
                .all(|(a, b)| same_metrics(a, b)),
        "straight/resumed live progress differs"
    );
    ensure!(
        state_shape(&interrupted.checkpoint.state.storage)?
            == state_shape(&resumed.checkpoint.state.storage)?
            && state_shape(&resumed.checkpoint.state.storage)?
                == state_shape(&straight.checkpoint.state.storage)?,
        "checkpoint storage layouts differ"
    );
    ensure!(
        same_file(
            &resumed.directory.join(formats::RUN_HU_CHECKPOINT_FILE),
            &straight.directory.join(formats::RUN_HU_CHECKPOINT_FILE)
        )?,
        "final checkpoint bytes differ"
    );
    // Raw equality also checks floating-point bits, including signed zero;
    // PartialEq alone would not provide that guarantee.
    ensure!(
        resumed.checkpoint == straight.checkpoint,
        "decoded checkpoint state differs"
    );
    ensure!(
        same_sol(&resumed.sol, &straight.sol),
        "final SOL profile/value/live metadata differ"
    );
    for run in [&interrupted, &resumed, &straight] {
        ensure!(
            identities(&run.directory)? == run.files,
            "input changed during audit: {}",
            run.directory.display()
        );
    }
    Ok(json!({
        "schema": "r1.hu-checkpoint-audit/v1",
        "status": "pass",
        "interrupted_iteration": m,
        "target_iteration": n,
        "resumed_iterations": n - m,
        "config_blake3": formats::config_hash_hex(&interrupted.checkpoint.config_hash),
        "source_storage": resumed.sol.meta.storage,
        "runs": {
            "interrupted": {"directory": interrupted.directory, "files": interrupted.files},
            "resumed": {"directory": resumed.directory, "files": resumed.files},
            "straight": {"directory": straight.directory, "files": straight.files}
        },
        "checks": {
            "input_identities_unchanged": true, "config_bytes_exact": true,
            "nonfinal_canceled_checkpoint": true, "resumed_progress_prefix_exact": true,
            "all_progress_numeric_fields_equal": true, "final_checkpoint_raw_bytes_equal": true,
            "final_checkpoint_full_decoded_state_equal": true,
            "final_full_sol_except_wall_equal": true
        },
        "live_profile_metadata": {
            "ev": resumed.sol.meta.ev, "gains": resumed.sol.meta.expl,
            "nash_conv": resumed.sol.meta.nash_conv
        },
        "comparison_exclusions": [
            "SolMeta.wall_secs (validated finite/nonnegative and against its own run.json)",
            "progress.elapsed_secs (validated finite/nonnegative/monotonic; copied prefix remains exact)",
            "manifest run_id, pid, command paths, created/started/finished times",
            "event unix_ms"
        ],
        "scope_limits": [
            "External controller must bind source/binary/config, signal delivery, process exit codes and resume input lineage.",
            "The two final Full artifacts still require separate hu_saved_profile_audit for compiled-tree validation and stored-quantized EV/BR evaluation.",
            "No training or artifact rewriting is performed. No external solution quality or performance conclusion."
        ],
        "saved_profile_quality": "not_evaluated",
        "r1_acceptance": null
    }))
}

fn main() -> Result<()> {
    let report = audit(Args::parse())?;
    let mut output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &report)?;
    writeln!(output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> SolPayload {
        SolPayload {
            config_toml: "fixed config".to_owned(),
            meta: formats::SolMeta {
                iterations: 10,
                expl: [0.1, 0.2],
                ev: [1.0, 2.0],
                nash_conv: 0.3,
                storage: "f32".to_owned(),
                wall_secs: 1.0,
            },
            mode: formats::StreetsStored::Full,
            node_count: 3,
            blocks: vec![formats::StrategyBlock {
                sref: 0,
                probs: vec![255, 255],
            }],
            values: vec![formats::ValueBlock {
                sref: 0,
                scale: 0.0,
                values: vec![0, 0],
            }],
        }
    }

    #[test]
    fn only_wall_is_excluded_from_sol_comparison() {
        let original = payload();
        let mut other = original.clone();
        other.meta.wall_secs = 9.0;
        assert!(same_sol(&original, &other));
        other.blocks[0].probs[0] ^= 1;
        assert!(!same_sol(&original, &other));
        other = original.clone();
        other.values[0].scale = -0.0;
        assert!(!same_sol(&original, &other));
        other = original.clone();
        other.meta.ev[0] = f64::from_bits(original.meta.ev[0].to_bits() + 1);
        assert!(!same_sol(&original, &other));
        other = original.clone();
        other.config_toml.push('\n');
        assert!(!same_sol(&original, &other));
    }

    #[test]
    fn storage_rejects_nonfinite_and_negative_strategy_state() {
        let invalid = |regret, strategy| StorageState::F32 {
            regrets: vec![regret],
            strategy_sum: vec![strategy],
        };
        assert!(state_shape(&invalid(f32::NAN, 1.0)).is_err());
        assert!(state_shape(&invalid(0.0, -1.0)).is_err());
        assert!(state_shape(&invalid(-1.0, 1.0)).is_ok());
    }
}
