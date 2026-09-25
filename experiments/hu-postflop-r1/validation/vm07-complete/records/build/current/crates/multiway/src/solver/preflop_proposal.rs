//! Production-Holdem preflop posterior proposals for read-only diagnostics.
use super::conditioned::profile_action_probability;
use super::eval::{purify_strategy, validate_purify_threshold};
use super::support::{validate_action_labels, validate_private_info};
use super::*;
use crate::abstraction::{BucketContext, MultiwayAbstraction};
use crate::{BettingState, HoldemGame};
use cards::{NUM_CLASSES, NUM_COMBOS, Range, class_index, combo_cards};

// A positive floor keeps actual cumulative draw intervals representable. It
// changes only the proposal: every rounding/floor adjustment is corrected.
const PROPOSAL_FLOOR: f64 = 1e-7;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreflopProposalMetadata {
    pub preflop_actions: Vec<usize>,
    pub preflop_history: HistoryKey,
    pub proposal_range_fingerprint: [u8; 32],
    pub root_range_fingerprint: [u8; 32],
    pub positive_target_combos_by_seat: Vec<usize>,
    pub floor_adjusted_combos_by_seat: Vec<usize>,
    pub target_scale_by_seat: Vec<f64>,
    pub proposal_floor_fraction: f64,
    pub pilot_samples: u32,
    pub pilot_accepted: u32,
}

/// Conditional estimates under a corrected preflop proposal. Absolute root
/// reach must be estimated separately; no field below claims to estimate it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreflopConditionalProfileEvaluation {
    pub samples: u64,
    pub seed: u64,
    /// All prefixes in this call share one physical world per sample id.
    pub total_deal_attempts: u64,
    pub proposal: PreflopProposalMetadata,
    pub prefixes: Vec<PreflopConditionalPrefixEvaluation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreflopConditionalPrefixEvaluation {
    pub history: HistoryKey,
    pub action_indices: Vec<usize>,
    pub positive_weight_samples: u64,
    /// Mean unnormalized importance weight under the proposal; NOT root reach.
    pub relative_weight_mean: WeightedEstimate,
    /// Weight concentration diagnostic, not a universal variance certificate.
    pub effective_sample_size: f64,
    pub max_normalized_weight: f64,
    /// Reach-weight fractions of worlds using each source anywhere in the
    /// forced path. These flags may overlap; suffix source fractions do not.
    pub prefix_current_fraction: Option<WeightedEstimate>,
    pub prefix_regret_fallback_fraction: Option<WeightedEstimate>,
    pub prefix_uniform_fallback_fraction: Option<WeightedEstimate>,
    /// Whole-hand utility conditional on reaching the prefix, per seat.
    pub seats: Vec<Option<WeightedEstimate>>,
    /// Decisions at/below the prefix, in preflop/flop/turn/river order.
    pub coverage_by_street: [ConditionalStreetCoverage; 4],
    pub coverage_by_seat: Vec<[ConditionalStreetCoverage; 4]>,
}

impl From<ConditionalPrefixEvaluation> for PreflopConditionalPrefixEvaluation {
    fn from(value: ConditionalPrefixEvaluation) -> Self {
        Self {
            history: value.history,
            action_indices: value.action_indices,
            positive_weight_samples: value.positive_weight_samples,
            relative_weight_mean: value.reach_probability,
            effective_sample_size: value.effective_sample_size,
            max_normalized_weight: value.max_normalized_weight,
            prefix_current_fraction: value.prefix_current_fraction,
            prefix_regret_fallback_fraction: value.prefix_regret_fallback_fraction,
            prefix_uniform_fallback_fraction: value.prefix_uniform_fallback_fraction,
            seats: value.seats,
            coverage_by_street: value.coverage_by_street,
            coverage_by_seat: value.coverage_by_seat,
        }
    }
}

pub(super) struct PreparedPreflopProposal {
    pub sampler: DealSampler,
    pub actions: Vec<usize>,
    corrections: Vec<Vec<f64>>,
    pub(super) metadata: PreflopProposalMetadata,
}

