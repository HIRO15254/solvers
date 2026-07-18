//! Background solve worker: owns a `MultiwaySession` on its own thread and
//! drives it in small, time-boxed chunks so the UI thread can observe
//! progress, pause, or cancel between chunks (see `docs/native-gui-plan.md`
//! section F, "worker protocol").

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use app_core::session::{self, MultiwaySession};
use eframe::egui;
use formats::{MultiwaySolution, MwsolStorage};
use multiway::checkpoint::MultiwayCheckpoint;
use multiway::solver::{HistoryKey, InfoKey, NodeActionEvaluation, ProfileEvaluation};

/// Target wall-clock duration of one drive-loop chunk. Chunks are sized from
/// the solver's own measured throughput (see `sweep_rate` in [`run`]) to hit
/// this cadence, so the UI sees a fresh [`ProgressSnapshot`] about once a
/// second regardless of how fast or slow the solve itself runs -- subject to
/// never crossing an evaluation/checkpoint boundary (see
/// `session::distance_to_boundary`).
const TARGET_CHUNK_SECS: f64 = 1.0;

/// EWMA smoothing factor for the adaptive chunk-size rate estimate: closer
/// to `1.0` reacts to a changing sweep rate faster; closer to `0.0` damps
/// out one chunk's timing jitter more. `0.3` was picked to settle within a
/// handful of chunks without overreacting to a single slow chunk (e.g. one
/// that happened to land right before a checkpoint write).
const RATE_EWMA_ALPHA: f64 = 0.3;

/// Wall-clock throttle for the watched-node [`NodeSnapshot`] refresh: about
/// 1 Hz -- live enough to watch strategies move without letting the
/// `O(node's buckets)` `strategies_at_with_mass` scan eat into drive-loop
/// time.
const NODE_SNAPSHOT_THROTTLE_MILLIS: u128 = 1_000;

/// Minimum interval between O(infosets) [`multiway::solver::MultiwaySolver::metrics`]
/// refreshes backing a [`ProgressSnapshot`]'s infoset/memory readout: at
/// most once every 10 seconds. The effective interval is adaptive: `max` of
/// this floor and [`METRICS_COST_MULTIPLIER`] times the last scan's own
/// measured duration, so the scan can never consume more than
/// ~1/[`METRICS_COST_MULTIPLIER`] of wall time no matter how large the
/// arena is.
const METRICS_THROTTLE_MILLIS: u64 = 10_000;

/// See [`METRICS_THROTTLE_MILLIS`]: caps the metrics scan's share of wall
/// time at roughly `1 / METRICS_COST_MULTIPLIER` (at most 1% of solve
/// time).
const METRICS_COST_MULTIPLIER: u32 = 100;

#[derive(Debug, Clone)]
pub enum WorkerCmd {
    Pause,
    Resume,
    /// Stop taking new sweeps and save whatever the profile is now.
    Finish,
    /// Stop and discard; no artifact is written.
    Cancel,
    /// Start (or, with `None`, stop) watching one public-history node for
    /// live average-strategy snapshots. Carries the raw history key rather
    /// than `HistoryKey` so the command stays independent of the multiway
    /// crate's internal type. Does not affect [`RunState`]: the worker keeps
    /// running/paused exactly as before.
    WatchNode(Option<[u8; 16]>),
    /// Evaluate per-hand-group, per-action EVs at the node reached by
    /// replaying `path` (action indices from the root) against the current
    /// average profile. Serviced between drive-loop chunks (and immediately
    /// while paused): the worker never runs `evaluate_node_actions`
    /// concurrently with a sweep batch. Replies with
    /// [`WorkerEvent::NodeEvaluation`] or [`WorkerEvent::NodeEvaluationFailed`],
    /// each carrying `path` back so a view showing a different node by the
    /// time the reply arrives can drop it.
    EvaluateNode {
        path: Vec<usize>,
        samples: u64,
    },
}

