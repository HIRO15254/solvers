//! Version 3: checked metadata and indexed, bounded groups of node frames.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::hash::{config_hash, config_hash_hex};
use crate::sol::{
    HEADER_LEN, SolError, SolMeta, SolPayload, StrategyBlock, StreetsStored, ValueBlock,
};

const MAGIC: &[u8; 8] = b"SLVRSOLV";
pub const SOL_FORMAT_VERSION: u16 = 3;
const PREFIX_LEN: u64 = HEADER_LEN as u64 + 8 + 8 + 32 + 8;
const ENTRY_LEN: u64 = 64;
const MAX_CHUNK_NODES: usize = 64;
/// Limits apply to one decoded section, not the complete artifact.
pub const SOL_MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;
pub const SOL_MAX_NODE_BYTES: u64 = 64 * 1024 * 1024;
// zstd's worst-case expansion at 64 MiB is below 1 MiB. Keep the input
// extent bounded too, rather than accepting gigabytes of empty frames.
const MAX_COMPRESSED_CHUNK_BYTES: u64 = SOL_MAX_NODE_BYTES + 1024 * 1024;
const MAX_COMPRESSED_METADATA_BYTES: u64 = SOL_MAX_METADATA_BYTES + 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolMetadata {
    pub config_toml: String,
    /// Measured on the live average profile, before artifact quantization.
    pub meta: SolMeta,
    pub mode: StreetsStored,
    pub node_count: u64,
    pub stored_nodes: u64,
}

#[derive(Debug)]
struct Entry {
    first: u32,
    last: u32,
    count: u32,
    offset: u64,
    compressed_len: u32,
    decoded_len: u32,
    digest: [u8; 32],
}

/// Opening verifies metadata, the complete directory and file extent.
/// Node contents are authenticated when requested; opening alone does not
/// claim that every strategy/value frame has been checked.
pub struct SolReader {
    file: File,
    metadata: SolMetadata,
    entries: Vec<Entry>,
}

fn invalid(message: &str) -> SolError {
    SolError::InvalidLayout(message.to_owned())
}

fn validate_meta(meta: &SolMeta) -> Result<(), SolError> {
    if meta
        .ev
        .into_iter()
        .chain(meta.expl)
        .chain([meta.nash_conv, meta.wall_secs])
        .any(|value| !value.is_finite())
    {
        return Err(invalid("metadata metrics must be finite"));
    }
    if meta.wall_secs < 0.0 {
        return Err(invalid("metadata wall_secs must be nonnegative"));
    }
    if !matches!(meta.storage.as_str(), "f32" | "i16") {
        return Err(invalid("metadata storage must be f32 or i16"));
    }
    Ok(())
}

fn read_u64(reader: &mut impl Read) -> Result<u64, SolError> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> Result<u32, SolError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

struct StoredSrefs<'a> {
    entries: &'a [Entry],
    group: usize,
    position: u32,
    remaining: usize,
}

impl Iterator for StoredSrefs<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let entry = self.entries.get(self.group)?;
        let sref = entry.first + self.position;
        self.position += 1;
        self.remaining -= 1;
        if self.position == entry.count {
            self.group += 1;
            self.position = 0;
        }
        Some(sref)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for StoredSrefs<'_> {}

