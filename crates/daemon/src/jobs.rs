//! Job execution: spawning the CLI, queueing, and cancelling.
//!
//! The daemon never solves anything itself (R1). Every run is a child
//! `solvers` process writing into its own run directory, which is what keeps
//! local and remote execution identical and keeps a 6 GiB policy arena out
//! of the daemon's address space.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

/// A run waiting for a slot.
struct Queued {
    run_id: String,
    directory: PathBuf,
    command: JobCommand,
}

/// What to run for a job. Both forms take the run directory and nothing
/// else, because that directory already carries the config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobCommand {
    /// `solvers solve <run-dir>/run.toml --out <run-dir>` for a fresh run.
    Solve,
    /// `solvers resume <run-dir>` to continue a stopped one.
    Resume,
}

struct Running {
    run_id: String,
    child: Option<Child>,
    directory: PathBuf,
    pid: u32,
    started: u64,
    signalled: bool,
}

/// Runs jobs, at most `max_concurrent` at a time.
///
/// The policy is FIFO with a fixed slot count. A run reserves several GiB
/// for its policy arena, so the interesting limit is memory rather than CPU,
/// and the operator is in a better position to know the budget than any
/// heuristic here -- hence a flag rather than a guess.
pub struct JobRunner {
    solver: PathBuf,
    cache_dir: Option<PathBuf>,
    max_concurrent: usize,
    state: Arc<Mutex<RunnerState>>,
}

#[derive(Default)]
struct RunnerState {
    queue: VecDeque<Queued>,
    running: Vec<Running>,
    next_sequence: u64,
}