/// The proposal excludes one endpoint actor's prior action probabilities.
/// Multiplying its corrected weight by the saved own probability recovers a
/// proportional actual-prefix target under this same proposal, not root reach.
pub(super) struct PreparedCounterfactualPreflopProposal {
    pub proposal: PreparedPreflopProposal,
    pub own_prefix_probability_by_bucket: Vec<f64>,
    /// Public decision contexts checked, including the endpoint itself.
    pub validated_class_contexts: usize,
}

impl PreparedPreflopProposal {
    fn from_factors(
        root: &DealSampler,
        actions: Vec<usize>,
        history: HistoryKey,
        factors: Vec<Vec<f64>>,
    ) -> Result<Self, SolverError> {
        if factors.len() != root.num_players() || factors.iter().any(|v| v.len() != NUM_COMBOS) {
            return Err(SolverError::InvalidState(
                "proposal factors have invalid dimensions",
            ));
        }
        let mut ranges = Vec::with_capacity(root.num_players());
        let mut scaled_targets = Vec::with_capacity(root.num_players());
        let mut scales = Vec::new();
        let mut positive_counts = Vec::new();
        let mut floor_counts = Vec::new();
        for (seat, factors) in factors.into_iter().enumerate() {
            let mut target = Vec::with_capacity(NUM_COMBOS);
            for (combo, factor) in factors.into_iter().enumerate() {
                if !factor.is_finite() || !(0.0..=1.0).contains(&factor) {
                    return Err(SolverError::NumericOverflow);
                }
                let prior = root.evaluation_combo_weight(seat, combo);
                let value = prior * factor;
                if prior > 0.0 && factor > 0.0 && value == 0.0 {
                    return Err(SolverError::NumericOverflow);
                }
                target.push(value);
            }
            let scale = target.iter().copied().fold(0.0, f64::max);
            if scale == 0.0 {
                return Err(SampleError::EmptyRange { seat }.into());
            }
            let mut range = Range::default();
            let mut positive = 0;
            let mut floored = 0;
            for (combo, value) in target.iter_mut().enumerate() {
                if *value == 0.0 {
                    continue;
                }
                *value /= scale;
                if *value == 0.0 || !value.is_finite() {
                    return Err(SolverError::NumericOverflow);
                }
                positive += 1;
                floored += usize::from(*value < PROPOSAL_FLOOR);
                range.set_weight(combo, value.max(PROPOSAL_FLOOR) as f32);
            }
            ranges.push(range);
            scaled_targets.push(target);
            scales.push(scale);
            positive_counts.push(positive);
            floor_counts.push(floored);
        }
        let sampler = DealSampler::with_max_attempts(ranges, root.max_attempts())?;
        let mut corrections = scaled_targets;
        for (seat, row) in corrections.iter_mut().enumerate() {
            for (combo, value) in row.iter_mut().enumerate() {
                let actual = sampler.evaluation_combo_weight(seat, combo);
                if *value > 0.0 && actual == 0.0 {
                    return Err(SolverError::InvalidState(
                        "proposal lost positive target support",
                    ));
                }
                *value = if actual > 0.0 { *value / actual } else { 0.0 };
                if !value.is_finite() {
                    return Err(SolverError::NumericOverflow);
                }
            }
        }
        let diagnostics = sampler.diagnostics();
        let metadata = PreflopProposalMetadata {
            preflop_actions: actions.clone(),
            preflop_history: history,
            proposal_range_fingerprint: sampler.range_fingerprint(),
            root_range_fingerprint: root.range_fingerprint(),
            positive_target_combos_by_seat: positive_counts,
            floor_adjusted_combos_by_seat: floor_counts,
            target_scale_by_seat: scales,
            proposal_floor_fraction: PROPOSAL_FLOOR,
            pilot_samples: diagnostics.pilot_samples,
            pilot_accepted: diagnostics.pilot_accepted,
        };
        Ok(Self {
            sampler,
            actions,
            corrections,
            metadata,
        })
    }

