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
use multiway::solver::{HistoryKey, InfoKey, ProfileEvaluation};

#[derive(Debug, Clone, Copy)]
pub enum WorkerCmd {
    Pause,
    Resume,
    /// Stop taking new sweeps and save whatever the profile is now.
    Finish,
    /// Stop and discard; no artifact is written.
    Cancel,
    /// Start (or, with `None`, stop) watching one public-history node for
    /// live average-strategy snapshots. Carries the raw history key rather
    /// than `HistoryKey` so the command stays `Copy`. Does not affect
    /// [`RunState`]: the worker keeps running/paused exactly as before.
    WatchNode(Option<[u8; 16]>),
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

/// One child edge out of a watched node, for the "Live node view" button row.
#[derive(Debug, Clone)]
pub struct NodeChild {
    pub key: [u8; 16],
    pub actor: u8,
    pub action: String,
}

/// One live policy column at the watched node: a `(actor, street,
/// active_opponents, bucket_path)` block with its current average strategy.
#[derive(Debug, Clone)]
pub struct NodeBlock {
    pub actor: u8,
    pub street: u8,
    pub active_opponents: u8,
    pub bucket_path: [u32; 4],
    pub actions: Vec<String>,
    pub probabilities: Vec<f32>,
}

/// Live snapshot of one watched public-history node, rebuilt from the
/// solver's cold query accessors (`node_children`/`strategies_at`) at UI
/// refresh rate.
#[derive(Debug, Clone)]
pub struct NodeSnapshot {
    pub history: [u8; 16],
    pub sweeps: u64,
    /// Root-to-node path, one "SEATNAME action" entry per edge; empty at
    /// ROOT.
    pub path: Vec<String>,
    pub children: Vec<NodeChild>,
    pub blocks: Vec<NodeBlock>,
}

pub enum WorkerEvent {
    /// Sent once, before the (potentially slow) card abstraction/game build.
    Building,
    Progress(ProgressSnapshot),
    Evaluated(ProfileEvaluation),
    /// Live average strategy at a watched node; see [`WorkerCmd::WatchNode`].
    NodeStrategies(Box<NodeSnapshot>),
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
        // Handled directly by `drain_commands`/the Paused loop below, which
        // need the target itself; never changes `RunState`.
        WorkerCmd::WatchNode(_) => state,
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
    // Live node-strategy view (see `WorkerCmd::WatchNode`): `watch_dirty`
    // accumulates across every `drain_commands` call since the last
    // `NodeSnapshot` send, so a target change is never silently absorbed by
    // the 250ms throttle below.
    let mut watched: Option<[u8; 16]> = None;
    let mut watch_dirty = false;
    let mut last_node_sent_at = Instant::now();

