//! Fixed-fixture typed-API preflight; this does not parse/normalize CLI TOML.
//! Default: native counting only. --build explicitly constructs the game and
//! rank tables, but never allocates solver storage or runs CFR / EV / BR.
//! Invoke one case per externally supervised process: estimate narrow [--build].

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

use cards::script::{POSTFLOP, PostflopVar, Script};
use cards::{Card, Chips, PerPlayer, Player, Range, Street, combo_cards};
use engine::NodeKind;
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{
    PerStreet, PostflopConfig, PostflopHands, StreetTree, build_postflop_game, memory_usage,
};

const SCRIPT: &str = "flop, turn, river {\n  replace bet [75]\n  replace raise [75]\n}\n";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        args.len() == 1 || (args.len() == 2 && args[1] == "--build"),
        "usage: estimate narrow|expanded [--build]"
    );
    let build = args.len() == 2;
    let (oop, ip, expected_raw, expected_support, expected_union, expected_mass, expected_elements) =
        match args[0].as_str() {
            "narrow" => (
                "TT+,AQs+,KQs",
                "JJ-99,AQs-ATs,KQs,QJs",
                [42, 38],
                [34, 30],
                49,
                870.0,
                10_176_768,
            ),
            "expanded" => (
                "TT+,AQs+,AQo+,A5s-A4s,KQs",
                "JJ-22,AQs-A2s,KQs-KTs,QJs-QTs,JTs,T9s,98s,AQo-ATo,KQo",
                [74, 184],
                [63, 160],
                191,
                8700.0,
                35_459_676,
            ),
            _ => panic!("unknown case; expected narrow or expanded"),
        };
    let board: Vec<Card> = "Qs Jh 2h"
        .split_whitespace()
        .map(|c| c.parse().unwrap())
        .collect();
    assert_eq!(board.iter().copied().collect::<BTreeSet<_>>().len(), 3);
    let ranges = PerPlayer::new(oop.parse::<Range>().unwrap(), ip.parse::<Range>().unwrap());
    for player in Player::BOTH {
        assert!(
            ranges[player]
                .weights()
                .iter()
                .all(|&w| w.is_finite() && (w == 0.0 || w == 1.0))
        );
    }
    // Same native tree-script compiler and street selection as CLI setup.
    let script = Script::<PostflopVar>::compile(SCRIPT, &BTreeMap::new(), &POSTFLOP).unwrap();
    assert!(script.params.is_empty());
    assert_eq!(script.rules.len(), 6);
    let street = |s| StreetTree::from_script(s, &script.rules, 2, false, None);
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(200),
        effective_stack: Chips(900),
        streets: PerStreet {
            flop: street(Street::Flop),
            turn: street(Street::Turn),
            river: street(Street::River),
        },
        min_bet: Chips(10),
        iso_merging: false,
        track_node_info: true, // Matches CLI setup; only allocates metadata under --build.
        preflop_aggressor: Some(Player::P0),
    };
    let hands = PostflopHands::from_ranges(&config.board, &config.ranges);
    let raw = Player::BOTH.map(|p| {
        config.ranges[p]
            .weights()
            .iter()
            .filter(|&&w| w > 0.0)
            .count()
    });
    let support = Player::BOTH.map(|p| hands.len(p));
    let union = hands
        .combos(Player::P0)
        .iter()
        .chain(hands.combos(Player::P1))
        .copied()
        .collect::<BTreeSet<_>>()
        .len();
    // Independent direct pair enumeration is tiny here (at most 63 * 160).
    let mut joint_mass = 0.0_f64;
    for &h in hands.combos(Player::P0) {
        let (a, b) = combo_cards(h as usize);
        for &o in hands.combos(Player::P1) {
            let (c, d) = combo_cards(o as usize);
            if a != c && a != d && b != c && b != d {
                joint_mass += f64::from(config.ranges[Player::P0].weight(h as usize))
                    * f64::from(config.ranges[Player::P1].weight(o as usize));
            }
        }
    }
    assert!(joint_mass.is_finite() && joint_mass > 0.0);
    let started = Instant::now();
    let estimate = memory_usage(&config);
    let seconds = started.elapsed().as_secs_f64();
    assert_eq!(estimate.f32_bytes % 8, 0);
    let elements = estimate.f32_bytes / 8;
    let scale_bytes = estimate.i16_bytes.checked_sub(elements * 4).unwrap();
    assert_eq!(scale_bytes % 8, 0);
    let actions = scale_bytes / 8;
    let chances = estimate
        .nodes
        .checked_sub(estimate.terminals + actions)
        .unwrap();
    let static_match = raw == expected_raw
        && support == expected_support
        && union == expected_union
        && joint_mass == expected_mass
        && elements == expected_elements
        && estimate.nodes == 367_662
        && estimate.terminals == 219_524
        && estimate.rank_tables == 1176
        && actions == 147_104
        && chances == 1034;
    println!(
        "{{\"schema\":\"r1.flop-native-preflight/v1\",\"phase\":\"count\",\"case\":\"{}\",\"cli_toml_normalized\":false,\"raw_support\":{:?},\"root_support\":{:?},\"union_support\":{},\"compatible_joint_mass\":{},\"root_combo_ids\":[{:?},{:?}],\"nodes\":{},\"terminals\":{},\"action_nodes_from_storage_formula\":{},\"chance_nodes_from_difference\":{},\"storage_elements_per_buffer\":{},\"f32_arena_bytes\":{},\"i16_arenas_and_scales_bytes\":{},\"distinct_showdown_boards\":{},\"rule_hits\":[{:?},{:?},{:?}],\"elapsed_seconds\":{},\"static_match\":{},\"build_requested\":{}}}",
        args[0],
        raw,
        support,
        union,
        joint_mass,
        hands.combos(Player::P0),
        hands.combos(Player::P1),
        estimate.nodes,
        estimate.terminals,
        actions,
        chances,
        elements,
        estimate.f32_bytes,
        estimate.i16_bytes,
        estimate.rank_tables,
        estimate.rule_hits.flop,
        estimate.rule_hits.turn,
        estimate.rule_hits.river,
        seconds,
        static_match,
        build,
    );
    std::io::stdout().flush().unwrap();
    assert!(
        static_match,
        "native count differs from fixed static fixture"
    );
    if !build {
        return;
    }

    // This is the only heavy entry point, reachable solely by explicit flag.
    // It creates native tree/masks/metadata/rank tables, but no F32Storage.
    let started = Instant::now();
    let built = build_postflop_game(
        &config,
        PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        },
    );
    let seconds = started.elapsed().as_secs_f64();
    let tree = &built.game.tree;
    let mut kinds = [0_u64; 3]; // action, chance, terminal
    let mut bets = [0_u64; 3];
    let mut raises = [0_u64; 3];
    for (id, node) in tree.nodes.iter().enumerate() {
        match node.kind {
            NodeKind::Action => {
                kinds[0] += 1;
                let info = &built.node_info[tree.tags[id] as usize];
                assert_eq!(info.actions.len(), node.num_children as usize);
                let s = match info.street {
                    Street::Flop => 0,
                    Street::Turn => 1,
                    Street::River => 2,
                    _ => unreachable!(),
                };
                bets[s] += u64::from(info.actions.iter().any(|a| a.starts_with("bet ")));
                raises[s] += u64::from(info.actions.iter().any(|a| a.starts_with("raise to ")));
            }
            NodeKind::Chance => kinds[1] += 1,
            NodeKind::Terminal => kinds[2] += 1,
        }
    }
    let built_support = Player::BOTH.map(|p| built.game.evaluator.hands().len(p));
    let build_match = tree.nodes.len() as u64 == estimate.nodes
        && kinds == [actions, chances, estimate.terminals]
        && tree.storage_len as u64 == elements
        && built_support == support
        && Player::BOTH.into_iter().all(|p| {
            tree.root_dims[p] as usize == support[p.index()]
                && built.game.root_ranges[p].len() == support[p.index()]
                && built.game.evaluator.hands().combos(p) == hands.combos(p)
        })
        && built.game.normalizer == joint_mass
        && built.game.zero_sum
        && built.rule_hits == estimate.rule_hits
        && bets == [2, 490, 61_152]
        && raises == [2, 294, 23_520];
    println!(
        "{{\"schema\":\"r1.flop-native-preflight/v1\",\"phase\":\"build\",\"case\":\"{}\",\"nodes\":{},\"action_chance_terminal_nodes\":{:?},\"storage_elements_per_buffer\":{},\"root_support\":{:?},\"normalizer\":{},\"zero_sum\":{},\"bet_available_nodes_by_street\":{:?},\"raise_available_nodes_by_street\":{:?},\"elapsed_seconds\":{},\"estimate_and_static_match\":{},\"solver_storage_allocated\":false,\"solve_executed\":false}}",
        args[0],
        tree.nodes.len(),
        kinds,
        tree.storage_len,
        built_support,
        built.game.normalizer,
        built.game.zero_sum,
        bets,
        raises,
        seconds,
        build_match,
    );
    std::io::stdout().flush().unwrap();
    assert!(
        build_match,
        "native build differs from estimate or fixed static fixture"
    );
}