    pub(super) fn correction(&self, world: &SampledWorld) -> Result<f64, SolverError> {
        let mut result = 1.0;
        for (seat, &combo) in world.hole_combos().iter().enumerate() {
            let factor = self.corrections[seat][combo];
            result *= factor;
            if !result.is_finite() || result <= 0.0 || result * result == 0.0 {
                return Err(SolverError::NumericOverflow);
            }
        }
        Ok(result)
    }
}

impl<A: MultiwayAbstraction> MultiwaySolver<HoldemGame<A>> {
    /// Replay postflop prefixes sharing one complete preflop trunk under a
    /// proposal proportional to each seat's original range times its forced
    /// preflop action probabilities. Whole-tuple rejection preserves blockers,
    /// including folded seats. Correct actual f32/CDF proposal rounding and
    /// floor mass, then multiply only postflop action reach. Prefix source
    /// provenance still includes every forced action.
    ///
    /// The preflop factorization follows Holdem's BucketContext contract:
    /// preflop board is empty and the only private input is the acting combo.
    /// It is not assumed for arbitrary ExternalSamplingGame implementations.
    /// All paths must reach a postflop decision; mixed trunks are rejected.
    /// Absolute root reach is deliberately absent from the result.
    pub fn evaluate_profile_conditioned_preflop(
        &self,
        samples: u64,
        seed: u64,
        variant: ProfileVariant,
        threads: usize,
        prefixes: &[Vec<usize>],
    ) -> Result<PreflopConditionalProfileEvaluation, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        if samples < 2 || threads == 0 || prefixes.is_empty() || prefixes.len() > 64 {
            return Err(SolverError::InvalidState(
                "preflop proposal requires >=2 samples, positive threads and 1..=64 prefixes",
            ));
        }
        let trunk = self.preflop_trunk(&prefixes[0])?;
        for (index, path) in prefixes.iter().enumerate() {
            if prefixes[..index].contains(path) {
                return Err(SolverError::InvalidState(
                    "conditional prefixes must be unique",
                ));
            }
            if self.preflop_trunk(path)? != trunk {
                return Err(SolverError::InvalidState(
                    "preflop proposal prefixes must share the same preflop trunk",
                ));
            }
        }
        let proposal = self.prepare_preflop_proposal(trunk, variant)?;
        let result = self.evaluate_conditioned_core(
            samples,
            seed,
            variant,
            threads,
            prefixes,
            Some(&proposal),
        )?;
        Ok(PreflopConditionalProfileEvaluation {
            samples,
            seed,
            total_deal_attempts: result.total_deal_attempts,
            proposal: proposal.metadata,
            prefixes: result.prefixes.into_iter().map(Into::into).collect(),
        })
    }

    pub(super) fn preflop_trunk(&self, path: &[usize]) -> Result<Vec<usize>, SolverError> {
        let (actions, state) = self.validated_preflop_prefix(path)?;
        if state.street == Street::Preflop || self.game.actor(&state).is_none() {
            return Err(SolverError::InvalidState(
                "preflop proposal requires a postflop decision endpoint",
            ));
        }
        Ok(actions)
    }

    /// Endpoint deviations may stop before preflop has finished. The proposal
    /// then includes precisely the actions before that decision, never the
    /// deviated action itself. Empty actions describe the root range law.
    pub(super) fn endpoint_preflop_trunk(&self, path: &[usize]) -> Result<Vec<usize>, SolverError> {
        let (actions, state) = self.validated_preflop_prefix(path)?;
        if self.game.actor(&state).is_none() {
            return Err(SolverError::InvalidState(
                "endpoint proposal requires a decision endpoint",
            ));
        }
        Ok(actions)
    }

    fn validated_preflop_prefix(
        &self,
        path: &[usize],
    ) -> Result<(Vec<usize>, BettingState), SolverError> {
        if path.len() >= self.config.max_traversal_depth as usize {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }
        let mut state = self.game.root_state();
        let mut length = 0;
        for &action in path {
            if self.game.actor(&state).is_none() {
                return Err(SolverError::InvalidState(
                    "proposal prefix passes a terminal state",
                ));
            }
            let menu = self.game.node_actions(&state);
            if action >= self.game.num_actions_of(&menu) {
                return Err(SolverError::InvalidState(
                    "proposal prefix action is out of bounds",
                ));
            }
            if state.street == Street::Preflop {
                length += 1;
            }
            state = self.game.next_state_with(&state, &menu, action);
        }
        Ok((path[..length].to_vec(), state))
    }

    pub(super) fn prepare_preflop_proposal(
        &self,
        actions: Vec<usize>,
        variant: ProfileVariant,
    ) -> Result<PreparedPreflopProposal, SolverError> {
        let (factors, history) = self.preflop_proposal_factors(&actions, variant)?;
        PreparedPreflopProposal::from_factors(&self.sampler, actions, history, factors)
    }

    /// Prepare an opponents-only prefix target without changing the ordinary
    /// actual-prefix proposal. All physical seats, including folded players,
    /// remain in the collision-conditioned deal sampler.
    pub(super) fn prepare_counterfactual_preflop_proposal(
        &self,
        actions: Vec<usize>,
        variant: ProfileVariant,
        excluded_actor: usize,
    ) -> Result<PreparedCounterfactualPreflopProposal, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        if excluded_actor >= self.game.num_players() {
            return Err(SolverError::InvalidActor {
                actor: excluded_actor,
                num_players: self.game.num_players(),
            });
        }
        if self.game.recall_mode() != RecallMode::Street {
            return Err(SolverError::InvalidState(
                "counterfactual preflop proposal requires current-street recall",
            ));
        }
        if actions.len() >= self.config.max_traversal_depth as usize {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }
        let mut state = self.game.root_state();
        for depth in 0..=actions.len() {
            if state.street != Street::Preflop {
                return Err(SolverError::InvalidState(
                    "counterfactual proposal requires a preflop decision endpoint",
                ));
            }
            let actor = self.game.actor(&state).ok_or(SolverError::InvalidState(
                "counterfactual proposal passes a terminal state",
            ))?;
            if actor >= self.game.num_players() {
                return Err(SolverError::InvalidActor {
                    actor,
                    num_players: self.game.num_players(),
                });
            }
            // Bucket lookup uses the state's per-street count, whereas the
            // policy InfoKey uses its current non-folded count. Validate the
            // same bucket context as factor preparation and Holdem replay.
            let context = self.game.dense_node_context(&state);
            if self
                .game
                .bucket_count(Street::Preflop, context.bucket_active_opponents)
                != NUM_CLASSES as u32
            {
                return Err(SolverError::InvalidState(
                    "counterfactual proposal requires exactly 169 preflop classes",
                ));
            }
            for combo in 0..NUM_COMBOS {
                let (hi, lo) = combo_cards(combo);
                let expected = class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit());
                let bucket = self.game.abstraction().bucket(BucketContext {
                    street: Street::Preflop,
                    board: &[],
                    combo,
                    active_opponents: context.bucket_active_opponents,
                });
                if bucket != expected as u32 {
                    return Err(SolverError::InvalidState(
                        "counterfactual proposal requires exact preflop class mapping",
                    ));
                }
            }
            let menu = self.game.node_actions(&state);
            let labels = (0..self.game.num_actions_of(&menu))
                .map(|i| self.game.action_label_of(&menu, i))
                .collect::<Vec<_>>();
            validate_action_labels(&labels)?;
            if let Some(&action) = actions.get(depth) {
                if action >= labels.len() {
                    return Err(SolverError::InvalidState(
                        "counterfactual proposal prefix action is out of bounds",
                    ));
                }
                state = self.game.next_state_with(&state, &menu, action);
            } else if actor != excluded_actor {
                return Err(SolverError::InvalidState(
                    "counterfactual proposal excluded actor differs from endpoint actor",
                ));
            }
        }
        let (mut factors, history) = self.preflop_proposal_factors(&actions, variant)?;
        let mut own_by_bucket: Vec<Option<f64>> = vec![None; NUM_CLASSES];
        for (combo, &probability) in factors[excluded_actor].iter().enumerate() {
            let (hi, lo) = combo_cards(combo);
            let bucket = class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit());
            if let Some(previous) = own_by_bucket[bucket] {
                if previous.to_bits() != probability.to_bits() {
                    return Err(SolverError::InvalidState(
                        "counterfactual own prefix probability varies within a class",
                    ));
                }
            } else {
                own_by_bucket[bucket] = Some(probability);
            }
        }
        let own_prefix_probability_by_bucket = own_by_bucket
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(SolverError::InvalidState(
                "counterfactual proposal has an unrepresented preflop class",
            ))?;
        factors[excluded_actor].fill(1.0);
        let validated_class_contexts = actions.len() + 1;
        let proposal =
            PreparedPreflopProposal::from_factors(&self.sampler, actions, history, factors)?;
        Ok(PreparedCounterfactualPreflopProposal {
            proposal,
            own_prefix_probability_by_bucket,
            validated_class_contexts,
        })
    }

    // Shared solely to preserve the existing per-combo probability arithmetic
    // and action order in both preparations. The legacy caller remains free
    // of the counterfactual path's stricter class/endpoint validation.
    fn preflop_proposal_factors(
        &self,
        actions: &[usize],
        variant: ProfileVariant,
    ) -> Result<(Vec<Vec<f64>>, HistoryKey), SolverError> {
        let mut factors = vec![vec![1.0; NUM_COMBOS]; self.game.num_players()];
        let mut state: BettingState = self.game.root_state();
        let mut history = HistoryKey::ROOT;
        for &action in actions {
            let actor = self.game.actor(&state).ok_or(SolverError::InvalidState(
                "preflop proposal passed terminal state",
            ))?;
            let menu = self.game.node_actions(&state);
            let labels = (0..self.game.num_actions_of(&menu))
                .map(|i| self.game.action_label_of(&menu, i))
                .collect::<Vec<_>>();
            validate_action_labels(&labels)?;
            for (combo, factor) in factors[actor].iter_mut().enumerate() {
                let bucket = self.game.abstraction().bucket(BucketContext {
                    street: Street::Preflop,
                    board: &[],
                    combo,
                    active_opponents: state.players_on_street(Street::Preflop).saturating_sub(1),
                });
                let private = PrivateInfo::from_current_bucket(
                    Street::Preflop,
                    state.non_folded_mask().len().saturating_sub(1) as u8,
                    bucket,
                );
                validate_private_info(private, self.game.num_players(), self.game.recall_mode())?;
                let key = InfoKey {
                    history,
                    player: actor as u8,
                    street: 0,
                    active_opponents: private.active_opponents,
                    bucket_path: private.bucket_path,
                };
                let strategy = if let Some(policy) = self.evaluation_policy(key) {
                    if policy.action_labels != labels {
                        return Err(SolverError::ActionLabelsChanged { key });
                    }
                    let mut strategy = policy.strategy(variant.use_current_strategy)?.0;
                    if variant.purify_threshold > 0.0 {
                        purify_strategy(&mut strategy, variant.purify_threshold);
                    }
                    strategy
                } else {
                    vec![1.0 / labels.len() as f32; labels.len()]
                };
                // No additional normalization: match sample_profile_action.
                let probability = profile_action_probability(&strategy, action);
                let next = *factor * probability;
                if *factor > 0.0 && probability > 0.0 && next == 0.0 {
                    return Err(SolverError::NumericOverflow);
                }
                *factor = next;
            }
            state = self.game.next_state_with(&state, &menu, action);
            history = history.child(actor, action);
        }
        Ok((factors, history))
    }
}

