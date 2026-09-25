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
    child: Child,
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
        {
            let mut state = self.state.lock().expect("job state");
            self.reap(&mut state);
            if state.running.len() >= self.max_concurrent {
                state.queue.push_back(Queued {
                    run_id: run_id.to_string(),
                    directory: directory.to_path_buf(),
                    command,
                });
                return Ok(false);
            }
        }
        self.spawn(run_id, directory, command)?;
        Ok(true)
    }

    /// Starts anything the freed slots can take. Called after reaping.
    pub fn pump(&self) -> Result<()> {
        loop {
            let next = {
                let mut state = self.state.lock().expect("job state");
                self.reap(&mut state);
                if state.running.len() >= self.max_concurrent {
                    return Ok(());
                }
                state.queue.pop_front()
            };
            let Some(job) = next else { return Ok(()) };
            self.spawn(&job.run_id, &job.directory, job.command)?;
        }
    }

    /// Asks a running job to stop.
    ///
    /// SIGINT rather than SIGKILL: the CLI treats it as a cooperative
    /// cancel, finishing the current evaluation boundary and writing a
    /// checkpoint, so the run stays resumable. A queued job is simply
    /// dropped from the queue.
    pub fn cancel(&self, run_id: &str) -> CancelOutcome {
        let mut state = self.state.lock().expect("job state");
        if let Some(index) = state
            .queue
            .iter()
            .position(|queued| queued.run_id == run_id)
        {
            state.queue.remove(index);
            return CancelOutcome::Dequeued;
        }
        let Some(running) = state
            .running
            .iter()
            .find(|running| running.run_id == run_id)
        else {
            return CancelOutcome::NotRunning;
        };
        signal_interrupt(running.child.id());
        CancelOutcome::Signalled
    }

    /// Whether this daemon is running or holding that job.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_active(&self, run_id: &str) -> bool {
        let mut state = self.state.lock().expect("job state");
        self.reap(&mut state);
        state.running.iter().any(|running| running.run_id == run_id)
            || state.queue.iter().any(|queued| queued.run_id == run_id)
    }

    fn spawn(&self, run_id: &str, directory: &Path, command: JobCommand) -> Result<()> {
        let mut process = Command::new(&self.solver);
        if let Some(cache) = &self.cache_dir {
            process.arg("--cache-dir").arg(cache);
        }
        match command {
            JobCommand::Solve => {
                process
                    .arg("solve")
                    .arg(directory.join(formats::RUN_CONFIG_FILE))
                    .arg("--out")
                    .arg(directory);
            }
            JobCommand::Resume => {
                process.arg("resume").arg(directory);
            }
        }
        // The child's own output is a debugging aid, not the run's record --
        // that is `events.jsonl`. Keeping it beside the run means a failure
        // the manifest summarizes can still be read in full.
        let log = std::fs::File::create(directory.join("stdout.log"))
            .with_context(|| format!("creating stdout.log in {}", directory.display()))?;
        let errors = log
            .try_clone()
            .context("duplicating the run log handle for stderr")?;
        let child = process
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(errors))
            .spawn()
            .with_context(|| format!("spawning {}", self.solver.display()))?;

        self.state.lock().expect("job state").running.push(Running {
            run_id: run_id.to_string(),
            child,
        });
        Ok(())
    }

    /// Drops finished children.
    ///
    /// The run's outcome is not read from the exit status: the child records
    /// it in the manifest before exiting, and that is what every reader
    /// uses. This only frees the slot.
    fn reap(&self, state: &mut RunnerState) {
        state
            .running
            .retain_mut(|running| !matches!(running.child.try_wait(), Ok(Some(_)) | Err(_)));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// A signal was delivered to a running child.
    Signalled,
    /// The job was still queued and never started.
    Dequeued,
    /// This daemon is not running that job. It may have finished, or it may
    /// belong to another process entirely.
    NotRunning,
}

#[cfg(unix)]
fn signal_interrupt(pid: u32) {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGINT) };
}

#[cfg(not(unix))]
fn signal_interrupt(_pid: u32) {
    // No portable cooperative interrupt; a Windows daemon would need a
    // console control event or a shared cancel file. Cancelling there is
    // reported as unsupported rather than silently killing the run and
    // losing everything since the last checkpoint.
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

    fn sleeping_child() -> Child {
        Command::new(std::env::current_exe().unwrap())
            .env("SOLVERS_QUEUE_TEST_CHILD", "1")
            .arg("--ignored")
            .arg("--exact")
            .arg("jobs::tests::child_process_helper")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn run_dir(base: &Path, name: &str) -> PathBuf {
        let directory = base.join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(formats::RUN_CONFIG_FILE), "schema = \"x\"\n").unwrap();
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
            state.running.push(Running {
                run_id: "hold".into(),
                child: sleeping_child(),
            });
            drop(state);
            runner.submit("b", &second, JobCommand::Solve).unwrap()
        };
        assert!(!queued, "the second job must wait for a slot");
        assert!(runner.is_active("b"));

        // Releasing the slot lets the queue drain.
        runner.cancel("hold");
        let mut state = runner.state.lock().unwrap();
        for running in &mut state.running {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
        drop(state);
        runner.pump().unwrap();
    }

    #[test]
    fn cancelling_a_queued_job_removes_it_without_signalling() {
        let (base, runner) = runner(1);
        let held = run_dir(base.path(), "held");
        {
            let mut state = runner.state.lock().unwrap();
            state.queue.push_back(Queued {
                run_id: "waiting".into(),
                directory: held,
                command: JobCommand::Solve,
            });
        }
        assert_eq!(runner.cancel("waiting"), CancelOutcome::Dequeued);
        assert!(!runner.is_active("waiting"));
    }

    #[test]
    fn cancelling_an_unknown_job_says_so() {
        let (_base, runner) = runner(1);
        assert_eq!(runner.cancel("nobody"), CancelOutcome::NotRunning);
    }
}
