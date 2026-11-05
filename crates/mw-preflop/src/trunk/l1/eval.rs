use super::{
    Board, Strategies,
    pass::{
        CHUNK, Pass, class_values, informed_best_response, inputs, relative_leaf_buckets, scratch,
        terminal_values,
    },
};
use crate::ExternalSamplingGame;
use crate::trunk::{
    classes::{Classes, class},
    l0::{
        Model, Profile, Tree,
        eval::{Reach, reaches},
        leaves::{K4Plan, leaf_values},
    },
};
use anyhow::{Result, ensure};
use nlh::{NUM_COMBOS, combo_cards};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Class values (BB per combo, the `leaf_diagnostic` value/mass basis)
/// that replace one seat's L1 values at one leaf in `evaluate_with_overrides`.
#[derive(Debug, Clone)]
pub struct LeafOverride {
    pub terminal: usize,
    pub seat: usize,
    pub values: [Option<f64>; 169],
}

pub fn evaluate(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    strategies: &Strategies,
    boards: &[Board],
    control: bool,
    regression: bool,
) -> Result<Evaluation> {
    evaluate_with_overrides(
        tree,
        profile,
        model,
        strategies,
        boards,
        control,
        regression,
        &[],
    )
}

type OverrideMap<'a> = std::collections::BTreeMap<(usize, usize), &'a [Option<f64>; 169]>;

pub(super) fn override_map<'a>(
    tree: &Tree,
    overrides: &'a [LeafOverride],
) -> Result<OverrideMap<'a>> {
    let mut replacements = std::collections::BTreeMap::new();
    for entry in overrides {
        let subtree = tree
            .nodes
            .get(entry.terminal)
            .and_then(|n| n.terminal.as_ref())
            .and_then(|t| t.l1.as_ref());
        ensure!(
            subtree.is_some(),
            "node {} is not an L1 leaf",
            entry.terminal
        );
        ensure!(
            subtree.unwrap().active.contains(&entry.seat),
            "seat {} is not at leaf {}",
            entry.seat,
            entry.terminal
        );
        ensure!(
            replacements
                .insert((entry.terminal, entry.seat), &entry.values)
                .is_none(),
            "duplicate override for leaf {} seat {}",
            entry.terminal,
            entry.seat
        );
    }
    Ok(replacements)
}

