use super::{
    BucketSource, Evaluation, Strategies,
    cards::boards,
    evaluate,
    pass::{Pass, Scratch, class_values, discount, inputs},
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
    /// The same control variate for the evaluator's leaf estimates.
    pub l1_eval_control: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            trunk: SolveOptions::default(),
            l1_boards: 1,
            l1_seed: 0,
            l1_eval_boards: 1024,
            l1_eval_seed: 0,
            l1_train_control: false,
            l1_eval_control: false,
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
    for t in 1..=o.iterations {
        let dcfr = Discounts::new(t, o)?;
        let phase = Instant::now();
        let training_boards = boards(
            source,
            b"solvers.p2.trunk.l1.board.v1",
            options.l1_seed,
            Some(t),
            options.l1_boards,
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
                .map_init(Scratch::default, |scratch, storage| {
                    let z = storage.terminal;
                    let terminal = tree.nodes[z].terminal.as_ref().unwrap();
                    let subtree = terminal.l1.as_ref().unwrap();
                    if !subtree.active.contains(&p) {
                        return None;
                    }
                    let (opponent, own, scale) = inputs(tree, model, &reach[z], z, p);
                    if opponent.iter().all(|&r| r == 0.0) {
                        return Some((z, [0.0; 169]));
                    }
                    let rows = storage.profile(subtree, false);
                    let mut increments = vec![0.0; rows.len()];
                    let mut additions = vec![0.0; rows.len()];
                    let mut values = [0.0; 169];
                    let weight = 1.0 / training_boards.len() as f64;
                    for board in &training_boards {
                        scratch.pass(
                            &Pass {
                                tree: subtree,
                                storage,
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
                    }
                    discount(storage, subtree, p, &increments, &additions, &dcfr);
                    Some((z, values))
                })
                .collect();
            for (z, v) in outputs.into_iter().flatten() {
                if options.l1_train_control {
                    // leaf_values left the exact checkdown value here.
                    for (value, correction) in leaves.values[p][z].iter_mut().zip(v) {
                        *value += correction;
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
