//! Experimental, mass-preserving logit prior for the L1 leaf game.
use crate::trunk::{
    classes::{Classes, class},
    l0::{Model, Profile, Tree, eval::Reach, solve::backward_values},
};
use anyhow::{Result, ensure};
use nlh::NUM_COMBOS;
use serde::Serialize;

/// Inverse temperature in bb^-1. The prior mass comes from Relative::prior.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Qre {
    pub lambda: f64,
}

impl Qre {
    pub fn parse(text: &str) -> Result<Self> {
        let result = Self {
            lambda: text.parse()?,
        };
        result.validate()?;
        Ok(result)
    }

    pub(super) fn validate(self) -> Result<()> {
        ensure!(
            self.lambda.is_finite() && self.lambda >= 0.0,
            "QRE lambda must be finite and nonnegative"
        );
        Ok(())
    }
}

/// Reuse current trunk action values; no extra terminal contractions or boards.
#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    tree: &Tree,
    model: &Model<'_>,
    profile: &Profile,
    reach: &[Reach],
    seat: usize,
    values: &[[f64; 169]],
    qre: Qre,
    logit: &mut Profile,
    mass: &mut Vec<[f64; 169]>,
) {
    mass.resize(tree.nodes.len(), [0.0; 169]);
    for (z, node) in tree.nodes.iter().enumerate() {
        if node.terminal.is_some() {
            for &c in &model.support[seat] {
                mass[z][c] = (0..tree.seats)
                    .filter(|&s| s != seat)
                    .map(|s| reach[z].mass(s, c))
                    .product();
            }
        }
    }
    backward_values(tree, profile, &model.support[seat], seat, mass);
    for (z, node) in tree.nodes.iter().enumerate() {
        if node.actor != Some(seat) {
            continue;
        }
        for &c in &model.support[seat] {
            if mass[z][c] <= 0.0 {
                continue;
            }
            let maximum = node
                .children
                .iter()
                .map(|&child| values[child][c])
                .fold(f64::NEG_INFINITY, f64::max);
            let row = logit.row_mut(tree, z, c);
            let mut total = 0.0;
            for (out, &child) in row.iter_mut().zip(&node.children) {
                // Keep representable probabilities for log-space path products.
                *out = (qre.lambda * ((values[child][c] - maximum) / mass[z][c]))
                    .max(-700.0)
                    .exp();
                total += *out;
            }
            for out in row {
                *out /= total;
            }
        }
    }
}

/// Log of each player's own path probability. Fixed order, independent of Rayon.
pub(super) fn paths(
    tree: &Tree,
    model: &Model<'_>,
    logit: &Profile,
    out: &mut Vec<Vec<[f64; 169]>>,
) {
    out.resize_with(tree.seats, Vec::new);
    for (seat, path) in out.iter_mut().enumerate() {
        path.resize(tree.nodes.len(), [0.0; 169]);
        for (z, node) in tree.nodes.iter().enumerate() {
            if let Some((parent, action)) = node.parent {
                path[z] = path[parent];
                if tree.nodes[parent].actor == Some(seat) {
                    for &c in &model.support[seat] {
                        path[z][c] += logit.row(tree, parent, c)[action].ln();
                    }
                }
            } else {
                path[z].fill(0.0);
            }
        }
    }
}

/// Add M combos distributed according to the logit path reach and input range.
/// Normalize over all preflop combos, before conditioning on the sampled board.
pub(super) fn add_prior(
    model: &Model<'_>,
    seat: usize,
    path: &[f64; 169],
    prior: f64,
    out: &mut [f64; NUM_COMBOS],
) {
    let catalog = Classes::get();
    let maximum = model.support[seat]
        .iter()
        .map(|&c| path[c])
        .fold(f64::NEG_INFINITY, f64::max);
    let mut weights = [0.0; 169];
    let mut total = 0.0;
    for &c in &model.support[seat] {
        weights[c] = model.weights[seat][c] * (path[c] - maximum).exp();
        total += catalog.n(c) as f64 * weights[c];
    }
    for (h, reach) in out.iter_mut().enumerate() {
        *reach += prior * weights[class(h)] / total;
    }
}

/// Preserve the actual counterfactual mass while using the smoothed conditional
/// opponent distribution for the strategic L1-minus-checkdown residual.
pub(super) fn residual_factors(
    model: &Model<'_>,
    opponent: usize,
    reach: &Reach,
    smoothed: &[f64; NUM_COMBOS],
) -> [f64; 169] {
    let mut factors = [0.0; 169];
    let catalog = Classes::get();
    for (c, factor) in factors.iter_mut().enumerate() {
        let mass: f64 = model.support[opponent]
            .iter()
            .map(|&d| f64::from(catalog.k(c, d)) * smoothed[catalog.representative(d)])
            .sum();
        *factor = reach.mass(opponent, c) / mass;
    }
    factors
}
