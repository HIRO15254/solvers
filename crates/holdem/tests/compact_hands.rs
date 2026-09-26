//! Exact compact-vs-dense regression checks on small postflop games.
//! The dense layout is a differential reference, not a second solver oracle.

use cards::{Card, CardSet, Chips, NUM_COMBOS, PerPlayer, Player, Range, combo_cards, combo_index};
use engine::{
    CompiledGame, Dcfr, F32Storage, NodeKind, ParConfig, ReachMap, Solver, StorageStateRef,
};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{
    PerStreet, PostflopConfig, PostflopEvaluator, PostflopGame, PostflopHands, StreetTree,
    build_postflop_game, build_postflop_game_dense,
};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn cards(text: &str) -> Vec<Card> {
    text.split_whitespace()
        .map(|card| card.parse().unwrap())
        .collect()
}

fn combo(text: &str) -> usize {
    combo_index(text[..2].parse().unwrap(), text[2..].parse().unwrap())
}

fn weighted(hands: &[(&str, f32)]) -> Range {
    let mut range = Range::default();
    for &(hand, weight) in hands {
        range.set_weight(combo(hand), weight);
    }
    range
}

fn class_weights(classes: &[(&str, f32)]) -> Range {
    let mut range = Range::default();
    for &(class, weight) in classes {
        let support: Range = class.parse().unwrap();
        for hand in 0..NUM_COMBOS {
            if support.weight(hand) > 0.0 {
                range.set_weight(hand, weight);
            }
        }
    }
    range
}

fn river_config() -> PostflopConfig {
    PostflopConfig {
        board: cards("2c 7d 9h Js Qs"),
        ranges: PerPlayer::new(
            weighted(&[
                ("AsAh", 1.0),
                ("AcAd", 0.125),
                ("KsKh", 0.5),
                ("TcTd", f32::MIN_POSITIVE),
                ("2c3c", 0.75),
            ]),
            weighted(&[
                ("AsAh", 0.25),
                ("AhKh", 0.5),
                ("KdKc", 0.125),
                ("7d8d", 0.25),
            ]),
        ),
        pot: Chips(4),
        effective_stack: Chips(6),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[], &[], 0),
            river: StreetTree::pot_fractions(&[0.5], &[1.0], 1),
        },
        ..Default::default()
    }
}

fn turn_config(iso_merging: bool) -> PostflopConfig {
    PostflopConfig {
        board: cards("2s 7s Ks 2h"),
        // c/d suit exchange preserves both ranges even though the seats
        // have different dimensions and each seat has nonuniform weights.
        ranges: PerPlayer::new(
            class_weights(&[("44", 0.25), ("55", 0.75)]),
            class_weights(&[("33", 0.5), ("66", 0.125), ("AA", 1.0)]),
        ),
        pot: Chips(4),
        effective_stack: Chips(6),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[0.5], &[0.5], 1),
            river: StreetTree::pot_fractions(&[], &[], 0),
        },
        iso_merging,
        ..Default::default()
    }
}

