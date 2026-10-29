//! Differential traversal tests with real cards, full river deals, compatible
//! hands and independent pairwise showdown/fold payoffs. The product tests
//! separately compare the actual rank-sweep evaluator with recursive CFR.
use super::*;
use crate::{F32Storage, StorageOps, TempNode, TreeSpec};
use nlh::{Card, Range, combo_cards, rank_of};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

thread_local! {
    static RECURSIVE: Cell<bool> = const { Cell::new(false) };
    static VALUES: RefCell<Option<BTreeMap<NodeId, Vec<f32>>>> = const { RefCell::new(None) };
}
pub(super) fn recursive_mode() -> bool {
    RECURSIVE.get()
}
pub(super) fn record(id: NodeId, values: &[f32]) {
    VALUES.with_borrow_mut(|rows| {
        if let Some(rows) = rows {
            rows.insert(id, values.to_vec());
        }
    });
}

struct Poker {
    hands: PerPlayer<Vec<[Card; 2]>>,
    matrices: Vec<PerPlayer<Vec<f32>>>,
}
// The evaluator is Sync; the test stays serial, but the trait requires it.
// Use atomics for the call count rather than any unsafe Sync implementation.
struct Evaluator {
    hands: PerPlayer<Vec<[Card; 2]>>,
    matrices: Vec<PerPlayer<Vec<f32>>>,
    batches: std::sync::atomic::AtomicUsize,
}
impl TerminalEvaluator for Evaluator {
    fn eval(&self, id: u32, p: Player, reach: &[f32], out: &mut [f32]) {
        for (dst, payoffs) in out
            .iter_mut()
            .zip(self.matrices[id as usize][p].chunks_exact(reach.len().max(1)))
        {
            let mut value = 0.0;
            for (&u, &r) in payoffs.iter().zip(reach) {
                value += u * r;
            }
            *dst = value;
        }
    }
    fn eval_cfr_batch(&self, ids: &[u32], p: Player, reaches: &[&[f32]], outs: &mut [&mut [f32]]) {
        self.batches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for ((&id, &r), out) in ids.iter().zip(reaches).zip(outs) {
            self.eval_cfr(id, p, r, out);
        }
    }
    fn compat_mass(&self, _space: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        for (dst, &own) in out.iter_mut().zip(&self.hands[p]) {
            *dst = self.hands[p.opponent()]
                .iter()
                .zip(opp_reach)
                .filter(|&(&opp, _)| mask(opp) & mask(own) == 0)
                .map(|(_, &r)| r)
                .sum();
        }
    }
    fn supports_compat_mass(&self) -> bool {
        true
    }
}
fn mask(cards: impl IntoIterator<Item = Card>) -> u64 {
    cards.into_iter().fold(0, |m, c| m | (1 << c.index()))
}
fn terminal(e: &mut Poker, board: &[Card], fold: Option<Player>, amount: f32) -> TempNode {
    let board_mask = mask(board.iter().copied());
    let matrix = PerPlayer::new(Player::P0, Player::P1).map(|p| {
        let mut matrix = Vec::new();
        for own in &e.hands[p] {
            for opp in &e.hands[p.opponent()] {
                let own_mask = mask(*own);
                let opp_mask = mask(*opp);
                let value = if (own_mask | opp_mask) & board_mask != 0 || own_mask & opp_mask != 0 {
                    0.0
                } else if let Some(winner) = fold {
                    if p == winner { amount } else { -amount }
                } else {
                    let a = rank_of(board.iter().copied().chain(*own));
                    let b = rank_of(board.iter().copied().chain(*opp));
                    match a.cmp(&b) {
                        std::cmp::Ordering::Greater => amount,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Less => -amount,
                    }
                };
                matrix.push(value);
            }
        }
        matrix
    });
    let id = e.matrices.len() as u32;
    e.matrices.push(matrix);
    TempNode::Terminal { id, tag: 0 }
}
fn action(p: Player, children: Vec<TempNode>) -> TempNode {
    TempNode::Action {
        player: p,
        children,
        tag: 0,
    }
}
fn river(e: &mut Poker, board: &[Card], base: f32) -> TempNode {
    // Matched prior contributions give +/-base at checkdown. A river bet15
    // or raise40 adds the matched stake; a folded raise refunds its excess.
    let check_show = terminal(e, board, None, base);
    let check_fold = terminal(e, board, Some(Player::P1), base);
    let check_call = terminal(e, board, None, base + 15.0);
    let check = action(
        Player::P1,
        vec![check_show, action(Player::P0, vec![check_fold, check_call])],
    );
    let bet_fold = terminal(e, board, Some(Player::P0), base);
    let bet_call = terminal(e, board, None, base + 15.0);
    let raise_fold = terminal(e, board, Some(Player::P1), base + 15.0);
    let raise_call = terminal(e, board, None, base + 40.0);
    let bet = action(
        Player::P1,
        vec![
            bet_fold,
            bet_call,
            action(Player::P0, vec![raise_fold, raise_call]),
        ],
    );
    action(Player::P0, vec![check, bet])
}
fn fixture(turn: bool) -> (PublicTree, Evaluator) {
    let mut board: Vec<Card> = "Ks 7h 2d 3c"
        .split_whitespace()
        .map(|c| c.parse().unwrap())
        .collect();
    if !turn {
        board.push("9s".parse().unwrap());
    }
    let board_mask = mask(board.iter().copied());
    let hands = PerPlayer::new(Player::P0, Player::P1).map(|p| {
        let range: Range = if p == Player::P0 {
            "AA,77,AQs,KQs,JTs"
        } else {
            "QQ,22,AJs,KJs,T9s"
        }
        .parse()
        .unwrap();
        range
            .weights()
            .iter()
            .enumerate()
            .filter_map(|(i, &w)| {
                let (a, b) = combo_cards(i);
                (w > 0.0 && mask([a, b]) & board_mask == 0).then_some([a, b])
            })
            .collect::<Vec<_>>()
    });
    let dims = hands.as_ref().map(|h| h.len() as u32);
    let mut e = Poker {
        hands,
        matrices: Vec::new(),
    };
    let mut masks = Vec::new();
    let root = if turn {
        let mut deals = Vec::new();
        for card in nlh::ALL_CARDS {
            if board_mask & mask([card]) != 0 {
                continue;
            }
            let maps = PerPlayer::new(Player::P0, Player::P1).map(|p| {
                let id = masks.len() as u32;
                masks.push(
                    e.hands[p]
                        .iter()
                        .map(|h| if h.contains(&card) { 0.0 } else { 1.0 })
                        .collect(),
                );
                crate::ReachMap::Mask(id)
            });
            let mut complete = board.clone();
            complete.push(card);
            deals.push((1.0 / 44.0, maps, river(&mut e, &complete, 25.0)));
        }
        let fold = terminal(&mut e, &board, Some(Player::P0), 10.0);
        // Facing a turn bet15 into pot20: fold, or call then the full river.
        // The called turn stake carries into each river payoff above.
        action(Player::P1, vec![fold, TempNode::Chance { deals, tag: 0 }])
    } else {
        river(&mut e, &board, 10.0)
    };
    let tree = PublicTree::compile(TreeSpec {
        root,
        root_dims: dims,
        masks,
        transitions: vec![],
    });
    (
        tree,
        Evaluator {
            hands: e.hands,
            matrices: e.matrices,
            batches: std::sync::atomic::AtomicUsize::new(0),
        },
    )
}
fn relative(exact: &[f32], actual: &[f32]) -> f64 {
    let scale = exact.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max);
    let error = exact
        .iter()
        .zip(actual)
        .map(|(&a, &b)| {
            assert!(b.is_finite());
            (f64::from(a) - f64::from(b)).abs()
        })
        .fold(0.0, f64::max);
    if scale == 0.0 {
        assert_eq!(error, 0.0);
        0.0
    } else {
        error / scale
    }
}
fn check<const PRUNE: bool>(
    tree: &PublicTree,
    e: &Evaluator,
    p: Player,
    zero: bool,
    bit_exact: bool,
) -> f64 {
    let seed = Discounts {
        pos: 1.0,
        neg: 1.0,
        avg: 1.0,
        floor_neg: false,
        reset_avg: false,
    };
    let mut reference = F32Storage::new(tree.storage_len, tree.storage_refs.len());
    for node in tree.nodes.iter().filter(|n| n.kind == NodeKind::Action) {
        let sref = tree.storage_ref(node);
        let mut regrets = vec![1.0; sref.len()];
        // Zero whole action rows at opponent nodes, including ancestors of
        // own nodes: these descendants must still discount/update storage.
        if node.player != p && sref.index % 2 == 1 {
            let len = sref.num_hands as usize;
            regrets[sref.len() - len..].fill(-1.0);
        }
        reference.update_regrets(sref, sref.index, &regrets, &seed);
        reference.accumulate_strategy(sref, sref.index, &regrets, &seed);
    }
    let mut actual = F32Storage::new(tree.storage_len, tree.storage_refs.len());
    actual.restore_state(reference.state()).unwrap();
    let discounts = Discounts {
        pos: 0.9,
        neg: 0.7,
        avg: 0.8,
        floor_neg: false,
        reset_avg: false,
    };
    let ctx = PassCtx {
        tree,
        evaluator: e,
        p,
        discounts: &discounts,
        cfr_precision: CfrPrecision::F32,
        par: ParConfig::default(),
        profile: None,
    };
    let my = vec![0.75; e.hands[p].len()];
    let opp: Vec<_> = (0..e.hands[p.opponent()].len())
        .map(|h| {
            if zero || h % 5 == 0 {
                -0.0
            } else {
                (h + 1) as f32 / 31.0
            }
        })
        .collect();
    let mut out = vec![0.0; my.len()];
    let mut expected = out.clone();
    RECURSIVE.set(true);
    VALUES.set(Some(BTreeMap::new()));
    cfr_pass::<_, _, PRUNE>(
        &ctx,
        &mut reference.view_mut(),
        &mut Scratch::new(),
        0,
        &my,
        &opp,
        &mut expected,
        0,
    );
    let expected_nodes = VALUES.take().unwrap();
    RECURSIVE.set(false);
    VALUES.set(Some(BTreeMap::new()));
    let before = e.batches.load(std::sync::atomic::Ordering::Relaxed);
    let mut scratch = Scratch::new();
    cfr_pass::<_, _, PRUNE>(
        &ctx,
        &mut actual.view_mut(),
        &mut scratch,
        0,
        &my,
        &opp,
        &mut out,
        0,
    );
    let actual_nodes = VALUES.take().unwrap();
    assert!(e.batches.load(std::sync::atomic::Ordering::Relaxed) > before);
    let mut maximum = relative(&expected, &out);
    let (
        crate::StorageState::F32 {
            regrets: ar,
            strategy_sum: aa,
        },
        crate::StorageState::F32 {
            regrets: br,
            strategy_sum: ba,
        },
    ) = (reference.state(), actual.state())
    else {
        unreachable!()
    };
    for (id, node) in tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind == NodeKind::Action)
    {
        if bit_exact {
            let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
            assert_eq!(
                bits(&expected_nodes[&(id as u32)]),
                bits(&actual_nodes[&(id as u32)]),
                "node {id}"
            );
        }
        maximum = maximum.max(relative(
            &expected_nodes[&(id as u32)],
            &actual_nodes[&(id as u32)],
        ));
        let sref = tree.storage_ref(node);
        maximum = maximum.max(relative(
            &ar[sref.offset..sref.offset + sref.len()],
            &br[sref.offset..sref.offset + sref.len()],
        ));
        maximum = maximum.max(relative(
            &aa[sref.offset..sref.offset + sref.len()],
            &ba[sref.offset..sref.offset + sref.len()],
        ));
    }
    if bit_exact {
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&expected), bits(&out));
        assert_eq!(bits(&ar), bits(&br), "regrets");
        assert_eq!(bits(&aa), bits(&ba), "strategy sums");
    }
    assert!(
        maximum < 1e-5,
        "{p:?} PRUNE={PRUNE} zero={zero}: {maximum:e}"
    );
    // A second warm call checks recycling of reference metadata allocations.
    out.fill(0.0);
    cfr_pass::<_, _, PRUNE>(
        &ctx,
        &mut actual.view_mut(),
        &mut scratch,
        0,
        &my,
        &opp,
        &mut out,
        0,
    );
    maximum
}
/// Full, partial, soft and zero-confidence lock rows plus incentives, some on
/// the same storage refs.
fn mixed_profile(tree: &PublicTree) -> crate::Profile {
    let n = tree.storage_refs.len();
    let mut profile = crate::Profile {
        locks: vec![None; n],
        incentives: vec![None; n],
    };
    for (i, sref) in tree.storage_refs.iter().enumerate() {
        let hands = sref.num_hands as usize;
        let actions = sref.num_actions as usize;
        if i % 3 == 1 {
            let mut fixed = vec![f32::NAN; actions * hands];
            let mut confidence = vec![0.0; hands];
            for h in 0..hands {
                let mut set = |a: usize, v: f32| fixed[a * hands + h] = v;
                match h % 4 {
                    0 => {
                        confidence[h] = 1.0;
                        (0..actions).for_each(|a| set(a, 0.0));
                        set(0, 0.25);
                        set(actions - 1, 0.75);
                    }
                    1 => {
                        confidence[h] = 1.0;
                        set(0, 0.5);
                    }
                    2 => {
                        confidence[h] = 0.5;
                        (0..actions).for_each(|a| set(a, 0.0));
                        set(actions - 1, 1.0);
                    }
                    _ => {}
                }
            }
            profile.locks[i] = Some(crate::NodeLock { fixed, confidence });
        }
        if i % 2 == 1 {
            let bonus = (0..actions * hands)
                .map(|i| (i % 5) as f32 * 0.25 - 0.5)
                .collect();
            profile.incentives[i] = Some(crate::NodeIncentive { bonus, space: 0 });
        }
    }
    profile
}
#[test]
fn dual_values_match_separate_passes_with_zero_reaches_chance_and_profiles() {
    for threads in [1, 4] {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                // RECURSIVE is per thread; only a single worker takes one path.
                let modes: &[bool] = if threads == 1 {
                    &[false, true]
                } else {
                    &[false]
                };
                for &recursive in modes {
                    for turn in [false, true] {
                        for zero in [None, Some(Player::P0), Some(Player::P1)] {
                            for profiled in [false, true] {
                                check_dual(turn, zero, profiled, recursive);
                            }
                        }
                    }
                }
            });
    }
}
fn check_dual(turn: bool, zero: Option<Player>, profiled: bool, recursive: bool) {
    let (tree, evaluator) = fixture(turn);
    let ranges = PerPlayer::new(Player::P0, Player::P1).map(|p| {
        (0..evaluator.hands[p].len())
            .map(|h| {
                if zero == Some(p) || h % 5 == 0 {
                    0.0
                } else {
                    (h + 1) as f32 / 31.0
                }
            })
            .collect()
    });
    let game = CompiledGame {
        tree,
        evaluator,
        root_ranges: ranges,
        normalizer: 1.0,
        zero_sum: false,
    };
    let mut solver = Solver::<_, F32Storage>::new(game, Box::new(crate::Vanilla), None);
    if profiled {
        let profile = mixed_profile(&solver.game.tree);
        solver.set_profile(profile).unwrap();
    }
    let discounts = Discounts {
        pos: 1.0,
        neg: 1.0,
        avg: 1.0,
        floor_neg: false,
        reset_avg: false,
    };
    for &sref in &solver.game.tree.storage_refs {
        let weights: Vec<_> = (0..sref.len())
            .map(|i| if i % 3 == 0 { 0.0 } else { (i % 7 + 1) as f32 })
            .collect();
        solver
            .storage
            .accumulate_strategy(sref, sref.index, &weights, &discounts);
    }
    let game = &solver.game;
    let profile = solver.profile.as_ref();
    let par = ParConfig::default();
    RECURSIVE.set(recursive);
    let actual = super::super::dual::evaluate(game, &solver.storage, profile, par);
    for p in Player::BOTH {
        let ctx = ValueCtx {
            tree: &game.tree,
            evaluator: &game.evaluator,
            storage: &solver.storage,
            p,
            par,
            profile,
            incentives: true,
        };
        let mut ev = vec![0.0; game.root_ranges[p].len()];
        let mut br = ev.clone();
        value_pass::<_, _, _, true, true, false>(
            &ctx,
            &mut Scratch::new(),
            0,
            &game.root_ranges[p.opponent()],
            &mut ev,
            &mut br,
            &no_record,
            par.chance_depth,
        );
        let aggregate = |values: &[f32]| {
            game.root_ranges[p]
                .iter()
                .zip(values)
                .map(|(&r, &v)| r as f64 * v as f64)
                .sum::<f64>()
        };
        let case =
            format!("turn={turn} zero={zero:?} profiled={profiled} recursive={recursive} {p:?}");
        assert_eq!(actual[p].0.to_bits(), aggregate(&ev).to_bits(), "EV {case}");
        assert_eq!(actual[p].1.to_bits(), aggregate(&br).to_bits(), "BR {case}");
    }
    RECURSIVE.set(false);
}

