//! Exercises the legacy wire format with a solved, multi-street tree.
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use hu_engine::NodeKind;
use hu_postflop::{
    artifact, prepare, run,
    sol::{self, HEADER_LEN, SolReader},
};
use serde::{Deserialize, Serialize};

const TURN: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 4h"
[ranges]
BB = "AK,KQ,AA"
BTN = "QQ,JJ"
[tree]
script = "turn { replace bet [50] } river { replace bet [50] } river when aggressions >= 1 { remove raise }"
[solver]
storage = "f32"
[solver.stop]
max_iterations = 2
check_every = 1
[run]
memory = "1GiB"
threads = 1
"#;

// These are the v4 on-disk directory types, not the current private codec types.
#[derive(Serialize, Deserialize)]
struct Entry {
    first_sref: u32,
    last_sref: u32,
    nodes: u32,
    offset: u64,
    compressed_len: u64,
    decoded_len: u64,
    checksum: [u8; 32],
}
#[derive(Serialize, Deserialize)]
struct Directory {
    strategy: Vec<Entry>,
    values: Vec<Entry>,
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}
fn u64_at(b: &[u8], at: usize) -> usize {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap()) as usize
}
fn put_u64(b: &mut [u8], at: usize, value: usize) {
    b[at..at + 8].copy_from_slice(&(value as u64).to_le_bytes());
}

// Rewrite the label table and edges as the v4 builder did: wager amounts
// are cumulative from the start of the subgame, not street-relative. The
// rest of the index and the actual solver-produced blocks remain unchanged.
fn legacy_index(index: &[u8], loaded: &artifact::LoadedSol) -> Vec<u8> {
    let nodes = u64_at(index, 0);
    let deals = u64_at(index, 8);
    let masks = u32_at(index, 16);
    let transitions = u32_at(index, 20);
    let labels = u32_at(index, 24);
    let kinds = &index[36..36 + nodes];
    let mut at = 36
        + nodes
        + kinds.iter().filter(|&&k| k != 3).count() * 2
        + kinds.iter().filter(|&&k| k == 3).count() * 4
        + deals * 13;
    for _ in 0..masks {
        let len = u32_at(index, at);
        at += 4 + len * 4;
    }
    for _ in 0..transitions {
        let len = u32_at(index, at + 8);
        at += 12 + len * 12;
    }
    let labels_start = at;
    for _ in 0..labels {
        let len = u32_at(index, at);
        at += 4 + len;
    }
    let edges_start = at;
    let tree = &loaded.game.tree;
    let view = loaded.view();
    let initial_pot = view.pot(0);
    let mut intern = HashMap::<String, u32>::new();
    let mut new_labels = Vec::<String>::new();
    let mut edges = Vec::new();
    let mut changed = 0;
    for (id, node) in tree.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let mut entry = id as u32;
        while entry != 0 {
            let parent = loaded.game.index.parents[entry as usize];
            if view.street(parent) != view.street(entry) {
                break;
            }
            entry = parent;
        }
        let baseline = (view.pot(entry) - initial_pot) / 2;
        for label in view.actions(id as u32) {
            let legacy = if let Some((prefix, amount)) = label.split_once(' ') {
                if prefix == "bet" || label.starts_with("raise to ") {
                    let (prefix, amount) = if prefix == "bet" {
                        ("bet ", amount)
                    } else {
                        ("raise to ", label.strip_prefix("raise to ").unwrap())
                    };
                    let value = nlh::ChipScale::DEFAULT.parse_bb(amount).unwrap().0;
                    if baseline != 0 {
                        changed += 1;
                    }
                    format!(
                        "{prefix}{}",
                        nlh::ChipScale::DEFAULT
                            .amount_text(nlh::MwChips(value + u64::from(baseline)))
                    )
                } else {
                    label.to_owned()
                }
            } else {
                label.to_owned()
            };
            let next = new_labels.len() as u32;
            let id = *intern.entry(legacy.clone()).or_insert_with(|| {
                new_labels.push(legacy);
                next
            });
            edges.extend_from_slice(&id.to_le_bytes());
        }
    }
    assert!(changed > 0, "fixture must exercise cumulative river wagers");
    let mut out = index[..labels_start].to_vec();
    out[24..28].copy_from_slice(&(new_labels.len() as u32).to_le_bytes());
    for label in new_labels {
        out.extend_from_slice(&(label.len() as u32).to_le_bytes());
        out.extend_from_slice(label.as_bytes());
    }
    out.extend_from_slice(&edges);
    out.extend_from_slice(&index[edges_start + edges.len()..]);
    out
}

