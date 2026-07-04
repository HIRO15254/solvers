//! Checkpoint (`.ckpt`) and metrics (JSONL) file formats for solver runs,
//! plus config-file hashing used to stamp checkpoints against the config
//! that produced them.
//!
//! This crate is the persistence layer for the M3 "research workflow"
//! slice: the `cli` crate's `solve`/`resume`/`bench` subcommands build on
//! it to autosave progress and let a killed run pick back up exactly where
//! it left off.

mod checkpoint;
mod hash;
mod metrics;

pub use checkpoint::{
    Checkpoint, CheckpointError, CheckpointHeader, HEADER_LEN, peek_header, read_checkpoint,
    write_checkpoint,
};
pub use hash::{config_hash, config_hash_hex};
pub use metrics::{MetricsRow, MetricsWriter};
