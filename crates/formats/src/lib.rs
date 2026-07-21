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

pub use checkpoint::{Checkpoint, CheckpointError, HEADER_LEN, read_checkpoint, write_checkpoint};
pub use hash::{config_hash, config_hash_hex};
pub use metrics::{MetricsRow, MetricsWriter};
pub use multiway::{
    Estimate, MULTIWAY_SCHEMA_VERSION, MultiwayMetricsRow, MultiwayMetricsWriter,
    MultiwaySeatMetrics,
};
pub use mwsol::{
    MWSOL_FORMAT_VERSION, MWSOL_HEADER_LEN, MWSOL_MAX_PAGE_LIMIT, MWSOL_MIN_FORMAT_VERSION,
    MultiwayHistoryAction, MultiwayHistoryNode, MultiwayPublicAction, MultiwayPublicState,
    MultiwaySeatResult, MultiwaySolution, MultiwaySolutionMetadata, MultiwayStrategyBlock,
    MultiwayStrategyKey, MultiwayStrategyWeight, MwSolError, MwSolReader, MwSolStrategyPage,
    MwsolStorage, write_mwsol_with,
};
pub use sol::{
    SolError, SolMeta, SolPayload, StrategyBlock, StreetsStored, dequantize_probs, quantize_probs,
    read_sol, write_sol,
};
