//! P1 solve and read-only solve drivers. The caller owns cancellation and run lifecycle.
use crate::index::GameIndex;
use crate::input::{self, Algorithm, SolutionStreets};
use crate::prepare::{self, Prepared, compatibility_hash, warnings};
use anyhow::{Context, Result, bail};
use hu_engine::{
    CompiledGame, DiscountSchedule, F32Storage, I16Storage, MixedStorage, ParConfig, Solver,
    Storage, TerminalEvaluator,
};
use nlh::{PerPlayer, Player};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Progress and durable boundaries from a P1 solve segment.
pub enum Observation {
    ProgressOpened,
    Progress(runfiles::MetricsRow),
    /// Frequency-target diagnostics at the same evaluation boundary as Progress.
    ProfileProgress {
        iteration: u64,
        frequencies: Vec<FrequencyMetric>,
    },
    Checkpoint {
        iterations: u64,
    },
    Stop {
        reason: &'static str,
    },
}
/// Display diagnostics emitted at their original computation boundaries.
pub enum Diagnostic {
    Memory(crate::MemoryEstimate),
    Warning(String),
    Done(RunSummary),
    Artifact(crate::artifact::Diagnostic),
}
/// Caller-owned progress persistence and event handling.
pub type Observer<'a> = dyn FnMut(Observation) -> Result<()> + Send + 'a;
/// Caller-owned diagnostic rendering.
pub type Diagnostics<'a> = dyn FnMut(Diagnostic) + Send + 'a;
/// Explicit artifact destinations and continuation state for one P1 segment.
pub struct RunRequest<'a> {
    pub prepared: &'a Prepared,
    pub checkpoint: &'a Path,
    pub solution: &'a Path,
    pub state: Option<crate::checkpoint::CheckpointReader>,
    /// Evaluations through the restored checkpoint, as (iteration, NashConv / 2).
    pub evaluation_history: Vec<(u64, f64)>,
    pub elapsed_before: Duration,
    pub cancel: &'a AtomicBool,
}

// PF10: all scheduling constants live here, independent of execution resources.
const BASE_CHECK_STEP: u64 = 25;
const MIN_CHECK_STEP: u64 = 3;
const MAX_CHECK_STEP: u64 = 50;
const PREDICTION_FRACTION: f64 = 0.8;
const MIN_CONVERGENCE_SLOPE: f64 = 0.05;

/// Deterministic evaluation schedule. `history` is sorted, with unique iterations.
/// A checkpoint between evaluations keeps the schedule anchored at the last check.
pub fn next_evaluation(
    history: &[(u64, f64)],
    target: Option<f64>,
    current: u64,
    max_iterations: u64,
    check_every: input::CheckEvery,
) -> u64 {
    if let input::CheckEvery::Fixed(step) = check_every {
        return current.saturating_add(step).min(max_iterations);
    }
    let base = history.last().map_or(current, |&(t, _)| t);
    let mut step = BASE_CHECK_STEP;
    if let Some(target) = target
        && let [.., (t0, v0), (t1, v1)] = history
        && *t0 > 0
        && t1 > t0
        && v0.is_finite()
        && v1.is_finite()
        && *v0 > 0.0
        && *v1 > 0.0
        && target.is_finite()
        && target > 0.0
    {
        let slope = (v0 / v1).ln() / (*t1 as f64 / *t0 as f64).ln();
        if slope.is_finite() && slope > MIN_CONVERGENCE_SLOPE {
            let prediction = *t1 as f64 * (v1 / target).powf(1.0 / slope);
            let predicted_step = (PREDICTION_FRACTION * (prediction - *t1 as f64)).ceil();
            if predicted_step.is_finite() {
                step = predicted_step.clamp(MIN_CHECK_STEP as f64, MAX_CHECK_STEP as f64) as u64;
            }
        }
    }
    base.saturating_add(step).max(current).min(max_iterations)
}

/// Reconstruct the append-only progress history; the last row wins for duplicates.
/// Missing, unreadable or invalid complete records fall back to no history;
/// an incomplete final record is ignored.
pub fn read_evaluation_history(path: &Path, checkpoint_iteration: u64) -> Vec<(u64, f64)> {
    // The schedule must see the exact in-memory values, so NashConv is parsed
    // with Rust's correctly rounded f64 parser rather than serde_json's.
    #[derive(serde::Deserialize)]
    struct Row<'a> {
        iteration: u64,
        #[serde(borrow)]
        nash_conv: &'a serde_json::value::RawValue,
    }
    fn read(path: &Path, checkpoint_iteration: u64) -> Result<Vec<(u64, f64)>> {
        use std::io::BufRead;
        let mut rows = std::collections::BTreeMap::new();
        let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line)? != 0 {
            if line.last() != Some(&b'\n') {
                break;
            }
            let row: Row<'_> = serde_json::from_slice(&line)?;
            if row.iteration <= checkpoint_iteration {
                let nash_conv: f64 = row.nash_conv.get().parse()?;
                rows.insert(row.iteration, nash_conv / 2.0);
            }
            line.clear();
        }
        Ok(rows.into_iter().collect())
    }
    read(path, checkpoint_iteration).unwrap_or_default()
}

fn batch_step(check_every: input::CheckEvery, remaining: u64) -> u64 {
    match check_every {
        input::CheckEvery::Auto => remaining.min(BASE_CHECK_STEP),
        input::CheckEvery::Fixed(_) => remaining,
    }
}
/// Build/restore and solve P1, writing checkpoint and solution artifacts and returning convergence data.
pub fn run(
    request: RunRequest<'_>,
    observer: &mut Observer<'_>,
    diagnostics: &mut Diagnostics<'_>,
) -> Result<RunSummary> {
    let threads = prepare::threads(request.prepared)?;
    with_threads(threads, || match request.prepared.settings.solver.storage {
        input::Storage::F32 => drive::<F32Storage>(request, observer, diagnostics),
        input::Storage::I16 => drive::<I16Storage>(request, observer, diagnostics),
        input::Storage::I16F32Avg => drive::<MixedStorage>(request, observer, diagnostics),
    })
}

type QueryResult<S> = (
    Solver<crate::PostflopEvaluator, S>,
    GameIndex,
    (PerPlayer<f64>, PerPlayer<f64>),
    Option<ProfileReport>,
);

