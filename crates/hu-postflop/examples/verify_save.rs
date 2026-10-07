//! Saved-state fingerprints, v2 block equivalence and streaming codec timing.
use anyhow::{Result, bail};
use hu_engine::StorageState;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

fn hash_f32(hash: &mut blake3::Hasher, values: &[f32]) {
    let mut bytes = [0; 65536];
    for chunk in values.chunks(bytes.len() / 4) {
        for (v, dst) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
            dst.copy_from_slice(&v.to_le_bytes());
        }
        hash.update(&bytes[..chunk.len() * 4]);
    }
}
fn hash_i16(hash: &mut blake3::Hasher, values: &[i16]) {
    let mut bytes = [0; 65536];
    for chunk in values.chunks(bytes.len() / 2) {
        for (v, dst) in chunk.iter().zip(bytes.chunks_exact_mut(2)) {
            dst.copy_from_slice(&v.to_le_bytes());
        }
        hash.update(&bytes[..chunk.len() * 2]);
    }
}
#[derive(Default)]
struct Count(u64);
impl Write for Count {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("finalization-memory") => {
            let path = Path::new(&args[2]);
            let p = hu_postflop::prepare::prepare(&std::fs::read_to_string(path)?, path)?;
            let peak = p.estimate.parallel_save_bytes(p.settings.solver.storage);
            println!(
                "{}",
                serde_json::json!({
                    "serial_peak": p.estimate.required_bytes(p.settings.solver.storage),
                    "parallel_peak": peak, "memory_limit": p.limit,
                    "save_bytes": p.estimate.save_bytes,
                    "compression_bytes": p.estimate.compression_bytes,
                    "parallel_if_final_save_needed": p.document.spot.run.final_checkpoint && peak.is_some_and(|bytes| bytes <= p.limit),
                })
            );
        }
        Some("solution-stream") => compare_solution_streams(&args[2], &args[3])?,
        Some("checkpoint-compare") => compare_checkpoints(&args[2], &args[3])?,
        Some("checkpoint") => {
            let checkpoint = hu_postflop::checkpoint::read_checkpoint(Path::new(&args[2]))?;
            let mut hash = blake3::Hasher::new();
            hash.update(&checkpoint.iteration.to_le_bytes());
            match checkpoint.state.storage {
                StorageState::F32 {
                    regrets,
                    strategy_sum,
                } => {
                    hash_f32(&mut hash, &regrets);
                    hash_f32(&mut hash, &strategy_sum);
                }
                StorageState::Mixed {
                    regrets,
                    strategy_sum,
                    regret_scales,
                } => {
                    hash_i16(&mut hash, &regrets);
                    hash_f32(&mut hash, &strategy_sum);
                    hash_f32(&mut hash, &regret_scales);
                }
                StorageState::I16 {
                    regrets,
                    strategy_sum,
                    regret_scales,
                    strategy_scales,
                } => {
                    hash_i16(&mut hash, &regrets);
                    hash_i16(&mut hash, &strategy_sum);
                    hash_f32(&mut hash, &regret_scales);
                    hash_f32(&mut hash, &strategy_scales);
                }
            }
            println!(
                "iteration={} storage_blake3={}",
                checkpoint.iteration,
                hash.finalize()
            );
        }
        Some("solution") => {
            let mut a = hu_postflop::sol::read_sol(Path::new(&args[2]))?;
            let mut b = hu_postflop::sol::read_sol(Path::new(&args[3]))?;
            let mut payload_sizes = Vec::new();
            for (path, payload) in [(&args[2], &a), (&args[3], &b)] {
                let file = std::fs::read(path)?;
                let raw = zstd::decode_all(&file[hu_postflop::sol::HEADER_LEN..])?;
                if raw != postcard::to_allocvec(payload)? {
                    bail!("noncanonical postcard stream in {path}");
                }
                payload_sizes.push(raw.len());
            }
            a.meta.wall_secs = 0.0;
            b.meta.wall_secs = 0.0;
            // Explicit opt-ins: the legacy/PF5 comparison drops the recorded
            // precision, and thread-count comparisons drop the operational [run].
            let flags = &args[4..];
            for flag in flags {
                if !matches!(flag.as_str(), "--ignore-cfr-precision" | "--ignore-run") {
                    bail!("unknown solution flag {flag}");
                }
            }
            let ignore_precision = flags.iter().any(|s| s == "--ignore-cfr-precision");
            let ignore_run = flags.iter().any(|s| s == "--ignore-run");
            if ignore_precision || ignore_run {
                for payload in [&mut a, &mut b] {
                    let mut config: toml_edit::DocumentMut = payload.config_toml.parse()?;
                    if ignore_precision
                        && let Some(solver) = config
                            .get_mut("solver")
                            .and_then(toml_edit::Item::as_table_mut)
                    {
                        solver.remove("cfr_precision");
                    }
                    if ignore_run {
                        config.remove("run");
                    }
                    payload.config_toml = config.to_string();
                }
            }
            let a_bytes = postcard::to_allocvec(&a)?;
            let b_bytes = postcard::to_allocvec(&b)?;
            if a_bytes != b_bytes {
                bail!("v2 payloads differ (excluding wall_secs)");
            }
            println!(
                "{}",
                serde_json::json!({
                    "payload_bit_equal_except_wall_secs": true,
                    "config_precision_excluded": ignore_precision,
                    "config_run_excluded": ignore_run,
                    "canonical_postcard_streams": true,
                    "payload_bytes": payload_sizes,
                    "normalized_payload_blake3": blake3::hash(&a_bytes).to_hex().as_str(),
                    "strategy_blocks": a.blocks.len(),
                    "value_blocks": a.values.len(),
                    "file_bytes_equal": std::fs::read(&args[2])? == std::fs::read(&args[3])?,
                })
            );
        }
        Some("compression") => {
            let mut results = Vec::new();
            for level in 1..=3 {
                let start = Instant::now();
                let mut file = std::fs::File::open(&args[2])?;
                let mut header = [0; 50];
                file.read_exact(&mut header)?;
                let mut decoder = zstd::stream::read::Decoder::new(file)?;
                let mut encoder = zstd::stream::write::Encoder::new(Count::default(), level)?;
                encoder.window_log(20)?;
                encoder.multithread(8)?;
                encoder.include_checksum(true)?;
                let input = std::io::copy(&mut decoder, &mut encoder)?;
                let output = encoder.finish()?.0;
                results.push(serde_json::json!({"level": level, "decode_compress_seconds": start.elapsed().as_secs_f64(), "input_bytes": input, "compressed_bytes": output}));
            }
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        Some("compression-sample") => {
            let mut file = std::fs::File::open(&args[2])?;
            let mut header = [0; 50];
            file.read_exact(&mut header)?;
            let mut decoder = zstd::stream::read::Decoder::new(file)?;
            let mut sample = vec![0; 128 * 1024 * 1024];
            decoder.read_exact(&mut sample)?;
            let mut results = Vec::new();
            for level in [1, 2, 3, 3, 2, 1] {
                let start = Instant::now();
                let mut encoder = zstd::stream::write::Encoder::new(Count::default(), level)?;
                encoder.window_log(20)?;
                encoder.multithread(8)?;
                encoder.include_checksum(true)?;
                encoder.write_all(&sample)?;
                let output = encoder.finish()?.0;
                results.push(serde_json::json!({"level": level, "compress_seconds": start.elapsed().as_secs_f64(), "input_bytes": sample.len(), "compressed_bytes": output}));
            }
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        _ => bail!(
            "verify_save checkpoint PATH | checkpoint-compare OLD NEW | solution OLD NEW | solution-stream OLD NEW | compression PATH"
        ),
    }
    Ok(())
}

/// Compare the v2 field stream directly, without retaining decoded blocks or
/// multi-GB postcard Vecs. Only wall_secs' eight bytes may differ.
fn compare_solution_streams(old: &str, new: &str) -> Result<()> {
    use anyhow::ensure;
    let mut a = std::fs::File::open(old)?;
    let mut b = std::fs::File::open(new)?;
    let mut header_a = [0; hu_postflop::sol::HEADER_LEN];
    let mut header_b = header_a;
    a.read_exact(&mut header_a)?;
    b.read_exact(&mut header_b)?;
    ensure!(&header_a[..10] == b"SLVRSOLV\x02\x00", "expected .sol v2");
    ensure!(header_a == header_b, "headers differ");
    let mut pair = FieldStreams {
        a: std::io::BufReader::new(zstd::stream::read::Decoder::new(a)?),
        b: std::io::BufReader::new(zstd::stream::read::Decoder::new(b)?),
        hash: blake3::Hasher::new(),
        position: 0,
        left: vec![0; 64 * 1024],
        right: vec![0; 64 * 1024],
    };
    let config_len = pair.varint()?;
    let mut config_hash = blake3::Hasher::new();
    pair.equal(config_len, Some(&mut config_hash))?;
    ensure!(
        config_hash.finalize().as_bytes() == &header_a[10..42],
        "config hash mismatch"
    );
    let iterations = pair.varint()?;
    ensure!(
        iterations == u64::from_le_bytes(header_a[42..50].try_into()?),
        "iteration mismatch"
    );
    pair.equal(40, None)?; // expl[2], ev[2], nash_conv: five fixed f64s
    let storage_len = pair.varint()?;
    pair.equal(storage_len, None)?;
    let mut wall_a = [0; 8];
    let mut wall_b = [0; 8];
    pair.a.read_exact(&mut wall_a)?;
    pair.b.read_exact(&mut wall_b)?;
    pair.hash.update(&[0; 8]);
    pair.position += 8;
    ensure!(pair.varint()? <= 1, "unsupported street mode");
    let blocks = pair.varint()?;
    let mut strategy_srefs = blake3::Hasher::new();
    let mut previous = None;
    for _ in 0..blocks {
        let sref = u32::try_from(pair.varint()?)?;
        ensure!(previous.is_none_or(|p| sref > p), "unordered strategy sref");
        previous = Some(sref);
        strategy_srefs.update(&sref.to_le_bytes());
        let len = pair.varint()?;
        ensure!(len % 2 == 0, "invalid u16 block length");
        pair.equal(len, None)?;
    }
    let values = pair.varint()?;
    ensure!(values == blocks, "strategy/value count mismatch");
    let mut value_srefs = blake3::Hasher::new();
    previous = None;
    for _ in 0..values {
        let sref = u32::try_from(pair.varint()?)?;
        ensure!(previous.is_none_or(|p| sref > p), "unordered value sref");
        previous = Some(sref);
        value_srefs.update(&sref.to_le_bytes());
        pair.equal(4, None)?; // fixed f32 scale
        let len = pair.varint()?;
        ensure!(len % 2 == 0, "invalid i16 block length");
        pair.equal(len, None)?;
    }
    ensure!(
        strategy_srefs.finalize() == value_srefs.finalize(),
        "sref sets differ"
    );
    ensure!(
        pair.a.read(&mut wall_a[..1])? == 0 && pair.b.read(&mut wall_b[..1])? == 0,
        "trailing payload"
    );
    println!(
        "{}",
        serde_json::json!({
            "payload_bit_equal_except_wall_secs": true,
            "comparison_method": "bounded v2 field-stream comparison, exact bytes",
            "payload_bytes": [pair.position, pair.position],
            "normalized_payload_blake3": pair.hash.finalize().to_hex().as_str(),
            "strategy_blocks": blocks, "value_blocks": values,
            "wall_secs": [f64::from_le_bytes(wall_a), f64::from_le_bytes(wall_b)],
            "file_bytes_equal": files_equal(old, new)?,
        })
    );
    Ok(())
}

struct FieldStreams<R> {
    a: R,
    b: R,
    hash: blake3::Hasher,
    position: u64,
    left: Vec<u8>,
    right: Vec<u8>,
}

impl<R: Read> FieldStreams<R> {
    fn varint(&mut self) -> Result<u64> {
        let mut value = 0;
        for shift in (0..=63).step_by(7) {
            let mut a = [0];
            let mut b = [0];
            self.a.read_exact(&mut a)?;
            self.b.read_exact(&mut b)?;
            anyhow::ensure!(a == b, "varint differs at byte {}", self.position);
            let part = a[0] & 0x7f;
            anyhow::ensure!(shift < 63 || part <= 1, "varint overflow");
            self.hash.update(&a);
            self.position += 1;
            value |= (part as u64) << shift;
            if a[0] < 0x80 {
                anyhow::ensure!(shift == 0 || part != 0, "noncanonical varint");
                return Ok(value);
            }
        }
        bail!("unterminated varint")
    }

