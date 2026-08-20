//! The run directory: the only durable record of a solve.
//!
//! A run directory is self-describing. `manifest.json` says what the run is
//! and what state it is in; `progress.jsonl` carries periodic metric
//! samples; `events.jsonl` carries the discrete lifecycle events; the
//! remaining files are the config it ran and the artifacts it produced.
//!
//! Two rules make a run watchable by a process that did not start it, and
//! that can attach long after it began:
//!
//! * `manifest.json` is only ever replaced atomically (write a sibling
//!   temporary file, then rename), so a reader never observes half a state
//!   transition.
//! * `events.jsonl` is append-only with a monotonic `seq`. A reader keeps a
//!   byte offset, resumes from it, and uses `seq` continuity to detect a
//!   gap. Nothing rewrites an existing line.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const RUN_MANIFEST_VERSION: u16 = 1;

/// Files that make up a run directory. Every producer and reader resolves
/// names through these constants rather than spelling them out.
pub const RUN_MANIFEST_FILE: &str = "manifest.json";
pub const RUN_EVENTS_FILE: &str = "events.jsonl";
pub const RUN_PROGRESS_FILE: &str = "progress.jsonl";
pub const RUN_RESULT_FILE: &str = "run.json";
pub const RUN_CONFIG_FILE: &str = "run.toml";
pub const RUN_CHECKPOINT_FILE: &str = "checkpoint.mwckpt";
pub const RUN_SOLUTION_FILE: &str = "solution.mwsol";

/// Where a run is in its lifecycle.
///
/// `Interrupted` is not written by the process that owns the run -- a killed
/// process writes nothing. It is what a reader concludes when the manifest
/// still says `Running` but the recorded pid is gone; see
/// [`RunManifest::is_stale`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunState {
    /// Accepted but not started. Only a scheduler produces this; a direct
    /// `solvers solve` goes straight to `Running`.
    Queued,
    Running,
    Completed,
    Failed,
    /// Stopped on request, with a checkpoint written first.
    Canceled,
    /// The owning process died without recording an outcome.
    Interrupted,
}

impl RunState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunState::Completed | RunState::Failed | RunState::Canceled | RunState::Interrupted
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Queued => "queued",
            RunState::Running => "running",
            RunState::Completed => "completed",
            RunState::Failed => "failed",
            RunState::Canceled => "canceled",
            RunState::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunManifest {
    pub schema_version: u16,
    /// The run directory's own name. Kept in the file too so a manifest
    /// stays self-describing after being copied somewhere else.
    pub run_id: String,
    pub state: RunState,
    pub game_kind: String,
    /// The config's `schema` value, or `None` for a config family that does
    /// not declare one yet.
    pub config_schema: Option<String>,
    /// Hex blake3 of the raw config bytes, matching the checkpoint stamp.
    pub config_hash: String,
    pub cli_version: String,
    /// Argv tail that produced the run, for reproducing it by hand.
    pub command: Vec<String>,
    pub pid: u32,
    pub created_unix_ms: u64,
    pub started_unix_ms: Option<u64>,
    pub finished_unix_ms: Option<u64>,
    /// Set when `state` is `Failed`.
    pub failure: Option<String>,
    /// Terminal reason reported by the solver, e.g. `target-reached`.
    pub completion: Option<String>,
}

impl RunManifest {
    pub fn new(
        run_id: impl Into<String>,
        game_kind: impl Into<String>,
        config_schema: Option<String>,
        config_hash: impl Into<String>,
        command: Vec<String>,
    ) -> Self {
        let now = unix_millis();
        Self {
            schema_version: RUN_MANIFEST_VERSION,
            run_id: run_id.into(),
            state: RunState::Running,
            game_kind: game_kind.into(),
            config_schema,
            config_hash: config_hash.into(),
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            command,
            pid: std::process::id(),
            created_unix_ms: now,
            started_unix_ms: Some(now),
            finished_unix_ms: None,
            failure: None,
            completion: None,
        }
    }

    pub fn path_in(directory: &Path) -> PathBuf {
        directory.join(RUN_MANIFEST_FILE)
    }

    pub fn read(directory: &Path) -> io::Result<Self> {
        let raw = std::fs::read_to_string(Self::path_in(directory))?;
        serde_json::from_str(&raw).map_err(io::Error::other)
    }

    /// Writes the manifest through a temporary sibling and renames it into
    /// place, so a concurrent reader sees either the old or the new file and
    /// never a partial one.
    pub fn write_atomic(&self, directory: &Path) -> io::Result<()> {
        let final_path = Self::path_in(directory);
        let temporary = directory.join(format!(".{RUN_MANIFEST_FILE}.tmp"));
        let mut json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        json.push('\n');
        {
            let mut file = File::create(&temporary)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, &final_path)
    }

