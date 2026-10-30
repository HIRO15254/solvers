//! `.sol` viewer-artifact codec: a compact, read-only export of a solved
//! postflop game's *normalized average strategy*, quantized to 16-bit
//! fixed point, plus the original config TOML, the compiled tree with its
//! viewer index (see [`crate::index`]) and cached solve metadata.
//!
//! This is deliberately not a superset of `.ckpt`: `.ckpt` stores
//! full-precision regrets and strategy sums so a solve can resume bit-for-
//! bit, while `.sol` throws that away and keeps only what a viewer needs to
//! render strategies. Readers never rebuild the tree: it is stored. In
//! `NoRivers` mode retains the full tree/index but omits river strategy and
//! value blocks; the viewer re-solves rivers lazily on demand.
//!
//! Format versions 7, 8 and 9 are indexed so a reader never decodes more than it asks
//! for. All integers are little-endian:
//!
//! ```text
//! header | metadata frame | index frame | strategy chunks | value chunks | directory frame
//! ```
//!
//! Every frame and chunk is an independent zstd frame whose BLAKE3 is
//! recorded: the metadata, index and directory hashes in the header, each
//! chunk's in its directory entry. A chunk's decoded bytes are stored byte-
//! shuffled (all even-offset bytes, then all odd-offset bytes), which puts
//! the low and high bytes of its 16-bit values in separate planes. Summary
//! queries read the header, metadata and directory only; a node query also
//! decodes the index and the one chunk holding that node. The file is
//! written to a temp file and renamed into place.

use parking_lot::{Mutex, RwLock};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use runfiles::{config_hash, config_hash_hex};

mod upgrade;
pub use upgrade::upgrade_sol;

/// Fixed header: magic (8) + format version (u16) + config hash (32) +
/// iteration (u64) + metadata frame length (u64) + index frame length
/// (u64) + directory offset (u64) + directory frame length (u64) +
/// metadata, index and directory BLAKE3 (32 each). The first 50 bytes keep
/// the checkpoint header's identity layout.
pub const HEADER_LEN: usize = 8 + 2 + 32 + 8 + 8 + 8 + 8 + 8 + 32 + 32 + 32;

/// Maximum strategy elements generated together (one oversized node is allowed).
pub const STRATEGY_BATCH_ELEMENTS: usize = 8 * 1024 * 1024;

pub(crate) fn strategy_batch_workspace(
    elements: u64,
    min_node: u64,
    max_node: u64,
    nodes: u64,
) -> u64 {
    let batch = elements.min((STRATEGY_BATCH_ELEMENTS as u64).max(max_node));
    // f32 average + quantized u16 bytes per element. Per-node upper bound
    // also includes the batch entry and the temporary average's header.
    let headers =
        std::mem::size_of::<(hu_engine::StorageRef, Vec<u8>)>() + std::mem::size_of::<Vec<f32>>();
    batch * 6
        + nodes.min(batch / min_node.max(1)) * headers as u64
        + std::mem::size_of::<Vec<(hu_engine::StorageRef, Vec<u8>)>>() as u64
}

const MAGIC: &[u8; 8] = b"SLVRSOLV";
const FORMAT_VERSION: u16 = 7;
/// A chunk is sealed once its decoded bytes reach this size. Large enough
/// that independent frames compress about as well as one stream, small
/// enough that a single-node read stays cheap.
pub const CHUNK_TARGET_BYTES: usize = 1 << 20;
/// Sealed chunks compressed in parallel before they are written in order.
const PARALLEL_CHUNKS: usize = 16;
/// Upper bound of one compression context at [`ZSTD_LEVEL`], for a chunk
/// or for the streamed index frame.
const COMPRESSION_CONTEXT_BYTES: u64 = 8 << 20;
const ZSTD_LEVEL: i32 = 3;
/// Largest byte blob one block may declare: far above any real node
/// (`actions * 1326 * 2`), small enough that a corrupt length cannot request
/// an absurd allocation.
const MAX_BLOCK_BYTES: usize = 1 << 26;
/// Chunk entry headers: `sref, len` for strategies, `sref, scale, len` for
/// values, all `u32`/`f32`.
const STRATEGY_ENTRY_BYTES: usize = 8;
const VALUE_ENTRY_BYTES: usize = 12;
const MAX_CHUNK_BYTES: usize = CHUNK_TARGET_BYTES + VALUE_ENTRY_BYTES + MAX_BLOCK_BYTES;
const MAX_METADATA_BYTES: usize = 1 << 26;
const MAX_DIRECTORY_BYTES: usize = 1 << 30;
/// Decoded chunks a reader keeps; node queries walk root-to-node paths, so
/// the ancestors' chunks stay hot.
const CACHED_CHUNKS: usize = 32;

/// Memory the writer holds besides the blocks themselves: the sealed chunks
/// in flight (at most [`PARALLEL_CHUNKS`], and no more than the payload can
/// fill, since a chunk is sealed only once it reaches
/// [`CHUNK_TARGET_BYTES`], plus the final partial one), their shuffled and
/// compressed copies and compression contexts, the open chunk of each
/// section, the strategy `sref` list that value blocks are checked against,
/// and the directory (entries, encoded and compressed). The index frame is
/// streamed before any chunk through one context and a small buffer, which
/// the chunk phase's contexts bound. `max_block_bytes` is the largest
/// strategy or value blob of the tree, `payload_bytes` bounds all blob
/// bytes.
pub(crate) fn compression_workspace(
    max_block_bytes: u64,
    payload_bytes: u64,
    stored_nodes: u64,
) -> u64 {
    let chunk = (CHUNK_TARGET_BYTES + VALUE_ENTRY_BYTES) as u64 + max_block_bytes;
    let full_chunks = payload_bytes / CHUNK_TARGET_BYTES as u64;
    let in_flight = (full_chunks + 1).min(PARALLEL_CHUNKS as u64);
    let chunks = 2 * (full_chunks + 1);
    (3 * in_flight + 4) * chunk
        + in_flight * COMPRESSION_CONTEXT_BYTES
        + stored_nodes * 4
        + chunks * 3 * std::mem::size_of::<ChunkEntry>() as u64
}

/// Additional workspace for the optional no-rivers profile constraint stream.
/// Every node can contain fixed and bonus arrays (one f32 per action/hand),
/// confidence (one f32 per hand), postcard headers, and a chunk entry.
pub(crate) fn river_constraint_workspace(
    max_strategy_elements: u64,
    strategy_elements: u64,
    widest_hands: u64,
    action_nodes: u64,
) -> u64 {
    let block = max_strategy_elements
        .saturating_mul(8)
        .saturating_add(widest_hands.saturating_mul(4))
        .saturating_add(64);
    let payload = strategy_elements.saturating_mul(8).saturating_add(
        action_nodes.saturating_mul(widest_hands.saturating_mul(4).saturating_add(72)),
    );
    compression_workspace(block, payload, action_nodes).saturating_add(block.saturating_mul(2))
}

/// Errors from reading or writing a `.sol` file.
#[derive(Debug, thiserror::Error)]
pub enum SolError {
    #[error("not a solvers .sol file (bad magic bytes)")]
    BadMagic,
    #[error(
        "unsupported .sol format version {found} (expected {expected}, 8 or 9); use solvers upgrade for versions 4–6; versions 1–3 require a re-solve (docs/hu-postflop.jp.md)"
    )]
    BadVersion { found: u16, expected: u16 },
    #[error(".sol file is already current")]
    AlreadyCurrent,
    #[error("invalid embedded .sol config: {0}")]
    Config(String),
    #[error(".sol file truncated: expected at least {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
    #[error(".sol header hash {header_hash} does not match blake3(config_toml) {computed_hash}")]
    HashMismatch {
        header_hash: String,
        computed_hash: String,
    },
    #[error(".sol {0} BLAKE3 checksum mismatch")]
    Checksum(&'static str),
    #[error(".sol I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error(".sol payload codec error: {0}")]
    Codec(#[from] postcard::Error),
    #[error("malformed .sol file: {0}")]
    Malformed(&'static str),
}

/// Cached solve metadata: results a viewer wants to display immediately
/// without recomputing exploitability itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolMeta {
    pub iterations: u64,
    pub expl: [f64; 2],
    pub ev: [f64; 2],
    pub nash_conv: f64,
    /// Informational only: the storage representation ("f32" | "i16" | "i16-f32avg") the
    /// solve ran with. Never round-tripped back into `hu_engine::StorageState`
    /// — `.sol` always quantizes to u16 fixed point regardless of this.
    pub storage: String,
    /// Informational only: wall-clock seconds the solve took.
    pub wall_secs: f64,
}

/// Which streets have stored strategy/value blocks. `NoRivers` retains the
/// full tree but omits river blocks; the viewer re-solves those subtrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreetsStored {
    Full,
    NoRivers,
}

