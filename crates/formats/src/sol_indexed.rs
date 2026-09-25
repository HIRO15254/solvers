//! Version 2: independently checked metadata and indexed node frames.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::hash::{config_hash, config_hash_hex};
use crate::sol::{
    HEADER_LEN, SolError, SolMeta, SolPayload, StrategyBlock, StreetsStored, ValueBlock,
};

const MAGIC: &[u8; 8] = b"SLVRSOLV";
pub const SOL_FORMAT_VERSION: u16 = 2;
const PREFIX_LEN: u64 = HEADER_LEN as u64 + 8 + 8 + 32 + 8;
const ENTRY_LEN: u64 = 4 + 8 + 8 + 8 + 32;
/// Limits apply to one decoded section, not the complete artifact.
pub const SOL_MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;
pub const SOL_MAX_NODE_BYTES: u64 = 64 * 1024 * 1024;

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
    sref: u32,
    offset: u64,
    compressed_len: u64,
    decoded_len: u64,
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

fn decode_frame<T: serde::de::DeserializeOwned>(
    file: &mut File,
    offset: u64,
    compressed_len: u64,
    decoded_len: u64,
    digest: &[u8; 32],
    limit: u64,
) -> Result<T, SolError> {
    if decoded_len > limit {
        return Err(invalid("decoded section exceeds its format limit"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let section = file.take(compressed_len);
    let mut decoder = zstd::Decoder::new(section)?.take(decoded_len + 1);
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded)?;
    if decoded.len() as u64 != decoded_len || blake3::hash(&decoded).as_bytes() != digest {
        return Err(invalid("section length or BLAKE3 checksum mismatch"));
    }
    let (value, trailing) = postcard::take_from_bytes(&decoded)?;
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
        let metadata: SolMetadata = decode_frame(
            &mut file,
            PREFIX_LEN,
            metadata_len,
            metadata_decoded_len,
            &metadata_digest,
            SOL_MAX_METADATA_BYTES,
        )?;
        let computed = config_hash(metadata.config_toml.as_bytes());
        if computed != header_hash {
            return Err(SolError::HashMismatch {
                header_hash: config_hash_hex(&header_hash),
                computed_hash: config_hash_hex(&computed),
            });
        }
        if metadata.meta.iterations != iteration
            || metadata.stored_nodes != count
            || count > metadata.node_count
        {
            return Err(invalid("header, metadata and directory counts disagree"));
        }
        validate_meta(&metadata.meta)?;
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
        for _ in 0..count {
            let mut sref = [0; 4];
            directory.read_exact(&mut sref)?;
            let sref = u32::from_le_bytes(sref);
            let offset = read_u64(&mut directory)?;
            let compressed_len = read_u64(&mut directory)?;
            let decoded_len = read_u64(&mut directory)?;
            let mut digest = [0; 32];
            directory.read_exact(&mut digest)?;
            if previous.is_some_and(|id| id >= sref)
                || u64::from(sref) >= metadata.node_count
                || offset != end
                || compressed_len == 0
                || decoded_len > SOL_MAX_NODE_BYTES
            {
                return Err(invalid("unordered, overlapping or invalid node directory"));
            }
            end = offset
                .checked_add(compressed_len)
                .ok_or_else(|| invalid("node extent overflow"))?;
            if end > size {
                return Err(invalid("node frame extends beyond the file"));
            }
            entries.push(Entry {
                sref,
                offset,
                compressed_len,
                decoded_len,
                digest,
            });
            previous = Some(sref);
        }
        if end != size {
            return Err(invalid("unindexed trailing bytes"));
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
        self.entries.iter().map(|entry| entry.sref)
    }

    pub fn read_node(
        &mut self,
        sref: u32,
    ) -> Result<Option<(StrategyBlock, ValueBlock)>, SolError> {
        let Ok(index) = self.entries.binary_search_by_key(&sref, |entry| entry.sref) else {
            return Ok(None);
        };
        let entry = &self.entries[index];
        let pair: (StrategyBlock, ValueBlock) = decode_frame(
            &mut self.file,
            entry.offset,
            entry.compressed_len,
            entry.decoded_len,
            &entry.digest,
            SOL_MAX_NODE_BYTES,
        )?;
        if pair.0.sref != sref || pair.1.sref != sref {
            return Err(invalid("node frame identity does not match directory"));
        }
        Ok(Some(pair))
    }
}

pub fn read_sol(path: &Path) -> Result<SolPayload, SolError> {
    let mut reader = SolReader::open(path)?;
    let ids: Vec<_> = reader.stored_srefs().collect();
    let mut blocks = Vec::with_capacity(ids.len());
    let mut values = Vec::with_capacity(ids.len());
    for id in ids {
        let (strategy, value) = reader.read_node(id)?.expect("directory entry exists");
        blocks.push(strategy);
        values.push(value);
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
    file.write_all(&(payload.blocks.len() as u64).to_le_bytes())?;
    file.write_all(&compressed)?;
    let directory_start = file.stream_position()?;
    let data_start = directory_start
        .checked_add(
            (payload.blocks.len() as u64)
                .checked_mul(ENTRY_LEN)
                .ok_or_else(|| invalid("directory size overflow"))?,
        )
        .ok_or_else(|| invalid("directory extent overflow"))?;
    file.seek(SeekFrom::Start(data_start))?;
    let mut entries = Vec::with_capacity(payload.blocks.len());
    for (block, value) in payload.blocks.iter().zip(&payload.values) {
        let raw = postcard::to_allocvec(&(block, value))?;
        if raw.len() as u64 > SOL_MAX_NODE_BYTES {
            return Err(invalid("node exceeds format limit"));
        }
        let offset = file.stream_position()?;
        let mut encoder = zstd::Encoder::new(&mut *file, 0)?;
        encoder.include_checksum(true)?;
        encoder.write_all(&raw)?;
        encoder.finish()?;
        entries.push(Entry {
            sref: block.sref,
            offset,
            compressed_len: file.stream_position()? - offset,
            decoded_len: raw.len() as u64,
            digest: *blake3::hash(&raw).as_bytes(),
        });
    }
    file.seek(SeekFrom::Start(directory_start))?;
    {
        let mut directory = BufWriter::new(&mut *file);
        for entry in entries {
            directory.write_all(&entry.sref.to_le_bytes())?;
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
