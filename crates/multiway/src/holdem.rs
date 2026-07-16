//! Production adapter joining the betting, deal, settlement, utility, and
//! abstraction layers into one generative no-limit Hold'em game.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cards::combo_cards;

use crate::abstraction::{BucketContext, BucketId, BucketPath, MultiwayAbstraction};
use crate::betting::{Action, BettingState, HandPhase};
use crate::config::{
    CompiledRake, MultiwayConfig, RakeConfig, RecallMode, UtilityConfig, ValidatedMultiwayConfig,
};
use crate::icm::{IcmEstimate, estimate_icm, terminal_icm_delta_with_baseline};
use crate::sampler::{DealSampler, SampleError, SampledWorld};
use crate::settlement::{Settlement, SettlementError, settle_showdown, settle_uncontested};
use crate::solver::{ExternalSamplingGame, PrivateInfo};
use crate::tree::DenseNodeContext;
use crate::types::{MwChips, SeatId, SeatVec, Street};

const ICM_TERMINAL_CACHE_ENTRIES: usize = 65_536;

#[derive(Clone)]
enum UtilityRuntime {
    ChipEv,
    TournamentIcm {
        starting: SeatVec<MwChips>,
        outside_field: Vec<MwChips>,
        payouts: Vec<f64>,
        samples: u64,
        seed: u64,
        baseline: IcmEstimate,
        terminal_cache: Arc<Mutex<HashMap<Vec<u64>, SeatVec<f64>>>>,
    },
}

/// A lazy public game. No betting tree is expanded up front; every child is
/// generated from a cloned [`BettingState`] only when MCCFR visits it.
pub struct HoldemGame<A> {
    config: ValidatedMultiwayConfig,
    root: BettingState,
    abstraction: A,
    utility: UtilityRuntime,
    rake: CompiledRake,
    game_fingerprint: [u8; 32],
}

impl<A: MultiwayAbstraction> HoldemGame<A> {
    pub fn new(
        config: &MultiwayConfig,
        utility: &UtilityConfig,
        rake: &RakeConfig,
        abstraction: A,
    ) -> Result<Self, HoldemGameError> {
        config.validate_economics(utility, rake)?;
        let validated = config.validated()?;
        let root = BettingState::new(&validated)?;
        let compiled_rake = rake.compile()?;
        let utility_runtime = match utility {
            UtilityConfig::ChipEv => UtilityRuntime::ChipEv,
            UtilityConfig::TournamentIcm {
                outside_field,
                payouts,
                samples,
                seed,
            } => {
                let outside_field = outside_field
                    .iter()
                    .map(|player| MwChips::try_from_bb(player.stack_bb))
                    .collect::<Result<Vec<_>, _>>()?;
                let starting = SeatVec::try_new(
                    validated
                        .seats
                        .iter()
                        .map(|seat| seat.starting_stack)
                        .collect(),
                )
                .expect("validated seat count is supported");
                let mut baseline_stacks = starting.as_slice().to_vec();
                baseline_stacks.extend_from_slice(&outside_field);
                let baseline = estimate_icm(&baseline_stacks, payouts, *samples, *seed)?;
                UtilityRuntime::TournamentIcm {
                    starting,
                    outside_field,
                    payouts: payouts.clone(),
                    samples: *samples,
                    seed: *seed,
                    baseline,
                    terminal_cache: Arc::new(Mutex::new(HashMap::new())),
                }
            }
        };

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.holdem.v1");
        let mut game_identity = config.clone();
        game_identity.abstraction.artifact_cache = None;
        hasher.update(&serde_json::to_vec(&game_identity)?);
        hasher.update(&serde_json::to_vec(utility)?);
        hasher.update(&serde_json::to_vec(rake)?);
        let game_fingerprint = *hasher.finalize().as_bytes();
        Ok(Self {
            config: validated,
            root,
            abstraction,
            utility: utility_runtime,
            rake: compiled_rake,
            game_fingerprint,
        })
    }

    pub fn config(&self) -> &ValidatedMultiwayConfig {
        &self.config
    }

    /// The card abstraction backing this game. Exposed so callers (e.g. the
    /// CLI, after a solve finishes) can persist any assignment-cache growth
    /// accumulated during the run without threading a second handle through
    /// `MultiwaySolver`.
    pub fn abstraction(&self) -> &A {
        &self.abstraction
    }

    pub fn deal_sampler(&self) -> Result<DealSampler, SampleError> {
        DealSampler::new(
            self.config
                .seats
                .iter()
                .map(|seat| seat.range.clone())
                .collect(),
        )
    }