#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub sweeps: u64,
    pub target: u64,
    pub elapsed_secs: f64,
    pub sweeps_per_sec: f64,
    /// Cumulative individual player traversals so far this run (see
    /// `multiway::solver::MultiwaySolver::traversals`).
    pub traversals: u64,
    pub traversals_per_sec: f64,
    /// Cumulative individual hand updates so far this run -- the headline
    /// throughput number (see
    /// `multiway::solver::MultiwaySolver::hand_updates`).
    pub hand_updates: u64,
    pub hand_updates_per_sec: f64,
    pub infosets: u64,
    pub memory_bytes: u64,
    pub seat_avg_pos_regret: Vec<f64>,
    pub seat_drift_l1: Vec<f64>,
    /// Live wall-clock countdown to the next convergence-stop-rule
    /// evaluation, reported on every chunk while `run.stop_dev_gain` is set
    /// (`None` otherwise). [`StopRuleProgress`] (carried on the rarer
    /// [`WorkerEvent::Evaluated`]) reports the rule's actual pass/fail
    /// state; this only drives the Solve tab's between-evaluations
    /// countdown.
    pub stop_rule: Option<StopRuleClock>,
}

/// Wall-clock countdown to the next stop-rule evaluation; see
/// [`ProgressSnapshot::stop_rule`].
#[derive(Debug, Clone)]
pub struct StopRuleClock {
    pub threshold: f64,
    pub confirmations_required: u32,
    pub next_eval_in_secs: f64,
}

/// Convergence-stop-rule progress as of the evaluation that produced this
/// [`WorkerEvent::Evaluated`] -- `Some` only when that evaluation was itself
/// a stop-rule check (`run.stop_dev_gain`'s own wall-clock cadence), `None`
/// for an ordinary evaluation-cadence evaluation.
#[derive(Debug, Clone)]
pub struct StopRuleProgress {
    /// Maximum per-seat `deviation_gain_lower_bound` CI upper bound this
    /// evaluation measured.
    pub max_ci_upper: f64,
    pub threshold: f64,
    pub confirmations: u32,
    pub confirmations_required: u32,
    /// Current adaptive sample count (doubles, capped at
    /// [`app_core::session::MAX_STOP_RULE_SAMPLES`], whenever the CI is too wide
    /// to ever settle below `threshold`).
    pub sample_count: u64,
}

/// Payload of [`WorkerEvent::Evaluated`]: the held-out profile evaluation
/// plus (when this evaluation was a stop-rule check) its convergence
/// progress.
#[derive(Debug)]
pub struct EvaluationEvent {
    pub evaluation: ProfileEvaluation,
    pub stop_rule: Option<StopRuleProgress>,
}

/// How a finished run stopped taking new sweeps. See
/// `app_core::multiway_solve::CompletionStatus`, whose `Converged` variant this
/// mirrors for the GUI drive loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FinishedOutcome {
    /// `run.sweeps` (or the Auto-mode safety-cap sweep count) was reached.
    TargetReached,
    /// The user clicked "Finish & Save" before either of the other outcomes.
    UserRequested,
    /// The convergence stop rule (`run.stop_dev_gain`) fired: the maximum
    /// per-seat deviation-gain CI upper bound stayed below `threshold` for
    /// `confirmations` consecutive wall-clock-spaced evaluations.
    Converged {
        max_ci_upper: f64,
        threshold: f64,
        confirmations: u32,
    },
    /// The Auto/Advanced max-wall-time cap elapsed.
    WallTimeCap,
}

#[derive(Debug)]
pub struct FinishedRun {
    pub solution: MultiwaySolution,
    pub mwsol_path: PathBuf,
    pub outcome: FinishedOutcome,
}

/// One child edge out of a watched node, for the "Live node view" button row.
#[derive(Debug, Clone)]
pub struct NodeChild {
    pub key: [u8; 16],
    pub actor: u8,
    pub action_index: usize,
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
    /// This column's linear-CFR reach-weighted strategy mass (see
    /// `multiway::solver::MultiwaySolver::strategies_at_with_mass`) -- the
    /// weight the live range-wide action-frequency aggregate uses.
    pub mass: f64,
}