/// Everything except the blocks: the metadata frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolMetadata {
    /// Full original config TOML text, kept verbatim (not just its hash):
    /// ranges, board and payoff for the stored tree, and the settings a
    /// river re-solve reuses.
    /// The header's config hash MUST equal `blake3(config_toml.as_bytes())`
    /// — the writer derives it from this field and the reader verifies it,
    /// so a `.sol` can never silently drift from the config it describes.
    pub config_toml: String,
    pub meta: SolMeta,
    pub mode: StreetsStored,
    /// Node count of the stored tree.
    pub tree_nodes: u64,
    /// Stored action nodes; strategy and value chunks each hold this many
    /// blocks.
    pub stored_nodes: u64,
}

/// One action node's quantized strategy. `probs` is action-major (all
/// root-support hands for action 0, then action 1, ...) little-endian u16
/// fixed point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyBlock {
    /// Index into the stored tree's `storage_refs` (i.e. the action node's
    /// `Node::aux`).
    pub sref: u32,
    pub probs: Vec<u8>,
}

/// One action node's per-hand values on the original subgame-start basis.
/// The utility-valued baseline offset is identical at every node; wagers
/// already made in the subgame remain costs. Values use chips for chip EV
/// and prize units for ICM, including tournament ICM.
///
/// Stored rather than recomputed because a `NoRivers` artifact cannot
/// recompute them: its river strategies are gone, and the viewer's lazy
/// re-solve produces different play and therefore different values. Even
/// for `Full` artifacts, recomputing means a whole value pass per query.
///
/// A hand that cannot be held at the node is stored as zero. A
/// counterfactual value exists for every hand whether or not it can arrive
/// there, so keeping them would make these blocks dense where the strategy
/// blocks are sparse — most of an artifact's size — for numbers no reader
/// wants.
/// Quantized to `i16` against a per-block scale, the way strategies are
/// quantized to `u16` against a fixed denominator: a node's values are
/// bounded by the chips it can still win or lose, so one scale per block
/// carries them at about one part in 32,767 — far finer than any solve's
/// own convergence error, at half the bytes of `f32` and with much better
/// compression, since the low mantissa bits that survive `f32` are noise
/// no reader can use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueBlock {
    /// Same `sref` space as [`StrategyBlock`].
    pub sref: u32,
    /// Chips (or prize units) one quantization step represents. Zero when
    /// every value in the block is zero.
    pub scale: f32,
    /// OOP's compact support values then IP's (seat dimensions may differ),
    /// little-endian `i16` multiples of `scale`.
    pub values: Vec<u8>,
}

/// The full decoded `.sol` contents, for tools that need every block.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolPayload {
    pub config_toml: String,
    pub meta: SolMeta,
    pub mode: StreetsStored,
    pub tree_nodes: u64,
    /// One entry per stored action node, ascending by `sref`.
    pub blocks: Vec<StrategyBlock>,
    /// Per-hand values for the same nodes as `blocks`, ascending by
    /// `sref`. The two lists always cover the same node set: a reader that
    /// found a strategy for a node can always find its values too.
    pub values: Vec<ValueBlock>,
    /// The decoded index stream (see [`crate::index`]).
    pub index: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SolHeader {
    version: u16,
    config_hash: [u8; 32],
    iteration: u64,
    metadata_len: u64,
    index_len: u64,
    directory_offset: u64,
    directory_len: u64,
    metadata_checksum: [u8; 32],
    index_checksum: [u8; 32],
    directory_checksum: [u8; 32],
}

/// Checks the magic bytes and format version, which every version keeps at
/// the start of the file.
fn check_identity(buf: &[u8; 10]) -> Result<(), SolError> {
    if &buf[0..8] != MAGIC {
        return Err(SolError::BadMagic);
    }
    let version = u16::from_le_bytes([buf[8], buf[9]]);
    if version != FORMAT_VERSION && version != 8 && version != 9 {
        return Err(SolError::BadVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }
    Ok(())
}

fn parse_header(buf: &[u8; HEADER_LEN]) -> SolHeader {
    let u64_at = |at: usize| u64::from_le_bytes(buf[at..at + 8].try_into().expect("8 bytes"));
    let hash_at = |at: usize| -> [u8; 32] { buf[at..at + 32].try_into().expect("32 bytes") };
    SolHeader {
        version: u16::from_le_bytes([buf[8], buf[9]]),
        config_hash: hash_at(10),
        iteration: u64_at(42),
        metadata_len: u64_at(50),
        index_len: u64_at(58),
        directory_offset: u64_at(66),
        directory_len: u64_at(74),
        metadata_checksum: hash_at(82),
        index_checksum: hash_at(114),
        directory_checksum: hash_at(146),
    }
}

fn build_header(header: &SolHeader) -> [u8; HEADER_LEN] {
    let mut buf = [0u8; HEADER_LEN];
    buf[0..8].copy_from_slice(MAGIC);
    buf[8..10].copy_from_slice(&header.version.to_le_bytes());
    buf[10..42].copy_from_slice(&header.config_hash);
    buf[42..50].copy_from_slice(&header.iteration.to_le_bytes());
    buf[50..58].copy_from_slice(&header.metadata_len.to_le_bytes());
    buf[58..66].copy_from_slice(&header.index_len.to_le_bytes());
    buf[66..74].copy_from_slice(&header.directory_offset.to_le_bytes());
    buf[74..82].copy_from_slice(&header.directory_len.to_le_bytes());
    buf[82..114].copy_from_slice(&header.metadata_checksum);
    buf[114..146].copy_from_slice(&header.index_checksum);
    buf[146..178].copy_from_slice(&header.directory_checksum);
    buf
}

/// Directory entry of one chunk: a run of consecutive stored `sref`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ChunkEntry {
    first_sref: u32,
    last_sref: u32,
    nodes: u32,
    offset: u64,
    compressed_len: u64,
    decoded_len: u64,
    checksum: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RiverConstraint {
    pub fixed: Option<Vec<f32>>,
    pub confidence: Option<Vec<f32>>,
    pub bonus: Option<Vec<f32>>,
}
#[derive(Serialize)]
pub(crate) struct RiverConstraintRef<'a> {
    pub fixed: Option<&'a [f32]>,
    pub confidence: Option<&'a [f32]>,
    pub bonus: Option<&'a [f32]>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Directory {
    strategy: Vec<ChunkEntry>,
    values: Vec<ChunkEntry>,
    #[serde(skip)]
    constraints: Vec<ChunkEntry>,
}

impl Directory {
    fn entries(&self, section: Section) -> &[ChunkEntry] {
        match section {
            Section::Strategy => &self.strategy,
            Section::Values => &self.values,
            Section::Constraints => &self.constraints,
        }
    }

