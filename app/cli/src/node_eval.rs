//! Per-node, per-action EV evaluation over a loaded `.mwsol` solution.
//!
//! This is the `.mwsol`-side counterpart of
//! [`multiway::solver::MultiwaySolver::evaluate_node_actions`]: it rebuilds
//! just the generative game and deal sampler from the solution's embedded
//! `config_toml` (via [`crate::session::build_multiway_game`], which does
//! not construct a solver or need `[algorithm]`/`[run]`), wraps the
//! solution's indexed strategy blocks in an
//! [`multiway::solver::AverageStrategyLookup`], and runs the exact same
//! evaluator a live solve uses.
//!
//! Cost note: for the `ehs2-table` abstraction backend this is cheap on an
//! artifact-cache hit (the bucket tables are just read back from disk); for
//! the `rollout-kmeans` backend, a missing/stale artifact cache makes
//! `build_multiway_game` retrain the abstraction from scratch, exactly as
//! `build_multiway_session` already documents for a fresh solve.

use anyhow::Result;
use formats::{MultiwaySolution, MultiwayStrategyKey};
use multiway::solver::{AverageStrategyLookup, InfoKey, NodeActionEvaluation};

use crate::session::build_multiway_game;

/// Guard against a malformed/cyclic game producing runaway recursion during
/// a playout; matches `SolverConfig::default().max_traversal_depth`, the
/// same bound a live solve normally runs under.
const DEFAULT_MAX_PLAYOUT_DEPTH: u32 = 512;

/// [`AverageStrategyLookup`] over a loaded `.mwsol`'s indexed strategy
/// blocks. A miss (an infoset the solve never visited, or one the `.mwsol`
/// simply never stored) resolves to `None`, letting the shared evaluator
/// apply its documented uniform fallback.
pub struct MwsolAverageStrategy<'a> {
    solution: &'a MultiwaySolution,
}

impl<'a> MwsolAverageStrategy<'a> {
    pub fn new(solution: &'a MultiwaySolution) -> Self {
        Self { solution }
    }
}

impl AverageStrategyLookup for MwsolAverageStrategy<'_> {
    fn lookup(&self, key: InfoKey) -> Option<Vec<f64>> {
        let mwsol_key = MultiwayStrategyKey {
            history: key.history.0,
            actor: key.player,
            street: key.street,
            active_opponents: key.active_opponents,
            bucket_path: key.bucket_path,
        };
        self.solution
            .strategy(mwsol_key)
            .map(|block| block.probabilities.iter().map(|&p| f64::from(p)).collect())
    }
}

/// Runs [`multiway::solver::evaluate_node_actions`] at `path` against
/// `solution`'s stored average strategies, rebuilding the generative game
/// and deal sampler from `solution.config_toml`. See this module's doc
/// comment for the abstraction-backend cost caveat.
pub fn evaluate_mwsol_node_actions(
    solution: &MultiwaySolution,
    path: &[usize],
    samples: u64,
    seed: u64,
) -> Result<NodeActionEvaluation> {
    let (game, sampler, _game_config) = build_multiway_game(&solution.config_toml)?;
    let strategy = MwsolAverageStrategy::new(solution);
    let evaluation = multiway::solver::evaluate_node_actions(
        &game,
        &sampler,
        &strategy,
        path,
        samples,
        seed,
        DEFAULT_MAX_PLAYOUT_DEPTH,
    )?;
    Ok(evaluation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{build_multiway_session, make_solution, metrics_row};

    /// Solves the bundled 3-max smoke config for a handful of sweeps,
    /// exports it in memory as a `MultiwaySolution` (mirroring the CLI's
    /// `--mwsol` path via `make_solution`, without touching disk), and
    /// checks that evaluating the root node against the exported solution
    /// produces finite EVs and per-action frequencies broadly consistent
    /// with the solution's own stored root strategies.
    #[test]
    fn evaluate_mwsol_node_actions_matches_root_strategy_within_mc_noise() {
        let raw = include_str!("../../../examples/preflop_multiway_3max_smoke.toml");
        let mut session = build_multiway_session(raw, None).expect("build multiway session");
        session.solver.run_sweeps(200).expect("run a short solve");

        let state = session.solver.snapshot_state();
        let seats = session.game_config.seats.len();
        let row = metrics_row(&session.solver.metrics(), vec![0.0; seats], 1.0, None);
        let solution = make_solution(
            &session.config_toml,
            session.solver.abstraction_fingerprint(),
            &state,
            &row,
        );

        let evaluation = evaluate_mwsol_node_actions(&solution, &[], 2_000, 7)
            .expect("evaluating the root node of the exported solution");
        assert!(evaluation.samples > 0);
        assert_eq!(evaluation.aggregate.len(), evaluation.action_labels.len());
        for action in &evaluation.aggregate {
            assert!(action.ev.mean.is_finite());
            assert!(action.frequency.is_finite());
        }
        let frequency_sum: f64 = evaluation
            .aggregate
            .iter()
            .map(|action| action.frequency)
            .sum();
        assert!(
            (frequency_sum - 1.0).abs() < 1e-6,
            "aggregate frequencies must sum to 1, got {frequency_sum}"
        );
        for group in &evaluation.groups {
            let group_sum: f64 = group.frequencies.iter().sum();
            assert!((group_sum - 1.0).abs() < 1e-6);
        }

        // Root strategies actually stored by this solve should agree, up to
        // Monte Carlo noise, with the evaluator's own root-node frequency
        // lookups for the same bucket (the evaluator derives `frequencies`
        // from exactly the same stored blocks via `MwsolAverageStrategy`).
        let root_key = multiway::solver::HistoryKey::ROOT;
        let stored_root: Vec<_> = state
            .policies
            .iter()
            .filter(|entry| entry.key.history == root_key)
            .collect();
        assert!(
            !stored_root.is_empty(),
            "expected at least one visited root policy after a short solve"
        );
        let mut overlap = 0;
        for entry in stored_root {
            let group = evaluation
                .groups
                .iter()
                .find(|group| group.group == entry.key.bucket_path[0]);
            if let Some(group) = group {
                overlap += 1;
                let stored = entry.column.average_strategy();
                assert_eq!(group.frequencies.len(), stored.len());
                for (looked_up, &stored_probability) in group.frequencies.iter().zip(&stored) {
                    assert!((looked_up - f64::from(stored_probability)).abs() < 1e-6);
                }
            }
        }
        assert!(
            overlap > 0,
            "expected at least one preflop bucket visited by both training and evaluation"
        );
    }
}
