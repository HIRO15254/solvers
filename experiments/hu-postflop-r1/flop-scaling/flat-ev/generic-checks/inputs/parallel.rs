//! Chance and action fan-out determinism in synthetic engine-only games
//! (no dependency on `game`/`holdem`). Worker counts, mapped dimensions and
//! deep action splits must preserve full storage and value bit patterns.

use cards::{PerPlayer, Player};
use engine::{
    CompiledGame, Dcfr, F32Storage, I16Storage, ParConfig, PublicTree, ReachMap, Solver,
    SolverState, SparseTransition, Storage, StorageState, TempNode, TerminalEvaluator, TreeSpec,
};

/// Per-player private-state dimension: a small synthetic "hand space".
const DIM: usize = 8;
/// Root chance-node fan-out; comfortably over the default `min_children`
/// (12) so the default `ParConfig` actually parallelizes here.
const NUM_DEALS: usize = 16;

/// Fixed per-terminal `H*H` payoff matrices (action-major-free: just a
/// lookup table), the trivial `TerminalEvaluator` the spec calls for.
struct FixedEvaluator {
    /// `terminals[id][player]` is a flat `DIM*DIM` matrix, row-major by hero
    /// hand: entry `[h * DIM + o]`.
    terminals: Vec<PerPlayer<Vec<f32>>>,
}

impl TerminalEvaluator for FixedEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let matrix = &self.terminals[terminal as usize][p];
        debug_assert_eq!(opp_reach.len(), DIM);
        debug_assert_eq!(out.len(), DIM);
        for h in 0..DIM {
            let row = &matrix[h * DIM..(h + 1) * DIM];
            out[h] = row.iter().zip(opp_reach).map(|(&u, &r)| u * r).sum();
        }
    }
}

/// Deterministic pseudo-random-looking payoff, zero on the diagonal (a stand
/// in for card removal): varies with terminal id and both hands so the walk
/// actually exercises per-hand vector math instead of a constant payoff.
fn payoff_value(terminal: u32, h: usize, o: usize, sign: f32) -> f32 {
    if h == o {
        return 0.0;
    }
    let raw = ((terminal as u64 * 131 + h as u64 * 17 + o as u64 * 7) % 13) as f32 - 6.0;
    raw * sign
}

fn make_matrix(terminal: u32, sign: f32) -> Vec<f32> {
    let mut m = vec![0.0f32; DIM * DIM];
    for h in 0..DIM {
        for o in 0..DIM {
            m[h * DIM + o] = payoff_value(terminal, h, o, sign);
        }
    }
    m
}

fn make_terminal(next_terminal: &mut u32, terminals: &mut Vec<PerPlayer<Vec<f32>>>) -> TempNode {
    let id = *next_terminal;
    *next_terminal += 1;
    terminals.push(PerPlayer::new(make_matrix(id, 1.0), make_matrix(id, -1.0)));
    TempNode::Terminal { id, tag: 0 }
}

/// One deal's action subtree: 2 players alternating, 2-3 actions each,
/// ending in terminals. P0 picks one of 3 actions, then P1 picks one of 2.
fn action_subtree(next_terminal: &mut u32, terminals: &mut Vec<PerPlayer<Vec<f32>>>) -> TempNode {
    let p0_children = (0..3)
        .map(|_| {
            let p1_children = (0..2)
                .map(|_| make_terminal(next_terminal, terminals))
                .collect();
            TempNode::Action {
                player: Player::P1,
                children: p1_children,
                tag: 0,
            }
        })
        .collect();
    TempNode::Action {
        player: Player::P0,
        children: p0_children,
        tag: 0,
    }
}

/// Builds the synthetic game: a root chance node with `NUM_DEALS` deals
/// (each masking out one hand index, a stand-in for a dealt card), each
/// leading into its own small action subtree.
fn build_game() -> CompiledGame<FixedEvaluator> {
    let mut terminals = Vec::new();
    let mut next_terminal = 0u32;
    let mut masks = Vec::new();
    let deals = (0..NUM_DEALS)
        .map(|i| {
            let mut mask = vec![1.0f32; DIM];
            mask[i % DIM] = 0.0;
            masks.push(mask);
            let mask_id = (masks.len() - 1) as u32;
            let maps = PerPlayer::new(ReachMap::Mask(mask_id), ReachMap::Mask(mask_id));
            let weight = 1.0 / NUM_DEALS as f32;
            let child = action_subtree(&mut next_terminal, &mut terminals);
            (weight, maps, child)
        })
        .collect();
    let root = TempNode::Chance { deals, tag: 0 };
    let tree = PublicTree::compile(TreeSpec {
        root,
        masks,
        transitions: Vec::new(),
        root_dims: PerPlayer::new(DIM as u32, DIM as u32),
    });
    CompiledGame {
        tree,
        evaluator: FixedEvaluator { terminals },
        root_ranges: PerPlayer::new(vec![1.0; DIM], vec![1.0; DIM]),
        normalizer: (DIM * (DIM - 1)) as f64,
        // Arbitrary fixed payoffs, no zero-sum guarantee: keep the full
        // general-sum accounting.
        zero_sum: false,
    }
}