pub(crate) fn query<S: Storage>(
    p: &Prepared,
    iterations: Option<u64>,
    target_nash_conv: Option<f64>,
    cancel: &AtomicBool,
) -> Result<QueryResult<S>> {
    crate::prepare::check_memory_limit(p)?;
    let iterations = iterations.unwrap_or(p.settings.solver.stop.max_iterations);
    if iterations == 0 || target_nash_conv.is_some_and(|t| !t.is_finite() || t < 0.0) {
        bail!("NLH003: iterations must be positive and target_nash_conv finite and non-negative");
    }
    let (game, index, resolved, base) = build_profiled_with_base(p)?;
    let mut solver = Solver::<_, S>::new(
        game,
        schedule(&p.settings.solver.algorithm),
        Some(iterations),
    );
    solver.set_cfr_precision(p.settings.solver.cfr_precision);
    let mut controller = install_profile(&mut solver, resolved, None)?;
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    let target = target_nash_conv.map(|t| t / 2.0).or(p.target);
    let (subtree, _) = crate::subtree::prepare(
        &mut solver,
        &index,
        controller.summary.as_ref(),
        !controller.targets.is_empty(),
        base.as_ref(),
        target,
    )?;
    let start = Instant::now();
    drop(base);
    let check_every = p.settings.solver.stop.check_every;
    let mut history = Vec::new();
    let mut evaluated = None;
    let mut next_check = next_evaluation(&history, target, 0, iterations, check_every);
    while solver.iteration() < iterations {
        if cancel.load(std::sync::atomic::Ordering::SeqCst)
            || p.document
                .spot
                .run
                .max_time_seconds
                .is_some_and(|t| start.elapsed().as_secs_f64() >= t)
        {
            break;
        }
        let steps = batch_step(check_every, next_check - solver.iteration());
        if let Some(subtree) = &subtree {
            subtree.step(&mut solver, steps);
        } else {
            solver.run(steps);
        }
        evaluated = None;
        if solver.iteration() == next_check {
            controller.update(&mut solver)?;
            if let Some(target) = target {
                let (ev, expl) = subtree
                    .as_ref()
                    .map_or_else(|| solver.evaluate(), |s| s.evaluate(&solver));
                evaluated = Some((ev, expl));
                let value = (expl[Player::P0] + expl[Player::P1]) / 2.0;
                history.push((solver.iteration(), value));
                if value <= target {
                    break;
                }
            }
            next_check = next_evaluation(
                &history,
                target,
                solver.iteration(),
                iterations,
                check_every,
            );
        }
    }
    let (ev, expl) = if subtree.is_some() {
        solver.evaluate()
    } else {
        evaluated.unwrap_or_else(|| solver.evaluate())
    };
    let evaluation = if solver.profile().is_some() {
        (solver.true_expected_values(), expl)
    } else {
        (ev, expl)
    };
    let report = controller.report(&solver, p);
    Ok((solver, index, evaluation, report))
}

/// Builds the game and its viewer index, releasing the builder's per-node
/// strings before the solve allocates storage.
pub(crate) fn build_indexed(
    p: &Prepared,
) -> Result<(CompiledGame<crate::PostflopEvaluator>, GameIndex)> {
    let (game, index, _) = build_profiled(p)?;
    Ok((game, index))
}
/// Resolve profile selectors against the actual tree for validation.
pub fn validate_profile(p: &Prepared) -> Result<Option<crate::profile::ProfileSummary>> {
    Ok(build_profiled(p)?.2.map(|profile| profile.summary))
}

/// Build the game and resolve any configured profile against its tree.
pub fn build_profiled(
    p: &Prepared,
) -> Result<(
    CompiledGame<crate::PostflopEvaluator>,
    GameIndex,
    Option<crate::profile::ResolvedProfile>,
)> {
    let (game, index, profile, _) = build_profiled_with_base(p)?;
    Ok((game, index, profile))
}

type ProfiledWithBase = (
    CompiledGame<crate::PostflopEvaluator>,
    GameIndex,
    Option<crate::profile::ResolvedProfile>,
    Option<crate::artifact::LoadedSol>,
);

fn build_profiled_with_base(p: &Prepared) -> Result<ProfiledWithBase> {
    let mut built = crate::try_build_postflop_game(&p.config, p.payoff.pipeline())
        .map_err(|e| spot::SpotError::new(spot::Code::NLH003, "tree", e.to_string()))?;
    let start = crate::artifact::start_street_from_board_len(p.config.board.len());
    let index = GameIndex::from_game(&built.game, &built.node_info, start);
    let mut loaded_base = None;
    let resolved = if let Some(ir) = &p.document.spot.profile {
        if !matches!(
            p.document.spot.economics.utility,
            economics::UtilityConfig::ChipEv
        ) && (!ir.incentives.is_empty() || !ir.frequencies.is_empty())
        {
            return Err(spot::SpotError::new(
                spot::Code::NLH003,
                "profile",
                "incentives and frequency targets require chip EV",
            )
            .into());
        }
        let mut base_strategies = Vec::new();
        if let Some(freeze) = &ir.freeze_upstream {
            let mut input = std::fs::File::open(&freeze.solution)?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            let actual = format!("{:x}", hash.finalize());
            if freeze.sha256.as_deref() != Some(actual.as_str()) {
                return Err(spot::SpotError::new(
                    spot::Code::NLH003,
                    "profile.freeze_upstream",
                    "base solution SHA-256 changed",
                )
                .into());
            }
            let base = crate::artifact::load_sol(Path::new(&freeze.solution), 0, None)?;
            let tree = &built.game.tree;
            if base.game.tree.nodes.len() != tree.nodes.len()
                && p.config
                    .profile_suit_perms
                    .as_ref()
                    .is_some_and(|perms| perms.len() < 24)
            {
                return Err(spot::SpotError::new(
                    spot::Code::NLH003,
                    "profile.freeze_upstream",
                    "base tree differs from the profiled tree; suit-isomorphism restriction from profile selectors cannot be combined with freeze_upstream",
                ).into());
            }
            if base.config.board != p.config.board
                || base.config.pot != p.config.pot
                || base.config.effective_stack != p.config.effective_stack
                || base.game.tree.nodes.len() != tree.nodes.len()
                || Player::BOTH
                    .into_iter()
                    .any(|seat| base.game.index.hands.combos(seat) != index.hands.combos(seat))
            {
                bail!("NLH003: freeze_upstream solution does not match tree or hand support");
            }
            let view = crate::index::TreeView {
                tree,
                index: &index,
            };
            for (id, node) in tree.nodes.iter().enumerate() {
                let other = base.game.tree.node(id as u32);
                if node.kind != other.kind
                    || node.player != other.player
                    || node.num_children != other.num_children
                    || (node.kind == hu_engine::NodeKind::Action
                        && !view
                            .action_labels(id as u32)
                            .eq(base.view().action_labels(id as u32)))
                {
                    bail!("NLH003: freeze_upstream solution has a different tree");
                }
                base_strategies.push(if node.kind == hu_engine::NodeKind::Action {
                    base.stored_strategy(id as u32)?
                } else {
                    None
                });
            }
            loaded_base = Some(base);
        }
        let provider = |id: u32| base_strategies.get(id as usize).cloned().flatten();
        let resolved = crate::profile::resolve_profile(
            &mut built,
            &p.config,
            ir,
            ir.freeze_upstream
                .as_ref()
                .map(|_| &provider as &dyn Fn(u32) -> Option<Vec<f32>>),
        )?;
        Some(resolved)
    } else {
        None
    };
    drop(built.node_info);
    trim_builder_heap();
    Ok((built.game, index, resolved, loaded_base))
}

