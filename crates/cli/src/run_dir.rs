//! Run-directory lifecycle: create it, record what happens in it, close it.
//!
//! The solver itself knows nothing about run directories. This module owns
//! the contract described in `docs/app-architecture.md` §5, so `solve` and
//! `resume` record identical state, and so a watcher can attach to either.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use formats::{
    RUN_CHECKPOINT_FILE, RUN_CONFIG_FILE, RUN_HU_CHECKPOINT_FILE, RUN_HU_SOLUTION_FILE,
    RUN_MANIFEST_FILE, RUN_PROGRESS_FILE, RUN_RESULT_FILE, RUN_SOLUTION_FILE, RUN_STRATEGY_FILE,
    RunEventLevel, RunEventLog, RunEventPayload, RunManifest, RunState,
};

use crate::multiway_solve::MultiwayRunObservation;

/// The artifact paths inside one run directory.
///
/// The layout is the same for every game; only the two engine-specific
/// artifacts differ, because the heads-up engine and the multiway engine
/// write different checkpoint and solution formats.
pub struct RunPaths {
    pub directory: PathBuf,
    pub result: PathBuf,
    pub progress: PathBuf,
    pub checkpoint: PathBuf,
    pub solution: PathBuf,
    /// Average strategy for the requested betting lines. Toy games and the
    /// heads-up preflop family only: postflop publishes through
    /// `solution.sol` and multiway through `solution.mwsol`, both of which
    /// `export` reads.
    pub strategy: PathBuf,
}

impl RunPaths {
    /// Layout for the sampled multiway engine (`.mwckpt` / `.mwsol`).
    pub fn multiway(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            result: directory.join(RUN_RESULT_FILE),
            progress: directory.join(RUN_PROGRESS_FILE),
            checkpoint: directory.join(RUN_CHECKPOINT_FILE),
            solution: directory.join(RUN_SOLUTION_FILE),
            strategy: directory.join(RUN_STRATEGY_FILE),
        }
    }

    /// Layout for the exact heads-up engine (`.ckpt` / `.sol`).
    pub fn heads_up(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            result: directory.join(RUN_RESULT_FILE),
            progress: directory.join(RUN_PROGRESS_FILE),
            checkpoint: directory.join(RUN_HU_CHECKPOINT_FILE),
            solution: directory.join(RUN_HU_SOLUTION_FILE),
            strategy: directory.join(RUN_STRATEGY_FILE),
        }
    }
}

/// Prepares `directory` to receive a run: absent, empty, or queued.
///
/// A run directory is the run's identity, so a populated one is refused --
/// two runs writing into one directory would interleave their events and
/// artifacts. The one exception is a directory a scheduler prepared: a
/// `queued` manifest and the config to run, and nothing else. Queued runs
/// have to exist on disk, because that is the only place the daemon keeps
/// state, and a daemon restart has to find them again.
pub fn create_or_adopt(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return std::fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", directory.display()));
    }
    if !directory.is_dir() {
        anyhow::bail!("run output {} is not a directory", directory.display());
    }
    let entries: Vec<String> = std::fs::read_dir(directory)
        .with_context(|| format!("reading {}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    if entries.is_empty() {
        return Ok(());
    }
    // `stdout.log` is the scheduler's own capture of this process, opened
    // before the run starts.
    let prepared = [RUN_MANIFEST_FILE, RUN_CONFIG_FILE, "stdout.log"];
    let only_prepared = entries.iter().all(|name| prepared.contains(&name.as_str()));
    let queued = RunManifest::read(directory)
        .map(|manifest| manifest.state == RunState::Queued)
        .unwrap_or(false);
    if only_prepared && queued {
        return Ok(());
    }
    anyhow::bail!(
        "run output directory {} is not empty, and is not a queued run to adopt",
        directory.display()
    )
}

/// Owns a run's `manifest.json` and `events.jsonl` for the duration of a
/// solve, and closes both out with a terminal state.
pub struct RunRecorder {
    directory: PathBuf,
    manifest: RunManifest,
    events: RunEventLog,
}