fn decode_frame_bytes(
    file: &mut File,
    offset: u64,
    compressed_len: u64,
    decoded_len: u64,
    digest: &[u8; 32],
    limit: u64,
) -> Result<Vec<u8>, SolError> {
    if decoded_len > limit {
        return Err(invalid("decoded section exceeds its format limit"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let section = file.take(compressed_len);
    let mut decoder = zstd::Decoder::new(section)?.single_frame();
    // A chunk cannot decode beyond 64 MiB; reject a frame requesting a larger
    // history window before zstd allocates it, even when its output is small.
    decoder.window_log_max(26)?;
    let mut decoder = decoder.take(decoded_len + 1);
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded)?;
    if decoded.len() as u64 != decoded_len || blake3::hash(&decoded).as_bytes() != digest {
        return Err(invalid("section length or BLAKE3 checksum mismatch"));
    }
    let source = decoder.into_inner().finish();
    if source.get_ref().limit() != 0 || !source.buffer().is_empty() {
        return Err(invalid("trailing bytes in compressed section"));
    }
    Ok(decoded)
}

fn decode_payload<T: serde::de::DeserializeOwned>(decoded: &[u8]) -> Result<T, SolError> {
    let (value, trailing) = postcard::take_from_bytes(decoded)?;
    if !trailing.is_empty() {
        return Err(invalid("trailing bytes in decoded section"));
    }
    Ok(value)
}

impl SolReader {
    pub fn open(path: &Path) -> Result<Self, SolError> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        if size < HEADER_LEN as u64 {
            return Err(SolError::Truncated {
                expected: HEADER_LEN,
                actual: size as usize,
            });
        }
        let mut header = [0; HEADER_LEN];
        file.read_exact(&mut header)?;
        if &header[..8] != MAGIC {
            return Err(SolError::BadMagic);
        }
        let version = u16::from_le_bytes(header[8..10].try_into().unwrap());
        if version != SOL_FORMAT_VERSION {
            return Err(SolError::BadVersion {
                found: version,
                expected: SOL_FORMAT_VERSION,
            });
        }
        let header_hash: [u8; 32] = header[10..42].try_into().unwrap();
        let iteration = u64::from_le_bytes(header[42..50].try_into().unwrap());
        let metadata_len = read_u64(&mut file)?;
        if metadata_len == 0 || metadata_len > MAX_COMPRESSED_METADATA_BYTES {
            return Err(invalid(
                "compressed metadata exceeds format limit or is empty",
            ));
        }
        let metadata_decoded_len = read_u64(&mut file)?;
        let mut metadata_digest = [0; 32];
        file.read_exact(&mut metadata_digest)?;
        let count = read_u64(&mut file)?;
        let directory_start = PREFIX_LEN
            .checked_add(metadata_len)
            .ok_or_else(|| invalid("metadata extent overflow"))?;
        let directory_len = count
            .checked_mul(ENTRY_LEN)
            .ok_or_else(|| invalid("directory size overflow"))?;
        let data_start = directory_start
            .checked_add(directory_len)
            .ok_or_else(|| invalid("directory extent overflow"))?;
        if data_start > size {
            return Err(invalid("metadata or directory extends beyond the file"));
        }
        let metadata_bytes = decode_frame_bytes(
            &mut file,
            PREFIX_LEN,
            metadata_len,
            metadata_decoded_len,
            &metadata_digest,
            SOL_MAX_METADATA_BYTES,
        )?;
        let metadata: SolMetadata = decode_payload(&metadata_bytes)?;
        let computed = config_hash(metadata.config_toml.as_bytes());
        if computed != header_hash {
            return Err(SolError::HashMismatch {
                header_hash: config_hash_hex(&header_hash),
                computed_hash: config_hash_hex(&computed),
            });
        }
        if metadata.meta.iterations != iteration
            || count > metadata.stored_nodes
            || metadata.stored_nodes > metadata.node_count
        {
            return Err(invalid("header, metadata and directory counts disagree"));
        }
        validate_meta(&metadata.meta)?;
        usize::try_from(metadata.stored_nodes)
            .map_err(|_| invalid("stored node count exceeds address space"))?;
        let mut directory = BufReader::new(&mut file);
        directory.seek(SeekFrom::Start(directory_start))?;
        let count =
            usize::try_from(count).map_err(|_| invalid("directory count exceeds address space"))?;
        let mut entries = Vec::new();
        entries
            .try_reserve(count)
            .map_err(|_| invalid("directory allocation failed"))?;
        let mut end = data_start;
        let mut previous = None;
        let mut stored_nodes = 0u64;
        for _ in 0..count {
            let first = read_u32(&mut directory)?;
            let last = read_u32(&mut directory)?;
            let node_count = read_u32(&mut directory)?;
            let reserved = read_u32(&mut directory)?;
            let offset = read_u64(&mut directory)?;
            let compressed_len = read_u32(&mut directory)?;
            let decoded_len = read_u32(&mut directory)?;
            let mut digest = [0; 32];
            directory.read_exact(&mut digest)?;
            if node_count == 0
                || node_count > MAX_CHUNK_NODES as u32
                || first.checked_add(node_count - 1) != Some(last)
                || previous.is_some_and(|id| id >= first)
                || u64::from(last) >= metadata.node_count
                || reserved != 0
                || offset != end
                || compressed_len == 0
                || u64::from(compressed_len) > MAX_COMPRESSED_CHUNK_BYTES
                || decoded_len == 0
                || u64::from(decoded_len) > SOL_MAX_NODE_BYTES
            {
                return Err(invalid("unordered, overlapping or invalid node directory"));
            }
            end = offset
                .checked_add(u64::from(compressed_len))
                .ok_or_else(|| invalid("node extent overflow"))?;
            if end > size {
                return Err(invalid("node frame extends beyond the file"));
            }
            entries.push(Entry {
                first,
                last,
                count: node_count,
                offset,
                compressed_len,
                decoded_len,
                digest,
            });
            stored_nodes = stored_nodes
                .checked_add(u64::from(node_count))
                .ok_or_else(|| invalid("stored node count overflow"))?;
            previous = Some(last);
        }
        if end != size || stored_nodes != metadata.stored_nodes {
            return Err(invalid(
                "unindexed trailing bytes or stored node count mismatch",
            ));
        }
        drop(directory);
        Ok(Self {
            file,
            metadata,
            entries,
        })
    }

    pub fn metadata(&self) -> &SolMetadata {
        &self.metadata
    }

    pub fn stored_srefs(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        StoredSrefs {
            entries: &self.entries,
            group: 0,
            position: 0,
            remaining: self.metadata.stored_nodes as usize,
        }
    }

    fn read_chunk(&mut self, index: usize) -> Result<Vec<(StrategyBlock, ValueBlock)>, SolError> {
        let entry = &self.entries[index];
        let raw = decode_frame_bytes(
            &mut self.file,
            entry.offset,
            u64::from(entry.compressed_len),
            u64::from(entry.decoded_len),
            &entry.digest,
            SOL_MAX_NODE_BYTES,
        )?;
        // At most 64 elements means the canonical postcard vector count is
        // exactly one byte. Validate it before serde allocates any node list.
        if raw.first().copied() != Some(entry.count as u8) {
            return Err(invalid("chunk node count does not match directory"));
        }
        let pairs: Vec<(StrategyBlock, ValueBlock)> = decode_payload(&raw)?;
        if pairs.len() != entry.count as usize
            || pairs.iter().enumerate().any(|(position, pair)| {
                let expected = entry.first + position as u32;
                pair.0.sref != expected || pair.1.sref != expected
            })
        {
            return Err(invalid(
                "chunk node count or identity does not match directory",
            ));
        }
        Ok(pairs)
    }

    pub fn read_node(
        &mut self,
        sref: u32,
    ) -> Result<Option<(StrategyBlock, ValueBlock)>, SolError> {
        let index = self.entries.partition_point(|entry| entry.last < sref);
        let Some(entry) = self.entries.get(index) else {
            return Ok(None);
        };
        if sref < entry.first {
            return Ok(None);
        }
        let position = (sref - entry.first) as usize;
        Ok(Some(self.read_chunk(index)?.swap_remove(position)))
    }
}

pub fn read_sol(path: &Path) -> Result<SolPayload, SolError> {
    let mut reader = SolReader::open(path)?;
    let count = reader.metadata.stored_nodes as usize;
    let mut blocks = Vec::with_capacity(count);
    let mut values = Vec::with_capacity(count);
    for index in 0..reader.entries.len() {
        for (strategy, value) in reader.read_chunk(index)? {
            blocks.push(strategy);
            values.push(value);
        }
    }
    let meta = reader.metadata;
    Ok(SolPayload {
        config_toml: meta.config_toml,
        meta: meta.meta,
        mode: meta.mode,
        node_count: meta.node_count,
        blocks,
        values,
    })
}

pub fn write_sol(path: &Path, payload: &SolPayload) -> Result<(), SolError> {
    validate_meta(&payload.meta)?;
    if payload.blocks.len() != payload.values.len()
        || payload.blocks.len() as u64 > payload.node_count
    {
        return Err(invalid("strategy/value node counts disagree"));
    }
    let mut previous = None;
    for (block, value) in payload.blocks.iter().zip(&payload.values) {
        if block.sref != value.sref
            || previous.is_some_and(|id| id >= block.sref)
            || u64::from(block.sref) >= payload.node_count
        {
            return Err(invalid(
                "strategy/value blocks must have the same strictly ascending srefs below node_count",
            ));
        }
        previous = Some(block.sref);
    }
    // Size without allocating a serialized artifact. A vector of at most 64
    // elements has a one-byte postcard length prefix. Break at every sref gap
    // so the directory alone remains an exact index of stored nodes.
    let mut groups = Vec::new();
    let mut start = 0;
    let mut group_bytes = 1u64;
    for (index, (block, value)) in payload.blocks.iter().zip(&payload.values).enumerate() {
        let pair_bytes = postcard::experimental::serialized_size(&(block, value))? as u64;
        if pair_bytes >= SOL_MAX_NODE_BYTES {
            return Err(invalid("node exceeds chunk format limit"));
        }
        if index > start
            && (index - start == MAX_CHUNK_NODES
                || payload.blocks[index - 1].sref.checked_add(1) != Some(block.sref)
                || group_bytes + pair_bytes > SOL_MAX_NODE_BYTES)
        {
            groups.push(start..index);
            start = index;
            group_bytes = 1;
        }
        group_bytes += pair_bytes;
    }
    if start < payload.blocks.len() {
        groups.push(start..payload.blocks.len());
    }
    let meta = SolMetadata {
        config_toml: payload.config_toml.clone(),
        meta: payload.meta.clone(),
        mode: payload.mode,
        node_count: payload.node_count,
        stored_nodes: payload.blocks.len() as u64,
    };
    let raw = postcard::to_allocvec(&meta)?;
    if raw.len() as u64 > SOL_MAX_METADATA_BYTES {
        return Err(invalid("metadata exceeds format limit"));
    }
    let compressed = zstd::encode_all(raw.as_slice(), 0)?;
    if compressed.len() as u64 > MAX_COMPRESSED_METADATA_BYTES {
        return Err(invalid("compressed metadata exceeds format limit"));
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    let file = temporary.as_file_mut();
    file.write_all(MAGIC)?;
    file.write_all(&SOL_FORMAT_VERSION.to_le_bytes())?;
    file.write_all(&config_hash(payload.config_toml.as_bytes()))?;
    file.write_all(&payload.meta.iterations.to_le_bytes())?;
    file.write_all(&(compressed.len() as u64).to_le_bytes())?;
    file.write_all(&(raw.len() as u64).to_le_bytes())?;
    file.write_all(blake3::hash(&raw).as_bytes())?;
    file.write_all(&(groups.len() as u64).to_le_bytes())?;
    file.write_all(&compressed)?;
    let directory_start = file.stream_position()?;
    let data_start = directory_start
        .checked_add(
            (groups.len() as u64)
                .checked_mul(ENTRY_LEN)
                .ok_or_else(|| invalid("directory size overflow"))?,
        )
        .ok_or_else(|| invalid("directory extent overflow"))?;
    file.seek(SeekFrom::Start(data_start))?;
    let mut entries = Vec::with_capacity(groups.len());
    for group in groups {
        let pairs: Vec<_> = payload.blocks[group.clone()]
            .iter()
            .zip(&payload.values[group])
            .collect();
        let raw = postcard::to_allocvec(&pairs)?;
        if raw.len() as u64 > SOL_MAX_NODE_BYTES {
            return Err(invalid("chunk exceeds format limit"));
        }
        let offset = file.stream_position()?;
        let mut encoder = zstd::Encoder::new(&mut *file, 0)?;
        encoder.include_checksum(true)?;
        encoder.write_all(&raw)?;
        encoder.finish()?;
        let compressed_len = file.stream_position()? - offset;
        if compressed_len > MAX_COMPRESSED_CHUNK_BYTES {
            return Err(invalid("compressed chunk exceeds format limit"));
        }
        entries.push(Entry {
            first: pairs[0].0.sref,
            last: pairs[pairs.len() - 1].0.sref,
            count: pairs.len() as u32,
            offset,
            compressed_len: compressed_len as u32,
            decoded_len: raw.len() as u32,
            digest: *blake3::hash(&raw).as_bytes(),
        });
    }
    file.seek(SeekFrom::Start(directory_start))?;
    {
        let mut directory = BufWriter::new(&mut *file);
        for entry in entries {
            directory.write_all(&entry.first.to_le_bytes())?;
            directory.write_all(&entry.last.to_le_bytes())?;
            directory.write_all(&entry.count.to_le_bytes())?;
            directory.write_all(&0u32.to_le_bytes())?;
            directory.write_all(&entry.offset.to_le_bytes())?;
            directory.write_all(&entry.compressed_len.to_le_bytes())?;
            directory.write_all(&entry.decoded_len.to_le_bytes())?;
            directory.write_all(&entry.digest)?;
        }
        directory.flush()?;
    }
    file.sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}