fn assert_root_mapping(config: &PostflopConfig, compact: &PostflopGame, dense: &PostflopGame) {
    let hands = compact.game.evaluator.hands();
    let board: CardSet = config.board.iter().copied().collect();
    for player in Player::BOTH {
        let expected: Vec<_> = (0..NUM_COMBOS)
            .filter(|&hand| {
                let (a, b) = combo_cards(hand);
                config.ranges[player].weight(hand) > 0.0 && !board.contains(a) && !board.contains(b)
            })
            .map(|hand| hand as u16)
            .collect();
        assert_eq!(hands.combos(player), expected);
        assert_eq!(compact.game.tree.root_dims[player] as usize, expected.len());
        assert_eq!(dense.game.tree.root_dims[player] as usize, NUM_COMBOS);
        for (local, &hand) in expected.iter().enumerate() {
            assert_eq!(hands.index(player, hand as usize), Some(local));
            assert_eq!(
                compact.game.root_ranges[player][local].to_bits(),
                config.ranges[player].weight(hand as usize).to_bits()
            );
        }
        let expanded = hands.expand(player, &compact.game.root_ranges[player]);
        assert_eq!(
            expanded
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            dense.game.root_ranges[player]
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }
    // Independent direct compatible-pair integration. Fixture weights are
    // dyadic, so regrouping these small sums is exact in f64.
    let mut compatible_weight = 0.0;
    for (own, &h) in hands.combos(Player::P0).iter().enumerate() {
        let (h1, h2) = combo_cards(h as usize);
        for (opp, &o) in hands.combos(Player::P1).iter().enumerate() {
            let (o1, o2) = combo_cards(o as usize);
            if h1 != o1 && h1 != o2 && h2 != o1 && h2 != o2 {
                compatible_weight += compact.game.root_ranges[Player::P0][own] as f64
                    * compact.game.root_ranges[Player::P1][opp] as f64;
            }
        }
    }
    assert_eq!(
        compact.game.normalizer.to_bits(),
        compatible_weight.to_bits()
    );
    assert_eq!(
        compact.game.normalizer.to_bits(),
        dense.game.normalizer.to_bits()
    );
}

fn assert_chance_mapping(compact: &PostflopGame, dense: &PostflopGame) {
    let hands = compact.game.evaluator.hands();
    let (a, b) = (&compact.game.tree, &dense.game.tree);
    assert_eq!(a.deals.len(), b.deals.len());
    for (index, (compact_deal, dense_deal)) in a.deals.iter().zip(&b.deals).enumerate() {
        assert_eq!(
            compact.game.evaluator.deal_card(index),
            dense.game.evaluator.deal_card(index)
        );
        assert_eq!(compact_deal.weight.to_bits(), dense_deal.weight.to_bits());
        for player in Player::BOTH {
            match (compact_deal.maps[player], dense_deal.maps[player]) {
                (ReachMap::Identity, ReachMap::Identity) => {}
                (ReachMap::Mask(i), ReachMap::Mask(j)) => {
                    let actual = &a.masks[i as usize];
                    let reference = &b.masks[j as usize];
                    assert_eq!(actual.len(), hands.len(player));
                    assert_eq!(reference.len(), NUM_COMBOS);
                    for (local, &hand) in hands.combos(player).iter().enumerate() {
                        assert_eq!(actual[local].to_bits(), reference[hand as usize].to_bits());
                    }
                }
                (ReachMap::Transition(i), ReachMap::Transition(j)) => {
                    let actual = &a.transitions[i as usize];
                    let reference = &b.transitions[j as usize];
                    let dim = hands.len(player) as u32;
                    assert_eq!((actual.in_dim, actual.out_dim), (dim, dim));
                    assert_eq!(
                        (reference.in_dim, reference.out_dim),
                        (NUM_COMBOS as u32, NUM_COMBOS as u32)
                    );
                    let expected: Vec<_> = reference
                        .entries
                        .iter()
                        .filter_map(|&(from, to, weight)| {
                            match (
                                hands.index(player, from as usize),
                                hands.index(player, to as usize),
                            ) {
                                (Some(from), Some(to)) => Some((from as u32, to as u32, weight)),
                                (None, None) => None,
                                _ => panic!(
                                    "a suit transition must preserve each seat's root support"
                                ),
                            }
                        })
                        .collect();
                    assert_eq!(actual.entries, expected);
                    assert!(
                        actual
                            .entries
                            .iter()
                            .all(|&(from, to, _)| from < dim && to < dim)
                    );
                }
                maps => panic!("chance-map kinds differ: {maps:?}"),
            }
        }
    }
}

fn assert_columns(
    actual: &[f32],
    reference: &[f32],
    hands: &PostflopHands,
    player: Player,
    actions: usize,
    node: u32,
    label: &str,
) {
    assert_eq!(actual.len(), actions * hands.len(player));
    assert_eq!(reference.len(), actions * NUM_COMBOS);
    for action in 0..actions {
        for (local, &hand) in hands.combos(player).iter().enumerate() {
            assert_eq!(
                actual[action * hands.len(player) + local].to_bits(),
                reference[action * NUM_COMBOS + hand as usize].to_bits(),
                "{label}: node {node}, player {player:?}, action {action}, combo {hand}"
            );
        }
    }
}

fn root_reach(game: &CompiledGame<PostflopEvaluator>) -> PerPlayer<&[f32]> {
    PerPlayer::new(
        game.root_ranges[Player::P0].as_slice(),
        game.root_ranges[Player::P1].as_slice(),
    )
}

fn assert_solver_mapping(
    compact: &Solver<PostflopEvaluator, F32Storage>,
    dense: &Solver<PostflopEvaluator, F32Storage>,
) {
    let (a, b) = (compact.game(), dense.game());
    let hands = a.evaluator.hands();
    let (compact_state, dense_state) = (compact.state_ref(), dense.state_ref());
    assert_eq!(compact_state.iteration, dense_state.iteration);
    let StorageStateRef::F32 {
        regrets,
        strategy_sum,
    } = compact_state.storage
    else {
        panic!("compact test requires F32 state");
    };
    let StorageStateRef::F32 {
        regrets: dense_regrets,
        strategy_sum: dense_sum,
    } = dense_state.storage
    else {
        panic!("dense test requires F32 state");
    };
    for (id, node) in a.tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let (actual, reference) = (
            a.tree.storage_ref(node),
            b.tree.storage_ref(&b.tree.nodes[id]),
        );
        assert_eq!(actual.num_hands as usize, hands.len(node.player));
        assert_eq!(actual.num_actions, reference.num_actions);
        let compare = |left: &[f32], right: &[f32], label: &str| {
            assert_columns(
                left,
                right,
                hands,
                node.player,
                actual.num_actions as usize,
                id as u32,
                label,
            );
        };
        compare(
            &regrets[actual.offset..actual.offset + actual.len()],
            &dense_regrets[reference.offset..reference.offset + reference.len()],
            "regrets",
        );
        compare(
            &strategy_sum[actual.offset..actual.offset + actual.len()],
            &dense_sum[reference.offset..reference.offset + reference.len()],
            "strategy sum",
        );
        compare(
            &compact.current_strategy_at(id as u32),
            &dense.current_strategy_at(id as u32),
            "current strategy",
        );
        compare(
            &compact.average_strategy_at(id as u32),
            &dense.average_strategy_at(id as u32),
            "average strategy",
        );
    }
    for player in Player::BOTH {
        for (actual, reference, aggregate, dense_aggregate, label) in [
            (
                compact.expected_values_at(0, player, root_reach(a)),
                dense.expected_values_at(0, player, root_reach(b)),
                compact.expected_value(player),
                dense.expected_value(player),
                "root EV",
            ),
            (
                compact.best_response_values_at(0, player, root_reach(a)),
                dense.best_response_values_at(0, player, root_reach(b)),
                compact.best_response_value(player),
                dense.best_response_value(player),
                "root BR",
            ),
        ] {
            assert_columns(&actual, &reference, hands, player, 1, 0, label);
            let integral: f64 = a.root_ranges[player]
                .iter()
                .zip(&actual)
                .map(|(&weight, &value)| weight as f64 * value as f64)
                .sum::<f64>()
                / a.normalizer;
            assert_eq!(
                aggregate.to_bits(),
                integral.to_bits(),
                "{label} own-range integration"
            );
            assert_eq!(aggregate.to_bits(), dense_aggregate.to_bits(), "{label}");
        }
        let (actual, reference) = (
            compact.expected_values_everywhere(player),
            dense.expected_values_everywhere(player),
        );
        assert_eq!(actual.len(), reference.len());
        for (index, (actual, reference)) in actual.iter().zip(&reference).enumerate() {
            match (actual, reference) {
                (Some(actual), Some(reference)) => assert_columns(
                    actual,
                    reference,
                    hands,
                    player,
                    1,
                    index as u32,
                    "node CFV",
                ),
                (None, None) => {}
                _ => panic!("CFV presence differs at storage ref {index}"),
            }
        }
    }
}