fn solve(par: ParConfig, iters: u64) -> Solver<FixedEvaluator, F32Storage> {
    let game = build_game();
    let mut solver = Solver::<_, F32Storage>::new(game, Box::<Dcfr>::default(), Some(iters));
    solver.set_par(par);
    solver.run(iters);
    solver
}

fn bit_pattern(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn selected_value_recording_preserves_ancestor_values_and_storage() {
    for par in [
        ParConfig::default(),
        ParConfig {
            chance_depth: 0,
            min_children: usize::MAX,
        },
    ] {
        let solver = solve(par, 3);
        let tree = &solver.game().tree;
        let before = solver.storage().snapshot();
        // Retain selected upper action nodes while skipping their descendants:
        // recording must not prune the value computation beneath them.
        let include = |id| tree.storage_ref(tree.node(id)).index.is_multiple_of(3);
        for player in Player::BOTH {
            let all = solver.expected_values_everywhere(player);
            let selected = solver.expected_values_where(player, include);
            let none = solver.expected_values_where(player, |_| false);
            assert!(none.iter().all(Option::is_none));
            let mut kept = 0;
            let mut dropped = 0;
            for id in 0..tree.nodes.len() as engine::NodeId {
                let node = tree.node(id);
                if node.kind != engine::NodeKind::Action {
                    continue;
                }
                let index = node.aux as usize;
                if include(id) {
                    kept += 1;
                    assert_eq!(
                        bit_pattern(selected[index].as_ref().unwrap()),
                        bit_pattern(all[index].as_ref().unwrap()),
                    );
                } else {
                    dropped += 1;
                    assert!(selected[index].is_none());
                }
            }
            assert!(kept > 0 && dropped > 0);
        }
        assert_eq!(solver.storage().snapshot(), before);
    }
}

#[test]
fn parallel_chance_fanout_is_bitwise_deterministic() {
    // Root has 16 >= min_children(12) deals, chance_depth 2: this
    // parallelizes at the root.
    let par_on = ParConfig {
        chance_depth: 2,
        min_children: 12,
    };
    // chance_depth 0 alone already forces sequential everywhere;
    // min_children(usize::MAX) is redundant belt-and-suspenders.
    let par_off = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };

    let solver_par = solve(par_on, 50);
    let solver_seq = solve(par_off, 50);

    let (regrets_par, strategy_par) = solver_par.storage().snapshot();
    let (regrets_seq, strategy_seq) = solver_seq.storage().snapshot();

    assert_eq!(regrets_par.len(), regrets_seq.len());
    assert_eq!(strategy_par.len(), strategy_seq.len());
    assert_eq!(
        bit_pattern(&regrets_par),
        bit_pattern(&regrets_seq),
        "regret bit patterns diverged between parallel and sequential passes"
    );
    assert_eq!(
        bit_pattern(&strategy_par),
        bit_pattern(&strategy_seq),
        "strategy-sum bit patterns diverged between parallel and sequential passes"
    );

    for p in Player::BOTH {
        assert_eq!(
            solver_par.expected_value(p).to_bits(),
            solver_seq.expected_value(p).to_bits(),
            "expected_value diverged for {p:?}"
        );
    }

    let expl_par = solver_par.exploitability();
    let expl_seq = solver_seq.exploitability();
    for p in Player::BOTH {
        assert_eq!(
            expl_par[p].to_bits(),
            expl_seq[p].to_bits(),
            "exploitability diverged for {p:?}"
        );
    }
}

// A linear-time evaluator keeps the scheduling regression fixtures small
// enough for ordinary tests. Payoffs vary by terminal, seat and both hand
// classes; the two seats are deliberately not zero-sum shortcuts.
struct DimensionEvaluator {
    terminal_dims: Vec<PerPlayer<usize>>,
}

