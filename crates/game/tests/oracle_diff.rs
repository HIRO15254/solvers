//! Differential tests: the vectorized engine's average strategy is exported
//! and re-evaluated by the frozen scalar oracle (`cfr-ref`). Expected value
//! and best-response value must agree between the two independent
//! implementations, which cross-validates the CFR walk, the terminal
//! evaluators, chance masks, and both exploitability implementations.

use std::collections::HashMap;

use cards::Player;
use cfr_ref::games::{Kuhn, Leduc};
use cfr_ref::{RefGame, best_response_value, expected_value};
use engine::{Dcfr, F32Storage, NodeKind, Solver};
use game::{ChipEv, NoRake, PayoffPipeline, ToyGame};

/// Exports the engine's average strategy as an oracle profile keyed by
/// `"{card}|{history}"`.
fn export_profile(
    toy_info: &[game::ToyNodeInfo],
    solver: &Solver<game::ToyEvaluator, F32Storage>,
) -> HashMap<String, Vec<f64>> {
    let tree = &solver.game().tree;
    let mut profile = HashMap::new();
    for node_id in 0..tree.nodes.len() as u32 {
        let node = tree.node(node_id);
        if node.kind != NodeKind::Action {
            continue;
        }
        let info = &toy_info[tree.tags[node_id as usize] as usize];
        let sigma = solver.average_strategy_at(node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        for hand in 0..num_hands {
            let key = format!("{hand}|{}", info.history);
            let dist: Vec<f64> = (0..num_actions)
                .map(|a| sigma[a * num_hands + hand] as f64)
                .collect();
            profile.insert(key, dist);
        }
    }
    profile
}

fn solve_toy(
    toy: ToyGame,
    iters: u64,
) -> (
    Vec<game::ToyNodeInfo>,
    Solver<game::ToyEvaluator, F32Storage>,
) {
    let info = toy.node_info.clone();
    let mut solver = Solver::<_, F32Storage>::new(toy.game, Box::<Dcfr>::default(), Some(iters));
    solver.run(iters);
    (info, solver)
}

fn assert_engine_matches_oracle<G: RefGame>(oracle: &G, toy: ToyGame, iters: u64, tol: f64) {
    let (info, solver) = solve_toy(toy, iters);
    let profile = export_profile(&info, &solver);

    for (p, oracle_p) in [(Player::P0, 0), (Player::P1, 1)] {
        let engine_ev = solver.expected_value(p);
        let oracle_ev = expected_value(oracle, &profile, oracle_p);
        assert!(
            (engine_ev - oracle_ev).abs() < tol,
            "EV mismatch for {p:?}: engine {engine_ev} vs oracle {oracle_ev}"
        );

        let engine_br = solver.best_response_value(p);
        let oracle_br = best_response_value(oracle, &profile, oracle_p);
        assert!(
            (engine_br - oracle_br).abs() < tol,
            "BR mismatch for {p:?}: engine {engine_br} vs oracle {oracle_br}"
        );
    }
}

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

#[test]
fn kuhn_engine_matches_oracle() {
    assert_engine_matches_oracle(&Kuhn::default(), game::kuhn(chip_ev()), 1_000, 1e-5);
}

#[test]
fn kuhn_engine_matches_oracle_early_iterates() {
    // Also compare a barely-converged profile: agreement must hold for any
    // strategy, not just near equilibrium.
    assert_engine_matches_oracle(&Kuhn::default(), game::kuhn(chip_ev()), 3, 1e-5);
}

#[test]
fn leduc_engine_matches_oracle() {
    assert_engine_matches_oracle(&Leduc::default(), game::leduc(chip_ev()), 300, 1e-4);
}
