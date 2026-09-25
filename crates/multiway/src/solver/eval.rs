use super::support::*;
use super::*;

// Two fixed candidate policies are selected on the same held-out batch.
// A two-sided Bonferroni split of alpha=0.05 across them uses
// Phi^-1(1 - 0.05 / (2 tails * 2 candidates)) = Phi^-1(0.9875).
const TWO_CANDIDATE_CI95_Z: f64 = 2.2414;
const PARALLEL_EVALUATION_CHUNK_SAMPLES: u64 = 4_096;
const MAX_EVALUATION_PREFIXES: usize = 64;
const PREFIX_EVALUATION_CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Borrowed evaluation-only view of either sparse or dense policy storage.
/// Public `policy()` returns an owned snapshot, but evaluation only reads the
/// column; borrowing avoids cloning labels, regrets, and strategy sums at every
/// visited node.
pub(super) struct EvaluationPolicy<'a> {
    pub(super) action_labels: &'a [String],
    regrets: &'a [f32],
    strategy_sum: &'a [f32],
}

struct ProfileEvaluationSample {
    deal_attempts: u64,
    utilities: Vec<f64>,
    gains: Vec<[f64; 2]>,
    candidate_policy_coverage: Vec<CandidatePolicyCoverage>,
    prefixes: Vec<PrefixPolicyCoverage>,
}

struct ProfileEvaluationAccumulator {
    means: Vec<f64>,
    m2: Vec<f64>,
    gain_means: Vec<[f64; 2]>,
    gain_m2: Vec<[f64; 2]>,
    total_deal_attempts: u64,
    candidate_policy_coverage: Vec<CandidatePolicyCoverage>,
    prefixes: Vec<PrefixPolicyCoverage>,
}

fn empty_prefix_coverage(prefixes: &[HistoryKey], num_players: usize) -> Vec<PrefixPolicyCoverage> {
    prefixes
        .iter()
        .map(|&history| PrefixPolicyCoverage {
            history,
            reached_samples: 0,
            trajectory_visits_by_street: StreetVisitCounts::default(),
            candidate_policy_coverage: vec![CandidatePolicyCoverage::default(); num_players],
        })
        .collect()
}

impl PrefixPolicyCoverage {
    fn record(
        &mut self,
        actor: usize,
        street: u8,
        source: CandidatePolicySource,
    ) -> Result<(), SolverError> {
        if self.reached_samples != 0 {
            self.candidate_policy_coverage[actor].record(street, source)?;
            let count = match street {
                0 => &mut self.trajectory_visits_by_street.preflop,
                1 => &mut self.trajectory_visits_by_street.flop,
                2 => &mut self.trajectory_visits_by_street.turn,
                3 => &mut self.trajectory_visits_by_street.river,
                _ => return Err(SolverError::InvalidPrivateInfo("street is outside 0..=3")),
            };
            // This object represents one sample until accumulator.observe.
            *count = 1;
        }
        Ok(())
    }

    fn checked_add_assign(&mut self, other: Self) -> Result<(), SolverError> {
        self.reached_samples = self
            .reached_samples
            .checked_add(other.reached_samples)
            .ok_or(SolverError::CounterOverflow)?;
        self.trajectory_visits_by_street
            .checked_add_assign(other.trajectory_visits_by_street)?;
        for (ours, theirs) in self
            .candidate_policy_coverage
            .iter_mut()
            .zip(other.candidate_policy_coverage)
        {
            ours.checked_add_assign(theirs)?;
        }
        Ok(())
    }
}

fn evaluation_chunk_samples(prefix_count: usize, num_players: usize) -> u64 {
    if prefix_count == 0 {
        return PARALLEL_EVALUATION_CHUNK_SAMPLES;
    }
    let bytes_per_sample = size_of::<ProfileEvaluationSample>()
        + num_players * (size_of::<CandidatePolicyCoverage>() + 3 * size_of::<f64>())
        + prefix_count
            * (size_of::<PrefixPolicyCoverage>()
                + num_players * size_of::<CandidatePolicyCoverage>());
    (PREFIX_EVALUATION_CHUNK_BYTES / bytes_per_sample)
        .clamp(1, PARALLEL_EVALUATION_CHUNK_SAMPLES as usize) as u64
}

impl ProfileEvaluationAccumulator {
    fn new(num_players: usize, prefixes: &[HistoryKey]) -> Self {
        Self {
            means: vec![0.0; num_players],
            m2: vec![0.0; num_players],
            gain_means: vec![[0.0; 2]; num_players],
            gain_m2: vec![[0.0; 2]; num_players],
            total_deal_attempts: 0,
            candidate_policy_coverage: vec![CandidatePolicyCoverage::default(); num_players],
            prefixes: empty_prefix_coverage(prefixes, num_players),
        }
    }

    fn observe(
        &mut self,
        sample_id: u64,
        sample: ProfileEvaluationSample,
        has_trained_deviators: bool,
    ) -> Result<(), SolverError> {
        self.total_deal_attempts = self
            .total_deal_attempts
            .checked_add(sample.deal_attempts)
            .ok_or(SolverError::CounterOverflow)?;
        for (ours, theirs) in self.prefixes.iter_mut().zip(sample.prefixes) {
            ours.checked_add_assign(theirs)?;
        }
        let count = (sample_id + 1) as f64;
        for (seat, (&utility, gains)) in sample.utilities.iter().zip(&sample.gains).enumerate() {
            self.candidate_policy_coverage[seat]
                .checked_add_assign(sample.candidate_policy_coverage[seat])?;
            let delta = utility - self.means[seat];
            self.means[seat] += delta / count;
            self.m2[seat] += delta * (utility - self.means[seat]);

            let candidate_count = if has_trained_deviators { 2 } else { 1 };
            for (candidate, &gain) in gains.iter().take(candidate_count).enumerate() {
                let gain_delta = gain - self.gain_means[seat][candidate];
                self.gain_means[seat][candidate] += gain_delta / count;
                self.gain_m2[seat][candidate] +=
                    gain_delta * (gain - self.gain_means[seat][candidate]);
            }
        }
        Ok(())
    }

    fn finish(
        self,
        samples: u64,
        has_trained_deviators: bool,
        include_deviations: bool,
    ) -> PrefixProfileEvaluation {
        let seats = self
            .means
            .into_iter()
            .zip(self.m2)
            .map(|(mean, sum_squared_error)| profile_estimate(mean, sum_squared_error, samples))
            .collect();
        let deviation_gain_lower_bound = self
            .gain_means
            .into_iter()
            .zip(self.gain_m2)
            .map(|(seat_means, seat_m2)| {
                if has_trained_deviators {
                    two_candidate_nonnegative_gain_estimate(seat_means, seat_m2, samples)
                } else {
                    nonnegative_gain_estimate(seat_means[0], seat_m2[0], samples)
                }
            })
            .collect();
        PrefixProfileEvaluation {
            evaluation: ProfileEvaluation {
                samples,
                total_deal_attempts: self.total_deal_attempts,
                seats,
                deviation_gain_lower_bound: include_deviations
                    .then_some(deviation_gain_lower_bound),
                candidate_policy_coverage: self.candidate_policy_coverage,
            },
            prefixes: self.prefixes,
        }
    }
}

impl EvaluationPolicy<'_> {
    pub(super) fn strategy(
        &self,
        use_current_strategy: bool,
    ) -> Result<(Vec<f32>, CandidatePolicySource), SolverError> {
        if self
            .regrets
            .iter()
            .chain(self.strategy_sum)
            .any(|value| !value.is_finite())
        {
            return Err(SolverError::InvalidState(
                "policy contains a non-finite value",
            ));
        }
        if self.strategy_sum.iter().any(|&value| value < 0.0) {
            return Err(SolverError::InvalidState(
                "strategy sums must be non-negative",
            ));
        }
        let (strategy, source) = if use_current_strategy {
            (
                regret_matching_f32(self.regrets),
                CandidatePolicySource::Current,
            )
        } else if let Some(strategy) = normalize_nonnegative_f32(self.strategy_sum) {
            (strategy, CandidatePolicySource::Average)
        } else {
            (
                regret_matching_f32(self.regrets),
                CandidatePolicySource::RegretFallback,
            )
        };
        // Finite entries can still overflow their f32 normalization sum.
        // Reject the resulting invalid policy rather than report average
        // coverage while the action sampler silently chooses its last action.
        if strategy.iter().any(|value| !value.is_finite())
            || !strategy.iter().any(|&value| value > 0.0)
        {
            return Err(SolverError::NumericOverflow);
        }
        Ok((strategy, source))
    }
}

/// Samples one profile action at every visited decision, then optionally
/// replaces it with a fixed candidate action.
///
/// Always consuming the draw keeps cloned common-random-number streams aligned
/// along the baseline/candidate shared prefix. The returned fixed action is
/// deterministic, so consuming and discarding its profile draw does not change
/// the candidate policy or either profile's marginal distribution.
fn paired_profile_action(
    strategy: &[f32],
    fixed_action: Option<usize>,
    rng: &mut ChaCha20Rng,
) -> usize {
    let sampled_action = sample_profile_action(strategy, rng);
    fixed_action.unwrap_or(sampled_action)
}

