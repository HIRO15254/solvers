//! Compare the bulk byte adapter with independent, ordinary Vec<u8> derives.
//! The legacy records deliberately do not reuse the production field adapter.

use std::io::Write;

use formats::{
    SOL_FORMAT_VERSION, SolError, SolMeta, SolPayload, SolReader, StrategyBlock, StreetsStored,
    ValueBlock, config_hash, read_sol, write_sol,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SequenceStrategy {
    sref: u32,
    probs: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SequenceValue {
    sref: u32,
    scale: f32,
    values: Vec<u8>,
}

fn sequence_pair(
    strategy: &StrategyBlock,
    value: &ValueBlock,
) -> (SequenceStrategy, SequenceValue) {
    (
        SequenceStrategy {
            sref: strategy.sref,
            probs: strategy.probs.clone(),
        },
        SequenceValue {
            sref: value.sref,
            scale: value.scale,
            values: value.values.clone(),
        },
    )
}

#[test]
fn byte_fields_match_legacy_sequence_bytes_at_varint_boundaries() {
    for (index, len) in [0usize, 1, 127, 128, 16_383, 16_384]
        .into_iter()
        .enumerate()
    {
        let sref = [0, 1, 127, 128, 65_535, u32::MAX][index];
        // Includes every u8 value and the LE patterns of negative i16 values.
        let bytes: Vec<u8> = (0..len).map(|i| (i % 256) as u8).collect();
        for scale in [0.0, 0.5] {
            let strategy = StrategyBlock {
                sref,
                probs: bytes.clone(),
            };
            let value = ValueBlock {
                sref,
                scale,
                values: bytes.clone(),
            };
            let legacy = sequence_pair(&strategy, &value);
            let adapted = (strategy, value);
            let expected = postcard::to_allocvec(&legacy).unwrap();
            let actual = postcard::to_allocvec(&adapted).unwrap();
            assert_eq!(actual, expected, "length {len}, scale {scale}");
            assert_eq!(
                postcard::experimental::serialized_size(&adapted).unwrap(),
                expected.len()
            );
            assert_eq!(
                postcard::from_bytes::<(StrategyBlock, ValueBlock)>(&expected).unwrap(),
                adapted
            );
            assert_eq!(
                postcard::from_bytes::<(SequenceStrategy, SequenceValue)>(&actual).unwrap(),
                legacy
            );
        }
    }
}

#[test]
fn byte_fields_have_literal_canonical_postcard_bytes() {
    let strategy = StrategyBlock {
        sref: 127,
        probs: vec![0, 127, 128, 255],
    };
    assert_eq!(
        postcard::to_allocvec(&strategy).unwrap(),
        [127, 4, 0, 127, 128, 255]
    );
    let value = ValueBlock {
        sref: 128,
        scale: 1.0,
        values: vec![0, 128, 255, 127],
    };
    assert_eq!(
        postcard::to_allocvec(&value).unwrap(),
        [128, 1, 0, 0, 128, 63, 4, 0, 128, 255, 127]
    );
}

#[test]
fn byte_fields_preserve_json_arrays_and_integer_validation() {
    let strategy = StrategyBlock {
        sref: 3,
        probs: vec![0, 128, 255],
    };
    let value = ValueBlock {
        sref: 3,
        scale: 0.5,
        values: vec![255, 255, 0, 128],
    };
    let legacy = sequence_pair(&strategy, &value);
    let adapted = (strategy, value);
    let expected = serde_json::to_vec(&legacy).unwrap();
    assert_eq!(serde_json::to_vec(&adapted).unwrap(), expected);
    assert_eq!(
        serde_json::from_slice::<(StrategyBlock, ValueBlock)>(&expected).unwrap(),
        adapted
    );
    for invalid in ["[256]", "[-1]", "[1.5]", "[true]", "\"bytes\""] {
        let json = format!(r#"{{"sref":0,"probs":{invalid}}}"#);
        assert!(serde_json::from_str::<SequenceStrategy>(&json).is_err());
        assert!(serde_json::from_str::<StrategyBlock>(&json).is_err());
    }
}

// Exact v1 payload field order from baseline 9632d8b. Public v3 loading must
// still reject the v1 container; this tests the payload codec used by legacy
// research readers that decode these production block types.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct V1Payload<S, V> {
    config_toml: String,
    meta: SolMeta,
    mode: StreetsStored,
    blocks: Vec<S>,
    values: Vec<V>,
}

fn fixture() -> SolPayload {
    let srefs = (0..65).chain(128..133);
    let mut result = SolPayload {
        config_toml: "fixture = \"byte-serde-compatibility\"\n".to_owned(),
        meta: SolMeta {
            iterations: 128,
            expl: [0.125, -0.001],
            ev: [-10.0, 30.0],
            nash_conv: 0.124,
            storage: "f32".to_owned(),
            wall_secs: 1.0,
        },
        mode: StreetsStored::NoRivers,
        node_count: 140,
        blocks: Vec::new(),
        values: Vec::new(),
    };
    for sref in srefs {
        result.blocks.push(StrategyBlock {
            sref,
            probs: vec![0, 0, 255, 255, 0, 128, 255, 127],
        });
        result.values.push(ValueBlock {
            sref,
            scale: if sref == 0 { 0.0 } else { 0.5 },
            values: vec![0, 128, 255, 127, 255, 255, 0, 0],
        });
    }
    result
}

#[test]
fn legacy_v1_payload_decodes_unchanged_but_container_is_still_rejected() {
    let source = fixture();
    let (blocks, values) = source
        .blocks
        .iter()
        .zip(&source.values)
        .map(|(strategy, value)| sequence_pair(strategy, value))
        .unzip();
    let legacy = V1Payload {
        config_toml: source.config_toml.clone(),
        meta: source.meta.clone(),
        mode: source.mode,
        blocks,
        values,
    };
    let adapted = V1Payload {
        config_toml: source.config_toml,
        meta: source.meta,
        mode: source.mode,
        blocks: source.blocks,
        values: source.values,
    };
    let raw = postcard::to_allocvec(&legacy).unwrap();
    assert_eq!(postcard::to_allocvec(&adapted).unwrap(), raw);
    assert_eq!(
        postcard::from_bytes::<V1Payload<StrategyBlock, ValueBlock>>(&raw).unwrap(),
        adapted
    );
    assert_eq!(
        postcard::from_bytes::<V1Payload<SequenceStrategy, SequenceValue>>(
            &postcard::to_allocvec(&adapted).unwrap()
        )
        .unwrap(),
        legacy
    );
    let mut file = b"SLVRSOLV".to_vec();
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&config_hash(adapted.config_toml.as_bytes()));
    file.extend_from_slice(&adapted.meta.iterations.to_le_bytes());
    file.extend(zstd::encode_all(raw.as_slice(), 0).unwrap());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sol");
    std::fs::write(&path, file).unwrap();
    assert!(matches!(
        read_sol(&path),
        Err(SolError::BadVersion { found: 1, expected }) if expected == SOL_FORMAT_VERSION
    ));
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[test]
fn v3_chunks_keep_legacy_bytes_checksums_and_compression() {
    let payload = fixture();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("current.sol");
    write_sol(&path, &payload).unwrap();
    let file = std::fs::read(&path).unwrap();
    assert_eq!(u16::from_le_bytes(file[8..10].try_into().unwrap()), 3);
    // Independent v3 wire constants and expected 64-node/gap boundaries.
    let directory_start = 106 + u64_at(&file, 50) as usize;
    assert_eq!(u64_at(&file, 98), 3);
    for (index, range) in [0..64, 64..65, 65..70].into_iter().enumerate() {
        let entry = directory_start + index * 64;
        assert_eq!(u32_at(&file, entry), payload.blocks[range.start].sref);
        assert_eq!(u32_at(&file, entry + 4), payload.blocks[range.end - 1].sref);
        assert_eq!(u32_at(&file, entry + 8) as usize, range.len());
        let legacy: Vec<_> = payload.blocks[range.clone()]
            .iter()
            .zip(&payload.values[range])
            .map(|(strategy, value)| sequence_pair(strategy, value))
            .collect();
        let raw = postcard::to_allocvec(&legacy).unwrap();
        assert_eq!(u32_at(&file, entry + 28) as usize, raw.len());
        assert_eq!(&file[entry + 32..entry + 64], blake3::hash(&raw).as_bytes());
        let offset = u64_at(&file, entry + 16) as usize;
        let compressed_len = u32_at(&file, entry + 24) as usize;
        let actual_frame = &file[offset..offset + compressed_len];
        assert_eq!(zstd::decode_all(actual_frame).unwrap(), raw);
        // Same encoder settings and one write as the pre-adapter writer.
        let mut encoder = zstd::Encoder::new(Vec::new(), 0).unwrap();
        encoder.include_checksum(true).unwrap();
        encoder.write_all(&raw).unwrap();
        assert_eq!(encoder.finish().unwrap(), actual_frame);
    }
    assert_eq!(read_sol(&path).unwrap(), payload);
    let mut reader = SolReader::open(&path).unwrap();
    assert_eq!(reader.metadata().stored_nodes, 70);
    for index in [69, 0, 64, 63, 65] {
        let sref = payload.blocks[index].sref;
        assert_eq!(
            reader.read_node(sref).unwrap().unwrap(),
            (payload.blocks[index].clone(), payload.values[index].clone())
        );
    }
    assert!(reader.read_node(127).unwrap().is_none());
}

#[test]
fn truncated_byte_fields_and_excessive_declared_lengths_are_rejected() {
    let legacy = SequenceStrategy {
        sref: 128,
        probs: vec![0, 128, 255],
    };
    let raw = postcard::to_allocvec(&legacy).unwrap();
    for end in 0..raw.len() {
        assert!(postcard::from_bytes::<SequenceStrategy>(&raw[..end]).is_err());
        assert!(postcard::from_bytes::<StrategyBlock>(&raw[..end]).is_err());
    }
    // sref 0, declared byte length u32::MAX, no contents. The byte decoder
    // checks its source extent before the visitor can allocate the Vec.
    assert!(postcard::from_bytes::<StrategyBlock>(&[0, 255, 255, 255, 255, 15]).is_err());
}
