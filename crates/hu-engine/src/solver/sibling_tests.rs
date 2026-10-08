use super::*;
use crate::{Dcfr, F32Storage, TempNode, TreeSpec};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Evaluator {
    batches: AtomicUsize,
    calls: Mutex<BTreeMap<u32, usize>>,
}
impl TerminalEvaluator for Evaluator {
    fn eval(&self, terminal: u32, _: Player, reach: &[f32], out: &mut [f32]) {
        assert!(out.iter().all(|&v| v.to_bits() == 0));
        *self.calls.lock().unwrap().entry(terminal).or_default() += 1;
        let total: f32 = reach.iter().sum();
        for (h, v) in out.iter_mut().enumerate() {
            *v = total * ((terminal % 7) as f32 - (h % 3) as f32);
        }
    }
    fn eval_cfr_siblings(&self, ids: &[u32], p: Player, reach: &[f32], outs: &mut [&mut [f32]]) {
        self.batches.fetch_add(1, Ordering::Relaxed);
        assert_eq!(ids.len(), outs.len());
        for (&id, out) in ids.iter().zip(outs) {
            self.eval_cfr(id, p, reach, out);
        }
    }
}
// Uses the public default sibling implementation unchanged.
#[derive(Default)]
struct DefaultEvaluator(Evaluator);
impl TerminalEvaluator for DefaultEvaluator {
    fn eval(&self, terminal: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        self.0.eval(terminal, p, reach, out);
    }
}
fn actions(depth: usize, p: Player, next: &mut u32) -> TempNode {
    if depth == 0 {
        *next += 1;
        TempNode::Terminal { id: *next, tag: 0 }
    } else {
        TempNode::Action {
            player: p,
            tag: 0,
            children: (0..3)
                .map(|_| actions(depth - 1, p.opponent(), next))
                .collect(),
        }
    }
}

#[test]
fn sibling_rows_default_pruning_empty_dimensions_and_parallel_actions() {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                for dim in [0, 257] {
                    let mut next = 0;
                    let tree = PublicTree::compile(TreeSpec {
                        root: TempNode::Action {
                            player: Player::P0,
                            tag: 0,
                            children: vec![
                                TempNode::Terminal { id: 1000, tag: 0 },
                                actions(4, Player::P1, &mut next),
                                TempNode::Terminal { id: 1001, tag: 0 },
                                actions(4, Player::P1, &mut next),
                                TempNode::Terminal { id: 1002, tag: 0 },
                            ],
                        },
                        root_dims: PerPlayer::new(dim, dim),
                        masks: vec![],
                        transitions: vec![],
                    });
                    assert_eq!(parallel_actions(&tree, 0), threads > 1 && dim > 0);
                    for precision in [CfrPrecision::F32, CfrPrecision::F64] {
                        for zero in [false, true] {
                            let evaluator = Evaluator::default();
                            let default = DefaultEvaluator::default();
                            let discounts = Dcfr::default().at(1, Some(8));
                            let reach = vec![if zero { -0.0 } else { 0.5 }; dim as usize];
                            let own = vec![0.75; dim as usize];
                            let mut a = F32Storage::new(tree.storage_len, tree.storage_refs.len());
                            let mut b = F32Storage::new(tree.storage_len, tree.storage_refs.len());
                            let par = ParConfig::default();
                            let ctx = PassCtx {
                                tree: &tree,
                                evaluator: &evaluator,
                                p: Player::P0,
                                cfr_precision: precision,
                                discounts: &discounts,
                                par,
                            };
                            let old = PassCtx {
                                tree: &tree,
                                evaluator: &default,
                                p: Player::P0,
                                cfr_precision: precision,
                                discounts: &discounts,
                                par,
                            };
                            let mut out = vec![0.0; dim as usize];
                            let mut expected = out.clone();
                            cfr_pass::<_, _, true>(
                                &ctx,
                                &mut a.view_mut(),
                                &mut Scratch::new(),
                                0,
                                &own,
                                &reach,
                                &mut out,
                                2,
                            );
                            cfr_pass::<_, _, true>(
                                &old,
                                &mut b.view_mut(),
                                &mut Scratch::new(),
                                0,
                                &own,
                                &reach,
                                &mut expected,
                                2,
                            );
                            assert_eq!(a.state(), b.state());
                            assert_eq!(
                                out.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                            );
                            let calls = evaluator.calls.lock().unwrap();
                            assert_eq!(*calls, *default.0.calls.lock().unwrap());
                            if zero || dim == 0 {
                                assert!(calls.is_empty());
                                assert_eq!(evaluator.batches.load(Ordering::Relaxed), 0);
                            } else {
                                // Two terminal rows separated by action subtrees and
                                // an odd singleton must each be evaluated exactly once.
                                for id in [1000, 1001, 1002] {
                                    assert_eq!(calls[&id], 1);
                                }
                                assert!(calls.values().all(|&n| n == 1));
                                assert_eq!(
                                    evaluator.batches.load(Ordering::Relaxed) > 0,
                                    precision == CfrPrecision::F32
                                );
                            }
                            drop(calls);
                            // The unpruned empty/zero-reach instantiation still
                            // visits terminals, with zero-initialized rows.
                            if zero || dim == 0 {
                                let mut storage =
                                    F32Storage::new(tree.storage_len, tree.storage_refs.len());
                                let mut unpruned = vec![0.0; dim as usize];
                                cfr_pass::<_, _, false>(
                                    &ctx,
                                    &mut storage.view_mut(),
                                    &mut Scratch::new(),
                                    0,
                                    &own,
                                    &reach,
                                    &mut unpruned,
                                    2,
                                );
                                assert!(!evaluator.calls.lock().unwrap().is_empty());
                            }
                        }
                    }
                }
            });
    }
}
