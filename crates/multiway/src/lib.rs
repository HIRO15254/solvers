//! Sampled multiway NLHE solving for two through nine table seats.
//!
//! This crate is deliberately separate from the exact heads-up vector
//! engine. It samples one physical card world shared by every seat.
//! Production uses current-street recall and allocates the complete public
//! tree × bucket × action policy arena before sweep 0. The card abstraction
//! is an EHS² percentile table.

pub mod abstraction;
pub mod betting;
pub mod checkpoint;
pub mod config;
pub mod holdem;
pub mod icm;
pub mod rake_condition;
#[cfg(feature = "research-abstractions")]
mod research_draw_abstraction;
pub mod sampler;
pub mod settlement;
pub mod solver;
pub mod tree;
pub mod tree_rules;
pub mod types;

pub use abstraction::{
    AbstractionError, BucketContext, BucketId, BucketPath, FeatureHashAbstraction,
    FeatureHashParams, MultiwayAbstraction, MultiwayAbstractionBackend, StreetBucketCounts,
    TableAbstractionAdapter, ehs2_table_fingerprint,
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
pub use rake_condition::{CompiledRakeCondition, RakeConditionContext};
#[cfg(feature = "research-abstractions")]
pub use research_draw_abstraction::{DrawAwareAbstraction, DrawAwareAbstractionError};
pub use sampler::{CountedSample, DealSampler, SampleError, SampledWorld, SamplingDiagnostics};
pub use settlement::{PotLayer, Settlement};
#[cfg(feature = "research-regret-sampling")]
pub use solver::RaisedPreflopResearchWork;
pub use solver::{
    ActionProbability, CandidatePolicyCoverage, ConditionalPrefixEvaluation,
    ConditionalProfileEvaluation, ConditionalStreetCoverage,
    CounterfactualEndpointDeviationEvaluation, DenseNodeContext, DeviatorPolicy,
    DeviatorTrainingCoverage, DeviatorTrainingResult, EndpointDeviationConfig,
    EndpointDeviationEvaluation, EndpointDeviationFit, EndpointDeviationHeldOut,
    EndpointDeviationRow, EndpointDeviationSampling, ExternalSamplingGame, HistoryEntry,
    HistoryKey, InfoKey, MultiwaySolver, PolicyArenaAllocation, PolicyColumn, PolicyEntry,
    PrefixPolicyCoverage, PrefixProfileEvaluation, PreflopConditionalPrefixEvaluation,
    PreflopConditionalProfileEvaluation, PreflopProposalMetadata, PreflopSupportCensus,
    PreflopSupportNode, PrivateInfo, ProfileEstimate, ProfileEvaluation, ProfileVariant,
    PublicActionDestination, PublicNodeAction, PublicNodeView, ReferenceDeviationCoverage,
    ReferenceDeviationEvaluation, ReferenceDeviationWorld, SolverConfig, SolverError,
    SolverMetrics, SolverState, StreetVisitCounts, WeightedEstimate,
    abstraction_fingerprint_with_recall,
};
#[cfg(feature = "research-average-sampling")]
pub use solver::{
    AverageSamplingDiagnosticsConfig, AverageSamplingDiagnosticsResult,
    AverageSamplingResearchConfig, AverageSamplingResearchCoverageEvaluation,
    AverageSamplingResearchEvaluation, AverageSamplingResearchHistory,
    AverageSamplingResearchResult, AverageSamplingResearchRowStatus,
    AverageSamplingResearchStrategyRow, AverageSamplingResearchVariant,
    AverageSamplingWithDiagnostics, ResearchPolicySupport, ResearchPolicySupportRow,
};
pub use solver::{
    PreflopDeviationConfig, PreflopDeviationEvaluation, PreflopDeviationFitMode,
    PreflopDeviationHeldOut,
};
pub use tree::{PublicTree, TreeError};
pub use types::{MwChips, SeatId, SeatMask, SeatVec, Street};