impl RunRecorder {
    /// Opens a run directory for recording, writing `run.toml`, the initial
    /// `Running` manifest, and the opening event.
    pub fn start(
        directory: &Path,
        game_kind: &str,
        config_schema: Option<String>,
        config_hash: [u8; 32],
        effective_config: &str,
        command: Vec<String>,
    ) -> Result<Self> {
        std::fs::write(directory.join(RUN_CONFIG_FILE), effective_config)
            .with_context(|| format!("writing {RUN_CONFIG_FILE} in {}", directory.display()))?;
        let run_id = directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| directory.display().to_string());
        let mut manifest =
            RunManifest::new(run_id, game_kind, config_schema, hex(&config_hash), command);
        // Adopting a queued run keeps the moment it was accepted, so a
        // client sees the wait it actually had rather than the wait after
        // a slot opened.
        if let Ok(queued) = RunManifest::read(directory)
            && queued.state == RunState::Queued
        {
            manifest.created_unix_ms = queued.created_unix_ms;
        }
        manifest
            .write_atomic(directory)
            .with_context(|| format!("writing the run manifest in {}", directory.display()))?;
        let mut events = RunEventLog::create_or_append(directory)
            .with_context(|| format!("opening the run event log in {}", directory.display()))?;
        events.state(RunState::Running)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            manifest,
            events,
        })
    }

    /// Reopens an existing run directory for another segment.
    ///
    /// A resumed run keeps its identity: the same `run_id`, the same
    /// `createdUnixMs`, and the same event sequence, with this process
    /// recorded as the new owner. If the directory has no manifest (a run
    /// directory produced before manifests existed), one is started fresh.
    pub fn reopen(
        directory: &Path,
        game_kind: &str,
        config_schema: Option<String>,
        config_hash: [u8; 32],
        effective_config: &str,
        command: Vec<String>,
    ) -> Result<Self> {
        let Ok(previous) = RunManifest::read(directory) else {
            return Self::start(
                directory,
                game_kind,
                config_schema,
                config_hash,
                effective_config,
                command,
            );
        };
        let mut manifest = RunManifest::new(
            previous.run_id,
            game_kind,
            config_schema,
            hex(&config_hash),
            command,
        );
        manifest.created_unix_ms = previous.created_unix_ms;
        manifest
            .write_atomic(directory)
            .with_context(|| format!("writing the run manifest in {}", directory.display()))?;
        let mut events = RunEventLog::create_or_append(directory)
            .with_context(|| format!("opening the run event log in {}", directory.display()))?;
        events.state(RunState::Running)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            manifest,
            events,
        })
    }

    /// The run's single event writer.
    ///
    /// Everything that appends to `events.jsonl` must go through this one
    /// handle: two writers would each keep their own `seq` counter and
    /// produce duplicate sequence numbers, which is exactly what a reader
    /// uses to detect a gap.
    pub fn events_mut(&mut self) -> &mut RunEventLog {
        &mut self.events
    }

    /// Turns solver observations into run events. Live and quality samples
    /// are already in `progress.jsonl`, so only the discrete ones land here.
    pub fn observe(&mut self, observation: &MultiwayRunObservation) {
        let payload = match observation {
            MultiwayRunObservation::Abstraction(abstraction) => Some(RunEventPayload::Notice {
                message: format!(
                    "ehs2 tables {} in {:.2}s",
                    if abstraction.cached {
                        "loaded"
                    } else {
                        "built"
                    },
                    abstraction.secs
                ),
            }),
            MultiwayRunObservation::Checkpoint(checkpoint) => Some(RunEventPayload::Checkpoint {
                sweeps: checkpoint.sweeps,
            }),
            MultiwayRunObservation::Stop(stop) => Some(RunEventPayload::Stop {
                reason: stop.reason.clone(),
            }),
            MultiwayRunObservation::Live(_) | MultiwayRunObservation::Quality(_) => None,
        };
        if let Some(payload) = payload {
            // A failed event write must not abort a solve that is otherwise
            // fine; the run is still recoverable from its artifacts.
            let _ = self.events.info(payload);
        }
    }

    pub fn notice(&mut self, message: impl Into<String>) {
        let _ = self.events.append(
            RunEventLevel::Warn,
            RunEventPayload::Notice {
                message: message.into(),
            },
        );
    }

    /// Records the outcome of the solve and closes the manifest.
    ///
    /// `completion` is the solver's terminal status (`target-reached`,
    /// `cancelled`, ...). A `cancelled` status is not a failure: the run
    /// stopped on request with a checkpoint written, so it closes as
    /// `Canceled` and stays resumable.
    pub fn finish(mut self, outcome: Result<()>, completion: Option<String>) -> Result<()> {
        match outcome {
            Ok(()) => {
                let state = match completion.as_deref() {
                    Some("cancelled") => RunState::Canceled,
                    _ => RunState::Completed,
                };
                self.manifest.finish(state, completion);
                let _ = self.events.state(state);
            }
            Err(ref error) => {
                let message = format!("{error:#}");
                self.manifest.fail(message.clone());
                let _ = self
                    .events
                    .append(RunEventLevel::Error, RunEventPayload::Failure { message });
                let _ = self.events.state(RunState::Failed);
            }
        }
        self.manifest
            .write_atomic(&self.directory)
            .with_context(|| format!("closing the run manifest in {}", self.directory.display()))?;
        outcome
    }
}

