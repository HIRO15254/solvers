//! Sampled multiway NLHE solving for two through nine table seats.
//!
//! This crate is deliberately separate from the exact heads-up vector
//! engine.  It samples one physical card world shared by every seat, keeps
//! public betting history lazily, and stores only visited strategy buckets.

pub mod abstraction;
pub mod betting;
pub mod checkpoint;
pub mod config;
pub mod holdem;
pub mod icm;
mod rake_condition;
pub mod sampler;
pub mod settlement;
pub mod solver;
pub mod tree;
mod tree_rules;
pub mod types;

pub use abstraction::{
    AbstractionError, BucketContext, BucketId, BucketPath, FeatureHashAbstraction,
    FeatureHashParams, MultiwayAbstraction, MultiwayAbstractionBackend, RolloutAbstractionError,
    RolloutArtifactError, RolloutFeatures, RolloutKMeansAbstraction, RolloutKMeansBuilder,
    RolloutKMeansParams, RolloutTrainingParams, StreetBucketCounts, TableAbstractionAdapter,
    ehs2_table_fingerprint,
};
pub use betting::{Action, BettingState, SeatStatus};
pub use checkpoint::{
    CHECKPOINT_VERSION, CheckpointError, MultiwayCheckpoint, MultiwayCheckpointHeader,
};
pub use config::{
    AbstractionConfig, AbstractionKind, ActiveOpponentBucketConfig, BettingConfig, MultiwayConfig,
    RakeAllocation, RakeRounding, RecallMode, SeatConfig, StreetBettingConfig, UtilityConfig,
};
pub use holdem::{HoldemGame, HoldemGameError};
pub use icm::{IcmDeltaEstimate, IcmError, IcmEstimate, IcmMode, estimate_icm, terminal_icm_delta};
pub use sampler::{CountedSample, DealSampler, SampleError, SampledWorld, SamplingDiagnostics};
pub use settlement::{PotLayer, Settlement};
pub use solver::{
    ActionProbability, DenseNodeContext, DeviatorPolicy, ExternalSamplingGame, HistoryEntry,
    HistoryKey, InfoKey, MultiwaySolver, PolicyColumn, PolicyEntry, PrivateInfo, ProfileEstimate,
    ProfileEvaluation, ProfileVariant, SolverConfig, SolverError, SolverMetrics, SolverState,
};
pub use tree::{PublicTree, TreeError};
pub use types::{MwChips, SeatId, SeatMask, SeatVec, Street};
