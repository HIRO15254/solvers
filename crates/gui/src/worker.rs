//! Background solve worker: owns a `MultiwaySession` on its own thread and
//! drives it in small chunks so the UI thread can observe progress, pause,
//! or cancel between chunks (see `docs/native-gui-plan.md` section F,
//! "worker protocol").

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

use cli::session::{self, MultiwaySession};
use eframe::egui;
use formats::{MultiwaySolution, MwsolStorage};
use multiway::checkpoint::MultiwayCheckpoint;
use multiway::solver::{InfoKey, ProfileEvaluation};

#[derive(Debug, Clone, Copy)]
pub enum WorkerCmd {
    Pause,
    Resume,
    /// Stop taking new sweeps and save whatever the profile is now.
    Finish,
    /// Stop and discard; no artifact is written.
    Cancel,
}

#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub sweeps: u64,
    pub target: u64,
    pub elapsed_secs: f64,
    pub sweeps_per_sec: f64,
    pub infosets: u64,
    pub memory_bytes: u64,
    pub seat_avg_pos_regret: Vec<f64>,
    pub seat_drift_l1: Vec<f64>,
}

#[derive(Debug)]
pub struct FinishedRun {
    pub solution: MultiwaySolution,
    pub mwsol_path: PathBuf,
}

pub enum WorkerEvent {
    /// Sent once, before the (potentially slow) card abstraction/game build.
    Building,
    Progress(ProgressSnapshot),
    Evaluated(ProfileEvaluation),
    Finished(Box<FinishedRun>),
    Cancelled,
    Failed(String),
}