    pub fn legal_actions(&self, state: &BettingState) -> Vec<Action> {
        state
            .legal_actions(self.betting_for_state(state))
            .expect("a game-created state must preserve betting invariants")
    }

    fn betting_for_state(&self, state: &BettingState) -> &crate::config::BettingConfig {
        state
            .to_act
            .and_then(|seat| self.config.seats[seat].betting.as_ref())
            .unwrap_or(&self.config.betting)
    }

    pub fn action_label(action: &Action) -> String {
        let mut label = String::new();
        Self::write_label(action, &mut label);
        label
    }

    /// Appends `action`'s label to `out` instead of allocating a fresh
    /// `String`, so hot-path callers can reuse one scratch buffer across an
    /// entire node's action list.
    fn write_label(action: &Action, out: &mut String) {
        use std::fmt::Write as _;
        match action {
            Action::Fold => out.push_str("fold"),
            Action::Check => out.push_str("check"),
            Action::Call { amount, all_in } => {
                let _ = write!(out, "call:{}", amount.raw());
                if *all_in {
                    out.push_str(":all-in");
                }
            }
            Action::BetTo { to, all_in, .. } => {
                let _ = write!(out, "bet-to:{}", to.raw());
                if *all_in {
                    out.push_str(":all-in");
                }
            }
            Action::RaiseTo { to, all_in, .. } => {
                let _ = write!(out, "raise-to:{}", to.raw());
                if *all_in {
                    out.push_str(":all-in");
                }
            }
        }
    }

    pub fn settle_terminal(
        &self,
        state: &BettingState,
        world: &SampledWorld,
    ) -> Result<Settlement, SettlementError> {
        match state.phase {
            HandPhase::Uncontested { .. } => settle_uncontested(state, self.rake),
            HandPhase::Runout | HandPhase::Showdown => {
                let holes = SeatVec::try_new(
                    (0..self.config.seats.len())
                        .map(|seat| {
                            let (first, second) = combo_cards(world.hole_combo(seat));
                            Some([first, second])
                        })
                        .collect(),
                )
                .expect("configured seat count is valid");
                settle_showdown(state, *world.runout(), &holes, self.rake)
            }
            HandPhase::Betting => Err(SettlementError::WrongPhase),
        }
    }

    /// Abstraction bucket for one `street` (assumed already reached),
    /// factored out so full-recall's [`Self::bucket_path`] and
    /// street-recall's single-street lookup (in [`ExternalSamplingGame::bucket`])
    /// share one implementation.
    fn bucket_for_street(
        &self,
        state: &BettingState,
        world: &SampledWorld,
        actor: usize,
        street: Street,
    ) -> BucketId {
        let combo = world.hole_combo(actor);
        self.abstraction.bucket(BucketContext {
            street,
            board: world.board(street),
            combo,
            active_opponents: state.players_on_street(street).saturating_sub(1),
        })
    }

    fn bucket_path(&self, state: &BettingState, world: &SampledWorld, actor: usize) -> BucketPath {
        let bucket = |street: Street| {
            if street.index() <= state.street.index() {
                self.bucket_for_street(state, world, actor, street)
            } else {
                0
            }
        };
        BucketPath {
            preflop: bucket(Street::Preflop),
            flop: bucket(Street::Flop),
            turn: bucket(Street::Turn),
            river: bucket(Street::River),
        }
    }

    fn utilities(&self, settlement: &Settlement) -> Result<SeatVec<f64>, HoldemGameError> {
        match &self.utility {
            UtilityRuntime::ChipEv => Ok(SeatVec::try_new(
                self.config
                    .seats
                    .seats()
                    .map(|seat| {
                        (settlement.final_stacks[seat].raw() as f64
                            - self.config.seats[seat].starting_stack.raw() as f64)
                            / crate::types::CHIPS_PER_BB as f64
                    })
                    .collect(),
            )
            .expect("configured seat count is valid")),
            UtilityRuntime::TournamentIcm {
                starting,
                outside_field,
                payouts,
                samples,
                seed,
                baseline,
                terminal_cache,
            } => {
                let key: Vec<u64> = settlement
                    .final_stacks
                    .iter()
                    .map(|stack| stack.raw())
                    .collect();
                if let Some(cached) = terminal_cache
                    .lock()
                    .map_err(|_| HoldemGameError::IcmCachePoisoned)?
                    .get(&key)
                    .cloned()
                {
                    return Ok(cached);
                }
                let deltas = terminal_icm_delta_with_baseline(
                    starting,
                    &settlement.final_stacks,
                    outside_field,
                    payouts,
                    *samples,
                    *seed,
                    baseline,
                )?
                .deltas;
                let mut cache = terminal_cache
                    .lock()
                    .map_err(|_| HoldemGameError::IcmCachePoisoned)?;
                if cache.len() < ICM_TERMINAL_CACHE_ENTRIES {
                    cache.entry(key).or_insert_with(|| deltas.clone());
                }
                Ok(deltas)
            }
        }
    }
}