impl JobRunner {
    pub fn new(solver: PathBuf, cache_dir: Option<PathBuf>, max_concurrent: usize) -> Self {
        Self {
            solver,
            cache_dir,
            max_concurrent: max_concurrent.max(1),
            state: Arc::new(Mutex::new(RunnerState::default())),
        }
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Accepts a job, starting it now if a slot is free.
    ///
    /// Returns whether it started; a queued job has no process yet, and its
    /// manifest still says `queued` until one is spawned.
    pub fn submit(&self, run_id: &str, directory: &Path, command: JobCommand) -> Result<bool> {
        let mut state = self.state.lock().expect("job state");
        if state.queue.iter().any(|job| job.run_id == run_id)
            || state.running.iter().any(|job| job.run_id == run_id)
        {
            anyhow::bail!("run {run_id:?} is already active");
        }
        match std::fs::remove_file(running_marker(directory)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        state.next_sequence = state
            .next_sequence
            .max(runfiles::unix_millis())
            .saturating_add(1);
        let mut manifest = runfiles::RunManifest::read(directory)?;
        manifest.state = runfiles::RunState::Queued;
        manifest.queue_sequence = Some(state.next_sequence);
        manifest.queued_command = Some(
            match command {
                JobCommand::Solve => "solve",
                JobCommand::Resume => "resume",
            }
            .to_string(),
        );
        manifest.started_unix_ms = None;
        manifest.finished_unix_ms = None;
        manifest.failure = None;
        manifest.write_atomic(directory)?;
        state.queue.push_back(Queued {
            run_id: run_id.to_string(),
            directory: directory.to_path_buf(),
            command,
        });
        self.drain(&mut state);
        Ok(state.running.iter().any(|running| running.run_id == run_id))
    }

    pub fn restore(&self, run_id: &str, directory: &Path, manifest: &runfiles::RunManifest) {
        let mut state = self.state.lock().expect("job state");
        state.next_sequence = state
            .next_sequence
            .max(manifest.queue_sequence.unwrap_or(0));
        let recorded = std::fs::read_to_string(running_marker(directory))
            .ok()
            .and_then(|text| {
                let (pid, started) = text.split_once(' ')?;
                Some((pid.parse::<u32>().ok()?, started.parse::<u64>().ok()?))
            });
        let mut probe = manifest.clone();
        probe.state = runfiles::RunState::Running;
        if let Some((pid, started)) = recorded {
            probe.pid = pid;
            probe.started_unix_ms = Some(started);
        }
        if (manifest.state == runfiles::RunState::Running && !manifest.is_stale())
            || (manifest.state == runfiles::RunState::Queued && recorded.is_some())
        {
            state.running.push(Running {
                run_id: run_id.to_string(),
                directory: directory.to_path_buf(),
                child: None,
                pid: probe.pid,
                started: probe.started_unix_ms.unwrap_or(probe.created_unix_ms),
                signalled: false,
            });
        } else if manifest.state == runfiles::RunState::Queued {
            state.queue.push_back(Queued {
                run_id: run_id.to_string(),
                directory: directory.to_path_buf(),
                command: if manifest.queued_command.as_deref() == Some("resume") {
                    JobCommand::Resume
                } else {
                    JobCommand::Solve
                },
            });
        }
    }

    /// Starts anything the freed slots can take. Called after reaping.
    pub fn pump(&self) {
        let mut state = self.state.lock().expect("job state");
        self.drain(&mut state);
    }

    fn drain(&self, state: &mut RunnerState) {
        self.reap(state);
        loop {
            if state.running.len() >= self.max_concurrent {
                return;
            }
            let Some(job) = state.queue.pop_front() else {
                return;
            };
            if let Err(error) = self.spawn(state, &job.run_id, &job.directory, job.command) {
                match record_failure_if_unowned(&job.directory, &format!("{error:#}")) {
                    Ok(true) => {}
                    Ok(false) => {
                        state.queue.push_front(job);
                        return;
                    }
                    Err(record_error) => {
                        eprintln!(
                            "failed to record startup failure for run {:?}: {record_error:#}",
                            job.run_id
                        );
                    }
                }
            }
        }
    }

    /// Asks a running job to stop.
    ///
    /// An interrupt rather than a kill: the CLI treats it as a cooperative
    /// cancel, finishing at an evaluation boundary and saving a final
    /// checkpoint when configured. Resumability depends on a checkpoint
    /// actually being present. Unix sends SIGINT; Windows sends Ctrl-Break
    /// to the child's own process group. A queued job is
    /// persisted as canceled before it is removed from the queue. A signal
    /// or manifest write that fails is an error, never a silent no-op.
    pub fn cancel(&self, run_id: &str) -> Result<CancelOutcome> {
        let mut state = self.state.lock().expect("job state");
        if let Some(index) = state
            .queue
            .iter()
            .position(|queued| queued.run_id == run_id)
        {
            let directory = &state.queue[index].directory;
            let mut manifest = runfiles::RunManifest::read(directory)
                .with_context(|| format!("reading manifest for queued run {run_id:?}"))?;
            manifest.state = runfiles::RunState::Canceled;
            manifest.finished_unix_ms = Some(runfiles::unix_millis());
            manifest.queue_sequence = None;
            manifest.queued_command = None;
            manifest
                .write_atomic(directory)
                .with_context(|| format!("canceling queued run {run_id:?}"))?;
            state.queue.remove(index);
            return Ok(CancelOutcome::Dequeued);
        }
        let Some(running) = state
            .running
            .iter_mut()
            .find(|running| running.run_id == run_id)
        else {
            return Ok(CancelOutcome::NotRunning);
        };
        if !running.signalled {
            signal_interrupt(running.pid)
                .with_context(|| format!("interrupting the solver of run {run_id:?}"))?;
            running.signalled = true;
        }
        Ok(CancelOutcome::Signalled)
    }

    /// Whether this daemon is running or holding that job.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_active(&self, run_id: &str) -> bool {
        let mut state = self.state.lock().expect("job state");
        self.reap(&mut state);
        state.running.iter().any(|running| running.run_id == run_id)
            || state.queue.iter().any(|queued| queued.run_id == run_id)
    }

    fn spawn(
        &self,
        state: &mut RunnerState,
        run_id: &str,
        directory: &Path,
        command: JobCommand,
    ) -> Result<()> {
        let mut process = Command::new(&self.solver);
        if let Some(cache) = &self.cache_dir {
            process.arg("--cache-dir").arg(cache);
        }
        match command {
            JobCommand::Solve => {
                process
                    .arg("solve")
                    .arg(directory.join(runfiles::RUN_CONFIG_FILE))
                    .arg("--out")
                    .arg(directory);
            }
            JobCommand::Resume => {
                process.arg("resume").arg(directory);
            }
        }
        // Its own process group is what lets `cancel` address Ctrl-Break to
        // this child alone; it also keeps a Ctrl-C typed at the daemon's
        // console from reaching the solve.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            process.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        // The child's own output is a debugging aid, not the run's record --
        // that is `events.jsonl`. Keeping it beside the run means a failure
        // the manifest summarizes can still be read in full.
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("stdout.log"))
            .with_context(|| format!("opening stdout.log in {}", directory.display()))?;
        let errors = log
            .try_clone()
            .context("duplicating the run log handle for stderr")?;
        let child = process
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(errors))
            .spawn()
            .with_context(|| format!("spawning {}", self.solver.display()))?;