/// Return freed builder pages to the system when using glibc's allocator.
/// Other platforms and allocators need no explicit trimming.
pub fn trim_builder_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: malloc_trim accepts a padding value and accesses no Rust data.
        unsafe { libc::malloc_trim(0) };
    }
}

/// State for the frequency targets installed with a profile.
pub struct FrequencyController {
    targets: Vec<crate::profile::FrequencyTarget>,
    lambdas: Vec<f64>,
    base_bonuses: Vec<Option<Vec<f32>>>,
    summary: Option<crate::profile::ProfileSummary>,
    frozen_nodes: Vec<hu_engine::NodeId>,
}

/// Install a resolved profile and initialize its frequency controller.
pub fn install_profile<E: TerminalEvaluator, S: Storage>(
    solver: &mut Solver<E, S>,
    resolved: Option<crate::profile::ResolvedProfile>,
    saved: Option<&[f64]>,
) -> Result<FrequencyController> {
    let Some(resolved) = resolved else {
        if saved.is_some_and(|v| !v.is_empty()) {
            bail!("checkpoint has unexpected frequency state");
        }
        return Ok(FrequencyController {
            targets: Vec::new(),
            lambdas: Vec::new(),
            base_bonuses: Vec::new(),
            summary: None,
            frozen_nodes: Vec::new(),
        });
    };
    let lambdas = saved.unwrap_or(&[]).to_vec();
    if saved.is_some() && lambdas.len() != resolved.targets.len() {
        bail!("checkpoint frequency controller shape mismatch");
    }
    let lambdas = if lambdas.is_empty() {
        vec![0.0; resolved.targets.len()]
    } else {
        lambdas
    };
    let base_bonuses = resolved
        .engine
        .incentives
        .iter()
        .map(|entry| entry.as_ref().map(|e| e.bonus.clone()))
        .collect();
    solver
        .set_profile(resolved.engine)
        .map_err(|e| spot::SpotError::new(spot::Code::NLH003, "profile", e.to_string()))?;
    let controller = FrequencyController {
        targets: resolved.targets,
        lambdas,
        base_bonuses,
        summary: Some(resolved.summary),
        frozen_nodes: resolved.frozen_nodes,
    };
    controller.sync(solver);
    Ok(controller)
}

impl FrequencyController {
    fn report<S: Storage>(
        &self,
        solver: &Solver<crate::PostflopEvaluator, S>,
        p: &Prepared,
    ) -> Option<ProfileReport> {
        self.summary.clone().map(|summary| ProfileReport {
            summary,
            frozen_nodes: self.frozen_nodes.clone(),
            general_sum: p
                .document
                .spot
                .profile
                .as_ref()
                .is_some_and(|ir| !ir.incentives.is_empty() || !ir.frequencies.is_empty()),
            frequencies: self.metrics(solver, false),
        })
    }

    fn sync<E: TerminalEvaluator, S: Storage>(&self, solver: &mut Solver<E, S>) {
        let sizes: Vec<_> = solver
            .game()
            .tree
            .storage_refs
            .iter()
            .map(|r| r.num_hands as usize)
            .collect();
        let profile = solver.profile_mut().unwrap();
        for (entry, base) in profile.incentives.iter_mut().zip(&self.base_bonuses) {
            if let (Some(entry), Some(base)) = (entry, base) {
                entry.bonus.copy_from_slice(base);
            }
        }
        for (target, &lambda) in self.targets.iter().zip(&self.lambdas) {
            let incentive = profile.incentives[target.storage_ref as usize]
                .as_mut()
                .unwrap();
            for &h in &target.hands {
                incentive.bonus[target.action * sizes[target.storage_ref as usize] + h as usize] +=
                    lambda as f32;
            }
        }
    }

    fn metrics<E: TerminalEvaluator, S: Storage>(
        &self,
        solver: &Solver<E, S>,
        current: bool,
    ) -> Vec<FrequencyMetric> {
        if self.targets.is_empty() {
            return Vec::new();
        }
        let parents = hu_engine::parent_array(&solver.game().tree);
        self.targets
            .iter()
            .zip(&self.lambdas)
            .map(|(target, &lambda)| {
                let game = solver.game();
                let node = game.tree.node(target.node);
                let root = PerPlayer::new(
                    game.root_ranges[Player::P0].as_slice(),
                    game.root_ranges[Player::P1].as_slice(),
                );
                let path = hu_engine::path_from_root(&parents, target.node);
                let reach = hu_engine::reach_on_path(&game.tree, root, &path, |id, _, out| {
                    out.copy_from_slice(&solver.average_strategy_at(id))
                });
                let strategy = if current {
                    solver.current_strategy_at(target.node)
                } else {
                    solver.average_strategy_at(target.node)
                };
                let hands = game.tree.storage_ref(node).num_hands as usize;
                let (mut numerator, mut denominator) = (0.0, 0.0);
                for &hand in &target.hands {
                    let h = hand as usize;
                    let weight = f64::from(reach[node.player][h]);
                    denominator += weight;
                    numerator += weight * f64::from(strategy[target.action * hands + h]);
                }
                let achieved = (denominator > 0.0).then(|| numerator / denominator);
                FrequencyMetric {
                    entry: target.entry,
                    node: target.node,
                    target: target.target,
                    achieved,
                    error: achieved.map(|f| (f - target.target).abs()),
                    lambda,
                }
            })
            .collect()
    }

    fn update<E: TerminalEvaluator, S: Storage>(
        &mut self,
        solver: &mut Solver<E, S>,
    ) -> Result<()> {
        for (i, metric) in self.metrics(solver, true).iter().enumerate() {
            if let Some(achieved) = metric.achieved {
                self.lambdas[i] += 0.1 * (metric.target - achieved) * self.targets[i].pot_utility;
            }
        }
        if !self.targets.is_empty() {
            self.sync(solver);
        }
        Ok(())
    }
}

