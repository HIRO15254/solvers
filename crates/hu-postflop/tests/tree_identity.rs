//! Integer structural regressions for retained common-input P1 acceptance fixtures.
//! No payoff, strategy, deal probability, or other floating-point data enters the hash.
use std::path::Path;
use std::time::Instant;

use hu_engine::NodeKind;
use hu_postflop::input::{NlhPayoff, Settings, lower};
use hu_postflop::{PostflopGame, try_build_postflop_game};
use nlh::{Player, Street};

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    /// Action, chance, terminal nodes, respectively.
    nodes: [u64; 3],
    /// Public decision nodes by OOP/IP.
    decisions: [u64; 2],
    /// Concrete engine infoset slots by OOP/IP, including runout-masked slots within root support.
    // PF1 (2026-10-06): pins now count seat-specific positive root support,
    // excluding starting-board conflicts. Public topology/hash pins are unchanged.
    infosets: [u64; 2],
    /// Builder action labels use integer milli-BB amounts.
    root_actions: Vec<String>,
    structure: String,
}

fn text(hash: &mut blake3::Hasher, value: &str) {
    hash.update(&(value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}

fn identity(game: &PostflopGame) -> Identity {
    let tree = &game.game.tree;
    let mut nodes = [0; 3];
    let mut decisions = [0; 2];
    let mut infosets = [0; 2];
    let mut hash = blake3::Hasher::new();
    hash.update(b"solvers.nlh/v1 P1 integer public tree identity v1\0");
    hash.update(&(tree.nodes.len() as u64).to_le_bytes());
    for (id, node) in tree.nodes.iter().enumerate() {
        let kind = match node.kind {
            NodeKind::Action => 0,
            NodeKind::Chance => 1,
            NodeKind::Terminal => 2,
        };
        nodes[kind] += 1;
        if node.kind == NodeKind::Action {
            let player = node.player.index();
            decisions[player] += 1;
            infosets[player] += tree.storage_ref(node).num_hands as u64;
        }
        // First child and child count retain the full ordering of the compiled topology.
        hash.update(&(id as u64).to_le_bytes());
        hash.update(&[kind as u8]);
        hash.update(&node.first_child.to_le_bytes());
        hash.update(&node.num_children.to_le_bytes());
        hash.update(&[if node.kind == NodeKind::Action {
            node.player.index() as u8
        } else {
            0
        }]);
        let info = &game.node_info[tree.tags[id] as usize];
        hash.update(&[match info.street {
            Street::Preflop => 0,
            Street::Flop => 1,
            Street::Turn => 2,
            Street::River => 3,
        }]);
        for player in Player::BOTH {
            hash.update(&info.contrib[player].0.to_le_bytes());
        }
        // Histories include chance card identities; action labels include every integer amount.
        text(&mut hash, &info.history);
        hash.update(&(info.actions.len() as u64).to_le_bytes());
        for action in &info.actions {
            text(&mut hash, action);
        }
    }
    Identity {
        nodes,
        decisions,
        infosets,
        root_actions: game.node_info[tree.tags[0] as usize].actions.clone(),
        structure: hash.finalize().to_hex().to_string(),
    }
}

fn check(name: &str, expected: Identity) {
    // These exact-equivalence fixtures stay at their existing owner; no duplicated configs.
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../cli/tests/fixtures/{name}.toml"));
    let start = Instant::now();
    let document = spot::Document::parse(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    let settings = Settings::parse(&document.spot, &document.solver, &document.output).unwrap();
    let config = lower(&document.spot, &settings).unwrap();
    let payoff = NlhPayoff::new(&document.spot).unwrap();
    let game = try_build_postflop_game(&config, payoff.pipeline()).unwrap();
    let build_seconds = start.elapsed().as_secs_f64();
    let actual = identity(&game);
    eprintln!("{name}: build_seconds={build_seconds:.3} {actual:?}");
    assert_eq!(actual, expected, "{name}");
}

#[test]
fn river_small() {
    check(
        "river_small",
        Identity {
            nodes: [6, 0, 9],
            decisions: [3, 3],
            infosets: [408, 153],
            root_actions: vec!["check".into(), "bet 5000".into()],
            structure: "c669c8ac1217f22c389445709867bdfc2a356ea751184311fa69937cf9f2b44f".into(),
        },
    );
}
#[test]
fn turn_small() {
    check(
        "turn_small",
        Identity {
            nodes: [580, 3, 722],
            decisions: [290, 290],
            infosets: [5510, 5800],
            root_actions: vec!["check".into(), "bet 20000".into()],
            structure: "3da11a7eeda55c11a1fa19e0ebed6beae60c32a1c5214e2d00d5c57efa24301f".into(),
        },
    );
}
#[test]
fn postflop_srp20() {
    check(
        "postflop_srp20",
        Identity {
            nodes: [52436, 642, 87420],
            decisions: [26218, 26218],
            infosets: [7839182, 10801816],
            root_actions: vec!["check".into(), "bet 50000".into()],
            structure: "49cf040541382b2cc52e806e01a88b08e56cdd500938f1aadd9624e0ee29cda9".into(),
        },
    );
}
#[test]
fn three_bet_pot_fast() {
    check(
        "3betpot_fast",
        Identity {
            nodes: [63602, 545, 84675],
            decisions: [31801, 31801],
            infosets: [2003463, 5088160],
            root_actions: vec!["check".into(), "bet 150000".into()],
            structure: "d6907929912e16d8a8969f20b16a60f88de290384065c21d5db3d5dd8b53853e".into(),
        },
    );
}
#[test]
fn postflop_pio_tree() {
    check(
        "postflop_pio_tree",
        Identity {
            nodes: [108045, 436, 172695],
            decisions: [54023, 54022],
            infosets: [1836782, 1620660],
            root_actions: vec!["check".into()],
            structure: "e17ced7199fdd8b8c807722a2227b0bf995ce3e7ad92d990714d7f693d2b72c4".into(),
        },
    );
}
#[test]
fn postflop_pio_icm() {
    check(
        "postflop_pio_icm",
        Identity {
            nodes: [108045, 436, 172695],
            decisions: [54023, 54022],
            infosets: [1836782, 1620660],
            root_actions: vec!["check".into()],
            structure: "e17ced7199fdd8b8c807722a2227b0bf995ce3e7ad92d990714d7f693d2b72c4".into(),
        },
    );
}
