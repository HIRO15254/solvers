//! Viewer/river-resolve tests: street tagging, history replay against a
//! trunk config, and — the critical guard test — that a trunk's river
//! subtree is structurally identical to a from-scratch river-start build at
//! every river-entry node. None of these are `#[ignore]`d: the guard test
//! in particular must stay fast enough to run on every `cargo test`, since
//! it is the only thing standing between the postflop builder's
//! betting/sizing logic drifting silently out of sync with
//! `holdem::viewer`'s replay/reconstruction logic.

use cards::{Card, Chips, NUM_COMBOS, PerPlayer, Player, Range, Street};
use engine::{Dcfr, F32Storage, NodeId, NodeKind, ParConfig, PublicTree, Solver};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{
    PerStreet, PostflopConfig, build_postflop_game, node_streets, river_entry_state,
    river_resolve_config,
};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn parse_cards(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

fn parse_board5(s: &str) -> [Card; 5] {
    parse_cards(s).try_into().expect("5-card board string")
}

/// No chance-node parallelism, matching `rake_icm.rs`'s helper of the same
/// name: keeps float reduction order (and therefore bit patterns) fixed
/// across runs.
fn sequential() -> ParConfig {
    ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    }
}

/// Small turn-start config, copied locally from
/// `crates/holdem/tests/rake_icm.rs`'s `small_turn_config` (board "2s 7s Ks
/// 2h", ranges "44,55" vs "33,66", pot 2, stack 20, turn 0.75 / river 1.0,
/// max_raises 1/1): a single chance node (turn -> river) whose children are
/// exactly the river-entry nodes the guard test needs, and small enough to
/// build/replay/re-solve fast.
fn small_turn_config() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("2s 7s Ks 2h"),
        ranges: PerPlayer::new(
            "44,55".parse::<Range>().unwrap(),
            "33,66".parse::<Range>().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![1.0], vec![1.0]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 1,
            river: 1,
        },
        ..Default::default()
    }
}

/// Flop-start variant exercising two chance levels (flop -> turn -> river)
/// with a bet/call on *each* street, so the contribution-accumulation math
/// in `river_entry_state` is tested across more than one street. Single-
/// combo ranges (`AhAd`/`QhQd`, disjoint from the board and from each
/// other) keep the tree tiny and the test fast even though `iso_merging`
/// defaults on.
fn small_flop_config() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("2s 7s Ks"),
        ranges: PerPlayer::new(
            "AhAd".parse::<Range>().unwrap(),
            "QhQd".parse::<Range>().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![0.5], vec![0.5]),
            turn: PerPlayer::new(vec![0.5], vec![0.5]),
            river: PerPlayer::new(vec![1.0], vec![1.0]),
        },
        max_raises: PerStreet {
            flop: 1,
            turn: 1,
            river: 1,
        },
        ..Default::default()
    }
}

// --- node_streets ---------------------------------------------------

#[test]
fn node_streets_tags_turn_start_board_correctly() {
    let config = small_turn_config();
    let trunk = build_postflop_game(&config, chip_ev());
    let streets = node_streets(&trunk.game.tree, Street::Turn);

    assert_eq!(
        streets[0],
        Street::Turn,
        "root keeps the caller-supplied start street"
    );

    let mut checked_any = false;
    for (id, node) in trunk.game.tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Chance {
            for child in trunk.game.tree.children(id as NodeId) {
                let child_node = trunk.game.tree.node(child);
                assert_eq!(
                    child_node.kind,
                    NodeKind::Action,
                    "this fixture never goes all-in before the river, so every \
                     river deal must lead into an action node"
                );
                assert_eq!(streets[child as usize], Street::River);
                checked_any = true;
            }
        }
    }
    assert!(
        checked_any,
        "expected at least one chance node in the turn-start fixture"
    );
}

#[test]
fn node_streets_layers_flop_turn_river_on_multi_street_board() {
    let config = small_flop_config();
    let trunk = build_postflop_game(&config, chip_ev());
    let streets = node_streets(&trunk.game.tree, Street::Flop);

    assert_eq!(streets[0], Street::Flop);

    let mut seen = [false; 3]; // [Flop, Turn, River]
    for (id, node) in trunk.game.tree.nodes.iter().enumerate() {
        let street = streets[id];
        match street {
            Street::Flop => seen[0] = true,
            Street::Turn => seen[1] = true,
            Street::River => seen[2] = true,
            Street::Preflop => panic!("a postflop tree must never tag a node Preflop"),
        }
        if node.kind == NodeKind::Chance {
            let expected_child_street = match street {
                Street::Flop => Street::Turn,
                Street::Turn => Street::River,
                other => panic!("no chance node should appear at street {other:?}"),
            };
            for child in trunk.game.tree.children(id as NodeId) {
                assert_eq!(streets[child as usize], expected_child_street);
            }
        }
    }
    assert!(
        seen[0] && seen[1] && seen[2],
        "expected all three streets present in a flop-start tree: {seen:?}"
    );
}

