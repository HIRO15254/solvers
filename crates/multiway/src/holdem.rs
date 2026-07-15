//! Production adapter joining the betting, deal, settlement, utility, and
//! abstraction layers into one generative no-limit Hold'em game.

use cards::combo_cards;

use crate::abstraction::{BucketContext, BucketPath, MultiwayAbstraction};
use crate::betting::{Action, BettingState, HandPhase};
use crate::config::{
    CompiledRake, MultiwayConfig, RakeConfig, UtilityConfig, ValidatedMultiwayConfig,
};
use crate::icm::terminal_icm_delta;
use crate::sampler::{DealSampler, SampleError, SampledWorld};
use crate::settlement::{Settlement, SettlementError, settle_showdown, settle_uncontested};
use crate::solver::{ExternalSamplingGame, PrivateInfo};
use crate::types::{MwChips, SeatId, SeatVec, Street};

#[derive(Clone)]
enum UtilityRuntime {
    ChipEv,
    TournamentIcm {
        outside_field: Vec<MwChips>,
        payouts: Vec<f64>,
        samples: u64,
        seed: u64,
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
            } => UtilityRuntime::TournamentIcm {
                outside_field: outside_field
                    .iter()
                    .map(|player| MwChips::try_from_bb(player.stack_bb))
                    .collect::<Result<Vec<_>, _>>()?,
                payouts: payouts.clone(),
                samples: *samples,
                seed: *seed,
            },
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
        match action {
            Action::Fold => "fold".to_string(),
            Action::Check => "check".to_string(),
            Action::Call { amount, all_in } => {
                format!(
                    "call:{}{}",
                    amount.raw(),
                    if *all_in { ":all-in" } else { "" }
                )
            }
            Action::BetTo { to, all_in, .. } => {
                format!(
                    "bet-to:{}{}",
                    to.raw(),
                    if *all_in { ":all-in" } else { "" }
                )
            }
            Action::RaiseTo { to, all_in, .. } => {
                format!(
                    "raise-to:{}{}",
                    to.raw(),
                    if *all_in { ":all-in" } else { "" }
                )
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

    fn bucket_path(&self, state: &BettingState, world: &SampledWorld, actor: usize) -> BucketPath {
        let combo = world.hole_combo(actor);
        let bucket = |street: Street| {
            if street.index() <= state.street.index() {
                self.abstraction.bucket(BucketContext {
                    street,
                    board: world.board(street),
                    combo,
                    active_opponents: state.players_on_street(street).saturating_sub(1),
                })
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
                outside_field,
                payouts,
                samples,
                seed,
            } => {
                let starting = SeatVec::try_new(
                    self.config
                        .seats
                        .iter()
                        .map(|seat| seat.starting_stack)
                        .collect(),
                )
                .expect("configured seat count is valid");
                Ok(terminal_icm_delta(
                    &starting,
                    &settlement.final_stacks,
                    outside_field,
                    payouts,
                    *samples,
                    *seed,
                )?
                .deltas)
            }
        }
    }
}

impl<A: MultiwayAbstraction> ExternalSamplingGame for HoldemGame<A> {
    type State = BettingState;

    fn num_players(&self) -> usize {
        self.config.seats.len()
    }

    fn root_state(&self) -> Self::State {
        self.root.clone()
    }

    fn actor(&self, state: &Self::State) -> Option<usize> {
        state.to_act.map(SeatId::index)
    }

    fn num_actions(&self, state: &Self::State) -> usize {
        self.legal_actions(state).len()
    }

    fn next_state(&self, state: &Self::State, action_index: usize) -> Self::State {
        let actions = self.legal_actions(state);
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
        next.apply(action, self.betting_for_state(state))
            .expect("legal action must apply");
        next
    }

    fn action_label(&self, state: &Self::State, action_index: usize) -> String {
        let actions = self.legal_actions(state);
        actions
            .get(action_index)
            .map(HoldemGame::<A>::action_label)
            .unwrap_or_else(|| format!("invalid-action:{action_index}"))
    }

    fn bucket(&self, state: &Self::State, world: &SampledWorld, actor: usize) -> PrivateInfo {
        let opponents = state.non_folded_mask().len().saturating_sub(1) as u8;
        PrivateInfo::from_path(
            state.street,
            opponents,
            self.bucket_path(state, world, actor),
        )
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
        let trait_labels: Vec<_> = (0..game.num_actions(&root))
            .map(|action| game.action_label(&root, action))
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
            flop = game.next_state(&flop, passive);
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