#[test]
fn river_and_turn_values_and_updates_match_recursive_with_and_without_prune() {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            let mut maximum = 0.0f64;
            for turn in [false, true] {
                let (tree, e) = fixture(turn);
                for p in Player::BOTH {
                    for zero in [false, true] {
                        maximum = maximum.max(check::<true>(&tree, &e, p, zero, false));
                        maximum = maximum.max(check::<false>(&tree, &e, p, zero, false));
                    }
                }
            }
            println!("T25b traversal maximum relative per-node difference={maximum:e}");
        });
}

#[test]
fn storage_preorder_matches_recursive_bits_when_node_order_differs() {
    // Uniform-depth branches avoid the independent recursive optimization
    // that adds direct opponent terminals before nonterminal children.
    let (_, e) = fixture(false);
    let leaf = |a, b| {
        action(
            Player::P0,
            vec![
                TempNode::Terminal { id: a, tag: 0 },
                TempNode::Terminal { id: b, tag: 0 },
            ],
        )
    };
    let tree = PublicTree::compile(TreeSpec {
        root: action(
            Player::P0,
            vec![
                action(Player::P1, vec![leaf(0, 1), leaf(2, 3)]),
                action(Player::P1, vec![leaf(4, 5), leaf(6, 0)]),
            ],
        ),
        root_dims: e.hands.as_ref().map(|h| h.len() as u32),
        masks: vec![],
        transitions: vec![],
    });
    let id_order: Vec<_> = Subtree::new(&tree, 0)
        .walk()
        .filter(|&id| tree.node(id).kind == NodeKind::Action)
        .collect();
    assert_ne!(tree.storage_nodes, id_order);
    for (index, &id) in tree.storage_nodes.iter().enumerate() {
        assert_eq!(tree.node(id).aux as usize, index);
    }
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            for p in Player::BOTH {
                for zero in [false, true] {
                    check::<true>(&tree, &e, p, zero, true);
                    check::<false>(&tree, &e, p, zero, true);
                }
            }
        });
}

