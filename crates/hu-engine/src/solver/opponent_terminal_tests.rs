use super::*;
use crate::{Dcfr, F32Storage, StorageOps, TempNode, TreeSpec};
use std::collections::BTreeMap;

type RecordedBatch = Vec<(u32, Vec<u32>)>;

#[derive(Default)]
struct Evaluator {
    calls: Mutex<BTreeMap<u32, usize>>,
    batches: Mutex<Vec<RecordedBatch>>,
}
fn write(id: u32, reach: &[f32], out: &mut [f32]) {
    let total: f32 = reach.iter().sum();
    let utility = match id {
        1000 => 1e12,
        1001 => -1e12,
        _ => (id % 7) as f32 - 3.25,
    };
    for (h, v) in out.iter_mut().enumerate() {
        *v = total * (utility + (h % 3) as f32);
    }
}
impl TerminalEvaluator for Evaluator {
    fn eval(&self, id: u32, _: Player, reach: &[f32], out: &mut [f32]) {
        assert!(out.iter().all(|&v| v.to_bits() == 0));
        *self.calls.lock().unwrap().entry(id).or_default() += 1;
        write(id, reach, out);
    }
    fn add_cfr_opponent_terminals(
        &self,
        ids: &[u32],
        _: Player,
        reaches: &[&[f32]],
        out: &mut [f32],
        tmp: &mut [f32],
    ) {
        self.batches.lock().unwrap().push(
            ids.iter()
                .zip(reaches)
                .map(|(&id, r)| (id, r.iter().map(|r| r.to_bits()).collect()))
                .collect(),
        );
        for (&id, &reach) in ids.iter().zip(reaches) {
            tmp.fill(0.0);
            // Deliberately bypass eval: calls to these ids detect accidental
            // terminal recursion, even if evaluated twice with the same reach.
            write(id, reach, tmp);
            for (v, &x) in out.iter_mut().zip(tmp.iter()) {
                *v += x;
            }
        }
    }
}
#[derive(Default)]
struct DefaultEvaluator(Evaluator);
impl TerminalEvaluator for DefaultEvaluator {
    fn eval(&self, id: u32, p: Player, r: &[f32], out: &mut [f32]) {
        self.0.eval(id, p, r, out);
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
fn run<const PRUNE: bool>(
    threads: usize,
    own_dim: u32,
    opp_dim: u32,
    precision: CfrPrecision,
    zero: bool,
    zero_child: bool,
) -> (Vec<u32>, StorageState) {
    let mut next = 0;
    let tree = PublicTree::compile(TreeSpec {
        root: TempNode::Action {
            player: Player::P1,
            tag: 0,
            children: vec![
                TempNode::Terminal { id: 1000, tag: 0 },
                actions(5, Player::P0, &mut next),
                TempNode::Terminal { id: 1001, tag: 0 },
                actions(5, Player::P0, &mut next),
                TempNode::Terminal { id: 1002, tag: 0 },
            ],
        },
        root_dims: PerPlayer::new(own_dim, opp_dim),
        masks: vec![],
        transitions: vec![],
    });
    assert_eq!(
        own_dim != 0 && parallel_actions(&tree, 0),
        threads > 1 && own_dim != 0
    );
    let mut a = F32Storage::new(tree.storage_len, tree.storage_refs.len());
    let mut b = F32Storage::new(tree.storage_len, tree.storage_refs.len());
    let sref = tree.storage_ref(tree.node(0));
    let seed = Discounts {
        pos: 1.0,
        neg: 1.0,
        avg: 1.0,
        floor_neg: false,
        reset_avg: false,
    };
    let mut regrets = vec![1.0; sref.len()];
    if zero_child {
        regrets[2 * opp_dim as usize..3 * opp_dim as usize].fill(-1.0);
    }
    a.update_regrets(sref, sref.index, &regrets, &seed);
    b.update_regrets(sref, sref.index, &regrets, &seed);
    let mut sigma = vec![0.0; sref.len()];
    a.regret_matching_cfr(sref, sref.index, &mut sigma, precision);
    let discounts = Dcfr::default().at(1, Some(8));
    let par = ParConfig::default();
    let evaluator = Evaluator::default();
    let default = DefaultEvaluator::default();
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
    let own = vec![0.75; own_dim as usize];
    let reach: Vec<_> = (0..opp_dim)
        .map(|h| if zero || h % 13 == 0 { -0.0 } else { 0.5 })
        .collect();
    let mut out = vec![0.0; own_dim as usize];
    let mut expected = out.clone();
    cfr_pass::<_, _, PRUNE>(
        &ctx,
        &mut a.view_mut(),
        &mut Scratch::new(),
        0,
        &own,
        &reach,
        &mut out,
        2,
    );
    cfr_pass::<_, _, PRUNE>(
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
    let batches = evaluator.batches.lock().unwrap();
    let root_batches: Vec<_> = batches
        .iter()
        .filter(|batch| batch.first().is_some_and(|(id, _)| *id >= 1000))
        .collect();
    let all_zero = reach.iter().all(|&r| r == 0.0);
    // T21 is retained outside the T25b subtree entry. In particular, the
    // zero-dimension tree fits the new bound and uses the default whole batch.
    let subtree_batch = precision == CfrPrecision::F32
        && !tree.subtree_has_chance[0]
        && subtree_elements(&tree, 0) < 2 * ACTION_PAR_MIN_ELEMENTS;
    let batched = precision == CfrPrecision::F32 && !subtree_batch && !(PRUNE && all_zero);
    let mut wanted = Vec::new();
    for (action, id) in [(0, 1000), (2, 1001), (4, 1002)] {
        let r: Vec<_> = reach
            .iter()
            .enumerate()
            .map(|(h, &r)| r * sigma[action * opp_dim as usize + h])
            .collect();
        if !PRUNE || r.iter().any(|&r| r != 0.0) {
            wanted.push((id, r.iter().map(|r| r.to_bits()).collect::<Vec<_>>()));
        }
    }
    if batched {
        assert_eq!(
            root_batches
                .iter()
                .flat_map(|b| b.iter())
                .cloned()
                .collect::<Vec<_>>(),
            wanted
        );
        assert!(root_batches.iter().all(|b| b.len() <= 2));
    } else {
        assert!(root_batches.is_empty());
    }
    let calls = evaluator.calls.lock().unwrap();
    let default_calls = default.0.calls.lock().unwrap();
    for id in [1000, 1001, 1002] {
        assert_eq!(
            calls.get(&id).copied().unwrap_or(0),
            if batched {
                0
            } else {
                usize::from(wanted.iter().any(|(i, _)| *i == id))
            }
        );
        assert_eq!(
            default_calls.get(&id).copied().unwrap_or(0),
            usize::from(wanted.iter().any(|(i, _)| *i == id))
        );
    }
    drop(calls);
    drop(default_calls);
    drop(batches);
    // Independently reduce child values in the required order. Large opposite
    // utilities above expose accidental interleaving with nonterminal values.
    let mut ordered = vec![0.0; own_dim as usize];
    let mut storage = F32Storage::new(tree.storage_len, tree.storage_refs.len());
    storage.update_regrets(sref, sref.index, &regrets, &seed);
    let order: Vec<_> = if batched {
        vec![0, 2, 4, 1, 3]
    } else {
        vec![0, 1, 2, 3, 4]
    };
    for action in order {
        let r: Vec<_> = reach
            .iter()
            .enumerate()
            .map(|(h, &r)| {
                if PRUNE && all_zero {
                    0.0
                } else {
                    r * sigma[action * opp_dim as usize + h]
                }
            })
            .collect();
        let child = tree.node(0).first_child + action as u32;
        let mut row = vec![0.0; own_dim as usize];
        cfr_pass::<_, _, PRUNE>(
            &old,
            &mut storage.view_mut(),
            &mut Scratch::new(),
            child,
            &own,
            &r,
            &mut row,
            2,
        );
        if !(PRUNE && all_zero) {
            for (v, x) in ordered.iter_mut().zip(row) {
                *v += x;
            }
        }
    }
    assert_eq!(
        out.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        ordered.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    (out.iter().map(|v| v.to_bits()).collect(), a.state())
}
#[test]
fn opponent_terminal_batches_prune_zero_dimensions_and_action_parallelism() {
    let solve = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let mut results = Vec::new();
                for (own, opp) in [(0, 0), (0, 257), (257, 0), (257, 257)] {
                    for precision in [CfrPrecision::F32, CfrPrecision::F64] {
                        for zero in [false, true] {
                            for zero_child in [false, true] {
                                results.push(run::<true>(
                                    threads, own, opp, precision, zero, zero_child,
                                ));
                                results.push(run::<false>(
                                    threads, own, opp, precision, zero, zero_child,
                                ));
                            }
                        }
                    }
                }
                results
            })
    };
    assert_eq!(solve(1), solve(4));
}
