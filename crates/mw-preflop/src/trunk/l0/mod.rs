//! Experimental, full-width best response in the hero-conditioned L0 model.
//!
//! The tree, class profile, model and evaluation are independent values. Core
//! evaluation performs no I/O. Multiway deals ignore opponent/opponent overlap
//! and folded cards; T3 and larger showdowns retain their sampling uncertainty.

pub(crate) mod eval;
pub(crate) mod leaves;
mod profile;
mod real;
pub(crate) mod solve;
mod tree;

pub use eval::{Evaluation, EvaluationOptions, LocalGain, Model, SeatEvaluation, Tables, evaluate};
pub use profile::{ClassProfileDocument, Profile, ProfileNode};
pub use real::{
    Estimate, FOLLOW, FittedResponses, FittedSeat, GatedResponse, GatedSeat, RealEvaluation,
    RealOptions, RealSeat, ResponseEstimate, evaluate_real, fit_real_responses,
};
pub use solve::{
    Checkpoint, CheckpointSeat, Progress, Solution, SolveOptions, SolveTimings, solve,
};
pub use tree::{DecisionExport, FlopLeaves, Node, Terminal, Tree, TreeExport};

/// Rebuild a legacy artifact's game without loading its production abstraction.
pub fn game_from_solution(
    metadata: &crate::mwsol::MultiwaySolutionMetadata,
) -> anyhow::Result<crate::HoldemGame<crate::FeatureHashAbstraction>> {
    use crate::ExternalSamplingGame;
    crate::prepare::require_artifact_config(&metadata.config_toml)?;
    let game = game_from_config(&metadata.config_toml, std::path::Path::new("embedded.toml"))?;
    anyhow::ensure!(
        game.game_fingerprint() == metadata.game_fingerprint,
        "solution game fingerprint mismatch"
    );
    Ok(game)
}

pub fn game_from_config(
    raw: &str,
    path: &std::path::Path,
) -> anyhow::Result<crate::HoldemGame<crate::FeatureHashAbstraction>> {
    let p = crate::prepare::prepare(raw, path)?;
    let game = crate::HoldemGame::new(
        &p.lowered.game,
        &p.lowered.utility,
        &p.lowered.rake,
        crate::FeatureHashAbstraction::default(),
    )?;
    game.require_l0_chip_ev()?;
    Ok(game)
}

#[cfg(test)]
mod tests;