impl TerminalEvaluator for DimensionEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let dims = &self.terminal_dims[terminal as usize];
        assert_eq!(out.len(), dims[p]);
        assert_eq!(opp_reach.len(), dims[p.opponent()]);
        let mut mass = [0.0f32; 3];
        for (o, &reach) in opp_reach.iter().enumerate() {
            mass[o % 3] += reach;
        }
        let seat = if p == Player::P0 { 2 } else { 7 };
        for (h, value) in out.iter_mut().enumerate() {
            *value = 0.0;
            for (class, &reach) in mass.iter().enumerate() {
                let payoff =
                    ((terminal as usize * 11 + h * 3 + class * 5 + seat) % 19) as f32 - 9.0;
                *value += reach * payoff * 0.125;
            }
        }
    }
}

fn dimension_terminal(dims: PerPlayer<usize>, terminals: &mut Vec<PerPlayer<usize>>) -> TempNode {
    let id = terminals.len() as u32;
    terminals.push(dims);
    TempNode::Terminal { id, tag: 0 }
}

fn branching_actions(
    levels: usize,
    player: Player,
    dims: PerPlayer<usize>,
    terminals: &mut Vec<PerPlayer<usize>>,
) -> TempNode {
    if levels == 0 {
        return dimension_terminal(dims, terminals);
    }
    TempNode::Action {
        player,
        children: (0..3)
            .map(|_| branching_actions(levels - 1, player.opponent(), dims, terminals))
            .collect(),
        tag: 0,
    }
}

fn dimension_game(
    root: TempNode,
    dims: PerPlayer<usize>,
    terminal_dims: Vec<PerPlayer<usize>>,
    transitions: Vec<SparseTransition>,
) -> CompiledGame<DimensionEvaluator> {
    let ranges: PerPlayer<Vec<f32>> = PerPlayer::new(
        (0..dims[Player::P0])
            .map(|h| (h % 7 + 1) as f32 * 0.125)
            .collect(),
        (0..dims[Player::P1])
            .map(|h| (h % 5 + 1) as f32 * 0.25)
            .collect(),
    );
    let normalizer = ranges[Player::P0]
        .iter()
        .map(|&r| f64::from(r))
        .sum::<f64>()
        * ranges[Player::P1]
            .iter()
            .map(|&r| f64::from(r))
            .sum::<f64>();
    CompiledGame {
        tree: PublicTree::compile(TreeSpec {
            root,
            masks: vec![],
            transitions,
            root_dims: PerPlayer::new(dims[Player::P0] as u32, dims[Player::P1] as u32),
        }),
        evaluator: DimensionEvaluator { terminal_dims },
        root_ranges: ranges,
        normalizer,
        zero_sum: false,
    }
}

fn action_only_game(large: bool, single_parent: bool) -> CompiledGame<DimensionEvaluator> {
    let dims = if large {
        PerPlayer::new(256, 192)
    } else {
        PerPlayer::new(8, 5)
    };
    let mut terminals = Vec::new();
    let subtree = branching_actions(if large { 6 } else { 2 }, Player::P0, dims, &mut terminals);
    let root = if single_parent {
        // This parent cannot fan out itself. Its own storage must survive a
        // descendant split; its terminal sibling also exercises an empty span.
        TempNode::Action {
            player: Player::P1,
            children: vec![subtree, dimension_terminal(dims, &mut terminals)],
            tag: 0,
        }
    } else {
        subtree
    };
    dimension_game(root, dims, terminals, vec![])
}

fn deep_action_spine_game() -> CompiledGame<DimensionEvaluator> {
    let dims = PerPlayer::new(256, 192);
    let mut terminals = Vec::new();
    let mut root = branching_actions(6, Player::P0, dims, &mut terminals);
    // Five alternating own/opponent ancestors cannot fan out themselves:
    // each has only one nonempty storage subtree. Alternate the empty-span
    // terminal's position so a deep split must preserve both the ancestor's
    // own ref/scales and a sibling visited after the large branch.
    for level in 0..5 {
        let terminal = dimension_terminal(dims, &mut terminals);
        let (player, children) = if level % 2 == 0 {
            (Player::P1, vec![root, terminal])
        } else {
            (Player::P0, vec![terminal, root])
        };
        root = TempNode::Action {
            player,
            children,
            tag: 0,
        };
    }
    let game = dimension_game(root, dims, terminals, vec![]);
    let tree = &game.tree;
    assert!(!tree.subtree_has_chance[0]);
    let elements = |id: engine::NodeId| {
        let span = tree.storage_spans[id as usize];
        span.end - span.start
    };
    let mut fork = 0;
    for _ in 0..5 {
        let children: Vec<_> = tree.children(fork).collect();
        assert_eq!(children.len(), 2);
        let substantial: Vec<_> = children
            .iter()
            .copied()
            .filter(|&child| elements(child) > 0)
            .collect();
        assert_eq!(substantial.len(), 1);
        fork = substantial[0];
    }
    // Each of the three branches at action depth five exceeds even the
    // maximum work grain, so every multi-worker pool must reach a planned
    // fork beyond the former depth-two cap. Solver unit tests separately
    // check plan selection; this test does not rely on worker timing.
    assert_eq!(tree.children(fork).count(), 3);
    assert!(tree.children(fork).all(|child| elements(child) >= 65_536));
    game
}