#[cfg(test)]
#[path = "counterfactual_proposal_tests.rs"]
mod counterfactual_proposal_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use cards::{CardSet, combo_cards, combo_index};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn combo(text: &str) -> usize {
        combo_index(text[..2].parse().unwrap(), text[2..].parse().unwrap())
    }

    fn small_proposal() -> (
        DealSampler,
        PreparedPreflopProposal,
        Vec<Vec<f64>>,
        Vec<Vec<usize>>,
    ) {
        let support = [
            vec!["AsAh", "KsKh", "2c3c"],
            vec!["AsQs", "KdQd", "4c5c"],
            vec!["AhJh", "KsJs", "6c7c"],
        ]
        .into_iter()
        .map(|row| row.into_iter().map(combo).collect::<Vec<_>>())
        .collect::<Vec<_>>();
        let mut ranges = vec![Range::default(); 3];
        let mut factors = vec![vec![0.0; NUM_COMBOS]; 3];
        for seat in 0..3 {
            for (index, &hand) in support[seat].iter().enumerate() {
                ranges[seat].set_weight(hand, [0.9, 0.7, 0.3][index]);
                factors[seat][hand] = if seat == 0 && index == 2 {
                    1e-12
                } else {
                    [0.3, 0.8, 0.6][index]
                };
            }
        }
        let root = DealSampler::new(ranges).unwrap();
        let proposal =
            PreparedPreflopProposal::from_factors(&root, vec![], HistoryKey::ROOT, factors.clone())
                .unwrap();
        (root, proposal, factors, support)
    }

    #[test]
    fn preflop_proposal_exact_joint_oracle_includes_collisions_and_rounding() {
        let (root, proposal, factors, support) = small_proposal();
        assert_eq!(proposal.metadata.floor_adjusted_combos_by_seat, [1, 0, 0]);
        let mut direct = [0.0; 3];
        let mut corrected = [0.0; 3];
        let mut tuples = Vec::new();
        for &a in &support[0] {
            for &b in &support[1] {
                for &c in &support[2] {
                    let holes = [a, b, c];
                    let mut dead = CardSet::EMPTY;
                    let mut legal = true;
                    for hand in holes {
                        let (a, b) = combo_cards(hand);
                        if dead.contains(a) || dead.contains(b) {
                            legal = false;
                            break;
                        }
                        dead.insert(a);
                        dead.insert(b);
                    }
                    if !legal {
                        continue;
                    }
                    let target = holes
                        .iter()
                        .enumerate()
                        .map(|(i, &h)| root.evaluation_combo_weight(i, h) * factors[i][h])
                        .product::<f64>();
                    let q = holes
                        .iter()
                        .enumerate()
                        .map(|(i, &h)| proposal.sampler.evaluation_combo_weight(i, h))
                        .product::<f64>();
                    let correction = holes
                        .iter()
                        .enumerate()
                        .map(|(i, &h)| proposal.corrections[i][h])
                        .product::<f64>();
                    // Seat 2 can be folded, but its hole cards remain dead. The exact
                    // uniform-runout first-card expectation excludes all six cards.
                    let live = cards::ALL_CARDS
                        .into_iter()
                        .filter(|card| !dead.contains(*card))
                        .collect::<Vec<_>>();
                    let utility =
                        live.iter().map(|c| c.index() as f64).sum::<f64>() / live.len() as f64;
                    let source = f64::from(a == support[0][0]);
                    for (sum, w) in [(&mut direct, target), (&mut corrected, q * correction)] {
                        sum[0] += w;
                        sum[1] += w * utility;
                        sum[2] += w * source;
                    }
                    tuples.push((holes, q));
                }
            }
        }
        assert!(tuples.len() < 27 && tuples.len() > 2);
        for i in [1, 2] {
            assert!((direct[i] / direct[0] - corrected[i] / corrected[0]).abs() < 1e-12);
        }
        let q_total = tuples.iter().map(|(_, q)| q).sum::<f64>();
        let n = 50_000;
        let mut counts = vec![0_u64; tuples.len()];
        let mut rng = ChaCha20Rng::seed_from_u64(4456);
        let mut weighted = [0.0; 3];
        for _ in 0..n {
            let world = proposal.sampler.sample(&mut rng).unwrap();
            let index = tuples
                .iter()
                .position(|(h, _)| h.as_slice() == world.hole_combos())
                .unwrap();
            counts[index] += 1;
            let w = proposal.correction(&world).unwrap();
            weighted[0] += w;
            weighted[1] += w * world.runout()[0].index() as f64;
            weighted[2] += w * f64::from(world.hole_combo(0) == support[0][0]);
            for seat in 0..3 {
                let (a, b) = world.hole_cards(seat);
                assert!(!world.runout().contains(&a) && !world.runout().contains(&b));
            }
        }
        for ((_, q), count) in tuples.iter().zip(counts) {
            let p = q / q_total;
            let expected = n as f64 * p;
            assert!(
                (count as f64 - expected).abs() < 6.0 * (n as f64 * p * (1.0 - p)).sqrt() + 2.0
            );
        }
        assert!((weighted[1] / weighted[0] - direct[1] / direct[0]).abs() < 0.5);
        assert!((weighted[2] / weighted[0] - direct[2] / direct[0]).abs() < 0.02);
    }

    #[test]
    fn preflop_proposal_rejects_impossible_zero_and_invalid_targets() {
        let (root, _, factors, _) = small_proposal();
        for replacement in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut invalid = factors.clone();
            invalid[0].fill(replacement);
            assert!(
                PreparedPreflopProposal::from_factors(&root, vec![], HistoryKey::ROOT, invalid)
                    .is_err()
            );
        }
        let mut factors = vec![vec![0.0; NUM_COMBOS]; 3];
        factors[0][combo("AsAh")] = 1.0;
        factors[1][combo("AsQs")] = 1.0;
        factors[2][combo("6c7c")] = 1.0;
        assert!(matches!(
            PreparedPreflopProposal::from_factors(&root, vec![], HistoryKey::ROOT, factors),
            Err(SolverError::Sample(SampleError::IncompatibleRanges))
        ));
        // Root uniform fast-path mass must not recover tiny cumulative-roundoff
        // differences from uniformly scaled f32 ranges.
        let mut range = Range::default();
        for hand in 0..NUM_COMBOS {
            range.set_weight(hand, 0.3);
        }
        let uniform = DealSampler::new(vec![range; 2]).unwrap();
        assert!((0..NUM_COMBOS).all(|h| uniform.evaluation_combo_weight(0, h) == 1.0));
    }

    fn call_through(
        game: &HoldemGame<crate::abstraction::FeatureHashAbstraction>,
        fold_first: bool,
    ) -> Vec<usize> {
        let mut state = game.root_state();
        let mut path = Vec::new();
        while state.street == Street::Preflop {
            assert!(path.len() < 20);
            let menu = game.node_actions(&state);
            let labels = (0..game.num_actions_of(&menu))
                .map(|i| game.action_label_of(&menu, i))
                .collect::<Vec<_>>();
            let action = if fold_first && path.is_empty() {
                labels.iter().position(|s| s == "fold")
            } else {
                labels
                    .iter()
                    .position(|s| s.starts_with("call:"))
                    .or_else(|| labels.iter().position(|s| s == "check"))
            }
            .expect("call/check path");
            state = game.next_state_with(&state, &menu, action);
            path.push(action);
        }
        assert!(game.actor(&state).is_some());
        path
    }

    #[test]
    fn preflop_proposal_real_holdem_factorization_and_replay_match() {
        use super::super::conditioned::ForcedPrefixReplay;
        use super::super::support::{evaluation_action_rng, evaluation_deal_rng};
        for dense in [false, true] {
            let (game, sampler, config) = super::super::tests::initialization_holdem_fixture();
            let path = call_through(&game, false);
            let mut solver = if dense {
                MultiwaySolver::new_preallocated_with_threads(game, sampler, config, 2).unwrap()
            } else {
                MultiwaySolver::new(game, sampler, config).unwrap()
            };
            solver.run_sweeps(64).unwrap();
            let before = solver.snapshot_state();
            for variant in [
                ProfileVariant::default(),
                ProfileVariant {
                    use_current_strategy: true,
                    purify_threshold: 0.0,
                },
                ProfileVariant {
                    use_current_strategy: false,
                    purify_threshold: 0.35,
                },
            ] {
                let proposal = solver
                    .prepare_preflop_proposal(path.clone(), variant)
                    .unwrap();
                for id in 0..128 {
                    let world = proposal
                        .sampler
                        .sample(&mut evaluation_deal_rng(782, id))
                        .unwrap();
                    let mut forced = ForcedPrefixReplay {
                        actions: &path,
                        endpoint_action: None,
                        weight: 1.0,
                        skip_weight_actions: 0,
                        sources: [false; 3],
                    };
                    solver
                        .evaluate_world(
                            &world,
                            &mut evaluation_action_rng(782, id, None),
                            None,
                            None,
                            variant.purify_threshold,
                            variant.use_current_strategy,
                            None,
                            &mut [],
                            Some(&mut forced),
                        )
                        .unwrap();
                    let factors = world
                        .hole_combos()
                        .iter()
                        .enumerate()
                        .map(|(seat, &h)| {
                            proposal.sampler.evaluation_combo_weight(seat, h)
                                * proposal.metadata.target_scale_by_seat[seat]
                                / solver.sampler.evaluation_combo_weight(seat, h)
                        })
                        .product::<f64>();
                    let recovered = proposal.correction(&world).unwrap() * factors;
                    assert!((recovered - forced.weight).abs() <= 1e-12 * forced.weight.max(1e-100));
                    let mut later = path.clone();
                    later.push(0);
                    let mut root_weight = ForcedPrefixReplay {
                        actions: &later,
                        endpoint_action: None,
                        weight: 1.0,
                        skip_weight_actions: 0,
                        sources: [false; 3],
                    };
                    let mut proposed_weight = ForcedPrefixReplay {
                        actions: &later,
                        endpoint_action: None,
                        weight: proposal.correction(&world).unwrap(),
                        skip_weight_actions: path.len(),
                        sources: [false; 3],
                    };
                    let old = solver
                        .evaluate_world(
                            &world,
                            &mut evaluation_action_rng(783, id, None),
                            None,
                            None,
                            variant.purify_threshold,
                            variant.use_current_strategy,
                            None,
                            &mut [],
                            Some(&mut root_weight),
                        )
                        .unwrap();
                    let new = solver
                        .evaluate_world(
                            &world,
                            &mut evaluation_action_rng(783, id, None),
                            None,
                            None,
                            variant.purify_threshold,
                            variant.use_current_strategy,
                            None,
                            &mut [],
                            Some(&mut proposed_weight),
                        )
                        .unwrap();
                    assert_eq!(old, new);
                    assert_eq!(root_weight.sources, proposed_weight.sources);
                    assert!(
                        (root_weight.weight - proposed_weight.weight * factors).abs()
                            <= 1e-12 * root_weight.weight.max(1e-100)
                    );
                }
                let paths = vec![path.clone(), {
                    let mut p = path.clone();
                    p.push(0);
                    p
                }];
                let one = solver
                    .evaluate_profile_conditioned_preflop(1025, 786, variant, 1, &paths)
                    .unwrap();
                for threads in [2, 8] {
                    assert_eq!(
                        one,
                        solver
                            .evaluate_profile_conditioned_preflop(
                                1025, 786, variant, threads, &paths
                            )
                            .unwrap()
                    );
                }
                assert!(one.prefixes[0].effective_sample_size > 1024.9);
                let reversed = solver
                    .evaluate_profile_conditioned_preflop(
                        1025,
                        786,
                        variant,
                        2,
                        &[paths[1].clone(), paths[0].clone()],
                    )
                    .unwrap();
                assert_eq!(one.prefixes[0], reversed.prefixes[1]);
                assert_eq!(one.prefixes[1], reversed.prefixes[0]);
                let json = serde_json::to_value(one).unwrap();
                assert!(json["prefixes"][0].get("reach_probability").is_none());
                assert!(json["prefixes"][0].get("relative_weight_mean").is_some());
            }
            assert_eq!(solver.snapshot_state(), before);
            let different = call_through(solver.game(), true);
            assert!(
                solver
                    .evaluate_profile_conditioned_preflop(
                        2,
                        0,
                        ProfileVariant::default(),
                        1,
                        &[path.clone(), different]
                    )
                    .is_err()
            );
            for paths in [
                vec![],
                vec![vec![]],
                vec![vec![usize::MAX]],
                vec![path.clone(), path],
            ] {
                assert!(
                    solver
                        .evaluate_profile_conditioned_preflop(
                            2,
                            0,
                            ProfileVariant::default(),
                            1,
                            &paths
                        )
                        .is_err()
                );
            }
        }
    }
}