/// Live snapshot of one watched public-history node, rebuilt from the
/// solver's cold query accessors (`node_children`/`strategies_at_with_mass`)
/// at UI refresh rate.
#[derive(Debug, Clone)]
pub struct NodeSnapshot {
    pub history: [u8; 16],
    pub sweeps: u64,
    /// Root-to-current path, one "SEATNAME action" entry per edge; empty at
    /// ROOT.
    pub path: Vec<String>,
    /// The same path as `path`, but as the action indices `EvaluateNode`
    /// expects -- the GUI carries this alongside the human-readable path
    /// instead of re-deriving it from a separately tracked breadcrumb, so
    /// the "Evaluate EVs" button always targets exactly the node this
    /// snapshot describes.
    pub action_path: Vec<usize>,
    pub children: Vec<NodeChild>,
    pub blocks: Vec<NodeBlock>,
}

/// A completed [`WorkerCmd::EvaluateNode`] request.
#[derive(Debug)]
pub struct NodeEvaluationResult {
    pub path: Vec<usize>,
    pub evaluation: NodeActionEvaluation,
}

pub enum WorkerEvent {
    /// Sent once, before the (potentially slow) card abstraction/game build.
    Building,
    Progress(ProgressSnapshot),
    Evaluated(EvaluationEvent),
    /// Live average strategy at a watched node; see [`WorkerCmd::WatchNode`].
    NodeStrategies(Box<NodeSnapshot>),
    /// Reply to [`WorkerCmd::EvaluateNode`].
    NodeEvaluation(Box<NodeEvaluationResult>),
    /// `evaluate_node_actions` itself failed for a requested path (e.g. it
    /// pointed past the node's legal action count by the time it was
    /// serviced). Non-fatal: unlike [`WorkerEvent::Failed`], the solve keeps
    /// running.
    NodeEvaluationFailed {
        path: Vec<usize>,
        error: String,
    },
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
    /// GUI-only wall-clock cap (Auto mode's max-wall-time field, or
    /// Advanced's optional Run field); never part of the config TOML. When
    /// set, the worker finishes through the same graceful path a "Finish &
    /// Save" click uses (solution + `.mwsol` + final checkpoint written),
    /// stamped with `FinishedOutcome::WallTimeCap`.
    pub max_wall_time_secs: Option<f64>,
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
        // need the target/session itself; never changes `RunState`.
        WorkerCmd::WatchNode(_) | WorkerCmd::EvaluateNode { .. } => state,
    }
}

/// Deterministic seed for one `EvaluateNode` request: a function of the
/// solve's own algorithm seed and the requested path, so repeating the same
/// request (the same node, re-clicked) reproduces the same estimate instead
/// of fresh Monte Carlo noise every time. Exposed for the Results tab's
/// background evaluation, which needs the same reproducibility property
/// against a loaded `.mwsol`'s own `[algorithm] seed`.
pub fn node_eval_seed(run_seed: u64, path: &[usize]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.gui.node_eval.seed.v1");
    hasher.update(&run_seed.to_le_bytes());
    for &action_index in path {
        hasher.update(&(action_index as u64).to_le_bytes());
    }
    u64::from_le_bytes(
        hasher.finalize().as_bytes()[..8]
            .try_into()
            .expect("hash output is at least 8 bytes"),
    )
}

/// Rounds `sweeps` to the nearest multiple of `sweep_batch` (never to zero),
/// so the adaptive chunk size lines up with the solver's own internal
/// sweep-batch granularity instead of splitting one batch across two chunks.
/// `sweep_batch <= 1` (no batching) is a no-op past clamping to at least `1`.
fn round_to_sweep_batch(sweeps: u64, sweep_batch: u64) -> u64 {
    if sweep_batch <= 1 {
        return sweeps.max(1);
    }
    let rounded = (sweeps + sweep_batch / 2) / sweep_batch * sweep_batch;
    rounded.max(sweep_batch)
}