fn compare_game(config: &PostflopConfig, iterations: u64) {
    let compact = build_postflop_game(config, chip_ev());
    let dense = build_postflop_game_dense(config, chip_ev());
    assert_root_mapping(config, &compact, &dense);
    assert_chance_mapping(&compact, &dense);
    let pairs = engine::pair_subtrees(&compact.game.tree, 0, &dense.game.tree, 0).unwrap();
    assert_eq!(pairs.len(), compact.game.tree.nodes.len());
    assert_eq!(pairs.len(), dense.game.tree.nodes.len());
    assert!(pairs.iter().all(|&(left, right)| left == right));
    assert!(compact.game.tree.storage_len < dense.game.tree.storage_len);
    if config.board.len() == 4 {
        assert!(!compact.game.tree.masks.is_empty());
        assert_eq!(
            !compact.game.tree.transitions.is_empty(),
            config.iso_merging
        );
    }
    let mut compact =
        Solver::<_, F32Storage>::new(compact.game, Box::<Dcfr>::default(), Some(iterations));
    let mut dense =
        Solver::<_, F32Storage>::new(dense.game, Box::<Dcfr>::default(), Some(iterations));
    for solver in [&mut compact, &mut dense] {
        solver.set_par(ParConfig {
            chance_depth: 0,
            min_children: usize::MAX,
        });
        solver.run(iterations);
    }
    assert_solver_mapping(&compact, &dense);
}