/// Reads the terminal status the solver recorded in `run.json`.
pub fn completion_status(directory: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(directory.join(RUN_RESULT_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("status")?
        .as_str()
        .map(|status| status.to_string())
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_populated_directory_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().join("run");
        std::fs::create_dir(&run).unwrap();
        std::fs::write(run.join("stray.txt"), "x").unwrap();
        let error = create_or_adopt(&run).unwrap_err().to_string();
        assert!(error.contains("not empty"), "{error}");
    }

    /// A scheduler writes the manifest and config before a slot frees up,
    /// so the run is visible while it waits. The solve then adopts that
    /// directory instead of refusing it.
    #[test]
    fn a_queued_run_directory_is_adopted_and_keeps_its_identity() {
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().join("queued-run");
        std::fs::create_dir(&run).unwrap();
        let mut prepared = RunManifest::new(
            "queued-run",
            "pending",
            None,
            "pending",
            vec!["solve".into()],
        );
        prepared.state = RunState::Queued;
        prepared.created_unix_ms = 1_700_000_000_000;
        prepared.write_atomic(&run).unwrap();
        std::fs::write(run.join(RUN_CONFIG_FILE), "schema = \"x\"\n").unwrap();

        create_or_adopt(&run).expect("a queued directory must be adoptable");
        let recorder = RunRecorder::start(
            &run,
            "kuhn",
            Some("solvers.toy/v1".into()),
            [1; 32],
            "schema = \"solvers.toy/v1\"\n",
            vec!["solve".into()],
        )
        .unwrap();
        recorder.finish(Ok(()), Some("completed".into())).unwrap();

        let manifest = RunManifest::read(&run).unwrap();
        assert_eq!(manifest.state, RunState::Completed);
        assert_eq!(manifest.game_kind, "kuhn");
        assert_eq!(
            manifest.created_unix_ms, 1_700_000_000_000,
            "the queued moment must survive adoption"
        );
    }

    /// A directory holding a finished run is not a queued one, and must not
    /// be silently reused.
    #[test]
    fn a_finished_run_directory_is_not_adopted() {
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().join("done");
        std::fs::create_dir(&run).unwrap();
        let mut manifest = RunManifest::new("done", "kuhn", None, "aa", vec!["solve".into()]);
        manifest.finish(RunState::Completed, None);
        manifest.write_atomic(&run).unwrap();
        assert!(create_or_adopt(&run).is_err());
    }

    #[test]
    fn a_completed_run_closes_its_manifest_and_events() {
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().join("run");
        create_or_adopt(&run).unwrap();
        let recorder = RunRecorder::start(
            &run,
            "preflop-multiway",
            Some("solvers.multiway-preflop/v1".into()),
            [7; 32],
            "schema = \"solvers.multiway-preflop/v1\"\n",
            vec!["solve".into()],
        )
        .unwrap();
        recorder
            .finish(Ok(()), Some("target-reached".into()))
            .unwrap();

        let manifest = RunManifest::read(&run).unwrap();
        assert_eq!(manifest.state, RunState::Completed);
        assert_eq!(manifest.completion.as_deref(), Some("target-reached"));
        assert_eq!(manifest.config_hash, "07".repeat(32));
        assert_eq!(manifest.run_id, "run");
        assert!(run.join(RUN_CONFIG_FILE).is_file());

        let (events, _) = formats::read_events(&RunEventLog::path_in(&run), 0).unwrap();
        assert_eq!(
            events.first().unwrap().payload,
            RunEventPayload::State {
                state: RunState::Running
            }
        );
        assert_eq!(
            events.last().unwrap().payload,
            RunEventPayload::State {
                state: RunState::Completed
            }
        );
    }

    /// Cancelling is a resumable stop, not a failure.
    #[test]
    fn a_cancelled_run_closes_as_canceled() {
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().join("run");
        create_or_adopt(&run).unwrap();
        let recorder =
            RunRecorder::start(&run, "preflop-multiway", None, [0; 32], "", Vec::new()).unwrap();
        recorder.finish(Ok(()), Some("cancelled".into())).unwrap();
        assert_eq!(RunManifest::read(&run).unwrap().state, RunState::Canceled);
    }

    #[test]
    fn a_failed_run_records_the_error_and_returns_it() {
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().join("run");
        create_or_adopt(&run).unwrap();
        let recorder =
            RunRecorder::start(&run, "preflop-multiway", None, [0; 32], "", Vec::new()).unwrap();
        let error = recorder
            .finish(Err(anyhow::anyhow!("policy arena allocation failed")), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("policy arena"), "{error}");

        let manifest = RunManifest::read(&run).unwrap();
        assert_eq!(manifest.state, RunState::Failed);
        assert!(
            manifest
                .failure
                .as_deref()
                .is_some_and(|failure| failure.contains("policy arena"))
        );
        let (events, _) = formats::read_events(&RunEventLog::path_in(&run), 0).unwrap();
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            RunEventPayload::Failure { message } if message.contains("policy arena")
        )));
    }
}
