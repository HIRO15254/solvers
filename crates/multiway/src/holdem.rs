//! Production adapter joining the betting, deal, settlement, utility, and
//! abstraction layers into one generative no-limit Hold'em game.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cards::{combo_cards, rank_of};

use crate::abstraction::{BucketContext, BucketId, BucketPath, MultiwayAbstraction};
use crate::betting::{Action, BettingState, HandPhase, SeatStatus};
use crate::config::{
    CompiledRake, MultiwayConfig, RakeConfig, RecallMode, UtilityConfig, ValidatedMultiwayConfig,
};
use crate::icm::{IcmEstimate, estimate_icm, terminal_icm_delta_with_baseline};
use crate::sampler::{DealSampler, SampleError, SampledWorld};
use crate::settlement::{
    Settlement, SettlementError, settle_ranked, settle_showdown, settle_uncontested,
};
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
        self.bucket_for_combo_and_street(state, world, world.hole_combo(actor), street)
    }

    /// Like [`Self::bucket_for_street`], but for an arbitrary hole combo
    /// instead of the actor's own dealt one. Used by the vector-traverser
    /// path (only valid for [`RecallMode::Street`], i.e. `street ==
    /// state.street`, the only street `ExternalSamplingGame::bucket_for_combo`
    /// ever queries) to bucket every feasible traverser combo against the
    /// same fixed board/active-opponent context.
    fn bucket_for_combo_and_street(
        &self,
        state: &BettingState,
        world: &SampledWorld,
        combo: usize,
        street: Street,
    ) -> BucketId {
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

    /// Vector-traverser terminal evaluation: the traverser's utility for
    /// every combo in `combos`, appended to `out` in the same order.
    ///
    /// The betting line (and therefore the pot structure: refunds, side
    /// pots, rake, eligible seats, and every *other* seat's rank) is fixed
    /// for the whole traversal regardless of which traverser combo is being
    /// evaluated -- only `traverser`'s own two cards vary. Each combo is
    /// therefore settled by substituting it into `traverser`'s rank and
    /// re-running the exact same tested [`settle_ranked`]/[`settle_uncontested`]
    /// machinery [`Self::terminal_utilities`] uses (so any single combo's
    /// result here is *identical*, by construction, to calling
    /// [`Self::terminal_utilities`] with a world whose only difference is
    /// `traverser`'s dealt combo). Pot construction and rake, which do not
    /// depend on any hole cards, are therefore recomputed once per combo
    /// (inside `settle_ranked`) rather than hoisted out of the loop --
    /// deliberately simpler than a bespoke per-pot cache, and cheap relative
    /// to the unavoidable per-combo 7-card rank evaluation.
    fn terminal_utilities_for_combos(
        &self,
        state: &BettingState,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        out.clear();
        if combos.is_empty() {
            return;
        }
        match state.phase {
            HandPhase::Uncontested { .. } => {
                // Card-independent: the traverser's stack delta is the same
                // for every combo.
                let value = self
                    .settle_terminal(state, world)
                    .map_err(HoldemGameError::from)
                    .and_then(|settlement| self.utilities(&settlement))
                    .map(|utilities| utilities[SeatId(traverser as u8)])
                    .unwrap_or(f64::NAN);
                out.resize(combos.len(), value);
            }
            HandPhase::Runout | HandPhase::Showdown => {
                let board = *world.runout();
                let folded = state.seats[SeatId(traverser as u8)].status == SeatStatus::Folded;
                let mut base_ranks: Vec<Option<u16>> = Vec::with_capacity(state.num_seats());
                for seat in state.seats.seats() {
                    if seat.index() == traverser {
                        base_ranks.push(None);
                        continue;
                    }
                    if state.seats[seat].status == SeatStatus::Folded {
                        base_ranks.push(None);
                        continue;
                    }
                    let (first, second) = world.hole_cards(seat.index());
                    base_ranks.push(Some(rank_of(board.into_iter().chain([first, second])).0));
                }
                for &combo in combos {
                    let mut ranks = base_ranks.clone();
                    if !folded {
                        let (first, second) = combo_cards(combo);
                        ranks[traverser] =
                            Some(rank_of(board.into_iter().chain([first, second])).0);
                    }
                    let value = settle_ranked(state, SeatVec::new_unchecked(ranks), self.rake)
                        .map_err(HoldemGameError::from)
                        .and_then(|settlement| self.utilities(&settlement))
                        .map(|utilities| utilities[SeatId(traverser as u8)])
                        .unwrap_or(f64::NAN);
                    out.push(value);
                }
            }
            HandPhase::Betting => out.resize(combos.len(), f64::NAN),
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

    fn bucket_for_combo(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        _actor: usize,
        combo: usize,
    ) -> BucketId {
        self.bucket_for_combo_and_street(state, world, combo, state.street)
    }

    fn terminal_utilities_for_combos(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        HoldemGame::terminal_utilities_for_combos(self, state, world, traverser, combos, out);
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

    /// Manually built 3-seat all-in showdown, mirroring
    /// `settlement::tests::manual` (individual commits 5000/3000/1000 chips
    /// -- i.e. 5/3/1 bb -- with no common pot, all-in): pot layers are a
    /// 3000-chip main pot (all three seats eligible), a 4000-chip side pot
    /// (only seats 0 and 1 eligible), and a 2000-chip refund to seat 0.
    fn manual_three_way_allin() -> BettingState {
        use crate::betting::SeatState;
        use crate::types::SeatMask;
        let individual = [5_000u64, 3_000, 1_000];
        let seats = individual
            .iter()
            .map(|&amount| SeatState {
                starting_stack: MwChips(amount),
                remaining: MwChips::ZERO,
                status: SeatStatus::AllIn,
                dead_committed: MwChips::ZERO,
                common_committed: MwChips::ZERO,
                street_committed: [MwChips(amount), MwChips::ZERO, MwChips::ZERO, MwChips::ZERO],
                raise_reopen_at: None,
            })
            .collect();
        BettingState {
            seats: SeatVec::try_new(seats).unwrap(),
            button: SeatId(0),
            small_blind_seat: SeatId(1),
            big_blind_seat: SeatId(2),
            big_blind: MwChips(1_000),
            street: Street::River,
            street_active_players: [3; 4],
            to_act: None,
            bet_to_match: MwChips::ZERO,
            last_full_raise: MwChips(1_000),
            full_wager_established: false,
            pending: SeatMask::EMPTY,
            aggressive_actions: 0,
            flop_dealt: true,
            phase: HandPhase::Showdown,
            preflop_voluntary_call_seen: false,
        }
    }

    /// Correctness oracle: for a fixed side-pot showdown and fixed
    /// opponents, [`HoldemGame::terminal_utilities_for_combos`] (the
    /// vector-traverser terminal path) must agree, combo by combo, with the
    /// existing scalar [`ExternalSamplingGame::terminal_utilities`] applied
    /// to a world whose only difference is the traverser's own dealt combo.
    /// Covers a combo that ties an opponent for both pots and a combo that
    /// wins/loses the side pot differently from the main pot.
    #[test]
    fn vector_terminal_utilities_match_scalar_settlement_for_several_combos() {
        let mut game_config = config();
        game_config.seats[0].stack_bb = 5.0;
        game_config.seats[1].stack_bb = 3.0;
        game_config.seats[2].stack_bb = 1.0;
        let game = HoldemGame::new(
            &game_config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap();
        let state = manual_three_way_allin();

        let card = |text: &str| text.parse::<cards::Card>().unwrap();
        let board = [card("2c"), card("7d"), card("9h"), card("Jc"), card("4s")];
        let seat1 = combo_index(card("Ah"), card("Kd"));
        let seat2 = combo_index(card("3h"), card("3d"));
        // Ties seat1's ace-high exactly (same board, no flush possible).
        let hero_tie = combo_index(card("Ad"), card("Ks"));
        // Trip jacks: beats both opponents outright on every pot.
        let hero_win = combo_index(card("Jd"), card("Js"));
        // Jack-high: loses the main pot to seat2's pair and the side pot to
        // seat1's ace-high.
        let hero_lose = combo_index(card("5h"), card("6d"));

        let base_world = SampledWorld::new(vec![hero_win, seat1, seat2], board).unwrap();
        let combos = [hero_tie, hero_win, hero_lose];
        let mut vector_utilities = Vec::new();
        game.terminal_utilities_for_combos(&state, &base_world, 0, &combos, &mut vector_utilities);
        assert_eq!(vector_utilities.len(), combos.len());

        for (&combo, &vector_utility) in combos.iter().zip(&vector_utilities) {
            let world = SampledWorld::new(vec![combo, seat1, seat2], board).unwrap();
            let mut scalar_utilities = vec![0.0; 3];
            game.terminal_utilities(&state, &world, &mut scalar_utilities);
            assert!(
                (scalar_utilities[0] - vector_utility).abs() < 1e-9,
                "combo {combo}: vector {vector_utility} != scalar {}",
                scalar_utilities[0]
            );
        }

        // Sanity-check the three constructed outcomes actually exercise
        // win/tie/lose, with hand-worked expected bb deltas:
        // - refund (the uncalled 2000-chip / 2bb top of hero's stack) always
        //   comes back to hero regardless of cards;
        // - `hero_win` (trips) wins the 3000-chip main pot (all three
        //   eligible) and the 4000-chip side pot (only hero/seat1 eligible)
        //   outright: (2000 refund + 3000 + 4000 - 5000 start) / 1000 = 4.0.
        // - `hero_tie` ties seat1's ace-high exactly, but seat2's pair beats
        //   both of them in the main pot, so hero only splits the side pot
        //   with seat1: (2000 + 0 + 2000 - 5000) / 1000 = -1.0.
        // - `hero_lose` loses the main pot to seat2's pair and the side pot
        //   to seat1's ace-high: (2000 + 0 + 0 - 5000) / 1000 = -3.0.
        let [tie_utility, win_utility, lose_utility] = [
            vector_utilities[0],
            vector_utilities[1],
            vector_utilities[2],
        ];
        assert_eq!(win_utility, 4.0);
        assert_eq!(tie_utility, -1.0);
        assert_eq!(lose_utility, -3.0);
    }

    #[test]
    fn vector_terminal_utilities_are_constant_for_uncontested_pots() {
        let game = HoldemGame::new(
            &config(),
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::default(),
        )
        .unwrap();
        let mut state = manual_three_way_allin();
        state.phase = HandPhase::Uncontested { winner: SeatId(1) };
        state.seats[SeatId(0)].status = SeatStatus::Folded;
        state.seats[SeatId(2)].status = SeatStatus::Folded;

        let card = |text: &str| text.parse::<cards::Card>().unwrap();
        let board = [card("2c"), card("7d"), card("9h"), card("Jc"), card("4s")];
        let seat1 = combo_index(card("Ah"), card("Kd"));
        let seat2 = combo_index(card("3h"), card("3d"));
        let combo_a = combo_index(card("Ad"), card("Ks"));
        let combo_b = combo_index(card("Jd"), card("Js"));
        let world = SampledWorld::new(vec![combo_a, seat1, seat2], board).unwrap();

        let mut out = Vec::new();
        game.terminal_utilities_for_combos(&state, &world, 0, &[combo_a, combo_b], &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], out[1]);
        assert!(out[0].is_finite());
    }
}