/// Probability intervals used by `sample_profile_action`: f32 normalization
/// may not sum to exactly one, and the sampler assigns remaining mass to its
/// last action. Do not renormalize a second time when taking expectations.
fn profile_action_probabilities(strategy: &[f32]) -> Vec<f64> {
    let mut cumulative = 0.0f64;
    strategy
        .iter()
        .enumerate()
        .map(|(index, &probability)| {
            let previous = cumulative.min(1.0);
            cumulative += f64::from(probability);
            let next = if index + 1 == strategy.len() {
                1.0
            } else {
                cumulative.min(1.0)
            };
            next - previous
        })
        .collect()
}

/// Selection-adjusted estimate for the maximum gain of two fixed candidates.
///
/// `mean` remains the nonnegative maximum sample mean, and `stderr` remains
/// the standard error of that argmax candidate as a diagnostic. The interval
/// is the simultaneous approximate-normal envelope: the maximum of the two
/// Bonferroni-adjusted lower endpoints and the maximum of their upper
/// endpoints. Thus a high-variance candidate cannot disappear from the upper
/// confidence bound merely because its observed mean ranked second.
fn two_candidate_nonnegative_gain_estimate(
    means: [f64; 2],
    sum_squared_errors: [f64; 2],
    samples: u64,
) -> ProfileEstimate {
    let stderrs = sum_squared_errors.map(|m2| standard_error(m2, samples));
    let selected = usize::from(means[1] > means[0]);
    let lower = (0..2)
        .map(|candidate| (means[candidate] - TWO_CANDIDATE_CI95_Z * stderrs[candidate]).max(0.0))
        .fold(0.0, f64::max);
    let upper = (0..2)
        .map(|candidate| (means[candidate] + TWO_CANDIDATE_CI95_Z * stderrs[candidate]).max(0.0))
        .fold(0.0, f64::max);
    ProfileEstimate {
        mean: means[selected].max(0.0),
        stderr: stderrs[selected],
        ci95: [lower, upper],
    }
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    pub(super) fn evaluation_policy(&self, key: InfoKey) -> Option<EvaluationPolicy<'_>> {
        match &self.dense {
            None => self.policies.get(&key).map(|column| EvaluationPolicy {
                action_labels: &column.action_labels,
                regrets: &column.regrets,
                strategy_sum: &column.strategy_sum,
            }),
            Some(dense) => dense.column_view(key).map(|column| EvaluationPolicy {
                action_labels: column.action_labels,
                regrets: column.regrets,
                strategy_sum: column.strategy_sum,
            }),
        }
    }

    /// Trains a fixed deviation policy for `seat` against this solver's CURRENT
    /// profile at `variant` (frozen for the duration of training -- this method
    /// takes `&self` and never touches solver state). Runs `traversals`
    /// independent external-sampling traversals: at `seat`'s own decision
    /// nodes, every action is recursed into and a purely local, purely
    /// unweighted regret table is updated (standard external-sampling MCCFR
    /// restricted to a single traverser against a frozen opponent policy); at
    /// every other seat's decision nodes, one action is sampled from that
    /// seat's stored average strategy (uniform fallback for an infoset the
    /// main solver never visited). Deterministic: identical `(seat,
    /// traversals, seed, variant)` against identical solver state always
    /// produces a bit-identical [`DeviatorPolicy`].
    ///
    /// `variant.purify_threshold` first purifies every opponent
    /// average-strategy read taken while sampling other seats' actions with
    /// [`purify_strategy`] (Ganzfried & Sandholm, AAMAS 2012): entries below
    /// the threshold are zeroed and the remainder renormalized. This trains
    /// the deviator against the SAME (possibly purified) profile
    /// [`Self::evaluate_profile`] replays as the baseline at that variant --
    /// pairing a deviator trained at one variant with a baseline evaluated at
    /// a different variant would compare two different profiles and produce
    /// an incoherent gain estimate. `variant.purify_threshold` must be finite
    /// and in `[0.0, 1.0]`; `0.0` skips the purify call entirely rather than
    /// performing a no-op renormalization (byte-identical to the default
    /// variant).
    ///
    /// `variant.use_current_strategy` additionally swaps every opponent read
    /// from the linear average to the last-iterate regret-matched strategy
    /// (diagnostic; see [`Self::evaluate_profile`]).
    ///
    /// Only information sets visited at least
    /// [`MIN_DEVIATOR_POLICY_VISITS`] times make it into the returned
    /// policy: the argmax of a one- or two-sample local regret is close to
    /// random, and on a large tree a short burst sprinkles exactly such
    /// single visits across thousands of deep infosets. Measured on a 6-max
    /// auto-shape run, including those noisy entries made the trained
    /// deviator strictly WEAKER than the plain regret-greedy heuristic it
    /// was meant to improve on (its measured gain collapsed to zero);
    /// leaving them out lets the evaluation's fallback (main-regret greedy,
    /// which is densely trained precisely at those deep nodes) handle them
    /// instead.
    pub fn train_deviator(
        &self,
        seat: usize,
        traversals: u64,
        seed: u64,
        variant: ProfileVariant,
    ) -> Result<DeviatorPolicy, SolverError> {
        Ok(self
            .train_deviator_with_report(seat, traversals, seed, variant)?
            .policy)
    }

    /// Like [`Self::train_deviator`], retaining reference-partition visit
    /// counts for coverage reporting.
    ///
    /// When the game overrides [`ExternalSamplingGame::deviation_bucket`],
    /// only the deviating seat's local regret/action table uses that common
    /// reference key. Every frozen candidate-policy lookup remains keyed by
    /// [`ExternalSamplingGame::bucket`]. With the trait defaults this takes
    /// the historical key path exactly.
    pub fn train_deviator_with_report(
        &self,
        seat: usize,
        traversals: u64,
        seed: u64,
        variant: ProfileVariant,
    ) -> Result<DeviatorTrainingResult, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        self.train_deviator_core::<false>(
            seat,
            traversals,
            seed,
            variant.purify_threshold,
            variant.use_current_strategy,
        )
    }

    pub(super) fn train_deviator_core<const PREFLOP_ONLY: bool>(
        &self,
        seat: usize,
        traversals: u64,
        seed: u64,
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<DeviatorTrainingResult, SolverError> {
        self.train_deviator_core_with_retention_gate::<PREFLOP_ONLY, false>(
            seat,
            traversals,
            seed,
            purify_threshold,
            use_current_strategy,
        )
    }

    pub(super) fn train_deviator_core_with_retention_gate<
        const PREFLOP_ONLY: bool,
        const RETENTION_GATE: bool,
    >(
        &self,
        seat: usize,
        traversals: u64,
        seed: u64,
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<DeviatorTrainingResult, SolverError> {
        if RETENTION_GATE && !PREFLOP_ONLY {
            return Err(SolverError::InvalidState(
                "retention gating requires preflop-only fitting",
            ));
        }
        let num_players = self.game.num_players();
        if seat >= num_players {
            return Err(SolverError::InvalidActor {
                actor: seat,
                num_players,
            });
        }
        let mut regrets: FxHashMap<InfoKey, (Vec<f32>, u64)> = FxHashMap::default();
        for traversal in 0..traversals {
            let mut deal_rng = deviator_training_deal_rng(seed, seat, traversal);
            let sample = self.sampler.sample_counted(&mut deal_rng)?;
            let mut action_rng = deviator_training_action_rng(seed, seat, traversal);
            self.train_deviator_traverse::<PREFLOP_ONLY, RETENTION_GATE>(
                &sample.world,
                seat,
                self.game.root_state(),
                HistoryKey::ROOT,
                &mut regrets,
                &mut action_rng,
                0,
                purify_threshold,
                use_current_strategy,
            )?;
        }
        let coverage = DeviatorTrainingCoverage {
            traversals,
            visited_infosets: regrets.len() as u64,
            retained_infosets: regrets
                .values()
                .filter(|(_, visits)| *visits >= u64::from(MIN_DEVIATOR_POLICY_VISITS))
                .count() as u64,
            total_visits: regrets.values().try_fold(0u64, |total, (_, visits)| {
                total
                    .checked_add(*visits)
                    .ok_or(SolverError::CounterOverflow)
            })?,
            retained_visits: regrets
                .values()
                .filter(|(_, visits)| *visits >= u64::from(MIN_DEVIATOR_POLICY_VISITS))
                .try_fold(0u64, |total, (_, visits)| {
                    total
                        .checked_add(*visits)
                        .ok_or(SolverError::CounterOverflow)
                })?,
        };
        let actions = regrets
            .into_iter()
            .filter(|(_, (_, visits))| *visits >= u64::from(MIN_DEVIATOR_POLICY_VISITS))
            .map(|(key, (r, _))| (key, regret_greedy_action(&r) as u16))
            .collect();
        Ok(DeviatorTrainingResult {
            policy: DeviatorPolicy { seat, actions },
            coverage,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn train_deviator_traverse<const PREFLOP_ONLY: bool, const RETENTION_GATE: bool>(
        &self,
        world: &SampledWorld,
        seat: usize,
        state: G::State,
        history: HistoryKey,
        regrets: &mut FxHashMap<InfoKey, (Vec<f32>, u64)>,
        rng: &mut ChaCha20Rng,
        depth: u32,
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<f64, SolverError> {
        if depth > self.config.max_traversal_depth {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }
        let num_players = self.game.num_players();
        let Some(actor) = self.game.actor(&state) else {
            let mut utilities = vec![0.0; num_players];
            self.game.terminal_utilities(&state, world, &mut utilities);
            let utility = utilities[seat];
            if !utility.is_finite() {
                return Err(SolverError::NonFiniteUtility { seat, utility });
            }
            return Ok(utility);
        };
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = self.game.node_actions(&state);
        let num_actions = self.game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        // The research scope uses candidate street metadata to decide where
        // an own deviation is allowed. Postflop continuation uses candidate
        // keys, even when the reference abstraction has different buckets.
        let candidate_private = PREFLOP_ONLY.then(|| self.game.bucket(&state, world, actor));
        if let Some(candidate) = candidate_private {
            validate_private_info(candidate, num_players, self.game.recall_mode())?;
        }
        let train_here = actor == seat
            && (!PREFLOP_ONLY
                || candidate_private.as_ref().unwrap().street == Street::Preflop as u8);
        let (private, recall) = if train_here {
            (
                self.game.deviation_bucket(&state, world, actor),
                self.game.deviation_recall_mode(),
            )
        } else {
            (
                candidate_private.unwrap_or_else(|| self.game.bucket(&state, world, actor)),
                self.game.recall_mode(),
            )
        };
        validate_private_info(private, num_players, recall)?;
        if PREFLOP_ONLY && train_here && private.street != Street::Preflop as u8 {
            return Err(SolverError::InvalidPrivateInfo(
                "preflop deviation reference street mismatch",
            ));
        }
        let key = InfoKey {
            history,
            player: actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        let labels = (0..num_actions)
            .map(|index| self.game.action_label_of(&actions, index))
            .collect::<Vec<_>>();
        validate_action_labels(&labels)?;

        if train_here {
            let entry = regrets
                .entry(key)
                .or_insert_with(|| (vec![0.0f32; num_actions], 0));
            if entry.0.len() != num_actions {
                return Err(SolverError::ActionCountChanged {
                    key,
                    stored: entry.0.len(),
                    current: num_actions,
                });
            }
            entry.1 = entry.1.checked_add(1).ok_or(SolverError::CounterOverflow)?;
            let sigma = if RETENTION_GATE && entry.1 < u64::from(MIN_DEVIATOR_POLICY_VISITS) {
                // Keep unsupported continuations on the exact candidate
                // baseline. Still enumerate every own action and accumulate
                // local regrets, including behind zero-own-reach actions.
                // Once the key is retained, ordinary local RM becomes active.
                let candidate = candidate_private.expect("preflop-only gate");
                let candidate_key = InfoKey {
                    history,
                    player: actor as u8,
                    street: candidate.street,
                    active_opponents: candidate.active_opponents,
                    bucket_path: candidate.bucket_path,
                };
                let strategy = self.preflop_fit_baseline_strategy(
                    candidate_key,
                    &labels,
                    purify_threshold,
                    use_current_strategy,
                )?;
                profile_action_probabilities(&strategy)
            } else {
                regret_matching(&entry.0)
            };
            let mut action_values = vec![0.0f64; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let next = self.game.next_state_with(&state, &actions, action);
                let child_history = history.child(actor, action);
                *value = self.train_deviator_traverse::<PREFLOP_ONLY, RETENTION_GATE>(
                    world,
                    seat,
                    next,
                    child_history,
                    regrets,
                    rng,
                    depth + 1,
                    purify_threshold,
                    use_current_strategy,
                )?;
            }
            let node_value = sigma
                .iter()
                .zip(&action_values)
                .map(|(&p, &v)| p * v)
                .sum::<f64>();
            let entry = regrets.get_mut(&key).expect("inserted above");
            for (regret, &value) in entry.0.iter_mut().zip(&action_values) {
                checked_add_f32(regret, value - node_value)?;
            }
            Ok(node_value)
        } else {
            let strategy = if PREFLOP_ONLY {
                // Use exactly the frozen replay policy, with borrowed arena
                // slices and its finite-normalization validation.
                self.preflop_fit_baseline_strategy(
                    key,
                    &labels,
                    purify_threshold,
                    use_current_strategy,
                )?
            } else if let Some(column) = self.policy(key) {
                if column.action_labels != labels {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
                // Same profile-source switch as `evaluate_world`: the
                // deviator must train against the profile that will be
                // replayed as the baseline.
                let mut strategy = if use_current_strategy {
                    regret_matching_f32(&column.regrets)
                } else {
                    column.average_strategy()
                };
                if purify_threshold > 0.0 {
                    purify_strategy(&mut strategy, purify_threshold);
                }
                strategy
            } else {
                vec![1.0 / num_actions as f32; num_actions]
            };
            let action = sample_profile_action(&strategy, rng);
            let next = self.game.next_state_with(&state, &actions, action);
            let child_history = history.child(actor, action);
            self.train_deviator_traverse::<PREFLOP_ONLY, RETENTION_GATE>(
                world,
                seat,
                next,
                child_history,
                regrets,
                rng,
                depth + 1,
                purify_threshold,
                use_current_strategy,
            )
        }
    }

    fn preflop_fit_baseline_strategy(
        &self,
        key: InfoKey,
        labels: &[String],
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<Vec<f32>, SolverError> {
        if let Some(column) = self.evaluation_policy(key) {
            if column.action_labels != labels {
                return Err(SolverError::ActionLabelsChanged { key });
            }
            let (mut strategy, _) = column.strategy(use_current_strategy)?;
            if purify_threshold > 0.0 {
                purify_strategy(&mut strategy, purify_threshold);
            }
            Ok(strategy)
        } else {
            Ok(vec![1.0 / labels.len() as f32; labels.len()])
        }
    }

    /// Equivalent to `evaluate_profile(samples, seed, None,
    /// ProfileVariant::default())`. See [`Self::evaluate_profile`] for the
    /// full description of held-out evaluation semantics.
    pub fn evaluate_average_profile(
        &self,
        samples: u64,
        seed: u64,
    ) -> Result<ProfileEvaluation, SolverError> {
        self.evaluate_profile(samples, seed, None, ProfileVariant::default())
    }

    /// Held-out Monte Carlo evaluation of the profile at `variant`.
    ///
    /// Every sample has its own `(seed, sample_id)` substream. The method is
    /// read-only: it does not advance training counters, change policy sums,
    /// or share the training traversal's random stream. It also evaluates a
    /// fixed candidate deviation for every seat on the same held-out physical
    /// worlds. The candidate chooses the largest stored cumulative regret at
    /// each visited information set and otherwise retains average-profile
    /// play. The reported gain is paired against the baseline profile, then
    /// transformed with the always-available no-deviation option: mean and
    /// confidence endpoints are clamped at zero. This is only a lower bound
    /// for that candidate set, never a full best-response calculation.
    ///
    /// Each seat's held-out deviation can optionally consider a
    /// [`DeviatorPolicy`] trained by [`Self::train_deviator`] IN ADDITION TO
    /// the plain regret-greedy heuristic: when `deviators` is present, both
    /// candidates are replayed on every sample (the greedy candidate on the
    /// exact RNG stream a plain [`Self::evaluate_average_profile`] would
    /// use, so its estimate is identical to the plain call's) and each
    /// seat's reported mean is the larger candidate mean. Its confidence
    /// interval is a simultaneous approximate-normal envelope over both
    /// candidates, using a two-candidate Bonferroni adjustment; in
    /// particular, the upper endpoint retains a lower-mean candidate when
    /// its sampling uncertainty reaches higher. (The obvious
    /// alternative of REPLACING the greedy candidate with the trained one
    /// was measured to weaken the bound on large trees: a short burst
    /// leaves most deep infosets barely visited, and per-infoset noise
    /// there loses to the densely trained main-regret greedy fallback.)
    /// `deviators`, when present, must contain exactly one policy per seat,
    /// seat-indexed (`deviators[i].seat == i`). Passing `None` at the
    /// default variant is byte-identical to [`Self::evaluate_average_profile`].
    ///
    /// `variant.purify_threshold` first purifies every average-strategy read
    /// taken while replaying the baseline profile (and the regret-greedy /
    /// trained-deviator candidates' opponents) with [`purify_strategy`]
    /// (Ganzfried & Sandholm, AAMAS 2012): entries below the threshold are
    /// zeroed and the remainder renormalized; if every entry is below
    /// threshold, only the argmax survives. `variant.purify_threshold` must
    /// be finite and in `[0.0, 1.0]` (`SolverError::InvalidState`
    /// otherwise). `0.0` produces EXACTLY the result the default variant
    /// would (the purify call is skipped entirely rather than performing a
    /// no-op renormalization, so the RNG and floating-point paths are
    /// untouched). When passing `deviators`, train them with
    /// [`Self::train_deviator`] at this SAME `variant`: the deviator must
    /// exploit the exact profile this evaluation replays as the baseline, or
    /// the reported gain compares two different profiles and is not a
    /// coherent measurement.
    ///
    /// `variant.use_current_strategy` additionally swaps the profile under
    /// evaluation (and the deviator candidates' opponents) from the linear
    /// AVERAGE strategy to the LAST-ITERATE regret-matched current
    /// strategy. Plain regret matching carries no last-iterate convergence
    /// guarantee -- the average is the object with the CCE-style bound --
    /// so this is a diagnostic: it measures how exploitable the final
    /// iterate is on its own, which is the question that decides whether a
    /// last-iterate method (e.g. MMD-style regularization) could ever
    /// replace averaging and free the `strategy_sum` half of the dense
    /// arena.
    pub fn evaluate_profile(
        &self,
        samples: u64,
        seed: u64,
        deviators: Option<&[DeviatorPolicy]>,
        variant: ProfileVariant,
    ) -> Result<ProfileEvaluation, SolverError> {
        self.evaluate_profile_with_threads(samples, seed, deviators, variant, 1)
    }

    /// Parallel counterpart to [`Self::evaluate_profile`].
    ///
    /// Physical worlds and every profile replay remain keyed only by
    /// `(seed, sample_id)`. Workers return one independent sample result;
    /// those results are then accumulated on the calling thread in ascending
    /// sample-id order. Consequently every reported mean, standard error,
    /// confidence interval, and deal-attempt count is bit-identical to the
    /// one-thread method for the same inputs.
    ///
    /// `threads == 1` retains the original online O(1)-sample-memory path.
    /// A larger value temporarily stores at most 4096 samples' scalar results
    /// so it can preserve the sequential accumulation order with bounded
    /// memory.
    pub fn evaluate_profile_with_threads(
        &self,
        samples: u64,
        seed: u64,
        deviators: Option<&[DeviatorPolicy]>,
        variant: ProfileVariant,
        threads: usize,
    ) -> Result<ProfileEvaluation, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        self.evaluate_average_profile_core(
            samples,
            seed,
            deviators,
            variant.purify_threshold,
            variant.use_current_strategy,
            threads,
            &[],
            true,
        )
        .map(|result| result.evaluation)
    }

    /// Evaluate the same baseline/candidates as [`Self::evaluate_profile_with_threads`],
    /// also counting baseline decisions at and below up to 64 known public
    /// histories. Prefixes are unique and may overlap. Unknown histories fail
    /// before sampling; valid but unobserved prefixes retain zero visits.
    /// No additional random draws are consumed. Prefix bookkeeping reduces
    /// parallel chunk length to keep counter storage near 8 MiB (excluding
    /// allocator overhead), independently of the total sample count.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_profile_with_prefixes(
        &self,
        samples: u64,
        seed: u64,
        deviators: Option<&[DeviatorPolicy]>,
        variant: ProfileVariant,
        threads: usize,
        prefixes: &[HistoryKey],
    ) -> Result<PrefixProfileEvaluation, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        self.validate_evaluation_prefixes(prefixes)?;
        self.evaluate_average_profile_core(
            samples,
            seed,
            deviators,
            variant.purify_threshold,
            variant.use_current_strategy,
            threads,
            prefixes,
            true,
        )
    }

    /// Baseline-only counterpart to [`Self::evaluate_profile_with_prefixes`].
    /// Uses the identical physical worlds and baseline action streams, but
    /// skips every deviation replay so rare-prefix coverage can use larger
    /// samples economically. `deviation_gain_lower_bound` is explicitly None.
    pub fn evaluate_profile_coverage(
        &self,
        samples: u64,
        seed: u64,
        variant: ProfileVariant,
        threads: usize,
        prefixes: &[HistoryKey],
    ) -> Result<PrefixProfileEvaluation, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        self.validate_evaluation_prefixes(prefixes)?;
        self.evaluate_average_profile_core(
            samples,
            seed,
            None,
            variant.purify_threshold,
            variant.use_current_strategy,
            threads,
            prefixes,
            false,
        )
    }

    pub(super) fn validate_evaluation_prefixes(
        &self,
        prefixes: &[HistoryKey],
    ) -> Result<(), SolverError> {
        if prefixes.len() > MAX_EVALUATION_PREFIXES {
            return Err(SolverError::InvalidState(
                "at most 64 evaluation prefixes are supported",
            ));
        }
        for (index, &prefix) in prefixes.iter().enumerate() {
            if prefixes[..index].contains(&prefix) {
                return Err(SolverError::InvalidState(
                    "evaluation prefixes must be unique",
                ));
            }
            if prefix != HistoryKey::ROOT && self.history_entry(prefix).is_none() {
                return Err(SolverError::InvalidState(
                    "unknown evaluation prefix history",
                ));
            }
        }
        Ok(())
    }

    /// Held-out evaluation of only the trained deviator keyed by the game's
    /// common deviation/reference abstraction.
    ///
    /// Unlike [`Self::evaluate_profile`], this does not also consider the
    /// candidate-specific regret-greedy heuristic. At the deviating seat it
    /// looks up trained actions only with
    /// [`ExternalSamplingGame::deviation_bucket`]. An unvisited reference key
    /// samples the candidate's ordinary baseline strategy, making the
    /// fallback a no-deviation policy rather than a candidate-specific greedy
    /// action. Opponent and fallback strategy reads always use candidate
    /// [`ExternalSamplingGame::bucket`] keys.
    ///
    /// The raw per-world values are returned so callers can make paired
    /// comparisons across candidate abstractions driven by the same
    /// `(seed, sample_id)` physical deals.
    pub fn evaluate_reference_deviators(
        &self,
        samples: u64,
        seed: u64,
        deviators: &[DeviatorPolicy],
        variant: ProfileVariant,
    ) -> Result<ReferenceDeviationEvaluation, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        if samples == 0 {
            return Err(SolverError::ZeroEvaluationSamples);
        }
        let num_players = self.game.num_players();
        if deviators.len() != num_players {
            return Err(SolverError::InvalidState(
                "deviators must contain exactly one policy per seat",
            ));
        }
        for (seat, dev) in deviators.iter().enumerate() {
            if dev.seat != seat {
                return Err(SolverError::InvalidState(
                    "deviators must be seat-indexed (deviators[i].seat == i)",
                ));
            }
        }

        let mut means = vec![0.0; num_players];
        let mut m2 = vec![0.0; num_players];
        let mut gain_means = vec![0.0; num_players];
        let mut gain_m2 = vec![0.0; num_players];
        let mut candidate_policy_coverage = vec![CandidatePolicyCoverage::default(); num_players];
        let mut coverage = vec![ReferenceDeviationCoverage::default(); num_players];
        let mut worlds = Vec::new();
        let mut total_deal_attempts = 0u64;

        for sample_id in 0..samples {
            let mut deal_rng = evaluation_deal_rng(seed, sample_id);
            let sample = self.sampler.sample_counted(&mut deal_rng)?;
            total_deal_attempts = total_deal_attempts
                .checked_add(u64::from(sample.attempts))
                .ok_or(SolverError::CounterOverflow)?;
            // Clone one action stream for the baseline and every deviation.
            // Their sampled play is therefore identical until a fixed
            // deviator action first changes the public history. This is a
            // common-random-numbers coupling: each replay keeps its original
            // marginal distribution while the paired gain sheds unrelated
            // opponent-action noise on the shared prefix.
            let common_action_rng = evaluation_action_rng(seed, sample_id, None);
            let mut profile_rng = common_action_rng.clone();
            let baseline = self.evaluate_world(
                &sample.world,
                &mut profile_rng,
                None,
                None,
                variant.purify_threshold,
                variant.use_current_strategy,
                Some(&mut candidate_policy_coverage),
                &mut [],
                None,
            )?;
            let count = (sample_id + 1) as f64;
            for seat in 0..num_players {
                let delta = baseline[seat] - means[seat];
                means[seat] += delta / count;
                m2[seat] += delta * (baseline[seat] - means[seat]);
            }

            let mut deviating_seat_utilities = Vec::with_capacity(num_players);
            let mut gains = Vec::with_capacity(num_players);
            for seat in 0..num_players {
                let mut trained_rng = common_action_rng.clone();
                let (trained, sample_coverage) = self.evaluate_reference_world::<false>(
                    &sample.world,
                    &mut trained_rng,
                    seat,
                    deviators,
                    variant.purify_threshold,
                    variant.use_current_strategy,
                )?;
                coverage[seat].checked_add_assign(sample_coverage)?;

                let deviating_utility = trained[seat];
                let gain = deviating_utility - baseline[seat];
                let gain_delta = gain - gain_means[seat];
                gain_means[seat] += gain_delta / count;
                gain_m2[seat] += gain_delta * (gain - gain_means[seat]);
                deviating_seat_utilities.push(deviating_utility);
                gains.push(gain);
            }
            worlds.push(ReferenceDeviationWorld {
                sample_id,
                baseline_utilities: baseline,
                deviating_seat_utilities,
                gains,
            });
        }

        let seats = means
            .into_iter()
            .zip(m2)
            .map(|(mean, sum_squared_error)| profile_estimate(mean, sum_squared_error, samples))
            .collect();
        let deviation_gain_lower_bound = gain_means
            .into_iter()
            .zip(gain_m2)
            .map(|(mean, sum_squared_error)| {
                nonnegative_gain_estimate(mean, sum_squared_error, samples)
            })
            .collect();
        Ok(ReferenceDeviationEvaluation {
            evaluation: ProfileEvaluation {
                samples,
                total_deal_attempts,
                seats,
                deviation_gain_lower_bound: Some(deviation_gain_lower_bound),
                candidate_policy_coverage: candidate_policy_coverage.clone(),
            },
            candidate_policy_coverage,
            coverage,
            worlds,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_average_profile_core(
        &self,
        samples: u64,
        seed: u64,
        deviators: Option<&[DeviatorPolicy]>,
        purify_threshold: f32,
        use_current_strategy: bool,
        threads: usize,
        prefixes: &[HistoryKey],
        include_deviations: bool,
    ) -> Result<PrefixProfileEvaluation, SolverError> {
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        if samples == 0 {
            return Err(SolverError::ZeroEvaluationSamples);
        }
        let num_players = self.game.num_players();
        if let Some(devs) = deviators {
            if devs.len() != num_players {
                return Err(SolverError::InvalidState(
                    "deviators must contain exactly one policy per seat",
                ));
            }
            for (seat, dev) in devs.iter().enumerate() {
                if dev.seat != seat {
                    return Err(SolverError::InvalidState(
                        "deviators must be seat-indexed (deviators[i].seat == i)",
                    ));
                }
            }
        }
        let has_trained_deviators = deviators.is_some();
        let mut accumulator = ProfileEvaluationAccumulator::new(num_players, prefixes);
        if threads == 1 {
            for sample_id in 0..samples {
                let sample = self.evaluate_profile_sample(
                    sample_id,
                    seed,
                    deviators,
                    purify_threshold,
                    use_current_strategy,
                    prefixes,
                    include_deviations,
                )?;
                accumulator.observe(sample_id, sample, has_trained_deviators)?;
            }
        } else {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .map_err(|error| SolverError::ThreadPoolBuild(error.to_string()))?;
            let mut chunk_start = 0;
            while chunk_start < samples {
                let chunk_end = chunk_start
                    .saturating_add(evaluation_chunk_samples(prefixes.len(), num_players))
                    .min(samples);
                let chunk_len = usize::try_from(chunk_end - chunk_start)
                    .expect("the fixed evaluation chunk length fits usize");
                let results = pool.install(|| {
                    (0..chunk_len)
                        .into_par_iter()
                        .map(|offset| {
                            let sample_id = chunk_start + offset as u64;
                            self.evaluate_profile_sample(
                                sample_id,
                                seed,
                                deviators,
                                purify_threshold,
                                use_current_strategy,
                                prefixes,
                                include_deviations,
                            )
                        })
                        .collect::<Vec<_>>()
                });
                // IndexedParallelIterator preserves sample-id order. Resolve
                // failures and update Welford moments in that order as well.
                for (offset, result) in results.into_iter().enumerate() {
                    let sample_id = chunk_start + offset as u64;
                    accumulator.observe(sample_id, result?, has_trained_deviators)?;
                }
                chunk_start = chunk_end;
            }
        }
        Ok(accumulator.finish(samples, has_trained_deviators, include_deviations))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_profile_sample(
        &self,
        sample_id: u64,
        seed: u64,
        deviators: Option<&[DeviatorPolicy]>,
        purify_threshold: f32,
        use_current_strategy: bool,
        prefixes: &[HistoryKey],
        include_deviations: bool,
    ) -> Result<ProfileEvaluationSample, SolverError> {
        let mut deal_rng = evaluation_deal_rng(seed, sample_id);
        let sample = self.sampler.sample_counted(&mut deal_rng)?;
        // All profile replays begin from the same action stream. See
        // `paired_profile_action` for why fixed candidate decisions still
        // consume one draw and preserve alignment along the shared prefix.
        let common_action_rng = evaluation_action_rng(seed, sample_id, None);
        let mut profile_rng = common_action_rng.clone();
        let mut candidate_policy_coverage =
            vec![CandidatePolicyCoverage::default(); self.game.num_players()];
        let mut prefix_coverage = empty_prefix_coverage(prefixes, self.game.num_players());
        let utilities = self.evaluate_world(
            &sample.world,
            &mut profile_rng,
            None,
            None,
            purify_threshold,
            use_current_strategy,
            Some(&mut candidate_policy_coverage),
            &mut prefix_coverage,
            None,
        )?;
        let mut gains = vec![[0.0; 2]; utilities.len()];
        for seat in 0..if include_deviations {
            utilities.len()
        } else {
            0
        } {
            let mut deviation_rng = common_action_rng.clone();
            let deviation = self.evaluate_world(
                &sample.world,
                &mut deviation_rng,
                Some(seat),
                None,
                purify_threshold,
                use_current_strategy,
                None,
                &mut [],
                None,
            )?;
            gains[seat][0] = deviation[seat] - utilities[seat];

            if deviators.is_some() {
                let mut trained_rng = common_action_rng.clone();
                let trained = self.evaluate_world(
                    &sample.world,
                    &mut trained_rng,
                    Some(seat),
                    deviators,
                    purify_threshold,
                    use_current_strategy,
                    None,
                    &mut [],
                    None,
                )?;
                gains[seat][1] = trained[seat] - utilities[seat];
            }
        }
        Ok(ProfileEvaluationSample {
            deal_attempts: u64::from(sample.attempts),
            utilities,
            gains,
            candidate_policy_coverage,
            prefixes: prefix_coverage,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn evaluate_world(
        &self,
        world: &SampledWorld,
        rng: &mut ChaCha20Rng,
        deviator: Option<usize>,
        deviators: Option<&[DeviatorPolicy]>,
        purify_threshold: f32,
        use_current_strategy: bool,
        mut candidate_policy_coverage: Option<&mut [CandidatePolicyCoverage]>,
        prefixes: &mut [PrefixPolicyCoverage],
        mut forced: Option<&mut super::conditioned::ForcedPrefixReplay<'_>>,
    ) -> Result<Vec<f64>, SolverError> {
        let num_players = self.game.num_players();
        let mut state = self.game.root_state();
        let mut history = HistoryKey::ROOT;
        for depth in 0..=self.config.max_traversal_depth {
            for prefix in prefixes.iter_mut() {
                if history == prefix.history {
                    prefix.reached_samples = 1;
                }
            }
            let Some(actor) = self.game.actor(&state) else {
                let mut utilities = vec![0.0; num_players];
                self.game.terminal_utilities(&state, world, &mut utilities);
                if let Some((seat, &utility)) = utilities
                    .iter()
                    .enumerate()
                    .find(|(_, utility)| !utility.is_finite())
                {
                    return Err(SolverError::NonFiniteUtility { seat, utility });
                }
                return Ok(utilities);
            };
            if actor >= num_players {
                return Err(SolverError::InvalidActor { actor, num_players });
            }
            let actions = self.game.node_actions(&state);
            let num_actions = self.game.num_actions_of(&actions);
            if num_actions == 0 {
                return Err(SolverError::NoActions { actor });
            }
            let private = self.game.bucket(&state, world, actor);
            validate_private_info(private, num_players, self.game.recall_mode())?;
            let key = InfoKey {
                history,
                player: actor as u8,
                street: private.street,
                active_opponents: private.active_opponents,
                bucket_path: private.bucket_path,
            };
            let labels = (0..num_actions)
                .map(|index| self.game.action_label_of(&actions, index))
                .collect::<Vec<_>>();
            validate_action_labels(&labels)?;
            // The borrowed view dispatches on storage mode without cloning
            // the policy column at every evaluated decision.
            let stored = self.evaluation_policy(key);
            let (strategy, source) = if let Some(column) = &stored {
                if column.action_labels != labels {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
                // `use_current_strategy` swaps the profile under evaluation
                // from the linear average to the LAST-ITERATE regret-matched
                // strategy -- a diagnostic for how far the current iterate
                // is from the average (plain regret matching has no
                // last-iterate convergence guarantee; see the MMD/QRE
                // literature for methods that do).
                let (mut strategy, source) = column.strategy(use_current_strategy)?;
                if purify_threshold > 0.0 {
                    purify_strategy(&mut strategy, purify_threshold);
                }
                (strategy, source)
            } else {
                (
                    vec![1.0 / num_actions as f32; num_actions],
                    CandidatePolicySource::UniformFallback,
                )
            };
            let forced_action = forced
                .as_ref()
                .and_then(|prefix| prefix.actions.get(depth as usize))
                .copied();
            let endpoint_action = forced.as_ref().and_then(|prefix| {
                (depth as usize == prefix.actions.len())
                    .then_some(prefix.endpoint_action)
                    .flatten()
            });
            if endpoint_action.is_some_and(|action| action >= num_actions) {
                return Err(SolverError::InvalidState(
                    "endpoint deviation action is out of bounds",
                ));
            }
            if forced_action.is_none()
                && let Some(coverage) = candidate_policy_coverage.as_deref_mut()
            {
                let seat = coverage
                    .get_mut(actor)
                    .ok_or(SolverError::InvalidActor { actor, num_players })?;
                seat.record(private.street, source)?;
            }
            for prefix in prefixes.iter_mut() {
                prefix.record(actor, private.street, source)?;
            }
            let fixed_action = if deviator == Some(actor) {
                let trained = deviators
                    .and_then(|devs| devs[actor].actions.get(&key))
                    .copied()
                    .filter(|&index| (index as usize) < num_actions);
                match trained {
                    Some(index) => Some(index as usize),
                    None => stored
                        .as_ref()
                        .map(|column| regret_greedy_action(column.regrets)),
                }
            } else {
                None
            };
            if let Some(action) = forced_action {
                let prefix = forced.as_mut().expect("forced action has a prefix");
                match source {
                    CandidatePolicySource::Average => {}
                    CandidatePolicySource::Current => prefix.sources[0] = true,
                    CandidatePolicySource::RegretFallback => prefix.sources[1] = true,
                    CandidatePolicySource::UniformFallback => prefix.sources[2] = true,
                }
                let probability = super::conditioned::profile_action_probability(&strategy, action);
                let weight = if (depth as usize) < prefix.skip_weight_actions {
                    prefix.weight
                } else {
                    prefix.weight * probability
                };
                if weight > 0.0 && weight * weight == 0.0
                    || weight == 0.0 && prefix.weight > 0.0 && probability > 0.0
                {
                    return Err(SolverError::NumericOverflow);
                }
                prefix.weight = weight;
                if weight == 0.0 {
                    // A zero-mass world contributes zeros, never conditional evidence.
                    return Ok(vec![0.0; num_players]);
                }
            }
            let action = paired_profile_action(
                &strategy,
                forced_action.or(endpoint_action).or(fixed_action),
                rng,
            );
            state = self.game.next_state_with(&state, &actions, action);
            history = history.child(actor, action);
            if depth == self.config.max_traversal_depth {
                return Err(SolverError::DepthLimit {
                    limit: self.config.max_traversal_depth,
                });
            }
        }
        unreachable!("depth loop returns at its upper bound")
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn evaluate_reference_world<const PREFLOP_ONLY: bool>(
        &self,
        world: &SampledWorld,
        rng: &mut ChaCha20Rng,
        deviator: usize,
        deviators: &[DeviatorPolicy],
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<(Vec<f64>, ReferenceDeviationCoverage), SolverError> {
        let num_players = self.game.num_players();
        let mut coverage = ReferenceDeviationCoverage::default();
        let mut state = self.game.root_state();
        let mut history = HistoryKey::ROOT;
        for depth in 0..=self.config.max_traversal_depth {
            let Some(actor) = self.game.actor(&state) else {
                let mut utilities = vec![0.0; num_players];
                self.game.terminal_utilities(&state, world, &mut utilities);
                if let Some((seat, &utility)) = utilities
                    .iter()
                    .enumerate()
                    .find(|(_, utility)| !utility.is_finite())
                {
                    return Err(SolverError::NonFiniteUtility { seat, utility });
                }
                return Ok((utilities, coverage));
            };
            if actor >= num_players {
                return Err(SolverError::InvalidActor { actor, num_players });
            }
            let actions = self.game.node_actions(&state);
            let num_actions = self.game.num_actions_of(&actions);
            if num_actions == 0 {
                return Err(SolverError::NoActions { actor });
            }
            let private = self.game.bucket(&state, world, actor);
            validate_private_info(private, num_players, self.game.recall_mode())?;
            let candidate_key = InfoKey {
                history,
                player: actor as u8,
                street: private.street,
                active_opponents: private.active_opponents,
                bucket_path: private.bucket_path,
            };
            let labels = (0..num_actions)
                .map(|index| self.game.action_label_of(&actions, index))
                .collect::<Vec<_>>();
            validate_action_labels(&labels)?;
            let stored = self.evaluation_policy(candidate_key);
            let strategy = if let Some(column) = &stored {
                if column.action_labels != labels {
                    return Err(SolverError::ActionLabelsChanged { key: candidate_key });
                }
                let (mut strategy, _) = column.strategy(use_current_strategy)?;
                if purify_threshold > 0.0 {
                    purify_strategy(&mut strategy, purify_threshold);
                }
                strategy
            } else {
                vec![1.0 / num_actions as f32; num_actions]
            };

            let fixed_action =
                if actor == deviator && PREFLOP_ONLY && private.street != Street::Preflop as u8 {
                    coverage.record(private.street, false)?;
                    None
                } else if actor == deviator {
                    let reference_private = self.game.deviation_bucket(&state, world, actor);
                    validate_private_info(
                        reference_private,
                        num_players,
                        self.game.deviation_recall_mode(),
                    )?;
                    if PREFLOP_ONLY && reference_private.street != Street::Preflop as u8 {
                        return Err(SolverError::InvalidPrivateInfo(
                            "preflop deviation reference street mismatch",
                        ));
                    }
                    let reference_key = InfoKey {
                        history,
                        player: actor as u8,
                        street: reference_private.street,
                        active_opponents: reference_private.active_opponents,
                        bucket_path: reference_private.bucket_path,
                    };
                    let trained = deviators[actor].actions.get(&reference_key).copied();
                    if PREFLOP_ONLY && trained.is_some_and(|index| index as usize >= num_actions) {
                        return Err(SolverError::InvalidState(
                            "preflop deviator action is outside the current menu",
                        ));
                    }
                    let trained = trained.filter(|&index| (index as usize) < num_actions);
                    match trained {
                        Some(index) => {
                            coverage.record(private.street, true)?;
                            Some(index as usize)
                        }
                        None => {
                            coverage.record(private.street, false)?;
                            None
                        }
                    }
                } else {
                    None
                };
            let action = paired_profile_action(&strategy, fixed_action, rng);
            state = self.game.next_state_with(&state, &actions, action);
            history = history.child(actor, action);
            if depth == self.config.max_traversal_depth {
                return Err(SolverError::DepthLimit {
                    limit: self.config.max_traversal_depth,
                });
            }
        }
        unreachable!("depth loop returns at its upper bound")
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn traverse(
        &mut self,
        state: G::State,
        world: &SampledWorld,
        traverser: usize,
        history: HistoryKey,
        reach: &mut [f64],
        sample_importance: f64,
        rng: &mut ChaCha20Rng,
        depth: u32,
    ) -> Result<f64, SolverError> {
        if depth > self.config.max_traversal_depth {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }

        let Some(actor) = self.game.actor(&state) else {
            let mut utilities = vec![0.0; self.game.num_players()];
            self.game.terminal_utilities(&state, world, &mut utilities);
            if let Some((seat, &utility)) = utilities
                .iter()
                .enumerate()
                .find(|(_, utility)| !utility.is_finite())
            {
                return Err(SolverError::NonFiniteUtility { seat, utility });
            }
            self.terminal_evaluations = self
                .terminal_evaluations
                .checked_add(1)
                .ok_or(SolverError::CounterOverflow)?;
            return Ok(utilities[traverser]);
        };

        let num_players = self.game.num_players();
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = self.game.node_actions(&state);
        let num_actions = self.game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let private = self.game.bucket(&state, world, actor);
        validate_private_info(private, num_players, RecallMode::Full)?;
        let key = InfoKey {
            history,
            player: actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        let strategy = self.strategy_for(key, &actions)?;
        let mut label_buf = String::new();

        if actor == traverser {
            let mut action_values = vec![0.0; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let old_reach = reach[actor];
                reach[actor] *= strategy[action];
                label_buf.clear();
                self.game
                    .write_action_label(&actions, action, &mut label_buf);
                let child_history = self.record_history(history, actor, action, &label_buf)?;
                let next = self.game.next_state_with(&state, &actions, action);
                *value = self.traverse(
                    next,
                    world,
                    traverser,
                    child_history,
                    reach,
                    sample_importance,
                    rng,
                    depth + 1,
                )?;
                reach[actor] = old_reach;
            }
            let node_value = strategy
                .iter()
                .zip(&action_values)
                .map(|(&probability, &value)| probability * value)
                .sum::<f64>();
            let column = self
                .policies
                .get_mut(&key)
                .expect("policy inserted before traversal");
            for (regret, &value) in column.regrets.iter_mut().zip(&action_values) {
                checked_add_f32(regret, sample_importance * (value - node_value))?;
            }
            Ok(node_value)
        } else {
            {
                let column = self
                    .policies
                    .get_mut(&key)
                    .expect("policy inserted before traversal");
                let linear_weight = (self.completed_sweeps + 1) as f64;
                for (sum, &probability) in column.strategy_sum.iter_mut().zip(&strategy) {
                    checked_add_f32(sum, linear_weight * reach[actor] * probability)?;
                }
            }
            let (action, sampling_probability) =
                sample_exploratory_action(&strategy, self.config.exploration_epsilon, rng);
            let importance = strategy[action] / sampling_probability;
            let child_importance = sample_importance * importance;
            if !child_importance.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            let old_reach = reach[actor];
            reach[actor] *= strategy[action];
            label_buf.clear();
            self.game
                .write_action_label(&actions, action, &mut label_buf);
            let child_history = self.record_history(history, actor, action, &label_buf)?;
            let next = self.game.next_state_with(&state, &actions, action);
            let result = self.traverse(
                next,
                world,
                traverser,
                child_history,
                reach,
                child_importance,
                rng,
                depth + 1,
            );
            reach[actor] = old_reach;
            // Exploration samples q rather than sigma. This importance
            // ratio keeps the recursive target-policy value unbiased.
            let weighted_value = result? * importance;
            if !weighted_value.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            Ok(weighted_value)
        }
    }
}

/// Strategy purification/thresholding (Ganzfried & Sandholm, AAMAS 2012):
/// zeroes every entry strictly below `threshold` and renormalizes the
/// remainder to sum to `1`. If every entry falls below `threshold`
/// (including the degenerate `threshold >= 1.0` case, which purifies down
/// to a pure strategy), only the single highest-probability action
/// survives with probability `1`; ties are broken toward the lowest index.
/// Callers wanting the untouched profile should skip calling this at
/// `threshold == 0.0` rather than relying on it being a no-op: `< 0.0` is
/// never true for a probability, so it always takes the "renormalize by
/// dividing by the (already ~1) sum" branch, which is only a rounding
/// no-op, not a bit-identical one.
pub(super) fn purify_strategy(strategy: &mut [f32], threshold: f32) {
    // The argmax (lowest index on ties) is computed up front, over the
    // UNMODIFIED values, because it's also needed as the fallback when
    // every entry turns out to be below threshold -- computing it after
    // zeroing entries in place would read back zeros instead of the
    // original probabilities.
    let mut argmax = 0usize;
    let mut best = strategy[0];
    for (index, &value) in strategy.iter().enumerate().skip(1) {
        if value > best {
            best = value;
            argmax = index;
        }
    }
    let sum: f32 = strategy
        .iter()
        .copied()
        .filter(|&value| value >= threshold)
        .sum();
    if sum > 0.0 {
        for value in strategy.iter_mut() {
            *value = if *value < threshold {
                0.0
            } else {
                *value / sum
            };
        }
    } else {
        // Every entry was below threshold: keep only the argmax with
        // probability 1.
        for (index, value) in strategy.iter_mut().enumerate() {
            *value = f32::from(u8::from(index == argmax));
        }
    }
}

/// Shared validation for `purify_threshold` parameters: must be finite and
/// within `[0.0, 1.0]` (`1.0` is the degenerate case where every entry
/// short of an exact tie at `1.0` is purified away, i.e. full purification
/// to the argmax).
pub(super) fn validate_purify_threshold(purify_threshold: f32) -> Result<(), SolverError> {
    if !purify_threshold.is_finite() || !(0.0..=1.0).contains(&purify_threshold) {
        return Err(SolverError::InvalidState(
            "purify_threshold must be finite and within [0.0, 1.0]",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod estimate_tests {
    use super::*;

    #[test]
    fn borrowed_sparse_and_dense_views_match_owned_strategy_arithmetic() {
        let owned = PolicyColumn {
            action_labels: vec!["fold".into(), "call".into(), "raise".into()],
            regrets: vec![-2.0, 1.0, 3.0],
            strategy_sum: vec![2.0, 3.0, 5.0],
        };
        let sparse_view = EvaluationPolicy {
            action_labels: &owned.action_labels,
            regrets: &owned.regrets,
            strategy_sum: &owned.strategy_sum,
        };
        assert_eq!(sparse_view.action_labels, owned.action_labels);
        assert_eq!(
            sparse_view.strategy(false).unwrap().0,
            owned.average_strategy()
        );
        assert_eq!(
            sparse_view.strategy(true).unwrap().0,
            owned.current_strategy()
        );

        // Dense storage exposes the same three fields as independent arena
        // slices. Include zero strategy mass to exercise the exact fallback
        // to regret matching used by `PolicyColumn::average_strategy`.
        let dense_labels = vec!["check".into(), "bet".into()];
        let dense_regrets = vec![-1.0, 4.0];
        let dense_strategy_sum = vec![0.0, 0.0];
        let dense_view = EvaluationPolicy {
            action_labels: &dense_labels,
            regrets: &dense_regrets,
            strategy_sum: &dense_strategy_sum,
        };
        let equivalent_owned = PolicyColumn {
            action_labels: dense_labels.clone(),
            regrets: dense_regrets.clone(),
            strategy_sum: dense_strategy_sum.clone(),
        };
        assert_eq!(dense_view.action_labels, equivalent_owned.action_labels);
        assert_eq!(
            dense_view.strategy(false).unwrap().0,
            equivalent_owned.average_strategy()
        );
        assert_eq!(
            dense_view.strategy(true).unwrap().0,
            equivalent_owned.current_strategy()
        );
    }

    #[test]
    fn evaluation_policy_source_requires_valid_observed_average_mass() {
        let labels = vec!["check".into(), "bet".into()];
        let regrets = [1.0, 3.0];
        for (mass, expected) in [
            ([2.0, 6.0], CandidatePolicySource::Average),
            ([0.0, 0.0], CandidatePolicySource::RegretFallback),
            ([f32::MAX, 0.0], CandidatePolicySource::Average),
        ] {
            let view = EvaluationPolicy {
                action_labels: &labels,
                regrets: &regrets,
                strategy_sum: &mass,
            };
            assert_eq!(view.strategy(false).unwrap().1, expected);
            assert_eq!(
                view.strategy(true).unwrap(),
                (vec![0.25, 0.75], CandidatePolicySource::Current)
            );
        }
        for mass in [
            [-1.0, 2.0],
            [f32::NAN, 1.0],
            [f32::INFINITY, 1.0],
            [f32::NEG_INFINITY, 1.0],
        ] {
            let view = EvaluationPolicy {
                action_labels: &labels,
                regrets: &regrets,
                strategy_sum: &mass,
            };
            assert!(matches!(
                view.strategy(false),
                Err(SolverError::InvalidState(_))
            ));
        }
        let overflow = EvaluationPolicy {
            action_labels: &labels,
            regrets: &regrets,
            strategy_sum: &[f32::MAX, f32::MAX],
        };
        assert!(matches!(
            overflow.strategy(false),
            Err(SolverError::NumericOverflow)
        ));
        let regret_overflow = EvaluationPolicy {
            action_labels: &labels,
            regrets: &[f32::MAX, f32::MAX],
            strategy_sum: &[0.0, 0.0],
        };
        for current in [false, true] {
            assert!(matches!(
                regret_overflow.strategy(current),
                Err(SolverError::NumericOverflow)
            ));
        }
    }

    #[test]
    fn paired_profile_action_matches_the_existing_sampler_without_a_fixed_action() {
        let strategy = [0.15, 0.35, 0.5];
        let mut expected_rng = evaluation_action_rng(17, 29, None);
        let mut paired_rng = expected_rng.clone();

        for _ in 0..64 {
            assert_eq!(
                paired_profile_action(&strategy, None, &mut paired_rng),
                sample_profile_action(&strategy, &mut expected_rng)
            );
        }
    }

    #[test]
    fn fixed_candidate_action_consumes_one_draw_and_keeps_shared_prefix_aligned() {
        let first_strategy = [0.2, 0.3, 0.5];
        let next_strategy = [0.4, 0.6];
        let mut baseline_rng = evaluation_action_rng(31, 47, None);
        let mut candidate_rng = baseline_rng.clone();

        let _baseline_first = paired_profile_action(&first_strategy, None, &mut baseline_rng);
        assert_eq!(
            paired_profile_action(&first_strategy, Some(1), &mut candidate_rng),
            1
        );
        assert_eq!(
            paired_profile_action(&next_strategy, None, &mut candidate_rng),
            paired_profile_action(&next_strategy, None, &mut baseline_rng),
            "a fixed candidate decision must not shift later common draws"
        );
    }

    #[test]
    fn common_action_stream_eliminates_shared_prefix_noise_from_paired_gain() {
        let stochastic_opponent = [0.5, 0.5];
        let pure_baseline_deviator = [1.0, 0.0];
        let mut paired_gains = Vec::new();
        let mut independent_gains = Vec::new();

        for sample_id in 0..512 {
            let common_rng = evaluation_action_rng(73, sample_id, None);
            let mut baseline_rng = common_rng.clone();
            let mut paired_candidate_rng = common_rng.clone();
            let mut independent_candidate_rng = evaluation_action_rng(73, sample_id, Some(0));

            let baseline_noise =
                paired_profile_action(&stochastic_opponent, None, &mut baseline_rng) as f64;
            let baseline_action =
                paired_profile_action(&pure_baseline_deviator, None, &mut baseline_rng) as f64;
            let paired_noise =
                paired_profile_action(&stochastic_opponent, None, &mut paired_candidate_rng) as f64;
            let paired_action =
                paired_profile_action(&pure_baseline_deviator, Some(1), &mut paired_candidate_rng)
                    as f64;
            let independent_noise =
                paired_profile_action(&stochastic_opponent, None, &mut independent_candidate_rng)
                    as f64;
            let independent_action = paired_profile_action(
                &pure_baseline_deviator,
                Some(1),
                &mut independent_candidate_rng,
            ) as f64;

            let baseline_utility = baseline_noise + baseline_action;
            paired_gains.push(paired_noise + paired_action - baseline_utility);
            independent_gains.push(independent_noise + independent_action - baseline_utility);
        }

        assert!(paired_gains.iter().all(|&gain| gain == 1.0));
        let independent_mean =
            independent_gains.iter().sum::<f64>() / independent_gains.len() as f64;
        let independent_m2 = independent_gains
            .iter()
            .map(|gain| (gain - independent_mean).powi(2))
            .sum::<f64>();
        assert!(independent_m2 > 100.0);
        assert!((independent_mean - 1.0).abs() < 0.1);
    }

    #[test]
    fn simultaneous_envelope_retains_lower_mean_high_variance_upper() {
        let samples = 101;
        let means = [0.5, 0.4];
        let m2 = [1.0, 100.0];
        let estimate = two_candidate_nonnegative_gain_estimate(means, m2, samples);
        let stderrs = m2.map(|value| standard_error(value, samples));

        assert_eq!(estimate.mean, means[0]);
        assert_eq!(estimate.stderr, stderrs[0]);
        assert_eq!(
            estimate.ci95,
            [
                (means[0] - TWO_CANDIDATE_CI95_Z * stderrs[0]).max(0.0),
                (means[1] + TWO_CANDIDATE_CI95_Z * stderrs[1]).max(0.0),
            ]
        );
    }

    #[test]
    fn simultaneous_envelope_is_candidate_order_invariant() {
        let forward = two_candidate_nonnegative_gain_estimate([0.5, 0.4], [1.0, 100.0], 101);
        let reversed = two_candidate_nonnegative_gain_estimate([0.4, 0.5], [100.0, 1.0], 101);
        assert_eq!(forward, reversed);
    }

    #[test]
    fn duplicate_candidate_keeps_diagnostic_and_widens_for_selection() {
        let mean = 0.3;
        let m2 = 9.0;
        let samples = 101;
        let single = nonnegative_gain_estimate(mean, m2, samples);
        let duplicate = two_candidate_nonnegative_gain_estimate([mean; 2], [m2; 2], samples);

        assert_eq!(duplicate.mean, single.mean);
        assert_eq!(duplicate.stderr, single.stderr);
        assert!(duplicate.ci95[0] <= single.ci95[0]);
        assert!(duplicate.ci95[1] >= single.ci95[1]);
    }

    #[test]
    fn simultaneous_envelope_clamps_the_no_deviation_option_at_zero() {
        let estimate = two_candidate_nonnegative_gain_estimate([-0.2, -0.3], [0.01, 0.01], 101);
        assert_eq!(estimate.mean, 0.0);
        assert_eq!(estimate.ci95, [0.0, 0.0]);
    }
}

#[cfg(test)]
mod preflop_fit_counter_tests {
    use super::*;
    use crate::solver::preflop_deviation_tests::{ContinuationState, continuation_solver};

    #[test]
    fn retention_gate_uses_candidate_variant_until_eighth_visit_and_keeps_all_actions() {
        let state = ContinuationState::Safe;
        let reference_key = state.key(7);
        for (current, threshold, average, regrets, baseline) in [
            (false, 0.0, [1.0, 0.0], [0.0, 1.0], 4.0),
            (true, 0.0, [1.0, 0.0], [0.0, 1.0], 2.0),
            (false, 0.5, [2.0, 3.0], [1.0, 0.0], 2.0),
            (true, 0.5, [0.0, 1.0], [3.0, 2.0], 4.0),
        ] {
            let mut solver = continuation_solver();
            let column = solver.policies.get_mut(&state.key(5)).unwrap();
            column.strategy_sum = average.to_vec();
            column.regrets = regrets.to_vec();
            let before = solver.snapshot_state();
            let mut rng = deviator_training_deal_rng(620, 0, 0);
            let world = solver.sampler.sample_counted(&mut rng).unwrap().world;
            for (previous_visits, expected) in [(0, baseline), (6, baseline), (7, 3.0)] {
                let mut local =
                    FxHashMap::from_iter([(reference_key, (vec![0.0, 0.0], previous_visits))]);
                let value = solver
                    .train_deviator_traverse::<true, true>(
                        &world,
                        0,
                        state,
                        reference_key.history,
                        &mut local,
                        &mut deviator_training_action_rng(620, 0, 0),
                        0,
                        threshold,
                        current,
                    )
                    .unwrap();
                assert_eq!(value, expected);
                let (r, visits) = &local[&reference_key];
                assert_eq!(*visits, previous_visits + 1);
                assert_eq!(r, &vec![(4.0 - expected) as f32, (2.0 - expected) as f32]);
                assert_eq!(local.len(), 1);
            }
            assert_eq!(solver.snapshot_state(), before);
        }
    }

    #[test]
    fn retention_gate_expectations_use_sampler_intervals_including_last_action_remainder() {
        let a = f64::from(0.1f32);
        let b = f64::from(0.2f32);
        assert_eq!(
            profile_action_probabilities(&[0.1, 0.2, 0.7]),
            vec![a, b, 1.0 - a - b]
        );
        assert_eq!(
            profile_action_probabilities(&[0.75, 0.75, 0.0]),
            vec![0.75, 0.25, 0.0]
        );
        assert_eq!(
            profile_action_probabilities(&[0.0, 0.0, 1.0]),
            vec![0.0, 0.0, 1.0]
        );
    }

    fn one_visit<const PREFLOP_ONLY: bool>(
        initial_visits: u64,
    ) -> (Result<f64, SolverError>, (Vec<f32>, u64)) {
        let solver = continuation_solver();
        let before = solver.snapshot_state();
        let mut deal_rng = deviator_training_deal_rng(618, 0, 0);
        let world = solver.sampler.sample_counted(&mut deal_rng).unwrap().world;
        // Safe is one own decision immediately before terminal values 4/2.
        // Its initial uniform policy has value 3 and regret increment 1/-1.
        let state = ContinuationState::Safe;
        let key = state.key(7);
        let mut regrets = FxHashMap::from_iter([(key, (vec![0.0, 0.0], initial_visits))]);
        let result = solver.train_deviator_traverse::<PREFLOP_ONLY, false>(
            &world,
            0,
            state,
            key.history,
            &mut regrets,
            &mut deviator_training_action_rng(618, 0, 0),
            0,
            0.0,
            false,
        );
        assert_eq!(solver.snapshot_state(), before);
        (result, regrets.remove(&key).unwrap())
    }

    #[test]
    fn preflop_deviation_fit_visits_advance_beyond_the_old_u32_limit() {
        for (result, (regrets, visits)) in [
            one_visit::<true>(u64::from(u32::MAX)),
            one_visit::<false>(u64::from(u32::MAX)),
        ] {
            assert_eq!(result.unwrap(), 3.0);
            assert_eq!(regrets, vec![1.0, -1.0]);
            assert_eq!(visits, u64::from(u32::MAX) + 1);
        }
    }

    #[test]
    fn preflop_deviation_fit_visit_overflow_is_explicit_before_local_regret_updates() {
        for (result, (regrets, visits)) in
            [one_visit::<true>(u64::MAX), one_visit::<false>(u64::MAX)]
        {
            assert!(matches!(result, Err(SolverError::CounterOverflow)));
            assert_eq!(regrets, vec![0.0, 0.0]);
            assert_eq!(visits, u64::MAX);
        }
    }
}