// --- river_entry_state replay -----------------------------------------

#[test]
fn river_entry_state_check_check_turn_line() {
    let config = small_turn_config();
    // Two checks (P0 checks, P1 checks back) end the turn with no chips
    // added: contrib stays (0, 0). The chance token deals the river card
    // "9d" (unused by the board "2s 7s Ks 2h" or by either range).
    // c = 0, so pot' = trunk.pot(2) + 2*0 = 2, eff' = trunk.stack(20) - 0 = 20.
    let state = river_entry_state(&config, "xx[9d]").expect("valid check-check-deal history");
    assert_eq!(state.board, parse_board5("2s 7s Ks 2h 9d"));
    assert_eq!(state.pot, Chips(2));
    assert_eq!(state.effective_stack, Chips(20));
}

#[test]
fn river_entry_state_bet_call_turn_line() {
    let config = small_turn_config();
    // P0 bets: pot_now = config.pot(2) + contrib(0) + contrib(0) = 2,
    // pot_after_call = pot_now + outstanding(0) = 2, raw = round(0.75*2) =
    // round(1.5) = 2, extra = min(max(2,1), behind(20)-outstanding(0)) = 2,
    // additional = outstanding(0) + extra(2) = 2 => contrib[P0] = 0+2 = 2,
    // history token "b2".
    // P1 calls: contrib[P1] = contrib[P0] = 2, token "c".
    // Chance deals "2c" (distinct suit from the board's existing "2s"/"2h").
    // c = 2, so pot' = 2 + 2*2 = 6, eff' = 20 - 2 = 18.
    let state = river_entry_state(&config, "b2c[2c]").expect("valid bet-call-deal history");
    assert_eq!(state.board, parse_board5("2s 7s Ks 2h 2c"));
    assert_eq!(state.pot, Chips(6));
    assert_eq!(state.effective_stack, Chips(18));
}

#[test]
fn river_entry_state_accumulates_contribution_across_two_streets() {
    let config = small_flop_config();
    // Flop: P0 bets. pot_now = 2+0+0=2, pot_after_call = 2+0=2,
    // raw = round(0.5*2) = 1, extra = min(max(1,1), 20-0-0=20) = 1,
    // additional = 0+1 = 1 => contrib[P0] = 1, token "b1".
    // P1 calls: contrib[P1] = 1, token "c". Chance deals turn card "2c"
    // (board becomes 2s 7s Ks 2c).
    //
    // Turn: pot_now = 2+1+1=4, pot_after_call = 4+0=4,
    // raw = round(0.5*4) = 2, extra = min(max(2,1), (20-1)-0=19) = 2,
    // additional = 0+2 = 2 => contrib[P0] = 1+2 = 3, token "b3".
    // P1 calls: contrib[P1] = 3, token "c". Chance deals river card "9d"
    // (board becomes 2s 7s Ks 2c 9d).
    //
    // c = 3 chips, accumulated across both streets' bets (1 then +2), so
    // pot' = trunk.pot(2) + 2*3 = 8, eff' = trunk.stack(20) - 3 = 17.
    let state = river_entry_state(&config, "b1c[2c]b3c[9d]")
        .expect("valid two-street bet-call-deal history");
    assert_eq!(state.board, parse_board5("2s 7s Ks 2c 9d"));
    assert_eq!(state.pot, Chips(8));
    assert_eq!(state.effective_stack, Chips(17));
}

#[test]
fn river_entry_state_rejects_a_fold_and_a_short_board() {
    let config = small_turn_config();
    assert!(matches!(
        river_entry_state(&config, "f"),
        Err(holdem::ReplayError::FoldedBeforeRiver)
    ));
    // Consumes no chance token at all: board stays at 4 cards.
    assert!(matches!(
        river_entry_state(&config, "xx"),
        Err(holdem::ReplayError::IncompleteBoard { cards: 4 })
    ));
}

// --- THE GUARD TEST -----------------------------------------------------

/// Every node id whose own street is River and whose parent is a Chance
/// node — i.e. every river-entry `Action` node in `tree`.
fn river_entry_action_nodes(
    tree: &PublicTree,
    streets: &[Street],
    parents: &[NodeId],
) -> Vec<NodeId> {
    (0..tree.nodes.len() as NodeId)
        .filter(|&id| {
            tree.node(id).kind == NodeKind::Action
                && streets[id as usize] == Street::River
                && parents[id as usize] != NodeId::MAX
                && tree.node(parents[id as usize]).kind == NodeKind::Chance
        })
        .collect()
}

