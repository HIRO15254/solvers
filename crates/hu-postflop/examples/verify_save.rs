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
            a.meta.wall_secs = 0.0;
            b.meta.wall_secs = 0.0;
            if a != b {
                bail!("v2 payloads differ (excluding wall_secs)");
            }
            println!(
                "v2 payload bit identical: {} strategy/value blocks",
                a.blocks.len()
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
        _ => bail!("verify_save checkpoint PATH | solution OLD NEW | compression PATH"),
    }
    Ok(())
}
