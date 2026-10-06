use super::*;
use crate::schedule::{CfrPlus, Dcfr, HsDcfr};
use crate::storage::{F32Storage, I16Storage, StorageOps};
use crate::tree::{ReachMap, SparseTransition, TempNode, TreeSpec};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Evaluator {
    zero_calls: AtomicUsize,
}

impl TerminalEvaluator for Evaluator {
    fn eval(&self, terminal: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        if reach.iter().all(|&x| x == 0.0) {
            self.zero_calls.fetch_add(1, Ordering::Relaxed);
        }
        let total: f32 = reach.iter().sum();
        for (h, v) in out.iter_mut().enumerate() {
            let payoff = (terminal % 7) as f32 - 3.0 + (h % 3) as f32;
            *v = total
                * if p == Player::P0 {
                    payoff
                } else {
                    -payoff - 0.25
                };
        }
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

fn game(dim: usize) -> CompiledGame<Evaluator> {
    let mut next = 0;
    let root = TempNode::Action {
        player: Player::P1,
        tag: 0,
        children: (0..3)
            .map(|_| TempNode::Chance {
                tag: 0,
                deals: vec![
                    (
                        0.25,
                        PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
                        actions(4, Player::P0, &mut next),
                    ),
                    (
                        0.5,
                        PerPlayer::new(ReachMap::Transition(0), ReachMap::Transition(0)),
                        actions(3, Player::P1, &mut next),
                    ),
                    (
                        0.25,
                        PerPlayer::new(ReachMap::Mask(0), ReachMap::Mask(0)),
                        actions(3, Player::P0, &mut next),
                    ),
                ],
            })
            .collect(),
    };
    CompiledGame {
        tree: PublicTree::compile(TreeSpec {
            root,
            root_dims: PerPlayer::new(dim as u32, dim as u32),
            masks: vec![vec![0.0; dim]],
            transitions: vec![SparseTransition {
                in_dim: dim as u32,
                out_dim: (dim / 2) as u32,
                entries: (0..dim / 2).map(|h| (h as u32, h as u32, 1.0)).collect(),
            }],
        }),
        evaluator: Evaluator::default(),
        root_ranges: PerPlayer::new(vec![0.75; dim], vec![0.5; dim]),
        normalizer: 1.0,
        zero_sum: false,
    }
}

fn seeded<S: Storage>(dim: usize, schedule: Box<dyn DiscountSchedule>) -> Solver<Evaluator, S> {
    let mut solver = Solver::<_, S>::new(game(dim), schedule, Some(64));
    let d = Discounts {
        pos: 1.0,
        neg: 1.0,
        avg: 1.0,
        floor_neg: false,
        reset_avg: false,
    };
    for &r in &solver.game.tree.storage_refs {
        let hands = r.num_hands as usize;
        let mut regrets = vec![-2.0; r.len()];
        regrets[..hands].fill(4.0);
        solver.storage.update_regrets(r, r.index, &regrets, &d);
        let mut weighted = vec![0.0; r.len()];
        weighted[..hands].fill(0.75);
        solver
            .storage
            .accumulate_strategy(r, r.index, &weighted, &d);
    }
    solver.set_par(ParConfig {
        chance_depth: 2,
        min_children: 2,
    });
    solver
}

fn unpruned_values<S: Storage>(solver: &Solver<Evaluator, S>, p: Player) -> (f64, f64) {
    let ctx = ValueCtx {
        tree: &solver.game.tree,
        evaluator: &solver.game.evaluator,
        storage: &solver.storage,
        p,
        par: solver.par,
    };
    let mut scratch = Scratch::new();
    let mut ev = vec![0.0; ctx.tree.root_dims[p] as usize];
    let mut br = ev.clone();
    // Recording preserves the old full traversal, including zero reaches.
    value_pass::<_, _, _, true, true, true>(
        &ctx,
        &mut scratch,
        0,
        &solver.game.root_ranges[p.opponent()],
        &mut ev,
        &mut br,
        &no_record,
        solver.par.chance_depth,
    );
    (solver.root_aggregate(p, &ev), solver.root_aggregate(p, &br))
}

fn check<S: Storage>(
    dim: usize,
    threads: usize,
    schedule: fn() -> Box<dyn DiscountSchedule>,
) -> StorageState {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            let mut old = seeded::<S>(dim, schedule());
            let mut new = seeded::<S>(dim, schedule());
            for iteration in 1..=20 {
                old.step_impl::<false>();
                new.step();
                assert_eq!(
                    old.state(),
                    new.state(),
                    "iteration={iteration}, threads={threads}"
                );
                for p in Player::BOTH {
                    let (ev, br) = unpruned_values(&old, p);
                    assert_eq!(ev, new.expected_value(p));
                    assert_eq!(br, new.best_response_value(p));
                    let (evaluated_ev, expl) = new.evaluate();
                    assert_eq!(ev, evaluated_ev[p]);
                    assert_eq!(br - ev, expl[p]);
                    assert_eq!(
                        old.expected_values_everywhere(p),
                        new.expected_values_everywhere(p)
                    );
                }
            }
            assert!(old.game.evaluator.zero_calls.load(Ordering::Relaxed) > 0);
            // The only zero-reach evaluations in the new solver came from the
            // deliberately unpruned recording walk above. Root-only EV/BR skip all.
            new.game.evaluator.zero_calls.store(0, Ordering::Relaxed);
            new.evaluate();
            new.step();
            assert_eq!(new.game.evaluator.zero_calls.load(Ordering::Relaxed), 0);
            let recorded = new.expected_values_everywhere(Player::P0);
            let visited = Mutex::new(vec![false; recorded.len()]);
            new.visit_expected_values(|id, _, values, _| {
                let r = new.game.tree.storage_ref(new.game.tree.node(id));
                assert_eq!(
                    values[Player::P0],
                    recorded[r.index as usize].as_ref().unwrap()
                );
                assert!(
                    values[Player::P0]
                        .iter()
                        .zip(recorded[r.index as usize].as_ref().unwrap())
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
                visited.lock().unwrap()[r.index as usize] = true;
            });
            assert!(visited.into_inner().unwrap().into_iter().all(|x| x));
            new.state().storage
        })
}

fn check_storage<S: Storage>() {
    let schedules: [fn() -> Box<dyn DiscountSchedule>; 3] = [
        || Box::<Dcfr>::default(),
        || Box::new(CfrPlus),
        || Box::new(HsDcfr { gamma0: 30.0 }),
    ];
    for schedule in schedules {
        for dim in [0, 5, 257] {
            assert_eq!(check::<S>(dim, 1, schedule), check::<S>(dim, 8, schedule));
        }
    }
}

#[test]
fn zero_opponent_subtree_f32_matches_full_walk() {
    check_storage::<F32Storage>();
}

#[test]
fn zero_opponent_subtree_i16_matches_full_walk() {
    check_storage::<I16Storage>();
}

#[test]
fn nonzero_subnormal_opponent_reach_is_evaluated() {
    let mut g = game(1);
    g.tree = PublicTree::compile(TreeSpec {
        root: TempNode::Terminal { id: 4, tag: 0 },
        root_dims: PerPlayer::new(1, 1),
        masks: vec![],
        transitions: vec![],
    });
    g.root_ranges[Player::P1][0] = f32::from_bits(1);
    let solver = Solver::<_, F32Storage>::new(g, Box::<Dcfr>::default(), None);
    assert_eq!(
        solver.expected_value(Player::P0),
        0.75 * f64::from(f32::from_bits(1))
    );
    let discounts = Dcfr::default().at(1, None);
    let ctx = PassCtx {
        tree: &solver.game.tree,
        evaluator: &solver.game.evaluator,
        p: Player::P0,
        discounts: &discounts,
        par: solver.par,
    };
    let mut storage = F32Storage::new(0, 0);
    let mut view = storage.view_mut();
    let mut scratch = Scratch::new();
    let mut out = scratch.take(1);
    cfr_pass::<_, _, true>(
        &ctx,
        &mut view,
        &mut scratch,
        0,
        &[0.75],
        &[f32::from_bits(1)],
        &mut out,
        0,
    );
    assert_eq!(out[0].to_bits(), 1);
    scratch.put(out);
    let mut out = scratch.take(1);
    // Reusing a formerly nonzero buffer still satisfies the terminal contract.
    cfr_pass::<_, _, true>(
        &ctx,
        &mut view,
        &mut scratch,
        0,
        &[0.75],
        &[0.0],
        &mut out,
        0,
    );
    assert_eq!(out[0].to_bits(), 0);
    assert_eq!(solver.game.evaluator.zero_calls.load(Ordering::Relaxed), 0);
}

#[derive(Default)]
struct Activity {
    matching: AtomicUsize,
    update_workers: Mutex<BTreeSet<usize>>,
}

// Observe real storage updates inside a dead subtree, where terminal calls
// cannot reveal parallel execution because they are all skipped.
struct TrackingView<V> {
    inner: V,
    activity: Arc<Activity>,
}

impl<V: StorageView> StorageOps for TrackingView<V> {
    fn regret_matching(&self, r: StorageRef, index: u32, out: &mut [f32]) {
        self.activity.matching.fetch_add(1, Ordering::Relaxed);
        self.inner.regret_matching(r, index, out);
    }