    pub fn finish(&mut self, state: RunState, completion: Option<String>) {
        self.state = state;
        self.completion = completion;
        self.finished_unix_ms = Some(unix_millis());
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.state = RunState::Failed;
        self.failure = Some(message.into());
        self.finished_unix_ms = Some(unix_millis());
    }

    /// True when the manifest claims the run is live but its process is not.
    ///
    /// Only meaningful on the host that produced the run: a pid from another
    /// machine says nothing here, so callers that read run directories over
    /// a network must not use this.
    pub fn is_stale(&self) -> bool {
        self.state == RunState::Running && !process_is_alive(self.pid)
    }

    /// [`Self::state`], resolved through [`Self::is_stale`].
    pub fn observed_state(&self) -> RunState {
        if self.is_stale() {
            RunState::Interrupted
        } else {
            self.state
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunEventLevel {
    Info,
    Warn,
    Error,
}

/// One discrete lifecycle event.
///
/// This is deliberately not where periodic metrics go: those stay in
/// `progress.jsonl`, whose rows are a fixed numeric schema that charting
/// tools parse. Mixing the two would force every reader to filter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub seq: u64,
    pub unix_ms: u64,
    pub level: RunEventLevel,
    #[serde(flatten)]
    pub payload: RunEventPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RunEventPayload {
    /// The run entered `state`.
    State { state: RunState },
    /// A checkpoint was written and the run is resumable from it.
    Checkpoint { sweeps: u64 },
    /// The solver stopped for `reason` (its completion status).
    Stop { reason: String },
    /// Something worth surfacing that did not stop the run.
    Notice { message: String },
    /// The run failed.
    Failure { message: String },
}

/// Append-only writer for `events.jsonl`.
pub struct RunEventLog {
    file: File,
    next_seq: u64,
}

impl RunEventLog {
    pub fn path_in(directory: &Path) -> PathBuf {
        directory.join(RUN_EVENTS_FILE)
    }

    /// Opens the log, continuing the sequence if one is already there (a
    /// resumed run appends to the same file rather than restarting at 0).
    pub fn create_or_append(directory: &Path) -> io::Result<Self> {
        let path = Self::path_in(directory);
        let next_seq = match read_events(&path, 0) {
            Ok((events, _)) => events.last().map_or(0, |event| event.seq.saturating_add(1)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        Ok(Self {
            file: OpenOptions::new().create(true).append(true).open(&path)?,
            next_seq,
        })
    }

    pub fn append(&mut self, level: RunEventLevel, payload: RunEventPayload) -> io::Result<()> {
        let event = RunEvent {
            seq: self.next_seq,
            unix_ms: unix_millis(),
            level,
            payload,
        };
        let mut line = serde_json::to_string(&event).map_err(io::Error::other)?;
        line.push('\n');
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }

    pub fn info(&mut self, payload: RunEventPayload) -> io::Result<()> {
        self.append(RunEventLevel::Info, payload)
    }

    pub fn state(&mut self, state: RunState) -> io::Result<()> {
        self.info(RunEventPayload::State { state })
    }
}

/// Reads `events.jsonl` from `from_offset` bytes, returning the events and
/// the offset to resume from.
///
/// A trailing partial line (the writer was mid-append) is left unread, and
/// its bytes are excluded from the returned offset, so the next call sees
/// the complete line.
pub fn read_events(path: &Path, from_offset: u64) -> io::Result<(Vec<RunEvent>, u64)> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if from_offset >= length {
        return Ok((Vec::new(), length));
    }
    file.seek(SeekFrom::Start(from_offset))?;
    let mut buffer = String::new();
    BufReader::new(&mut file).read_to_string(&mut buffer)?;

    let complete = match buffer.rfind('\n') {
        Some(index) => &buffer[..=index],
        None => return Ok((Vec::new(), from_offset)),
    };
    let mut events = Vec::new();
    for line in complete.lines() {
        if line.trim().is_empty() {
            continue;
        }
        events.push(serde_json::from_str(line).map_err(io::Error::other)?);
    }
    Ok((events, from_offset + complete.len() as u64))
}

/// Reads the last `progress.jsonl` row as raw JSON, if the file has one.
pub fn last_progress_row(directory: &Path) -> io::Result<Option<serde_json::Value>> {
    let path = directory.join(RUN_PROGRESS_FILE);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let last = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .last();
    last.map(|line| serde_json::from_str(&line).map_err(io::Error::other))
        .transpose()
}

/// True when `directory` looks like a run directory.
pub fn is_run_directory(directory: &Path) -> bool {
    RunManifest::path_in(directory).is_file()
}

pub fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    // `kill` treats pid 0 as "every process in my group", so it would report
    // alive for a manifest that never recorded a real pid. Rule it out first.
    if pid == 0 {
        return false;
    }
    // Signal 0 performs the permission and existence checks without
    // delivering anything. EPERM means the process exists but belongs to
    // someone else, which still counts as alive.
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    // Without a portable liveness probe, assume the owner is alive. A stale
    // `Running` manifest is then reported as running until something else
    // rewrites it, which is the conservative direction: it never claims a
    // live run died.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> RunManifest {
        RunManifest::new(
            "run-1",
            "preflop-multiway",
            Some("solvers.multiway-preflop/v1".into()),
            "abc123",
            vec!["solve".into(), "config.toml".into()],
        )
    }

    #[test]
    fn manifest_round_trips_through_the_run_directory() {
        let directory = tempfile::tempdir().unwrap();
        let original = manifest();
        original.write_atomic(directory.path()).unwrap();
        assert_eq!(RunManifest::read(directory.path()).unwrap(), original);
        assert!(is_run_directory(directory.path()));
    }

    #[test]
    fn manifest_rewrite_leaves_no_temporary_behind() {
        let directory = tempfile::tempdir().unwrap();
        let mut current = manifest();
        current.write_atomic(directory.path()).unwrap();
        current.finish(RunState::Completed, Some("target-reached".into()));
        current.write_atomic(directory.path()).unwrap();

        let entries: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec![RUN_MANIFEST_FILE.to_string()]);
        let reloaded = RunManifest::read(directory.path()).unwrap();
        assert_eq!(reloaded.state, RunState::Completed);
        assert_eq!(reloaded.completion.as_deref(), Some("target-reached"));
        assert!(reloaded.finished_unix_ms.is_some());
    }

    /// A manifest owned by this very process must read as running.
    #[test]
    fn a_manifest_owned_by_a_live_process_is_not_stale() {
        let current = manifest();
        assert_eq!(current.pid, std::process::id());
        assert!(!current.is_stale());
        assert_eq!(current.observed_state(), RunState::Running);
    }

    /// A `Running` manifest whose process is gone is what `Interrupted`
    /// means; nothing writes that state, a reader derives it.
    #[cfg(unix)]
    #[test]
    fn a_running_manifest_without_its_process_reads_as_interrupted() {
        let mut current = manifest();
        // pid 0 addresses the caller's process group rather than a process,
        // so it stands in for "no owning process was recorded".
        current.pid = 0;
        assert!(current.is_stale());
        assert_eq!(current.observed_state(), RunState::Interrupted);
    }

    /// Staleness only applies to a run that still claims to be running: a
    /// finished run's pid is expected to be gone.
    #[test]
    fn a_finished_manifest_is_never_stale() {
        let mut current = manifest();
        current.pid = 0;
        current.finish(RunState::Completed, None);
        assert!(!current.is_stale());
        assert_eq!(current.observed_state(), RunState::Completed);
    }

    #[test]
    fn events_resume_from_a_byte_offset_without_gaps() {
        let directory = tempfile::tempdir().unwrap();
        let path = RunEventLog::path_in(directory.path());
        let mut log = RunEventLog::create_or_append(directory.path()).unwrap();
        log.state(RunState::Running).unwrap();
        log.info(RunEventPayload::Checkpoint { sweeps: 100 })
            .unwrap();

        let (first, offset) = read_events(&path, 0).unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].seq, 0);
        assert_eq!(first[1].seq, 1);