    /// Index of the chunk whose `sref` range covers `sref`.
    fn chunk_of(&self, section: Section, sref: u32) -> Option<usize> {
        let entries = self.entries(section);
        let index = entries.partition_point(|e| e.last_sref < sref);
        (index < entries.len() && entries[index].first_sref <= sref).then_some(index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Strategy,
    Values,
    Constraints,
}

impl Section {
    fn entry_bytes(self) -> usize {
        match self {
            Section::Strategy | Section::Constraints => STRATEGY_ENTRY_BYTES,
            Section::Values => VALUE_ENTRY_BYTES,
        }
    }
}

/// A decoded chunk and the position of each block in it.
struct Chunk {
    bytes: Vec<u8>,
    blocks: Vec<ChunkBlock>,
}

struct ChunkBlock {
    sref: u32,
    scale: f32,
    start: usize,
    len: usize,
}

impl Chunk {
    fn parse(section: Section, entry: &ChunkEntry, bytes: Vec<u8>) -> Result<Self, SolError> {
        let mut blocks = Vec::with_capacity(entry.nodes as usize);
        let mut at = 0;
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
        while at < bytes.len() {
            if bytes.len() - at < section.entry_bytes() {
                return Err(SolError::Malformed("chunk entry header is truncated"));
            }
            let sref = word(at);
            let (scale, len) = match section {
                Section::Strategy | Section::Constraints => (0.0, word(at + 4)),
                Section::Values => (f32::from_bits(word(at + 4)), word(at + 8)),
            };
            let start = at + section.entry_bytes();
            let len = len as usize;
            if bytes.len() - start < len {
                return Err(SolError::Malformed("chunk block is truncated"));
            }
            if blocks
                .last()
                .is_some_and(|last: &ChunkBlock| sref <= last.sref)
            {
                return Err(SolError::Malformed(
                    "chunk blocks are not ascending by sref",
                ));
            }
            blocks.push(ChunkBlock {
                sref,
                scale,
                start,
                len,
            });
            at = start + len;
        }
        if blocks.len() != entry.nodes as usize
            || blocks.first().map(|b| b.sref) != Some(entry.first_sref)
            || blocks.last().map(|b| b.sref) != Some(entry.last_sref)
        {
            return Err(SolError::Malformed(
                "chunk contents disagree with the directory",
            ));
        }
        Ok(Self { bytes, blocks })
    }

    fn block(&self, sref: u32) -> Option<(f32, &[u8])> {
        let i = self.blocks.binary_search_by_key(&sref, |b| b.sref).ok()?;
        let b = &self.blocks[i];
        Some((b.scale, &self.bytes[b.start..b.start + b.len]))
    }
}

fn decompress(compressed: &[u8], limit: usize, what: &'static str) -> Result<Vec<u8>, SolError> {
    zstd::bulk::decompress(compressed, limit).map_err(|_| SolError::Malformed(what))
}

/// Random access to a `.sol` file: header, metadata and directory are read
/// and verified at open; the index and chunks are read, verified and decoded
/// on demand.
pub struct SolReader {
    file: File,
    metadata: SolMetadata,
    profile: Option<crate::run::ProfileReport>,
    directory: Directory,
    index_offset: u64,
    index_len: u64,
    index_checksum: [u8; 32],
}

impl SolReader {
    pub fn open(path: &Path) -> Result<Self, SolError> {
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        let mut identity = [0u8; 10];
        if file_len >= identity.len() as u64 {
            file.read_exact(&mut identity)?;
            check_identity(&identity)?;
        }
        if file_len < HEADER_LEN as u64 {
            return Err(SolError::Truncated {
                expected: HEADER_LEN,
                actual: file_len as usize,
            });
        }
        let mut header_buf = [0u8; HEADER_LEN];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header_buf)?;
        let header = parse_header(&header_buf);

        let index_offset = HEADER_LEN as u64 + header.metadata_len;
        let chunks_start = (HEADER_LEN as u64)
            .checked_add(header.metadata_len)
            .and_then(|end| end.checked_add(header.index_len))
            .filter(|&end| end <= header.directory_offset)
            .ok_or(SolError::Malformed(
                "metadata or index frame overlaps the directory",
            ))?;
        if header.directory_offset.checked_add(header.directory_len) != Some(file_len) {
            return Err(SolError::Malformed(
                "directory frame does not end the file (truncated or extended)",
            ));
        }
        let metadata_frame = read_exact_at(&mut file, HEADER_LEN as u64, header.metadata_len)?;
        if *blake3::hash(&metadata_frame).as_bytes() != header.metadata_checksum {
            return Err(SolError::Checksum("metadata"));
        }
        let decoded = decompress(
            &metadata_frame,
            MAX_METADATA_BYTES,
            "metadata frame does not decompress",
        )?;
        let (metadata, profile): (SolMetadata, Option<crate::run::ProfileReport>) =
            if header.version == 8 || header.version == 9 {
                let (metadata, profile) = postcard::from_bytes(&decoded)?;
                (metadata, Some(profile))
            } else {
                (postcard::from_bytes(&decoded)?, None)
            };
        let computed = config_hash(metadata.config_toml.as_bytes());
        if computed != header.config_hash {
            return Err(SolError::HashMismatch {
                header_hash: config_hash_hex(&header.config_hash),
                computed_hash: config_hash_hex(&computed),
            });
        }
        if metadata.meta.iterations != header.iteration {
            return Err(SolError::Malformed(
                "header iteration differs from the metadata",
            ));
        }

        let directory_frame =
            read_exact_at(&mut file, header.directory_offset, header.directory_len)?;
        if *blake3::hash(&directory_frame).as_bytes() != header.directory_checksum {
            return Err(SolError::Checksum("directory"));
        }
        let decoded_directory = decompress(
            &directory_frame,
            MAX_DIRECTORY_BYTES,
            "directory frame does not decompress",
        )?;
        let directory: Directory = if header.version == 9 {
            let (mut directory, constraints): (Directory, Vec<ChunkEntry>) =
                postcard::from_bytes(&decoded_directory)?;
            directory.constraints = constraints;
            directory
        } else {
            postcard::from_bytes(&decoded_directory)?
        };
        let mut offset = chunks_start;
        for (entries, expected_count) in [
            (&directory.strategy, true),
            (&directory.values, true),
            (&directory.constraints, false),
        ] {
            let mut nodes = 0u64;
            let mut previous: Option<u32> = None;
            for entry in entries {
                if entry.offset != offset
                    || entry.nodes == 0
                    || entry.first_sref > entry.last_sref
                    || u64::from(entry.last_sref - entry.first_sref) < u64::from(entry.nodes) - 1
                    || previous.is_some_and(|last| entry.first_sref <= last)
                    || entry.decoded_len > MAX_CHUNK_BYTES as u64
                {
                    return Err(SolError::Malformed("inconsistent chunk directory"));
                }
                previous = Some(entry.last_sref);
                nodes += u64::from(entry.nodes);
                offset = offset
                    .checked_add(entry.compressed_len)
                    .ok_or(SolError::Malformed("inconsistent chunk directory"))?;
            }
            if expected_count && nodes != metadata.stored_nodes {
                return Err(SolError::Malformed(
                    "chunk directory and metadata disagree on the stored node count",
                ));
            }
        }
        if offset != header.directory_offset {
            return Err(SolError::Malformed(
                "chunks do not end where the directory starts",
            ));
        }
        Ok(Self {
            file,
            metadata,
            profile,
            directory,
            index_offset,
            index_len: header.index_len,
            index_checksum: header.index_checksum,
        })
    }

    pub fn metadata(&self) -> &SolMetadata {
        &self.metadata
    }

    /// Profile diagnostics, present in profile-bearing version 8 and 9 artifacts.
    pub fn profile(&self) -> Option<&crate::run::ProfileReport> {
        self.profile.as_ref()
    }

    pub(crate) fn index_checksum(&self) -> [u8; 32] {
        self.index_checksum
    }

    pub(crate) fn verify_index(&mut self) -> Result<(), SolError> {
        self.index_frame().map(|_| ())
    }

    fn index_frame(&mut self) -> Result<Vec<u8>, SolError> {
        let frame = read_exact_at(&mut self.file, self.index_offset, self.index_len)?;
        if *blake3::hash(&frame).as_bytes() != self.index_checksum {
            return Err(SolError::Checksum("index"));
        }
        Ok(frame)
    }

    pub fn into_metadata(self) -> SolMetadata {
        self.metadata
    }

    /// Verifies the index frame and hands its decoded stream to `decode`,
    /// which must consume all of it.
    pub fn read_index<T>(
        &mut self,
        decode: impl FnOnce(&mut dyn Read) -> Result<T, SolError>,
    ) -> Result<T, SolError> {
        let frame = self.index_frame()?;
        let mut decoder = zstd::stream::read::Decoder::with_buffer(frame.as_slice())
            .map_err(|_| SolError::Malformed("index frame does not decompress"))?
            .single_frame();
        decode(&mut decoder)
    }

    fn entries(&self, section: Section) -> &[ChunkEntry] {
        self.directory.entries(section)
    }

    fn read_chunk(&mut self, section: Section, index: usize) -> Result<Chunk, SolError> {
        let entry = self.entries(section)[index];
        let compressed = read_exact_at(&mut self.file, entry.offset, entry.compressed_len)?;
        decode_chunk(section, &entry, &compressed)
    }

    /// Decodes every chunk in order, strategies first, then values.
    pub fn visit_blocks(
        &mut self,
        mut strategy: impl FnMut(u32, &[u8]),
        mut values: impl FnMut(u32, f32, &[u8]),
    ) -> Result<(), SolError> {
        for index in 0..self.directory.strategy.len() {
            let chunk = self.read_chunk(Section::Strategy, index)?;
            for b in &chunk.blocks {
                strategy(b.sref, &chunk.bytes[b.start..b.start + b.len]);
            }
        }
        for index in 0..self.directory.values.len() {
            let chunk = self.read_chunk(Section::Values, index)?;
            for b in &chunk.blocks {
                values(b.sref, b.scale, &chunk.bytes[b.start..b.start + b.len]);
            }
        }
        Ok(())
    }

    /// Decodes the whole file. Verifies that strategy and value blocks cover
    /// the same nodes.
    #[cfg(any(test, feature = "test-support"))]
    pub fn read_payload(mut self) -> Result<SolPayload, SolError> {
        let mut blocks = Vec::new();
        let mut values = Vec::new();
        self.visit_blocks(
            |sref, probs| {
                blocks.push(StrategyBlock {
                    sref,
                    probs: probs.to_vec(),
                })
            },
            |sref, scale, bytes| {
                values.push(ValueBlock {
                    sref,
                    scale,
                    values: bytes.to_vec(),
                })
            },
        )?;
        if !blocks
            .iter()
            .map(|b| b.sref)
            .eq(values.iter().map(|v| v.sref))
        {
            return Err(SolError::Malformed(
                "strategy and value blocks cover different nodes",
            ));
        }
        let index = self.read_index(|stream| {
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes)?;
            Ok(bytes)
        })?;
        let metadata = self.metadata;
        Ok(SolPayload {
            config_toml: metadata.config_toml,
            meta: metadata.meta,
            mode: metadata.mode,
            tree_nodes: metadata.tree_nodes,
            blocks,
            values,
            index,
        })
    }