pub struct WorkerHandle {
    pub commands: Sender<WorkerCmd>,
    pub events: Receiver<WorkerEvent>,
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    pub fn send(&self, cmd: WorkerCmd) {
        let _ = self.commands.send(cmd);
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCmd::Cancel);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Parameters needed to start a solve that live outside the `SolveConfig`
/// TOML itself (artifact destinations, same as the CLI's `--checkpoint`/
/// `--sol` flags).
pub struct RunTarget {
    pub config_toml: String,
    pub resume_checkpoint: Option<PathBuf>,
    pub output_path: PathBuf,
    pub checkpoint_path: Option<PathBuf>,
    /// `run.check_every` from the model; used only to bound the worker's
    /// internal chunk size, not sent to the solver.
    pub check_every: u64,
}

pub fn spawn(target: RunTarget, ctx: egui::Context) -> WorkerHandle {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let join = std::thread::spawn(move || run(target, command_rx, event_tx, ctx));
    WorkerHandle {
        commands: command_tx,
        events: event_rx,
        join: Some(join),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunState {
    Running,
    Paused,
    Finishing,
    Cancelled,
}

fn apply_cmd(cmd: WorkerCmd, state: RunState) -> RunState {
    match cmd {
        WorkerCmd::Pause => RunState::Paused,
        WorkerCmd::Resume => {
            if state == RunState::Cancelled || state == RunState::Finishing {
                state
            } else {
                RunState::Running
            }
        }
        WorkerCmd::Finish => RunState::Finishing,
        WorkerCmd::Cancel => RunState::Cancelled,
    }
}

fn run(
    target: RunTarget,
    commands: Receiver<WorkerCmd>,
    events: Sender<WorkerEvent>,
    ctx: egui::Context,
) {
    let send = |event: WorkerEvent| {
        let _ = events.send(event);
        ctx.request_repaint();
    };

    send(WorkerEvent::Building);
    let mut mw_session = match session::build_multiway_session(
        &target.config_toml,
        target.resume_checkpoint.as_deref(),
    ) {
        Ok(session) => session,
        Err(error) => {
            send(WorkerEvent::Failed(format!("{error:#}")));
            return;
        }
    };

    let started = Instant::now();
    let mut state = RunState::Running;
    // Resumed checkpoints start with a non-zero sweep count; throughput must
    // only count sweeps produced by this run.
    let initial_sweeps = mw_session.solver.completed_sweeps();
    // Seed `prior` with the current averages (drift result discarded).
    let mut prior: HashMap<InfoKey, Vec<f32>> = HashMap::new();
    mw_session.solver.strategy_drift_refresh(&mut prior);
    let mut last_drift = vec![0.0; mw_session.game_config.seats.len()];
    // Full SolverMetrics costs O(infosets); refresh at most a few times per
    // second and reuse the last reading for in-between progress frames.
    let mut last_metrics = mw_session.solver.metrics();
    let mut last_metrics_at = Instant::now();

    'drive: loop {
        state = drain_commands(&commands, state);
        match state {
            RunState::Cancelled => break 'drive,
            RunState::Finishing => break 'drive,
            RunState::Paused => match commands.recv() {
                Ok(cmd) => {
                    state = apply_cmd(cmd, state);
                    continue 'drive;
                }
                Err(_) => break 'drive,
            },
            RunState::Running => {}
        }

        let current = mw_session.solver.completed_sweeps();
        if current >= mw_session.sweeps_target {
            break 'drive;
        }
        let remaining = mw_session.sweeps_target - current;
        let base_chunk = (mw_session.sweeps_target / 1_000).clamp(1, target.check_every.max(1));
        // Never step across an evaluation or checkpoint boundary: the
        // cadence gates below use `sweeps % cadence == 0` and would silently
        // starve on misaligned counts (e.g. after a mid-chunk pause).
        let evaluation_delta =
            session::distance_to_boundary(current, mw_session.evaluation_cadence);
        let checkpoint_delta = mw_session
            .checkpoint_every
            .map(|cadence| session::distance_to_boundary(current, cadence))
            .unwrap_or(u64::MAX);
        let chunk = base_chunk
            .min(remaining)
            .min(evaluation_delta)
            .min(checkpoint_delta)
            .max(1);

        let result =
            mw_session
                .solver
                .run_sweeps_with_threads_until(chunk, mw_session.threads, || {
                    state = drain_commands(&commands, state);
                    state == RunState::Running
                });
        if let Err(error) = result {
            send(WorkerEvent::Failed(format!(
                "running multiway MCCFR: {error}"
            )));
            return;
        }

        let sweeps_now = mw_session.solver.completed_sweeps();
        let at_evaluation = sweeps_now % mw_session.evaluation_cadence == 0;
        if at_evaluation || last_metrics_at.elapsed().as_millis() >= 250 {
            last_metrics = mw_session.solver.metrics();
            last_metrics_at = Instant::now();
        }
        let metrics = &last_metrics;
        if at_evaluation {
            last_drift = mw_session.solver.strategy_drift_refresh(&mut prior);
            match mw_session
                .solver
                .evaluate_average_profile(mw_session.evaluation_samples, mw_session.evaluation_seed)
            {
                Ok(evaluation) => send(WorkerEvent::Evaluated(evaluation)),
                Err(error) => {
                    send(WorkerEvent::Failed(format!(
                        "evaluating held-out multiway profile: {error}"
                    )));
                    return;
                }
            }
        }
        // Checkpoint cadence is independent of the evaluation cadence.
        if let Some(path) = target.checkpoint_path.as_deref()
            && mw_session
                .checkpoint_every
                .is_some_and(|cadence| sweeps_now % cadence == 0)
            && let Err(error) = write_checkpoint(&mw_session, path)
        {
            send(WorkerEvent::Failed(format!(
                "writing checkpoint: {error:#}"
            )));
            return;
        }

        let elapsed = started.elapsed().as_secs_f64();
        send(WorkerEvent::Progress(ProgressSnapshot {
            sweeps: sweeps_now,
            target: mw_session.sweeps_target,
            elapsed_secs: elapsed,
            sweeps_per_sec: if elapsed > 0.0 {
                (sweeps_now - initial_sweeps) as f64 / elapsed
            } else {
                0.0
            },
            infosets: metrics.infosets,
            memory_bytes: metrics.memory_bytes,
            seat_avg_pos_regret: metrics.average_positive_regret.clone(),
            seat_drift_l1: last_drift.clone(),
        }));

        state = drain_commands(&commands, state);
        if state == RunState::Cancelled {
            break 'drive;
        }
    }

    if state == RunState::Cancelled {
        send(WorkerEvent::Cancelled);
        return;
    }

    // Finishing (explicit "Finish & Save") or natural completion: persist
    // the current average profile.
    let final_evaluation = match mw_session
        .solver
        .evaluate_average_profile(mw_session.evaluation_samples, mw_session.evaluation_seed)
    {
        Ok(evaluation) => Some(evaluation),
        Err(error) => {
            send(WorkerEvent::Failed(format!(
                "evaluating final held-out multiway profile: {error}"
            )));
            return;
        }
    };
    let snapshot = mw_session.solver.snapshot_state();
    let drift = session::strategy_drift(
        &snapshot.policies,
        &prior,
        mw_session.game_config.seats.len(),
    );
    let final_metrics = mw_session.solver.metrics();
    let row = session::metrics_row(
        &final_metrics,
        drift,
        started.elapsed().as_secs_f64(),
        final_evaluation.as_ref(),
    );
    let solution = session::make_solution(
        &mw_session.config_toml,
        mw_session.solver.abstraction_fingerprint(),
        &snapshot,
        &row,
    );
    if let Some(parent) = target
        .output_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        send(WorkerEvent::Failed(format!(
            "creating output directory {}: {error}",
            parent.display()
        )));
        return;
    }
    let storage = match mw_session.storage {
        cli::config::StorageKind::F32 => MwsolStorage::F32,
        cli::config::StorageKind::I16 => MwsolStorage::I16,
    };
    if let Err(error) = formats::write_mwsol_with(&target.output_path, &solution, storage) {
        send(WorkerEvent::Failed(format!(
            "writing {}: {error}",
            target.output_path.display()
        )));
        return;
    }
    if let Some(path) = target.checkpoint_path.as_deref()
        && let Err(error) = write_checkpoint(&mw_session, path)
    {
        send(WorkerEvent::Failed(format!(
            "writing final checkpoint: {error:#}"
        )));
        return;
    }
    send(WorkerEvent::Finished(Box::new(FinishedRun {
        solution,
        mwsol_path: target.output_path,
    })));
}

fn drain_commands(commands: &Receiver<WorkerCmd>, mut state: RunState) -> RunState {
    loop {
        match commands.try_recv() {
            Ok(cmd) => state = apply_cmd(cmd, state),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state = RunState::Cancelled;
                break;
            }
        }
    }
    state
}

fn write_checkpoint(session: &MultiwaySession, path: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    MultiwayCheckpoint::capture(&session.solver).write_atomic(path)?;
    Ok(())
}