impl<A: MultiwayAbstraction> ExternalSamplingGame for HoldemGame<A> {
    type State = BettingState;
    type Actions = Vec<Action>;

    fn num_players(&self) -> usize {
        self.config.seats.len()
    }

    fn root_state(&self) -> Self::State {
        self.root.clone()
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        state.to_act.map(SeatId::index)
    }

    /// Expands the node's legal actions exactly once; every other method
    /// below is handed this value instead of recomputing it.
    fn node_actions(&self, state: &Self::State) -> Self::Actions {
        self.legal_actions(state)
    }

    fn num_actions_of(&self, actions: &Self::Actions) -> usize {
        actions.len()
    }

    fn next_state_with(
        &self,
        state: &Self::State,
        actions: &Self::Actions,
        action_index: usize,
    ) -> Self::State {
        let action = actions
            .get(action_index)
            .unwrap_or_else(|| {
                panic!(
                    "action index {action_index} outside {} actions",
                    actions.len()
                )
            })
            .clone();
        let mut next = state.clone();
        next.apply_from_actions(action, actions)
            .expect("legal action must apply");
        next
    }

    fn write_action_label(&self, actions: &Self::Actions, action_index: usize, out: &mut String) {
        match actions.get(action_index) {
            Some(action) => HoldemGame::<A>::write_label(action, out),
            None => {
                use std::fmt::Write as _;
                let _ = write!(out, "invalid-action:{action_index}");
            }
        }
    }

    fn bucket(&self, state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
        let opponents = state.non_folded_mask().len().saturating_sub(1) as u8;
        match self.config.abstraction.recall {
            RecallMode::Full => PrivateInfo::from_path(
                state.street,
                opponents,
                self.bucket_path(state, world, actor),
            ),
            // Only the current street's bucket is ever computed: earlier
            // streets are never revisited (this is both the imperfect-recall
            // key and a speed bonus over full recall), and later streets stay
            // `UNREACHED_BUCKET`.
            RecallMode::Street => {
                let bucket = self.bucket_for_street(state, world, actor, state.street);
                PrivateInfo::from_current_bucket(state.street, opponents, bucket)
            }
        }
    }

    fn recall_mode(&self) -> RecallMode {
        self.config.abstraction.recall
    }

    fn bucket_count(&self, street: Street, active_opponents: u8) -> u32 {
        self.abstraction.num_buckets(street, active_opponents)
    }

    fn dense_node_context(&self, state: &Self::State) -> DenseNodeContext {
        DenseNodeContext {
            street: state.street,
            active_opponents: state.non_folded_mask().len().saturating_sub(1) as u8,
            bucket_active_opponents: state.players_on_street(state.street).saturating_sub(1),
        }
    }

    fn terminal_utilities(&self, state: &Self::State, world: &SampledWorld, utilities: &mut [f64]) {
        let result = self
            .settle_terminal(state, world)
            .map_err(HoldemGameError::from)
            .and_then(|settlement| self.utilities(&settlement));
        match result {
            Ok(values) if values.len() == utilities.len() => {
                utilities.copy_from_slice(values.as_slice());
            }
            _ => utilities.fill(f64::NAN),
        }
    }

    fn game_fingerprint(&self) -> [u8; 32] {
        self.game_fingerprint
    }