    'drive: loop {
        let drained = drain_commands(&commands, state, &mut watched);
        state = drained.state;
        watch_dirty |= drained.watch_changed;
        match state {
            RunState::Cancelled => break 'drive,
            RunState::Finishing => break 'drive,
            RunState::Paused => {
                // Block for the next command, but a `WatchNode` must answer
                // immediately (with a fresh snapshot) and keep blocking --
                // it does not count as "the next command" that ends the
                // pause.
                loop {
                    match commands.recv() {
                        Ok(WorkerCmd::WatchNode(target)) => {
                            watched = target;
                            if let Some(key) = watched {
                                send(WorkerEvent::NodeStrategies(Box::new(node_snapshot(
                                    &mw_session,
                                    key,
                                ))));
                                last_node_sent_at = Instant::now();
                                watch_dirty = false;
                            }
                        }
                        Ok(cmd) => {
                            state = apply_cmd(cmd, state);
                            break;
                        }
                        Err(_) => {
                            state = RunState::Cancelled;
                            break;
                        }
                    }
                }
                continue 'drive;
            }
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
                    let drained = drain_commands(&commands, state, &mut watched);
                    state = drained.state;
                    watch_dirty |= drained.watch_changed;
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

        if let Some(key) = watched
            && (watch_dirty || last_node_sent_at.elapsed().as_millis() >= 250)
        {
            send(WorkerEvent::NodeStrategies(Box::new(node_snapshot(
                &mw_session,
                key,
            ))));
            last_node_sent_at = Instant::now();
            watch_dirty = false;
        }

        let drained = drain_commands(&commands, state, &mut watched);
        state = drained.state;
        watch_dirty |= drained.watch_changed;
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
    // The rollout abstraction's assignment cache is pure memoization
    // (deterministic f(centroids, key)), so it only grows as the solve
    // visits more concrete rollout keys. Re-save it here so a later run
    // against the same `artifact_cache` path starts warm instead of
    // re-paying every cache miss this run already resolved. Unlike the CLI
    // (`multiway_solve.rs`), a failure here must not fail the whole run --
    // the GUI has already written the `.mwsol`/checkpoint successfully, so
    // this is only a lost warm-start optimization, not a lost result.
    if let Some(path) = mw_session.game_config.abstraction.artifact_cache.as_deref()
        && let Err(error) = mw_session
            .solver
            .game()
            .abstraction()
            .persist_assignment_cache(path)
    {
        eprintln!(
            "warning: could not persist rollout assignment cache {}: {error}",
            path.display()
        );
    }
    send(WorkerEvent::Finished(Box::new(FinishedRun {
        solution,
        mwsol_path: target.output_path,
    })));
}

/// Result of one [`drain_commands`] call. `watch_changed` is `true` iff a
/// `WatchNode` command updated `*watched` during this call; callers that
/// drain repeatedly before acting on it should accumulate with `|=` rather
/// than overwrite.
struct DrainedCommands {
    state: RunState,
    watch_changed: bool,
}

/// Drains every pending command without blocking. `WatchNode` is applied
/// directly to `*watched` here (never through `apply_cmd`) so it can never
/// perturb `RunState`.
fn drain_commands(
    commands: &Receiver<WorkerCmd>,
    mut state: RunState,
    watched: &mut Option<[u8; 16]>,
) -> DrainedCommands {
    let mut watch_changed = false;
    loop {
        match commands.try_recv() {
            Ok(WorkerCmd::WatchNode(target)) => {
                *watched = target;
                watch_changed = true;
            }
            Ok(cmd) => state = apply_cmd(cmd, state),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state = RunState::Cancelled;
                break;
            }
        }
    }
    DrainedCommands {
        state,
        watch_changed,
    }
}

/// Builds a [`NodeSnapshot`] for `history` from the solver's cold query
/// accessors. Called at most a few times per second (see the 250ms throttle
/// in `run`), so the `O(histories)`/`O(policies)` scans in `node_children`/
/// `strategies_at` are cheap relative to a whole chunk of sweeps.
fn node_snapshot(session: &MultiwaySession, history: [u8; 16]) -> NodeSnapshot {
    let key = HistoryKey(history);
    let children = session
        .solver
        .node_children(key)
        .into_iter()
        .map(|entry| NodeChild {
            key: entry.key.0,
            actor: entry.actor,
            action: entry.action_label,
        })
        .collect();
    let blocks = session
        .solver
        .strategies_at(key)
        .into_iter()
        .map(|(info_key, actions, probabilities)| NodeBlock {
            actor: info_key.player,
            street: info_key.street,
            active_opponents: info_key.active_opponents,
            bucket_path: info_key.bucket_path,
            actions,
            probabilities,
        })
        .collect();
    NodeSnapshot {
        history,
        sweeps: session.solver.completed_sweeps(),
        path: resolve_watch_path(session, key),
        children,
        blocks,
    }
}

/// Root-to-`history` path as "SEATNAME action" labels; empty at ROOT. Walks
/// `history_entry` parent links directly (rather than `resolve_history`,
/// which only carries action labels) so each edge can be prefixed with its
/// seat name.
fn resolve_watch_path(session: &MultiwaySession, mut history: HistoryKey) -> Vec<String> {
    let mut reversed = Vec::new();
    while history != HistoryKey::ROOT {
        let Some(entry) = session.solver.history_entry(history) else {
            break;
        };
        reversed.push(format!(
            "{} {}",
            seat_label(session, entry.actor),
            entry.action_label
        ));
        history = entry.parent;
    }
    reversed.reverse();
    reversed
}

fn seat_label(session: &MultiwaySession, actor: u8) -> String {
    session
        .game_config
        .seats
        .get(actor as usize)
        .and_then(|seat| seat.name.clone())
        .unwrap_or_else(|| format!("Seat {actor}"))
}

fn write_checkpoint(session: &MultiwaySession, path: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    MultiwayCheckpoint::capture(&session.solver).write_atomic(path)?;
    Ok(())
}