pub(super) fn class_mass(
    board: &Board,
    opponent: &[f64],
    ones: &[Vec<f64>],
    seat: usize,
    scale: &[f64; NUM_COMBOS],
    mass: &mut [f64],
) -> [f64; 169] {
    terminal_values(board, opponent, ones, seat, 0, mass);
    class_values(mass, scale)
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_with_overrides(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    strategies: &Strategies,
    boards: &[Board],
    control: bool,
    regression: bool,
    overrides: &[LeafOverride],
) -> Result<Evaluation> {
    let replacements = override_map(tree, overrides)?;
    let ones = (!overrides.is_empty()).then(|| vec![vec![1.0; tree.seats]]);
    ensure!(
        !regression || control,
        "regression needs the control variate"
    );
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
            .map(|storage| {
                let z = storage.terminal;
                let terminal = tree.nodes[z].terminal.as_ref().unwrap();
                let subtree = terminal.l1.as_ref().unwrap();
                if !subtree.active.contains(&p) {
                    return None;
                }
                let rows = storage.profile(subtree, true);
                let (opponent, own, scale) = inputs(tree, model, &reach[z], z, p);
                let overridden = replacements.contains_key(&(z, p));
                let mut masses = [[0.0; 169]; 2];
                // Per half; on the heap like inputs(). moments holds the sums of
                // checkdown, its square, and its products with the primary and
                // auxiliary differences.
                let mut sigma = vec![[0.0; 169]; 2];
                let mut beta = vec![[0.0; 169]; 2];
                let mut moments = vec![[[0.0; 169]; 2]; 4];
                if opponent.iter().any(|&r| r != 0.0) {
                    let chunks: Vec<_> = boards
                        .par_chunks(CHUNK)
                        .enumerate()
                        .map_init(scratch, |scratch, (i, chunk)| {
                            let mut sigma = vec![[0.0; 169]; 2];
                            let mut beta = vec![[0.0; 169]; 2];
                            let mut moments = vec![[[0.0; 169]; 2]; 4];
                            let mut masses = [[0.0; 169]; 2];
                            let mut mass = overridden.then(|| vec![0.0; NUM_COMBOS]);
                            for (k, board) in chunk.iter().enumerate() {
                                let j = i * CHUNK + k;
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
                                        mapping: None,
                                        own_reach_regret: false,
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
                                let half = (boards.len() / 2) as f64;
                                if let Some(mass) = &mut mass {
                                    let m = class_mass(
                                        board,
                                        opponent.as_slice(),
                                        ones.as_ref().unwrap(),
                                        p,
                                        &scale,
                                        mass,
                                    );
                                    for c in 0..169 {
                                        masses[j % 2][c] += m[c] / half;
                                    }
                                }
                                for c in 0..169 {
                                    sigma[j % 2][c] += (s[c] - checkdown[c]) / half;
                                    beta[j % 2][c] += (b[c] - checkdown[c]) / half;
                                }
                                if regression {
                                    let m = &mut moments;
                                    for c in 0..169 {
                                        let x = checkdown[c];
                                        m[0][j % 2][c] += x;
                                        m[1][j % 2][c] += x * x;
                                        m[2][j % 2][c] += (s[c] - x) * x;
                                        m[3][j % 2][c] += (b[c] - x) * x;
                                    }
                                }
                            }
                            (sigma, beta, moments, masses)
                        })
                        .collect();
                    let mut chunks = chunks.into_iter();
                    (sigma, beta, moments, masses) = chunks.next().unwrap();
                    for (s, b, m, mass) in chunks {
                        for h in 0..2 {
                            for c in 0..169 {
                                sigma[h][c] += s[h][c];
                                beta[h][c] += b[h][c];
                                masses[h][c] += mass[h][c];
                                for k in 0..4 {
                                    moments[k][h][c] += m[k][h][c];
                                }
                            }
                        }
                    }
                }
                Some((z, sigma, beta, moments, masses))
            })
            .collect();
        let n = (boards.len() / 2) as f64;
        let mut override_masses = Vec::new();
        for (z, s, b, m, masses) in output.into_iter().flatten() {
            if let Some(values) = replacements.get(&(z, p)) {
                override_masses.push((z, values, masses));
            }
            for h in 0..2 {
                if control {
                    // leaf_values left the exact checkdown value (T2) here.
                    for c in 0..169 {
                        let t2 = sigma[h][z][c];
                        sigma[h][z][c] += s[h][c];
                        beta[h][z][c] += b[h][c];
                        // Regression: mean(L1) - beta (mean(checkdown) - T2) with
                        // beta = Cov(L1, checkdown) / Var(checkdown) from this half.
                        let mean = m[0][h][c] / n;
                        let variance = m[1][h][c] / n - mean * mean;
                        if regression && variance > 1e-12 * (m[1][h][c] / n) {
                            let primary = (m[2][h][c] / n - s[h][c] * mean) / variance;
                            let auxiliary = (m[3][h][c] / n - b[h][c] * mean) / variance;
                            sigma[h][z][c] -= primary * (mean - t2);
                            beta[h][z][c] -= auxiliary * (mean - t2);
                        }
                    }
                } else {
                    sigma[h][z] = s[h];
                    beta[h][z] = b[h];
                }
            }
        }
        for (z, values, masses) in override_masses {
            for (c, value) in values.iter().enumerate() {
                if let Some(value) = value {
                    for h in 0..2 {
                        sigma[h][z][c] = value * masses[h][c];
                        beta[h][z][c] = sigma[h][z][c];
                    }
                }
            }
        }
        let all = |halves: [&Slab; 2]| {
            halves[0]
                .iter()
                .zip(halves[1])
                .map(|(a, b)| std::array::from_fn(|c| (a[c] + b[c]) * 0.5))
                .collect::<Slab>()
        };
        let sa = all([&sigma[0], &sigma[1]]);
        let ba = all([&beta[0], &beta[1]]);
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

/// One L1 leaf's per-class values for one hero, summed over `boards`: the
/// saved Postflop average, the hero's real-card best response to it, the
/// checkdown line, and the opponent mass they are relative to. Every entry
/// carries the same folded-seat scale, so `value / mass` is the hero's
/// expected payoff per combo at this leaf.
#[derive(Debug, Clone, Serialize)]
pub struct LeafDiagnostic {
    pub profile: Vec<f64>,
    pub best: Vec<f64>,
    pub checkdown: Vec<f64>,
    pub mass: Vec<f64>,
    /// Opponent matchup mass with the folded-seat scale removed. Multiplying
    /// by the hero's reach and nominal combo count gives the conditional
    /// range-vs-range weighting used by real-card P1 root EVs.
    pub matchup_mass: Vec<f64>,
}

pub fn leaf_diagnostic(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    strategies: &Strategies,
    boards: &[Board],
    terminal: usize,
    hero: usize,
) -> Result<LeafDiagnostic> {
    profile.validate(tree)?;
    let storage = strategies
        .leaves
        .iter()
        .find(|s| s.terminal == terminal)
        .ok_or_else(|| anyhow::anyhow!("node {terminal} is not an L1 leaf"))?;
    let leaf = tree.nodes[terminal].terminal.as_ref().unwrap();
    let subtree = leaf.l1.as_ref().unwrap();
    ensure!(subtree.active.contains(&hero), "seat {hero} is not at leaf");
    let reach = reaches(tree, profile, model);
    let rows = storage.profile(subtree, true);
    let (opponent, own, scale) = inputs(tree, model, &reach[terminal], terminal, hero);
    let ones = vec![vec![1.0; tree.seats]];
    let sums = boards
        .par_chunks(CHUNK)
        .map_init(scratch, |scratch, chunk| {
            let mut sums = [[0.0; 169]; 4];
            let mut mass = vec![0.0; NUM_COMBOS];
            for board in chunk {
                scratch.pass(
                    &Pass {
                        tree: subtree,
                        storage,
                        rows: &rows,
                        board,
                        hero,
                        opponent: &opponent,
                        own: &own,
                        scale: &scale,
                        auxiliary: true,
                        mapping: None,
                        own_reach_regret: false,
                    },
                    None,
                );
                let s = class_values(&scratch.values, &scale);
                let b = class_values(&scratch.best, &scale);
                let x = scratch.checkdown(board, leaf, hero, &opponent, &scale);
                terminal_values(board, opponent.as_slice(), &ones, hero, 0, &mut mass);
                let m = class_values(&mass, &scale);
                for c in 0..169 {
                    sums[0][c] += s[c];
                    sums[1][c] += b[c];
                    sums[2][c] += x[c];
                    sums[3][c] += m[c];
                }
            }
            sums
        })
        .reduce(
            || [[0.0; 169]; 4],
            |mut a, b| {
                for (a, b) in a.iter_mut().zip(&b) {
                    for (a, b) in a.iter_mut().zip(b) {
                        *a += b;
                    }
                }
                a
            },
        );
    Ok(LeafDiagnostic {
        profile: sums[0].to_vec(),
        best: sums[1].to_vec(),
        checkdown: sums[2].to_vec(),
        mass: sums[3].to_vec(),
        matchup_mass: (0..169)
            .map(|c| {
                let folded = scale[Classes::get().combos(c)[0]];
                if folded > 0.0 {
                    sums[3][c] / folded
                } else {
                    0.0
                }
            })
            .collect(),
    })
}

/// The hero's per-class value at one L1 leaf when every combo best-responds
/// to the saved opponent strategy with only the cards it has seen (flop
/// decisions see the flop, turn decisions the turn), summed over one flop's
/// `boards` on the scale of [`LeafDiagnostic::profile`]. Comparing it with the
/// saved strategy's value isolates what sharing a bucket's strategy costs a
/// class; [`LeafDiagnostic::best`] instead sees the whole runout.
pub fn leaf_informed_best_response(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    strategies: &Strategies,
    boards: &[Board],
    terminal: usize,
    hero: usize,
) -> Result<Vec<f64>> {
    profile.validate(tree)?;
    ensure!(
        boards.iter().all(|b| b.cards[..3] == boards[0].cards[..3]),
        "the boards do not share one flop"
    );
    ensure!(
        boards.iter().all(|b| b
            .relative
            .as_ref()
            .is_none_or(|s| s.iter().all(|s| s.opponent.is_none()))),
        "the informed best response does not support opponent levels"
    );
    let storage = strategies
        .leaves
        .iter()
        .find(|s| s.terminal == terminal)
        .ok_or_else(|| anyhow::anyhow!("node {terminal} is not an L1 leaf"))?;
    let subtree = tree.nodes[terminal]
        .terminal
        .as_ref()
        .unwrap()
        .l1
        .as_ref()
        .unwrap();
    ensure!(subtree.active.contains(&hero), "seat {hero} is not at leaf");
    let reach = reaches(tree, profile, model);
    let rows = storage.profile(subtree, true);
    let (opponent, _, scale) = inputs(tree, model, &reach[terminal], terminal, hero);
    let values = informed_best_response(subtree, storage, &rows, boards, hero, &opponent);
    Ok(class_values(&values, &scale).to_vec())
}

/// The hero's saved average Postflop strategy at one decision node of an L1
/// leaf, per class, over `boards`.
#[derive(Debug, Clone, Serialize)]
pub struct LeafStrategyNode {
    pub node: usize,
    /// Action labels from the leaf root.
    pub path: Vec<String>,
    pub labels: Vec<String>,
    /// Hero reach times card-compatible opponent reach, summed over the class's
    /// live combos and the boards. `weight / root` is the share of the class
    /// that reaches this node.
    pub weight: Vec<f64>,
    /// `[class][action]`, each row summing to 1 where `weight` is positive.
    pub frequencies: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeafStrategyDump {
    /// The class's weight at the leaf root, the denominator of reach shares.
    pub root: Vec<f64>,
    pub nodes: Vec<LeafStrategyNode>,
}

/// Every hero decision of one L1 leaf with the bucket mapping the solver uses
/// on each board (relative buckets from the actor's current reach), so class
/// frequencies are comparable with a real-card solve of the same spot.
pub fn leaf_strategy(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    strategies: &Strategies,
    boards: &[Board],
    terminal: usize,
    hero: usize,
) -> Result<LeafStrategyDump> {
    profile.validate(tree)?;
    let storage = strategies
        .leaves
        .iter()
        .find(|s| s.terminal == terminal)
        .ok_or_else(|| anyhow::anyhow!("node {terminal} is not an L1 leaf"))?;
    let leaf = tree.nodes[terminal].terminal.as_ref().unwrap();
    let subtree = leaf.l1.as_ref().unwrap();
    ensure!(subtree.active.contains(&hero), "seat {hero} is not at leaf");
    let local = usize::from(subtree.active[0] != hero);
    let reach = reaches(tree, profile, model);
    let rows = storage.profile(subtree, true);
    let (opponent, own, _) = inputs(tree, model, &reach[terminal], terminal, hero);
    let nodes = &subtree.nodes;
    let width = |z: usize| 169 * nodes[z].children.len();
    // [root weight, then per node weight [169] followed by frequencies [169 * k]].
    let layout: Vec<usize> = std::iter::once(0)
        .chain(nodes.iter().enumerate().scan(169, |at, (z, n)| {
            let start = *at;
            if n.actor == Some(local) {
                *at += 169 + width(z);
            }
            Some(start)
        }))
        .collect();
    let total = layout[nodes.len()]
        + nodes
            .last()
            .filter(|n| n.actor == Some(local))
            .map_or(0, |_| 169 + width(nodes.len() - 1));
    let sums = boards
        .par_iter()
        .fold(
            || vec![0.0; total],
            |mut sums, board| {
                let mut own_slab = vec![0.0; nodes.len() * NUM_COMBOS];
                let mut opponent_slab = vec![0.0; nodes.len() * NUM_COMBOS];
                for &h in &board.sorted {
                    own_slab[h] = own[h];
                    opponent_slab[h] = opponent[h];
                }
                let mut mapped = vec![0_u16; NUM_COMBOS];
                let mut stack = vec![0_usize];
                while let Some(z) = stack.pop() {
                    let n = &nodes[z];
                    let Some(actor) = n.actor else { continue };
                    let mine = actor == local;
                    let at = z * NUM_COMBOS;
                    let street = n.street.index() - 1;
                    let map: &[u16] = if let Some(strength) = &board.relative {
                        let (slab, other) = if mine {
                            (&own_slab, &opponent_slab)
                        } else {
                            (&opponent_slab, &own_slab)
                        };
                        // The actor's and the opponent's reach at the node.
                        relative_leaf_buckets(
                            &strength[street],
                            subtree.flop_spr,
                            storage.variant,
                            &slab[at..at + NUM_COMBOS],
                            Some(&other[at..at + NUM_COMBOS]),
                            &mut mapped,
                        );
                        &mapped
                    } else {
                        &board.buckets[street]
                    };
                    let k = n.children.len();
                    let offset = storage.offsets[z];
                    if mine {
                        // Opponent combos sharing no card with h, by inclusion-exclusion.
                        let mut cards = [0.0; 52];
                        let mut all = 0.0;
                        for &h in &board.sorted {
                            let w = opponent_slab[at + h];
                            let (a, b) = combo_cards(h);
                            cards[a.index()] += w;
                            cards[b.index()] += w;
                            all += w;
                        }
                        let base = layout[z + 1];
                        for &h in &board.sorted {
                            let (a, b) = combo_cards(h);
                            let compatible =
                                all - cards[a.index()] - cards[b.index()] + opponent_slab[at + h];
                            let w = own_slab[at + h] * compatible;
                            let c = class(h);
                            if z == 0 {
                                sums[c] += w;
                            }
                            sums[base + c] += w;
                            let row = offset + map[h] as usize * k;
                            for a in 0..k {
                                sums[base + 169 + c * k + a] += w * rows[row + a];
                            }
                        }
                    } else if z == 0 {
                        // The hero's root weight before the opponent acts first.
                        let mut cards = [0.0; 52];
                        let mut all = 0.0;
                        for &h in &board.sorted {
                            let w = opponent_slab[h];
                            let (a, b) = combo_cards(h);
                            cards[a.index()] += w;
                            cards[b.index()] += w;
                            all += w;
                        }
                        for &h in &board.sorted {
                            let (a, b) = combo_cards(h);
                            sums[class(h)] += own_slab[h]
                                * (all - cards[a.index()] - cards[b.index()] + opponent_slab[h]);
                        }
                    }
                    for (a, &child) in n.children.iter().enumerate() {
                        let to = child * NUM_COMBOS;
                        for &h in &board.sorted {
                            let p = rows[offset + map[h] as usize * k + a];
                            let (o, m) = (opponent_slab[at + h], own_slab[at + h]);
                            opponent_slab[to + h] = if mine { o } else { o * p };
                            own_slab[to + h] = if mine { m * p } else { m };
                        }
                        stack.push(child);
                    }
                }
                sums
            },
        )
        .reduce(
            || vec![0.0; total],
            |mut a, b| {
                for (a, b) in a.iter_mut().zip(&b) {
                    *a += b;
                }
                a
            },
        );
    let path = |mut z: usize| {
        let mut labels = Vec::new();
        while let Some((parent, a)) = nodes[z].parent {
            labels.push(nodes[parent].labels[a].clone());
            z = parent;
        }
        labels.reverse();
        labels
    };
    Ok(LeafStrategyDump {
        root: sums[..169].to_vec(),
        nodes: nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.actor == Some(local))
            .map(|(z, n)| {
                let k = n.children.len();
                let base = layout[z + 1];
                let weight = sums[base..base + 169].to_vec();
                LeafStrategyNode {
                    node: z,
                    path: path(z),
                    labels: n.labels.clone(),
                    frequencies: (0..169)
                        .map(|c| {
                            let row = &sums[base + 169 + c * k..base + 169 + (c + 1) * k];
                            if weight[c] > 0.0 {
                                row.iter().map(|x| x / weight[c]).collect()
                            } else {
                                vec![0.0; k]
                            }
                        })
                        .collect(),
                    weight,
                }
            })
            .collect(),
    })
}
