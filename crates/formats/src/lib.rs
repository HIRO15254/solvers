//! Checkpoint (`.ckpt`) and metrics (JSONL) file formats for solver runs,
//! plus config-file hashing used to stamp checkpoints against the config
//! that produced them. Also home to read-only `.sol` and `.mwsol` viewer
//! artifacts.

mod checkpoint;
mod hash;
mod metrics;
mod multiway;
mod mwsol;
mod sol;

pub use checkpoint::{
    Checkpoint, CheckpointError, CheckpointHeader, HEADER_LEN, peek_header, read_checkpoint,
    write_checkpoint,
};
pub use hash::{config_hash, config_hash_hex};
pub use metrics::{MetricsRow, MetricsWriter};
pub use multiway::{
    Estimate, MULTIWAY_SCHEMA_VERSION, MultiwayMetricsRow, MultiwayMetricsWriter,
    MultiwaySeatMetrics,
};
pub use mwsol::{
    MWSOL_FORMAT_VERSION, MWSOL_HEADER_LEN, MWSOL_MAX_PAGE_LIMIT, MWSOL_MIN_FORMAT_VERSION,
    MultiwayHistoryAction, MultiwayHistoryNode, MultiwaySeatResult, MultiwaySolution,
    MultiwaySolutionMetadata, MultiwayStrategyBlock, MultiwayStrategyKey, MwSolError, MwSolHeader,
    MwSolReader, MwSolStrategyPage, MwsolStorage, peek_mwsol_header, read_mwsol, write_mwsol,
    write_mwsol_with,
};
pub use sol::{
    HEADER_LEN as SOL_HEADER_LEN, SolError, SolHeader, SolMeta, SolPayload, StrategyBlock,
    StreetsStored, dequantize_probs, peek_sol_header, quantize_probs, read_sol, write_sol,
};