fn write_v4(current: &Path, legacy: &Path) {
    let loaded = artifact::load_sol(current, 2, None).unwrap();
    let bytes = std::fs::read(current).unwrap();
    let meta_len = u64_at(&bytes, 50);
    let index_len = u64_at(&bytes, 58);
    let dir_offset = u64_at(&bytes, 66);
    let index_start = HEADER_LEN + meta_len;
    let index = zstd::stream::decode_all(&bytes[index_start..index_start + index_len]).unwrap();
    let index = zstd::bulk::compress(&legacy_index(&index, &loaded), 3).unwrap();
    let directory_bytes = zstd::stream::decode_all(&bytes[dir_offset..]).unwrap();
    let mut directory: Directory = postcard::from_bytes(&directory_bytes).unwrap();
    let chunks = &bytes[index_start + index_len..dir_offset];
    let new_chunks_start = index_start + index.len();
    for entry in directory
        .strategy
        .iter_mut()
        .chain(directory.values.iter_mut())
    {
        entry.offset =
            (entry.offset as usize - (index_start + index_len) + new_chunks_start) as u64;
    }
    let directory = zstd::bulk::compress(&postcard::to_allocvec(&directory).unwrap(), 3).unwrap();
    let mut header = bytes[..HEADER_LEN].to_vec();
    header[8..10].copy_from_slice(&4u16.to_le_bytes());
    put_u64(&mut header, 58, index.len());
    put_u64(&mut header, 66, new_chunks_start + chunks.len());
    put_u64(&mut header, 74, directory.len());
    header[114..146].copy_from_slice(blake3::hash(&index).as_bytes());
    header[146..178].copy_from_slice(blake3::hash(&directory).as_bytes());
    let mut result = header;
    result.extend_from_slice(&bytes[HEADER_LEN..index_start]);
    result.extend_from_slice(&index);
    result.extend_from_slice(chunks);
    result.extend_from_slice(&directory);
    std::fs::write(legacy, result).unwrap();
}

#[test]
fn v4_upgrade_preserves_real_solve_and_converts_every_history() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("current.sol");
    let legacy = dir.path().join("legacy.sol");
    let upgraded = dir.path().join("upgraded.sol");
    let prepared = prepare::prepare(TURN, &dir.path().join("spot.toml")).unwrap();
    run::run(
        run::RunRequest {
            prepared: &prepared,
            checkpoint: &dir.path().join("solve.ckpt"),
            solution: &current,
            state: None,
            evaluation_history: vec![],
            elapsed_before: Duration::ZERO,
            cancel: &AtomicBool::new(false),
        },
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .unwrap();
    write_v4(&current, &legacy);
    sol::upgrade_sol(&legacy, &upgraded).unwrap();

    let original = artifact::load_sol(&current, 2, None).unwrap();
    let actual = artifact::load_sol(&upgraded, 2, None).unwrap();
    assert_eq!(original.meta, actual.meta);
    assert_eq!(original.mode, actual.mode);
    assert_eq!(original.game.tree.nodes.len(), actual.game.tree.nodes.len());
    for (id, node) in original.game.tree.nodes.iter().enumerate() {
        let id = id as u32;
        assert_eq!(
            original.view().history(id),
            actual.view().history(id),
            "node {id}"
        );
        if node.kind == NodeKind::Action {
            assert_eq!(
                original.view().actions(id),
                actual.view().actions(id),
                "node {id}"
            );
            assert_eq!(
                original.stored_strategy(id).unwrap(),
                actual.stored_strategy(id).unwrap(),
                "node {id}"
            );
            assert_eq!(
                original.stored_values(id).unwrap(),
                actual.stored_values(id).unwrap(),
                "node {id}"
            );
        }
    }
    let mut expected_blocks = Vec::new();
    let mut expected_values = Vec::new();
    SolReader::open(&current)
        .unwrap()
        .visit_blocks(
            |s, b| expected_blocks.push((s, b.to_vec())),
            |s, scale, b| expected_values.push((s, scale, b.to_vec())),
        )
        .unwrap();
    let mut actual_blocks = Vec::new();
    let mut actual_values = Vec::new();
    SolReader::open(&upgraded)
        .unwrap()
        .visit_blocks(
            |s, b| actual_blocks.push((s, b.to_vec())),
            |s, scale, b| actual_values.push((s, scale, b.to_vec())),
        )
        .unwrap();
    assert!(!expected_blocks.is_empty());
    assert_eq!(expected_blocks, actual_blocks);
    assert_eq!(expected_values, actual_values);
}
