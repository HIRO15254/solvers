//! Typed session construction. The caller supplies the ready card abstraction;
//! loading its operational cache is independent of the input contract.
use super::{Lowered, Run};
use crate::HoldemGame;
use crate::abstraction::MultiwayAbstraction;
use crate::checkpoint::{CheckpointRuntimeState, MultiwayCheckpoint};
use crate::solver::{
    MultiwaySolver, abstraction_fingerprint_with_recall, configuration_fingerprint_for_setup,
};
use spot::{Code, SpotError};
use std::path::Path;

pub struct Session<A: MultiwayAbstraction> {
    pub solver: MultiwaySolver<HoldemGame<A>>,
    pub game_config: crate::MultiwayConfig,
    pub run: Run,
    pub output: super::Output,
    pub config_toml: String,
    pub config_hash: [u8; 32],
    pub checkpoint_runtime: Option<CheckpointRuntimeState>,
}

/// Construct or restore a preallocated production solver from typed input.
/// `effective_config` is retained verbatim for artifacts, never reparsed as a game.
pub fn build_session<A: MultiwayAbstraction>(
    input: Lowered,
    abstraction: A,
    effective_config: String,
    resume_checkpoint: Option<&Path>,
) -> Result<Session<A>, SpotError> {
    let error = |e: String| SpotError::new(Code::NLH003, "solver", e);
    let game = HoldemGame::new(&input.game, &input.utility, &input.rake, abstraction)
        .map_err(|e| error(e.to_string()))?
        .with_tree_rule_hits();
    let sampler = game.deal_sampler().map_err(|e| error(e.to_string()))?;
    let mut checkpoint_runtime = None;
    let mut solver = if let Some(path) = resume_checkpoint {
        use crate::ExternalSamplingGame;
        let configuration = configuration_fingerprint_for_setup(&game, &sampler, input.solver);
        let abstraction =
            abstraction_fingerprint_with_recall(game.abstraction_fingerprint(), game.recall_mode());
        let checkpoint = MultiwayCheckpoint::load(path, configuration, abstraction)
            .map_err(|e| error(e.to_string()))?;
        if checkpoint.state.completed_sweeps > input.run.stop.max_sweeps {
            return Err(error(
                "checkpoint sweeps exceed solver.stop.max_sweeps".into(),
            ));
        }
        checkpoint_runtime = checkpoint.config_toml.as_ref().map(|_| checkpoint.runtime);
        MultiwaySolver::from_state_with_config_preallocated_with_threads(
            game,
            sampler,
            checkpoint.state,
            input.solver,
            input.run.threads,
        )
    } else {
        MultiwaySolver::new_preallocated_with_threads(
            game,
            sampler,
            input.solver,
            input.run.threads,
        )
    }
    .map_err(|e| error(e.to_string()))?;
    solver.game_mut().finish_tree_rule_hits();
    Ok(Session {
        solver,
        game_config: input.game,
        run: input.run,
        output: input.output,
        config_hash: runfiles::config_hash(effective_config.as_bytes()),
        config_toml: effective_config,
        checkpoint_runtime,
    })
}
