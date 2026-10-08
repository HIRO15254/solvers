use hu_engine::{NodeKind, Storage, TerminalEvaluator};
use hu_postflop::game::{ChipEv, NoRake, PayoffPipeline};
use hu_postflop::{PerStreet, PostflopConfig, try_build_postflop_game, try_memory_usage};
use nlh::{Card, Chips, PerPlayer, Player, combo_cards};

#[test]
fn runouts_mask_fixed_asymmetric_support_and_zero_dead_terminal_values() {
    let config = PostflopConfig {
        board: "Ks 7s 2s 3d"
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect(),
        ranges: PerPlayer::new(
            "AA,QQ,44:0.25,AJs".parse().unwrap(),
            "AA,TT,KQs:0.5".parse().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(5),
        streets: PerStreet::default(),
        iso_merging: false,
        ..Default::default()
    };
    let estimate = try_memory_usage(&config).unwrap();
    let game = try_build_postflop_game(
        &config,
        PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        },
    )
    .unwrap();
    let tree = &game.game.tree;
    let hands = &game.game.evaluator.hands;
    assert_ne!(hands.len(Player::P0), hands.len(Player::P1));
    assert_eq!(estimate.f32_bytes, tree.storage_len as u64 * 8);
    assert_eq!(
        estimate.i16_bytes,
        tree.storage_len as u64 * 4 + tree.storage_refs.len() as u64 * 8
    );
    assert_eq!(
        estimate.i16_f32avg_bytes,
        hu_engine::MixedStorage::bytes_for(tree.storage_len, tree.storage_refs.len())
    );
    for node in &tree.nodes {
        if node.kind == NodeKind::Action {
            assert_eq!(
                tree.storage_ref(node).num_hands as usize,
                hands.len(node.player)
            );
        }
    }
    let card: Card = "As".parse().unwrap();
    let entry = game.node_by_history("xx[As]").unwrap();
    let ip = tree.children(entry).next().unwrap();
    let terminal = tree.children(ip).next().unwrap();
    assert_eq!(tree.node(terminal).kind, NodeKind::Terminal);
    for p in Player::BOTH {
        let reach: Vec<_> = hands
            .combos(p.opponent())
            .iter()
            .map(|&h| {
                let (a, b) = combo_cards(h as usize);
                if a == card || b == card { 0.0 } else { 1.0 }
            })
            .collect();
        let mut out = vec![f32::NAN; hands.len(p)];
        game.game
            .evaluator
            .eval(tree.node(terminal).aux, p, &reach, &mut out);
        let mut dead = 0;
        for (&h, &value) in hands.combos(p).iter().zip(&out) {
            let (a, b) = combo_cards(h as usize);
            assert!(value.is_finite());
            if a == card || b == card {
                dead += 1;
                assert_eq!(value, 0.0);
            }
        }
        assert!(dead > 0);
    }
}

#[test]
fn queries_keep_global_combos_and_exclude_zero_weight_hands() {
    let board: Vec<Card> = "Ks 7h 2d 3c 9s"
        .split_whitespace()
        .map(|c| c.parse().unwrap())
        .collect();
    let ranges = PerPlayer::new(
        "AhAd:0.25,QhQd:0".parse().unwrap(),
        "JhJd,ThTd".parse().unwrap(),
    );
    let hands = hu_postflop::PostflopHands::new(&board, &ranges);
    let class = hu_postflop::queries::parse_class("AA,QQ").unwrap();
    let rows = hu_postflop::queries::combo_rows(&class, &hands, Player::P0, 2, &[0.25, 0.75]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].combo, "AhAd");
    assert_eq!(rows[0].probabilities, [0.25, 0.75]);
    let absent = hu_postflop::queries::parse_class("QhQd").unwrap();
    assert!(
        hu_postflop::queries::combo_rows(&absent, &hands, Player::P0, 2, &[0.25, 0.75]).is_empty()
    );
}
