//! Macro-benchmark of a turn-start postflop solve: builds the tree once,
//! then benches `Solver::run(5)` from a fixed snapshot, sequentially and in
//! parallel. Compare against `docs/development.md`'s bench-bar targets, and use
//! `--save-baseline`/`--baseline` (see that doc's "Criterion suite" section)
//! to A/B solver changes.
//!
//! Run with `cargo bench -p holdem`; `cargo bench -p holdem -- --test` runs
//! one iteration per bench as a smoke test.

use std::time::Duration;

use cards::{Card, Chips, PerPlayer, Range};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use engine::{Dcfr, F32Storage, ParConfig, Solver};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{PerStreet, PostflopConfig, build_postflop_game};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn parse_cards(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

/// No chance-node parallelism: matches the fixed-reduction-order helper used
/// throughout the test suite (`rake_icm.rs`, `viewer.rs`), so "sequential"
/// measures the single-threaded walk rather than "parallel with nothing to
/// fan out onto".
fn sequential() -> ParConfig {
    ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    }
}

/// Turn-start spot: board "2s 7s Ks 2h" (matching `examples/turn_small.toml`
/// and several `holdem` test fixtures), `postflop_srp20.toml`'s wide
/// single-raised-pot ranges (so the per-node hand math is realistic),
/// at `turn_small.toml`'s smaller pot/stack. Per-node cost is driven by hand
/// count (always 1,326, independent of range sparsity) and tree shape, not
/// range size, so widening the ranges doesn't slow this down relative to
/// `turn_small.toml`'s tiny ones -- a single chance node (turn into river)
/// keeps a `run(5)` fast enough for `sample_size(10)`.
fn turn_config() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("2s 7s Ks 2h"),
        ranges: PerPlayer::new(
            "22+,A2s+,K9s+,Q9s+,J9s+,T8s+,97s+,87s,76s,A8o+,KTo+,QTo+,JTo"
                .parse::<Range>()
                .unwrap(),
            "22+,A2s+,K7s+,Q8s+,J8s+,T7s+,96s+,86s+,75s+,65s,54s,A5o+,K9o+,Q9o+,J9o+,T9o"
                .parse::<Range>()
                .unwrap(),
        ),
        pot: Chips(20),
        effective_stack: Chips(80),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![1.0], vec![1.0]),
        },
        raise_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![1.0], vec![1.0]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 1,
            river: 1,
        },
        iso_merging: true,
        track_node_info: true,
    }
}

fn bench_solve(c: &mut Criterion) {
    let config = turn_config();
    let built = build_postflop_game(&config, chip_ev());
    let mut solver = Solver::<_, F32Storage>::new(built.game, Box::<Dcfr>::default(), Some(5));
    let snapshot = solver.state();

    let mut group = c.benchmark_group("solve");

    solver.set_par(sequential());
    group.bench_function("sequential", |b| {
        b.iter_batched(
            || snapshot.clone(),
            |state| {
                solver.restore_state(state).unwrap();
                solver.run(5);
            },
            BatchSize::SmallInput,
        );
    });

    // Default `ParConfig` (turn+river chance-fanout eligible); this lazily
    // spins up rayon's global thread pool on first use.
    solver.set_par(ParConfig::default());
    group.bench_function("parallel", |b| {
        b.iter_batched(
            || snapshot.clone(),
            |state| {
                solver.restore_state(state).unwrap();
                solver.run(5);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(7));
    targets = bench_solve
}
criterion_main!(benches);
