use anyhow::{Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::time::Instant;

use super::{Profile, Tree};
use crate::trunk::{
    classes::Classes,
    tables::{HuShowdownTable, ThreeWayTable},
};
use crate::{ExternalSamplingGame, FeatureHashAbstraction, HoldemGame};

/// Lookups occur only while preparing class slabs, never in contraction loops.
/// T2 is unnormalized (sums to K); T3 is hero/first/second oriented, including
/// the opponent swap convention of `ThreeWayTable::p3`.
pub trait Tables: Sync {
    fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]>;
    fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f64; 13]>;
}

impl Tables for (&HuShowdownTable, &ThreeWayTable) {
    fn t2(&self, c: usize, d: usize) -> Result<[f64; 3]> {
        self.0.t2(c, d)
    }
    fn p3(&self, c: usize, d: usize, e: usize) -> Result<[f64; 13]> {
        Ok(self.1.p3(c, d, e)?.map(f64::from))
    }
}

#[derive(Clone, Copy)]
pub struct EvaluationOptions {
    pub k4_samples: u64,
    pub seed: u64,
    pub top_infosets: usize,
    #[cfg(test)]
    pub(crate) sample_three_way: bool,
}

impl Default for EvaluationOptions {
    fn default() -> Self {
        Self {
            k4_samples: 2048,
            seed: 0,
            top_infosets: 20,
            #[cfg(test)]
            sample_three_way: false,
        }
    }
}

pub struct Model<'a> {
    game: &'a HoldemGame<FeatureHashAbstraction>,
    pub(crate) tables: &'a dyn Tables,
    pub(crate) options: EvaluationOptions,
    pub(crate) weights: Vec<[f64; 169]>,
    pub warnings: Vec<String>,
    pub(crate) support: Vec<Vec<usize>>,
    pub(crate) live: Vec<Vec<Vec<usize>>>,
    pub(crate) normalizers: Vec<f64>,
}

impl<'a> Model<'a> {
    pub fn evaluation_options(&self) -> EvaluationOptions {
        self.options
    }

    pub fn weights(&self) -> &[[f64; 169]] {
        &self.weights
    }

    pub fn game(&self) -> &HoldemGame<FeatureHashAbstraction> {
        self.game
    }

    pub fn tables(&self) -> &dyn Tables {
        self.tables
    }

    pub fn new(
        game: &'a HoldemGame<FeatureHashAbstraction>,
        tables: &'a dyn Tables,
        options: EvaluationOptions,
    ) -> Result<Self> {
        game.require_l0_chip_ev()?;
        ensure!(options.k4_samples > 0, "k4 sample count must be positive");
        let catalog = Classes::get();
        let mut warnings = Vec::new();
        let weights: Vec<_> = game
            .config()
            .seats
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = catalog.weights(&s.range);
                warnings.extend(w.warnings.iter().map(|w| {
                    format!(
                        "seat {i}: suit-asymmetric {} ({}..{}), using class mean",
                        w.name, w.min, w.max
                    )
                }));
                w.weights
            })
            .collect();
        let support: Vec<Vec<usize>> = weights
            .iter()
            .map(|w| (0..169).filter(|&c| w[c] > 0.0).collect())
            .collect();
        let normalizers = deal_normalizers(&weights, &support)?;
        let live = (0..169)
            .map(|c| {
                let dead = catalog.combo_mask(catalog.representative(c));
                (0..169)
                    .map(|d| {
                        catalog
                            .combos(d)
                            .iter()
                            .copied()
                            .filter(|&v| catalog.combo_mask(v) & dead == 0)
                            .collect()
                    })
                    .collect()
            })
            .collect();
        Ok(Self {
            game,
            tables,
            options,
            weights,
            warnings,
            support,
            live,
            normalizers,
        })
    }

    pub(crate) fn sample_three_way(&self) -> bool {
        #[cfg(test)]
        {
            self.options.sample_three_way
        }
        #[cfg(not(test))]
        {
            false
        }
    }
}

