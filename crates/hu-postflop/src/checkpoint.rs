//! Version 4 streaming checkpoints. Only bounded metadata is postcard;
//! storage is raw little-endian arenas, protected by a BLAKE3 digest.
use hu_engine::{SolverState, Storage, StorageArrays, StorageArraysMut, StorageState};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

pub const HEADER_LEN: usize = 50;
const MAGIC: &[u8; 8] = b"SLVRCKPT";
const FORMAT_VERSION: u16 = 4;
const MAX_METADATA: usize = 16 * 1024 * 1024;
const IO_CHUNK: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("not a solvers checkpoint file (bad magic bytes)")]
    BadMagic,
    #[error(
        "unsupported checkpoint format version {found} (expected {expected}); re-solve from solvers.nlh/v1 to create a streaming checkpoint (docs/hu-postflop.jp.md)"
    )]
    BadVersion { found: u16, expected: u16 },
    #[error("checkpoint file truncated: expected at least {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
    #[error("invalid checkpoint: {0}")]
    Invalid(&'static str),
    #[error("checkpoint I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checkpoint payload codec error: {0}")]
    Codec(#[from] postcard::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckpointHeader {
    pub config_hash: [u8; 32],
    pub iteration: u64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    pub config_hash: [u8; 32],
    pub iteration: u64,
    pub state: SolverState,
    pub config_toml: Option<String>,
    pub elapsed_secs: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    config_toml: Option<String>,
    elapsed_secs: Option<f64>,
    iteration: u64,
    i16: bool,
    lengths: [u64; 4],
}

fn parse_header(buf: &[u8; HEADER_LEN]) -> Result<CheckpointHeader, CheckpointError> {
    if &buf[..8] != MAGIC {
        return Err(CheckpointError::BadMagic);
    }
    let version = u16::from_le_bytes(buf[8..10].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(CheckpointError::BadVersion {
            found: version,
            expected: FORMAT_VERSION,
        });
    }
    Ok(CheckpointHeader {
        config_hash: buf[10..42].try_into().unwrap(),
        iteration: u64::from_le_bytes(buf[42..50].try_into().unwrap()),
    })
}
fn build_header(hash: [u8; 32], iteration: u64) -> [u8; HEADER_LEN] {
    let mut buf = [0; HEADER_LEN];
    buf[..8].copy_from_slice(MAGIC);
    buf[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    buf[10..42].copy_from_slice(&hash);
    buf[42..].copy_from_slice(&iteration.to_le_bytes());
    buf
}
fn shape(arrays: &StorageArrays<'_>) -> (bool, [u64; 4]) {
    match arrays {
        StorageArrays::F32 {
            regrets,
            strategy_sum,
        } => (
            false,
            [regrets.len() as u64, strategy_sum.len() as u64, 0, 0],
        ),
        StorageArrays::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => (
            true,
            [
                regrets.len() as u64,
                strategy_sum.len() as u64,
                regret_scales.len() as u64,
                strategy_scales.len() as u64,
            ],
        ),
    }
}

/// Open metadata only. The same open file is retained until restoration, so
/// replacing the path cannot substitute a different checkpoint mid-resume.
pub struct CheckpointReader {
    pub config_hash: [u8; 32],
    pub iteration: u64,
    pub config_toml: Option<String>,
    pub elapsed_secs: Option<f64>,
    i16: bool,
    lengths: [u64; 4],
    decoder: zstd::stream::read::Decoder<'static, BufReader<File>>,
    hasher: blake3::Hasher,
}
impl CheckpointReader {
    pub fn open(path: &Path) -> Result<Self, CheckpointError> {
        let mut file = File::open(path)?;
        let mut header = [0; HEADER_LEN];
        let actual = usize::try_from(file.metadata()?.len()).unwrap_or(usize::MAX);
        if actual < HEADER_LEN {
            return Err(CheckpointError::Truncated {
                expected: HEADER_LEN,
                actual,
            });
        }
        file.read_exact(&mut header)?;
        let parsed = parse_header(&header)?;
        let mut decoder = zstd::stream::read::Decoder::new(file)?;
        decoder.window_log_max(20)?;
        let mut length = [0; 4];
        decoder.read_exact(&mut length)?;
        let len = u32::from_le_bytes(length) as usize;
        if len > MAX_METADATA {
            return Err(CheckpointError::Invalid("metadata too large"));
        }
        let mut bytes = vec![0; len];
        decoder.read_exact(&mut bytes)?;
        let metadata: Metadata = postcard::from_bytes(&bytes)?;
        if metadata.iteration != parsed.iteration {
            return Err(CheckpointError::Invalid(
                "header/payload iteration mismatch",
            ));
        }
        if !metadata.i16 && metadata.lengths[2..] != [0, 0] {
            return Err(CheckpointError::Invalid("unexpected f32 scales"));
        }
        if metadata
            .elapsed_secs
            .is_some_and(|t| !t.is_finite() || t < 0.0)
        {
            return Err(CheckpointError::Invalid("invalid elapsed time"));
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&header);
        hasher.update(&length);
        hasher.update(&bytes);
        Ok(Self {
            config_hash: parsed.config_hash,
            iteration: parsed.iteration,
            config_toml: metadata.config_toml,
            elapsed_secs: metadata.elapsed_secs,
            i16: metadata.i16,
            lengths: metadata.lengths,
            decoder,
            hasher,
        })
    }
    /// Writes into the final storage allocation; never owns a second arena.
    /// On failure the caller must discard the partially restored backend.
    pub fn read_storage(mut self, storage: &mut impl Storage) -> Result<(), CheckpointError> {
        if shape(&storage.arrays()) != (self.i16, self.lengths) {
            return Err(CheckpointError::Invalid(
                "storage backend or lengths mismatch",
            ));
        }
        self.read_arrays(storage.arrays_mut())
    }
    fn read_arrays(&mut self, arrays: StorageArraysMut<'_>) -> Result<(), CheckpointError> {
        match arrays {
            StorageArraysMut::F32 {
                regrets,
                strategy_sum,
            } => {
                self.read_array(regrets)?;
                self.read_array(strategy_sum)?;
            }
            StorageArraysMut::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            } => {
                self.read_array(regrets)?;
                self.read_array(strategy_sum)?;
                self.read_array(regret_scales)?;
                self.read_array(strategy_scales)?;
            }
        }
        let mut digest = [0; 32];
        self.decoder.read_exact(&mut digest)?;
        if self.hasher.finalize().as_bytes() != &digest {
            return Err(CheckpointError::Invalid("checksum mismatch"));
        }
        let mut tail = [0];
        if self.decoder.read(&mut tail)? != 0 {
            return Err(CheckpointError::Invalid("trailing payload bytes"));
        }
        Ok(())
    }
    fn read_array<T: Element>(&mut self, out: &mut [T]) -> Result<(), CheckpointError> {
        let mut buf = [0; IO_CHUNK];
        for chunk in out.chunks_mut(IO_CHUNK / T::SIZE) {
            let bytes = &mut buf[..chunk.len() * T::SIZE];
            self.decoder.read_exact(bytes)?;
            self.hasher.update(bytes);
            for (dst, src) in chunk.iter_mut().zip(bytes.chunks_exact(T::SIZE)) {
                *dst = T::decode(src);
            }
        }
        Ok(())
    }
}
trait Element: Sized {
    const SIZE: usize;
    fn encode(&self, bytes: &mut [u8]);
    fn decode(bytes: &[u8]) -> Self;
}
impl Element for f32 {
    const SIZE: usize = 4;
    fn encode(&self, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.to_le_bytes());
    }
    fn decode(bytes: &[u8]) -> Self {
        Self::from_le_bytes(bytes.try_into().unwrap())
    }
}
impl Element for i16 {
    const SIZE: usize = 2;
    fn encode(&self, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.to_le_bytes());
    }
    fn decode(bytes: &[u8]) -> Self {
        Self::from_le_bytes(bytes.try_into().unwrap())
    }
}
fn allocate<T: Default + Clone>(len: u64) -> Result<Vec<T>, CheckpointError> {
    let len =
        usize::try_from(len).map_err(|_| CheckpointError::Invalid("array length overflow"))?;
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| CheckpointError::Invalid("array allocation failed"))?;
    out.resize(len, T::default());
    Ok(out)
}
/// Compatibility owned-state reader. Production resume uses `CheckpointReader`.
pub fn read_checkpoint(path: &Path) -> Result<Checkpoint, CheckpointError> {
    let mut reader = CheckpointReader::open(path)?;
    let [a, b, c, d] = reader.lengths;
    let mut storage = if reader.i16 {
        StorageState::I16 {
            regrets: allocate(a)?,
            strategy_sum: allocate(b)?,
            regret_scales: allocate(c)?,
            strategy_scales: allocate(d)?,
        }
    } else {
        StorageState::F32 {
            regrets: allocate(a)?,
            strategy_sum: allocate(b)?,
        }
    };
    reader.read_arrays(storage.arrays_mut())?;
    Ok(Checkpoint {
        config_hash: reader.config_hash,
        iteration: reader.iteration,
        state: SolverState {
            iteration: reader.iteration,
            storage,
        },
        config_toml: reader.config_toml,
        elapsed_secs: reader.elapsed_secs,
    })
}
pub fn write_checkpoint(
    path: &Path,
    hash: [u8; 32],
    state: &SolverState,
) -> Result<(), CheckpointError> {
    write_arrays(
        path,
        hash,
        state.iteration,
        state.storage.arrays(),
        None,
        None,
        1,
        1,
    )
}
pub fn write_checkpoint_with_config(
    path: &Path,
    hash: [u8; 32],
    state: &SolverState,
    config: &str,
    elapsed: f64,
) -> Result<(), CheckpointError> {
    write_arrays(
        path,
        hash,
        state.iteration,
        state.storage.arrays(),
        Some(config),
        Some(elapsed),
        1,
        1,
    )
}
/// Production writer: borrowed arenas, bounded conversion buffer, threaded zstd.
pub fn write_storage_with_config(
    path: &Path,
    hash: [u8; 32],
    iteration: u64,
    storage: &impl Storage,
    config: &str,
    elapsed: f64,
    threads: usize,
) -> Result<(), CheckpointError> {
    write_arrays(
        path,
        hash,
        iteration,
        storage.arrays(),
        Some(config),
        Some(elapsed),
        threads,
        1,
    )
}
#[allow(clippy::too_many_arguments)]
fn write_arrays(
    path: &Path,
    hash: [u8; 32],
    iteration: u64,
    arrays: StorageArrays<'_>,
    config: Option<&str>,
    elapsed: Option<f64>,
    threads: usize,
    level: i32,
) -> Result<(), CheckpointError> {
    let (i16, lengths) = shape(&arrays);
    let metadata = postcard::to_allocvec(&Metadata {
        config_toml: config.map(str::to_owned),
        elapsed_secs: elapsed,
        iteration,
        i16,
        lengths,
    })?;
    if metadata.len() > MAX_METADATA {
        return Err(CheckpointError::Invalid("metadata too large"));
    }
    let header = build_header(hash, iteration);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&header)?;
    {
        let mut encoder = zstd::stream::write::Encoder::new(tmp.as_file_mut(), level)?;
        encoder.window_log(20)?;
        if threads > 1 {
            encoder.multithread(
                u32::try_from(threads)
                    .map_err(|_| CheckpointError::Invalid("thread count overflow"))?,
            )?;
        }
        encoder.include_checksum(true)?;
        let length = (metadata.len() as u32).to_le_bytes();
        let mut hasher = blake3::Hasher::new();
        hasher.update(&header);
        hasher.update(&length);
        hasher.update(&metadata);
        encoder.write_all(&length)?;
        encoder.write_all(&metadata)?;
        match arrays {
            StorageArrays::F32 {
                regrets,
                strategy_sum,
            } => {
                write_array(&mut encoder, &mut hasher, regrets)?;
                write_array(&mut encoder, &mut hasher, strategy_sum)?;
            }
            StorageArrays::I16 {
                regrets,
                strategy_sum,
                regret_scales,
                strategy_scales,
            } => {
                write_array(&mut encoder, &mut hasher, regrets)?;
                write_array(&mut encoder, &mut hasher, strategy_sum)?;
                write_array(&mut encoder, &mut hasher, regret_scales)?;
                write_array(&mut encoder, &mut hasher, strategy_scales)?;
            }
        }
        encoder.write_all(hasher.finalize().as_bytes())?;
        encoder.finish()?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .map_err(|e| CheckpointError::Io(e.error))?;
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}
fn write_array<T: Element>(
    writer: &mut impl Write,
    hasher: &mut blake3::Hasher,
    array: &[T],
) -> Result<(), CheckpointError> {
    let mut buf = [0; IO_CHUNK];
    for chunk in array.chunks(IO_CHUNK / T::SIZE) {
        let bytes = &mut buf[..chunk.len() * T::SIZE];
        for (value, dst) in chunk.iter().zip(bytes.chunks_exact_mut(T::SIZE)) {
            value.encode(dst);
        }
        hasher.update(bytes);
        writer.write_all(bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hu_engine::StorageState;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "hu-postflop-ckpt-test-{}-{}-{}",
            std::process::id(),
            id,
            name
        ))
    }

    fn sample_state_f32() -> SolverState {
        SolverState {
            iteration: 42,
            storage: StorageState::F32 {
                regrets: vec![1.0, -2.0, 3.5],
                strategy_sum: vec![0.1, 0.2, 0.3],
            },
        }
    }

    fn sample_state_i16() -> SolverState {
        SolverState {
            iteration: 7,
            storage: StorageState::I16 {
                regrets: vec![1, -2, 3],
                strategy_sum: vec![4, 5, 6],
                regret_scales: vec![0.01, 0.02],
                strategy_scales: vec![0.03, 0.04],
            },
        }
    }

    fn streaming_resume<S: hu_engine::Storage>() {
        use crate::game::{ChipEv, NoRake, PayoffPipeline, kuhn};
        use hu_engine::{Dcfr, Solver};
        let make = || {
            Solver::<_, S>::new(
                kuhn(PayoffPipeline {
                    rake: &NoRake,
                    utility: &ChipEv,
                })
                .game,
                Box::new(Dcfr::default()),
                Some(10),
            )
        };
        let mut original = make();
        original.run(4);
        let path = temp_path("stream.ckpt");
        write_storage_with_config(
            &path,
            [2; 32],
            original.iteration(),
            original.storage(),
            "config",
            1.25,
            4,
        )
        .unwrap();
        let reader = CheckpointReader::open(&path).unwrap();
        let mut resumed = make();
        resumed
            .restore_stream(reader.iteration, |storage| reader.read_storage(storage))
            .unwrap();
        assert_eq!(
            postcard::to_allocvec(&original.state()).unwrap(),
            postcard::to_allocvec(&resumed.state()).unwrap()
        );
        original.run(3);
        resumed.run(3);
        assert_eq!(
            postcard::to_allocvec(&original.state()).unwrap(),
            postcard::to_allocvec(&resumed.state()).unwrap()
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn direct_resume_f32_and_i16_bit_identical_after_iterations() {
        streaming_resume::<hu_engine::F32Storage>();
        streaming_resume::<hu_engine::I16Storage>();
    }
    #[test]
    fn atomic_replacement_and_failed_save_preserve_the_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.ckpt");
        write_checkpoint(&path, [1; 32], &sample_state_f32()).unwrap();
        write_checkpoint(&path, [2; 32], &sample_state_i16()).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(
            write_checkpoint_with_config(
                &path,
                [0; 32],
                &sample_state_f32(),
                &"x".repeat(MAX_METADATA + 1),
                0.0
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(read_checkpoint(&path).unwrap().state, sample_state_i16());
    }
    #[test]
    fn direct_reader_rejects_wrong_backend_or_lengths_before_writing() {
        use hu_engine::{F32Storage, I16Storage};
        let path = temp_path("shape.ckpt");
        write_checkpoint(&path, [0; 32], &sample_state_f32()).unwrap();
        let mut wrong_length = F32Storage::new(4, 0);
        let before = wrong_length.state();
        assert!(
            CheckpointReader::open(&path)
                .unwrap()
                .read_storage(&mut wrong_length)
                .is_err()
        );
        assert_eq!(wrong_length.state(), before);
        let mut wrong_backend = I16Storage::new(3, 2);
        assert!(
            CheckpointReader::open(&path)
                .unwrap()
                .read_storage(&mut wrong_backend)
                .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn raw_f32_preserves_signed_zero_and_nan_bits() {
        let path = temp_path("bits.ckpt");
        let state = SolverState {
            iteration: 0,
            storage: StorageState::F32 {
                regrets: vec![f32::from_bits(0x80000000), f32::from_bits(0x7fc00001)],
                strategy_sum: vec![f32::INFINITY],
            },
        };
        write_checkpoint(&path, [0; 32], &state).unwrap();
        let loaded = read_checkpoint(&path).unwrap();
        let StorageState::F32 {
            regrets,
            strategy_sum,
        } = loaded.state.storage
        else {
            panic!()
        };
        assert_eq!(
            regrets.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            vec![0x80000000, 0x7fc00001]
        );
        assert_eq!(strategy_sum[0].to_bits(), f32::INFINITY.to_bits());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_corruption_and_trailing_bytes_even_in_valid_zstd_frames() {
        let path = temp_path("integrity.ckpt");
        write_checkpoint(&path, [0; 32], &sample_state_f32()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let raw = zstd::decode_all(&original[HEADER_LEN..]).unwrap();
        let replace = |header: &[u8], data: &[u8]| {
            let mut bytes = header.to_vec();
            bytes.extend(zstd::encode_all(data, 1).unwrap());
            std::fs::write(&path, bytes).unwrap();
            assert!(read_checkpoint(&path).is_err());
        };
        let mut header = original[..HEADER_LEN].to_vec();
        header[10] ^= 1;
        replace(&header, &raw);
        let mut corrupt = raw.clone();
        let last = corrupt.len() - 33;
        corrupt[last] ^= 1;
        replace(&original[..HEADER_LEN], &corrupt);
        let mut extra = raw.clone();
        extra.push(0);
        replace(&original[..HEADER_LEN], &extra);
        replace(&original[..HEADER_LEN], &raw[..raw.len() - 1]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn round_trip_f32() {
        let path = temp_path("f32.ckpt");
        let hash = [7u8; 32];
        let state = sample_state_f32();
        write_checkpoint(&path, hash, &state).unwrap();
        let loaded = read_checkpoint(&path).unwrap();
        assert_eq!(loaded.config_hash, hash);
        assert_eq!(loaded.iteration, state.iteration);
        assert_eq!(loaded.state, state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn common_input_round_trip_preserves_config_and_elapsed_time() {
        let path = temp_path("nlh.ckpt");
        let state = sample_state_f32();
        let config = "schema = \"solvers.nlh/v1\"\n";
        write_checkpoint_with_config(&path, [8; 32], &state, config, 12.5).unwrap();
        let checkpoint = read_checkpoint(&path).unwrap();
        assert_eq!(checkpoint.state, state);
        assert_eq!(checkpoint.config_hash, [8; 32]);
        assert_eq!(checkpoint.config_toml.as_deref(), Some(config));
        assert_eq!(checkpoint.elapsed_secs, Some(12.5));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn round_trip_i16() {
        let path = temp_path("i16.ckpt");
        let hash = [9u8; 32];
        let state = sample_state_i16();
        write_checkpoint(&path, hash, &state).unwrap();
        let loaded = read_checkpoint(&path).unwrap();
        assert_eq!(loaded.state, state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_bad_magic() {
        let path = temp_path("badmagic.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] = b'X';
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            read_checkpoint(&path),
            Err(CheckpointError::BadMagic)
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_pre_compact_versions_before_decoding() {
        for version in [1u16, 2, 3] {
            let path = temp_path("old-format.checkpoint");
            let mut header = build_header([0; 32], 0);
            header[8..10].copy_from_slice(&version.to_le_bytes());
            std::fs::write(&path, header).unwrap();
            assert!(
                matches!(read_checkpoint(&path), Err(CheckpointError::BadVersion { found, expected }) if found == version && expected == FORMAT_VERSION)
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn rejects_bad_version() {
        let path = temp_path("badversion.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[8..10].copy_from_slice(&99u16.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        match read_checkpoint(&path) {
            Err(CheckpointError::BadVersion { found, expected }) => {
                assert_eq!(found, 99);
                assert_eq!(expected, FORMAT_VERSION);
            }
            other => panic!("expected BadVersion, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_header() {
        let path = temp_path("truncated-header.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..HEADER_LEN - 5]).unwrap();
        match read_checkpoint(&path) {
            Err(CheckpointError::Truncated { expected, actual }) => {
                assert_eq!(expected, HEADER_LEN);
                assert_eq!(actual, HEADER_LEN - 5);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_truncated_payload() {
        let path = temp_path("truncated-payload.ckpt");
        write_checkpoint(&path, [0u8; 32], &sample_state_f32()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // Keep the full header but chop the zstd payload short.
        let cut = bytes.len() - 3;
        std::fs::write(&path, &bytes[..cut]).unwrap();
        assert!(read_checkpoint(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_is_atomic_leaves_no_tmp_file_behind() {
        let path = temp_path("atomic.ckpt");
        write_checkpoint(&path, [1u8; 32], &sample_state_f32()).unwrap();
        let dir = path.parent().unwrap();
        let tmp_name = format!(".{}.tmp", path.file_name().unwrap().to_str().unwrap());
        assert!(!dir.join(tmp_name).exists());
        let _ = std::fs::remove_file(&path);
    }
}