pub fn schedule(algorithm: &Algorithm) -> Box<dyn DiscountSchedule> {
    match algorithm {
        Algorithm::Vanilla => Box::new(hu_engine::Vanilla),
        Algorithm::CfrPlus => Box::new(hu_engine::CfrPlus),
        Algorithm::Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        } => Box::new(hu_engine::Dcfr {
            alpha: *alpha,
            beta: *beta,
            gamma: *gamma,
            pow4_reset: *pow4_reset,
        }),
        Algorithm::LinearCfr => Box::new(hu_engine::linear_cfr()),
        Algorithm::HsDcfr { gamma0 } => Box::new(hu_engine::HsDcfr { gamma0: *gamma0 }),
    }
}

fn drive<S: Storage>(
    request: RunRequest<'_>,
    observer: &mut Observer<'_>,
    diagnostics: &mut Diagnostics<'_>,
) -> Result<RunSummary> {
    let RunRequest {
        prepared: p,
        checkpoint,
        solution,
        state,
        evaluation_history: mut history,
        elapsed_before,
        cancel,
    } = request;
    crate::prepare::check_memory_limit(p)?;
    diagnostics(Diagnostic::Memory(p.estimate.clone()));
    for warning in warnings(p) {
        diagnostics(Diagnostic::Warning(warning));
    }
    let (game, index, resolved, base) = build_profiled_with_base(p)?;
    let stop = &p.settings.solver.stop;
    let mut solver = Solver::<_, S>::new(
        game,
        schedule(&p.settings.solver.algorithm),
        Some(stop.max_iterations),
    );
    solver.set_cfr_precision(p.settings.solver.cfr_precision);
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    let saved_lambdas = state.as_ref().map(|s| s.frequency_lambdas.as_slice());
    let mut controller = install_profile(&mut solver, resolved, saved_lambdas)?;
    let (subtree, solve_scope) = crate::subtree::prepare(
        &mut solver,
        &index,
        controller.summary.as_ref(),
        !controller.targets.is_empty(),
        base.as_ref(),
        p.target,
    )?;
    drop(base);
    let mode = if subtree.is_some() {
        crate::checkpoint::CheckpointMode::Subtree
    } else {
        crate::checkpoint::CheckpointMode::WholeTree
    };
    if let Some(scope) = &solve_scope {
        diagnostics(Diagnostic::Warning(format!(
            "freeze_upstream solve scope: {}{}",
            scope.scope,
            scope
                .fallback_reason
                .map_or(String::new(), |reason| format!(" (fallback: {reason})"))
        )));
    }
    if let Some(state) = state {
        if state.mode != mode {
            bail!(
                "checkpoint solve mode differs from this run; restart the solve rather than resuming an old whole-tree freeze checkpoint"
            );
        }
        let iteration = state.iteration;
        solver
            .restore_stream(iteration, |storage| state.read_storage(storage))
            .context("restoring checkpoint state")?;
    }
    observer(Observation::ProgressOpened)?;
    let start = Instant::now();
    let mut last_checkpoint = Instant::now();
    let mut saved_iteration = None;
    let mut evaluated = None;
    let interval = Duration::try_from_secs_f64(p.document.spot.run.checkpoint_interval_seconds)?;
    let max_time = p.document.spot.run.max_time_seconds;
    let mut canceled = false;
    let mut reason = "max-iterations";
    let mut next_check = next_evaluation(
        &history,
        p.target,
        solver.iteration(),
        stop.max_iterations,
        stop.check_every,
    );
    while solver.iteration() < stop.max_iterations {
        if matches!(stop.check_every, input::CheckEvery::Auto)
            && history.last().is_some_and(|&(t, v)| {
                t == solver.iteration() && p.target.is_some_and(|target| v <= target)
            })
        {
            reason = "target-reached";
            break;
        }
        if max_time.is_some_and(|limit| (elapsed_before + start.elapsed()).as_secs_f64() >= limit) {
            reason = "time-limit";
            break;
        }
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            canceled = true;
            reason = "cancelled";
            break;
        }
        let steps = batch_step(stop.check_every, next_check - solver.iteration());
        if let Some(subtree) = &subtree {
            subtree.step(&mut solver, steps);
        } else {
            solver.run(steps);
        }
        // A stop between evaluations must summarize the actual checkpoint state.
        evaluated = None;
        let mut target_reached = false;
        if solver.iteration() == next_check {
            controller.update(&mut solver)?;
            let (ev, expl) = subtree
                .as_ref()
                .map_or_else(|| solver.evaluate(), |s| s.evaluate(&solver));
            evaluated = Some((ev, expl));
            let nash_conv = expl[Player::P0] + expl[Player::P1];
            observer(Observation::Progress(runfiles::MetricsRow {
                iteration: solver.iteration(),
                elapsed_secs: (elapsed_before + start.elapsed()).as_secs_f64(),
                expl_p0: expl[Player::P0],
                expl_p1: expl[Player::P1],
                nash_conv,
            }))?;
            if !controller.targets.is_empty() {
                observer(Observation::ProfileProgress {
                    iteration: solver.iteration(),
                    frequencies: controller.metrics(&solver, false),
                })?;
            }
            history.push((solver.iteration(), nash_conv / 2.0));
            target_reached = p.target.is_some_and(|target| nash_conv / 2.0 <= target);
            next_check = next_evaluation(
                &history,
                p.target,
                solver.iteration(),
                stop.max_iterations,
                stop.check_every,
            );
        }
        let elapsed_secs = (elapsed_before + start.elapsed()).as_secs_f64();
        if last_checkpoint.elapsed() >= interval {
            save(
                &solver,
                p,
                checkpoint,
                elapsed_secs,
                observer,
                &controller.lambdas,
                mode,
            )?;
            last_checkpoint = Instant::now();
            saved_iteration = Some(solver.iteration());
        }
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            canceled = true;
            reason = "cancelled";
            break;
        }
        if target_reached {
            reason = "target-reached";
            break;
        }
        if matches!(stop.check_every, input::CheckEvery::Auto)
            && max_time
                .is_some_and(|limit| (elapsed_before + start.elapsed()).as_secs_f64() >= limit)
        {
            reason = "time-limit";
            break;
        }
    }
    let elapsed = elapsed_before + start.elapsed();
    observer(Observation::Stop { reason })?;
    let offset = p.payoff.ev_offset();
    let (ev, expl) = if subtree.is_some() {
        solver.evaluate()
    } else {
        evaluated.unwrap_or_else(|| solver.evaluate())
    };
    let ev = if solver.profile().is_some() {
        solver.true_expected_values()
    } else {
        ev
    };
    let mut summary = summary_from_evaluation(&solver, elapsed, subgame_ev(ev, offset), expl);
    summary.profile = controller.report(&solver, p);
    summary.solve_scope = solve_scope;
    if let Some(profile) = &summary.profile {
        summary.frequencies.clone_from(&profile.frequencies);
    }
    diagnostics(Diagnostic::Done(summary.clone()));
    summary.canceled = canceled;
    let final_checkpoint =
        p.document.spot.run.final_checkpoint && saved_iteration != Some(solver.iteration());
    let spec = crate::artifact::SolExportSpec {
        path: solution.to_path_buf(),
        mode: match p.settings.output.solution_streets {
            SolutionStreets::Full => crate::artifact::SolStreets::Full,
            SolutionStreets::NoRivers => crate::artifact::SolStreets::NoRivers,
        },
        config_toml: p.effective.clone(),
        storage_name: match p.settings.solver.storage {
            input::Storage::F32 => "f32",
            input::Storage::I16 => "i16",
            input::Storage::I16F32Avg => "i16-f32avg",
        }
        .into(),
    };
    if parallel_finalize(
        final_checkpoint,
        &p.estimate,
        p.settings.solver.storage,
        p.limit,
    ) {
        // join inherits the installed run pool for BOTH closures. In particular,
        // save's current_num_threads and the solution's strategy batches must
        // never fall back to the global pool. Each closure only borrows solver.
        let (checkpoint_result, solution_result) = rayon::join(
            || {
                save(
                    &solver,
                    p,
                    checkpoint,
                    elapsed.as_secs_f64(),
                    observer,
                    &controller.lambdas,
                    mode,
                )
            },
            || {
                crate::artifact::export_sol(
                    &spec,
                    &solver,
                    &index,
                    offset,
                    p.document.spot.context.street,
                    &summary,
                    &mut |d| diagnostics(Diagnostic::Artifact(d)),
                )
            },
        );
        // join waits for both writers, even on error. A successful .sol may
        // remain on checkpoint failure; the caller records the run as failed.
        checkpoint_result?;
        solution_result?;
    } else {
        if final_checkpoint {
            save(
                &solver,
                p,
                checkpoint,
                elapsed.as_secs_f64(),
                observer,
                &controller.lambdas,
                mode,
            )?;
        }
        solver.release_regrets();
        crate::artifact::export_sol(
            &spec,
            &solver,
            &index,
            offset,
            p.document.spot.context.street,
            &summary,
            &mut |d| diagnostics(Diagnostic::Artifact(d)),
        )?;
    }
    Ok(summary)
}

