//! Diagnostic decoder only: compares legacy/new scales without enabling P1 resume compatibility.
use hu_engine::{SolverState, StorageState};
use std::path::Path;

fn state(path: &Path) -> SolverState {
    let bytes = std::fs::read(path).expect("checkpoint");
    assert_eq!(&bytes[..8], b"SLVRCKPT");
    let version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    let payload = zstd::decode_all(&bytes[50..]).expect("zstd");
    match version {
        2 => {
            postcard::from_bytes::<(SolverState, String, f64)>(&payload)
                .unwrap()
                .0
        }
        3 => {
            postcard::from_bytes::<(SolverState, Option<String>, Option<f64>)>(&payload)
                .unwrap()
                .0
        }
        _ => panic!("diagnostic expects version 2 or 3"),
    }
}

fn scales(state: SolverState) -> (usize, Vec<f32>, Vec<f32>) {
    match state.storage {
        StorageState::I16 {
            regrets,
            regret_scales,
            strategy_scales,
            ..
        } => (regrets.len(), regret_scales, strategy_scales),
        _ => panic!("i16 checkpoint required"),
    }
}

fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(paths.len(), 3);
    let old = state(Path::new(&paths[0]));
    let new = state(Path::new(&paths[1]));
    assert_eq!(old.iteration, new.iteration);
    let iteration = old.iteration;
    let config_path = Path::new(&paths[2]);
    let raw = std::fs::read_to_string(config_path).unwrap();
    let mut prepared = hu_postflop::prepare::prepare(&raw, config_path).unwrap();
    prepared.config.track_node_info = true;
    let game =
        hu_postflop::try_build_postflop_game(&prepared.config, prepared.payoff.pipeline()).unwrap();
    let hands = &game.game.evaluator.hands;
    let tree = &game.game.tree;
    let mut old_offset = 0;
    let mut compared = 0;
    let mut differences = 0;
    let mut removed_max_blocks = 0;
    let mut dead_slots = 0;
    // Public node IDs reserve siblings together; storage refs are DFS preorder.
    let mut nodes: Vec<_> = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind == hu_engine::NodeKind::Action)
        .collect();
    nodes.sort_by_key(|(_, n)| n.aux);
    for (id, node) in nodes {
        let r = tree.storage_ref(node);
        let support = hands.combos(node.player);
        let mut board = prepared.config.board.clone();
        for token in game.node_info[tree.tags[id] as usize]
            .history
            .split('[')
            .skip(1)
        {
            board.push(
                token
                    .split(']')
                    .next()
                    .unwrap()
                    .parse::<nlh::Card>()
                    .unwrap(),
            );
        }
        match (&old.storage, &new.storage) {
            (
                StorageState::F32 {
                    regrets: ar,
                    strategy_sum: as_,
                },
                StorageState::F32 {
                    regrets: br,
                    strategy_sum: bs,
                },
            ) => {
                for a in 0..r.num_actions as usize {
                    for (local, &global) in support.iter().enumerate() {
                        let (c1, c2) = nlh::combo_cards(global as usize);
                        if board.contains(&c1) || board.contains(&c2) {
                            dead_slots += 1;
                            continue;
                        }
                        let oi = old_offset + a * nlh::NUM_COMBOS + global as usize;
                        let ni = r.offset + a * support.len() + local;
                        compared += 2;
                        differences += usize::from(ar[oi].to_bits() != br[ni].to_bits());
                        differences += usize::from(as_[oi].to_bits() != bs[ni].to_bits());
                    }
                }
            }
            (StorageState::I16 { regrets, .. }, StorageState::I16 { .. }) => {
                let block =
                    &regrets[old_offset..old_offset + r.num_actions as usize * nlh::NUM_COMBOS];
                let all_max = block.iter().map(|v| v.unsigned_abs()).max().unwrap();
                let supported_max = (0..r.num_actions as usize)
                    .flat_map(|a| {
                        support
                            .iter()
                            .map(move |&h| block[a * nlh::NUM_COMBOS + h as usize].unsigned_abs())
                    })
                    .max()
                    .unwrap();
                removed_max_blocks += usize::from(all_max > supported_max);
            }
            _ => panic!("matching storage variants required"),
        }
        old_offset += r.num_actions as usize * nlh::NUM_COMBOS;
    }
    if matches!(old.storage, StorageState::F32 { .. }) {
        println!(
            "{}",
            serde_json::json!({"iteration":iteration,"comparedF32Values":compared,"bitDifferences":differences,"deadBoardSlotsSkipped":dead_slots})
        );
        assert_eq!(differences, 0, "support f32 checkpoint bits differ");
        return;
    }
    let (old_len, old_r, old_s) = scales(old);
    let (new_len, new_r, new_s) = scales(new);
    assert_eq!(old_r.len(), new_r.len());
    assert_eq!(old_s.len(), new_s.len());
    let blocks: Vec<_> = (0..old_r.len())
        .map(|i| {
            serde_json::json!({
                "block": i,
                "oldRegretScale": old_r[i], "newRegretScale": new_r[i],
                "oldStrategyScale": old_s[i], "newStrategyScale": new_s[i],
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "iteration": iteration,
        "legacyMaxStrictlyOutsideSupportBlocks": removed_max_blocks,
        "oldElements": old_len, "newElements": new_len,
        "changedRegretScales": old_r.iter().zip(&new_r).filter(|(a,b)| a.to_bits() != b.to_bits()).count(),
        "changedStrategyScales": old_s.iter().zip(&new_s).filter(|(a,b)| a.to_bits() != b.to_bits()).count(),
        "blocks": blocks,
    })).unwrap());
}