#[test]
fn profiled_sibling_batches_and_matches_recursive() {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            let make = || {
                let (tree, evaluator) = fixture(false);
                let child = tree.node(0).first_child;
                let locked = tree.storage_ref(tree.node(child));
                let mut locks = vec![None; tree.storage_refs.len()];
                let hands = locked.num_hands as usize;
                let mut fixed = vec![f32::NAN; locked.len()];
                fixed[..hands].fill(0.25);
                locks[locked.index as usize] = Some(crate::NodeLock {
                    fixed,
                    confidence: vec![1.0; hands],
                });
                let profile = crate::Profile {
                    incentives: vec![None; locks.len()],
                    locks,
                };
                let dims = tree.root_dims;
                let game = CompiledGame {
                    tree,
                    evaluator,
                    root_ranges: dims.map(|n| vec![1.0; n as usize]),
                    normalizer: 1.0,
                    zero_sum: true,
                };
                let mut solver = Solver::<_, F32Storage>::new(game, Box::new(crate::Vanilla), None);
                solver.set_cfr_precision(CfrPrecision::F32);
                solver.set_profile(profile).unwrap();
                solver
            };
            let mut reference = make();
            let mut actual = make();
            RECURSIVE.set(true);
            reference.run(3);
            RECURSIVE.set(false);
            actual.run(3);
            assert!(
                actual
                    .game
                    .evaluator
                    .batches
                    .load(std::sync::atomic::Ordering::Relaxed)
                    > 0
            );
            for node in 0..actual.game.tree.nodes.len() as u32 {
                if actual.game.tree.node(node).kind == NodeKind::Action {
                    assert!(
                        relative(
                            &reference.average_strategy_at(node),
                            &actual.average_strategy_at(node)
                        ) < 1e-5
                    );
                }
            }
            RECURSIVE.set(true);
            let recursive = actual.evaluate();
            RECURSIVE.set(false);
            let batched = actual.evaluate();
            for p in Player::BOTH {
                assert_eq!(recursive.0[p].to_bits(), batched.0[p].to_bits());
                assert_eq!(recursive.1[p].to_bits(), batched.1[p].to_bits());
            }
        });
}