fn mapped_chance_game(outer_deals: usize) -> CompiledGame<DimensionEvaluator> {
    let root_dims = PerPlayer::new(4, 3);
    let mut terminals = Vec::new();
    let mut transitions = Vec::new();
    let mut root_children = Vec::new();
    for sibling in 0..3 {
        let mut deals = Vec::new();
        for deal in 0..outer_deals {
            let dims = PerPlayer::new(2 + deal % 5, 2 + (deal + sibling) % 4);
            let mut maps = PerPlayer::new(ReachMap::Identity, ReachMap::Identity);
            for player in Player::BOTH {
                let id = transitions.len() as u32;
                transitions.push(SparseTransition {
                    in_dim: root_dims[player] as u32,
                    out_dim: dims[player] as u32,
                    entries: (0..root_dims[player])
                        .flat_map(|i| {
                            [
                                (i as u32, (i % dims[player]) as u32, 0.5),
                                (i as u32, ((i + 1) % dims[player]) as u32, 0.5),
                            ]
                        })
                        .collect(),
                });
                maps[player] = ReachMap::Transition(id);
            }
            let nested = TempNode::Chance {
                deals: (0..16)
                    .map(|_| {
                        (
                            1.0 / 16.0,
                            PerPlayer::new(ReachMap::Identity, ReachMap::Identity),
                            branching_actions(1, Player::P0, dims, &mut terminals),
                        )
                    })
                    .collect(),
                tag: 0,
            };
            let child = TempNode::Action {
                player: Player::P1,
                children: vec![dimension_terminal(dims, &mut terminals), nested],
                tag: 0,
            };
            deals.push((1.0 / outer_deals as f32, maps, child));
        }
        root_children.push(TempNode::Chance { deals, tag: 0 });
    }
    let root = TempNode::Action {
        player: Player::P0,
        children: root_children,
        tag: 0,
    };
    dimension_game(root, root_dims, terminals, transitions)
}

#[derive(Debug, PartialEq, Eq)]
struct StateBits {
    iteration: u64,
    arrays: Vec<Vec<u32>>,
}

fn state_bits(state: &SolverState) -> StateBits {
    let arrays = match &state.storage {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => vec![bit_pattern(regrets), bit_pattern(strategy_sum)],
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => vec![
            regrets.iter().map(|&x| x as u16 as u32).collect(),
            strategy_sum.iter().map(|&x| x as u16 as u32).collect(),
            bit_pattern(regret_scales),
            bit_pattern(strategy_scales),
        ],
    };
    StateBits {
        iteration: state.iteration,
        arrays,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct WalkObservation {
    state: StateBits,
    ev: Vec<u64>,
    br: Vec<u64>,
    gains: Vec<u64>,
    nodes: Vec<Vec<Option<Vec<u32>>>>,
}

fn observe<S: Storage>(
    build: impl FnOnce() -> CompiledGame<DimensionEvaluator> + Send,
    threads: usize,
    par: ParConfig,
) -> WalkObservation {
    // Scheduling must use the pool running the walk, even when construction
    // occurs outside that pool.
    let mut solver = Solver::<_, S>::new(build(), Box::<Dcfr>::default(), Some(4));
    solver.set_par(par);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            assert_eq!(rayon::current_num_threads(), threads);
            solver.run(4);
            let before = state_bits(&solver.state());
            let mut ev = Vec::new();
            let mut br = Vec::new();
            let mut nodes = Vec::new();
            for player in Player::BOTH {
                ev.push(solver.expected_value(player).to_bits());
                br.push(solver.best_response_value(player).to_bits());
                let all = solver.expected_values_everywhere(player);
                let tree = &solver.game().tree;
                let selected = solver.expected_values_where(player, |id| {
                    tree.storage_ref(tree.node(id)).index.is_multiple_of(3)
                });
                assert!(
                    solver
                        .expected_values_where(player, |_| false)
                        .iter()
                        .all(Option::is_none)
                );
                for (index, (full, selected)) in all.iter().zip(&selected).enumerate() {
                    if index.is_multiple_of(3) {
                        assert_eq!(
                            full.as_deref().map(bit_pattern),
                            selected.as_deref().map(bit_pattern)
                        );
                    } else {
                        assert!(selected.is_none());
                    }
                }
                nodes.push(all.iter().map(|v| v.as_deref().map(bit_pattern)).collect());
                // The root query uses a separate value-pass entry point.
                let reaches = PerPlayer::new(
                    solver.game().root_ranges[Player::P0].as_slice(),
                    solver.game().root_ranges[Player::P1].as_slice(),
                );
                let root = solver.expected_values_at(0, player, reaches);
                assert_eq!(
                    bit_pattern(&root),
                    bit_pattern(all[tree.node(0).aux as usize].as_ref().unwrap())
                );
                let root_br = solver.best_response_values_at(0, player, reaches);
                let aggregate: f64 = root_br
                    .iter()
                    .zip(&solver.game().root_ranges[player])
                    .map(|(&value, &reach)| value as f64 * reach as f64)
                    .sum();
                assert_eq!(
                    (aggregate / solver.game().normalizer).to_bits(),
                    br[player as usize]
                );
            }
            let gains = solver
                .exploitability()
                .0
                .into_iter()
                .map(f64::to_bits)
                .collect();
            assert_eq!(
                before,
                state_bits(&solver.state()),
                "read-only evaluation changed solver state"
            );
            WalkObservation {
                state: before,
                ev,
                br,
                gains,
                nodes,
            }
        })
}

