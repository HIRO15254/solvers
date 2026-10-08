//! Alternating, full-width DCFR in the hero-conditioned L0 game.
use anyhow::{Result, ensure};
use serde::Serialize;
use std::time::Instant;

use super::{Model, Profile, Tree, evaluate};
use super::{
    eval::{solver_reaches, update_reach},
    leaves::{K4Plan, leaf_values},
};
use crate::ExternalSamplingGame;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct SolveOptions {
    /// Maximum number of iterations (at least one).
    pub iterations: u64,
    /// Positive regret discount exponent.
    pub alpha: f64,
    /// Nonpositive regret discount exponent.
    pub beta: f64,
    /// Own-reach average strategy weight exponent (nonnegative).
    pub gamma: f64,
    /// Checkpoint interval; zero evaluates only the final average.
    pub eval_every: u64,
    /// Stop at the first checkpoint at or below this NashConv.
    pub target_nash_conv: Option<f64>,
    /// Solver-only K4 budget; None retains the model's fixed sample stream.
    pub k4_samples: Option<u64>,
    /// Reach-scaled minimum budget; requires k4_samples, 1 <= min <= samples.
    pub k4_min_samples: Option<u64>,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self {
            iterations: 1000,
            alpha: 1.5,
            beta: 0.0,
            gamma: 2.0,
            eval_every: 100,
            target_nash_conv: None,
            k4_samples: None,
            k4_min_samples: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CheckpointSeat {
    pub seat: usize,
    pub value: f64,
    pub best_response: f64,
    pub gain: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Checkpoint {
    /// Zero denotes the initial uniform profile.
    pub iteration: u64,
    pub nash_conv: f64,
    pub seats: Vec<CheckpointSeat>,
    /// Wall time since solve began.
    pub seconds: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SolveTimings {
    pub reaches: f64,
    pub t2: f64,
    pub t3: f64,
    pub k4: f64,
    pub update: f64,
    pub evaluation: f64,
}

pub struct Progress<'a> {
    pub iteration: u64,
    pub timings: &'a SolveTimings,
    pub checkpoint: Option<&'a Checkpoint>,
}

pub struct Solution {
    pub iterations: u64,
    pub reached_target: bool,
    /// Explicit own-reach weighted average rows, including unsupported classes.
    pub average: Profile,
    pub checkpoints: Vec<Checkpoint>,
    pub timings: SolveTimings,
}

impl SolveOptions {
    pub(crate) fn k4_plan(self, model: &Model<'_>, iteration: u64) -> K4Plan {
        let Some(samples) = self.k4_samples else {
            return K4Plan::model(model);
        };
        let mut hash = blake3::Hasher::new();
        hash.update(b"solvers.p2.trunk.l0.solve.k4.v1");
        hash.update(&model.options.seed.to_le_bytes());
        hash.update(&iteration.to_le_bytes());
        let mut seed = u64::from_le_bytes(hash.finalize().as_bytes()[..8].try_into().unwrap());
        // Reserve the model seed even in the event of a truncated-hash collision.
        if seed == model.options.seed {
            seed = seed.wrapping_add(1);
        }
        K4Plan {
            samples,
            min_samples: self.k4_min_samples,
            seed,
        }
    }
}

pub(crate) fn regret_matching(regrets: &[f64], row: &mut [f64]) {
    let sum: f64 = regrets.iter().map(|r| r.max(0.0)).sum();
    if sum > 0.0 {
        for (s, r) in row.iter_mut().zip(regrets) {
            *s = r.max(0.0) / sum;
        }
    } else {
        row.fill(1.0 / row.len() as f64);
    }
}

pub(crate) struct Discounts {
    pub(crate) positive: f64,
    pub(crate) nonpositive: f64,
    pub(crate) average: f64,
}

impl Discounts {
    pub(crate) fn new(t: u64, options: SolveOptions) -> Result<Self> {
        let factor = |exponent: f64| {
            let x = (t as f64).powf(exponent);
            if x.is_infinite() { 1.0 } else { x / (x + 1.0) }
        };
        let average = (t as f64).powf(options.gamma);
        ensure!(
            average.is_finite(),
            "average weight overflow at iteration {t}"
        );
        Ok(Self {
            positive: factor(options.alpha),
            nonpositive: factor(options.beta),
            average,
        })
    }
}

pub(crate) fn update_row(
    regrets: &mut [f64],
    sums: &mut [f64],
    row: &[f64],
    children: impl Iterator<Item = f64>,
    value: f64,
    own_reach: f64,
    discount: &Discounts,
) {
    for (((r, s), &probability), child) in regrets.iter_mut().zip(sums).zip(row).zip(children) {
        *r += child - value;
        *r *= if *r > 0.0 {
            discount.positive
        } else {
            discount.nonpositive
        };
        *s += discount.average * own_reach * probability;
    }
}

pub(crate) fn backward_values(
    tree: &Tree,
    profile: &Profile,
    support: &[usize],
    seat: usize,
    values: &mut [[f64; 169]],
) {
    for z in (0..tree.nodes.len()).rev() {
        let node = &tree.nodes[z];
        if node.terminal.is_some() {
            continue;
        }
        for &c in support {
            values[z][c] = if node.actor == Some(seat) {
                let row = profile.row(tree, z, c);
                let mut value = 0.0;
                for (a, &child) in node.children.iter().enumerate() {
                    value += row[a] * values[child][c];
                }
                value
            } else {
                node.children.iter().map(|&child| values[child][c]).sum()
            };
        }
    }
}

pub(crate) fn average_profile(tree: &Tree, sums: &Profile, model: &Model<'_>) -> Profile {
    let mut average = Profile::uniform(tree);
    for (z, node) in tree.nodes.iter().enumerate() {
        let Some(p) = node.actor else { continue };
        for &c in &model.support[p] {
            let sum: f64 = sums.row(tree, z, c).iter().sum();
            if sum > 0.0 {
                for (a, &s) in average
                    .row_mut(tree, z, c)
                    .iter_mut()
                    .zip(sums.row(tree, z, c))
                {
                    *a = s / sum;
                }
            }
        }
    }
    average
}

/// Solve with alternating seat updates and evaluator-certified average checkpoints.
pub fn solve(
    tree: &Tree,
    model: &Model<'_>,
    options: SolveOptions,
    mut observer: impl FnMut(&Progress<'_>),
) -> Result<Solution> {
    ensure!(tree.l1_leaf_count() == 0, "L1 leaves require the L1 solver");
    ensure!(options.iterations >= 1, "iterations must be positive");
    ensure!(
        options.alpha.is_finite()
            && options.beta.is_finite()
            && options.gamma.is_finite()
            && options.gamma >= 0.0,
        "discount exponents must be finite and gamma nonnegative"
    );
    ensure!(
        options
            .target_nash_conv
            .is_none_or(|x| x.is_finite() && x >= 0.0),
        "target NashConv must be finite and nonnegative"
    );
    ensure!(
        tree.game_fingerprint == model.game().game_fingerprint(),
        "model/tree mismatch"
    );
    ensure!(
        tree.nodes.iter().all(|n| n.children.len() <= 254),
        "L0 nodes may have at most 254 actions"
    );
    ensure!(
        options.k4_samples != Some(0),
        "solver K4 samples must be positive"
    );
    ensure!(
        options
            .k4_min_samples
            .is_none_or(|min| { min >= 1 && options.k4_samples.is_some_and(|n| min <= n) }),
        "solver K4 minimum requires samples and 1 <= min <= samples"
    );
    let start = Instant::now();
    let mut profile = Profile::uniform(tree);
    let mut regrets = profile.clone();
    let mut sums = profile.clone();
    for (z, node) in tree.nodes.iter().enumerate() {
        if node.actor.is_some() {
            for c in 0..169 {
                regrets.row_mut(tree, z, c).fill(0.0);
                sums.row_mut(tree, z, c).fill(0.0);
            }
        }
    }
    let mut timings = SolveTimings::default();
    let mut checkpoints = Vec::new();
    let checkpoint =
        |iteration, average: &Profile, timings: &mut SolveTimings| -> Result<Checkpoint> {
            let evaluation_start = Instant::now();
            let evaluation = evaluate(tree, average, model)?;
            timings.evaluation += evaluation_start.elapsed().as_secs_f64();
            Ok(Checkpoint {
                iteration,
                nash_conv: evaluation.nash_conv,
                seats: evaluation
                    .seats
                    .into_iter()
                    .map(|s| CheckpointSeat {
                        seat: s.seat,
                        value: s.value,
                        best_response: s.best_response,
                        gain: s.gain,
                    })
                    .collect(),
                seconds: start.elapsed().as_secs_f64(),
            })
        };
    let target_met = |c: &Checkpoint| options.target_nash_conv.is_some_and(|x| c.nash_conv <= x);
    if options.eval_every > 0 {
        checkpoints.push(checkpoint(0, &profile, &mut timings)?);
        observer(&Progress {
            iteration: 0,
            timings: &timings,
            checkpoint: checkpoints.last(),
        });
        if target_met(checkpoints.last().unwrap()) {
            return Ok(Solution {
                iterations: 0,
                reached_target: true,
                average: profile,
                checkpoints,
                timings,
            });
        }
    }
    let mut average = profile.clone();
    let mut iterations = 0;
    let mut reached_target = false;
    let phase = Instant::now();
    let mut reach = solver_reaches(tree, &profile, model);
    timings.reaches += phase.elapsed().as_secs_f64();
    for t in 1..=options.iterations {
        let discount = Discounts::new(t, options)?;
        let plan = options.k4_plan(model, t);
        for p in 0..tree.seats {
            let mut leaves = leaf_values(tree, model, &reach, &[p], plan)?;
            timings.t2 += leaves.t2;
            timings.t3 += leaves.t3;
            timings.k4 += leaves.k4;
            let phase = Instant::now();
            let values = &mut leaves.values[p];
            backward_values(tree, &profile, &model.support[p], p, values);
            for (z, node) in tree.nodes.iter().enumerate() {
                if node.actor != Some(p) {
                    continue;
                }
                for &c in &model.support[p] {
                    let regrets = regrets.row_mut(tree, z, c);
                    update_row(
                        regrets,
                        sums.row_mut(tree, z, c),
                        profile.row(tree, z, c),
                        node.children.iter().map(|&child| values[child][c]),
                        values[z][c],
                        reach[z].pi(p, c),
                        &discount,
                    );
                    regret_matching(regrets, profile.row_mut(tree, z, c));
                }
            }
            timings.update += phase.elapsed().as_secs_f64();
            let phase = Instant::now();
            update_reach(tree, &profile, model, &mut reach, p);
            timings.reaches += phase.elapsed().as_secs_f64();
        }
        let take_checkpoint =
            t == options.iterations || (options.eval_every > 0 && t % options.eval_every == 0);
        if take_checkpoint {
            let phase = Instant::now();
            average = average_profile(tree, &sums, model);
            timings.update += phase.elapsed().as_secs_f64();
            checkpoints.push(checkpoint(t, &average, &mut timings)?);
            reached_target = target_met(checkpoints.last().unwrap());
        }
        iterations = t;
        observer(&Progress {
            iteration: t,
            timings: &timings,
            checkpoint: if take_checkpoint {
                checkpoints.last()
            } else {
                None
            },
        });
        if reached_target {
            break;
        }
    }
    Ok(Solution {
        iterations,
        reached_target,
        average,
        checkpoints,
        timings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcfr_row_arithmetic_and_matching() {
        let mut regrets = [0.0; 3];
        let mut sums = [0.0; 3];
        let mut row = [1.0 / 3.0; 3];
        // t1: uniform value 1, regrets [1,-1,0], sums [1/6;3].
        // t2: pure action 0, value -2; undiscounted regrets [1,5,3].
        // t3: strategy [1,5,3]/9, value 2/9, own reach 3/4.
        let d2 = 2.0_f64.powf(1.5) / (2.0_f64.powf(1.5) + 1.0);
        let d3 = 3.0_f64.powf(1.5) / (3.0_f64.powf(1.5) + 1.0);
        let expected = [
            ([1.0, -1.0, 0.0], [1.0 / 6.0; 3]),
            ([d2, 5.0 * d2, 3.0 * d2], [7.0 / 6.0, 1.0 / 6.0, 1.0 / 6.0]),
            (
                [
                    (d2 + 7.0 / 9.0) * d3,
                    (5.0 * d2 + 16.0 / 9.0) * d3,
                    (3.0 * d2 - 29.0 / 9.0) / 2.0,
                ],
                [23.0 / 12.0, 47.0 / 12.0, 29.0 / 12.0],
            ),
        ];
        for (i, (children, reach)) in [
            ([3.0, -1.0, 1.0], 0.5),
            ([-2.0, 4.0, 1.0], 0.25),
            ([1.0, 2.0, -3.0], 0.75),
        ]
        .into_iter()
        .enumerate()
        {
            let value: f64 = row.iter().zip(children).map(|(s, v)| s * v).sum();
            update_row(
                &mut regrets,
                &mut sums,
                &row,
                children.into_iter(),
                value,
                reach,
                &Discounts::new(i as u64 + 1, SolveOptions::default()).unwrap(),
            );
            for a in 0..3 {
                assert!((regrets[a] - expected[i].0[a]).abs() < 1e-14);
                assert!((sums[a] - expected[i].1[a]).abs() < 1e-14);
            }
            regret_matching(&regrets, &mut row);
        }
        regret_matching(&[-1.0, 0.0, -2.0], &mut row);
        assert_eq!(row, [1.0 / 3.0; 3]);
        regret_matching(&[-1.0, 2.0, 6.0], &mut row);
        assert_eq!(row, [0.0, 0.25, 0.75]);
    }
}
