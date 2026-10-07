use super::{
    BucketSource, Evaluation, Sampling, Strategies,
    cards::boards,
    evaluate,
    pass::{CHUNK, Pass, Scratch, class_values, discount, inputs},
};
use crate::trunk::l0::{
    Model, Profile, SolveOptions, SolveTimings, Tree,
    eval::{solver_reaches, update_reach},
    leaves::leaf_values,
    solve::{Discounts, average_profile, backward_values, regret_matching, update_row},
};
use crate::{ExternalSamplingGame, Street};
use anyhow::{Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::time::Instant;

/// Per-iteration decay of the moments behind the training regression
/// coefficient. Only earlier iterations' boards enter it, so each iteration's
/// estimate stays unbiased.
const REGRESSION_DECAY: f64 = 0.95;

/// Decayed sums over boards of the checkdown `x`, `x²`, the difference
/// `d = L1 − x` and `d·x`, per class.
#[derive(Clone)]
struct Moments {
    weight: f64,
    sums: [[f64; 169]; 4],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Options {
    #[serde(flatten)]
    pub trunk: SolveOptions,
    pub l1_boards: u32,
    pub l1_seed: u64,
    pub l1_eval_boards: u32,
    pub l1_eval_seed: u64,
    /// Train on the exact L0 checkdown value plus the sampled boards' mean
    /// L1-minus-checkdown difference (an unbiased control variate).
    pub l1_train_control: bool,
    /// Scale the training control variate by a regression coefficient per
    /// leaf, seat and class fitted on earlier iterations (requires
    /// `l1_train_control`).
    pub l1_train_regression: bool,
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
}

/// The settings S4-2a measured on B6: 32 stratified boards per iteration, both
/// control variates with their regression coefficients, and DCFR's
/// nonpositive-regret exponent 1 for the trunk instead of L0's 0, which keeps
/// the trunk from chasing the boards' noise. The postflop strategies keep 0:
/// 1 there did not lower the primary metric and made them more exploitable
/// with the real board.
impl Default for Options {
    fn default() -> Self {
        Self {
            trunk: SolveOptions {
                beta: 1.0,
                ..SolveOptions::default()
            },
            l1_boards: 32,
            l1_seed: 0,
            l1_eval_boards: 1024,
            l1_eval_seed: 0,
            l1_train_control: true,
            l1_train_regression: true,
            l1_eval_control: true,
            l1_eval_regression: true,
            l1_sampling: Sampling::Stratified,
            l1_eval_sampling: Sampling::Random,
            l1_postflop_beta: 0.0,
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
}

pub struct Solution {
    pub iterations: u64,
    pub reached_target: bool,
    pub average: Profile,
    pub postflop: Strategies,
    pub checkpoints: Vec<Checkpoint>,
    pub timings: Timings,
}

pub fn solve(
    tree: &Tree,
    model: &Model<'_>,
    source: &dyn BucketSource,
    options: Options,
    mut observer: impl FnMut(&Progress<'_>),
) -> Result<Solution> {
    let o = options.trunk;
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
    ensure!(options.l1_boards > 0, "l1_boards must be positive");
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
        boards(
            source,
            b"solvers.p2.trunk.l1.eval.v1",
            options.l1_eval_seed,
            None,
            options.l1_eval_boards,
            options.l1_eval_sampling,
        )
    } else {
        Vec::new()
    };
    timings.evaluation_board_preparation = phase.elapsed().as_secs_f64();
    let mut postflop = Strategies::new(tree, source);
    let mut profile = Profile::uniform(tree);
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
    let mut average = profile.clone();
    let mut checkpoints = Vec::new();
    let checkpoint = |iteration,
                      average: &Profile,
                      postflop: &Strategies,
                      timings: &mut Timings|
     -> Result<Checkpoint> {
        let e = evaluate(
            tree,
            average,
            model,
            postflop,
            &eval_boards,
            options.l1_eval_control,
            options.l1_eval_regression,
        )?;
        timings.trunk.evaluation += e.seconds;
        Ok(Checkpoint {
            iteration,
            evaluation: e,
            seconds: start.elapsed().as_secs_f64(),
        })
    };
    let target_met = |c: &Checkpoint| {
        o.target_nash_conv
            .is_some_and(|x| c.evaluation.nash_conv <= x)
    };
    if o.eval_every > 0 {
        checkpoints.push(checkpoint(0, &profile, &postflop, &mut timings)?);
        observer(&Progress {
            iteration: 0,
            timings: &timings,
            checkpoint: checkpoints.last(),
        });
        if target_met(checkpoints.last().unwrap()) {
            return Ok(Solution {
                iterations: 0,
                reached_target: true,
                average,
                postflop,
                checkpoints,
                timings,
            });
        }
    }
    let phase = Instant::now();
    let mut reach = solver_reaches(tree, &profile, model);
    timings.trunk.reaches += phase.elapsed().as_secs_f64();
    let mut iterations = 0;
    let mut reached_target = false;
    let mut regression = vec![
        Moments {
            weight: 0.0,
            sums: [[0.0; 169]; 4],
        };
        postflop.leaves.len() * tree.seats
    ];
    for t in 1..=o.iterations {
        let dcfr = Discounts::new(t, o)?;
        let postflop_dcfr = Discounts::new(
            t,
            SolveOptions {
                beta: options.l1_postflop_beta,
                ..o
            },
        )?;
        let phase = Instant::now();
        let training_boards = boards(
            source,
            b"solvers.p2.trunk.l1.board.v1",
            options.l1_seed,
            Some(t),
            options.l1_boards,
            options.l1_sampling,
        );
        timings.board_preparation += phase.elapsed().as_secs_f64();
        for p in 0..tree.seats {
            let mut leaves = leaf_values(tree, model, &reach, &[p], o.k4_plan(model, t))?;
            timings.trunk.t2 += leaves.t2;
            timings.trunk.t3 += leaves.t3;
            timings.trunk.k4 += leaves.k4;
            let phase = Instant::now();
            let outputs: Vec<_> = postflop
                .leaves
                .par_iter_mut()
                .enumerate()
                .map(|(i, storage)| {
                    let z = storage.terminal;
                    let terminal = tree.nodes[z].terminal.as_ref().unwrap();
                    let subtree = terminal.l1.as_ref().unwrap();
                    if !subtree.active.contains(&p) {
                        return None;
                    }
                    let (opponent, own, scale) = inputs(tree, model, &reach[z], z, p);
                    if opponent.iter().all(|&r| r == 0.0) {
                        return Some((i, z, [0.0; 169], None));
                    }
                    let rows = storage.profile(subtree, false);
                    let weight = 1.0 / training_boards.len() as f64;
                    let shared = &*storage;
                    let mut chunks = training_boards
                        .par_chunks(CHUNK)
                        .map_init(Scratch::default, |scratch, chunk| {
                            let mut increments = vec![0.0; rows.len()];
                            let mut additions = vec![0.0; rows.len()];
                            let mut values = [0.0; 169];
                            let mut moments = [[0.0; 169]; 4];
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
                                    },
                                    Some((&mut increments, &mut additions, weight)),
                                );
                                let v = class_values(&scratch.values, &scale);
                                let checkdown = if options.l1_train_control {
                                    scratch.checkdown(board, terminal, p, &opponent, &scale)
                                } else {
                                    [0.0; 169]
                                };
                                for c in 0..169 {
                                    values[c] += weight * (v[c] - checkdown[c]);
                                }
                                if options.l1_train_regression {
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
                    let (mut increments, mut additions, mut values, mut moments) =
                        chunks.next().unwrap();
                    for (inc, a, v, m) in chunks {
                        for (x, y) in moments.iter_mut().flatten().zip(m.iter().flatten()) {
                            *x += y;
                        }
                        for (x, y) in increments.iter_mut().zip(inc) {
                            *x += y;
                        }
                        for (x, y) in additions.iter_mut().zip(a) {
                            *x += y;
                        }
                        for (x, y) in values.iter_mut().zip(v) {
                            *x += y;
                        }
                    }
                    discount(storage, subtree, p, &increments, &additions, &postflop_dcfr);
                    Some((i, z, values, Some(moments)))
                })
                .collect();
            let n = training_boards.len() as f64;
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
                    leaves.values[p][z] = v;
                }
            }
            timings.postflop += phase.elapsed().as_secs_f64();
            let phase = Instant::now();
            let values = &mut leaves.values[p];
            backward_values(tree, &profile, &model.support[p], p, values);
            for (z, n) in tree.nodes.iter().enumerate() {
                if n.actor != Some(p) {
                    continue;
                }
                for &c in &model.support[p] {
                    let r = regrets.row_mut(tree, z, c);
                    update_row(
                        r,
                        sums.row_mut(tree, z, c),
                        profile.row(tree, z, c),
                        n.children.iter().map(|&child| values[child][c]),
                        values[z][c],
                        reach[z].pi(p, c),
                        &dcfr,
                    );
                    regret_matching(r, profile.row_mut(tree, z, c));
                }
            }
            timings.trunk.update += phase.elapsed().as_secs_f64();
            let phase = Instant::now();
            update_reach(tree, &profile, model, &mut reach, p);
            timings.trunk.reaches += phase.elapsed().as_secs_f64();
        }
        // L1's zero interval explicitly disables evaluation (B7 timing runs).
        let take = o.eval_every > 0 && (t == o.iterations || t.is_multiple_of(o.eval_every));
        if take || t == o.iterations {
            average = average_profile(tree, &sums, model);
        }
        if take {
            checkpoints.push(checkpoint(t, &average, &postflop, &mut timings)?);
            reached_target = target_met(checkpoints.last().unwrap());
        }
        iterations = t;
        observer(&Progress {
            iteration: t,
            timings: &timings,
            checkpoint: if take { checkpoints.last() } else { None },
        });
        if reached_target {
            break;
        }
    }
    Ok(Solution {
        iterations,
        reached_target,
        average,
        postflop,
        checkpoints,
        timings,
    })
}