    fn abstraction_fingerprint(&self) -> [u8; 32] {
        self.abstraction.fingerprint()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HoldemGameError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Chips(#[from] crate::types::ChipAmountError),
    #[error(transparent)]
    Betting(#[from] crate::betting::BettingError),
    #[error(transparent)]
    Settlement(#[from] SettlementError),
    #[error(transparent)]
    Icm(#[from] crate::icm::IcmError),
    #[error("ICM terminal utility cache lock was poisoned")]
    IcmCachePoisoned,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::{FeatureHashAbstraction, FeatureHashParams};
    use crate::config::{AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, SeatConfig};
    use crate::solver::ExternalSamplingGame;
    use crate::types::SeatId;
    use cards::combo_index;

    fn config() -> MultiwayConfig {
        MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 10.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: AbstractionConfig::default(),
        }
    }

    #[test]
    fn adapter_generates_three_seat_root_and_structured_actions() {
        let game = HoldemGame::new(
            &config(),
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::new(FeatureHashParams {
                flop_buckets: 32,
                turn_buckets: 32,
                river_buckets: 32,
            })
            .unwrap(),
        )
        .unwrap();
        let root = game.root_state();
        assert_eq!(game.actor(&root), Some(0));
        let labels: Vec<_> = game
            .legal_actions(&root)
            .iter()
            .map(HoldemGame::<FeatureHashAbstraction>::action_label)
            .collect();
        assert!(labels.iter().any(|label| label == "fold"));
        assert!(labels.iter().any(|label| label.starts_with("raise-to:")));
        let actions = game.node_actions(&root);
        let trait_labels: Vec<_> = (0..game.num_actions_of(&actions))
            .map(|action| game.action_label_of(&actions, action))
            .collect();
        assert_eq!(labels, trait_labels);
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn actor_specific_betting_profile_overrides_global_sizes() {
        let mut config = config();
        let mut seat_profile = BettingConfig::default();
        seat_profile.preflop.bet_sizes = vec![crate::config::SizeSpec::ToBb { value: 7.0 }];
        seat_profile.preflop.include_allin = false;
        config.seats[0].betting = Some(seat_profile);
        let game = HoldemGame::new(
            &config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap();
        let root = game.root_state();
        let labels: Vec<_> = game
            .legal_actions(&root)
            .iter()
            .map(HoldemGame::<FeatureHashAbstraction>::action_label)
            .collect();
        assert!(labels.iter().any(|label| label == "raise-to:7000"));
        assert!(!labels.iter().any(|label| label == "raise-to:2500"));
    }

    #[test]
    fn future_runout_never_enters_preflop_or_flop_private_info() {
        let game = HoldemGame::new(
            &config(),
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap();
        let holes = [("As", "Ah"), ("Ks", "Kh"), ("Qd", "Qc")]
            .map(|(a, b)| combo_index(a.parse().unwrap(), b.parse().unwrap()))
            .to_vec();
        let first = SampledWorld::new(
            holes.clone(),
            ["2c", "3d", "4h", "5s", "6c"].map(|card| card.parse().unwrap()),
        )
        .unwrap();
        let second = SampledWorld::new(
            holes,
            ["2c", "3d", "4h", "Qs", "Jc"].map(|card| card.parse().unwrap()),
        )
        .unwrap();

        let root = game.root_state();
        let root_actor = game.actor(&root).unwrap();
        let root_first = game.bucket(&root, &first, root_actor);
        let root_second = game.bucket(&root, &second, root_actor);
        assert_eq!(root_first, root_second);
        assert!(
            root_first.bucket_path[1..]
                .iter()
                .all(|&bucket| bucket == crate::solver::UNREACHED_BUCKET)
        );

        let mut flop = root;
        while flop.street == Street::Preflop {
            let actions = game.legal_actions(&flop);
            let passive = actions
                .iter()
                .position(|action| matches!(action, Action::Check | Action::Call { .. }))
                .expect("call/check line reaches the flop");
            flop = game.next_state_with(&flop, &actions, passive);
        }
        assert_eq!(flop.street, Street::Flop);
        let flop_actor = game.actor(&flop).unwrap();
        let flop_first = game.bucket(&flop, &first, flop_actor);
        let flop_second = game.bucket(&flop, &second, flop_actor);
        assert_eq!(flop_first, flop_second);
        assert_ne!(flop_first.bucket_path[0], crate::solver::UNREACHED_BUCKET);
        assert_ne!(flop_first.bucket_path[1], crate::solver::UNREACHED_BUCKET);
        assert!(
            flop_first.bucket_path[2..]
                .iter()
                .all(|&bucket| bucket == crate::solver::UNREACHED_BUCKET)
        );
    }

    #[test]
    fn operational_artifact_path_does_not_change_game_identity() {
        let mut first = config();
        first.abstraction.artifact_cache = Some("cache/first.mwab".into());
        let mut second = first.clone();
        second.abstraction.artifact_cache = Some("other/second.mwab".into());
        let first = HoldemGame::new(
            &first,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap();
        let second = HoldemGame::new(
            &second,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap();
        assert_eq!(first.game_fingerprint(), second.game_fingerprint());
    }
}
