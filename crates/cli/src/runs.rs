//! Reading run directories: `status`, `watch`, and `runs ls`.
//!
//! Nothing here talks to the solver. A run directory is the whole interface,
//! which is what lets these commands report on a run this process did not
//! start -- including one that is still going. The future job daemon serves
//! the same three views over HTTP by reading the same files.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::ValueEnum;
use formats::{
    RunEvent, RunEventLog, RunManifest, RunState, is_run_directory, last_progress_row, read_events,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ReportFormat {
    #[default]
    Human,
    Json,
}

/// What a run directory currently says about itself.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunStatus {
    run_id: String,
    /// The manifest's state resolved against the owning process: a manifest
    /// still claiming `running` with a dead pid reports `interrupted`.
    state: String,
    /// The state as literally recorded, when it differs from `state`.
    #[serde(skip_serializing_if = "Option::is_none")]
    recorded_state: Option<String>,
    game_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_schema: Option<String>,
    config_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sweeps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_secs: Option<f64>,
    /// Byte offset a watcher should resume the event log from.
    events_offset: u64,
    resumable: bool,
}

fn read_status(directory: &Path) -> Result<RunStatus> {
    let manifest = RunManifest::read(directory)
        .with_context(|| format!("reading the run manifest in {}", directory.display()))?;
    let observed = manifest.observed_state();
    let progress = last_progress_row(directory)
        .with_context(|| format!("reading progress in {}", directory.display()))?;
    let events_offset = std::fs::metadata(RunEventLog::path_in(directory))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let checkpoint = directory.join(formats::RUN_CHECKPOINT_FILE).is_file();

    Ok(RunStatus {
        run_id: manifest.run_id.clone(),
        state: observed.as_str().to_string(),
        recorded_state: (observed != manifest.state).then(|| manifest.state.as_str().to_string()),
        game_kind: manifest.game_kind.clone(),
        config_schema: manifest.config_schema.clone(),
        config_hash: manifest.config_hash.clone(),
        completion: manifest.completion.clone(),
        failure: manifest.failure.clone(),
        // Multiway rows count sweeps; the heads-up engine counts
        // iterations. Both mean "how far has this run got".
        sweeps: progress
            .as_ref()
            .and_then(|row| row.get("sweeps").or_else(|| row.get("iteration"))?.as_u64()),
        elapsed_secs: progress
            .as_ref()
            .and_then(|row| row.get("elapsedSecs")?.as_f64()),
        events_offset,
        // Only a stopped run is worth resuming, and only if it left a
        // checkpoint behind. A completed run is not: it already reached its
        // target.
        resumable: checkpoint
            && matches!(
                observed,
                RunState::Canceled | RunState::Interrupted | RunState::Failed
            ),
    })
}

pub fn status(directory: &Path, format: ReportFormat) -> Result<()> {
    let status = read_status(directory)?;
    match format {
        ReportFormat::Json => println!("{}", serde_json::to_string_pretty(&status)?),
        ReportFormat::Human => {
            print!("{} {}", status.run_id, status.state);
            if let Some(recorded) = &status.recorded_state {
                print!(" (manifest says {recorded}; its process is gone)");
            }
            if let Some(sweeps) = status.sweeps {
                print!(" sweeps={sweeps}");
            }
            if let Some(elapsed) = status.elapsed_secs {
                print!(" elapsed={elapsed:.1}s");
            }
            if let Some(completion) = &status.completion {
                print!(" completion={completion}");
            }
            if status.resumable {
                print!(" resumable");
            }
            println!();
            if let Some(failure) = &status.failure {
                println!("failure: {failure}");
            }
        }
    }
    Ok(())
}

/// Follows a run's event log until the run stops.
///
/// `from` is a byte offset into `events.jsonl`, so a client that was
/// disconnected resumes exactly where it left off. The final line reports
/// the offset to resume from next time.
pub fn watch(
    directory: &Path,
    from: u64,
    poll: Duration,
    format: ReportFormat,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    if !is_run_directory(directory) {
        anyhow::bail!(
            "{} is not a run directory (no {})",
            directory.display(),
            formats::RUN_MANIFEST_FILE
        );
    }
    let events_path = RunEventLog::path_in(directory);
    let mut offset = from;
    loop {
        let (events, next) = match read_events(&events_path, offset) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), offset),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", events_path.display()));
            }
        };
        offset = next;
        for event in &events {
            print_event(event, format)?;
        }

        let manifest = RunManifest::read(directory)
            .with_context(|| format!("reading the run manifest in {}", directory.display()))?;
        if manifest.observed_state().is_terminal() {
            // One last read: the owner may have appended its closing events
            // between the read above and the manifest rewrite.
            let (tail, next) = match read_events(&events_path, offset) {
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), offset),
                Err(error) => return Err(error.into()),
            };
            offset = next;
            for event in &tail {
                print_event(event, format)?;
            }
            if matches!(format, ReportFormat::Human) {
                println!(
                    "run {} is {}; resume watching from offset {offset}",
                    manifest.run_id,
                    manifest.observed_state().as_str()
                );
            }
            return Ok(());
        }
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            if matches!(format, ReportFormat::Human) {
                println!("stopped watching; resume from offset {offset}");
            }
            return Ok(());
        }
        std::thread::sleep(poll);
    }
}