#[test]
fn asymmetric_weighted_river_matches_dense_state_and_values() {
    let config = river_config();
    let hands = PostflopHands::from_ranges(&config.board, &config.ranges);
    assert_eq!((hands.len(Player::P0), hands.len(Player::P1)), (4, 3));
    assert!(hands.index(Player::P0, combo("TcTd")).is_some());
    assert_eq!(hands.index(Player::P0, combo("2c3c")), None);
    assert_eq!(hands.index(Player::P1, combo("7d8d")), None);
    compare_game(&config, 4);
}

#[test]
fn turn_without_isomorphism_matches_dense_state_and_values() {
    compare_game(&turn_config(false), 2);
}

#[test]
fn turn_isomorphism_preserves_seat_support_and_matches_dense_state_and_values() {
    compare_game(&turn_config(true), 2);
}

#[test]
fn flop_keeps_root_hand_dimensions_through_both_deals() {
    // Build only the compact no-bet tree; no dense flop or solver run is
    // needed to check that later board masks do not renumber root hands.
    let mut config = river_config();
    config.board.truncate(3);
    config.streets.river = StreetTree::pot_fractions(&[], &[], 0);
    config.track_node_info = false;
    config.iso_merging = false;
    let game = build_postflop_game(&config, chip_ev());
    let hands = game.game.evaluator.hands();
    assert_eq!((hands.len(Player::P0), hands.len(Player::P1)), (4, 3));
    for player in Player::BOTH {
        assert_eq!(game.game.tree.root_dims[player] as usize, hands.len(player));
    }
    assert!(
        game.game
            .tree
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Chance)
            .count()
            > 1
    );
    for node in &game.game.tree.nodes {
        if node.kind == NodeKind::Action {
            assert_eq!(
                game.game.tree.storage_ref(node).num_hands as usize,
                hands.len(node.player)
            );
        }
    }
    for deal in &game.game.tree.deals {
        for player in Player::BOTH {
            let dim = hands.len(player);
            match deal.maps[player] {
                ReachMap::Identity => {}
                ReachMap::Mask(index) => {
                    assert_eq!(game.game.tree.masks[index as usize].len(), dim)
                }
                ReachMap::Transition(index) => {
                    let transition = &game.game.tree.transitions[index as usize];
                    assert_eq!(
                        (transition.in_dim as usize, transition.out_dim as usize),
                        (dim, dim)
                    );
                }
            }
        }
    }
    // Validate every representative card without relying on node_info or
    // deducing it from a compact mask (several cards have all-one masks).
    let mut pending = vec![(0, config.board.clone())];
    let mut visited_deals = 0;
    while let Some((id, board)) = pending.pop() {
        let node = game.game.tree.node(id);
        let mut seen = [false; 52];
        if node.kind == NodeKind::Chance {
            assert_eq!(node.num_children as usize, 52 - board.len());
        }
        for (offset, child) in game.game.tree.children(id).enumerate() {
            let mut next_board = board.clone();
            if node.kind == NodeKind::Chance {
                let card = game.game.evaluator.deal_card(node.aux as usize + offset);
                assert!(!board.contains(&card));
                assert!(!seen[card.index()]);
                seen[card.index()] = true;
                next_board.push(card);
                visited_deals += 1;
            }
            pending.push((child, next_board));
        }
    }
    assert_eq!(visited_deals, game.game.tree.deals.len());
}