fn parallel_finalize(
    final_checkpoint: bool,
    estimate: &crate::MemoryEstimate,
    backend: input::Storage,
    limit: u64,
) -> bool {
    final_checkpoint
        && estimate
            .parallel_save_bytes(backend)
            .is_some_and(|peak| peak <= limit)
}

fn save<S: Storage>(
    solver: &Solver<crate::PostflopEvaluator, S>,
    p: &Prepared,
    checkpoint: &Path,
    elapsed: f64,
    observer: &mut Observer<'_>,
    lambdas: &[f64],
    mode: crate::checkpoint::CheckpointMode,
) -> Result<()> {
    if mode == crate::checkpoint::CheckpointMode::Subtree {
        crate::checkpoint::write_storage_with_mode(
            checkpoint,
            compatibility_hash(&p.effective)?,
            solver.iteration(),
            solver.storage(),
            &p.effective,
            elapsed,
            rayon::current_num_threads(),
            mode,
            (!lambdas.is_empty()).then_some(lambdas),
        )?;
    } else if lambdas.is_empty() {
        crate::checkpoint::write_storage_with_config(
            checkpoint,
            compatibility_hash(&p.effective)?,
            solver.iteration(),
            solver.storage(),
            &p.effective,
            elapsed,
            rayon::current_num_threads(),
        )?;
    } else {
        crate::checkpoint::write_storage_with_frequency(
            checkpoint,
            compatibility_hash(&p.effective)?,
            solver.iteration(),
            solver.storage(),
            &p.effective,
            elapsed,
            rayon::current_num_threads(),
            lambdas,
        )?;
    }
    observer(Observation::Checkpoint {
        iterations: solver.iteration(),
    })?;
    Ok(())
}

pub(crate) fn with_threads<T: Send>(
    threads: Option<usize>,
    work: impl FnOnce() -> Result<T> + Send,
) -> Result<T> {
    match threads {
        Some(threads) => rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?
            .install(work),
        None => work(),
    }
}

pub(crate) fn subgame_ev(solver_ev: PerPlayer<f64>, offset: PerPlayer<f64>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver_ev[Player::P0] + offset[Player::P0],
        solver_ev[Player::P1] + offset[Player::P1],
    )
}

#[cfg(test)]
pub(crate) fn solver_ev<E: TerminalEvaluator, S: Storage>(solver: &Solver<E, S>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver.expected_value(Player::P0),
        solver.expected_value(Player::P1),
    )
}

/// Achieved reach-weighted frequency for one selector/runout target.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FrequencyMetric {
    pub entry: usize,
    pub node: hu_engine::NodeId,
    pub target: f64,
    pub achieved: Option<f64>,
    pub error: Option<f64>,
    pub lambda: f64,
}

/// Profile diagnostics retained with an exported solution.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProfileReport {
    pub summary: crate::profile::ProfileSummary,
    pub frozen_nodes: Vec<hu_engine::NodeId>,
    pub general_sum: bool,
    pub frequencies: Vec<FrequencyMetric>,
}

/// Final convergence and cancellation data, on the subgame-start reporting basis.
#[derive(Clone)]
pub struct RunSummary {
    pub iterations: u64,
    /// Root EV per player, on the subgame-start reporting basis.
    pub ev: PerPlayer<f64>,
    pub wall: Duration,
    pub expl_p0: f64,
    pub expl_p1: f64,
    pub nash_conv: f64,
    pub frequencies: Vec<FrequencyMetric>,
    pub profile: Option<ProfileReport>,
    pub solve_scope: Option<crate::subtree::SolveScope>,
    /// The run stopped on request at a checkpoint boundary, not because it
    /// reached its budget or its convergence target.
    pub canceled: bool,
}

#[cfg(test)]
pub(crate) fn summarize<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
    ev: PerPlayer<f64>,
) -> RunSummary {
    let expl = solver.exploitability();
    summary_from_evaluation(solver, elapsed, ev, expl)
}

