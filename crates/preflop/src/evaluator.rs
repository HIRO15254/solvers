//! Class-level terminal evaluator: three shared 169x169 tables plus three
//! scalars per terminal per player (see the crate docs for the derivation).

use cards::{NUM_CLASSES, PerPlayer, Player};
use engine::TerminalEvaluator;

use crate::equity::EquityTable;
use crate::model::TermCoef;

/// Terminal evaluator over the 169 preflop hand classes.
///
/// Shared tables are row-major `h * NUM_CLASSES + o`:
/// - `compat[h][o] = N(h, o) / (n_h * n_o)`,
/// - `compat_win[h][o] = compat[h][o] * e_win(h, o)`,
/// - `compat_tie[h][o] = compat[h][o] * e_tie(h, o)`.
///
/// `eval` computes, for player `p`'s class `h`:
///
/// ```text
/// out[h] = Σ_o opp_reach[o] * (a * compat + b * compat_win + c * compat_tie)[h][o]
/// ```
///
/// with `(a, b, c)` the terminal's coefficients for `p`. Equity tables are
/// hero-perspective and player-agnostic (`e_win(h, o)` is "h beats o"), so
/// both players share the same three tables; orientation lives entirely in
/// the coefficients.
pub struct PreflopEvaluator {
    compat: Vec<f32>,
    compat_win: Vec<f32>,
    compat_tie: Vec<f32>,
    terminals: Vec<PerPlayer<[f64; 3]>>,
}

impl PreflopEvaluator {
    /// Builds the shared tables from the equity table and
    /// [`crate::compat_counts`], with no terminals yet.
    pub fn new(table: &EquityTable) -> Self {
        let counts = crate::classes::compat_counts();
        let combo_counts = crate::classes::class_combo_counts();
        let len = NUM_CLASSES * NUM_CLASSES;
        let mut compat = vec![0f32; len];
        let mut compat_win = vec![0f32; len];
        let mut compat_tie = vec![0f32; len];
        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                let idx = h * NUM_CLASSES + o;
                let c = counts[idx] as f64 / (combo_counts[h] as f64 * combo_counts[o] as f64);
                compat[idx] = c as f32;
                compat_win[idx] = (c * table.win(h, o)) as f32;
                compat_tie[idx] = (c * table.tie(h, o)) as f32;
            }
        }
        PreflopEvaluator {
            compat,
            compat_win,
            compat_tie,
            terminals: Vec::new(),
        }
    }

    /// Registers a terminal's per-player coefficients, returning its id
    /// (the `TempNode::Terminal { id, .. }` the builder must use).
    pub fn push_terminal(&mut self, coef: PerPlayer<TermCoef>) -> u32 {
        let id = self.terminals.len() as u32;
        self.terminals.push(coef.map(|c| [c.a, c.b, c.c]));
        id
    }

    /// Number of registered terminals.
    pub fn num_terminals(&self) -> usize {
        self.terminals.len()
    }
}

impl TerminalEvaluator for PreflopEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        debug_assert_eq!(opp_reach.len(), NUM_CLASSES);
        debug_assert_eq!(out.len(), NUM_CLASSES);
        let [a, b, c] = self.terminals[terminal as usize][p];
        for (h, out_h) in out.iter_mut().enumerate() {
            let row_c = &self.compat[h * NUM_CLASSES..(h + 1) * NUM_CLASSES];
            let row_w = &self.compat_win[h * NUM_CLASSES..(h + 1) * NUM_CLASSES];
            let row_t = &self.compat_tie[h * NUM_CLASSES..(h + 1) * NUM_CLASSES];
            let mut acc = 0f64;
            for o in 0..NUM_CLASSES {
                acc += opp_reach[o] as f64
                    * (a * row_c[o] as f64 + b * row_w[o] as f64 + c * row_t[o] as f64);
            }
            *out_h = acc as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    /// Naive triple loop straight from `N(h, o) / (n_h * n_o)` and the raw
    /// table, independent of the evaluator's shared-table machinery.
    fn naive_eval(
        win: &[f64],
        tie: &[f64],
        coef: TermCoef,
        opp_reach: &[f32],
        counts: &[u32],
        combo_counts: &[u32; NUM_CLASSES],
    ) -> Vec<f32> {
        let mut out = vec![0f32; NUM_CLASSES];
        for h in 0..NUM_CLASSES {
            let mut acc = 0f64;
            for o in 0..NUM_CLASSES {
                let idx = h * NUM_CLASSES + o;
                let compat = counts[idx] as f64 / (combo_counts[h] as f64 * combo_counts[o] as f64);
                let u = coef.a + coef.b * win[idx] + coef.c * tie[idx];
                acc += opp_reach[o] as f64 * compat * u;
            }
            out[h] = acc as f32;
        }
        out
    }

    #[test]
    fn eval_matches_naive_triple_loop_differential() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let len = NUM_CLASSES * NUM_CLASSES;

        // Random win/tie tables, values in [0, 0.5].
        let win: Vec<f64> = (0..len).map(|_| rng.gen_range(0.0..0.5)).collect();
        let tie: Vec<f64> = (0..len).map(|_| rng.gen_range(0.0..0.5)).collect();
        let table = EquityTable::from_probabilities(win.clone(), tie.clone());

        let counts = crate::classes::compat_counts();
        let combo_counts = crate::classes::class_combo_counts();

        let random_coef = |rng: &mut ChaCha8Rng| TermCoef {
            a: rng.gen_range(-5.0..5.0),
            b: rng.gen_range(-5.0..5.0),
            c: rng.gen_range(-5.0..5.0),
        };
        let opp_reach: Vec<f32> = (0..NUM_CLASSES).map(|_| rng.gen_range(0.0..1.0)).collect();

        for p in Player::BOTH {
            let coef_p0 = random_coef(&mut rng);
            let coef_p1 = random_coef(&mut rng);
            let mut evaluator = PreflopEvaluator::new(&table);
            let id = evaluator.push_terminal(PerPlayer::new(coef_p0, coef_p1));
            let mut out = vec![0f32; NUM_CLASSES];
            evaluator.eval(id, p, &opp_reach, &mut out);

            let coef = if p == Player::P0 { coef_p0 } else { coef_p1 };
            let expected = naive_eval(&win, &tie, coef, &opp_reach, &counts, &combo_counts);

            for h in 0..NUM_CLASSES {
                let e = expected[h] as f64;
                let a = out[h] as f64;
                let tol = 1e-4 * e.abs().max(1.0);
                assert!((e - a).abs() <= tol, "p={p:?} h={h}: expected {e}, got {a}");
            }
        }
    }
}
