use super::{
    Board, BucketSource, Evaluation, FixedFlops, LeafOverride, LeafStrategy, Sampling, Strategies,
    WeightedFlopSubset,
    cards::{boards, boards_into, runouts_into, subset_boards_into},
    eval::{class_mass, evaluate_with_overrides, override_map},
    pass::{CHUNK, LEAF_POOL, Mapping, Pass, class_values, discount, inputs, scratch},
};
use crate::trunk::l0::{
    Model, Profile, SolveOptions, SolveTimings, Tree,
    eval::{Reach, solver_reaches, update_reach},
    leaves::leaf_values,
    solve::{Discounts, average_profile, backward_values, update_rows},
};
use crate::trunk::scratch::Reused;
use crate::{ExternalSamplingGame, Street};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::{cell::RefCell, path::Path, time::Instant};

thread_local! {
    static UPDATES: RefCell<Vec<Vec<f64>>> = const { RefCell::new(Vec::new()) };
}

fn updates(size: usize) -> Reused<Vec<f64>> {
    let mut values = Reused::take_bounded(&UPDATES, LEAF_POOL, Vec::new);
    values.resize(size, 0.0);
    values.fill(0.0);
    values
}

/// Per-iteration decay of the moments behind the training regression
/// coefficient. Only earlier iterations' boards enter it, so each iteration's
/// estimate stays unbiased.
const REGRESSION_DECAY: f64 = 0.95;

/// Decayed sums over boards of the checkdown `x`, `x²`, the difference
/// `d = L1 − x` and `d·x`, per class.
#[derive(Clone)]
pub(super) struct Moments {
    pub(super) weight: f64,
    pub(super) sums: [[f64; 169]; 4],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Options<'a> {
    /// Total solve wall time, including saved elapsed time; checked after each iteration.
    pub max_seconds: Option<f64>,
    /// Stop after a finished iteration when this file exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_file: Option<&'a Path>,
    #[serde(flatten)]
    pub trunk: SolveOptions,
    pub l1_boards: u32,
    pub l1_seed: u64,
    /// Restrict training to a weighted representative-flop game. Evaluation
    /// still uses the full board distribution.
    #[serde(skip)]
    pub l1_flop_subset: Option<&'a WeightedFlopSubset>,
    /// Apply the representative flops only to leaves with flop SPR <= this limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_flop_subset_max_spr: Option<f64>,
    pub l1_eval_boards: u32,
    pub l1_eval_seed: u64,
    /// Train on the exact L0 checkdown value plus the sampled boards' mean
    /// L1-minus-checkdown difference (an unbiased control variate).
    pub l1_train_control: bool,
    /// Scale the training control variate by a regression coefficient per
    /// leaf, seat and class fitted on earlier iterations (requires
    /// `l1_train_control`).
    pub l1_train_regression: bool,
    /// Fix selected L1 leaf class values during trunk training and checkpoints.
    #[serde(skip)]
    pub l1_train_overrides: &'a [LeafOverride],
    /// The same control variate for the evaluator's leaf estimates.
    pub l1_eval_control: bool,
    /// Scale the evaluator's control variate by each half's fitted regression
    /// coefficient per leaf and class (requires `l1_eval_control`).
    pub l1_eval_regression: bool,
    pub l1_sampling: Sampling,
    pub l1_eval_sampling: Sampling,
    /// DCFR's nonpositive-regret exponent for the postflop strategies; the
    /// trunk's `alpha` and `gamma` apply to both.
    pub l1_postflop_beta: f64,
    /// Replace the uniform relative prior with a logit-shaped L1 training reach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qre: Option<super::Qre>,
    /// Weight the postflop regret updates by each combo's own reach within
    /// its bucket (see `Pass::own_reach_regret`). Experimental; off by default.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub l1_own_reach_regret: bool,
    /// From iteration `from` on, train on `boards` boards per iteration
    /// instead of `l1_boards`. Experimental; off by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_boards_late: Option<LateBoards>,
}

/// The late part of a two-step training board schedule (`Options::l1_boards_late`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct LateBoards {
    pub boards: u32,
    pub from: u64,
}

impl LateBoards {
    /// Parses `BOARDS:FROM` (both positive).
    pub fn parse(spec: &str) -> Result<Self> {
        let (boards, from) = spec
            .split_once(':')
            .context("late training boards are BOARDS:FROM")?;
        let late = Self {
            boards: boards.parse().context("late training boards")?,
            from: from
                .parse()
                .context("first iteration of the late training boards")?,
        };
        ensure!(
            late.boards > 0 && late.from > 0,
            "late training boards and their first iteration must be positive"
        );
        Ok(late)
    }
}

impl Options<'_> {
    /// Training boards of iteration `t` (1-based).
    pub(super) fn training_boards(&self, t: u64) -> u32 {
        match self.l1_boards_late {
            Some(late) if t >= late.from => late.boards,
            _ => self.l1_boards,
        }
    }
}

