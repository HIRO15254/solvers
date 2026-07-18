use super::*;

#[derive(Debug, thiserror::Error)]
pub enum SolverError {
    #[error(transparent)]
    Sample(#[from] SampleError),
    #[error("multiway solver requires {MIN_SEATS}..={MAX_SEATS} players, found {found}")]
    PlayerCount { found: usize },
    #[error("game has {game} players but sampler has {sampler}")]
    SamplerPlayerCount { game: usize, sampler: usize },
    #[error("memory limit must be positive")]
    ZeroMemoryLimit,
    #[error("traversal depth limit must be positive")]
    ZeroDepthLimit,
    #[error("exploration epsilon must be finite and in [0, 1], found {epsilon}")]
    InvalidExploration { epsilon: f64 },
    #[error("invalid private information: {0}")]
    InvalidPrivateInfo(&'static str),
    #[error("action labels are invalid: {0}")]
    InvalidActionLabels(&'static str),
    #[error("action labels changed at {key:?}")]
    ActionLabelsChanged { key: InfoKey },
    #[error("actor {actor} is outside a {num_players}-player game")]
    InvalidActor { actor: usize, num_players: usize },
    #[error("non-terminal state for actor {actor} has no actions")]
    NoActions { actor: usize },
    #[error("public game traversal exceeded depth limit {limit}")]
    DepthLimit { limit: u32 },
    #[error("terminal utility for seat {seat} is not finite: {utility}")]
    NonFiniteUtility { seat: usize, utility: f64 },
    #[error("policy action count changed at {key:?}: stored {stored}, current {current}")]
    ActionCountChanged {
        key: InfoKey,
        stored: usize,
        current: usize,
    },
    #[error("sparse policy memory cap {limit} bytes exceeded; next node needs {needed} bytes")]
    MemoryLimit { limit: u64, needed: u64 },
    #[error("discount cadence must be positive")]
    ZeroDiscountCadence,
    #[error("sweep batch size must be positive")]
    ZeroSweepBatch,
    #[error("multiway parallel worker count must be positive")]
    ZeroThreads,
    #[error("failed to build deterministic multiway worker pool: {0}")]
    ThreadPoolBuild(String),
    #[error("checkpoint algorithm configuration does not match the current solver configuration")]
    ResumeConfigurationMismatch,
    #[error("complete parallel sweeps cannot start from a partial-sweep solver state")]
    IncompleteSweepState,
    #[error("profile evaluation sample count must be positive")]
    ZeroEvaluationSamples,
    #[error(
        "evaluate_node_actions path index {index} at depth {depth} is out of range for \
         {actions} legal actions"
    )]
    EvaluationPathIndexOutOfRange {
        depth: usize,
        index: usize,
        actions: usize,
    },
    #[error(
        "evaluate_node_actions path hit a terminal state at depth {depth} before reaching the \
         requested node"
    )]
    EvaluationPathTerminalEarly { depth: usize },
    #[error("numeric accumulation exceeded f32 storage")]
    NumericOverflow,
    #[error("counter overflow")]
    CounterOverflow,
    #[error("traversal count overflow")]
    TraversalCountOverflow,
    #[error("policy memory accounting overflow")]
    MemoryAccountingOverflow,
    #[error("solver state version {found} is unsupported (expected {expected})")]
    StateVersion { found: u16, expected: u16 },
    #[error("invalid solver state: {0}")]
    InvalidState(&'static str),
    #[error("duplicate policy key in solver state: {0:?}")]
    DuplicatePolicy(InfoKey),
    #[error("duplicate public history key in solver state: {0:?}")]
    DuplicateHistory(HistoryKey),
    #[error("public history hash collision or unstable action label at {0:?}")]
    HistoryCollision(HistoryKey),
    #[error("public history actor/action index does not fit checkpoint format")]
    HistoryIndexOverflow,
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(
        "dense tree node {node} expected {expected} actions but the game produced {found}; the \
         betting tree changed since the arena was preallocated"
    )]
    TreeNodeMismatch {
        node: NodeId,
        expected: usize,
        found: usize,
    },
    #[error("dense policy entry at {key:?} does not map to an enumerated dense arena slot")]
    UnmappedDenseEntry { key: InfoKey },
    #[error("dense history entry {0:?} does not match the enumerated public tree")]
    UnmappedDenseHistory(HistoryKey),
    #[error(
        "SolverConfig::traverser_vector requires recall = \"street\" (the dense arena); the \
         current game uses RecallMode::Full"
    )]
    VectorTraverserRequiresStreetRecall,
    #[error("SolverConfig::prune requires SolverConfig::traverser_vector to be true")]
    PruneRequiresVector,
    #[error("prune threshold must be finite and strictly negative, found {0}")]
    PruneThresholdNotNegative(f64),
    #[error("prune skip probability must be finite and in [0, 1], found {0}")]
    PruneSkipProbabilityOutOfRange(f64),
}