fn compare_action_pools<S: Storage>() {
    for (large, single_parent) in [(false, false), (true, false), (true, true)] {
        // Chance knobs do not control action scheduling. Both extremes must
        // retain identical arithmetic in a game with no chance nodes.
        let baseline = observe::<S>(
            || action_only_game(large, single_parent),
            1,
            ParConfig::default(),
        );
        for threads in [1, 2, 4] {
            for chance_depth in [0, 2] {
                let actual = observe::<S>(
                    || action_only_game(large, single_parent),
                    threads,
                    ParConfig {
                        chance_depth,
                        min_children: 12,
                    },
                );
                assert_eq!(
                    actual, baseline,
                    "threads={threads}, large={large}, single_parent={single_parent}"
                );
            }
        }
    }
}

#[test]
fn action_pool_sizes_preserve_f32_state_and_all_values() {
    compare_action_pools::<F32Storage>();
}

#[test]
fn action_pool_sizes_preserve_i16_state_and_all_values() {
    compare_action_pools::<I16Storage>();
}

fn compare_deep_action_pools<S: Storage>() {
    // Disabling chance fan-out must not disable the independent action plan.
    let par = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };
    let baseline = observe::<S>(deep_action_spine_game, 1, par);
    for threads in [2, 4, 8, 16, 32] {
        let actual = observe::<S>(deep_action_spine_game, threads, par);
        assert_eq!(
            actual, baseline,
            "deep single-large-child spine, threads={threads}"
        );
    }
}

#[test]
fn deep_action_spine_preserves_f32_state_and_all_values() {
    compare_deep_action_pools::<F32Storage>();
}

#[test]
fn deep_action_spine_preserves_i16_state_and_all_values() {
    compare_deep_action_pools::<I16Storage>();
}

fn compare_chance_pools<S: Storage>() {
    // 2-way outer chance is below the threshold but still consumes depth.
    // 16-way outer chance exercises nested parallelism and mapped dimensions.
    for outer_deals in [2, 16] {
        let baseline = observe::<S>(|| mapped_chance_game(outer_deals), 1, ParConfig::default());
        for threads in [1, 2, 4] {
            for chance_depth in [0, 1, 2] {
                let actual = observe::<S>(
                    || mapped_chance_game(outer_deals),
                    threads,
                    ParConfig {
                        chance_depth,
                        min_children: 12,
                    },
                );
                assert_eq!(
                    actual, baseline,
                    "threads={threads}, outer_deals={outer_deals}, depth={chance_depth}"
                );
            }
        }
    }
}

#[test]
fn mapped_chance_and_action_siblings_preserve_f32_state() {
    compare_chance_pools::<F32Storage>();
}

#[test]
fn mapped_chance_and_action_siblings_preserve_i16_state() {
    compare_chance_pools::<I16Storage>();
}