        let pid = child.id();
        let started = runfiles::unix_millis();
        if let Err(error) = std::fs::write(running_marker(directory), format!("{pid} {started}")) {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
        state.running.push(Running {
            run_id: run_id.to_string(),
            directory: directory.to_path_buf(),
            child: Some(child),
            pid,
            started,
            signalled: false,
        });
        Ok(())
    }

    fn reap(&self, state: &mut RunnerState) {
        let mut index = 0;
        while index < state.running.len() {
            let running = &mut state.running[index];
            let result = (|| -> Result<bool> {
                let status = if let Some(child) = &mut running.child {
                    child.try_wait()?.map(|status| status.to_string())
                } else {
                    let manifest = runfiles::RunManifest::read(&running.directory)?;
                    let mut probe = manifest.clone();
                    probe.state = runfiles::RunState::Running;
                    probe.pid = running.pid;
                    probe.started_unix_ms = Some(running.started);
                    if probe.is_stale() {
                        Some("unknown status".to_string())
                    } else {
                        None
                    }
                };
                let Some(status) = status else {
                    return Ok(false);
                };
                if !record_failure_if_unowned(
                    &running.directory,
                    &format!("solver exited with {status} before starting; see stdout.log"),
                )? {
                    let manifest = runfiles::RunManifest::read(&running.directory)?;
                    if manifest.state == runfiles::RunState::Running && !manifest.is_stale() {
                        running.pid = manifest.pid;
                        running.started =
                            manifest.started_unix_ms.unwrap_or(manifest.created_unix_ms);
                        running.child = None;
                    }
                    return Ok(false);
                }
                Ok(true)
            })();
            match result {
                Ok(false) => index += 1,
                result => {
                    if let Err(error) = result {
                        eprintln!("failed to reap run {:?}: {error:#}", running.run_id);
                    }
                    let _ = std::fs::remove_file(running_marker(&running.directory));
                    state.running.remove(index);
                }
            }
        }
    }
}

fn running_marker(directory: &Path) -> PathBuf {
    let name = directory.file_name().unwrap_or_default().to_string_lossy();
    directory.with_file_name(format!(".{name}.daemon-running"))
}

