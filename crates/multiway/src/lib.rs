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
pub mod sampler;
pub mod settlement;
pub mod solver;
pub mod types;

pub use abstraction::{
    AbstractionError, BucketContext, BucketId, BucketPath, FeatureHashAbstraction,
    FeatureHashParams, MultiwayAbstraction, RolloutAbstractionError, RolloutArtifactError,
    RolloutFeatures, RolloutKMeansAbstraction, RolloutKMeansBuilder, RolloutKMeansParams,
    RolloutTrainingParams, StreetBucketCounts, TableAbstractionAdapter,
};
pub use betting::{Action, BettingState, SeatStatus};
pub use checkpoint::{
    CHECKPOINT_VERSION, CheckpointError, MultiwayCheckpoint, MultiwayCheckpointHeader,
};
pub use config::{
    AbstractionConfig, ActiveOpponentBucketConfig, BettingConfig, MultiwayConfig, SeatConfig,
    StreetBettingConfig, UtilityConfig,
};
pub use holdem::{HoldemGame, HoldemGameError};
pub use icm::{IcmDeltaEstimate, IcmError, IcmEstimate, IcmMode, estimate_icm, terminal_icm_delta};
pub use sampler::{CountedSample, DealSampler, SampleError, SampledWorld, SamplingDiagnostics};
pub use settlement::{PotLayer, Settlement};
pub use solver::{
    ActionProbability, ExternalSamplingGame, HistoryEntry, HistoryKey, InfoKey, MultiwaySolver,
    PolicyColumn, PolicyEntry, PrivateInfo, ProfileEstimate, ProfileEvaluation, SolverConfig,
    SolverError, SolverMetrics, SolverState,
};
pub use types::{MwChips, SeatId, SeatMask, SeatVec, Street};
