//! Saved-state fingerprints and `.sol` block equivalence.
use anyhow::{Result, bail};
use hu_engine::StorageState;
use std::io::Read;
use std::path::Path;

fn hash_f32(hash: &mut blake3::Hasher, values: &[f32]) {
    let mut bytes = [0; 65536];
    for chunk in values.chunks(bytes.len() / 4) {
        for (v, dst) in chunk.iter().zip(bytes.as_chunks_mut::<4>().0) {
            *dst = v.to_le_bytes();
        }
        hash.update(&bytes[..chunk.len() * 4]);
    }
}
fn hash_i16(hash: &mut blake3::Hasher, values: &[i16]) {
    let mut bytes = [0; 65536];
    for chunk in values.chunks(bytes.len() / 2) {
        for (v, dst) in chunk.iter().zip(bytes.as_chunks_mut::<2>().0) {
            *dst = v.to_le_bytes();
        }
        hash.update(&bytes[..chunk.len() * 2]);
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
        Some("solution") => compare_solutions(&args[2], &args[3], &args[4..])?,
        _ => bail!(
            "verify_save checkpoint PATH | checkpoint-compare OLD NEW | solution OLD NEW [--ignore-cfr-precision] [--ignore-run] [--equivalent-auto-cadence] | finalization-memory CONFIG"
        ),
    }
    Ok(())
}

/// Compare two `.sol` files chunk by chunk without retaining decoded blocks.
/// Only wall_secs, and the config sections the flags name, may differ.
fn compare_solutions(old: &str, new: &str, flags: &[String]) -> Result<()> {
    // Explicit opt-ins: the legacy/PF5 comparison drops the recorded
    // precision, and thread-count comparisons drop the operational [run].
    for flag in flags {
        if !matches!(
            flag.as_str(),
            "--ignore-cfr-precision" | "--ignore-run" | "--equivalent-auto-cadence"
        ) {
            bail!("unknown solution flag {flag}");
        }
    }
    let ignore_precision = flags.iter().any(|s| s == "--ignore-cfr-precision");
    let ignore_run = flags.iter().any(|s| s == "--ignore-run");
    let equivalent_auto = flags.iter().any(|s| s == "--equivalent-auto-cadence");
    let mut metadata = Vec::new();
    let mut digests = Vec::new();
    let mut counts = Vec::new();
    let mut walls = Vec::new();
    for path in [old, new] {
        let mut reader = hu_postflop::sol::SolReader::open(Path::new(path))?;
        let mut normalized = reader.metadata().clone();
        walls.push(normalized.meta.wall_secs);
        normalized.meta.wall_secs = 0.0;
        if ignore_precision || ignore_run || equivalent_auto {
            let mut config: toml_edit::DocumentMut = normalized.config_toml.parse()?;
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
            if equivalent_auto {
                // PF10 records auto explicitly even though a targetless run
                // computes identically to legacy fixed 25. Reject any other
                // cadence or target rather than masking a solver difference.
                let stop = &mut config["solver"]["stop"];
                if stop.get("target").is_some()
                    || !(stop["check_every"].as_str() == Some("auto")
                        || stop["check_every"].as_integer() == Some(25))
                {
                    bail!("auto cadence equivalence requires no target and auto or 25");
                }
                stop["check_every"] = toml_edit::value(25);
            }
            normalized.config_toml = config.to_string();
        }
        metadata.push(postcard::to_allocvec(&normalized)?);
        let mut strategy = (blake3::Hasher::new(), 0u64);
        let mut values = (blake3::Hasher::new(), 0u64);
        reader.visit_blocks(
            |sref, probs| {
                strategy.0.update(&sref.to_le_bytes());
                strategy.0.update(&(probs.len() as u64).to_le_bytes());
                strategy.0.update(probs);
                strategy.1 += 1;
            },
            |sref, scale, bytes| {
                values.0.update(&sref.to_le_bytes());
                values.0.update(&scale.to_le_bytes());
                values.0.update(&(bytes.len() as u64).to_le_bytes());
                values.0.update(bytes);
                values.1 += 1;
            },
        )?;
        let index = reader.read_index(|stream| {
            let mut hasher = blake3::Hasher::new();
            std::io::copy(stream, &mut hasher)?;
            Ok(hasher.finalize())
        })?;
        let mut digest = blake3::Hasher::new();
        digest.update(metadata.last().expect("pushed"));
        digest.update(index.as_bytes());
        digest.update(strategy.0.finalize().as_bytes());
        digest.update(values.0.finalize().as_bytes());
        digests.push(digest.finalize());
        counts.push((strategy.1, values.1));
    }
    if metadata[0] != metadata[1] {
        bail!(".sol metadata differs (excluding wall_secs)");
    }
    if digests[0] != digests[1] {
        bail!(".sol index or blocks differ");
    }
    println!(
        "{}",
        serde_json::json!({
            "payload_bit_equal_except_wall_secs": true,
            "config_precision_excluded": ignore_precision,
            "config_run_excluded": ignore_run,
            "targetless_auto_cadence_normalized": equivalent_auto,
            "normalized_payload_blake3": digests[0].to_hex().as_str(),
            "strategy_blocks": counts[0].0,
            "value_blocks": counts[0].1,
            "wall_secs": walls,
            "file_bytes_equal": files_equal(old, new)?,
        })
    );
    Ok(())
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
