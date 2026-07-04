//! Checkpoint (`.ckpt`) and metrics (JSONL) file formats for solver runs,
//! plus config-file hashing used to stamp checkpoints against the config
//! that produced them. Also home to the `.sol` viewer-artifact codec: a
//! small quantized read-only export of a solved strategy, distinct from
//! `.ckpt`'s full-precision resumable state.
//!
//! This crate is the persistence layer for the M3 "research workflow"
//! slice: the `cli` crate's `solve`/`resume`/`bench` subcommands build on
//! it to autosave progress and let a killed run pick back up exactly where
//! it left off.

mod checkpoint;
mod hash;
mod metrics;
mod sol;

pub use checkpoint::{
    Checkpoint, CheckpointError, CheckpointHeader, HEADER_LEN, peek_header, read_checkpoint,
    write_checkpoint,
};
pub use hash::{config_hash, config_hash_hex};
pub use metrics::{MetricsRow, MetricsWriter};
pub use sol::{
    HEADER_LEN as SOL_HEADER_LEN, SolError, SolHeader, SolMeta, SolPayload, StrategyBlock,
    StreetsStored, dequantize_probs, peek_sol_header, quantize_probs, read_sol, write_sol,
};
