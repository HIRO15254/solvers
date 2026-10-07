use super::{
    Board, Strategies,
    pass::{Pass, Scratch, class_values, inputs},
};
use crate::ExternalSamplingGame;
use crate::trunk::{
    classes::Classes,
    l0::{
        Model, Profile, Tree,
        eval::{Reach, reaches},
        leaves::{K4Plan, leaf_values},
    },
};
use anyhow::{Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeatEvaluation {
    pub seat: usize,
    pub value: f64,
    pub value_a: f64,
    pub value_b: f64,
    pub best_response: f64,
    pub gain: f64,
    pub held_a_to_b: f64,
    pub held_b_to_a: f64,
    pub held_gain: f64,
    pub auxiliary_best_response: f64,
    pub auxiliary_gain: f64,
    pub auxiliary_held_a_to_b: f64,
    pub auxiliary_held_b_to_a: f64,
    pub auxiliary_held_gain: f64,
    pub l1_reach_share: f64,
    pub telescoping_residual: f64,
    pub auxiliary_telescoping_residual: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evaluation {
    pub nash_conv: f64,
    pub held_nash_conv: f64,
    pub auxiliary_nash_conv: f64,
    pub auxiliary_held_nash_conv: f64,
    pub seats: Vec<SeatEvaluation>,
    #[serde(rename = "evaluation_seconds")]
    pub seconds: f64,
}

type Slab = Vec<[f64; 169]>;

fn aggregate(model: &Model<'_>, p: usize, values: &[f64; 169]) -> f64 {
    model.support[p]
        .iter()
        .map(|&c| Classes::get().n(c) as f64 * model.weights[p][c] * values[c])
        .sum::<f64>()
        / model.normalizers[p]
}

struct Backward {
    value: f64,
    best: f64,
    actions: Vec<u8>,
    residual: f64,
}

/// Both in-sample passes include their leaf deviation contribution in the
/// telescoping check. Primary leaves have a zero contribution.
fn backward(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    reach: &[Reach],
    p: usize,
    values: &Slab,
    best: &Slab,
) -> Result<Backward> {
    let mut values = values.clone();
    let mut best = best.clone();
    let mut actions = vec![0; tree.nodes.len() * 169];
    let mut local_total = 0.0;
    let mut terminal_value = 0.0;
    let mut terminal_reach = 0.0;
    for (z, n) in tree.nodes.iter().enumerate().rev() {
        for &c in &model.support[p] {
            let weight = Classes::get().n(c) as f64 * model.weights[p][c] / model.normalizers[p];
            if n.terminal.is_some() {
                local_total += weight * reach[z].pi(p, c) * (best[z][c] - values[z][c]);
                terminal_value += weight * reach[z].pi(p, c) * values[z][c];
                terminal_reach += weight
                    * reach[z].pi(p, c)
                    * (0..tree.seats)
                        .filter(|&s| s != p)
                        .map(|s| reach[z].mass(s, c))
                        .product::<f64>();
            } else if n.actor == Some(p) {
                let row = profile.row(tree, z, c);
                let mut maximum = f64::NEG_INFINITY;
                let mut mixed = 0.0;
                let mut value = 0.0;
                for (a, &child) in n.children.iter().enumerate() {
                    value += row[a] * values[child][c];
                    mixed += row[a] * best[child][c];
                    if best[child][c] > maximum {
                        maximum = best[child][c];
                        actions[z * 169 + c] = a as u8;
                    }
                }
                values[z][c] = value;
                best[z][c] = maximum;
                local_total += weight * reach[z].pi(p, c) * (maximum - mixed);
            } else {
                values[z][c] = n.children.iter().map(|&child| values[child][c]).sum();
                best[z][c] = n.children.iter().map(|&child| best[child][c]).sum();
            }
        }
    }
    let value = aggregate(model, p, &values[0]);
    let best = aggregate(model, p, &best[0]);
    let gain = best - value;
    let residual = local_total - gain;
    ensure!(
        gain >= -1e-9 * (1.0 + value.abs()),
        "L1 BR below profile value for seat {p}"
    );
    ensure!(
        residual.abs() <= 1e-9 * (1.0 + gain.abs()),
        "L1 local gains do not telescope for seat {p}: {residual}"
    );
    ensure!(
        (terminal_reach - 1.0).abs() <= 1e-9,
        "L1 terminal reach does not sum to one"
    );
    ensure!(
        (terminal_value - value).abs() <= 1e-9 * (1.0 + value.abs()),
        "L1 terminal values do not sum to value"
    );
    Ok(Backward {
        value,
        best,
        actions,
        residual,
    })
}

fn fixed_response(tree: &Tree, model: &Model<'_>, p: usize, leaves: &Slab, actions: &[u8]) -> f64 {
    let mut v = leaves.clone();
    for (z, n) in tree.nodes.iter().enumerate().rev() {
        if n.terminal.is_some() {
            continue;
        }
        for &c in &model.support[p] {
            v[z][c] = if n.actor == Some(p) {
                v[n.children[actions[z * 169 + c] as usize]][c]
            } else {
                n.children.iter().map(|&child| v[child][c]).sum()
            };
        }
    }
    aggregate(model, p, &v[0])
}

pub fn evaluate(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    strategies: &Strategies,
    boards: &[Board],
    control: bool,
) -> Result<Evaluation> {
    ensure!(
        boards.len() >= 2 && boards.len().is_multiple_of(2),
        "L1 evaluation boards must be even and >= 2"
    );
    profile.validate(tree)?;
    ensure!(
        tree.game_fingerprint == model.game().game_fingerprint(),
        "model/tree mismatch"
    );
    let start = Instant::now();
    let reach = reaches(tree, profile, model);
    // K4/T3 is computed exactly once, shared by all primary/auxiliary halves.
    let base = leaf_values(
        tree,
        model,
        &reach,
        &(0..tree.seats).collect::<Vec<_>>(),
        K4Plan::model(model),
    )?
    .values;
    let mut seats = Vec::new();
    for (p, base) in base.into_iter().enumerate() {
        let mut sigma = [base.clone(), base.clone()];
        let mut beta = [base.clone(), base];
        let output: Vec<_> = strategies
            .leaves
            .par_iter()
            .map_init(Scratch::default, |scratch, storage| {
                let z = storage.terminal;
                let terminal = tree.nodes[z].terminal.as_ref().unwrap();
                let subtree = terminal.l1.as_ref().unwrap();
                if !subtree.active.contains(&p) {
                    return None;
                }
                let rows = storage.profile(subtree, true);
                let (opponent, own, scale) = inputs(tree, model, &reach[z], z, p);
                let mut sigma = [[0.0; 169]; 2];
                let mut beta = [[0.0; 169]; 2];
                if opponent.iter().any(|&r| r != 0.0) {
                    for (j, board) in boards.iter().enumerate() {
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
                                auxiliary: true,
                            },
                            None,
                        );
                        let s = class_values(&scratch.values, &scale);
                        let b = class_values(&scratch.best, &scale);
                        let checkdown = if control {
                            scratch.checkdown(board, terminal, p, &opponent, &scale)
                        } else {
                            [0.0; 169]
                        };
                        for c in 0..169 {
                            sigma[j % 2][c] += (s[c] - checkdown[c]) / (boards.len() / 2) as f64;
                            beta[j % 2][c] += (b[c] - checkdown[c]) / (boards.len() / 2) as f64;
                        }
                    }
                }
                Some((z, sigma, beta))
            })
            .collect();
        for (z, s, b) in output.into_iter().flatten() {
            for h in 0..2 {
                if control {
                    // leaf_values left the exact checkdown value here.
                    for c in 0..169 {
                        sigma[h][z][c] += s[h][c];
                        beta[h][z][c] += b[h][c];
                    }
                } else {
                    sigma[h][z] = s[h];
                    beta[h][z] = b[h];
                }
            }
        }
        let all = |halves: &[Slab; 2]| {
            halves[0]
                .iter()
                .zip(&halves[1])
                .map(|(a, b)| std::array::from_fn(|c| (a[c] + b[c]) * 0.5))
                .collect::<Slab>()
        };
        let sa = all(&sigma);
        let ba = all(&beta);
        let primary = backward(tree, profile, model, &reach, p, &sa, &sa)?;
        let auxiliary = backward(tree, profile, model, &reach, p, &sa, &ba)?;
        let a = backward(tree, profile, model, &reach, p, &sigma[0], &sigma[0])?;
        let b = backward(tree, profile, model, &reach, p, &sigma[1], &sigma[1])?;
        let aa = backward(tree, profile, model, &reach, p, &sigma[0], &beta[0])?;
        let ab = backward(tree, profile, model, &reach, p, &sigma[1], &beta[1])?;
        let held_a_to_b = fixed_response(tree, model, p, &sigma[1], &a.actions) - b.value;
        let held_b_to_a = fixed_response(tree, model, p, &sigma[0], &b.actions) - a.value;
        let auxiliary_held_a_to_b = fixed_response(tree, model, p, &beta[1], &aa.actions) - b.value;
        let auxiliary_held_b_to_a = fixed_response(tree, model, p, &beta[0], &ab.actions) - a.value;
        let mut l1_reach_share = 0.0;
        for (z, n) in tree.nodes.iter().enumerate() {
            if n.terminal.as_ref().is_some_and(|t| t.l1.is_some()) {
                for &c in &model.support[p] {
                    l1_reach_share += Classes::get().n(c) as f64 * model.weights[p][c]
                        / model.normalizers[p]
                        * reach[z].pi(p, c)
                        * (0..tree.seats)
                            .filter(|&s| s != p)
                            .map(|s| reach[z].mass(s, c))
                            .product::<f64>();
                }
            }
        }
        seats.push(SeatEvaluation {
            seat: p,
            value: primary.value,
            value_a: a.value,
            value_b: b.value,
            best_response: primary.best,
            gain: primary.best - primary.value,
            held_a_to_b,
            held_b_to_a,
            held_gain: (held_a_to_b + held_b_to_a) * 0.5,
            auxiliary_best_response: auxiliary.best,
            auxiliary_gain: auxiliary.best - primary.value,
            auxiliary_held_a_to_b,
            auxiliary_held_b_to_a,
            auxiliary_held_gain: (auxiliary_held_a_to_b + auxiliary_held_b_to_a) * 0.5,
            l1_reach_share,
            telescoping_residual: primary.residual,
            auxiliary_telescoping_residual: auxiliary.residual,
        });
    }
    Ok(Evaluation {
        nash_conv: seats.iter().map(|s| s.gain).sum(),
        held_nash_conv: seats.iter().map(|s| s.held_gain).sum(),
        auxiliary_nash_conv: seats.iter().map(|s| s.auxiliary_gain).sum(),
        auxiliary_held_nash_conv: seats.iter().map(|s| s.auxiliary_held_gain).sum(),
        seats,
        seconds: start.elapsed().as_secs_f64(),
    })
}
