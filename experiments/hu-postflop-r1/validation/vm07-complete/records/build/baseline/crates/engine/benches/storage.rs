//! Micro-benchmarks for the storage backends' per-node operations
//! (`StorageOps`) and the tree's reach-map primitives (`PublicTree::
//! map_reach_into`/`accumulate_values`, `SparseTransition::apply_forward`),
//! at realistic postflop-node shapes: `A = 3` actions (a typical
//! bet/call/fold-shaped node), `H = 1,326` hands (every hold'em combo).
//!
//! Run with `cargo bench -p engine` (see `docs/development.md`'s "Criterion
//! suite" section for the full A/B workflow via `--save-baseline`/
//! `--baseline`); `cargo bench -p engine -- --test` runs one iteration per
//! bench as a compile+smoke-test check.

use std::time::Duration;

use cards::{PerPlayer, Player};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use engine::{
    Discounts, F32Storage, I16Storage, PublicTree, ReachMap, SparseTransition, Storage, StorageOps,
    StorageRef, TempNode, TreeSpec,
};

/// A typical postflop action node: bet/call/fold-shaped (3 actions) over
/// every hold'em combo (1,326 hands).
const NUM_ACTIONS: usize = 3;
const NUM_HANDS: usize = 1_326;

/// Representative of a mid-solve DCFR step: positive regrets barely
/// discounted, negative regrets discounted hard, average strategy in the
/// CFR+-like linear-weighting regime. Values other than exactly `1.0`
/// exercise the same multiply-then-add path a real solve takes (`1.0`
/// everywhere would let a sufficiently aggressive optimizer treat the
/// factor as a no-op).
fn discounts() -> Discounts {
    Discounts {
        pos: 0.98,
        neg: 0.4,
        avg: 0.95,
        floor_neg: false,
        reset_avg: false,
    }
}

fn node_ref() -> StorageRef {
    StorageRef {
        offset: 0,
        num_actions: NUM_ACTIONS as u16,
        num_hands: NUM_HANDS as u32,
        index: 0,
    }
}

/// Mixed-sign instantaneous regrets/counterfactual values, roughly
/// chip-scale -- realistic input for `update_regrets`.
fn mixed_signal(len: usize) -> Vec<f32> {
    (0..len).map(|i| ((i % 11) as f32 - 5.0) * 1.3).collect()
}

/// Nonnegative reach-weighted strategy contributions -- realistic input for
/// `accumulate_strategy`.
fn weighted_signal(len: usize) -> Vec<f32> {
    (0..len).map(|i| (i % 13) as f32 / 13.0 * 0.7).collect()
}

fn bench_f32(c: &mut Criterion) {
    let r = node_ref();
    let d = discounts();
    let inst = mixed_signal(r.len());
    let weighted = weighted_signal(r.len());
    let mut group = c.benchmark_group("f32");

    group.bench_function("update_regrets", |b| {
        let mut storage = F32Storage::new(r.len(), 1);
        b.iter(|| {
            storage.update_regrets(black_box(r), black_box(r.index), black_box(&inst), &d);
        });
    });

    group.bench_function("regret_matching", |b| {
        let mut storage = F32Storage::new(r.len(), 1);
        storage.update_regrets(r, r.index, &inst, &d);
        let mut out = vec![0.0f32; r.len()];
        b.iter(|| {
            storage.regret_matching(black_box(r), black_box(r.index), &mut out);
            black_box(&out);
        });
    });

    group.bench_function("accumulate_strategy", |b| {
        let mut storage = F32Storage::new(r.len(), 1);
        b.iter(|| {
            storage.accumulate_strategy(black_box(r), black_box(r.index), black_box(&weighted), &d);
        });
    });

    group.bench_function("average_strategy", |b| {
        let mut storage = F32Storage::new(r.len(), 1);
        storage.accumulate_strategy(r, r.index, &weighted, &d);
        let mut out = vec![0.0f32; r.len()];
        b.iter(|| {
            storage.average_strategy(black_box(r), black_box(r.index), &mut out);
            black_box(&out);
        });
    });

    group.finish();
}

