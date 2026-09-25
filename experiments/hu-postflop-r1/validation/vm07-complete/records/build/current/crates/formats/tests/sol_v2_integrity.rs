//! Small public-API corruption fixtures for the current v3 container.
//! The target keeps its original name so existing validation commands apply.
//! Offsets are deliberately specified here independently of the codec.

use std::path::PathBuf;

use formats::{
    SOL_MAX_METADATA_BYTES, SOL_MAX_NODE_BYTES, SolError, SolMeta, SolMetadata, SolPayload,
    SolReader, StrategyBlock, StreetsStored, ValueBlock, read_sol, write_sol,
};

const PREFIX_LEN: usize = 106;
const ENTRY_LEN: usize = 64;
const METADATA_LENGTH: usize = 50;
const METADATA_DECODED_LENGTH: usize = 58;
const METADATA_DIGEST: usize = 66;
const DIRECTORY_COUNT: usize = 98;

fn payload() -> SolPayload {
    SolPayload {
        config_toml: "fixture = \"sol-v3-integrity\"\n".to_owned(),
        meta: SolMeta {
            iterations: 7,
            expl: [0.1, 0.2],
            ev: [1.0, -1.0],
            nash_conv: 0.3,
            storage: "f32".to_owned(),
            wall_secs: 0.5,
        },
        mode: StreetsStored::NoRivers,
        node_count: 3,
        blocks: vec![
            StrategyBlock {
                sref: 0,
                probs: vec![0, 0, 255, 255],
            },
            StrategyBlock {
                sref: 2,
                probs: vec![255, 255, 0, 0],
            },
        ],
        values: vec![
            ValueBlock {
                sref: 0,
                scale: 0.5,
                values: vec![2, 0, 254, 255],
            },
            ValueBlock {
                sref: 2,
                scale: 0.25,
                values: vec![4, 0, 252, 255],
            },
        ],
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, Vec<u8>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fixture.sol");
    write_sol(&path, &payload()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    (directory, path, bytes)
}

fn grouped_payload() -> SolPayload {
    let mut result = payload();
    let block = result.blocks[0].clone();
    let value = result.values[0].clone();
    result.node_count = 128;
    result.blocks = (0..128)
        .map(|sref| StrategyBlock {
            sref,
            ..block.clone()
        })
        .collect();
    result.values = (0..128)
        .map(|sref| ValueBlock {
            sref,
            ..value.clone()
        })
        .collect();
    result
}

#[test]
fn bounded_groups_round_trip_and_corruption_is_isolated_to_a_chunk() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("groups.sol");
    let expected = grouped_payload();
    write_sol(&path, &expected).unwrap();
    let original = std::fs::read(&path).unwrap();
    assert_eq!(u64_at(&original, DIRECTORY_COUNT), 2);
    assert!(
        original.len() < 128 * ENTRY_LEN,
        "do not retain a per-node directory"
    );
    assert_eq!(read_sol(&path).unwrap(), expected);
    let mut reader = SolReader::open(&path).unwrap();
    assert_eq!(reader.stored_srefs().len(), 128);
    assert_eq!(
        reader.stored_srefs().collect::<Vec<_>>(),
        (0..128).collect::<Vec<_>>()
    );
    for sref in [127, 0, 64, 63, 65] {
        assert_eq!(
            reader.read_node(sref).unwrap().unwrap(),
            (
                expected.blocks[sref as usize].clone(),
                expected.values[sref as usize].clone()
            )
        );
    }
    assert!(reader.read_node(128).unwrap().is_none());
    drop(reader);

    let mut damaged = original.clone();
    *damaged.last_mut().unwrap() ^= 1;
    std::fs::write(&path, damaged).unwrap();
    let mut reader = SolReader::open(&path).unwrap();
    assert_eq!(reader.metadata().stored_nodes, 128);
    assert!(reader.read_node(63).unwrap().is_some());
    assert!(reader.read_node(64).is_err());
    assert!(reader.read_node(127).is_err());
    assert!(read_sol(&path).is_err());
    drop(reader);

    // A correct checksum cannot conceal a bad ID elsewhere in the requested
    // chunk. Read its first node while corrupting its final node.
    let mut pairs: Vec<_> = expected.blocks[64..]
        .iter()
        .cloned()
        .zip(expected.values[64..].iter().cloned())
        .collect();
    pairs[63].1.sref = 126;
    let raw = postcard::to_allocvec(&pairs).unwrap();
    std::fs::write(&path, replace_last_node(&original, &raw)).unwrap();
    let mut reader = SolReader::open(&path).unwrap();
    assert!(
        matches!(reader.read_node(64), Err(SolError::InvalidLayout(message)) if message.contains("identity"))
    );
    drop(reader);

    // A valid inner list with one missing node must fail its count check.
    pairs.pop();
    let raw = postcard::to_allocvec(&pairs).unwrap();
    std::fs::write(&path, replace_last_node(&original, &raw)).unwrap();
    let mut reader = SolReader::open(&path).unwrap();
    assert!(
        matches!(reader.read_node(64), Err(SolError::InvalidLayout(message)) if message.contains("count"))
    );
    drop(reader);

    let mut count_mismatch = original;
    let first = directory_start(&count_mismatch);
    set_u32(&mut count_mismatch, first + 4, 62);
    set_u32(&mut count_mismatch, first + 8, 63);
    std::fs::write(&path, count_mismatch).unwrap();
    assert!(SolReader::open(&path).is_err());
}

#[test]
fn gapped_srefs_and_maximum_sref_have_exact_indices_without_overflow() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gaps.sol");
    let mut expected = payload();
    expected.blocks[1].sref = u32::MAX;
    expected.values[1].sref = u32::MAX;
    expected.node_count = u64::from(u32::MAX) + 1;
    write_sol(&path, &expected).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut reader = SolReader::open(&path).unwrap();
    assert_eq!(reader.stored_srefs().collect::<Vec<_>>(), [0, u32::MAX]);
    assert!(reader.read_node(1).unwrap().is_none());
    assert!(reader.read_node(u32::MAX - 1).unwrap().is_none());
    assert_eq!(
        reader.read_node(u32::MAX).unwrap().unwrap().0.sref,
        u32::MAX
    );
    drop(reader);
    let mut overflow = original;
    let second = directory_start(&overflow) + ENTRY_LEN;
    set_u32(&mut overflow, second + 8, 2);
    std::fs::write(&path, overflow).unwrap();
    assert!(SolReader::open(&path).is_err());
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn set_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn directory_start(bytes: &[u8]) -> usize {
    PREFIX_LEN + usize::try_from(u64_at(bytes, METADATA_LENGTH)).unwrap()
}

/// Rebuild only the final node frame, including its digest. This makes
/// identity/codec tests reach their intended checks instead of failing an
/// earlier checksum or extent check.
fn replace_last_node(original: &[u8], raw: &[u8]) -> Vec<u8> {
    let entry = directory_start(original) + ENTRY_LEN;
    let offset = usize::try_from(u64_at(original, entry + 16)).unwrap();
    let compressed = zstd::encode_all(raw, 0).unwrap();
    let mut bytes = original[..offset].to_vec();
    bytes.extend_from_slice(&compressed);
    set_u32(&mut bytes, entry + 24, compressed.len() as u32);
    set_u32(&mut bytes, entry + 28, raw.len() as u32);
    bytes[entry + 32..entry + ENTRY_LEN].copy_from_slice(blake3::hash(raw).as_bytes());
    bytes
}

/// Replace metadata while preserving valid hashes, directory offsets and node
/// frames. The regular writer intentionally cannot create these invalid files.
fn replace_meta(original: &[u8], meta: &SolMeta) -> Vec<u8> {
    let old_directory = directory_start(original);
    let raw = zstd::decode_all(&original[PREFIX_LEN..old_directory]).unwrap();
    let mut metadata: SolMetadata = postcard::from_bytes(&raw).unwrap();
    metadata.meta = meta.clone();
    let raw = postcard::to_allocvec(&metadata).unwrap();
    let compressed = zstd::encode_all(raw.as_slice(), 0).unwrap();
    let mut bytes = original[..PREFIX_LEN].to_vec();
    set_u64(&mut bytes, METADATA_LENGTH, compressed.len() as u64);
    set_u64(&mut bytes, METADATA_DECODED_LENGTH, raw.len() as u64);
    bytes[METADATA_DIGEST..DIRECTORY_COUNT].copy_from_slice(blake3::hash(&raw).as_bytes());
    bytes.extend_from_slice(&compressed);
    let new_directory = bytes.len();
    bytes.extend_from_slice(&original[old_directory..]);
    let count = usize::try_from(u64_at(original, DIRECTORY_COUNT)).unwrap();
    for index in 0..count {
        let original_offset =
            usize::try_from(u64_at(original, old_directory + index * ENTRY_LEN + 16)).unwrap();
        let new_offset = new_directory + (original_offset - old_directory);
        set_u64(
            &mut bytes,
            new_directory + index * ENTRY_LEN + 16,
            new_offset as u64,
        );
    }
    bytes
}

#[test]
fn writer_and_reader_reject_invalid_metadata_even_with_valid_checksums() {
    let (directory, path, original) = fixture();
    let mut cases = Vec::new();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for field in 0..6 {
            let mut meta = payload().meta;
            *match field {
                0 => &mut meta.ev[0],
                1 => &mut meta.ev[1],
                2 => &mut meta.expl[0],
                3 => &mut meta.expl[1],
                4 => &mut meta.nash_conv,
                5 => &mut meta.wall_secs,
                _ => unreachable!(),
            } = value;
            cases.push((meta, "finite"));
        }
    }
    let mut meta = payload().meta;
    meta.wall_secs = -0.5;
    cases.push((meta, "nonnegative"));
    for storage in ["", "f64", "F32", "f32,extra\nspoof"] {
        let mut meta = payload().meta;
        meta.storage = storage.into();
        cases.push((meta, "storage"));
    }
    for (meta, expected_error) in cases {
        let mut invalid = payload();
        invalid.meta = meta;
        assert!(matches!(
            write_sol(&path, &invalid),
            Err(SolError::InvalidLayout(message)) if message.contains(expected_error)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);

        std::fs::write(&path, replace_meta(&original, &invalid.meta)).unwrap();
        assert!(matches!(
            SolReader::open(&path),
            Err(SolError::InvalidLayout(message)) if message.contains(expected_error)
        ));
        assert!(read_sol(&path).is_err());
        std::fs::write(&path, &original).unwrap();
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn metadata_accepts_both_storage_backends_and_finite_negative_gains() {
    let (_directory, path, original) = fixture();
    for storage in ["f32", "i16"] {
        let mut expected = payload();
        expected.meta.storage = storage.into();
        expected.meta.wall_secs = 0.0;
        expected.meta.ev = [-5.0, 2.0];
        expected.meta.expl = [-1e-12, -2e-12];
        expected.meta.nash_conv = -3e-12;
        write_sol(&path, &expected).unwrap();
        assert_eq!(read_sol(&path).unwrap(), expected);

        // Also prove the hand-built mutation helper produces a valid layout,
        // even when compressed metadata changes length and moves the frames.
        std::fs::write(&path, replace_meta(&original, &expected.meta)).unwrap();
        assert_eq!(read_sol(&path).unwrap(), expected);
    }
}

#[test]
fn writer_and_metadata_reader_reject_srefs_outside_the_declared_tree() {
    let (directory, path, original) = fixture();
    for sref in [3, u32::MAX] {
        let mut invalid = payload();
        invalid.blocks[1].sref = sref;
        invalid.values[1].sref = sref;
        assert!(matches!(
            write_sol(&path, &invalid),
            Err(SolError::InvalidLayout(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);

        let mut bytes = original.clone();
        let entry = directory_start(&bytes) + ENTRY_LEN;
        set_u32(&mut bytes, entry, sref);
        set_u32(&mut bytes, entry + 4, sref);
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(
            SolReader::open(&path),
            Err(SolError::InvalidLayout(_))
        ));
        std::fs::write(&path, &original).unwrap();
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn metadata_only_open_checks_every_directory_entry_and_extent() {
    let (_directory, path, original) = fixture();
    let first = directory_start(&original);
    let second = first + ENTRY_LEN;
    let mut cases = Vec::new();

    let mut duplicate = original.clone();
    set_u32(&mut duplicate, second, 0);
    set_u32(&mut duplicate, second + 4, 0);
    cases.push(("duplicate sref", duplicate));
    let mut descending = original.clone();
    set_u32(&mut descending, first, 2);
    set_u32(&mut descending, first + 4, 2);
    set_u32(&mut descending, second, 1);
    set_u32(&mut descending, second + 4, 1);
    cases.push(("descending sref", descending));

    for (name, offset, value) in [
        (
            "first frame gap",
            first + 16,
            u64_at(&original, first + 16) + 1,
        ),
        (
            "second frame overlap",
            second + 16,
            u64_at(&original, second + 16) - 1,
        ),
        ("metadata extent overflow", METADATA_LENGTH, u64::MAX),
        (
            "directory multiplication overflow",
            DIRECTORY_COUNT,
            u64::MAX,
        ),
    ] {
        let mut bytes = original.clone();
        set_u64(&mut bytes, offset, value);
        cases.push((name, bytes));
    }
    for (name, offset, value) in [
        ("empty chunk", second + 8, 0),
        ("too many chunk nodes", second + 8, 65),
        ("wrong chunk range", second + 4, 1),
        ("nonzero reserved field", second + 12, 1),
        ("empty compressed chunk", second + 24, 0),
        ("excessive compressed chunk", second + 24, u32::MAX),
        ("empty decoded chunk", second + 28, 0),
        (
            "decoded chunk limit",
            second + 28,
            SOL_MAX_NODE_BYTES as u32 + 1,
        ),
    ] {
        let mut bytes = original.clone();
        set_u32(&mut bytes, offset, value);
        cases.push((name, bytes));
    }
    let mut trailing = original;
    trailing.push(0);
    cases.push(("unindexed trailing byte", trailing));

    for (name, bytes) in cases {
        std::fs::write(&path, bytes).unwrap();
        assert!(SolReader::open(&path).is_err(), "accepted {name}");
    }
}

#[test]
fn all_truncated_prefixes_are_rejected_including_metadata_and_directory() {
    let (_directory, path, original) = fixture();
    // The entire artifact is a few hundred bytes; this checks every boundary
    // with bounded allocation and includes cuts inside both compressed frames.
    for cut in 0..original.len() {
        std::fs::write(&path, &original[..cut]).unwrap();
        assert!(
            SolReader::open(&path).is_err(),
            "accepted artifact truncated at byte {cut}"
        );
    }
}

#[test]
fn metadata_hash_length_and_header_identity_are_verified_on_open() {
    let (_directory, path, original) = fixture();
    for (name, offset) in [
        ("metadata BLAKE3 digest", METADATA_DIGEST),
        ("compressed metadata", PREFIX_LEN),
        ("header config identity", 10),
        ("header iteration", 42),
    ] {
        let mut bytes = original.clone();
        bytes[offset] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(SolReader::open(&path).is_err(), "accepted {name}");
    }
    for decoded_len in [
        0,
        u64_at(&original, METADATA_DECODED_LENGTH) - 1,
        u64_at(&original, METADATA_DECODED_LENGTH) + 1,
        SOL_MAX_METADATA_BYTES + 1,
    ] {
        let mut bytes = original.clone();
        set_u64(&mut bytes, METADATA_DECODED_LENGTH, decoded_len);
        std::fs::write(&path, bytes).unwrap();
        assert!(SolReader::open(&path).is_err());
    }
    for compressed_len in [0, SOL_MAX_METADATA_BYTES + 1024 * 1024 + 1] {
        let mut bytes = original.clone();
        set_u64(&mut bytes, METADATA_LENGTH, compressed_len);
        std::fs::write(&path, bytes).unwrap();
        assert!(SolReader::open(&path).is_err());
    }
}

#[test]
fn compressed_sections_reject_trailing_bytes_and_concatenated_empty_frames() {
    let (_directory, path, original) = fixture();
    let directory = directory_start(&original);
    let second = directory + ENTRY_LEN;
    for suffix in [vec![0], zstd::encode_all(&[][..], 0).unwrap()] {
        // The uncompressed length and checksum remain correct. The claimed
        // compressed extent must still contain exactly one complete frame.
        let mut bytes = original.clone();
        bytes.extend_from_slice(&suffix);
        let offset = u64_at(&original, second + 16) as usize;
        let compressed_len = (bytes.len() - offset) as u32;
        set_u32(&mut bytes, second + 24, compressed_len);
        std::fs::write(&path, bytes).unwrap();
        let mut reader = SolReader::open(&path).unwrap();
        assert!(reader.read_node(0).unwrap().is_some());
        assert!(reader.read_node(2).is_err());
        drop(reader);

        let mut bytes = original[..directory].to_vec();
        bytes.extend_from_slice(&suffix);
        bytes.extend_from_slice(&original[directory..]);
        set_u64(
            &mut bytes,
            METADATA_LENGTH,
            u64_at(&original, METADATA_LENGTH) + suffix.len() as u64,
        );
        for index in 0..2 {
            let old_entry = directory + index * ENTRY_LEN;
            let new_entry = old_entry + suffix.len();
            set_u64(
                &mut bytes,
                new_entry + 16,
                u64_at(&original, old_entry + 16) + suffix.len() as u64,
            );
        }
        std::fs::write(&path, bytes).unwrap();
        assert!(SolReader::open(&path).is_err());
    }
}

#[test]
fn node_identity_and_trailing_decoded_bytes_are_checked_lazily() {
    let (_directory, path, original) = fixture();
    let expected = payload();
    for change_strategy in [true, false] {
        let mut strategy = expected.blocks[1].clone();
        let mut value = expected.values[1].clone();
        if change_strategy {
            strategy.sref = 1;
        } else {
            value.sref = 1;
        }
        let raw = postcard::to_allocvec(std::slice::from_ref(&(strategy, value))).unwrap();
        std::fs::write(&path, replace_last_node(&original, &raw)).unwrap();
        let mut reader = SolReader::open(&path).unwrap();
        assert!(reader.read_node(0).unwrap().is_some());
        assert!(matches!(
            reader.read_node(2),
            Err(SolError::InvalidLayout(message)) if message.contains("identity")
        ));
        assert!(read_sol(&path).is_err());
    }
    let mut raw = postcard::to_allocvec(std::slice::from_ref(&(
        &expected.blocks[1],
        &expected.values[1],
    )))
    .unwrap();
    raw.push(0);
    std::fs::write(&path, replace_last_node(&original, &raw)).unwrap();
    let mut reader = SolReader::open(&path).unwrap();
    assert!(matches!(
        reader.read_node(2),
        Err(SolError::InvalidLayout(message)) if message.contains("trailing")
    ));
}

#[test]
fn corrupt_node_digest_does_not_prevent_other_nodes_or_metadata_from_loading() {
    let (_directory, path, mut bytes) = fixture();
    let expected = payload();
    let second = directory_start(&bytes) + ENTRY_LEN;
    bytes[second + 32] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let mut reader = SolReader::open(&path).unwrap();
    assert_eq!(reader.metadata().meta, expected.meta);
    assert!(reader.read_node(2).is_err());
    assert_eq!(
        reader.read_node(0).unwrap().unwrap(),
        (expected.blocks[0].clone(), expected.values[0].clone())
    );
    assert!(reader.read_node(1).unwrap().is_none());
    assert!(reader.read_node(2).is_err());
    assert!(read_sol(&path).is_err());
}

#[test]
fn empty_stored_node_sets_have_no_directory_or_unindexed_bytes() {
    let (directory, path, _original) = fixture();
    let mut expected = payload();
    expected.blocks.clear();
    expected.values.clear();
    for mode in [StreetsStored::Full, StreetsStored::NoRivers] {
        expected.mode = mode;
        write_sol(&path, &expected).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u64_at(&bytes, DIRECTORY_COUNT), 0);
        assert_eq!(bytes.len(), directory_start(&bytes));
        let mut reader = SolReader::open(&path).unwrap();
        assert_eq!(reader.metadata().stored_nodes, 0);
        assert_eq!(reader.stored_srefs().len(), 0);
        assert!(reader.read_node(0).unwrap().is_none());
        assert_eq!(read_sol(&path).unwrap(), expected);
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn failed_persist_preserves_existing_destination_and_removes_completed_tempfile() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("existing-directory.sol");
    std::fs::create_dir(&destination).unwrap();
    let previous = destination.join("previous.sol");
    write_sol(&previous, &payload()).unwrap();
    let original = std::fs::read(&previous).unwrap();

    // Serialization, compression, and sync succeed for this small valid
    // payload. The final rename cannot replace a nonempty directory on either
    // Windows or Unix, so this exercises a failure after writing has started.
    assert!(matches!(
        write_sol(&destination, &payload()),
        Err(SolError::Io(_))
    ));
    assert!(destination.is_dir());
    assert_eq!(std::fs::read(&previous).unwrap(), original);
    assert_eq!(read_sol(&previous).unwrap(), payload());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 1);
}