pub(crate) fn deal_normalizers(weights: &[[f64; 169]], support: &[Vec<usize>]) -> Result<Vec<f64>> {
    let catalog = Classes::get();
    let z: Vec<[f64; 169]> = weights
        .iter()
        .zip(support)
        .map(|(w, s)| {
            std::array::from_fn(|c| s.iter().map(|&d| f64::from(catalog.k(c, d)) * w[d]).sum())
        })
        .collect();
    let normalizers: Vec<_> = weights
        .iter()
        .enumerate()
        .map(|(p, w)| {
            support[p]
                .iter()
                .map(|&c| {
                    catalog.n(c) as f64
                        * w[c]
                        * (0..weights.len())
                            .filter(|&j| j != p)
                            .map(|j| z[j][c])
                            .product::<f64>()
                })
                .sum::<f64>()
        })
        .collect();
    ensure!(
        normalizers.iter().all(|&t| t.is_finite() && t > 0.0),
        "ranges have T_p = 0 (no hero-conditioned deal)"
    );
    Ok(normalizers)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LocalGain {
    pub path: Vec<String>,
    pub class: String,
    pub local_gain: f64,
    pub profile_row: Vec<f64>,
    pub best_action: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeatEvaluation {
    pub seat: usize,
    pub name: String,
    pub value: f64,
    pub best_response: f64,
    pub gain: f64,
    /// Sum over defaulted (node,class) pairs. Nested defaulted decisions can
    /// count the same trajectory repeatedly, so this mass can exceed one.
    pub defaulted_mass: f64,
    /// Slot k contains reach with k active seats; slot zero is unused.
    pub reach_by_active_count: Vec<f64>,
    /// Contribution to value from terminals with k active seats.
    pub value_by_active_count: Vec<f64>,
    pub telescoping_residual: f64,
    pub top_infosets: Vec<LocalGain>,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub seats: Vec<SeatEvaluation>,
    /// Pure class best responses, indexed by node * 169 + class for each seat.
    #[serde(skip)]
    pub best_response_actions: Vec<Vec<u8>>,
    pub nash_conv: f64,
    pub warnings: Vec<String>,
    pub timings: PhaseTimings,
}

#[derive(Debug, Serialize)]
pub struct PhaseTimings {
    pub reaches: f64,
    pub t2: f64,
    pub t3: f64,
    pub k4: f64,
    pub backward: f64,
}

pub(crate) struct Reach {
    pi: Vec<f64>,
    mass: Vec<f64>,
}

impl Reach {
    pub(crate) fn pi(&self, seat: usize, c: usize) -> f64 {
        self.pi[seat * 169 + c]
    }
    pub(crate) fn mass(&self, seat: usize, c: usize) -> f64 {
        self.mass[seat * 169 + c]
    }
    pub(crate) fn rho(&self, model: &Model<'_>, seat: usize) -> Vec<(usize, f64)> {
        model.support[seat]
            .iter()
            .filter_map(|&d| {
                let r = model.weights[seat][d] * self.pi(seat, d);
                (r > 0.0).then_some((d, r))
            })
            .collect()
    }
}

pub(crate) fn reaches(tree: &Tree, profile: &Profile, model: &Model<'_>) -> Vec<Reach> {
    let mut reaches: Vec<Reach> = Vec::with_capacity(tree.nodes.len());
    for n in &tree.nodes {
        let mut pi = if let Some((p, _)) = n.parent {
            reaches[p].pi.clone()
        } else {
            vec![1.0; tree.seats * 169]
        };
        if let Some((p, a)) = n.parent {
            let actor = tree.nodes[p].actor.unwrap();
            for &d in &model.support[actor] {
                pi[actor * 169 + d] *= profile.row(tree, p, d)[a];
            }
        }
        reaches.push(Reach {
            pi,
            mass: vec![0.0; tree.seats * 169],
        });
    }
    let classes: Vec<_> = (0..169)
        .filter(|&c| model.weights.iter().any(|w| w[c] > 0.0))
        .collect();
    reaches.par_iter_mut().for_each(|reach| {
        for seat in 0..tree.seats {
            let rho = reach.rho(model, seat);
            for &c in &classes {
                reach.mass[seat * 169 + c] = rho
                    .iter()
                    .map(|&(d, r)| f64::from(Classes::get().k(c, d)) * r)
                    .sum();
            }
        }
    });
    reaches
}

/// Solver reads pi at its own decisions (averaging) and rho at terminals.
/// Mass is read only by leaf_values at terminals; decision masses stay unused.
pub(crate) fn solver_reaches(tree: &Tree, profile: &Profile, model: &Model<'_>) -> Vec<Reach> {
    let mut reach: Vec<_> = tree
        .nodes
        .iter()
        .map(|_| Reach {
            pi: vec![1.0; tree.seats * 169],
            mass: vec![0.0; tree.seats * 169],
        })
        .collect();
    for seat in 0..tree.seats {
        update_reach(tree, profile, model, &mut reach, seat);
    }
    reach
}

/// Rebuild one seat along the path, retaining the full evaluator's exact
/// multiplication order and rho sum order, including unsupported pi entries.
pub(crate) fn update_reach(
    tree: &Tree,
    profile: &Profile,
    model: &Model<'_>,
    reach: &mut [Reach],
    seat: usize,
) {
    let offset = seat * 169;
    for (z, node) in tree.nodes.iter().enumerate() {
        if let Some((parent, action)) = node.parent {
            let (before, after) = reach.split_at_mut(z);
            after[0].pi[offset..offset + 169]
                .copy_from_slice(&before[parent].pi[offset..offset + 169]);
            if tree.nodes[parent].actor == Some(seat) {
                for &d in &model.support[seat] {
                    after[0].pi[offset + d] *= profile.row(tree, parent, d)[action];
                }
            }
        } else {
            reach[z].pi[offset..offset + 169].fill(1.0);
        }
    }
    let classes: Vec<_> = (0..169)
        .filter(|&c| model.weights.iter().any(|w| w[c] > 0.0))
        .collect();
    reach.par_iter_mut().zip(&tree.nodes).for_each(|(r, node)| {
        if node.terminal.is_some() {
            let rho = r.rho(model, seat);
            for &c in &classes {
                r.mass[offset + c] = rho
                    .iter()
                    .map(|&(d, v)| f64::from(Classes::get().k(c, d)) * v)
                    .sum();
            }
        }
    });
}

pub fn evaluate(tree: &Tree, profile: &Profile, model: &Model<'_>) -> Result<Evaluation> {
    ensure!(
        tree.l1_leaf_count() == 0,
        "L1 leaves require the L1 evaluator"
    );
    profile.validate(tree)?;
    ensure!(
        tree.game_fingerprint == model.game.game_fingerprint(),
        "model/tree mismatch"
    );
    let start = Instant::now();
    let reach = reaches(tree, profile, model);
    let reach_seconds = start.elapsed().as_secs_f64();
    let n = tree.nodes.len();
    ensure!(
        tree.nodes.iter().all(|node| node.children.len() <= 254),
        "L0 nodes may have at most 254 actions"
    );
    let leaves = super::leaves::leaf_values(
        tree,
        model,
        &reach,
        &(0..tree.seats).collect::<Vec<_>>(),
        super::leaves::K4Plan::model(model),
    )?;
    let t2_seconds = leaves.t2;
    let t3_seconds = leaves.t3;
    let k4_seconds = leaves.k4;
    let leaves = leaves.values;
    let start = Instant::now();
    let mut seats = Vec::new();
    let mut best_response_actions = Vec::new();
    for (p, mut values) in leaves.into_iter().enumerate() {
        let mut actions = vec![u8::MAX; n * 169];
        for (z, node) in tree.nodes.iter().enumerate() {
            if node.actor == Some(p) {
                actions[z * 169..(z + 1) * 169].fill(0);
            }
        }
        let mut best = values.clone();
        let mut locals = Vec::new();
        let mut defaulted_mass = 0.0;
        let mut reach_by_active_count = vec![0.0; tree.seats + 1];
        let mut value_by_active_count = vec![0.0; tree.seats + 1];
        let mut local_total = 0.0;
        for z in (0..n).rev() {
            let node = &tree.nodes[z];
            for &c in &model.support[p] {
                let weight =
                    Classes::get().n(c) as f64 * model.weights[p][c] / model.normalizers[p];
                let prob = weight
                    * reach[z].pi(p, c)
                    * (0..tree.seats)
                        .filter(|&j| j != p)
                        .map(|j| reach[z].mass(j, c))
                        .product::<f64>();
                if let Some(t) = &node.terminal {
                    reach_by_active_count[t.active.len()] += prob;
                    value_by_active_count[t.active.len()] +=
                        weight * reach[z].pi(p, c) * values[z][c];
                    continue;
                }
                if node.actor == Some(p) {
                    if profile.is_defaulted(z, c) {
                        defaulted_mass += prob;
                    }
                    let row = profile.row(tree, z, c);
                    let mut maximum = f64::NEG_INFINITY;
                    let mut best_action = 0;
                    let mut mixed = 0.0;
                    let mut value = 0.0;
                    for (a, &child) in node.children.iter().enumerate() {
                        value += row[a] * values[child][c];
                        mixed += row[a] * best[child][c];
                        if best[child][c] > maximum {
                            maximum = best[child][c];
                            best_action = a;
                        }
                    }
                    values[z][c] = value;
                    best[z][c] = maximum;
                    actions[z * 169 + c] = best_action as u8;
                    let local = weight * reach[z].pi(p, c) * (maximum - mixed);
                    local_total += local;
                    if local > 0.0 {
                        locals.push((local, z, c, best_action));
                    }
                } else {
                    values[z][c] = node.children.iter().map(|&child| values[child][c]).sum();
                    best[z][c] = node.children.iter().map(|&child| best[child][c]).sum();
                }
            }
        }
        let aggregate = |v: &[f64; 169]| {
            model.support[p]
                .iter()
                .map(|&c| Classes::get().n(c) as f64 * model.weights[p][c] * v[c])
                .sum::<f64>()
                / model.normalizers[p]
        };
        let value = aggregate(&values[0]);
        ensure!(
            (value_by_active_count.iter().sum::<f64>() - value).abs() <= 1e-9 * (1.0 + value.abs()),
            "terminal values do not sum to value for seat {p}"
        );
        let best_response = aggregate(&best[0]);
        let gain = best_response - value;
        let residual = local_total - gain;
        ensure!(
            gain >= -1e-9 * (1.0 + value.abs()),
            "BR below profile value for seat {p}"
        );
        ensure!(
            residual.abs() <= 1e-9 * (1.0 + gain.abs()),
            "local gains do not telescope for seat {p}: {residual}"
        );
        let reach_sum: f64 = reach_by_active_count.iter().sum();
        ensure!(
            (reach_sum - 1.0).abs() <= 1e-9,
            "terminal reach does not sum to one for seat {p}: {reach_sum}"
        );
        locals.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let top_infosets = locals
            .into_iter()
            .take(model.options.top_infosets)
            .map(|(local_gain, z, c, a)| LocalGain {
                path: tree.path(z),
                class: Classes::get().name(c),
                local_gain,
                profile_row: profile.row(tree, z, c).to_vec(),
                best_action: tree.nodes[z].labels[a].clone(),
            })
            .collect();
        seats.push(SeatEvaluation {
            seat: p,
            name: model.game.config().seats[crate::SeatId(p as u8)]
                .name
                .clone()
                .unwrap_or_else(|| format!("seat {p}")),
            value,
            best_response,
            gain: gain.max(0.0),
            defaulted_mass,
            reach_by_active_count,
            value_by_active_count,
            telescoping_residual: residual,
            top_infosets,
        });
        best_response_actions.push(actions);
    }
    Ok(Evaluation {
        nash_conv: seats.iter().map(|s| s.gain).sum(),
        seats,
        best_response_actions,
        warnings: model.warnings.clone(),
        timings: PhaseTimings {
            reaches: reach_seconds,
            t2: t2_seconds,
            t3: t3_seconds,
            k4: k4_seconds,
            backward: start.elapsed().as_secs_f64(),
        },
    })
}
