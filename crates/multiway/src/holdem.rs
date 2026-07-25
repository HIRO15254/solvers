//! Production adapter joining the betting, deal, settlement, utility, and
//! abstraction layers into one generative no-limit Hold'em game.

use std::sync::{Arc, Mutex};

use cards::{combo_cards, rank_of};
use rustc_hash::FxHashMap;

use crate::abstraction::{BucketContext, BucketId, BucketPath, MultiwayAbstraction};
use crate::betting::{Action, BettingState, HandPhase, SeatStatus};
use crate::config::{
    AbstractionConfig, CompiledRake, MultiwayConfig, RakeConfig, RecallMode, UtilityConfig,
    ValidatedMultiwayConfig,
};
use crate::icm::PreparedIcm;
use crate::sampler::{DealSampler, SampleError, SampledWorld};
use crate::settlement::{
    PotLayer, Settlement, SettlementError, build_rated_pots, settle_ranked, settle_showdown,
    settle_uncontested,
};
use crate::solver::{ExternalSamplingGame, PrivateInfo};
use crate::tree::DenseNodeContext;
use crate::types::{CHIPS_PER_BB, MAX_SEATS, MwChips, SeatId, SeatMask, SeatVec, Street};

const ICM_TERMINAL_CACHE_ENTRIES: usize = 65_536;

#[derive(Clone)]
enum UtilityRuntime {
    ChipEv,
    TournamentIcm {
        calculator: Arc<PreparedIcm>,
        terminal_cache: Arc<Mutex<FxHashMap<[u64; MAX_SEATS], SeatVec<f64>>>>,
    },
}