pub(crate) fn summary_from_evaluation<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
    ev: PerPlayer<f64>,
    expl: PerPlayer<f64>,
) -> RunSummary {
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    RunSummary {
        canceled: false,
        iterations: solver.iteration(),
        wall: elapsed,
        ev,
        expl_p0: expl[Player::P0],
        expl_p1: expl[Player::P1],
        nash_conv,
        frequencies: Vec::new(),
        solve_scope: None,
        profile: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toy_frequency_controller_converges_for_two_targets() {
        use crate::game::{ChipEv, NoRake, PayoffPipeline, kuhn};
        use crate::profile::{FrequencyTarget, ProfileSummary, ResolvedProfile};
        use hu_engine::{Dcfr, F32Storage, NodeIncentive, Profile};

        let toy = kuhn(PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        });
        let tree = &toy.game.tree;
        let root = tree.node(0);
        let s = root.aux as usize;
        let hands = tree.storage_ref(root).num_hands as usize;
        let mut incentives = vec![None; tree.storage_refs.len()];
        incentives[s] = Some(NodeIncentive {
            bonus: vec![0.0; root.num_children as usize * hands],
            space: 0,
        });
        let targets = [0.25, 0.75]
            .into_iter()
            .enumerate()
            .map(|(entry, target)| FrequencyTarget {
                entry,
                node: 0,
                storage_ref: s as u32,
                action: 0,
                hands: vec![(entry * 2) as u32],
                target,
                pot_utility: 2.0,
            })
            .collect();
        let resolved = ResolvedProfile {
            engine: Profile {
                locks: vec![None; tree.storage_refs.len()],
                incentives,
            },
            targets,
            summary: ProfileSummary::default(),
            frozen_nodes: Vec::new(),
        };
        let mut solver =
            Solver::<_, F32Storage>::new(toy.game, Box::<Dcfr>::default(), Some(100_000));
        let mut controller = install_profile(&mut solver, Some(resolved), None).unwrap();
        for _ in 0..4000 {
            solver.run(25);
            controller.update(&mut solver).unwrap();
        }
        let metrics = controller.metrics(&solver, false);
        assert_eq!(metrics.len(), 2);
        for metric in metrics {
            assert!(metric.error.unwrap() < 0.01, "{metric:?}");
        }
    }

    const RIVER_PROFILE: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2s 7s Ks 4h 3d"
[ranges]
BB = "AK,KQ,AA"
BTN = "QQ,JJ"
[tree]
script = "river { replace bet [50] } river when aggressions >= 1 { remove raise }"
[solver]
storage = "f32"
[solver.stop]
max_iterations = 4000
check_every = 25
[run]
memory = "1GiB"
threads = 1
[[profile.frequency]]
node = "root"
action = "check"
target = 0.5
"#;

    #[test]
    fn river_frequency_target_is_reported_and_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let p = crate::prepare::prepare(RIVER_PROFILE, &dir.path().join("spot.toml")).unwrap();
        let checkpoint = dir.path().join("checkpoint.ckpt");
        let solution = dir.path().join("solution.sol");
        let summary = run(
            RunRequest {
                prepared: &p,
                checkpoint: &checkpoint,
                solution: &solution,
                state: None,
                evaluation_history: Vec::new(),
                elapsed_before: Duration::ZERO,
                cancel: &AtomicBool::new(false),
            },
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        let metric = &summary.frequencies[0];
        assert!(metric.error.unwrap() < 0.01, "frequency: {metric:?}");
        let artifact = crate::artifact::load_sol(&solution, 0, None).unwrap();
        let nodes = &artifact.profile.as_ref().unwrap().summary.resolved_nodes;
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node, 0);
        assert_eq!(nodes[0].history, "");
        assert_eq!(nodes[0].entry, Some(0));
        let exported = crate::views::export(
            &solution,
            crate::views::ExportView::Summary,
            &[],
            false,
            None,
        )
        .unwrap();
        let json = serde_json::to_value(exported).unwrap();
        assert_eq!(
            json["profile"]["summary"]["resolved_nodes"][0]["kind"],
            "frequency"
        );
        assert_eq!(json["profile_config"]["frequency"][0]["target"], 0.5);
        let stored = &artifact.profile.unwrap().frequencies[0];
        assert_eq!(stored.lambda, metric.lambda);
        assert_eq!(stored.achieved, metric.achieved);
        assert_eq!(
            crate::checkpoint::CheckpointReader::open(&checkpoint)
                .unwrap()
                .frequency_lambdas,
            vec![metric.lambda]
        );
    }

    #[test]
    fn frequency_resume_preserves_state_and_rejects_changed_profile() {
        use std::sync::atomic::Ordering;
        let dir = tempfile::tempdir().unwrap();
        let p = crate::prepare::prepare(RIVER_PROFILE, &dir.path().join("spot.toml")).unwrap();
        let full_ckpt = dir.path().join("full.ckpt");
        let full_sol = dir.path().join("full.sol");
        let partial_ckpt = dir.path().join("partial.ckpt");
        let partial_sol = dir.path().join("partial.sol");
        let cancel = AtomicBool::new(false);
        fn request<'a>(
            p: &'a Prepared,
            cancel: &'a AtomicBool,
            checkpoint: &'a Path,
            solution: &'a Path,
            state: Option<crate::checkpoint::CheckpointReader>,
            history: Vec<(u64, f64)>,
        ) -> RunRequest<'a> {
            RunRequest {
                prepared: p,
                checkpoint,
                solution,
                state,
                evaluation_history: history,
                elapsed_before: Duration::ZERO,
                cancel,
            }
        }
        let full = run(
            request(&p, &cancel, &full_ckpt, &full_sol, None, Vec::new()),
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        let mut history = Vec::new();
        let partial = run(
            request(&p, &cancel, &partial_ckpt, &partial_sol, None, Vec::new()),
            &mut |event| {
                if let Observation::Progress(row) = event {
                    history.push((row.iteration, row.nash_conv / 2.0));
                    if row.iteration == 2000 {
                        cancel.store(true, Ordering::SeqCst);
                    }
                }
                Ok(())
            },
            &mut |_| {},
        )
        .unwrap();
        assert!(partial.canceled);
        cancel.store(false, Ordering::SeqCst);
        let changed = p.effective.replace("target = 0.5", "target = 0.6");
        assert!(crate::prepare::restore(&changed, &partial_ckpt).is_err());
        let state = crate::checkpoint::CheckpointReader::open(&partial_ckpt).unwrap();
        let resumed = run(
            request(
                &p,
                &cancel,
                &partial_ckpt,
                &partial_sol,
                Some(state),
                history,
            ),
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            full.frequencies[0].lambda.to_bits(),
            resumed.frequencies[0].lambda.to_bits()
        );
        assert_eq!(
            full.frequencies[0].achieved.unwrap().to_bits(),
            resumed.frequencies[0].achieved.unwrap().to_bits()
        );
        assert_eq!(
            crate::checkpoint::read_checkpoint(&full_ckpt)
                .unwrap()
                .state,
            crate::checkpoint::read_checkpoint(&partial_ckpt)
                .unwrap()
                .state
        );
        let a = crate::artifact::load_sol(&full_sol, 0, None).unwrap();
        let b = crate::artifact::load_sol(&partial_sol, 0, None).unwrap();
        for id in 0..a.game.tree.nodes.len() as hu_engine::NodeId {
            if a.game.tree.node(id).kind == hu_engine::NodeKind::Action {
                assert_eq!(
                    a.stored_strategy(id).unwrap(),
                    b.stored_strategy(id).unwrap()
                );
            }
        }
    }

    #[test]
    fn frozen_ancestors_follow_base_solution_and_changed_base_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let raw = RIVER_PROFILE.split("[[profile.frequency]]").next().unwrap();
        let path = dir.path().join("spot.toml");
        let base = crate::prepare::prepare(raw, &path).unwrap();
        let base_ckpt = dir.path().join("base.ckpt");
        let base_sol = dir.path().join("base.sol");
        let cancel = AtomicBool::new(false);
        let execute = |p: &Prepared, ckpt: &Path, sol: &Path| {
            run(
                RunRequest {
                    prepared: p,
                    checkpoint: ckpt,
                    solution: sol,
                    state: None,
                    evaluation_history: Vec::new(),
                    elapsed_before: Duration::ZERO,
                    cancel: &cancel,
                },
                &mut |_| Ok(()),
                &mut |_| {},
            )
        };
        execute(&base, &base_ckpt, &base_sol).unwrap();
        let path_text = base_sol.to_string_lossy().replace('\\', "/");
        let frozen_input = format!(
            "{raw}\n[profile]\nfreeze_upstream = {{ solution = '{path_text}' }}\n[[profile.lock]]\nnode = 'x'\nstrategy = {{ check = 1.0 }}\n"
        );
        let p = crate::prepare::prepare(&frozen_input, &path).unwrap();
        let frozen_ckpt = dir.path().join("frozen.ckpt");
        let frozen_sol = dir.path().join("frozen.sol");
        let summary = execute(&p, &frozen_ckpt, &frozen_sol).unwrap();
        assert!(summary.profile.unwrap().frozen_nodes.contains(&0));
        let base = crate::artifact::load_sol(&base_sol, 0, None).unwrap();
        let frozen = crate::artifact::load_sol(&frozen_sol, 0, None).unwrap();
        assert_eq!(
            base.stored_strategy(0).unwrap(),
            frozen.stored_strategy(0).unwrap()
        );
        let comparison = crate::views::compare(&base_sol, &frozen_sol, false).unwrap();
        assert!(comparison.profile_changed);
        let conditions = comparison.profile_conditions.unwrap();
        assert!(conditions[0].is_none());
        assert!(conditions[1].as_ref().unwrap().contains("freeze_upstream"));
        drop(base);
        let mut bytes = std::fs::read(&base_sol).unwrap();
        bytes.push(0);
        std::fs::write(&base_sol, bytes).unwrap();
        assert!(execute(&p, &frozen_ckpt, &frozen_sol).is_err());
        drop(frozen);
        std::fs::remove_file(&base_sol).unwrap();
        assert!(crate::artifact::load_sol(&frozen_sol, 0, None).is_ok());
    }

    #[test]
    fn incentive_changes_play_but_solution_ev_excludes_bonus() {
        let dir = tempfile::tempdir().unwrap();
        let raw = RIVER_PROFILE.split("[[profile.frequency]]").next().unwrap();
        let path = dir.path().join("spot.toml");
        let base = crate::prepare::prepare(raw, &path).unwrap();
        let cancel = AtomicBool::new(false);
        let (base_solver, index, _, _) = query::<F32Storage>(&base, None, None, &cancel).unwrap();
        let view = crate::index::TreeView {
            tree: &base_solver.game().tree,
            index: &index,
        };
        let actions = view.actions(0);
        let bet = actions.iter().position(|a| a.starts_with("bet ")).unwrap();
        let input = format!(
            "{raw}\n[[profile.incentive]]\nnode = 'root'\naction = '{}'\nbb = 10.0\n",
            actions[bet]
        );
        let p = crate::prepare::prepare(&input, &path).unwrap();
        let (solver, _, _, _) = query::<F32Storage>(&p, None, None, &cancel).unwrap();
        let hands = solver
            .game()
            .tree
            .storage_ref(solver.game().tree.node(0))
            .num_hands as usize;
        let frequency = |solver: &Solver<crate::PostflopEvaluator, F32Storage>| {
            prepare::action_frequencies(
                &solver.average_strategy_at(0),
                &solver.game().root_ranges[Player::P0],
                actions.len(),
                hands,
            )[bet]
        };
        assert!(frequency(&solver) > frequency(&base_solver) + 0.1);
        let true_ev = solver.true_expected_values();
        assert!(solver.evaluate().0[Player::P0] > true_ev[Player::P0] + 0.1);
        let solution = dir.path().join("incentive.sol");
        let summary = run(
            RunRequest {
                prepared: &p,
                checkpoint: &dir.path().join("incentive.ckpt"),
                solution: &solution,
                state: None,
                evaluation_history: Vec::new(),
                elapsed_before: Duration::ZERO,
                cancel: &cancel,
            },
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        assert!(summary.profile.as_ref().unwrap().general_sum);
        let stored = crate::artifact::load_sol(&solution, 0, None).unwrap();
        assert_eq!(stored.meta.ev[0], summary.ev[Player::P0]);
        assert!(
            (summary.ev[Player::P0] - subgame_ev(true_ev, p.payoff.ev_offset())[Player::P0]).abs()
                < 1e-6
        );
    }

    #[test]
    fn category_lock_survives_solution_quantization() {
        let dir = tempfile::tempdir().unwrap();
        let raw = RIVER_PROFILE.split("[[profile.frequency]]").next().unwrap();
        let input = format!(
            "{raw}\n[[profile.lock]]\nnode = 'root'\ncategory = 'toppair'\nstrategy = {{ check = 0.5 }}\n"
        );
        let p = crate::prepare::prepare(&input, &dir.path().join("spot.toml")).unwrap();
        let path = dir.path().join("lock.sol");
        run(
            RunRequest {
                prepared: &p,
                checkpoint: &dir.path().join("lock.ckpt"),
                solution: &path,
                state: None,
                evaluation_history: Vec::new(),
                elapsed_before: Duration::ZERO,
                cancel: &AtomicBool::new(false),
            },
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        let loaded = crate::artifact::load_sol(&path, 0, None).unwrap();
        let strategy = loaded.stored_strategy(0).unwrap().unwrap();
        let category = nlh::category::CategoryExpr::parse("toppair").unwrap();
        let actor = loaded.game.tree.node(0).player;
        let mut matched = 0;
        for (h, &global) in loaded.game.index.hands.combos(actor).iter().enumerate() {
            let (a, b) = nlh::combo_cards(global as usize);
            if category.matches(&p.config.board, [a, b]) {
                assert_eq!(strategy[h], 0.5);
                matched += 1;
            }
        }
        assert!(matched > 0);
    }

    #[test]
    fn opponent_full_lock_leaves_other_seat_near_best_response() {
        let dir = tempfile::tempdir().unwrap();
        let raw = RIVER_PROFILE.split("[[profile.frequency]]").next().unwrap();
        let path = dir.path().join("spot.toml");
        let p = crate::prepare::prepare(raw, &path).unwrap();
        let (game, index) = build_indexed(&p).unwrap();
        let view = crate::index::TreeView {
            tree: &game.tree,
            index: &index,
        };
        let mut input = raw.to_owned();
        for (id, node) in game.tree.nodes.iter().enumerate() {
            if node.kind == hu_engine::NodeKind::Action && node.player == Player::P1 {
                let id = id as hu_engine::NodeId;
                input.push_str(&format!(
                    "\n[[profile.lock]]\nnode = '{}'\nstrategy = {{ '{}' = 1.0 }}\n",
                    view.history(id),
                    view.actions(id)[0]
                ));
            }
        }
        let p = crate::prepare::prepare(&input, &path).unwrap();
        let summary = run(
            RunRequest {
                prepared: &p,
                checkpoint: &dir.path().join("full.ckpt"),
                solution: &dir.path().join("full.sol"),
                state: None,
                evaluation_history: Vec::new(),
                elapsed_before: Duration::ZERO,
                cancel: &AtomicBool::new(false),
            },
            &mut |_| Ok(()),
            &mut |_| {},
        )
        .unwrap();
        assert!(
            summary.expl_p0.abs() < 0.01,
            "other-seat deviation gain: {}",
            summary.expl_p0
        );
    }

    #[test]
    fn progress_values_round_trip_without_changing_schedule_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(runfiles::RUN_PROGRESS_FILE);
        let mut writer = runfiles::MetricsWriter::create_or_append(&path).unwrap();
        let mut bits = 123456789_u64;
        let mut expected = Vec::new();
        for iteration in 1..=256 {
            bits = bits.wrapping_mul(6364136223846793005).wrapping_add(1);
            let value = f64::from_bits((bits & ((1_u64 << 52) - 1)) | (1023_u64 << 52));
            writer
                .append(&runfiles::MetricsRow {
                    iteration,
                    elapsed_secs: 0.0,
                    expl_p0: value,
                    expl_p1: value,
                    nash_conv: value * 2.0,
                })
                .unwrap();
            expected.push((iteration, value));
        }
        drop(writer);
        assert_eq!(read_evaluation_history(&path, 256), expected);
    }

    #[test]
    fn adaptive_schedule_is_bounded_and_uses_only_evaluations() {
        let next = |h: &[(u64, f64)], target, current, max| {
            next_evaluation(h, target, current, max, input::CheckEvery::Auto)
        };
        assert_eq!(next(&[], Some(1.0), 0, 1000), 25);
        assert_eq!(next(&[], Some(1.0), 77, 1000), 102);
        assert_eq!(next(&[(25, 4.0)], Some(1.0), 25, 1000), 50);
        let h = [(25, 4.0), (50, 2.0)]; // slope 1, arrival 100, ceil(.8*50)=40
        assert_eq!(next(&h, Some(1.0), 50, 1000), 90);
        assert_eq!(next(&h, Some(1.0), 75, 1000), 90);
        assert_eq!(next(&h, None, 50, 1000), 75);
        assert_eq!(next(&h, Some(0.01), 50, 1000), 100); // upper bound
        assert_eq!(next(&h, Some(1.99), 50, 1000), 53); // lower bound
        assert_eq!(next(&h, Some(3.0), 50, 1000), 53); // prediction in the past
        assert_eq!(next(&h, Some(1.0), 50, 61), 61);
        assert_eq!(next(&[], Some(1.0), 0, 9), 9);
        for h in [
            [(25, 2.0), (50, 2.0)],
            [(25, 2.0), (50, 1.99)], // slope <= .05
            [(25, 1.0), (50, 2.0)],
            [(25, f64::NAN), (50, 2.0)],
            [(25, 2.0), (50, f64::INFINITY)],
            [(0, 4.0), (50, 2.0)],
            [(25, 0.0), (50, 2.0)],
        ] {
            assert_eq!(next(&h, Some(1.0), 50, 1000), 75);
        }
        assert_eq!(
            next(&[(25, 4.0), (50, 2.0)], Some(f64::MIN_POSITIVE), 50, 1000),
            75
        );
        assert_eq!(
            next_evaluation(&h, Some(1.0), 77, 1000, input::CheckEvery::Fixed(25)),
            102
        );
        assert_eq!(batch_step(input::CheckEvery::Auto, 50), 25);
        assert_eq!(batch_step(input::CheckEvery::Auto, 7), 7);
        assert_eq!(batch_step(input::CheckEvery::Fixed(50), 50), 50);
    }

    #[test]
    fn parallel_finalization_requires_a_save_and_room_for_both_codecs() {
        let estimate = crate::MemoryEstimate {
            f32_bytes: 100,
            i16_bytes: 60,
            i16_f32avg_bytes: 80,
            save_bytes: 40,
            compression_bytes: 10,
            game_bytes: 7,
            profile_bytes: 5,
            thread_bytes: 3,
            threads: 4,
            ..Default::default()
        };
        let resident = crate::postflop::PROCESS_BYTES + 7 + 5 + 3 * 4;
        assert_eq!(estimate.resident_bytes(), resident);
        // The .sol writer's budget and the four-worker checkpoint encoder.
        let codecs = 10 + crate::checkpoint::encoder_workspace(4);
        for (backend, storage) in [
            (input::Storage::F32, 100),
            (input::Storage::I16, 60),
            (input::Storage::I16F32Avg, 80),
        ] {
            let peak = storage + 40 + codecs + resident;
            assert_eq!(estimate.parallel_save_bytes(backend), Some(peak));
            assert!(!parallel_finalize(true, &estimate, backend, peak - 1));
            assert!(parallel_finalize(true, &estimate, backend, peak));
            assert!(parallel_finalize(true, &estimate, backend, peak + 1));
            assert!(!parallel_finalize(false, &estimate, backend, u64::MAX));
        }
        for (storage, workspace, codec, game) in [
            (u64::MAX, 1, 0, 0),
            (0, 0, u64::MAX, 0),
            (0, 0, 0, u64::MAX),
        ] {
            let overflow = crate::MemoryEstimate {
                f32_bytes: storage,
                save_bytes: workspace,
                compression_bytes: codec,
                game_bytes: game,
                ..Default::default()
            };
            assert!(!parallel_finalize(
                true,
                &overflow,
                input::Storage::F32,
                u64::MAX
            ));
        }
    }
}