/// The settings S4-2a measured on B6: 32 stratified boards per iteration, both
/// control variates with their regression coefficients, and DCFR's
/// nonpositive-regret exponent 1 for the trunk instead of L0's 0, which keeps
/// the trunk from chasing the boards' noise. The postflop strategies keep 0:
/// 1 there did not lower the primary metric and made them more exploitable
/// with the real board.
impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            max_seconds: None,
            stop_file: None,
            trunk: SolveOptions {
                beta: 1.0,
                ..SolveOptions::default()
            },
            l1_boards: 32,
            l1_seed: 0,
            l1_flop_subset: None,
            l1_flop_subset_max_spr: None,
            l1_eval_boards: 1024,
            l1_eval_seed: 0,
            l1_train_control: true,
            l1_train_regression: true,
            l1_train_overrides: &[],
            l1_eval_control: true,
            l1_eval_regression: true,
            l1_sampling: Sampling::Stratified,
            l1_eval_sampling: Sampling::Random,
            l1_postflop_beta: 0.0,
            qre: None,
            l1_own_reach_regret: false,
            l1_boards_late: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Timings {
    #[serde(flatten)]
    pub trunk: SolveTimings,
    pub board_preparation: f64,
    pub postflop: f64,
    pub evaluation_board_preparation: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Checkpoint {
    pub iteration: u64,
    #[serde(flatten)]
    pub evaluation: Evaluation,
    pub seconds: f64,
}

pub struct Progress<'a> {
    pub iteration: u64,
    pub timings: &'a Timings,
    pub checkpoint: Option<&'a Checkpoint>,
    /// Borrowed average only when computed for this checkpoint/final iteration.
    pub average: Option<&'a Profile>,
    pub postflop: &'a Strategies,
    pub seconds: f64,
    pub finished: bool,
    state: super::state::StateRef<'a>,
}

impl Progress<'_> {
    /// Stream a complete finished-iteration state without cloning solver arrays.
    pub fn write_state(&self, out: &mut impl std::io::Write) -> Result<()> {
        self.state.write(out)
    }
}

pub struct Solution {
    /// Total wall time spent inside solve across all invocations.
    pub seconds: f64,
    pub iterations: u64,
    pub reached_target: bool,
    pub stopped_by_stop_file: bool,
    pub average: Profile,
    pub postflop: Strategies,
    pub checkpoints: Vec<Checkpoint>,
    pub timings: Timings,
}

/// The evaluator's boards. It picks a best response on the even boards and
/// scores it on the odd ones, and the reverse, so the halves must be
/// independent: stratified halves each get their own random shift and are
/// interleaved, rather than splitting one stratified sample.
pub fn evaluation_boards(source: &dyn BucketSource, options: &Options) -> Vec<Board> {
    let count = options.l1_eval_boards;
    let seed = options.l1_eval_seed;
    if options.l1_eval_sampling == Sampling::Random {
        return boards(
            source,
            b"solvers.p2.trunk.l1.eval.v1",
            seed,
            None,
            count,
            Sampling::Random,
        );
    }
    let [even, odd] = [
        b"solvers.p2.trunk.l1.eval.v1.even".as_slice(),
        b"solvers.p2.trunk.l1.eval.v1.odd",
    ]
    .map(|domain| boards(source, domain, seed, None, count / 2, Sampling::Stratified));
    even.into_iter()
        .zip(odd)
        .flat_map(|(a, b)| [a, b])
        .collect()
}