// The CLI holds this lock from adoption through terminal manifest publication.
// Holding it across the read and write prevents a losing duplicate from
// replacing a live adopter's manifest.
fn record_failure_if_unowned(directory: &Path, message: &str) -> Result<bool> {
    let _lock = match runfiles::RunLock::acquire(directory) {
        Ok(lock) => lock,
        Err(runfiles::RunLockError::Busy { .. }) => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut manifest = runfiles::RunManifest::read(directory)?;
    if manifest.state == runfiles::RunState::Queued {
        if let Ok(marker) = std::fs::read_to_string(running_marker(directory))
            && let Some((pid, started)) = marker.split_once(' ')
            && let (Ok(pid), Ok(started)) = (pid.parse::<u32>(), started.parse::<u64>())
        {
            let mut probe = manifest.clone();
            probe.state = runfiles::RunState::Running;
            probe.pid = pid;
            probe.started_unix_ms = Some(started);
            if !probe.is_stale() {
                return Ok(false);
            }
        }
        manifest.fail(message);
        manifest.queue_sequence = None;
        manifest.queued_command = None;
        manifest.write_atomic(directory)?;
    }
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// The running child was signalled, either now or by an earlier cancel.
    Signalled,
    /// The job was still queued and never started.
    Dequeued,
    /// This daemon is not running that job. It may have finished, or it may
    /// belong to another process entirely.
    NotRunning,
}

#[cfg(unix)]
fn signal_interrupt(pid: u32) -> std::io::Result<()> {
    let pid = libc::pid_t::try_from(pid).map_err(std::io::Error::other)?;
    if unsafe { libc::kill(pid, libc::SIGINT) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// The child leads its own process group (see `spawn`), whose id is its pid.
/// Delivery needs the daemon to share a console with it; a daemon without a
/// console gets the OS error instead of a cancel that never arrives.
#[cfg(windows)]
fn signal_interrupt(pid: u32) -> std::io::Result<()> {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner(max: usize) -> (tempfile::TempDir, JobRunner) {
        let directory = tempfile::tempdir().unwrap();
        // Invoked with solver CLI arguments, the Rust test harness exits
        // immediately with an argument error. That is sufficient here:
        // these tests exercise process slots and queueing, not the solver.
        let runner = JobRunner::new(std::env::current_exe().unwrap(), None, max);
        (directory, runner)
    }

    #[test]
    #[ignore = "spawned explicitly by the queue test"]
    fn child_process_helper() {
        if std::env::var_os("SOLVERS_QUEUE_TEST_CHILD").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    /// Like a solver, it leads its own process group, so cancelling it on
    /// Windows reaches only this child.
    fn sleeping_child() -> Child {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .env("SOLVERS_QUEUE_TEST_CHILD", "1")
            .arg("--ignored")
            .arg("--exact")
            .arg("jobs::tests::child_process_helper")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        command.spawn().unwrap()
    }

    fn run_dir(base: &Path, name: &str) -> PathBuf {
        let directory = base.join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(runfiles::RUN_CONFIG_FILE),
            "schema = \"x\"\n",
        )
        .unwrap();
        let mut manifest = runfiles::RunManifest::new(name, "test", None, "", vec![]);
        manifest.state = runfiles::RunState::Queued;
        manifest.write_atomic(&directory).unwrap();
        directory
    }

    /// Beyond the slot count, jobs wait instead of starting.
    #[test]
    fn submissions_past_the_limit_are_queued() {
        let (base, runner) = runner(1);
        let first = run_dir(base.path(), "a");
        let second = run_dir(base.path(), "b");

        assert!(runner.submit("a", &first, JobCommand::Solve).unwrap());
        // `a` may already have exited, so force the queue decision by
        // checking before any reap can free the slot.
        let queued = {
            let mut state = runner.state.lock().unwrap();
            let child = sleeping_child();
            state.running.push(Running {
                run_id: "hold".into(),
                pid: child.id(),
                started: runfiles::unix_millis(),
                directory: first.clone(),
                child: Some(child),
                signalled: false,
            });
            drop(state);
            runner.submit("b", &second, JobCommand::Solve).unwrap()
        };
        assert!(!queued, "the second job must wait for a slot");
        assert!(runner.is_active("b"));

        // Releasing the slot lets the queue drain.
        runner.cancel("hold").unwrap();
        let mut state = runner.state.lock().unwrap();
        for running in &mut state.running {
            if let Some(child) = &mut running.child {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        drop(state);
        runner.pump();
    }

    #[test]
    fn submit_starts_older_queued_work_when_a_slot_is_free() {
        let (base, runner) = runner(1);
        let first = run_dir(base.path(), "first");
        let later = run_dir(base.path(), "later");
        {
            let mut state = runner.state.lock().unwrap();
            state.queue.push_back(Queued {
                run_id: "first".into(),
                directory: first.clone(),
                command: JobCommand::Solve,
            });
        }
        runner.submit("later", &later, JobCommand::Solve).unwrap();
        assert!(first.join("stdout.log").is_file());
    }

    #[test]
    fn repeated_cancel_does_not_signal_again() {
        let (base, runner) = runner(1);
        {
            let mut state = runner.state.lock().unwrap();
            let child = sleeping_child();
            state.running.push(Running {
                run_id: "hold".into(),
                pid: child.id(),
                started: runfiles::unix_millis(),
                directory: base.path().to_path_buf(),
                child: Some(child),
                signalled: true,
            });
        }
        assert_eq!(runner.cancel("hold").unwrap(), CancelOutcome::Signalled);
        let mut state = runner.state.lock().unwrap();
        let child = state.running[0].child.as_mut().unwrap();
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn failed_queued_spawn_does_not_block_later_jobs() {
        let (base, runner) = runner(1);
        let first = run_dir(base.path(), "first");
        std::fs::create_dir(first.join("stdout.log")).unwrap();
        let later = run_dir(base.path(), "later");
        {
            let mut state = runner.state.lock().unwrap();
            state.queue.push_back(Queued {
                run_id: "first".into(),
                directory: first.clone(),
                command: JobCommand::Solve,
            });
        }
        runner.submit("later", &later, JobCommand::Solve).unwrap();
        let failed = runfiles::RunManifest::read(&first).unwrap();
        assert_eq!(failed.state, runfiles::RunState::Failed);
        assert!(failed.failure.unwrap().contains("stdout.log"));
        assert!(failed.finished_unix_ms.is_some());
        assert!(later.join("stdout.log").is_file());
    }

    #[test]
    fn missing_queued_directory_does_not_block_later_jobs() {
        let (base, runner) = runner(1);
        let later = run_dir(base.path(), "later");
        {
            let mut state = runner.state.lock().unwrap();
            state.queue.push_back(Queued {
                run_id: "missing".into(),
                directory: base.path().join("missing"),
                command: JobCommand::Solve,
            });
        }
        runner.submit("later", &later, JobCommand::Solve).unwrap();
        assert!(later.join("stdout.log").is_file());
    }

    #[test]
    fn missing_running_directory_releases_slot() {
        let (base, runner) = runner(1);
        let missing = run_dir(base.path(), "missing");
        std::fs::remove_dir_all(&missing).unwrap();
        let later = run_dir(base.path(), "later");
        {
            let mut state = runner.state.lock().unwrap();
            state.running.push(Running {
                run_id: "missing".into(),
                directory: missing,
                child: None,
                pid: std::process::id(),
                started: runfiles::unix_millis(),
                signalled: false,
            });
        }
        runner.submit("later", &later, JobCommand::Solve).unwrap();
        assert!(later.join("stdout.log").is_file());
    }

    #[test]
    fn child_exit_before_adoption_records_failure() {
        let (base, runner) = runner(1);
        let directory = run_dir(base.path(), "early");
        runner
            .submit("early", &directory, JobCommand::Solve)
            .unwrap();
        {
            let mut state = runner.state.lock().unwrap();
            state.running[0].child.as_mut().unwrap().wait().unwrap();
        }
        runner.pump();
        let manifest = runfiles::RunManifest::read(&directory).unwrap();
        assert_eq!(manifest.state, runfiles::RunState::Failed);
        assert!(manifest.failure.unwrap().contains("before starting"));
        assert!(manifest.finished_unix_ms.is_some());
    }

    #[test]
    fn failed_duplicate_does_not_replace_live_or_terminal_manifest() {
        let (base, _) = runner(1);
        let directory = run_dir(base.path(), "winner");
        let lock = runfiles::RunLock::acquire(&directory).unwrap();
        assert!(!record_failure_if_unowned(&directory, "duplicate failed").unwrap());
        assert_eq!(
            runfiles::RunManifest::read(&directory).unwrap().state,
            runfiles::RunState::Queued
        );
        drop(lock);
        let mut manifest = runfiles::RunManifest::read(&directory).unwrap();
        manifest.state = runfiles::RunState::Completed;
        manifest.write_atomic(&directory).unwrap();
        assert!(record_failure_if_unowned(&directory, "duplicate failed").unwrap());
        assert_eq!(
            runfiles::RunManifest::read(&directory).unwrap().state,
            runfiles::RunState::Completed
        );
    }

    #[test]
    fn cancelling_a_queued_job_removes_it_without_signalling() {
        let (base, runner) = runner(1);
        let held = run_dir(base.path(), "held");
        let mut manifest = runfiles::RunManifest::new("waiting", "test", None, "", vec![]);
        manifest.state = runfiles::RunState::Queued;
        manifest.write_atomic(&held).unwrap();
        {
            let mut state = runner.state.lock().unwrap();
            state.queue.push_back(Queued {
                run_id: "waiting".into(),
                directory: held,
                command: JobCommand::Solve,
            });
        }
        assert_eq!(runner.cancel("waiting").unwrap(), CancelOutcome::Dequeued);
        assert!(!runner.is_active("waiting"));
    }

    #[test]
    fn cancelling_an_unknown_job_says_so() {
        let (_base, runner) = runner(1);
        assert_eq!(runner.cancel("nobody").unwrap(), CancelOutcome::NotRunning);
    }
}
