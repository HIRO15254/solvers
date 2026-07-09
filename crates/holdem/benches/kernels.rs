//! Micro-benchmarks for `PostflopEvaluator::eval`'s two terminal kernels
//! (`kernel::showdown_kernel`'s sorted-rank sweep, `kernel::fold_kernel`'s
//! inclusion-exclusion fold), exercised over a real river subgame with
//! wide (full-ish) ranges on both sides so the kernels sweep close to the
//! full 1,326-combo table, the same amount of work a real solve asks of
//! them every terminal visit.
//!
//! Run with `cargo bench -p holdem` (see `docs/bench.md`); `cargo bench -p
//! holdem -- --test` runs one iteration per bench as a smoke test.

use std::time::Duration;

use cards::{Card, Chips, NUM_COMBOS, PerPlayer, Player, Range};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use engine::TerminalEvaluator;
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{RiverConfig, RiverGame, build_river_game};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn parse_cards(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

/// A single-bet river subgame with full-ish ranges on both sides: `oop`
/// plays a value-heavy range, `ip` a wider one, so the showdown/fold kernels
/// sweep close to the full 1,326-combo table on both sides. One bet size
/// and `max_raises = 1` keep the tree tiny while still producing exactly
/// the two terminal shapes this bench needs (see `terminal_ids`).
fn river_config() -> RiverConfig {
    RiverConfig {
        board: parse_cards("Ks 7h 2d Jc 9s").try_into().unwrap(),
        ranges: PerPlayer::new(
            "22+,A2s+,KTo+".parse::<Range>().unwrap(),
            "55-22,QJs,A5s-A2s,KQo,T9s".parse::<Range>().unwrap(),
        ),
        pot: Chips(20),
        effective_stack: Chips(80),
        bet_fractions: PerPlayer::new(vec![0.75], vec![0.75]),
        max_raises: 1,
    }
}

/// Locates the fold-terminal and showdown-terminal ids reached after the
/// river's single bet. `max_raises = 1` means the facing player's node
/// (after that one bet) has no further raise available, so its `node_info`
/// entry is exactly the two-action `["fold", "call"]` shape; on the river,
/// `call` resolves straight to a showdown terminal (never a chance node).
/// `PublicTree::compile` lays a node's children out in the builder's action
/// order, so `first_child`/`first_child + 1` are the fold/showdown
/// terminals in that same order -- this is the "distinguish two terminals
/// from outside the crate" trick: `PostflopTerminal::kind` is private, but
/// the tree shape around a bet-with-no-more-raises node isn't.
fn terminal_ids(game: &RiverGame) -> (u32, u32) {
    let info = game
        .node_info
        .iter()
        .find(|info| {
            info.actions.len() == 2 && info.actions[0] == "fold" && info.actions[1] == "call"
        })
        .expect("expected a facing-bet node with exactly fold+call actions");
    let node_id = game
        .node_by_history(&info.history)
        .expect("node_by_history must find the tagged node");
    let node = game.game.tree.node(node_id);
    assert_eq!(
        node.num_children, 2,
        "expected exactly fold + call children"
    );
    let fold_id = game.game.tree.node(node.first_child).aux;
    let showdown_id = game.game.tree.node(node.first_child + 1).aux;
    (fold_id, showdown_id)
}

fn bench_kernels(c: &mut Criterion) {
    let config = river_config();
    let built = build_river_game(&config, chip_ev());
    let (fold_id, showdown_id) = terminal_ids(&built);

    // The already board-conflict-zeroed root range: a realistic dense-ish
    // reach vector, the same shape `cfr_pass`/`value_pass` pass to
    // `TerminalEvaluator::eval` at every terminal visit.
    let opp_reach = built.game.root_ranges[Player::P1].clone();
    let mut out = vec![0.0f32; NUM_COMBOS];

    let mut group = c.benchmark_group("kernels");

    group.bench_function("fold", |b| {
        b.iter(|| {
            built.game.evaluator.eval(
                black_box(fold_id),
                black_box(Player::P0),
                black_box(&opp_reach),
                &mut out,
            );
            black_box(&out);
        });
    });

    group.bench_function("showdown", |b| {
        b.iter(|| {
            built.game.evaluator.eval(
                black_box(showdown_id),
                black_box(Player::P0),
                black_box(&opp_reach),
                &mut out,
            );
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
    targets = bench_kernels
}
criterion_main!(benches);
