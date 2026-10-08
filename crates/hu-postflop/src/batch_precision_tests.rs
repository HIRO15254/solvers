//! Actual rank-sweep evaluator versus recursive per-terminal f32 CFR.
use super::*;
use hu_engine::{CompiledGame, Discounts, NodeKind, StorageOps};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Probe {
    evaluator: PostflopEvaluator,
    batches: AtomicUsize,
}
impl TerminalEvaluator for Probe {
    fn eval(&self, id: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        self.evaluator.eval(id, p, reach, out);
    }
    fn eval_cfr(&self, id: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        self.evaluator.eval_cfr(id, p, reach, out);
    }
    // Sibling/opponent hooks deliberately use the defaults: the reference
    // recursion evaluates every terminal individually, with f32 matching.
    fn eval_cfr_batch(&self, ids: &[u32], p: Player, reaches: &[&[f32]], outs: &mut [&mut [f32]]) {
        self.batches.fetch_add(1, Ordering::Relaxed);
        self.evaluator.eval_cfr_batch(ids, p, reaches, outs);
    }
    fn set_cfr_precision(&mut self, precision: CfrPrecision) {
        self.evaluator.set_cfr_precision(precision);
    }
}
fn solver(
    cfg: &PostflopConfig,
    recursive: bool,
    zero: Option<Player>,
) -> hu_engine::Solver<Probe, F32Storage> {
    let game = build_postflop_game(
        cfg,
        PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        },
    )
    .game;
    let mut tree = game.tree;
    if recursive {
        // Test-only metadata change prevents the new entry, without adding a
        // production switch. A one-thread pool keeps the old view ambient.
        tree.subtree_has_chance.fill(true);
    }
    let mut ranges = game.root_ranges;
    if let Some(p) = zero {
        ranges[p].fill(-0.0);
    }
    let mut solver = hu_engine::Solver::new(
        CompiledGame {
            tree,
            root_ranges: ranges,
            normalizer: game.normalizer,
            zero_sum: game.zero_sum,
            evaluator: Probe {
                evaluator: game.evaluator,
                batches: AtomicUsize::new(0),
            },
        },
        Box::<Dcfr>::default(),
        Some(12),
    );
    solver.set_cfr_precision(CfrPrecision::F32);
    let seed = Discounts {
        pos: 1.0,
        neg: 1.0,
        avg: 1.0,
        floor_neg: false,
        reset_avg: false,
    };
    let mut storage = F32Storage::new(
        solver.game().tree.storage_len,
        solver.game().tree.storage_refs.len(),
    );
    for node in solver
        .game()
        .tree
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Action)
    {
        let sref = solver.game().tree.storage_ref(node);
        let mut regrets = vec![1.0; sref.len()];
        if sref.index % 2 == 1 {
            let start = sref.len() - sref.num_hands as usize;
            regrets[start..].fill(-1.0);
        }
        storage.update_regrets(sref, sref.index, &regrets, &seed);
        storage.accumulate_strategy(sref, sref.index, &regrets, &seed);
    }
    solver
        .restore_state(hu_engine::SolverState {
            iteration: 1,
            storage: storage.state(),
        })
        .unwrap();
    solver
}
#[test]
fn actual_river_and_turn_batches_match_recursive_per_terminal_f32() {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            let mut maximum = 0.0f64;
            for river in [false, true] {
                let mut cfg = config();
                if river {
                    cfg.board.push("9s".parse().unwrap());
                    cfg.pot = Chips(20);
                    cfg.effective_stack = Chips(200);
                    cfg.streets.river =
                        StreetTree::pot_fractions(&[0.33, 0.75, 1.5], &[0.33, 0.75, 1.5], 3);
                }
                for p in Player::BOTH {
                    for zero in [None, Some(p.opponent())] {
                        let mut actual = solver(&cfg, false, zero);
                        let mut reference = solver(&cfg, true, zero);
                        actual.run(1);
                        reference.run(1);
                        assert!(actual.game().evaluator.batches.load(Ordering::Relaxed) > 0);
                        assert_eq!(
                            reference.game().evaluator.batches.load(Ordering::Relaxed),
                            0
                        );
                        let (
                            StorageState::F32 {
                                regrets: ar,
                                strategy_sum: aa,
                            },
                            StorageState::F32 {
                                regrets: br,
                                strategy_sum: ba,
                            },
                        ) = (reference.state().storage, actual.state().storage)
                        else {
                            unreachable!()
                        };
                        for node in actual
                            .game()
                            .tree
                            .nodes
                            .iter()
                            .filter(|n| n.kind == NodeKind::Action && n.player == p)
                        {
                            let sref = actual.game().tree.storage_ref(node);
                            let row = sref.offset..sref.offset + sref.len();
                            maximum =
                                maximum.max(relative_error(&ar[row.clone()], &br[row.clone()]));
                            maximum = maximum.max(relative_error(&aa[row.clone()], &ba[row]));
                        }
                        assert!(
                            maximum < 1e-5,
                            "river={river} p={p:?} zero={zero:?}: {maximum:e}"
                        );
                    }
                }
            }
            println!(
                "T25b actual rank evaluator maximum relative per-node state difference={maximum:e}"
            );
        });
}