fn bench_i16(c: &mut Criterion) {
    let r = node_ref();
    let d = discounts();
    let inst = mixed_signal(r.len());
    let weighted = weighted_signal(r.len());
    let mut group = c.benchmark_group("i16");

    group.bench_function("update_regrets", |b| {
        let mut storage = I16Storage::new(r.len(), 1);
        b.iter(|| {
            storage.update_regrets(black_box(r), black_box(r.index), black_box(&inst), &d);
        });
    });

    group.bench_function("regret_matching", |b| {
        let mut storage = I16Storage::new(r.len(), 1);
        storage.update_regrets(r, r.index, &inst, &d);
        let mut out = vec![0.0f32; r.len()];
        b.iter(|| {
            storage.regret_matching(black_box(r), black_box(r.index), &mut out);
            black_box(&out);
        });
    });

    group.bench_function("accumulate_strategy", |b| {
        let mut storage = I16Storage::new(r.len(), 1);
        b.iter(|| {
            storage.accumulate_strategy(black_box(r), black_box(r.index), black_box(&weighted), &d);
        });
    });

    group.bench_function("average_strategy", |b| {
        let mut storage = I16Storage::new(r.len(), 1);
        storage.accumulate_strategy(r, r.index, &weighted, &d);
        let mut out = vec![0.0f32; r.len()];
        b.iter(|| {
            storage.average_strategy(black_box(r), black_box(r.index), &mut out);
            black_box(&out);
        });
    });

    group.finish();
}

/// A root chance node with a single `Mask` deal over 1,326 dims -- the same
/// shape `PublicTree::compile` produces for a hold'em turn/river card deal
/// (see `crates/engine/src/reach.rs`'s tests for the fixture style this
/// mirrors). The mask itself doesn't need real card-removal semantics
/// (`engine` is poker-agnostic); it just needs realistic size and a mix of
/// zeroed/live entries.
fn mask_tree() -> PublicTree {
    let mask: Vec<f32> = (0..NUM_HANDS)
        .map(|i| if i % 50 == 0 { 0.0 } else { 1.0 })
        .collect();
    let maps = PerPlayer::new(ReachMap::Mask(0), ReachMap::Mask(0));
    let root = TempNode::Chance {
        deals: vec![(1.0, maps, TempNode::Terminal { id: 0, tag: 0 })],
        tag: 0,
    };
    PublicTree::compile(TreeSpec {
        root,
        masks: vec![mask],
        transitions: Vec::new(),
        root_dims: PerPlayer::new(NUM_HANDS as u32, NUM_HANDS as u32),
    })
}

/// A quotient transition matching `holdem::postflop`'s
/// `quotient_transition` sizing: a merged suit-isomorphism class of 2
/// members, each contributing `NUM_COMBOS - 51` live (non-dead-card) entries
/// at weight `1 / num_members` -- 2,550 entries total.
fn merged_class_transition() -> SparseTransition {
    const MEMBERS: usize = 2;
    const LIVE: usize = NUM_HANDS - 51;
    let weight = 1.0 / MEMBERS as f32;
    let mut entries = Vec::with_capacity(MEMBERS * LIVE);
    for m in 0..MEMBERS {
        for h in 0..LIVE {
            let out_idx = (h + m * 111) % NUM_HANDS;
            entries.push((h as u32, out_idx as u32, weight));
        }
    }
    SparseTransition {
        in_dim: NUM_HANDS as u32,
        out_dim: NUM_HANDS as u32,
        entries,
    }
}

fn bench_tree(c: &mut Criterion) {
    let tree = mask_tree();
    let node = tree.node(0);
    let deal = *tree.deal(node, 0);

    let reach_in: Vec<f32> = weighted_signal(NUM_HANDS).iter().map(|w| w + 0.1).collect();
    let mut reach_out = vec![0.0f32; NUM_HANDS];
    let child_vals = mixed_signal(NUM_HANDS);
    let mut acc = vec![0.0f32; NUM_HANDS];

    let mut group = c.benchmark_group("tree");

    group.bench_function("map_reach_mask", |b| {
        b.iter(|| {
            tree.map_reach_into(
                black_box(deal.maps[Player::P0]),
                black_box(&reach_in),
                &mut reach_out,
            );
            black_box(&reach_out);
        });
    });

    group.bench_function("accumulate_values_mask", |b| {
        b.iter(|| {
            acc.fill(0.0);
            tree.accumulate_values(
                black_box(deal.maps[Player::P0]),
                black_box(deal.weight),
                black_box(&child_vals),
                &mut acc,
            );
            black_box(&acc);
        });
    });

    let transition = merged_class_transition();
    let reach = weighted_signal(NUM_HANDS);
    let mut out = vec![0.0f32; NUM_HANDS];
    group.bench_function("transition_forward", |b| {
        b.iter(|| {
            transition.apply_forward(black_box(&reach), &mut out);
            black_box(&out);
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(3));
    targets = bench_f32, bench_i16, bench_tree
}
criterion_main!(benches);