        // Nothing new yet.
        let (none, same_offset) = read_events(&path, offset).unwrap();
        assert!(none.is_empty());
        assert_eq!(same_offset, offset);

        log.info(RunEventPayload::Stop {
            reason: "target-reached".into(),
        })
        .unwrap();
        let (tail, _) = read_events(&path, offset).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].seq, 2);
        assert_eq!(
            tail[0].payload,
            RunEventPayload::Stop {
                reason: "target-reached".into()
            }
        );
    }

    #[test]
    fn a_reopened_log_continues_the_sequence() {
        let directory = tempfile::tempdir().unwrap();
        {
            let mut log = RunEventLog::create_or_append(directory.path()).unwrap();
            log.state(RunState::Running).unwrap();
        }
        let mut resumed = RunEventLog::create_or_append(directory.path()).unwrap();
        resumed.state(RunState::Completed).unwrap();

        let (events, _) = read_events(&RunEventLog::path_in(directory.path()), 0).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].seq, 1);
    }

    /// A reader must not consume a line the writer has not finished, and
    /// must pick it up whole on the next read.
    #[test]
    fn a_partial_trailing_line_is_not_returned() {
        let directory = tempfile::tempdir().unwrap();
        let path = RunEventLog::path_in(directory.path());
        {
            let mut log = RunEventLog::create_or_append(directory.path()).unwrap();
            log.state(RunState::Running).unwrap();
        }
        let complete = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{complete}{{\"seq\":1,\"unixM")).unwrap();

        let (events, offset) = read_events(&path, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(offset, complete.len() as u64);

        std::fs::write(
            &path,
            format!("{complete}{{\"seq\":1,\"unixMs\":0,\"level\":\"info\",\"kind\":\"state\",\"state\":\"completed\"}}\n"),
        )
        .unwrap();
        let (tail, _) = read_events(&path, offset).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].seq, 1);
    }
}