    fn update_regrets(&mut self, r: StorageRef, index: u32, inst: &[f32], d: &Discounts) {
        assert!(inst.iter().all(|&x| x == 0.0));
        self.activity
            .update_workers
            .lock()
            .unwrap()
            .insert(rayon::current_thread_index().unwrap());
        self.inner.update_regrets(r, index, inst, d);
    }

    fn accumulate_strategy(&mut self, r: StorageRef, index: u32, weighted: &[f32], d: &Discounts) {
        self.inner.accumulate_strategy(r, index, weighted, d);
    }

    fn average_strategy(&self, r: StorageRef, index: u32, out: &mut [f32]) {
        self.inner.average_strategy(r, index, out);
    }

    fn raw_regrets(&self, r: StorageRef, index: u32, out: &mut [f32]) {
        self.inner.raw_regrets(r, index, out);
    }
}

impl<V: StorageView> StorageView for TrackingView<V> {
    fn split(&mut self, spans: &[StorageSpan]) -> Vec<Self> {
        self.inner
            .split(spans)
            .into_iter()
            .map(|inner| Self {
                inner,
                activity: self.activity.clone(),
            })
            .collect()
    }
}

fn check_parallel_updates<S: Storage>(par: ParConfig) {
    rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap()
        .install(|| {
            let g = game(257);
            let discounts = Dcfr::default().at(1, None);
            let ctx = PassCtx {
                tree: &g.tree,
                evaluator: &g.evaluator,
                p: Player::P0,
                discounts: &discounts,
                par,
            };
            let zeros = vec![0.0; 257];
            let mut old = S::new(g.tree.storage_len, g.tree.storage_refs.len());
            let mut new = S::new(g.tree.storage_len, g.tree.storage_refs.len());
            let old_activity = Arc::new(Activity::default());
            let new_activity = Arc::new(Activity::default());
            {
                let mut view = TrackingView {
                    inner: old.view_mut(),
                    activity: old_activity.clone(),
                };
                cfr_pass::<_, _, false>(
                    &ctx,
                    &mut view,
                    &mut Scratch::new(),
                    0,
                    &g.root_ranges[Player::P0],
                    &zeros,
                    &mut vec![0.0; 257],
                    par.chance_depth,
                );
            }
            g.evaluator.zero_calls.store(0, Ordering::Relaxed);
            {
                let mut view = TrackingView {
                    inner: new.view_mut(),
                    activity: new_activity.clone(),
                };
                cfr_pass::<_, _, true>(
                    &ctx,
                    &mut view,
                    &mut Scratch::new(),
                    0,
                    &g.root_ranges[Player::P0],
                    &zeros,
                    &mut vec![0.0; 257],
                    par.chance_depth,
                );
            }
            assert_eq!(old.state(), new.state());
            assert_eq!(g.evaluator.zero_calls.load(Ordering::Relaxed), 0);
            assert!(
                new_activity.matching.load(Ordering::Relaxed)
                    < old_activity.matching.load(Ordering::Relaxed)
            );
            assert!(
                new_activity.update_workers.lock().unwrap().len() > 1,
                "zero-opponent-reach storage updates must still run on multiple workers"
            );
        });
}

#[test]
fn zero_opponent_pass_keeps_parallel_updates() {
    for chance_depth in [0, 2] {
        let par = ParConfig {
            chance_depth,
            min_children: 2,
        };
        check_parallel_updates::<F32Storage>(par);
        check_parallel_updates::<I16Storage>(par);
    }
}
