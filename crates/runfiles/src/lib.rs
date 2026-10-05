//! Run-directory contracts, progress metrics, and config-file hashing.

mod hash;
mod metrics;
mod run;

pub use hash::{config_hash, config_hash_hex};
pub use metrics::{MetricsRow, MetricsWriter};
pub use run::{
    RUN_CHECKPOINT_FILE, RUN_CONFIG_FILE, RUN_EVENTS_FILE, RUN_HU_CHECKPOINT_FILE,
    RUN_HU_SOLUTION_FILE, RUN_MANIFEST_FILE, RUN_MANIFEST_VERSION, RUN_PROGRESS_FILE,
    RUN_RESULT_FILE, RUN_SOLUTION_FILE, RunEvent, RunEventLevel, RunEventLog, RunEventPayload,
    RunManifest, RunState, is_run_directory, last_progress_row, read_events, unix_millis,
};