    fn equal(&mut self, mut len: u64, mut extra: Option<&mut blake3::Hasher>) -> Result<()> {
        while len > 0 {
            let count = len.min(self.left.len() as u64) as usize;
            self.a.read_exact(&mut self.left[..count])?;
            self.b.read_exact(&mut self.right[..count])?;
            anyhow::ensure!(
                self.left[..count] == self.right[..count],
                "payload differs at byte {}",
                self.position
            );
            self.hash.update(&self.left[..count]);
            if let Some(hash) = extra.as_deref_mut() {
                hash.update(&self.left[..count]);
            }
            self.position += count as u64;
            len -= count as u64;
        }
        Ok(())
    }
}

fn files_equal(a: &str, b: &str) -> Result<bool> {
    let len = std::fs::metadata(a)?.len();
    if len != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut a = std::fs::File::open(a)?;
    let mut b = std::fs::File::open(b)?;
    let mut left = vec![0; 64 * 1024];
    let mut right = vec![0; left.len()];
    let mut remaining = len;
    while remaining > 0 {
        let count = remaining.min(left.len() as u64) as usize;
        a.read_exact(&mut left[..count])?;
        b.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] {
            return Ok(false);
        }
        remaining -= count as u64;
    }
    Ok(true)
}

/// Bounded comparison of v5 raw arenas, validating each digest. Only wall
/// time and operational [run] config may differ; recorded threads must match.
fn compare_checkpoints(old: &str, new: &str) -> Result<()> {
    use anyhow::ensure;
    #[derive(serde::Deserialize, PartialEq)]
    enum Backend {
        F32,
        I16,
        I16F32Avg,
    }
    #[derive(serde::Deserialize, PartialEq)]
    struct Metadata {
        config_toml: Option<String>,
        elapsed_secs: Option<f64>,
        iteration: u64,
        backend: Backend,
        lengths: [u64; 4],
    }
    fn open(path: &str) -> Result<(impl Read, Metadata, blake3::Hasher)> {
        let mut file = std::fs::File::open(path)?;
        let mut header = [0; hu_postflop::checkpoint::HEADER_LEN];
        file.read_exact(&mut header)?;
        ensure!(
            &header[..10] == b"SLVRCKPT\x05\x00",
            "expected checkpoint v5"
        );
        let mut decoder = zstd::stream::read::Decoder::new(file)?;
        decoder.window_log_max(20)?;
        let mut length = [0; 4];
        decoder.read_exact(&mut length)?;
        let len = u32::from_le_bytes(length) as usize;
        ensure!(len <= 16 * 1024 * 1024, "metadata too large");
        let mut bytes = vec![0; len];
        decoder.read_exact(&mut bytes)?;
        let meta: Metadata = postcard::from_bytes(&bytes)?;
        ensure!(
            meta.iteration == u64::from_le_bytes(header[42..50].try_into()?),
            "iteration mismatch"
        );
        if let Some(config) = &meta.config_toml {
            ensure!(
                header[10..42] == hu_postflop::prepare::compatibility_hash(config)?,
                "config hash mismatch"
            );
        }
        let mut hash = blake3::Hasher::new();
        hash.update(&header);
        hash.update(&length);
        hash.update(&bytes);
        Ok((decoder, meta, hash))
    }
    let (mut a, mut ma, mut ha) = open(old)?;
    let (mut b, mut mb, mut hb) = open(new)?;
    ma.elapsed_secs = None;
    mb.elapsed_secs = None;
    let mut threads = Vec::new();
    for meta in [&mut ma, &mut mb] {
        let mut config: toml_edit::DocumentMut = meta.config_toml.as_deref().unwrap().parse()?;
        threads.push(config["run"]["threads"].to_string());
        config.remove("run");
        meta.config_toml = Some(config.to_string());
    }
    ensure!(threads[0] == threads[1], "recorded thread counts differ");
    ensure!(
        ma == mb,
        "checkpoint metadata differs excluding elapsed_secs and [run]"
    );
    let mut left = vec![0; 65536];
    let mut right = vec![0; left.len()];
    let mut elements = 0u64;
    let mut arena_hashes = [blake3::Hasher::new(), blake3::Hasher::new()];
    for (index, len) in ma.lengths.into_iter().enumerate() {
        let integer = match ma.backend {
            Backend::F32 => false,
            Backend::I16 => index < 2,
            Backend::I16F32Avg => index == 0,
        };
        let width = if integer { 2 } else { 4 };
        let mut remaining = len;
        while remaining > 0 {
            let count = remaining.min((left.len() / width) as u64) as usize;
            let size = count * width;
            a.read_exact(&mut left[..size])?;
            b.read_exact(&mut right[..size])?;
            ha.update(&left[..size]);
            hb.update(&right[..size]);
            arena_hashes[0].update(&left[..size]);
            arena_hashes[1].update(&right[..size]);
            ensure!(
                left[..size] == right[..size],
                "arena {index} differs at element {}",
                len - remaining
            );
            elements += count as u64;
            remaining -= count as u64;
        }
    }
    for (reader, hash) in [(&mut a, ha), (&mut b, hb)] {
        let mut digest = [0; 32];
        reader.read_exact(&mut digest)?;
        ensure!(
            hash.finalize().as_bytes() == &digest,
            "checkpoint digest mismatch"
        );
        ensure!(
            reader.read(&mut digest[..1])? == 0,
            "trailing checkpoint bytes"
        );
    }
    println!(
        "{}",
        serde_json::json!({
            "iteration": ma.iteration, "storage": match ma.backend { Backend::F32 => "f32", Backend::I16 => "i16", Backend::I16F32Avg => "i16-f32avg" },
            "recorded_threads": threads, "elements": elements, "bit_equal": true,
            "signed_zero_bit_differences": 0,
            "other_bit_differences": 0,
            "arena_blake3": arena_hashes.map(|h| h.finalize().to_hex().to_string()),
            "digests_valid": true,
        })
    );
    Ok(())
}
