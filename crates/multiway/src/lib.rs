//! Sampled multiway NLHE solving for two through nine table seats.
//!
//! This crate is deliberately separate from the exact heads-up vector
//! engine. It samples one physical card world shared by every seat.
//! Production uses current-street recall and allocates the complete public
//! tree × bucket × action policy arena before sweep 0; the optional
//! `research-abstractions` feature retains historical sparse/full-recall and
//! rollout experimentation paths.

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
    ActionProbability, CandidatePolicyCoverage, DenseNodeContext, DeviatorPolicy,
    DeviatorTrainingCoverage, DeviatorTrainingResult, ExternalSamplingGame, HistoryEntry,
    HistoryKey, InfoKey, MultiwaySolver, OnlineTrainingEv, PolicyArenaAllocation, PolicyColumn,
    PolicyEntry, PrivateInfo, ProfileEstimate, ProfileEvaluation, ProfileVariant,
    ReferenceDeviationCoverage, ReferenceDeviationEvaluation, ReferenceDeviationWorld,
    SolverConfig, SolverError, SolverMetrics, SolverState, StreetVisitCounts,
    abstraction_fingerprint_with_recall,
};
pub use tree::{PublicTree, TreeError};
pub use types::{MwChips, SeatId, SeatMask, SeatVec, Street};
