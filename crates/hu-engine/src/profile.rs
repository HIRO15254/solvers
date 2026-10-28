//! Constraints and utility incentives indexed by action-node storage reference.

/// Per-storage-reference constraints and incentives, excluded from solver checkpoints.
#[derive(Clone, Debug)]
pub struct Profile {
    /// One optional lock for each action-node storage reference.
    pub locks: Vec<Option<NodeLock>>,
    /// One optional incentive for each action-node storage reference.
    pub incentives: Vec<Option<NodeIncentive>>,
}

/// Fixed action probabilities and the confidence of each private hand.
#[derive(Clone, Debug)]
pub struct NodeLock {
    /// Action-major probabilities; NaN marks a free action.
    pub fixed: Vec<f32>,
    /// Confidence per hand; zero ignores that hand's fixed probabilities.
    pub confidence: Vec<f32>,
}

/// Action-major utility bonuses, weighted by compatible opponent reach.
#[derive(Clone, Debug)]
pub struct NodeIncentive {
    /// Bonus per action and hand, in utility units.
    pub bonus: Vec<f32>,
    /// Evaluator-defined hand-space identifier.
    pub space: u32,
}

/// Invalid profile dimensions, probabilities, or evaluator capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileError(pub String);

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProfileError {}