pub fn solve(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    options: Options,
    mut observer: impl FnMut(&Progress<'_>),
) -> Result<Solution> {
    solve_with_state(tree, model, source, options, None, |p| {
        observer(p);
        Ok(())
    })
}

/// Resume at the next iteration. The iteration upper bound and time limit may
/// change; all settings that affect updates/evaluation must match the state.
pub fn solve_with_state(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    options: Options,
    state: Option<super::State>,
    mut observer: impl FnMut(&Progress<'_>) -> Result<()>,
) -> Result<Solution> {
    let o = options.trunk;
    ensure!(
        options.max_seconds.is_none_or(|x| x.is_finite() && x > 0.0),
        "max_seconds must be finite and positive"
    );
    ensure!(o.iterations >= 1, "iterations must be positive");
    ensure!(
        o.alpha.is_finite() && o.beta.is_finite() && o.gamma.is_finite() && o.gamma >= 0.0,
        "invalid DCFR exponents"
    );
    ensure!(
        o.target_nash_conv.is_none_or(|x| x.is_finite() && x >= 0.0),
        "invalid target NashConv"
    );
    ensure!(
        o.k4_samples != Some(0)
            && o.k4_min_samples
                .is_none_or(|min| min >= 1 && o.k4_samples.is_some_and(|n| min <= n)),
        "invalid solver K4 budget"
    );
    ensure!(
        tree.game_fingerprint == model.game().game_fingerprint(),
        "model/tree mismatch"
    );
    ensure!(
        tree.nodes.iter().all(|n| n.children.len() <= 254),
        "trunk nodes may have at most 254 actions"
    );
    ensure!(
        options.l1_postflop_beta.is_finite(),
        "invalid postflop DCFR beta"
    );
    ensure!(options.l1_boards > 0, "l1_boards must be positive");
    ensure!(
        options
            .l1_boards_late
            .is_none_or(|l| l.boards > 0 && l.from > 0),
        "late training boards and their first iteration must be positive"
    );
    validate_subset_routing(options, None)?;
    ensure!(
        source.relative_prior().is_finite() && source.relative_prior() >= 0.0,
        "relative prior must be finite and nonnegative"
    );
    let replacements = override_map(tree, options.l1_train_overrides)?;
    ensure!(
        replacements.is_empty() || options.qre.is_none(),
        "--l1-train-override cannot be combined with --l1-qre-prior"
    );
    let override_ones = (!replacements.is_empty()).then(|| vec![vec![1.0; tree.seats]]);
    let mut override_mass = (!replacements.is_empty()).then(|| vec![0.0; nlh::NUM_COMBOS]);
    if let Some(qre) = options.qre {
        qre.validate()?;
        ensure!(
            source.relative() && source.relative_prior() > 0.0 && !source.relative_average(),
            "QRE needs current relative buckets and a positive prior"
        );
        ensure!(
            options.l1_train_control && !options.l1_train_regression,
            "QRE needs training control and disables training regression"
        );
    }
    ensure!(
        !options.l1_train_regression || options.l1_train_control,
        "l1_train_regression needs l1_train_control"
    );
    ensure!(
        !options.l1_eval_regression || options.l1_eval_control,
        "l1_eval_regression needs l1_eval_control"
    );
    ensure!(
        options.l1_eval_boards >= 2 && options.l1_eval_boards.is_multiple_of(2),
        "l1_eval_boards must be even and >= 2"
    );
    ensure!(
        [Street::Flop, Street::Turn, Street::River]
            .iter()
            .all(|&s| (1..=u16::MAX as usize).contains(&source.count(s))),
        "invalid bucket counts"
    );
    let start = Instant::now();
    let mut timings = Timings::default();
    let phase = Instant::now();
    let eval_boards = if o.eval_every > 0 {
        evaluation_boards(source, &options)
    } else {
        Vec::new()
    };
    timings.evaluation_board_preparation = phase.elapsed().as_secs_f64();
    let (
        mut profile,
        mut regrets,
        mut sums,
        mut postflop,
        mut logit,
        mut reach,
        mut regression,
        mut checkpoints,
        mut iterations,
        elapsed,
        identity,
    ) = if let Some(state) = state {
        let identity = super::state::identity(tree, model, source, options, &state.postflop)?;
        ensure!(
            state.identity == identity,
            "L1 state options/source/layout identity mismatch"
        );
        ensure!(
            state.iteration <= o.iterations,
            "L1 state iteration exceeds iterations upper bound"
        );
        let preparation = timings.evaluation_board_preparation;
        timings = state.timings;
        timings.evaluation_board_preparation += preparation;
        (
            state.profile,
            state.regrets,
            state.sums,
            state.postflop,
            state.logit,
            state.reach,
            state.regression,
            state.checkpoints,
            state.iteration,
            state.elapsed,
            identity,
        )
    } else {
        let postflop = Strategies::new(tree, source);
        let identity = super::state::identity(tree, model, source, options, &postflop)?;
        let profile = Profile::uniform(tree);
        let mut regrets = profile.clone();
        let mut sums = profile.clone();
        for (z, n) in tree.nodes.iter().enumerate() {
            if n.actor.is_some() {
                for c in 0..169 {
                    regrets.row_mut(tree, z, c).fill(0.0);
                    sums.row_mut(tree, z, c).fill(0.0);
                }
            }
        }
        let phase = Instant::now();
        let reach = solver_reaches(tree, &profile, model);
        timings.trunk.reaches += phase.elapsed().as_secs_f64();
        let regression = vec![
            Moments {
                weight: 0.0,
                sums: [[0.0; 169]; 4]
            };
            postflop.leaves.len() * tree.seats
        ];
        (
            profile,
            regrets,
            sums,
            postflop,
            options.qre.map(|_| Profile::uniform(tree)),
            reach,
            regression,
            Vec::new(),
            0,
            0.0,
            identity,
        )
    };
    // The average carries no learning state: this pure, fixed-order function
    // of sums reconstructs exactly the same rows as a saved checkpoint.
    let mut average = if iterations == 0 {
        profile.clone()
    } else {
        average_profile(tree, &sums, model)
    };
    let checkpoint = |iteration,
                      average: &Profile,
                      postflop: &Strategies,
                      timings: &mut Timings|
     -> Result<Checkpoint> {
        let e = evaluate_with_overrides(
            tree,
            average,
            model,
            postflop,
            &eval_boards,
            options.l1_eval_control,
            options.l1_eval_regression,
            options.l1_train_overrides,
        )?;
        timings.trunk.evaluation += e.seconds;
        Ok(Checkpoint {
            iteration,
            evaluation: e,
            seconds: elapsed + start.elapsed().as_secs_f64(),
        })
    };
    let target_met = |c: &Checkpoint| {
        o.target_nash_conv
            .is_some_and(|x| c.evaluation.nash_conv <= x)
    };
    let mut reached_target = checkpoints.last().is_some_and(target_met);
    if iterations == 0 && checkpoints.is_empty() && o.eval_every > 0 {
        checkpoints.push(checkpoint(0, &profile, &postflop, &mut timings)?);
        reached_target = target_met(checkpoints.last().unwrap());
        let seconds = elapsed + start.elapsed().as_secs_f64();
        observer(&Progress {
            iteration: 0,
            timings: &timings,
            checkpoint: checkpoints.last(),
            average: Some(&average),
            postflop: &postflop,
            seconds,
            finished: reached_target,
            state: super::state::StateRef {
                iteration: 0,
                elapsed: seconds,
                identity: &identity,
                tree,
                profile: &profile,
                regrets: &regrets,
                sums: &sums,
                logit: logit.as_ref(),
                reach: &reach,
                postflop: &postflop,
                regression: &regression,
                timings: &timings,
                checkpoints: &checkpoints,
            },
        })?;
    }
    let mut stopped_by_stop_file = options.stop_file.is_some_and(Path::exists);
    let already_finished = reached_target
        || iterations == o.iterations
        || options.max_seconds.is_some_and(|limit| elapsed >= limit)
        || stopped_by_stop_file;
    if already_finished {
        let take = o.eval_every > 0 && checkpoints.last().is_none_or(|c| c.iteration != iterations);
        if take {
            checkpoints.push(checkpoint(iterations, &average, &postflop, &mut timings)?);
        }
        let seconds = elapsed + start.elapsed().as_secs_f64();
        observer(&Progress {
            iteration: iterations,
            timings: &timings,
            checkpoint: if take { checkpoints.last() } else { None },
            average: Some(&average),
            postflop: &postflop,
            seconds,
            finished: true,
            state: super::state::StateRef {
                iteration: iterations,
                elapsed: seconds,
                identity: &identity,
                tree,
                profile: &profile,
                regrets: &regrets,
                sums: &sums,
                logit: logit.as_ref(),
                reach: &reach,
                postflop: &postflop,
                regression: &regression,
                timings: &timings,
                checkpoints: &checkpoints,
            },
        })?;
    }
    let mut training_boards = TrainingBoards::new(
        options,
        postflop.leaves.iter().map(|s| leaf_spr(tree, s.terminal)),
    );
    let average_mapping = source.relative() && source.relative_average();
    let mut qre_paths = Vec::new();
    let mut qre_mass = Vec::new();
    for t in iterations.saturating_add(1)..=o.iterations {
        if already_finished {
            break;
        }
        let dcfr = Discounts::new(t, o)?;
        let postflop_dcfr = Discounts::new(
            t,
            SolveOptions {
                beta: options.l1_postflop_beta,
                ..o
            },
        )?;
        let phase = Instant::now();
        training_boards.prepare(source, options, None, t);
        if options.qre.is_some() {
            // The smoothed root ranges already include the entire prior.
            for board in training_boards
                .standard
                .iter_mut()
                .chain(&mut training_boards.restricted)
            {
                for strength in board.relative.as_mut().unwrap() {
                    strength.prior = 0.0;
                    strength.prior_fraction = 0.0;
                }
            }
        }
        timings.board_preparation += phase.elapsed().as_secs_f64();
        for p in 0..tree.seats {
            let mut leaves = leaf_values(tree, model, &reach, &[p], o.k4_plan(model, t))?;
            timings.trunk.t2 += leaves.t2;
            timings.trunk.t3 += leaves.t3;
            timings.trunk.k4 += leaves.k4;
            let phase = Instant::now();
            // Snapshot once per player turn, before any parallel leaf work.
            // Sums include earlier players' updates in this iteration.
            let average_reach = average_mapping
                .then(|| solver_reaches(tree, &average_profile(tree, &sums, model), model));
            if let Some(logit) = &logit {
                super::qre::paths(tree, model, logit, &mut qre_paths);
            }
            let outputs: Vec<_> = postflop
                .leaves
                .par_iter_mut()
                .enumerate()
                .map(|(i, storage)| {
                    let z = storage.terminal;
                    let (values, moments) = train_leaf(
                        tree,
                        model,
                        storage,
                        &reach,
                        average_reach.as_deref(),
                        options
                            .qre
                            .map(|_| (&qre_paths[..], source.relative_prior())),
                        p,
                        training_boards.for_spr(leaf_spr(tree, z)),
                        options.l1_train_control,
                        options.l1_train_regression,
                        &postflop_dcfr,
                        options.l1_own_reach_regret,
                    )?;
                    Some((i, z, values, moments))
                })
                .collect();
            let n = f64::from(options.training_boards(t));
            for (i, z, v, moments) in outputs.into_iter().flatten() {
                if options.l1_train_control {
                    // leaf_values left the exact checkdown value (T2) here.
                    let past = &mut regression[i * tree.seats + p];
                    for (c, (value, mut correction)) in
                        leaves.values[p][z].iter_mut().zip(v).enumerate()
                    {
                        let t2 = *value;
                        if let Some(m) = &moments
                            && past.weight > 0.0
                        {
                            let mean = past.sums[0][c] / past.weight;
                            let second = past.sums[1][c] / past.weight;
                            let variance = second - mean * mean;
                            if variance > 1e-12 * second {
                                // Cov(d, x) / Var(x) from earlier iterations.
                                let difference = past.sums[2][c] / past.weight;
                                let slope =
                                    (past.sums[3][c] / past.weight - difference * mean) / variance;
                                correction -= slope * (m[0][c] / n - t2);
                            }
                        }
                        *value = t2 + correction;
                    }
                    if let Some(m) = moments
                        && options.l1_train_regression
                    {
                        past.weight = REGRESSION_DECAY * past.weight + n;
                        for (x, y) in past.sums.iter_mut().flatten().zip(m.iter().flatten()) {
                            *x = REGRESSION_DECAY * *x + y;
                        }
                    }
                } else {
                    leaves.values[p][z].copy_from_slice(&v);
                }
            }
            for (&(z, seat), replacement) in &replacements {
                if seat != p {
                    continue;
                }
                let (opponent, _, scale) = inputs(tree, model, &reach[z], z, p);
                let boards = training_boards.for_spr(leaf_spr(tree, z));
                let mut mass = [0.0; 169];
                for board in boards {
                    let board_mass = class_mass(
                        board,
                        opponent.as_slice(),
                        override_ones.as_ref().unwrap(),
                        p,
                        &scale,
                        override_mass.as_mut().unwrap(),
                    );
                    for (total, value) in mass.iter_mut().zip(board_mass) {
                        *total += value / boards.len() as f64;
                    }
                }
                for (c, value) in replacement.iter().enumerate() {
                    if let Some(value) = value {
                        leaves.values[p][z][c] = value * mass[c];
                    }
                }
            }
            timings.postflop += phase.elapsed().as_secs_f64();
            let phase = Instant::now();
            let values = &mut leaves.values[p];
            backward_values(tree, &profile, &model.support[p], p, values);
            if let (Some(qre), Some(logit)) = (options.qre, &mut logit) {
                super::qre::update(
                    tree,
                    model,
                    &profile,
                    &reach,
                    p,
                    values,
                    qre,
                    logit,
                    &mut qre_mass,
                );
            }
            update_rows(
                tree,
                model,
                p,
                (&mut profile, &mut regrets, &mut sums),
                values,
                &reach,
                &dcfr,
            );
            timings.trunk.update += phase.elapsed().as_secs_f64();
            let phase = Instant::now();
            update_reach(tree, &profile, model, &mut reach, p);
            timings.trunk.reaches += phase.elapsed().as_secs_f64();
        }
        // L1's zero interval explicitly disables evaluation (B7 timing runs).
        stopped_by_stop_file = options.stop_file.is_some_and(Path::exists);
        let mut last = t == o.iterations
            || options
                .max_seconds
                .is_some_and(|limit| elapsed + start.elapsed().as_secs_f64() >= limit)
            || stopped_by_stop_file;
        let take = o.eval_every > 0 && (last || t.is_multiple_of(o.eval_every));
        if take || last {
            average = average_profile(tree, &sums, model);
        }
        if take {
            checkpoints.push(checkpoint(t, &average, &postflop, &mut timings)?);
            reached_target = target_met(checkpoints.last().unwrap());
        }
        // Evaluation is part of solve wall time, too. A checkpoint that
        // consumed the remaining budget already supplies the final average.
        if take {
            stopped_by_stop_file |= options.stop_file.is_some_and(Path::exists);
            last |= options
                .max_seconds
                .is_some_and(|limit| elapsed + start.elapsed().as_secs_f64() >= limit)
                || stopped_by_stop_file;
        }
        iterations = t;
        let seconds = elapsed + start.elapsed().as_secs_f64();
        observer(&Progress {
            iteration: t,
            timings: &timings,
            checkpoint: if take { checkpoints.last() } else { None },
            average: (take || last).then_some(&average),
            postflop: &postflop,
            seconds,
            finished: last || reached_target,
            state: super::state::StateRef {
                iteration: t,
                elapsed: seconds,
                identity: &identity,
                tree,
                profile: &profile,
                regrets: &regrets,
                sums: &sums,
                logit: logit.as_ref(),
                reach: &reach,
                postflop: &postflop,
                regression: &regression,
                timings: &timings,
                checkpoints: &checkpoints,
            },
        })?;
        if reached_target || last {
            break;
        }
    }
    Ok(Solution {
        seconds: elapsed + start.elapsed().as_secs_f64(),
        iterations,
        reached_target,
        stopped_by_stop_file,
        average,
        postflop,
        checkpoints,
        timings,
    })
}

/// `train_leaf`'s per-class values and regression moments.
type LeafTraining = (Vec<f64>, Option<Vec<[f64; 169]>>);

/// One player's Postflop regret and average updates at one L1 leaf over the
/// training boards. `None` when `p` is not at the leaf. Otherwise the
/// per-class board-weighted sum of L1 minus the checkdown (plain L1 without
/// `control`) and, when the opponent reaches the leaf, the regression moments.
/// `mapping` (average-reach relative buckets) pairs these reaches with the
/// leaf's average Postflop rows.
#[allow(clippy::too_many_arguments)]
fn train_leaf(
    tree: &Tree,
    model: &Model<'_>,
    storage: &mut LeafStrategy,
    reach: &[Reach],
    mapping: Option<&[Reach]>,
    qre: Option<(&[Vec<[f64; 169]>], f64)>,
    p: usize,
    boards: &[Board],
    control: bool,
    regression: bool,
    dcfr: &Discounts,
    own_reach_regret: bool,
) -> Option<LeafTraining> {
    let z = storage.terminal;
    let terminal = tree.nodes[z].terminal.as_ref().unwrap();
    let subtree = terminal.l1.as_ref().unwrap();
    if !subtree.active.contains(&p) {
        return None;
    }
    let (mut opponent, mut own, scale) = inputs(tree, model, &reach[z], z, p);
    let factors = if let Some((paths, prior)) = qre {
        let other = subtree.active[usize::from(subtree.active[0] == p)];
        super::qre::add_prior(model, other, &paths[other][z], prior, &mut opponent);
        super::qre::add_prior(model, p, &paths[p][z], prior, &mut own);
        Some(super::qre::residual_factors(
            model, other, &reach[z], &opponent,
        ))
    } else {
        if opponent.iter().all(|&r| r == 0.0) {
            return Some((vec![0.0; 169], None));
        }
        None
    };
    let rows = storage.profile(subtree, false);
    let mapping_inputs = mapping.map(|reach| inputs(tree, model, &reach[z], z, p));
    let mapping_rows = mapping.is_some().then(|| storage.profile(subtree, true));
    let weight = 1.0 / boards.len() as f64;
    let shared = &*storage;
    let mut chunks = boards
        .par_chunks(CHUNK)
        .map_init(scratch, |scratch, chunk| {
            let mut increments = updates(rows.len());
            let mut additions = updates(rows.len());
            // On the heap like inputs().
            let mut values = vec![0.0; 169];
            let mut moments = vec![[0.0; 169]; 4];
            for board in chunk {
                scratch.pass(
                    &Pass {
                        tree: subtree,
                        storage: shared,
                        rows: &rows,
                        board,
                        hero: p,
                        opponent: &opponent,
                        own: &own,
                        scale: &scale,
                        auxiliary: false,
                        mapping: mapping_inputs.as_ref().map(|(opponent, own, _)| Mapping {
                            opponent,
                            own,
                            rows: mapping_rows.as_ref().unwrap(),
                        }),
                        own_reach_regret,
                    },
                    Some((&mut increments, &mut additions, weight)),
                );
                let v = class_values(&scratch.values, &scale);
                let checkdown = if control {
                    scratch.checkdown(board, terminal, p, &opponent, &scale)
                } else {
                    [0.0; 169]
                };
                for c in 0..169 {
                    values[c] += weight * (v[c] - checkdown[c]);
                }
                if regression {
                    for c in 0..169 {
                        let (x, d) = (checkdown[c], v[c] - checkdown[c]);
                        moments[0][c] += x;
                        moments[1][c] += x * x;
                        moments[2][c] += d;
                        moments[3][c] += d * x;
                    }
                }
            }
            (increments, additions, values, moments)
        })
        .collect::<Vec<_>>()
        .into_iter();
    let (mut increments, mut additions, mut values, mut moments) = chunks.next().unwrap();
    for (inc, a, v, m) in chunks {
        for (x, y) in moments.iter_mut().flatten().zip(m.iter().flatten()) {
            *x += y;
        }
        for (x, y) in increments.iter_mut().zip(inc.iter().copied()) {
            *x += y;
        }
        for (x, y) in additions.iter_mut().zip(a.iter().copied()) {
            *x += y;
        }
        for (x, y) in values.iter_mut().zip(v) {
            *x += y;
        }
    }
    discount(storage, subtree, p, &increments, &additions, dcfr);
    if let Some(factors) = factors {
        for (value, factor) in values.iter_mut().zip(factors) {
            *value *= factor;
        }
    }
    Some((values, Some(moments)))
}

fn leaf_spr(tree: &Tree, terminal: usize) -> f64 {
    tree.nodes[terminal]
        .terminal
        .as_ref()
        .unwrap()
        .l1
        .as_ref()
        .unwrap()
        .flop_spr
}

fn validate_subset_routing(options: Options, flops: Option<&FixedFlops>) -> Result<()> {
    if let Some(max_spr) = options.l1_flop_subset_max_spr {
        ensure!(
            max_spr.is_finite() && max_spr >= 0.0,
            "subset MAX_SPR must be finite and nonnegative"
        );
        ensure!(
            options.l1_flop_subset.is_some() || matches!(flops, Some(FixedFlops::Weighted(_))),
            "l1_flop_subset_max_spr requires l1_flop_subset or weighted fixed flops"
        );
    }
    Ok(())
}

/// Prepare only the distributions actually used by the selected leaves.
pub(super) struct TrainingBoards {
    standard: Vec<Board>,
    restricted: Vec<Board>,
    max_spr: Option<f64>,
    need_standard: bool,
    need_restricted: bool,
}

impl TrainingBoards {
    pub(super) fn new(options: Options, sprs: impl Iterator<Item = f64>) -> Self {
        let mut result = Self {
            standard: Vec::new(),
            restricted: Vec::new(),
            max_spr: options.l1_flop_subset_max_spr,
            need_standard: false,
            need_restricted: false,
        };
        for spr in sprs {
            if result.max_spr.is_none_or(|max| spr <= max) {
                result.need_restricted = true;
            } else {
                result.need_standard = true;
            }
        }
        result
    }

    pub(super) fn for_spr(&self, spr: f64) -> &[Board] {
        if self.max_spr.is_none_or(|max| spr <= max) {
            &self.restricted
        } else {
            &self.standard
        }
    }

    pub(super) fn prepare(
        &mut self,
        source: &dyn BucketSource,
        options: Options,
        flops: Option<&FixedFlops>,
        iteration: u64,
    ) {
        let standard_into = |output: &mut Vec<Board>| {
            boards_into(
                source,
                b"solvers.p2.trunk.l1.board.v1",
                options.l1_seed,
                Some(iteration),
                options.training_boards(iteration),
                options.l1_sampling,
                output,
            );
        };
        if self.need_standard {
            standard_into(&mut self.standard);
        }
        if self.need_restricted {
            match flops {
                Some(FixedFlops::RoundRobin(flops)) => runouts_into(
                    source,
                    flops,
                    options.l1_seed,
                    iteration,
                    options.training_boards(iteration),
                    &mut self.restricted,
                ),
                Some(FixedFlops::Weighted(subset)) => subset_boards_into(
                    source,
                    subset,
                    options.l1_seed,
                    iteration,
                    options.training_boards(iteration),
                    &mut self.restricted,
                ),
                None => {
                    if let Some(subset) = options.l1_flop_subset {
                        subset_boards_into(
                            source,
                            subset,
                            options.l1_seed,
                            iteration,
                            options.training_boards(iteration),
                            &mut self.restricted,
                        );
                    } else {
                        standard_into(&mut self.restricted);
                    }
                }
            }
        }
    }
}

/// Trains only the L1 Postflop strategies of `leaves` (trunk terminal ids;
/// `None` = every L1 leaf) while the trunk plays the fixed `profile`, with the
/// training boards, Postflop DCFR and relative-bucket settings of `solve`.
/// The fixed profile is both the current and the average trunk strategy.
/// Other leaves get no accumulators (`Strategies::new_for`): read the saved
/// average with `Strategies::read_average_leaf`. `flops` restricts training to
/// round-robin flops with seeded uniform runouts or weighted stratified flops
/// and runouts. `observer` runs after each iteration; `options.max_seconds`
/// and `options.stop_file` stop after a finished iteration.
#[allow(clippy::too_many_arguments)]
pub fn solve_postflop_fixed(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    profile: &Profile,
    options: Options,
    leaves: Option<&[usize]>,
    flops: Option<&FixedFlops>,
    observer: impl FnMut(u64, &Strategies) -> Result<()>,
) -> Result<Strategies> {
    solve_postflop_fixed_with_state(
        tree, model, source, profile, options, leaves, flops, None, observer,
    )
}

/// Fixed-profile training with optional atomic, finished-iteration checkpoints.
/// Existing state must match all training inputs; the iteration/time bounds
/// may be extended. Board streams are seeded independently by iteration.
#[allow(clippy::too_many_arguments)]
pub fn solve_postflop_fixed_with_state(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    profile: &Profile,
    options: Options,
    leaves: Option<&[usize]>,
    flops: Option<&FixedFlops>,
    state: Option<super::state::FixedState<'_>>,
    mut observer: impl FnMut(u64, &Strategies) -> Result<()>,
) -> Result<Strategies> {
    let o = options.trunk;
    ensure!(o.iterations >= 1, "iterations must be positive");
    ensure!(
        options.max_seconds.is_none_or(|x| x.is_finite() && x > 0.0),
        "max_seconds must be finite and positive"
    );
    ensure!(
        options.l1_postflop_beta.is_finite(),
        "invalid postflop DCFR beta"
    );
    ensure!(options.l1_boards > 0, "l1_boards must be positive");
    ensure!(
        options
            .l1_boards_late
            .is_none_or(|l| l.boards > 0 && l.from > 0),
        "late training boards and their first iteration must be positive"
    );
    ensure!(
        !matches!(flops, Some(FixedFlops::RoundRobin(f)) if f.is_empty()),
        "no flops"
    );
    ensure!(
        flops.is_none() || options.l1_flop_subset.is_none(),
        "fixed flops and l1_flop_subset cannot be combined"
    );
    validate_subset_routing(options, flops)?;
    ensure!(
        tree.game_fingerprint == model.game().game_fingerprint(),
        "model/tree mismatch"
    );
    profile.validate(tree)?;
    // Only the trained leaves get accumulators: the others are never touched.
    let mut postflop = Strategies::new_for(tree, source, leaves);
    if let Some(leaves) = leaves {
        for z in leaves {
            ensure!(
                postflop.leaves.iter().any(|s| s.terminal == *z),
                "node {z} is not an L1 leaf"
            );
        }
    }
    let identity = state
        .map(|state| {
            ensure!(
                state.every_seconds.is_finite() && state.every_seconds > 0.0,
                "state interval must be finite and positive"
            );
            Ok(super::state::fixed_identity(
                super::state::identity(tree, model, source, options, &postflop)?,
                profile,
                &postflop,
                leaves,
                flops,
            ))
        })
        .transpose()?;
    let (mut iteration, elapsed) = if let Some(state) = state.filter(|s| s.path.exists()) {
        super::state::read_fixed(state.path, identity.as_ref().unwrap(), &mut postflop)?
    } else {
        (0, 0.0)
    };
    let reach = solver_reaches(tree, profile, model);
    let mapping = (source.relative() && source.relative_average()).then_some(&reach[..]);
    let start = Instant::now();
    let mut last_save = Instant::now();
    let mut boards = TrainingBoards::new(
        options,
        postflop
            .leaves
            .iter()
            .filter(|s| leaves.is_none_or(|l| l.contains(&s.terminal)))
            .map(|s| leaf_spr(tree, s.terminal)),
    );
    for t in iteration.saturating_add(1)..=o.iterations {
        let dcfr = Discounts::new(
            t,
            SolveOptions {
                beta: options.l1_postflop_beta,
                ..o
            },
        )?;
        boards.prepare(source, options, flops, t);
        for p in 0..tree.seats {
            postflop
                .leaves
                .par_iter_mut()
                .filter(|s| leaves.is_none_or(|l| l.contains(&s.terminal)))
                .for_each(|storage| {
                    let leaf_boards = boards.for_spr(leaf_spr(tree, storage.terminal));
                    train_leaf(
                        tree,
                        model,
                        storage,
                        &reach,
                        mapping,
                        None,
                        p,
                        leaf_boards,
                        false,
                        false,
                        &dcfr,
                        options.l1_own_reach_regret,
                    );
                });
        }
        iteration = t;
        observer(t, &postflop)?;
        let finished = t == o.iterations
            || options
                .max_seconds
                .is_some_and(|limit| elapsed + start.elapsed().as_secs_f64() >= limit)
            || options.stop_file.is_some_and(Path::exists);
        if let Some(state) = state
            && !finished
            && last_save.elapsed().as_secs_f64() >= state.every_seconds
        {
            super::state::write_fixed(
                state.path,
                identity.as_ref().unwrap(),
                iteration,
                elapsed + start.elapsed().as_secs_f64(),
                &postflop,
            )?;
            last_save = Instant::now();
        }
        if finished {
            break;
        }
    }
    if let Some(state) = state {
        super::state::write_fixed(
            state.path,
            identity.as_ref().unwrap(),
            iteration,
            elapsed + start.elapsed().as_secs_f64(),
            &postflop,
        )?;
    }
    Ok(postflop)
}