    pub fn into_store(self) -> SolStore {
        SolStore {
            file: Mutex::new(self.file),
            directory: self.directory,
            cache: Mutex::new(Vec::with_capacity(CACHED_CHUNKS)),
            pinned_strategies: RwLock::new(None),
        }
    }
}

/// Verifies and decodes one chunk frame read from `entry.offset`.
fn decode_chunk(
    section: Section,
    entry: &ChunkEntry,
    compressed: &[u8],
) -> Result<Chunk, SolError> {
    if *blake3::hash(compressed).as_bytes() != entry.checksum {
        return Err(SolError::Checksum("chunk"));
    }
    let shuffled = decompress(
        compressed,
        entry.decoded_len as usize,
        "chunk does not decompress",
    )?;
    if shuffled.len() as u64 != entry.decoded_len {
        return Err(SolError::Malformed("chunk decoded length differs"));
    }
    Chunk::parse(section, entry, unshuffle(&shuffled))
}

/// Splits bytes into the even-offset plane followed by the odd-offset plane.
fn shuffle(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    out.extend(raw.iter().step_by(2));
    out.extend(raw.iter().skip(1).step_by(2));
    out
}

/// Inverse of [`shuffle`].
fn unshuffle(planes: &[u8]) -> Vec<u8> {
    let (even, odd) = planes.split_at(planes.len().div_ceil(2));
    let mut out = vec![0u8; planes.len()];
    for (slot, &byte) in out.iter_mut().step_by(2).zip(even) {
        *slot = byte;
    }
    for (slot, &byte) in out.iter_mut().skip(1).step_by(2).zip(odd) {
        *slot = byte;
    }
    out
}

