//! Research-only, one-expansion regret sampling. Production always instantiates
//! the ordinary worker and does not construct this public eligibility map.
use super::*;
use crate::{BettingState, HoldemGame, MultiwayAbstraction};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaisedPreflopResearchWork {
    pub variant: &'static str,
    pub completed_sweeps: u64,
    pub public_preflop_nodes: u64,
    pub eligible_public_nodes: u64,
    pub eligibility_bytes: u64,
    pub max_expansions_per_path: u8,
}

fn eligibility<A: MultiwayAbstraction>(
    game: &HoldemGame<A>,
    tree: &PublicTree,
) -> Result<(Vec<bool>, u64), SolverError> {
    let mut eligible = vec![false; tree.nodes.len()];
    let mut preflop_nodes = 0u64;
    let mut pending: Vec<(NodeId, BettingState)> = vec![(0, game.root_state())];
    while let Some((id, state)) = pending.pop() {
        let node = tree
            .nodes
            .get(id as usize)
            .ok_or(SolverError::InvalidState(
                "raised-opponent eligibility refers to a missing public node",
            ))?;
        if node.street != state.street || game.actor(&state) != Some(node.actor as usize) {
            return Err(SolverError::InvalidState(
                "raised-opponent public state mismatch",
            ));
        }
        if state.street != Street::Preflop {
            continue;
        }
        preflop_nodes += 1;
        let actions = game.node_actions(&state);
        if game.num_actions_of(&actions) != node.action_labels.len() {
            return Err(SolverError::InvalidState(
                "raised-opponent public menu mismatch",
            ));
        }
        eligible[id as usize] = state.aggressive_actions >= 1 && node.action_labels.len() > 1;
        for (action, child) in node.children.iter().enumerate() {
            if game.action_label_of(&actions, action) != node.action_labels[action] {
                return Err(SolverError::InvalidState(
                    "raised-opponent public label mismatch",
                ));
            }
            if let Child::Decision(child_id) = *child
                && tree.nodes[child_id as usize].street == Street::Preflop
            {
                pending.push((child_id, game.next_state_with(&state, &actions, action)));
            }
        }
    }
    Ok((eligible, preflop_nodes))
}

impl<A: MultiwayAbstraction> MultiwaySolver<HoldemGame<A>> {
    /// Fresh research training with one opponent expansion after a preflop
    /// raise on each path. The independent average-policy walk is unchanged.
    ///
    /// The selection is deliberately outside v1 configuration and SolverState.
    /// Callers must retain this variant in experiment provenance. This API is
    /// not an experimental checkpoint/resume interface: it requires sweep zero.
    pub fn run_raised_preflop_research(
        &mut self,
        sweeps: u64,
        threads: usize,
    ) -> Result<RaisedPreflopResearchWork, SolverError> {
        if !self.config.traverser_vector
            || self.dense.is_none()
            || self.game.recall_mode() != RecallMode::Street
            || self.config.prune
            || self.config.exploration_epsilon != 0.0
        {
            return Err(SolverError::InvalidState(
                "raised-opponent research requires dense range-vector, no pruning and zero exploration",
            ));
        }
        if sweeps == 0
            || self.completed_sweeps != 0
            || self.traversals != 0
            || self.next_sample_id != 0
        {
            return Err(SolverError::InvalidState(
                "raised-opponent research requires a fresh solver and positive sweeps",
            ));
        }
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        let (nodes, public_preflop_nodes) =
            eligibility(&self.game, &self.dense.as_ref().unwrap().tree)?;
        let eligible_public_nodes = nodes.iter().filter(|&&eligible| eligible).count() as u64;
        let completed_sweeps = self.run_sweeps_with_sampling::<true, _, _>(
            sweeps,
            threads,
            || true,
            |_| {},
            AverageOpponentSampling::UniformOne,
            &nodes,
        )?;
        Ok(RaisedPreflopResearchWork {
            variant: "enumerate-first-raised-preflop",
            completed_sweeps,
            public_preflop_nodes,
            eligible_public_nodes,
            eligibility_bytes: nodes.len() as u64,
            max_expansions_per_path: 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MultiwaySolver<HoldemGame<crate::abstraction::FeatureHashAbstraction>> {
        let (game, sampler, mut config) = super::super::tests::initialization_holdem_fixture();
        config.traverser_vector = true;
        config.exploration_epsilon = 0.0;
        config.prune = false;
        config.sweep_batch = 4;
        MultiwaySolver::new(game, sampler, config).unwrap()
    }

    #[test]
    fn raised_opponent_holdem_eligibility_excludes_unopened_and_postflop() {
        let solver = fixture();
        let tree = &solver.dense.as_ref().unwrap().tree;
        let (nodes, preflop) = eligibility(&solver.game, tree).unwrap();
        assert!(!nodes[0]);
        assert_eq!(
            preflop,
            tree.nodes
                .iter()
                .filter(|n| n.street == Street::Preflop)
                .count() as u64
        );
        assert!(nodes.iter().any(|&v| v));
        for (node, &eligible) in tree.nodes.iter().zip(&nodes) {
            if node.street != Street::Preflop || node.action_labels.len() <= 1 {
                assert!(!eligible);
            }
        }
        let mut saw_raise = false;
        let mut saw_passive = false;
        for (label, child) in tree.nodes[0]
            .action_labels
            .iter()
            .zip(&tree.nodes[0].children)
        {
            if let Child::Decision(id) = *child {
                if label.starts_with("raise-to:") {
                    assert!(nodes[id as usize]);
                    saw_raise = true;
                } else {
                    assert!(!nodes[id as usize]);
                    saw_passive = true;
                }
            }
        }
        assert!(saw_raise && saw_passive);
    }

    #[test]
    fn raised_opponent_holdem_threads_and_aligned_chunks_preserve_state() {
        let mut serial = fixture();
        let mut parallel = fixture();
        let mut chunked = fixture();
        let serial_work = serial.run_raised_preflop_research(16, 1).unwrap();
        assert_eq!(
            parallel.run_raised_preflop_research(16, 4).unwrap(),
            serial_work
        );
        let (nodes, _) = eligibility(&chunked.game, &chunked.dense.as_ref().unwrap().tree).unwrap();
        for sweeps in [4, 12] {
            chunked
                .run_sweeps_with_sampling::<true, _, _>(
                    sweeps,
                    2,
                    || true,
                    |_| {},
                    AverageOpponentSampling::UniformOne,
                    &nodes,
                )
                .unwrap();
        }
        assert_eq!(serial.snapshot_state(), parallel.snapshot_state());
        assert_eq!(serial.snapshot_state(), chunked.snapshot_state());
        assert_eq!(serial_work.completed_sweeps, 16);
        assert_eq!(serial_work.max_expansions_per_path, 1);
    }

    #[test]
    fn raised_opponent_rejects_unsupported_or_nonfresh_runs_without_mutation() {
        for defect in 0..6 {
            let mut solver = fixture();
            match defect {
                0 => solver.config.prune = true,
                1 => solver.config.exploration_epsilon = 0.06,
                2 => solver.config.traverser_vector = false,
                3 => solver.run_sweeps(4).unwrap(),
                _ => (),
            }
            let before = solver.snapshot_state();
            assert!(
                solver
                    .run_raised_preflop_research(
                        if defect == 4 { 0 } else { 4 },
                        if defect == 5 { 0 } else { 2 },
                    )
                    .is_err()
            );
            assert_eq!(solver.snapshot_state(), before);
        }
    }
}