/// The critical guard test: for a real trunk (both without and with
/// suit-isomorphism merging), every river-entry node's subtree must be
/// structurally identical (per `engine::pair_subtrees`) to a from-scratch
/// river-start build seeded via `river_entry_state` + `river_resolve_config`
/// from that node's own history. This is exactly the design contract
/// documented on `holdem::viewer`'s module doc, checked against the real
/// builder rather than merely asserted in prose.
#[test]
fn river_trunk_matches_fresh_river_resolve_at_every_entry_node() {
    for iso_merging in [false, true] {
        let mut config = small_turn_config();
        config.iso_merging = iso_merging;
        let trunk = build_postflop_game(&config, chip_ev());

        let streets = node_streets(&trunk.game.tree, Street::Turn);
        let parents = engine::parent_array(&trunk.game.tree);
        let entries = river_entry_action_nodes(&trunk.game.tree, &streets, &parents);
        assert!(
            !entries.is_empty(),
            "expected at least one river-entry node (iso_merging={iso_merging})"
        );

        // Full (all-1.0) reach: `river_resolve_config` still zeroes
        // board-conflicting combos internally via `build_postflop_game`'s
        // own range_vec construction, so this is a valid "everyone always
        // reaches" range for the structural-equivalence check.
        let full_reach = PerPlayer::new(vec![1.0f32; NUM_COMBOS], vec![1.0f32; NUM_COMBOS]);

        for id in entries {
            let tag = trunk.game.tree.tags[id as usize];
            let history = trunk.node_info[tag as usize].history.clone();

            let state = river_entry_state(&config, &history)
                .unwrap_or_else(|e| panic!("replay failed for history {history:?}: {e}"));
            let sub_config = river_resolve_config(&config, &state, &full_reach);
            let sub = build_postflop_game(&sub_config, chip_ev());

            engine::pair_subtrees(&trunk.game.tree, id, &sub.game.tree, 0).unwrap_or_else(|e| {
                panic!(
                    "river-entry subtree mismatch for history {history:?} \
                     (iso_merging={iso_merging}): {e}"
                )
            });
        }
    }
}

// --- reach consistency at the holdem level -----------------------------

/// Same turn-start board/ranges/turn-bet-menu as `small_turn_config`, but
/// with the river bet menu emptied out (`max_raises.river = 0`, no river
/// fractions). `small_turn_config` itself is deliberately *not* reused for
/// an actual CFR solve here: `rake_icm.rs`'s own
/// `pure_hu_icm_postflop_solve_matches_chip_ev` marks even a 64-iteration
/// solve of that exact fixture `#[ignore]` as "slow unoptimized", and this
/// test must stay un-ignored. The turn's single chance node (~44-48 river
/// cards) is the dominant per-iteration cost regardless of what happens
/// after it, so trimming the river's own action tree to just a showdown
/// (no additional bet/raise terminals) is enough to keep a handful of
/// iterations debug-fast while leaving the root's actual two actions
/// (check vs. bet) intact — this test needs *some* strategic mixing at the
/// root for the reach multiplication to be a meaningful check, not just an
/// identity.
fn fast_turn_config_for_solve() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("2s 7s Ks 2h"),
        ranges: PerPlayer::new(
            "44,55".parse::<Range>().unwrap(),
            "33,66".parse::<Range>().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![], vec![]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 1,
            river: 0,
        },
        ..Default::default()
    }
}

#[test]
fn reach_at_matches_root_avg_strategy_column_after_a_short_solve() {
    let config = fast_turn_config_for_solve();
    let trunk = build_postflop_game(&config, chip_ev());
    let iterations = 4;
    let mut solver =
        Solver::<_, F32Storage>::new(trunk.game, Box::<Dcfr>::default(), Some(iterations));
    solver.set_par(sequential());
    solver.run(iterations);

    let tree = &solver.game().tree;
    let root = *tree.node(0);
    let target = root.first_child; // action index 0's child

    let root_ranges = &solver.game().root_ranges;
    let root_slices = PerPlayer::new(
        root_ranges[Player::P0].as_slice(),
        root_ranges[Player::P1].as_slice(),
    );

    let reach = engine::reach_at(tree, root_slices, target, |id, _sref, out: &mut [f32]| {
        out.copy_from_slice(&solver.average_strategy_at(id));
    });

    let root_sigma = solver.average_strategy_at(0);
    let column = &root_sigma[0..NUM_COMBOS]; // action 0's column, action-major

    let acting = root.player;
    let expected: Vec<f32> = root_ranges[acting]
        .iter()
        .zip(column)
        .map(|(&r, &s)| r * s)
        .collect();
    assert_eq!(reach[acting], expected);

    // The opponent is never touched at an action node.
    let opponent = acting.opponent();
    assert_eq!(reach[opponent], root_ranges[opponent]);
}
