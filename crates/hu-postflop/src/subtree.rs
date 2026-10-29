//! Frozen-profile subtree traversal with whole-tree reporting.
use crate::index::GameIndex;
use anyhow::Result;
use hu_engine::{NodeKind, Solver, Storage, SubtreeRoot};
use nlh::{PerPlayer, Player};
use std::collections::BTreeSet;

/// The selected execution scope, retained in run.json but not in legacy .sol metadata.
#[derive(Clone, Debug)]
pub struct SolveScope {
    pub scope: &'static str,
    pub subtree_roots: usize,
    pub fallback_reason: Option<&'static str>,
}

pub(crate) struct SubtreeSolve {
    roots: Vec<SubtreeRoot>,
    outside_ev: PerPlayer<f64>,
    outside_gain: PerPlayer<f64>,
}

/// Construct the maximal disjoint profile entries and their frozen entry reaches.
pub fn roots<S: Storage>(
    solver: &Solver<crate::PostflopEvaluator, S>,
    index: &GameIndex,
    targets: &BTreeSet<hu_engine::NodeId>,
) -> Vec<SubtreeRoot> {
    let game = solver.game();
    let tree = &game.tree;
    targets
        .iter()
        .filter_map(|&node| {
            let path = hu_engine::path_from_root(&index.parents, node);
            if path[..path.len() - 1].iter().any(|id| targets.contains(id)) {
                return None;
            }
            let reach = hu_engine::reach_on_path(
                tree,
                PerPlayer::new(
                    game.root_ranges[Player::P0].as_slice(),
                    game.root_ranges[Player::P1].as_slice(),
                ),
                &path,
                |id, _, out| out.copy_from_slice(&solver.average_strategy_at(id)),
            );
            let mut chance_weight = 1.0;
            let mut chance_depth = 0;
            for hop in path.windows(2) {
                let parent = tree.node(hop[0]);
                if parent.kind == NodeKind::Chance {
                    chance_depth += 1;
                    chance_weight *= f64::from(
                        tree.deal(parent, (hop[1] - parent.first_child) as usize)
                            .weight,
                    );
                }
            }
            Some(SubtreeRoot {
                node,
                reach,
                chance_weight,
                chance_depth,
            })
        })
        .collect()
}

fn subtree_values<S: Storage>(
    solver: &Solver<crate::PostflopEvaluator, S>,
    roots: &[SubtreeRoot],
) -> (PerPlayer<f64>, PerPlayer<f64>) {
    let mut ev = PerPlayer::new(0.0, 0.0);
    let mut gain = PerPlayer::new(0.0, 0.0);
    for value in solver.subtree_values(roots) {
        for p in Player::BOTH {
            ev[p] += value[p].0;
            gain[p] += value[p].1 - value[p].0;
        }
    }
    (ev, gain)
}

impl SubtreeSolve {
    pub fn step<S: Storage>(
        &self,
        solver: &mut Solver<crate::PostflopEvaluator, S>,
        iterations: u64,
    ) {
        for _ in 0..iterations {
            solver.step_subtrees(&self.roots);
        }
    }
    pub fn evaluate<S: Storage>(
        &self,
        solver: &Solver<crate::PostflopEvaluator, S>,
    ) -> (PerPlayer<f64>, PerPlayer<f64>) {
        let (ev, gain) = subtree_values(solver, &self.roots);
        (
            PerPlayer::new(
                self.outside_ev[Player::P0] + ev[Player::P0],
                self.outside_ev[Player::P1] + ev[Player::P1],
            ),
            PerPlayer::new(
                self.outside_gain[Player::P0] + gain[Player::P0],
                self.outside_gain[Player::P1] + gain[Player::P1],
            ),
        )
    }
}

