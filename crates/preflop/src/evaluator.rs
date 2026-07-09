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
    pub fn new(_table: &EquityTable) -> Self {
        todo!("implemented by trunk/evaluator agent")
    }

    /// Registers a terminal's per-player coefficients, returning its id
    /// (the `TempNode::Terminal { id, .. }` the builder must use).
    pub fn push_terminal(&mut self, _coef: PerPlayer<TermCoef>) -> u32 {
        todo!("implemented by trunk/evaluator agent")
    }

    /// Number of registered terminals.
    pub fn num_terminals(&self) -> usize {
        self.terminals.len()
    }
}

impl TerminalEvaluator for PreflopEvaluator {
    fn eval(&self, _terminal: u32, _p: Player, _opp_reach: &[f32], _out: &mut [f32]) {
        debug_assert_eq!(_opp_reach.len(), NUM_CLASSES);
        debug_assert_eq!(_out.len(), NUM_CLASSES);
        todo!("implemented by trunk/evaluator agent")
    }
}