fn print_event(event: &RunEvent, format: ReportFormat) -> Result<()> {
    match format {
        ReportFormat::Json => println!("{}", serde_json::to_string(event)?),
        ReportFormat::Human => println!(
            "[{:>4}] {}",
            event.seq,
            serde_json::to_string(&event.payload)?
        ),
    }
    std::io::stdout().flush()?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunListing {
    runs: Vec<RunStatus>,
}

/// Lists the run directories directly under `root`.
///
/// Only one level down: run directories are not nested, and recursing would
/// make an accidental `runs ls /` walk a whole filesystem.
pub fn list(root: &Path, format: ReportFormat) -> Result<()> {
    let mut directories: Vec<_> = std::fs::read_dir(root)
        .with_context(|| format!("reading {}", root.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_run_directory(path))
        .collect();
    directories.sort();

    let mut runs = Vec::new();
    for directory in &directories {
        runs.push(read_status(directory)?);
    }
    match format {
        ReportFormat::Json => println!("{}", serde_json::to_string_pretty(&RunListing { runs })?),
        ReportFormat::Human => {
            if runs.is_empty() {
                println!("no run directories under {}", root.display());
            }
            for run in &runs {
                println!(
                    "{:<28} {:<12} sweeps={:<10} {}",
                    run.run_id,
                    run.state,
                    run.sweeps
                        .map_or_else(|| "-".to_string(), |sweeps| sweeps.to_string()),
                    run.completion.as_deref().unwrap_or("")
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use formats::{RunEventPayload, RunState};

    fn finished_run(directory: &Path, state: RunState, checkpoint: bool) {
        std::fs::create_dir_all(directory).unwrap();
        let mut manifest = RunManifest::new(
            directory.file_name().unwrap().to_string_lossy().to_string(),
            "preflop-multiway",
            Some("solvers.multiway-preflop/v1".into()),
            "aa",
            vec!["solve".into()],
        );
        manifest.finish(state, Some("target-reached".into()));
        manifest.write_atomic(directory).unwrap();
        let mut events = RunEventLog::create_or_append(directory).unwrap();
        events.state(RunState::Running).unwrap();
        events.state(state).unwrap();
        if checkpoint {
            std::fs::write(directory.join(formats::RUN_CHECKPOINT_FILE), b"x").unwrap();
        }
        std::fs::write(
            directory.join(formats::RUN_PROGRESS_FILE),
            "{\"sweeps\":42,\"elapsedSecs\":1.5}\n",
        )
        .unwrap();
    }

    #[test]
    fn status_reports_progress_and_an_events_offset() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("run-a");
        finished_run(&run, RunState::Completed, false);

        let status = read_status(&run).unwrap();
        assert_eq!(status.state, "completed");
        assert_eq!(status.sweeps, Some(42));
        assert_eq!(status.elapsed_secs, Some(1.5));
        assert!(status.events_offset > 0);
        assert!(!status.resumable);
        assert!(status.recorded_state.is_none());
    }

    /// A stopped run with a checkpoint is the case `resume` exists for.
    #[test]
    fn a_canceled_run_with_a_checkpoint_is_resumable() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("run-b");
        finished_run(&run, RunState::Canceled, true);
        assert!(read_status(&run).unwrap().resumable);
    }

    /// A manifest left claiming `running` by a dead process must not be
    /// reported as live, and must say what it literally recorded.
    #[cfg(unix)]
    #[test]
    fn an_abandoned_run_reports_interrupted_and_keeps_the_recorded_state() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("run-c");
        std::fs::create_dir_all(&run).unwrap();
        let mut manifest = RunManifest::new(
            "run-c",
            "preflop-multiway",
            None,
            "aa",
            vec!["solve".into()],
        );
        manifest.pid = 0;
        manifest.write_atomic(&run).unwrap();
        std::fs::write(run.join(formats::RUN_CHECKPOINT_FILE), b"x").unwrap();

        let status = read_status(&run).unwrap();
        assert_eq!(status.state, "interrupted");
        assert_eq!(status.recorded_state.as_deref(), Some("running"));
        assert!(status.resumable);
    }

    #[test]
    fn listing_finds_run_directories_and_ignores_other_entries() {
        let root = tempfile::tempdir().unwrap();
        finished_run(&root.path().join("run-a"), RunState::Completed, false);
        finished_run(&root.path().join("run-b"), RunState::Failed, false);
        std::fs::create_dir_all(root.path().join("not-a-run")).unwrap();
        std::fs::write(root.path().join("stray.txt"), "x").unwrap();

        let found: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_run_directory(path))
            .collect();
        assert_eq!(found.len(), 2);
        list(root.path(), ReportFormat::Json).unwrap();
    }

    /// Watching a run that has already stopped must drain its events and
    /// return rather than poll forever.
    #[test]
    fn watching_a_finished_run_returns_immediately() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("run-d");
        finished_run(&run, RunState::Completed, false);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        watch(
            &run,
            0,
            Duration::from_millis(10),
            ReportFormat::Json,
            &cancel,
        )
        .unwrap();
    }

    /// A watcher that resumes from a saved offset sees only what is new.
    #[test]
    fn watching_from_an_offset_skips_what_was_already_read() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("run-e");
        finished_run(&run, RunState::Completed, false);
        let path = RunEventLog::path_in(&run);

        let (first, offset) = read_events(&path, 0).unwrap();
        assert_eq!(first.len(), 2);
        let mut events = RunEventLog::create_or_append(&run).unwrap();
        events
            .info(RunEventPayload::Notice {
                message: "late".into(),
            })
            .unwrap();

        let (tail, _) = read_events(&path, offset).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].seq, 2);
    }
}