/// Initialize outside averages before calculating a deterministic baseline. This
/// also runs before checkpoint restoration, so resume gets the same constant.
pub(crate) fn prepare<S: Storage>(
    solver: &mut Solver<crate::PostflopEvaluator, S>,
    index: &GameIndex,
    summary: Option<&crate::profile::ProfileSummary>,
    has_frequency: bool,
    base: Option<&crate::artifact::LoadedSol>,
    target: Option<f64>,
) -> Result<(Option<SubtreeSolve>, Option<SolveScope>)> {
    let Some(base) = base else {
        return Ok((None, None));
    };
    let summary = summary.expect("freeze has a resolved profile");
    let fallback = |reason| {
        Ok((
            None,
            Some(SolveScope {
                scope: "whole-tree",
                subtree_roots: 0,
                fallback_reason: Some(reason),
            }),
        ))
    };
    if has_frequency {
        return fallback("frequency-targets");
    }
    let targets: BTreeSet<_> = summary
        .resolved_nodes
        .iter()
        .filter(|n| !matches!(n.kind, crate::profile::ResolvedNodeKind::Frozen))
        .map(|n| n.node)
        .collect();
    if targets.is_empty() {
        return fallback("no-targets");
    }
    if targets.contains(&0) {
        return fallback("root-target");
    }
    let roots = roots(solver, index, &targets);
    let mut spans: Vec<_> = roots
        .iter()
        .map(|root| {
            let span = solver.game().tree.storage_spans[root.node as usize];
            (span.sref_start, span.sref_end)
        })
        .collect();
    spans.sort_unstable();
    let inside = |s: u32| {
        let i = spans.partition_point(|&(start, _)| start <= s);
        i != 0 && s < spans[i - 1].1
    };
    // Reuse the already hash-checked and tree-validated base reader.
    let outside = |id: u32| {
        let tree = &solver.game().tree;
        let node = tree.node(id);
        let s = tree.storage_ref(node).index;
        !inside(s)
    };
    for (id, node) in solver.game().tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Action && outside(id as u32) && !base.is_stored(id as u32) {
            return fallback("missing-base-strategy");
        }
    }
    // The base reader is chunked; only one node's strategy is retained at once.
    for id in 0..solver.game().tree.nodes.len() {
        let node = *solver.game().tree.node(id as u32);
        if node.kind != NodeKind::Action {
            continue;
        }
        let r = solver.game().tree.storage_ref(&node);
        if inside(r.index) {
            continue;
        }
        let strategy = base
            .stored_strategy(id as u32)?
            .expect("stored node checked above");
        solver.restore_stream(0, |storage| -> Result<()> {
            storage.seed_strategy(r, r.index, &strategy);
            Ok(())
        })?;
    }
    let (full_ev, full_gain) = solver.evaluate();
    let (part_ev, part_gain) = subtree_values(solver, &roots);
    let outside_ev = PerPlayer::new(
        full_ev[Player::P0] - part_ev[Player::P0],
        full_ev[Player::P1] - part_ev[Player::P1],
    );
    let outside_gain = PerPlayer::new(
        full_gain[Player::P0] - part_gain[Player::P0],
        full_gain[Player::P1] - part_gain[Player::P1],
    );
    if target
        .is_some_and(|target| (outside_gain[Player::P0] + outside_gain[Player::P1]) / 2.0 >= target)
    {
        // A whole-tree fallback must start from exactly the original zero state,
        // not the base averages installed for the residual calculation.
        solver.restore_stream(0, |storage| -> Result<()> {
            match storage.arrays_mut() {
                hu_engine::StorageArraysMut::F32 { strategy_sum, .. }
                | hu_engine::StorageArraysMut::Mixed { strategy_sum, .. } => strategy_sum.fill(0.0),
                hu_engine::StorageArraysMut::I16 {
                    strategy_sum,
                    strategy_scales,
                    ..
                } => {
                    strategy_sum.fill(0);
                    strategy_scales.fill(1.0);
                }
            }
            Ok(())
        })?;
        return fallback("outside-residual");
    }
    let scope = SolveScope {
        scope: "subtree",
        subtree_roots: roots.len(),
        fallback_reason: None,
    };
    Ok((
        Some(SubtreeSolve {
            roots,
            outside_ev,
            outside_gain,
        }),
        Some(scope),
    ))
}