/// Runs one queued [`WorkerCmd::EvaluateNode`] request against `session`'s
/// current average profile and turns the result into the matching
/// [`WorkerEvent`].
fn evaluate_node(session: &MultiwaySession, path: &[usize], samples: u64) -> WorkerEvent {
    let seed = node_eval_seed(session.solver.config().seed, path);
    match session.solver.evaluate_node_actions(path, samples, seed) {
        Ok(evaluation) => WorkerEvent::NodeEvaluation(Box::new(NodeEvaluationResult {
            path: path.to_vec(),
            evaluation,
        })),
        Err(error) => WorkerEvent::NodeEvaluationFailed {
            path: path.to_vec(),
            error: error.to_string(),
        },
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
    // Resumed checkpoints start with non-zero cumulative counters; every
    // rate below must only count what this run itself produced.
    let initial_sweeps = mw_session.solver.completed_sweeps();
    let initial_traversals = mw_session.solver.traversals();
    let initial_hand_updates = mw_session.solver.hand_updates();
    // EWMA estimate of sweeps/sec, used to size each chunk to roughly
    // `TARGET_CHUNK_SECS`; `0.0` means "no measurement yet", handled by
    // bootstrapping the first chunk at one sweep-batch.
    let mut sweep_rate = 0.0f64;
    // Seed `prior` with the current averages (drift result discarded).
    let mut prior: HashMap<InfoKey, Vec<f32>> = HashMap::new();
    mw_session.solver.strategy_drift_refresh(&mut prior);
    let mut last_drift = vec![0.0; mw_session.game_config.seats.len()];
    // Full SolverMetrics costs O(infosets) -- on a large dense arena a
    // single scan can take longer than a whole solve chunk, so a fixed
    // refresh cadence would let this bookkeeping dominate wall time. The
    // interval adapts to the measured scan cost instead: at least
    // `METRICS_THROTTLE_MILLIS`, and never more often than
    // `METRICS_MAX_DUTY.recip()` of wall time (20x the last scan's own
    // duration), so the readout stays fresh on small games and caps at ~5%
    // overhead on huge ones. In-between progress frames reuse the last
    // reading; evaluation boundaries always refresh.
    let metrics_started = Instant::now();
    let mut last_metrics = mw_session.solver.metrics();
    let mut metrics_interval = (metrics_started.elapsed() * METRICS_COST_MULTIPLIER)
        .max(Duration::from_millis(METRICS_THROTTLE_MILLIS));
    let mut last_metrics_at = Instant::now();
    // Live node-strategy view (see `WorkerCmd::WatchNode`): `watch_dirty`
    // accumulates across every `drain_commands` call since the last
    // `NodeSnapshot` send, so a target change is never silently absorbed by
    // the refresh throttle below.
    let mut watched: Option<[u8; 16]> = None;
    let mut watch_dirty = false;
    let mut last_node_sent_at = Instant::now();
    // `EvaluateNode` requests drained while a sweep batch is in flight (the
    // `should_continue` closure below cannot borrow `mw_session` -- it is
    // already mutably borrowed by the call driving it) are queued here and
    // serviced as soon as the current chunk returns.
    let mut pending_evaluations: Vec<(Vec<usize>, u64)> = Vec::new();
    // Convergence stop rule (`run.stop_dev_gain`), shared with the CLI drive
    // loop via `app_core::session::{StopRuleState, run_stop_rule_check}`. `None`
    // in `mw_session.stop_rule` means the rule is disabled, in which case
    // this never gets read.
    let mut stop_rule_state = session::StopRuleState::new(mw_session.evaluation_samples);
    // How this run finished; overwritten only by the three non-default
    // outcomes below, so a plain "ran out of sweeps" completion needs no
    // explicit assignment.
    let mut outcome = FinishedOutcome::TargetReached;

    'drive: loop {
        let drained = drain_commands(&commands, state, &mut watched, &mut pending_evaluations);
        state = drained.state;
        watch_dirty |= drained.watch_changed;
        match state {
            RunState::Cancelled => break 'drive,
            RunState::Finishing => {
                outcome = FinishedOutcome::UserRequested;
                break 'drive;
            }
            RunState::Paused => {
                for (path, samples) in pending_evaluations.drain(..) {
                    send(evaluate_node(&mw_session, &path, samples));
                }
                // Block for the next command, but `WatchNode`/`EvaluateNode`
                // must answer immediately and keep blocking -- neither
                // counts as "the next command" that ends the pause.
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
                        Ok(WorkerCmd::EvaluateNode { path, samples }) => {
                            send(evaluate_node(&mw_session, &path, samples));
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

        if let Some(max_secs) = target.max_wall_time_secs
            && started.elapsed().as_secs_f64() >= max_secs
        {
            outcome = FinishedOutcome::WallTimeCap;
            break 'drive;
        }

        for (path, samples) in pending_evaluations.drain(..) {
            send(evaluate_node(&mw_session, &path, samples));
        }

        let current = mw_session.solver.completed_sweeps();
        if current >= mw_session.sweeps_target {
            break 'drive;
        }
        let remaining = mw_session.sweeps_target - current;
        // Never step across an evaluation or checkpoint boundary: the
        // cadence gates below use `sweeps % cadence == 0` and would silently
        // starve on misaligned counts (e.g. after a mid-chunk pause).
        let evaluation_delta =
            session::distance_to_boundary(current, mw_session.evaluation_cadence);
        let checkpoint_delta = mw_session
            .checkpoint_every
            .map(|cadence| session::distance_to_boundary(current, cadence))
            .unwrap_or(u64::MAX);
        let boundary_distance = remaining.min(evaluation_delta).min(checkpoint_delta).max(1);
        let sweep_batch = mw_session.solver.config().sweep_batch.max(1);
        // Adaptive time-based chunk size: aim for `TARGET_CHUNK_SECS` of
        // work at the currently measured rate, rounded to a whole number of
        // sweep-batches, but never past the next cadence boundary. `chunk`
        // sizing never changes any result -- any chunking aligned to
        // `sweep_batch` is bit-identical, since `run_sweeps_with_threads_until`
        // itself re-batches internally regardless of the `sweeps` argument.
        let target_sweeps = if sweep_rate > 0.0 {
            round_to_sweep_batch((sweep_rate * TARGET_CHUNK_SECS).round() as u64, sweep_batch)
        } else {
            sweep_batch
        };
        let chunk = target_sweeps.min(boundary_distance).max(1);

        let chunk_started = Instant::now();
        let result =
            mw_session
                .solver
                .run_sweeps_with_threads_until(chunk, mw_session.threads, || {
                    let drained =
                        drain_commands(&commands, state, &mut watched, &mut pending_evaluations);
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
        let chunk_elapsed = chunk_started.elapsed().as_secs_f64();

        let sweeps_now = mw_session.solver.completed_sweeps();
        let sweeps_this_chunk = sweeps_now.saturating_sub(current);
        if chunk_elapsed > 0.0 && sweeps_this_chunk > 0 {
            let instantaneous = sweeps_this_chunk as f64 / chunk_elapsed;
            sweep_rate = if sweep_rate > 0.0 {
                RATE_EWMA_ALPHA * instantaneous + (1.0 - RATE_EWMA_ALPHA) * sweep_rate
            } else {
                instantaneous
            };
        }

        for (path, samples) in pending_evaluations.drain(..) {
            send(evaluate_node(&mw_session, &path, samples));
        }

        let at_evaluation = sweeps_now % mw_session.evaluation_cadence == 0;
        if at_evaluation || last_metrics_at.elapsed() >= metrics_interval {
            let metrics_started = Instant::now();
            last_metrics = mw_session.solver.metrics();
            metrics_interval = (metrics_started.elapsed() * METRICS_COST_MULTIPLIER)
                .max(Duration::from_millis(METRICS_THROTTLE_MILLIS));
            last_metrics_at = Instant::now();
        }
        let metrics = &last_metrics;
        if at_evaluation {
            last_drift = mw_session.solver.strategy_drift_refresh(&mut prior);
            match mw_session
                .solver
                .evaluate_average_profile(mw_session.evaluation_samples, mw_session.evaluation_seed)
            {
                Ok(evaluation) => send(WorkerEvent::Evaluated(EvaluationEvent {
                    evaluation,
                    stop_rule: None,
                })),
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

        // Convergence stop rule: an ADDITIONAL trigger layered on top of the
        // cadence-based evaluation/checkpoint boundaries above, never a
        // replacement for them. It fires on wall-clock time rather than a
        // sweep boundary, so it is checked once per drive-loop chunk
        // regardless of where `sweeps_now` falls relative to
        // `evaluation_cadence`/`checkpoint_every`. The check itself (the
        // best-response burst, the evaluation, threshold/width bookkeeping,
        // adaptive sample doubling, and the confirmations update) is shared
        // with the CLI drive loop via `app_core::session::run_stop_rule_check`;
        // this worker keeps only the wall-clock trigger, event-sending, and
        // the loop-break decision.
        let mut stop_rule_clock: Option<StopRuleClock> = None;
        if let Some(stop_rule) = mw_session.stop_rule {
            if stop_rule_state.last_eval.elapsed().as_secs_f64() >= stop_rule.eval_period_secs {
                let check = match session::run_stop_rule_check(
                    &mw_session.solver,
                    &stop_rule,
                    &mut stop_rule_state,
                    mw_session.game_config.seats.len(),
                    mw_session.threads,
                    mw_session.evaluation_seed,
                ) {
                    Ok(check) => check,
                    Err(error) => {
                        send(WorkerEvent::Failed(format!("{error:#}")));
                        return;
                    }
                };
                // Unlike the evaluation-cadence block above, this does not
                // refresh `last_metrics`/`last_metrics_at`: the GUI worker
                // has no metrics-row writer for a stop-rule check to feed
                // (see `app_core::multiway_solve::run_inner`, which does), so
                // there is nothing here that needs a fresh `SolverMetrics`
                // scan.
                last_drift = mw_session.solver.strategy_drift_refresh(&mut prior);

                send(WorkerEvent::Evaluated(EvaluationEvent {
                    evaluation: check.evaluation,
                    stop_rule: Some(StopRuleProgress {
                        max_ci_upper: check.max_upper,
                        threshold: stop_rule.dev_gain_threshold,
                        confirmations: stop_rule_state.confirmations_met,
                        confirmations_required: stop_rule.confirmations,
                        sample_count: stop_rule_state.samples,
                    }),
                }));

                if check.converged {
                    outcome = FinishedOutcome::Converged {
                        max_ci_upper: check.max_upper,
                        threshold: stop_rule.dev_gain_threshold,
                        confirmations: stop_rule_state.confirmations_met,
                    };
                    break 'drive;
                }
            }
            stop_rule_clock = Some(StopRuleClock {
                threshold: stop_rule.dev_gain_threshold,
                confirmations_required: stop_rule.confirmations,
                next_eval_in_secs: (stop_rule.eval_period_secs
                    - stop_rule_state.last_eval.elapsed().as_secs_f64())
                .max(0.0),
            });
        }

        let elapsed = started.elapsed().as_secs_f64();
        let traversals_now = mw_session.solver.traversals();
        let hand_updates_now = mw_session.solver.hand_updates();
        send(WorkerEvent::Progress(ProgressSnapshot {
            sweeps: sweeps_now,
            target: mw_session.sweeps_target,
            elapsed_secs: elapsed,
            sweeps_per_sec: if elapsed > 0.0 {
                (sweeps_now - initial_sweeps) as f64 / elapsed
            } else {
                0.0
            },
            traversals: traversals_now,
            traversals_per_sec: if elapsed > 0.0 {
                (traversals_now - initial_traversals) as f64 / elapsed
            } else {
                0.0
            },
            hand_updates: hand_updates_now,
            hand_updates_per_sec: if elapsed > 0.0 {
                (hand_updates_now - initial_hand_updates) as f64 / elapsed
            } else {
                0.0
            },
            infosets: metrics.infosets,
            memory_bytes: metrics.memory_bytes,
            seat_avg_pos_regret: metrics.average_positive_regret.clone(),
            seat_drift_l1: last_drift.clone(),
            stop_rule: stop_rule_clock,
        }));

        if let Some(key) = watched
            && (watch_dirty
                || last_node_sent_at.elapsed().as_millis() >= NODE_SNAPSHOT_THROTTLE_MILLIS)
        {
            send(WorkerEvent::NodeStrategies(Box::new(node_snapshot(
                &mw_session,
                key,
            ))));
            last_node_sent_at = Instant::now();
            watch_dirty = false;
        }

        let drained = drain_commands(&commands, state, &mut watched, &mut pending_evaluations);
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
        app_core::config::StorageKind::F32 => MwsolStorage::F32,
        app_core::config::StorageKind::I16 => MwsolStorage::I16,
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
    // `.rollout()` is `None` for the ehs2-table backend, whose content is
    // already fully determined (and disk-cached) at build time.
    if let Some(path) = mw_session.game_config.abstraction.artifact_cache.as_deref()
        && let Some(rollout) = mw_session.solver.game().abstraction().rollout()
        && let Err(error) = rollout.persist_assignment_cache(path)
    {
        eprintln!(
            "warning: could not persist rollout assignment cache {}: {error}",
            path.display()
        );
    }
    send(WorkerEvent::Finished(Box::new(FinishedRun {
        solution,
        mwsol_path: target.output_path,
        outcome,
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
/// perturb `RunState`; `EvaluateNode` requests are appended to
/// `*evaluations` (the caller services them once it is safe to borrow the
/// session again -- see the comment on `pending_evaluations` in [`run`]).
fn drain_commands(
    commands: &Receiver<WorkerCmd>,
    mut state: RunState,
    watched: &mut Option<[u8; 16]>,
    evaluations: &mut Vec<(Vec<usize>, u64)>,
) -> DrainedCommands {
    let mut watch_changed = false;
    loop {
        match commands.try_recv() {
            Ok(WorkerCmd::WatchNode(target)) => {
                *watched = target;
                watch_changed = true;
            }
            Ok(WorkerCmd::EvaluateNode { path, samples }) => {
                evaluations.push((path, samples));
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
/// accessors. Called at most a few times per second (see the refresh
/// throttle in `run`), so the `O(histories)`/`O(node's buckets)` scans in
/// `node_children`/`strategies_at_with_mass` are cheap relative to a whole
/// chunk of sweeps.
fn node_snapshot(session: &MultiwaySession, history: [u8; 16]) -> NodeSnapshot {
    let key = HistoryKey(history);
    let children = session
        .solver
        .node_children(key)
        .into_iter()
        .map(|entry| NodeChild {
            key: entry.key.0,
            actor: entry.actor,
            action_index: entry.action_index as usize,
            action: entry.action_label,
        })
        .collect();
    let blocks = session
        .solver
        .strategies_at_with_mass(key)
        .into_iter()
        .map(|(info_key, actions, probabilities, mass)| NodeBlock {
            actor: info_key.player,
            street: info_key.street,
            active_opponents: info_key.active_opponents,
            bucket_path: info_key.bucket_path,
            actions,
            probabilities,
            mass,
        })
        .collect();
    let (path, action_path) = resolve_watch_path(session, key);
    NodeSnapshot {
        history,
        sweeps: session.solver.completed_sweeps(),
        path,
        action_path,
        children,
        blocks,
    }
}

/// Root-to-`history` path as "SEATNAME action" labels (empty at ROOT) and
/// the parallel action-index path `EvaluateNode` expects. Walks
/// `history_entry` parent links directly (rather than `resolve_history`,
/// which only carries action labels) so each edge can be prefixed with its
/// seat name and its own action index recovered.
fn resolve_watch_path(
    session: &MultiwaySession,
    mut history: HistoryKey,
) -> (Vec<String>, Vec<usize>) {
    let mut reversed_labels = Vec::new();
    let mut reversed_actions = Vec::new();
    while history != HistoryKey::ROOT {
        let Some(entry) = session.solver.history_entry(history) else {
            break;
        };
        reversed_labels.push(format!(
            "{} {}",
            seat_label(session, entry.actor),
            entry.action_label
        ));
        reversed_actions.push(entry.action_index as usize);
        history = entry.parent;
    }
    reversed_labels.reverse();
    reversed_actions.reverse();
    (reversed_labels, reversed_actions)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_to_sweep_batch_rounds_to_nearest_multiple_but_never_to_zero() {
        assert_eq!(round_to_sweep_batch(0, 1), 1);
        assert_eq!(round_to_sweep_batch(7, 1), 7);
        assert_eq!(round_to_sweep_batch(0, 8), 8);
        assert_eq!(round_to_sweep_batch(3, 8), 8);
        assert_eq!(round_to_sweep_batch(5, 8), 8);
        assert_eq!(round_to_sweep_batch(12, 8), 16);
        // 100 is equidistant between 96 and 104; the `+ sweep_batch / 2`
        // round-half-up rule picks 104.
        assert_eq!(round_to_sweep_batch(100, 8), 104);
        assert_eq!(round_to_sweep_batch(97, 8), 96);
    }

    #[test]
    fn node_eval_seed_is_deterministic_and_path_sensitive() {
        let a = node_eval_seed(7, &[0, 1]);
        let b = node_eval_seed(7, &[0, 1]);
        let c = node_eval_seed(7, &[0, 2]);
        let d = node_eval_seed(9, &[0, 1]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