/// A lazy public game. No betting tree is expanded up front; every child is
/// generated from a cloned [`BettingState`] only when MCCFR visits it.
pub struct HoldemGame<A> {
    config: ValidatedMultiwayConfig,
    root: BettingState,
    abstraction: A,
    /// Optional common abstraction used only by trained-deviation
    /// diagnostics. It deliberately does not contribute to either game or
    /// candidate-abstraction fingerprints.
    deviation_abstraction: Option<(A, RecallMode)>,
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
                let calculator =
                    PreparedIcm::new(starting, outside_field, payouts.clone(), *samples, *seed)?;
                UtilityRuntime::TournamentIcm {
                    calculator: Arc::new(calculator),
                    terminal_cache: Arc::new(Mutex::new(FxHashMap::default())),
                }
            }
        };

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.holdem.v1");
        let mut game_identity = config.clone();
        game_identity.abstraction = AbstractionConfig::default();
        hasher.update(&serde_json::to_vec(&game_identity)?);
        hasher.update(&serde_json::to_vec(utility)?);
        hasher.update(&serde_json::to_vec(rake)?);
        let game_fingerprint = *hasher.finalize().as_bytes();
        Ok(Self {
            config: validated,
            root,
            abstraction,
            deviation_abstraction: None,
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

    /// Consumes the game and returns its primary abstraction backend. This is
    /// useful for moving a separately built reference backend into
    /// [`Self::with_deviation_abstraction`] without requiring the backend to
    /// implement `Clone`.
    pub fn into_abstraction(self) -> A {
        self.abstraction
    }

    /// Installs a common reference abstraction for deviator training and
    /// held-out reference-only evaluation.
    ///
    /// This is diagnostic state: it never changes the main solver's buckets,
    /// dense/sparse storage mode, checkpoint compatibility, game fingerprint,
    /// or candidate-abstraction fingerprint.
    pub fn with_deviation_abstraction(mut self, abstraction: A, recall: RecallMode) -> Self {
        self.deviation_abstraction = Some((abstraction, recall));
        self
    }

    /// Evaluation-only reference abstraction, when configured. Callers may
    /// use the concrete accessor after evaluation to persist backend caches.
    pub fn deviation_abstraction(&self) -> Option<&A> {
        self.deviation_abstraction
            .as_ref()
            .map(|(abstraction, _)| abstraction)
    }

    pub fn configured_deviation_recall_mode(&self) -> Option<RecallMode> {
        self.deviation_abstraction
            .as_ref()
            .map(|(_, recall)| *recall)
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
    /// instead of the actor's own dealt one. Used by the dense
    /// street-recall vector batch hook (`street == state.street`) to bucket
    /// every feasible traverser combo against one fixed
    /// board/active-opponent context. The full-recall sparse-vector fallback
    /// instead substitutes each combo into its sampled world and follows the
    /// ordinary scalar bucket-path lookup.
    fn bucket_for_combo_and_street(
        &self,
        state: &BettingState,
        world: &SampledWorld,
        combo: usize,
        street: Street,
    ) -> BucketId {
        Self::bucket_for_combo_and_street_with(&self.abstraction, state, world, combo, street)
    }

    fn bucket_for_combo_and_street_with(
        abstraction: &A,
        state: &BettingState,
        world: &SampledWorld,
        combo: usize,
        street: Street,
    ) -> BucketId {
        abstraction.bucket(BucketContext {
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

    fn bucket_path_with(
        &self,
        abstraction: &A,
        state: &BettingState,
        world: &SampledWorld,
        actor: usize,
    ) -> BucketPath {
        let bucket = |street: Street| {
            if street.index() <= state.street.index() {
                Self::bucket_for_combo_and_street_with(
                    abstraction,
                    state,
                    world,
                    world.hole_combo(actor),
                    street,
                )
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
                calculator,
                terminal_cache,
            } => {
                let mut key = [0u64; MAX_SEATS];
                for (slot, stack) in key.iter_mut().zip(&settlement.final_stacks) {
                    *slot = stack.raw();
                }
                let cached = {
                    let cache = terminal_cache
                        .lock()
                        .map_err(|_| HoldemGameError::IcmCachePoisoned)?;
                    cache.get(&key).cloned()
                };
                if let Some(cached) = cached {
                    return Ok(cached);
                }
                let deltas = calculator.terminal_delta(&settlement.final_stacks)?.deltas;
                let mut cache = terminal_cache
                    .lock()
                    .map_err(|_| HoldemGameError::IcmCachePoisoned)?;
                if cache.len() < ICM_TERMINAL_CACHE_ENTRIES {
                    cache.entry(key).or_insert_with(|| deltas.clone());
                }
                drop(cache);
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
    /// evaluated -- only `traverser`'s own two cards vary. Rather than
    /// re-running the full [`settle_ranked`] machinery (which reconstructs
    /// pots and clones a full seat-ranks vector) once per combo, the
    /// showdown/runout case delegates to
    /// [`Self::terminal_utilities_for_combos_showdown`], which hoists every
    /// combo-independent computation (pot construction, rake, opponent
    /// ranks, and -- for ChipEv -- the exact tie/odd-chip share the
    /// traverser would receive from each pot) out of the per-combo loop, so
    /// each combo pays only for its own 7-card rank evaluation plus an O(pots)
    /// comparison. See `terminal_utilities_for_combos_reference` (test-only)
    /// for the straightforward one-`settle_ranked`-per-combo version this is
    /// checked against.
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
                self.terminal_utilities_for_combos_showdown(state, world, traverser, combos, out);
            }
            HandPhase::Betting => out.resize(combos.len(), f64::NAN),
        }
    }

    /// Fast-path showdown/runout implementation of
    /// [`Self::terminal_utilities_for_combos`]. See that method's doc comment
    /// for the invariant this exploits (only `traverser`'s cards vary across
    /// `combos`).
    fn terminal_utilities_for_combos_showdown(
        &self,
        state: &BettingState,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        let board = *world.runout();
        let traverser_seat = SeatId(traverser as u8);
        let folded = state.seats[traverser_seat].status == SeatStatus::Folded;

        // Every other live seat's rank is independent of the traverser's
        // combo, so it is computed exactly once here rather than once per
        // combo.
        let mut base_ranks: Vec<Option<u16>> = Vec::with_capacity(state.num_seats());
        for seat in state.seats.seats() {
            if seat.index() == traverser || state.seats[seat].status == SeatStatus::Folded {
                base_ranks.push(None);
                continue;
            }
            let (first, second) = world.hole_cards(seat.index());
            base_ranks.push(Some(rank_of(board.into_iter().chain([first, second])).0));
        }

        if folded {
            // The traverser's rank never enters any pot's winner
            // computation once folded, so every combo yields the exact same
            // settlement: pay for it once instead of once per combo.
            let value = settle_ranked(state, SeatVec::new_unchecked(base_ranks), self.rake)
                .map_err(HoldemGameError::from)
                .and_then(|settlement| self.utilities(&settlement))
                .map(|utilities| utilities[traverser_seat])
                .unwrap_or(f64::NAN);
            out.resize(combos.len(), value);
            return;
        }

        // Pot construction and rake depend only on the betting line, so they
        // too are computed once and reused across every combo.
        let shape = match build_rated_pots(state, self.rake) {
            Ok(shape) => shape,
            Err(_) => {
                out.resize(combos.len(), f64::NAN);
                return;
            }
        };
        let views = hero_pot_views(
            &shape.pots,
            &base_ranks,
            traverser_seat,
            state.num_seats(),
            state.button,
        );

        match &self.utility {
            UtilityRuntime::ChipEv => {
                let hero_floor = state.seats[traverser_seat].remaining.raw()
                    + shape.refunds[traverser_seat].raw();
                let starting = self.config.seats[traverser_seat].starting_stack.raw();
                out.reserve(combos.len());
                for &combo in combos {
                    let (first, second) = combo_cards(combo);
                    let hero_rank = rank_of(board.into_iter().chain([first, second])).0;
                    let mut chips = hero_floor;
                    for view in &views {
                        if !view.hero_eligible {
                            continue;
                        }
                        match view.best_opp_rank {
                            None => chips += view.net.raw(),
                            Some(best) => match hero_rank.cmp(&best) {
                                std::cmp::Ordering::Greater => chips += view.net.raw(),
                                std::cmp::Ordering::Equal => chips += view.tie_share.raw(),
                                std::cmp::Ordering::Less => {}
                            },
                        }
                    }
                    out.push((chips as f64 - starting as f64) / CHIPS_PER_BB as f64);
                }
            }
            UtilityRuntime::TournamentIcm { .. } => {
                // ICM needs every seat's final stack, not just the
                // traverser's share, so a distinct per-pot win/tie/lose
                // outcome tuple still needs one real `settle_ranked` call.
                // But the outcome tuple only varies over the pots where the
                // traverser is eligible *and* has a live opponent to compare
                // against, so the number of distinct tuples is normally far
                // below `combos.len()` (at most 3 raised to the number of
                // such pots) -- cache one `settle_ranked` call per distinct
                // tuple instead of paying for one per combo.
                let variable_bounds: Vec<u16> = views
                    .iter()
                    .filter(|view| view.hero_eligible)
                    .filter_map(|view| view.best_opp_rank)
                    .collect();
                let mut cache: FxHashMap<u64, f64> = FxHashMap::default();
                out.reserve(combos.len());
                for &combo in combos {
                    let (first, second) = combo_cards(combo);
                    let hero_rank = rank_of(board.into_iter().chain([first, second])).0;
                    let mut key = 0u64;
                    for (bit, &best) in variable_bounds.iter().enumerate() {
                        let ordinal = match hero_rank.cmp(&best) {
                            std::cmp::Ordering::Less => 0u64,
                            std::cmp::Ordering::Equal => 1,
                            std::cmp::Ordering::Greater => 2,
                        };
                        key |= ordinal << (2 * bit);
                    }
                    let value = *cache.entry(key).or_insert_with(|| {
                        let mut ranks = base_ranks.clone();
                        ranks[traverser] = Some(hero_rank);
                        settle_ranked(state, SeatVec::new_unchecked(ranks), self.rake)
                            .map_err(HoldemGameError::from)
                            .and_then(|settlement| self.utilities(&settlement))
                            .map(|utilities| utilities[traverser_seat])
                            .unwrap_or(f64::NAN)
                    });
                    out.push(value);
                }
            }
        }
    }

    /// Reference implementation of the showdown/runout branch of
    /// [`Self::terminal_utilities_for_combos`]: one full [`settle_ranked`]
    /// call per combo, with no hoisted precomputation. Kept only to check
    /// [`Self::terminal_utilities_for_combos_showdown`] against in
    /// `holdem::tests::vector_terminal_utilities_fast_path_matches_reference_randomized`;
    /// production code never calls this.
    #[cfg(test)]
    fn terminal_utilities_for_combos_reference(
        &self,
        state: &BettingState,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        out: &mut Vec<f64>,
    ) {
        out.clear();
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
                ranks[traverser] = Some(rank_of(board.into_iter().chain([first, second])).0);
            }
            let value = settle_ranked(state, SeatVec::new_unchecked(ranks), self.rake)
                .map_err(HoldemGameError::from)
                .and_then(|settlement| self.utilities(&settlement))
                .map(|utilities| utilities[SeatId(traverser as u8)])
                .unwrap_or(f64::NAN);
            out.push(value);
        }
    }
}

/// Per-pot view of what the traverser (an arbitrary hero seat, fixed for a
/// whole [`HoldemGame::terminal_utilities_for_combos_showdown`] call) would
/// receive from that pot, factored so every field below is independent of
/// which traverser combo is ultimately compared against it.
struct HeroPotView {
    net: MwChips,
    /// Whether the traverser is eligible for this pot at all (folded or
    /// capped out below it otherwise). If `false`, the traverser's share is
    /// always zero and the remaining fields are unused.
    hero_eligible: bool,
    /// The best rank among this pot's eligible *non-traverser* seats. `None`
    /// means the traverser is the pot's only eligible seat (an automatic,
    /// rank-independent win), which is distinct from "no comparison
    /// happened" thanks to `hero_eligible`.
    best_opp_rank: Option<u16>,
    /// The traverser's exact share of `net` in the event the traverser ties
    /// `best_opp_rank` (floor share plus the traverser's odd chip, if any,
    /// following the same clockwise-from-button assignment
    /// [`crate::settlement`]'s `award_split` uses). The winner set for a tie
    /// is `{eligible non-traverser seats at best_opp_rank} union {traverser}`
    /// -- fixed regardless of which combo achieves the tie -- so this is
    /// exact, not an approximation.
    tie_share: MwChips,
}

fn hero_pot_views(
    pots: &[PotLayer],
    base_ranks: &[Option<u16>],
    traverser_seat: SeatId,
    num_seats: usize,
    button: SeatId,
) -> Vec<HeroPotView> {
    pots.iter()
        .map(|pot| {
            if !pot.eligible.contains(traverser_seat) {
                return HeroPotView {
                    net: pot.net,
                    hero_eligible: false,
                    best_opp_rank: None,
                    tie_share: MwChips::ZERO,
                };
            }
            let opponents = pot.eligible.difference(SeatMask::from_seat(traverser_seat));
            let best_opp_rank = opponents
                .iter()
                .filter_map(|seat| base_ranks[seat.index()])
                .max();
            let tie_share = match best_opp_rank {
                None => MwChips::ZERO,
                Some(best) => {
                    let tying = opponents.iter().fold(SeatMask::EMPTY, |mut mask, seat| {
                        if base_ranks[seat.index()] == Some(best) {
                            mask.insert(seat);
                        }
                        mask
                    });
                    let winners = tying.union(SeatMask::from_seat(traverser_seat));
                    let count = winners.len() as u64;
                    let share = pot.net.raw() / count;
                    let odd = (pot.net.raw() % count) as usize;
                    let hero_gets_extra = (1..=num_seats)
                        .map(|step| button.advance(step, num_seats))
                        .filter(|seat| winners.contains(*seat))
                        .take(odd)
                        .any(|seat| seat == traverser_seat);
                    MwChips(share + u64::from(hero_gets_extra))
                }
            };
            HeroPotView {
                net: pot.net,
                hero_eligible: true,
                best_opp_rank,
                tie_share,
            }
        })
        .collect()
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
        next.apply_from_actions(action, actions, self.betting_for_state(state))
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

    fn deviation_bucket(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        actor: usize,
    ) -> PrivateInfo {
        let Some((abstraction, recall)) = &self.deviation_abstraction else {
            return self.bucket(state, world, actor);
        };
        let opponents = state.non_folded_mask().len().saturating_sub(1) as u8;
        match recall {
            RecallMode::Full => PrivateInfo::from_path(
                state.street,
                opponents,
                self.bucket_path_with(abstraction, state, world, actor),
            ),
            RecallMode::Street => {
                let bucket = Self::bucket_for_combo_and_street_with(
                    abstraction,
                    state,
                    world,
                    world.hole_combo(actor),
                    state.street,
                );
                PrivateInfo::from_current_bucket(state.street, opponents, bucket)
            }
        }
    }

    fn recall_mode(&self) -> RecallMode {
        self.config.abstraction.recall
    }

    fn deviation_recall_mode(&self) -> RecallMode {
        self.configured_deviation_recall_mode()
            .unwrap_or_else(|| self.recall_mode())
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

    fn buckets_for_combos(
        &self,
        state: &Self::State,
        world: &SampledWorld,
        _actor: usize,
        combos: &[usize],
    ) -> Vec<BucketId> {
        let street = state.street;
        self.abstraction.bucket_batch(
            street,
            world.board(street),
            state.players_on_street(street).saturating_sub(1),
            combos,
        )
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
            forced_bets: None,
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
    fn abstraction_settings_do_not_change_game_identity() {
        let mut first = config();
        first.abstraction.artifact_cache = Some("cache/first.mwab".into());
        let mut second = first.clone();
        second.abstraction.artifact_cache = Some("other/second.mwab".into());
        second.abstraction.flop_buckets += 1;
        second.abstraction.rollout_samples += 1;
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

    #[test]
    fn deviation_abstraction_is_accessible_without_changing_fingerprints() {
        let candidate = FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: 32,
            turn_buckets: 64,
            river_buckets: 128,
        })
        .unwrap();
        let plain_game = HoldemGame::new(
            &config(),
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            candidate.clone(),
        )
        .unwrap();
        let game = HoldemGame::new(
            &config(),
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            candidate,
        )
        .unwrap();
        let game_fingerprint = plain_game.game_fingerprint();
        let abstraction_fingerprint = plain_game.abstraction_fingerprint();
        let reference = FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: 257,
            turn_buckets: 509,
            river_buckets: 1_021,
        })
        .unwrap();
        let reference_fingerprint = reference.fingerprint();

        let game = game.with_deviation_abstraction(reference, RecallMode::Street);
        assert_eq!(game.game_fingerprint(), game_fingerprint);
        assert_eq!(game.abstraction_fingerprint(), abstraction_fingerprint);
        assert_eq!(
            game.deviation_abstraction().unwrap().fingerprint(),
            reference_fingerprint
        );
        assert_eq!(
            game.configured_deviation_recall_mode(),
            Some(RecallMode::Street)
        );
        assert_eq!(game.deviation_recall_mode(), RecallMode::Street);

        let plain_sampler = plain_game.deal_sampler().unwrap();
        let reference_sampler = game.deal_sampler().unwrap();
        let plain_solver =
            crate::solver::MultiwaySolver::with_defaults(plain_game, plain_sampler).unwrap();
        let reference_solver =
            crate::solver::MultiwaySolver::with_defaults(game, reference_sampler).unwrap();
        assert_eq!(
            plain_solver.configuration_fingerprint(),
            reference_solver.configuration_fingerprint()
        );
        assert_eq!(
            plain_solver.abstraction_fingerprint(),
            reference_solver.abstraction_fingerprint()
        );
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
            preflop_limpers: 0,
            preflop_flats: 0,
            last_preflop_aggressor: None,
            preflop_participants: SeatMask::EMPTY,
            preflop_open_cold_calls: 0,
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

    /// General N-seat manual showdown/runout state: `individual[i]` chips
    /// committed (== `i`'s starting stack, since every constructed seat has
    /// already put its whole stack in), status `statuses[i]`. Generalizes
    /// `manual_three_way_allin` to arbitrary seat counts, stacks (hence
    /// arbitrary side-pot layering), and fold patterns.
    fn manual_n_way(individual: &[u64], statuses: &[SeatStatus], button: u8) -> BettingState {
        use crate::betting::SeatState;
        use crate::types::SeatMask;
        let num_seats = individual.len();
        let seats = individual
            .iter()
            .zip(statuses)
            .map(|(&amount, &status)| SeatState {
                starting_stack: MwChips(amount),
                remaining: MwChips::ZERO,
                status,
                dead_committed: MwChips::ZERO,
                common_committed: MwChips::ZERO,
                street_committed: [MwChips(amount), MwChips::ZERO, MwChips::ZERO, MwChips::ZERO],
                raise_reopen_at: None,
            })
            .collect();
        let non_folded = statuses
            .iter()
            .filter(|&&status| status != SeatStatus::Folded)
            .count() as u8;
        BettingState {
            seats: SeatVec::try_new(seats).unwrap(),
            button: SeatId(button),
            small_blind_seat: SeatId((button + 1) % num_seats as u8),
            big_blind_seat: SeatId((button + 2) % num_seats as u8),
            big_blind: MwChips(1_000),
            street: Street::River,
            street_active_players: [non_folded; 4],
            to_act: None,
            bet_to_match: MwChips::ZERO,
            last_full_raise: MwChips(1_000),
            full_wager_established: false,
            pending: SeatMask::EMPTY,
            aggressive_actions: 0,
            preflop_limpers: 0,
            preflop_flats: 0,
            last_preflop_aggressor: None,
            preflop_participants: SeatMask::EMPTY,
            preflop_open_cold_calls: 0,
            flop_dealt: true,
            phase: HandPhase::Showdown,
            preflop_voluntary_call_seen: false,
        }
    }

    fn config_with_stacks(stacks_bb: &[f64]) -> MultiwayConfig {
        MultiwayConfig {
            seats: stacks_bb
                .iter()
                .map(|&stack_bb| SeatConfig {
                    name: None,
                    stack_bb,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            forced_bets: None,
            abstraction: AbstractionConfig::default(),
        }
    }

    fn assert_fast_matches_reference(
        game: &HoldemGame<FeatureHashAbstraction>,
        state: &BettingState,
        world: &SampledWorld,
        traverser: usize,
        combos: &[usize],
        trial: u32,
    ) {
        let mut fast = Vec::new();
        game.terminal_utilities_for_combos(state, world, traverser, combos, &mut fast);
        let mut reference = Vec::new();
        game.terminal_utilities_for_combos_reference(
            state,
            world,
            traverser,
            combos,
            &mut reference,
        );
        assert_eq!(fast.len(), combos.len());
        assert_eq!(reference.len(), combos.len());
        for (index, (&fast_value, &reference_value)) in fast.iter().zip(&reference).enumerate() {
            let matches = (fast_value.is_nan() && reference_value.is_nan())
                || (fast_value - reference_value).abs() < 1e-9;
            assert!(
                matches,
                "trial {trial} combo index {index}: fast {fast_value} != reference {reference_value}"
            );
        }
    }

    /// Correctness gate for `terminal_utilities_for_combos_showdown` (the
    /// vector-traverser fast path): across many random seat counts, stacks
    /// (hence side-pot layerings), fold patterns, button positions, and
    /// dealt cards -- and both `ChipEv` and tournament `Icm` utility modes,
    /// with and without rake -- the fast path must agree with
    /// `terminal_utilities_for_combos_reference` (one `settle_ranked` call
    /// per combo, no hoisted precomputation) for *every* feasible traverser
    /// combo, including combos that tie an opponent (exercising the odd-chip
    /// tie-share precomputation) and cases where the traverser itself has
    /// folded.
    #[test]
    fn vector_terminal_utilities_fast_path_matches_reference_randomized() {
        use crate::config::FieldPlayerConfig;
        use cards::ALL_CARDS;
        use rand::seq::SliceRandom;
        use rand::{Rng, SeedableRng};
        use rand_chacha::ChaCha20Rng;

        let mut rng = ChaCha20Rng::seed_from_u64(0xF00D_CAFE_u64);
        for trial in 0..80u32 {
            let num_seats = rng.gen_range(2..=6usize);
            let stacks: Vec<u64> = (0..num_seats)
                .map(|_| rng.gen_range(1u64..=20) * 1_000)
                .collect();
            let mut statuses: Vec<SeatStatus> = (0..num_seats)
                .map(|_| {
                    if rng.gen_bool(0.25) {
                        SeatStatus::Folded
                    } else {
                        SeatStatus::AllIn
                    }
                })
                .collect();
            while statuses
                .iter()
                .filter(|&&status| status != SeatStatus::Folded)
                .count()
                < 2
            {
                let index = rng.gen_range(0..num_seats);
                statuses[index] = SeatStatus::AllIn;
            }
            let button = rng.gen_range(0..num_seats) as u8;
            let state = manual_n_way(&stacks, &statuses, button);

            let mut deck: Vec<cards::Card> = ALL_CARDS.into_iter().collect();
            deck.shuffle(&mut rng);
            let board: [cards::Card; 5] = deck[0..5].try_into().unwrap();
            let mut offset = 5;
            let mut hole_combos = Vec::with_capacity(num_seats);
            for _ in 0..num_seats {
                hole_combos.push(combo_index(deck[offset], deck[offset + 1]));
                offset += 2;
            }
            let world = SampledWorld::new(hole_combos, board).unwrap();

            let mut leftover = deck[offset..].to_vec();
            leftover.shuffle(&mut rng);
            leftover.truncate(leftover.len().min(16));
            let mut combos = Vec::new();
            for i in 0..leftover.len() {
                for j in (i + 1)..leftover.len() {
                    combos.push(combo_index(leftover[i], leftover[j]));
                }
            }
            if combos.is_empty() {
                continue;
            }

            let stacks_bb: Vec<f64> = stacks.iter().map(|&chips| chips as f64 / 1_000.0).collect();
            let traverser = rng.gen_range(0..num_seats);

            let rake = if rng.gen_bool(0.5) {
                RakeConfig::PercentCap {
                    rate: 0.05,
                    cap_bb: 3.0,
                    no_flop_no_drop: false,
                }
            } else {
                RakeConfig::None
            };
            let chip_ev_game = HoldemGame::new(
                &config_with_stacks(&stacks_bb),
                &UtilityConfig::ChipEv,
                &rake,
                FeatureHashAbstraction::default(),
            )
            .unwrap();
            assert_fast_matches_reference(&chip_ev_game, &state, &world, traverser, &combos, trial);

            let mut payouts = vec![0.0; num_seats];
            for (place, payout) in payouts.iter_mut().enumerate() {
                *payout = ((num_seats - place) * 10) as f64;
            }
            let icm_game = HoldemGame::new(
                &config_with_stacks(&stacks_bb),
                &UtilityConfig::TournamentIcm {
                    outside_field: Vec::<FieldPlayerConfig>::new(),
                    payouts,
                    samples: 8,
                    seed: 3,
                },
                &RakeConfig::None,
                FeatureHashAbstraction::default(),
            )
            .unwrap();
            assert_fast_matches_reference(&icm_game, &state, &world, traverser, &combos, trial);
        }
    }
}