fn read_exact_at(file: &mut File, offset: u64, len: u64) -> Result<Vec<u8>, SolError> {
    let len = usize::try_from(len).map_err(|_| SolError::Malformed("frame length overflows"))?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Quantized blocks of a `.sol` file, decoded chunk by chunk as nodes are
/// queried and dequantized per query. Shared by reference and safe to query
/// from many threads: only file reads and cache bookkeeping serialize; chunk
/// decoding and dequantization run concurrently, and threads that need the
/// same chunk wait for one decode instead of repeating it.
pub struct SolStore {
    file: Mutex<File>,
    directory: Directory,
    /// Most recently used last.
    cache: Mutex<Vec<(Section, usize, Arc<ChunkSlot>)>>,
    pinned_strategies: RwLock<Option<Vec<Arc<Chunk>>>>,
}

/// One cached chunk; empty until its first decode succeeds.
type ChunkSlot = Mutex<Option<Arc<Chunk>>>;

impl SolStore {
    /// Decode the strategy section once for parallel whole-tree passes.
    /// Callers release it while only subtree iterations are running.
    pub(crate) fn pin_strategies(&self) -> Result<(), SolError> {
        let mut chunks = Vec::with_capacity(self.directory.strategy.len());
        for index in 0..self.directory.strategy.len() {
            chunks.push(self.chunk(Section::Strategy, index)?);
        }
        *self.pinned_strategies.write() = Some(chunks);
        Ok(())
    }

    pub(crate) fn unpin_strategies(&self) {
        *self.pinned_strategies.write() = None;
    }

    fn chunk(&self, section: Section, index: usize) -> Result<Arc<Chunk>, SolError> {
        let slot = {
            let mut cache = self.cache.lock();
            let slot = match cache
                .iter()
                .position(|(s, i, _)| *s == section && *i == index)
            {
                Some(position) => cache.remove(position).2,
                None => {
                    if cache.len() == CACHED_CHUNKS {
                        cache.remove(0);
                    }
                    Arc::default()
                }
            };
            cache.push((section, index, Arc::clone(&slot)));
            slot
        };
        let mut decoded = slot.lock();
        if let Some(chunk) = &*decoded {
            return Ok(Arc::clone(chunk));
        }
        let entry = self.directory.entries(section)[index];
        let compressed = read_exact_at(&mut self.file.lock(), entry.offset, entry.compressed_len)?;
        let chunk = Arc::new(decode_chunk(section, &entry, &compressed)?);
        *decoded = Some(Arc::clone(&chunk));
        Ok(chunk)
    }

    fn with_block<T>(
        &self,
        section: Section,
        sref: u32,
        read: impl FnOnce(f32, &[u8]) -> Result<T, SolError>,
    ) -> Result<Option<T>, SolError> {
        let Some(index) = self.directory.chunk_of(section, sref) else {
            return Ok(None);
        };
        if section == Section::Strategy
            && let Some(chunks) = self.pinned_strategies.read().as_ref()
        {
            return chunks[index]
                .block(sref)
                .map(|(scale, bytes)| read(scale, bytes))
                .transpose();
        }
        self.chunk(section, index)?
            .block(sref)
            .map(|(scale, bytes)| read(scale, bytes))
            .transpose()
    }

    /// Dequantized `num_actions * num_hands` action-major strategy, or `None`
    /// when the file stores no block for `sref`.
    pub fn strategy(
        &self,
        sref: u32,
        num_actions: usize,
        num_hands: usize,
    ) -> Result<Option<Vec<f32>>, SolError> {
        self.with_block(Section::Strategy, sref, |_, bytes| {
            dequantize_probs(bytes, num_actions, num_hands)
        })
    }

    /// Dequantized `len` per-hand values (OOP's support then IP's), or `None`
    /// when the file stores no block for `sref`.
    pub fn values(&self, sref: u32, len: usize) -> Result<Option<Vec<f32>>, SolError> {
        self.with_block(Section::Values, sref, |scale, bytes| {
            dequantize_values(bytes, scale, len)
        })
    }
    pub(crate) fn river_constraint(&self, sref: u32) -> Result<Option<RiverConstraint>, SolError> {
        self.with_block(Section::Constraints, sref, |_, bytes| {
            Ok(postcard::from_bytes(bytes)?)
        })
    }

    pub(crate) fn strategy_into(
        &self,
        sref: u32,
        num_actions: usize,
        num_hands: usize,
        out: &mut Vec<f32>,
    ) -> Result<Option<()>, SolError> {
        self.with_block(Section::Strategy, sref, |_, bytes| {
            dequantize_probs_into(bytes, num_actions, num_hands, out)
        })
    }

    pub(crate) fn strategy_into_slice(
        &self,
        sref: u32,
        num_actions: usize,
        num_hands: usize,
        out: &mut [f32],
    ) -> Result<Option<()>, SolError> {
        self.with_block(Section::Strategy, sref, |_, bytes| {
            dequantize_probs_into_slice(bytes, num_actions, num_hands, out)
        })
    }

    pub(crate) fn values_into(
        &self,
        sref: u32,
        len: usize,
        out: &mut Vec<f32>,
    ) -> Result<Option<()>, SolError> {
        self.with_block(Section::Values, sref, |scale, bytes| {
            dequantize_values_into(bytes, scale, len, out)
        })
    }
}

/// Reads only the header, metadata and directory, returning the verified
/// embedded config.
pub fn read_sol_config(path: &Path) -> Result<String, SolError> {
    Ok(SolReader::open(path)?.into_metadata().config_toml)
}

/// Reads and fully decodes a `.sol` file.
#[cfg(any(test, feature = "test-support"))]
pub fn read_sol(path: &Path) -> Result<SolPayload, SolError> {
    SolReader::open(path)?.read_payload()
}

/// Writes a whole payload as a `.sol` file; see [`SolFileWriter`].
#[cfg(any(test, feature = "test-support"))]
pub fn write_sol(path: &Path, payload: &SolPayload) -> Result<(), SolError> {
    let mut writer = SolFileWriter::create(
        path,
        &SolMetadata {
            config_toml: payload.config_toml.clone(),
            meta: payload.meta.clone(),
            mode: payload.mode,
            tree_nodes: payload.tree_nodes,
            stored_nodes: payload.blocks.len() as u64,
        },
        |out| out.write_all(&payload.index),
    )?;
    for block in &payload.blocks {
        writer.strategy(block.sref, &block.probs)?;
    }
    for block in &payload.values {
        writer.values(block.sref, block.scale, &block.values)?;
    }
    writer.finish()
}

/// Streaming `.sol` writer. Strategy blocks arrive first, ascending by
/// `sref`, then value blocks for the same nodes in the same order. The file
/// is written to a temp file beside `path`, `fsync`ed, then renamed into
/// place; the header (config hash, iteration, frame offsets and hashes) is
/// written last, derived from the metadata so caller and header can never
/// disagree.
pub(crate) struct SolFileWriter {
    file: tempfile::NamedTempFile,
    path: PathBuf,
    header: SolHeader,
    stored_nodes: u64,
    offset: u64,
    strategy: SectionWriter,
    values: SectionWriter,
    constraints: SectionWriter,
    last_constraint: Option<u32>,
    strategy_srefs: Vec<u32>,
    values_written: usize,
}

struct SectionWriter {
    section: Section,
    raw: Vec<u8>,
    first: u32,
    last: u32,
    nodes: u32,
    sealed: Vec<(Vec<u8>, u32, u32, u32)>,
    entries: Vec<ChunkEntry>,
}

impl SectionWriter {
    fn new(section: Section) -> Self {
        Self {
            section,
            raw: Vec::new(),
            first: 0,
            last: 0,
            nodes: 0,
            sealed: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn push(
        &mut self,
        file: &mut File,
        offset: &mut u64,
        sref: u32,
        scale: Option<f32>,
        bytes: &[u8],
    ) -> Result<(), SolError> {
        if bytes.len() > MAX_BLOCK_BYTES {
            return Err(SolError::Malformed("block exceeds the format's size limit"));
        }
        if self.nodes == 0 {
            self.first = sref;
            self.raw
                .reserve(CHUNK_TARGET_BYTES + self.section.entry_bytes() + bytes.len());
        }
        self.raw.extend_from_slice(&sref.to_le_bytes());
        if let Some(scale) = scale {
            self.raw.extend_from_slice(&scale.to_le_bytes());
        }
        self.raw
            .extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.raw.extend_from_slice(bytes);
        self.last = sref;
        self.nodes += 1;
        if self.raw.len() >= CHUNK_TARGET_BYTES {
            self.seal();
            if self.sealed.len() == PARALLEL_CHUNKS {
                self.write_sealed(file, offset)?;
            }
        }
        Ok(())
    }

    fn seal(&mut self) {
        if self.nodes > 0 {
            self.sealed.push((
                std::mem::take(&mut self.raw),
                self.first,
                self.last,
                self.nodes,
            ));
            self.nodes = 0;
        }
    }

    fn write_sealed(&mut self, file: &mut File, offset: &mut u64) -> Result<(), SolError> {
        let compressed = self
            .sealed
            .par_iter()
            .map(|(raw, ..)| zstd::bulk::compress(&shuffle(raw), ZSTD_LEVEL))
            .collect::<Result<Vec<_>, _>>()?;
        for ((raw, first_sref, last_sref, nodes), frame) in self.sealed.drain(..).zip(compressed) {
            file.write_all(&frame)?;
            self.entries.push(ChunkEntry {
                first_sref,
                last_sref,
                nodes,
                offset: *offset,
                compressed_len: frame.len() as u64,
                decoded_len: raw.len() as u64,
                checksum: *blake3::hash(&frame).as_bytes(),
            });
            *offset += frame.len() as u64;
        }
        Ok(())
    }

    fn finish(&mut self, file: &mut File, offset: &mut u64) -> Result<(), SolError> {
        self.seal();
        self.write_sealed(file, offset)
    }
}

/// Counts and hashes the bytes it passes to a file.
struct HashingWriter<'a> {
    file: &'a mut File,
    hasher: blake3::Hasher,
    len: u64,
}

impl Write for HashingWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.file.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.len += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl SolFileWriter {
    /// Starts a file with its metadata frame and the index frame that
    /// `write_index` streams (see [`crate::index`]).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn create(
        path: &Path,
        metadata: &SolMetadata,
        write_index: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<Self, SolError> {
        Self::create_with_profile(path, metadata, None, write_index)
    }

    pub(crate) fn create_with_profile(
        path: &Path,
        metadata: &SolMetadata,
        profile: Option<&crate::run::ProfileReport>,
        write_index: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<Self, SolError> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(&[0; HEADER_LEN])?;
        let bytes = match profile {
            Some(profile) => postcard::to_allocvec(&(metadata, profile))?,
            None => postcard::to_allocvec(metadata)?,
        };
        let frame = zstd::bulk::compress(&bytes, ZSTD_LEVEL)?;
        file.write_all(&frame)?;
        let mut encoder = zstd::stream::write::Encoder::new(
            HashingWriter {
                file: file.as_file_mut(),
                hasher: blake3::Hasher::new(),
                len: 0,
            },
            ZSTD_LEVEL,
        )?;
        write_index(&mut encoder)?;
        let index = encoder.finish()?;
        let (index_len, index_checksum) = (index.len, *index.hasher.finalize().as_bytes());
        Ok(Self {
            file,
            path: path.to_path_buf(),
            header: SolHeader {
                version: if profile.is_some() { 8 } else { FORMAT_VERSION },
                config_hash: config_hash(metadata.config_toml.as_bytes()),
                iteration: metadata.meta.iterations,
                metadata_len: frame.len() as u64,
                index_len,
                directory_offset: 0,
                directory_len: 0,
                metadata_checksum: *blake3::hash(&frame).as_bytes(),
                index_checksum,
                directory_checksum: [0; 32],
            },
            stored_nodes: metadata.stored_nodes,
            offset: HEADER_LEN as u64 + frame.len() as u64 + index_len,
            strategy: SectionWriter::new(Section::Strategy),
            values: SectionWriter::new(Section::Values),
            constraints: SectionWriter::new(Section::Constraints),
            last_constraint: None,
            strategy_srefs: Vec::with_capacity(metadata.stored_nodes.min(1 << 24) as usize),
            values_written: 0,
        })
    }

    pub(crate) fn strategy(&mut self, sref: u32, probs: &[u8]) -> Result<(), SolError> {
        if self.values_written > 0 {
            return Err(SolError::Malformed("strategy block after value blocks"));
        }
        if self.strategy_srefs.last().is_some_and(|&last| sref <= last) {
            return Err(SolError::Malformed(
                "strategy blocks are not ascending by sref",
            ));
        }
        self.strategy_srefs.push(sref);
        self.strategy
            .push(self.file.as_file_mut(), &mut self.offset, sref, None, probs)
    }

    pub(crate) fn values(&mut self, sref: u32, scale: f32, values: &[u8]) -> Result<(), SolError> {
        if self.values_written == 0 {
            self.strategy
                .finish(self.file.as_file_mut(), &mut self.offset)?;
        }
        if self.strategy_srefs.get(self.values_written) != Some(&sref) {
            return Err(SolError::Malformed(
                "value blocks do not follow the strategy blocks' nodes",
            ));
        }
        self.values_written += 1;
        self.values.push(
            self.file.as_file_mut(),
            &mut self.offset,
            sref,
            Some(scale),
            values,
        )
    }
    pub(crate) fn river_constraint(
        &mut self,
        sref: u32,
        constraint: &impl Serialize,
    ) -> Result<(), SolError> {
        if self.values_written as u64 != self.stored_nodes {
            return Err(SolError::Malformed("river constraint before values"));
        }
        if self.last_constraint.is_none() {
            self.values
                .finish(self.file.as_file_mut(), &mut self.offset)?;
            self.header.version = 9;
        }
        if self.last_constraint.is_some_and(|last| sref <= last) {
            return Err(SolError::Malformed("river constraints are not ascending"));
        }
        self.last_constraint = Some(sref);
        let bytes = postcard::to_allocvec(constraint)?;
        self.constraints.push(
            self.file.as_file_mut(),
            &mut self.offset,
            sref,
            None,
            &bytes,
        )
    }

    pub(crate) fn finish(mut self) -> Result<(), SolError> {
        let file = self.file.as_file_mut();
        self.strategy.finish(file, &mut self.offset)?;
        self.values.finish(file, &mut self.offset)?;
        self.constraints.finish(file, &mut self.offset)?;
        if self.strategy_srefs.len() as u64 != self.stored_nodes
            || self.values_written as u64 != self.stored_nodes
        {
            return Err(SolError::Malformed(
                "written blocks differ from the declared stored node count",
            ));
        }
        let directory = Directory {
            strategy: std::mem::take(&mut self.strategy.entries),
            values: std::mem::take(&mut self.values.entries),
            constraints: std::mem::take(&mut self.constraints.entries),
        };
        let encoded = if self.header.version == 9 {
            postcard::to_allocvec(&(&directory, &directory.constraints))?
        } else {
            postcard::to_allocvec(&directory)?
        };
        let frame = zstd::bulk::compress(&encoded, ZSTD_LEVEL)?;
        file.write_all(&frame)?;
        self.header.directory_offset = self.offset;
        self.header.directory_len = frame.len() as u64;
        self.header.directory_checksum = *blake3::hash(&frame).as_bytes();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&build_header(&self.header))?;
        file.sync_all()?;
        self.file
            .persist(&self.path)
            .map_err(|e| SolError::Io(e.error))?;
        #[cfg(unix)]
        if let Some(dir) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
}

/// Quantizes a value block to `i16` against a scale chosen from its own
/// largest magnitude. Returns `(scale, bytes)`; `scale` is `0.0` when every
/// value is zero, which `dequantize_values` reads back as all zeros.
pub(crate) fn quantize_values(values: &[f32]) -> (f32, Vec<u8>) {
    let peak = values.iter().fold(0.0f32, |peak, v| peak.max(v.abs()));
    let scale = if peak > 0.0 { peak / 32_767.0 } else { 0.0 };
    let mut bytes = Vec::with_capacity(values.len() * 2);
    for &v in values {
        let q = if scale > 0.0 {
            (v / scale).round().clamp(-32_767.0, 32_767.0) as i16
        } else {
            0
        };
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    (scale, bytes)
}

/// Dequantizes a [`quantize_values`] blob. `bytes.len()` must be twice
/// `len`; a mismatch is `SolError::Truncated` rather than a panic, for the
/// same reason [`dequantize_probs`] reports it that way.
pub(crate) fn dequantize_values(
    bytes: &[u8],
    scale: f32,
    len: usize,
) -> Result<Vec<f32>, SolError> {
    let mut out = Vec::new();
    dequantize_values_into(bytes, scale, len, &mut out)?;
    Ok(out)
}

fn dequantize_values_into(
    bytes: &[u8],
    scale: f32,
    len: usize,
    out: &mut Vec<f32>,
) -> Result<(), SolError> {
    let expected = len * 2;
    if bytes.len() != expected {
        return Err(SolError::Truncated {
            expected,
            actual: bytes.len(),
        });
    }
    out.clear();
    out.extend(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&chunk| i16::from_le_bytes(chunk) as f32 * scale),
    );
    Ok(())
}

/// Quantizes an already-normalized probability slice (values in `[0, 1]`,
/// caller-guaranteed to be a valid strategy) to 16-bit fixed point:
/// `q = round(p * 65535)`. Deliberately does not renormalize on the way in
/// — `dequantize_probs` is responsible for restoring an exact per-hand sum
/// of 1.0 after the rounding error introduced here.
pub(crate) fn quantize_probs(probs: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(probs.len() * 2);
    for &p in probs {
        let q = (p * 65535.0).round().clamp(0.0, 65535.0) as u16;
        bytes.extend_from_slice(&q.to_le_bytes());
    }
    bytes
}

/// Dequantizes a `quantize_probs` byte blob back into `f32` probabilities,
/// renormalizing per hand column (stride `num_hands`, action-major layout)
/// so each hand's action probabilities sum to exactly 1.0 despite the
/// quantization rounding error. Falls back to a uniform distribution over
/// actions for a hand column whose quantized values all round to zero;
/// this cannot happen for genuine `average_strategy` output (every hand
/// reaches at least one action with positive probability) but is handled
/// defensively rather than dividing by zero.
///
/// `bytes.len()` must equal `num_actions * num_hands * 2`; returns
/// `SolError::Truncated` describing the mismatch (reusing that variant
/// rather than adding a dedicated one) instead of panicking, since `bytes`
/// ultimately comes from a file on disk that could be corrupt or
/// mismatched against a stale tree shape.
fn dequantize_probs(
    bytes: &[u8],
    num_actions: usize,
    num_hands: usize,
) -> Result<Vec<f32>, SolError> {
    let mut out = Vec::new();
    dequantize_probs_into(bytes, num_actions, num_hands, &mut out)?;
    Ok(out)
}

fn dequantize_probs_into(
    bytes: &[u8],
    num_actions: usize,
    num_hands: usize,
    out: &mut Vec<f32>,
) -> Result<(), SolError> {
    out.resize(num_actions * num_hands, 0.0);
    dequantize_probs_into_slice(bytes, num_actions, num_hands, out)
}

fn dequantize_probs_into_slice(
    bytes: &[u8],
    num_actions: usize,
    num_hands: usize,
    out: &mut [f32],
) -> Result<(), SolError> {
    let expected = num_actions * num_hands * 2;
    if bytes.len() != expected {
        return Err(SolError::Truncated {
            expected,
            actual: bytes.len(),
        });
    }

    if out.len() != num_actions * num_hands {
        return Err(SolError::Truncated {
            expected: num_actions * num_hands,
            actual: out.len(),
        });
    }
    // Every u16 fits exactly in f32, so the integer sum remains lossless.
    for (slot, &chunk) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *slot = u16::from_le_bytes(chunk) as f32;
    }
    for h in 0..num_hands {
        let mut sum: u32 = 0;
        for a in 0..num_actions {
            sum += out[a * num_hands + h] as u32;
        }
        if sum == 0 {
            let uniform = 1.0 / num_actions as f32;
            for a in 0..num_actions {
                out[a * num_hands + h] = uniform;
            }
        } else {
            for a in 0..num_actions {
                out[a * num_hands + h] /= sum as f32;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "hu-postflop-sol-test-{}-{}-{}",
            std::process::id(),
            id,
            name
        ))
    }

    fn sample_payload() -> SolPayload {
        SolPayload {
            config_toml: "[game]\nstreet = \"flop\"\n".to_string(),
            meta: SolMeta {
                iterations: 1234,
                expl: [0.5, 0.25],
                ev: [1.5, -1.5],
                nash_conv: 0.75,
                storage: "f32".to_string(),
                wall_secs: 12.5,
            },
            mode: StreetsStored::NoRivers,
            tree_nodes: 17,
            blocks: vec![
                StrategyBlock {
                    sref: 3,
                    probs: quantize_probs(&[
                        0.5, 0.25, 0.25, 0.25, // action 0, 4 hands
                        0.3, 0.5, 0.5, 0.5, // action 1, 4 hands
                        0.2, 0.25, 0.25, 0.25, // action 2, 4 hands
                    ]),
                },
                StrategyBlock {
                    sref: 9,
                    probs: quantize_probs(&[
                        1.0, 0.5, // action 0, 2 hands
                        0.0, 0.5, // action 1, 2 hands
                    ]),
                },
            ],
            // Same srefs as `blocks`, in the same order: the two lists
            // always describe the same nodes.
            values: vec![
                value_block(3, &[2.0, -1.0, 0.5, 0.0, -2.0, 1.0, -0.5, 0.0]),
                value_block(9, &[7.5, -7.5, -7.5, 7.5]),
            ],
            // The codec stores the index stream without interpreting it.
            index: (0..5000u32).map(|i| (i % 7) as u8).collect(),
        }
    }

    /// Blocks large enough that the strategy and value sections each span
    /// several chunks and at least one parallel compression group, with
    /// gaps in the `sref` sequence like a `NoRivers` artifact.
    fn multi_chunk_payload() -> SolPayload {
        let mut payload = sample_payload();
        let count = 2 * PARALLEL_CHUNKS as u32 * 4 + 3;
        let srefs: Vec<u32> = (0..count).map(|i| i * 3 + 1).collect();
        payload.blocks = srefs
            .iter()
            .map(|&sref| StrategyBlock {
                sref,
                probs: (0..300_000u32).map(|i| (i * 7 + sref) as u8).collect(),
            })
            .collect();
        payload.values = srefs
            .iter()
            .map(|&sref| {
                let values: Vec<f32> = (0..10_000)
                    .map(|i| (i as f32 - sref as f32) * 0.5)
                    .collect();
                value_block(sref, &values)
            })
            .collect();
        payload
    }

    fn value_block(sref: u32, values: &[f32]) -> ValueBlock {
        let (scale, bytes) = quantize_values(values);
        ValueBlock {
            sref,
            scale,
            values: bytes,
        }
    }

    /// Values survive the `i16` round trip well inside any solve's own
    /// convergence error, and an all-zero block stays exactly zero rather
    /// than dividing by a zero scale.
    #[test]
    fn value_quantization_round_trips_within_tolerance() {
        let values = [0.0, 1.0, -1.0, 250.0, -250.0, 0.125, -0.125];
        let (scale, bytes) = quantize_values(&values);
        let back = dequantize_values(&bytes, scale, values.len()).unwrap();
        let step = 250.0 / 32_767.0;
        for (original, restored) in values.iter().zip(&back) {
            assert!(
                (original - restored).abs() <= step,
                "{original} restored as {restored}"
            );
        }

        let (scale, bytes) = quantize_values(&[0.0, 0.0, 0.0]);
        assert_eq!(scale, 0.0);
        assert_eq!(dequantize_values(&bytes, scale, 3).unwrap(), vec![0.0; 3]);

        assert!(
            dequantize_values(&bytes, 1.0, 4).is_err(),
            "length mismatch"
        );
    }

    #[test]
    fn round_trip() {
        for payload in [sample_payload(), multi_chunk_payload()] {
            let path = temp_path("round-trip.sol");
            write_sol(&path, &payload).unwrap();
            assert_eq!(read_sol(&path).unwrap(), payload);
            let _ = std::fs::remove_file(&path);
        }
    }
    #[test]
    fn river_constraints_round_trip_and_detect_corruption_lazily() {
        let payload = sample_payload();
        let metadata = SolMetadata {
            config_toml: payload.config_toml.clone(),
            meta: payload.meta.clone(),
            mode: payload.mode,
            tree_nodes: payload.tree_nodes,
            stored_nodes: payload.blocks.len() as u64,
        };
        let report = crate::run::ProfileReport {
            summary: Default::default(),
            frozen_nodes: vec![],
            general_sum: true,
            frequencies: vec![],
        };
        let path = temp_path("river-constraints.sol");
        let mut writer =
            SolFileWriter::create_with_profile(&path, &metadata, Some(&report), |out| {
                out.write_all(&payload.index)
            })
            .unwrap();
        for block in &payload.blocks {
            writer.strategy(block.sref, &block.probs).unwrap();
        }
        for block in &payload.values {
            writer
                .values(block.sref, block.scale, &block.values)
                .unwrap();
        }
        let constraint = RiverConstraint {
            fixed: Some(vec![0.3, f32::NAN]),
            confidence: Some(vec![1.0]),
            bonus: Some(vec![0.125, -0.5]),
        };
        writer.river_constraint(42, &constraint).unwrap();
        writer.finish().unwrap();
        let reader = SolReader::open(&path).unwrap();
        assert_eq!(reader.directory.constraints.len(), 1);
        let offset = reader.directory.constraints[0].offset as usize;
        let store = reader.into_store();
        let decoded = store.river_constraint(42).unwrap().unwrap();
        assert_eq!(decoded.fixed.unwrap()[0], 0.3);
        assert_eq!(decoded.confidence.unwrap(), vec![1.0]);
        assert_eq!(decoded.bonus.unwrap(), vec![0.125, -0.5]);
        assert!(store.river_constraint(41).unwrap().is_none());
        drop(store);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[offset] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        let store = SolReader::open(&path).unwrap().into_store();
        assert!(matches!(
            store.river_constraint(42),
            Err(SolError::Checksum("chunk"))
        ));
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_payload_round_trips() {
        let mut payload = sample_payload();
        payload.blocks.clear();
        payload.values.clear();
        let path = temp_path("empty.sol");
        write_sol(&path, &payload).unwrap();
        assert_eq!(read_sol(&path).unwrap(), payload);
        let _ = std::fs::remove_file(&path);
    }

    /// Node queries through the store return exactly the dequantized block,
    /// in any order, across chunk boundaries and cache evictions; srefs in
    /// gaps and beyond the last chunk are absent.
    #[test]
    fn store_reads_single_nodes_in_any_order() {
        let payload = multi_chunk_payload();
        let path = temp_path("store.sol");
        write_sol(&path, &payload).unwrap();
        let reader = SolReader::open(&path).unwrap();
        assert!(reader.directory.strategy.len() > PARALLEL_CHUNKS);
        assert!(reader.directory.values.len() > 1);
        assert_eq!(reader.metadata().stored_nodes, payload.blocks.len() as u64);
        let store = reader.into_store();
        let order: Vec<usize> = (0..payload.blocks.len())
            .rev()
            .chain((0..payload.blocks.len()).step_by(5))
            .collect();
        for i in order {
            let (block, values) = (&payload.blocks[i], &payload.values[i]);
            let len = values.values.len() / 2;
            assert_eq!(
                store
                    .strategy(block.sref, 1, block.probs.len() / 2)
                    .unwrap(),
                Some(dequantize_probs(&block.probs, 1, block.probs.len() / 2).unwrap())
            );
            assert_eq!(
                store.values(block.sref, len).unwrap(),
                Some(dequantize_values(&values.values, values.scale, len).unwrap())
            );
            assert_eq!(store.strategy(block.sref + 1, 1, 1).unwrap(), None);
        }
        assert_eq!(store.strategy(0, 1, 1).unwrap(), None);
        assert_eq!(store.values(u32::MAX, 1).unwrap(), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pinned_strategy_chunks_match_streamed_reads() {
        let payload = multi_chunk_payload();
        let path = temp_path("pinned.sol");
        write_sol(&path, &payload).unwrap();
        let store = SolReader::open(&path).unwrap().into_store();
        let expected: Vec<_> = payload
            .blocks
            .iter()
            .map(|block| {
                store
                    .strategy(block.sref, 1, block.probs.len() / 2)
                    .unwrap()
            })
            .collect();
        store.pin_strategies().unwrap();
        for (block, expected) in payload.blocks.iter().zip(&expected).rev() {
            assert_eq!(
                &store
                    .strategy(block.sref, 1, block.probs.len() / 2)
                    .unwrap(),
                expected
            );
        }
        store.unpin_strategies();
        for (block, expected) in payload.blocks.iter().zip(&expected) {
            assert_eq!(
                &store
                    .strategy(block.sref, 1, block.probs.len() / 2)
                    .unwrap(),
                expected
            );
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_are_deterministic() {
        let payload = multi_chunk_payload();
        let a = temp_path("a.sol");
        let b = temp_path("b.sol");
        write_sol(&a, &payload).unwrap();
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| write_sol(&b, &payload))
            .unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        let _ = std::fs::remove_file(a);
        let _ = std::fs::remove_file(b);
    }

    #[test]
    fn writer_rejects_inconsistent_block_streams() {
        let payload = sample_payload();
        let metadata = SolMetadata {
            config_toml: payload.config_toml.clone(),
            meta: payload.meta.clone(),
            mode: payload.mode,
            tree_nodes: payload.tree_nodes,
            stored_nodes: 2,
        };
        let path = temp_path("inconsistent.sol");
        let mut writer = SolFileWriter::create(&path, &metadata, |_| Ok(())).unwrap();
        writer.strategy(3, &[0, 0]).unwrap();
        assert!(writer.strategy(3, &[0, 0]).is_err(), "repeated sref");
        writer.strategy(9, &[0, 0]).unwrap();
        assert!(writer.values(9, 1.0, &[0, 0]).is_err(), "value order");

        let mut writer = SolFileWriter::create(&path, &metadata, |_| Ok(())).unwrap();
        writer.strategy(3, &[0, 0]).unwrap();
        writer.values(3, 1.0, &[0, 0]).unwrap();
        assert!(writer.finish().is_err(), "fewer blocks than declared");
        assert!(!path.exists());
    }

    #[test]
    fn quantize_dequantize_round_trip() {
        // 3 actions x 5 hands, action-major, each column sums to 1.0.
        // Includes a uniform column and a one-hot column.
        #[rustfmt::skip]
        let probs: [f32; 15] = [
            // action 0
            0.2, 1.0 / 3.0, 1.0, 0.0, 0.6,
            // action 1
            0.3, 1.0 / 3.0, 0.0, 0.0, 0.1,
            // action 2
            0.5, 1.0 / 3.0, 0.0, 1.0, 0.3,
        ];
        let num_actions = 3;
        let num_hands = 5;

        let bytes = quantize_probs(&probs);
        assert_eq!(bytes.len(), probs.len() * 2);
        let restored = dequantize_probs(&bytes, num_actions, num_hands).unwrap();
        assert_eq!(restored.len(), probs.len());

        for (orig, got) in probs.iter().zip(restored.iter()) {
            assert!(
                (orig - got).abs() < 1e-4,
                "orig={orig} got={got} diff={}",
                (orig - got).abs()
            );
        }

        for h in 0..num_hands {
            let col_sum: f32 = (0..num_actions).map(|a| restored[a * num_hands + h]).sum();
            assert!(
                (col_sum - 1.0).abs() < 1e-6,
                "hand {h} column sum {col_sum} not within 1e-6 of 1.0"
            );
        }
    }

    fn written(name: &str, payload: &SolPayload) -> (std::path::PathBuf, Vec<u8>) {
        let path = temp_path(name);
        write_sol(&path, payload).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    }

    #[test]
    fn rejects_bad_magic() {
        let (path, mut bytes) = written("badmagic.sol", &sample_payload());
        bytes[0] = b'X';
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(read_sol(&path), Err(SolError::BadMagic)));
        let _ = std::fs::remove_file(&path);
    }

    /// Version 3 (no stored tree), version 2 (one zstd stream) and every
    /// other version are refused from the identity bytes alone, before any
    /// frame is decoded, even when the file is shorter than this version's
    /// header.
    #[test]
    fn rejects_other_versions_before_decoding() {
        for version in [1u16, 2, 3, 4, 5, 6, 99] {
            let path = temp_path("old-format.sol");
            let mut header = [0u8; 138];
            header[0..8].copy_from_slice(MAGIC);
            header[8..10].copy_from_slice(&version.to_le_bytes());
            std::fs::write(&path, header).unwrap();
            assert!(
                matches!(read_sol_config(&path), Err(SolError::BadVersion { found, expected }) if found == version && expected == FORMAT_VERSION)
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn rejects_truncated_header() {
        let (path, bytes) = written("truncated-header.sol", &sample_payload());
        std::fs::write(&path, &bytes[..HEADER_LEN - 5]).unwrap();
        match read_sol(&path) {
            Err(SolError::Truncated { expected, actual }) => {
                assert_eq!(expected, HEADER_LEN);
                assert_eq!(actual, HEADER_LEN - 5);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_file_at_open() {
        let (path, bytes) = written("truncated-payload.sol", &sample_payload());
        std::fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
        assert!(matches!(
            SolReader::open(&path),
            Err(SolError::Malformed(_))
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_hash_mismatch() {
        let (path, mut bytes) = written("hash-mismatch.sol", &sample_payload());
        // Flip one byte inside the header's config-hash field (bytes
        // 10..42) so it no longer matches blake3(config_toml).
        bytes[10] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        match read_sol(&path) {
            Err(SolError::HashMismatch { .. }) => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// Each frame is protected by its own hash: metadata and directory damage
    /// fails at open, index and chunk damage fail exactly when that frame is
    /// read while the rest of the file stays readable.
    #[test]
    fn detects_corruption_per_frame() {
        let payload = multi_chunk_payload();
        let (path, bytes) = written("corrupt.sol", &payload);
        let header = parse_header(bytes[..HEADER_LEN].try_into().unwrap());

        let mut damaged = bytes.clone();
        damaged[HEADER_LEN] ^= 1;
        std::fs::write(&path, &damaged).unwrap();
        assert!(matches!(
            SolReader::open(&path),
            Err(SolError::Checksum("metadata"))
        ));

        let mut damaged = bytes.clone();
        damaged[header.directory_offset as usize + 1] ^= 1;
        std::fs::write(&path, &damaged).unwrap();
        assert!(matches!(
            SolReader::open(&path),
            Err(SolError::Checksum("directory"))
        ));

        let mut damaged = bytes.clone();
        damaged[HEADER_LEN + header.metadata_len as usize + 2] ^= 1;
        std::fs::write(&path, &damaged).unwrap();
        let mut reader = SolReader::open(&path).unwrap();
        assert!(matches!(
            reader.read_index(|_| Ok(())),
            Err(SolError::Checksum("index"))
        ));
        let first = &payload.blocks[0];
        assert!(
            reader
                .into_store()
                .strategy(first.sref, 1, first.probs.len() / 2)
                .unwrap()
                .is_some()
        );

        let reader = SolReader::open(&written("intact.sol", &payload).0).unwrap();
        let second = reader.directory.strategy[1];
        let mut damaged = bytes.clone();
        damaged[second.offset as usize + 4] ^= 1;
        std::fs::write(&path, &damaged).unwrap();
        let store = SolReader::open(&path).unwrap().into_store();
        let first = &payload.blocks[0];
        assert!(
            store
                .strategy(first.sref, 1, first.probs.len() / 2)
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            store.strategy(second.first_sref, 1, first.probs.len() / 2),
            Err(SolError::Checksum("chunk"))
        ));
        assert!(matches!(read_sol(&path), Err(SolError::Checksum("chunk"))));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn shuffle_round_trips_odd_and_even_lengths() {
        for len in [0usize, 1, 2, 7, 10] {
            let raw: Vec<u8> = (0..len as u8).collect();
            let planes = shuffle(&raw);
            assert_eq!(unshuffle(&planes), raw);
        }
        assert_eq!(shuffle(&[1, 2, 3, 4, 5]), vec![1, 3, 5, 2, 4]);
    }

    #[test]
    fn constraint_stream_workspace_bounds_buffered_blocks() {
        let hands = 128usize;
        let elements = hands * 4;
        let nodes = 4096usize;
        let fixed = vec![0.25; elements];
        let bonus = vec![0.5; elements];
        let confidence = vec![1.0; hands];
        let block = postcard::to_allocvec(&RiverConstraintRef {
            fixed: Some(&fixed),
            confidence: Some(&confidence),
            bonus: Some(&bonus),
        })
        .unwrap();
        let bound = river_constraint_workspace(
            elements as u64,
            (elements * nodes) as u64,
            hands as u64,
            nodes as u64,
        );
        let mut file = tempfile::tempfile().unwrap();
        let mut section = SectionWriter::new(Section::Constraints);
        let mut offset = 0;
        for sref in 0..nodes as u32 {
            section
                .push(&mut file, &mut offset, sref, None, &block)
                .unwrap();
            let held = section.raw.capacity()
                + section
                    .sealed
                    .iter()
                    .map(|(raw, ..)| raw.capacity())
                    .sum::<usize>()
                + block.capacity()
                + section.entries.capacity() * std::mem::size_of::<ChunkEntry>();
            // Each in-flight raw chunk also requires a shuffled buffer, a
            // compressed buffer, and a separately bounded zstd context.
            let parallel = section
                .sealed
                .iter()
                .map(|(raw, ..)| 2 * raw.capacity() + COMPRESSION_CONTEXT_BYTES as usize)
                .sum::<usize>();
            assert!((held + parallel) as u64 <= bound);
        }
    }

    /// The workspace estimate charges [`COMPRESSION_CONTEXT_BYTES`] per
    /// context: a whole-buffer chunk compression and the streamed index
    /// compression must both stay within it for inputs past zstd's largest
    /// window at this level.
    #[test]
    fn compression_contexts_fit_the_workspace_bound() {
        use zstd::zstd_safe::zstd_sys::ZSTD_EndDirective::{ZSTD_e_continue, ZSTD_e_end};
        use zstd::zstd_safe::{CCtx, CParameter, InBuffer, OutBuffer};
        let input: Vec<u8> = (0..16u32 << 20)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();

        let mut bulk = CCtx::create();
        bulk.set_parameter(CParameter::CompressionLevel(ZSTD_LEVEL))
            .unwrap();
        let mut out = Vec::with_capacity(zstd::zstd_safe::compress_bound(input.len()));
        bulk.compress2(&mut out, &input).unwrap();
        assert!(bulk.sizeof() as u64 <= COMPRESSION_CONTEXT_BYTES);

        let mut stream = CCtx::create();
        stream
            .set_parameter(CParameter::CompressionLevel(ZSTD_LEVEL))
            .unwrap();
        let mut buffer = vec![0u8; 1 << 20];
        for piece in input.chunks(1 << 16) {
            let mut source = InBuffer::around(piece);
            while source.pos() < piece.len() {
                let mut sink = OutBuffer::around(buffer.as_mut_slice());
                stream
                    .compress_stream2(&mut sink, &mut source, ZSTD_e_continue)
                    .unwrap();
            }
        }
        loop {
            let mut sink = OutBuffer::around(buffer.as_mut_slice());
            let remaining = stream
                .compress_stream2(&mut sink, &mut InBuffer::around(&[]), ZSTD_e_end)
                .unwrap();
            if remaining == 0 {
                break;
            }
        }
        assert!(stream.sizeof() as u64 <= COMPRESSION_CONTEXT_BYTES);
    }
    #[test]
    fn dequantize_rejects_wrong_length() {
        let bytes = quantize_probs(&[0.5, 0.5]);
        // Claim 3 hands worth of data when only 2 hands' worth is present.
        match dequantize_probs(&bytes, 1, 3) {
            Err(SolError::Truncated { expected, actual }) => {
                assert_eq!(expected, 3 * 2);
                assert_eq!(actual, bytes.len());
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }
}
