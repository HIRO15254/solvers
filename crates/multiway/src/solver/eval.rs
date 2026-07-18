use super::support::*;
use super::*;

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
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
        validate_purify_threshold(variant.purify_threshold)?;
        self.train_deviator_core(
            seat,
            traversals,
            seed,
            variant.purify_threshold,
            variant.use_current_strategy,
        )
    }

    fn train_deviator_core(
        &self,
        seat: usize,
        traversals: u64,
        seed: u64,
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<DeviatorPolicy, SolverError> {
        let num_players = self.game.num_players();
        if seat >= num_players {
            return Err(SolverError::InvalidActor {
                actor: seat,
                num_players,
            });
        }
        let mut regrets: FxHashMap<InfoKey, (Vec<f32>, u32)> = FxHashMap::default();
        for traversal in 0..traversals {
            let mut deal_rng = deviator_training_deal_rng(seed, seat, traversal);
            let sample = self.sampler.sample_counted(&mut deal_rng)?;
            let mut action_rng = deviator_training_action_rng(seed, seat, traversal);
            self.train_deviator_traverse(
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
        let actions = regrets
            .into_iter()
            .filter(|(_, (_, visits))| *visits >= MIN_DEVIATOR_POLICY_VISITS)
            .map(|(key, (r, _))| (key, regret_greedy_action(&r) as u16))
            .collect();
        Ok(DeviatorPolicy { seat, actions })
    }

    #[allow(clippy::too_many_arguments)]
    fn train_deviator_traverse(
        &self,
        world: &SampledWorld,
        seat: usize,
        state: G::State,
        history: HistoryKey,
        regrets: &mut FxHashMap<InfoKey, (Vec<f32>, u32)>,
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

        if actor == seat {
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
            entry.1 = entry.1.saturating_add(1);
            let sigma = regret_matching(&entry.0);
            let mut action_values = vec![0.0f64; num_actions];
            for (action, value) in action_values.iter_mut().enumerate() {
                let next = self.game.next_state_with(&state, &actions, action);
                let child_history = history.child(actor, action);
                *value = self.train_deviator_traverse(
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
            let stored = self.policy(key);
            let strategy = if let Some(column) = &stored {
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
            self.train_deviator_traverse(
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
    /// seat's reported bound is the candidate with the higher estimated
    /// mean. Each candidate alone is a fixed policy evaluated on held-out
    /// samples — a valid lower bound — and picking the larger of two such
    /// bounds can only delay a threshold-crossing stop decision, i.e. it is
    /// conservative in exactly the direction that matters. (The obvious
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
        validate_purify_threshold(variant.purify_threshold)?;
        self.evaluate_average_profile_core(
            samples,
            seed,
            deviators,
            variant.purify_threshold,
            variant.use_current_strategy,
        )
    }

    fn evaluate_average_profile_core(
        &self,
        samples: u64,
        seed: u64,
        deviators: Option<&[DeviatorPolicy]>,
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<ProfileEvaluation, SolverError> {
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
        let mut means = vec![0.0; num_players];
        let mut m2 = vec![0.0; num_players];
        // Candidate 0: regret-greedy heuristic (always). Candidate 1: the
        // trained deviator (only when `deviators` is present). Accumulated
        // separately; the per-seat winner by mean is reported.
        let mut gain_means = vec![[0.0; 2]; num_players];
        let mut gain_m2 = vec![[0.0; 2]; num_players];
        let mut total_deal_attempts = 0u64;

        for sample_id in 0..samples {
            let mut deal_rng = evaluation_deal_rng(seed, sample_id);
            let sample = self.sampler.sample_counted(&mut deal_rng)?;
            total_deal_attempts = total_deal_attempts
                .checked_add(u64::from(sample.attempts))
                .ok_or(SolverError::CounterOverflow)?;
            let mut profile_rng = evaluation_action_rng(seed, sample_id, None);
            let utilities = self.evaluate_world(
                &sample.world,
                &mut profile_rng,
                None,
                None,
                purify_threshold,
                use_current_strategy,
            )?;
            let count = (sample_id + 1) as f64;
            for seat in 0..num_players {
                let delta = utilities[seat] - means[seat];
                means[seat] += delta / count;
                m2[seat] += delta * (utilities[seat] - means[seat]);

                let mut deviation_rng = evaluation_action_rng(seed, sample_id, Some(seat));
                let deviation = self.evaluate_world(
                    &sample.world,
                    &mut deviation_rng,
                    Some(seat),
                    None,
                    purify_threshold,
                    use_current_strategy,
                )?;
                let gain = deviation[seat] - utilities[seat];
                let gain_delta = gain - gain_means[seat][0];
                gain_means[seat][0] += gain_delta / count;
                gain_m2[seat][0] += gain_delta * (gain - gain_means[seat][0]);

                if deviators.is_some() {
                    let mut trained_rng = deviator_evaluation_action_rng(seed, sample_id, seat);
                    let trained = self.evaluate_world(
                        &sample.world,
                        &mut trained_rng,
                        Some(seat),
                        deviators,
                        purify_threshold,
                        use_current_strategy,
                    )?;
                    let gain = trained[seat] - utilities[seat];
                    let gain_delta = gain - gain_means[seat][1];
                    gain_means[seat][1] += gain_delta / count;
                    gain_m2[seat][1] += gain_delta * (gain - gain_means[seat][1]);
                }
            }
        }

        let seats = means
            .into_iter()
            .zip(m2)
            .map(|(mean, sum_squared_error)| profile_estimate(mean, sum_squared_error, samples))
            .collect();
        let deviation_gain_lower_bound = gain_means
            .into_iter()
            .zip(gain_m2)
            .map(|(seat_means, seat_m2)| {
                let candidate = if deviators.is_some() && seat_means[1] > seat_means[0] {
                    1
                } else {
                    0
                };
                nonnegative_gain_estimate(seat_means[candidate], seat_m2[candidate], samples)
            })
            .collect();
        Ok(ProfileEvaluation {
            samples,
            total_deal_attempts,
            seats,
            deviation_gain_lower_bound: Some(deviation_gain_lower_bound),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_world(
        &self,
        world: &SampledWorld,
        rng: &mut ChaCha20Rng,
        deviator: Option<usize>,
        deviators: Option<&[DeviatorPolicy]>,
        purify_threshold: f32,
        use_current_strategy: bool,
    ) -> Result<Vec<f64>, SolverError> {
        let num_players = self.game.num_players();
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
            // `policy` already dispatches on storage mode, so this evaluation
            // loop needs no dense/sparse branch of its own.
            let stored = self.policy(key);
            let strategy = if let Some(column) = &stored {
                if column.action_labels != labels {
                    return Err(SolverError::ActionLabelsChanged { key });
                }
                // `use_current_strategy` swaps the profile under evaluation
                // from the linear average to the LAST-ITERATE regret-matched
                // strategy -- a diagnostic for how far the current iterate
                // is from the average (plain regret matching has no
                // last-iterate convergence guarantee; see the MMD/QRE
                // literature for methods that do).
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
            let action = if deviator == Some(actor) {
                let trained = deviators
                    .and_then(|devs| devs[actor].actions.get(&key))
                    .copied()
                    .filter(|&index| (index as usize) < num_actions);
                match trained {
                    Some(index) => index as usize,
                    None => match &stored {
                        Some(column) => regret_greedy_action(&column.regrets),
                        None => sample_profile_action(&strategy, rng),
                    },
                }
            } else {
                sample_profile_action(&strategy, rng)
            };
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

    /// Per-hand-group, per-action expected-utility estimate at the decision
    /// node reached by replaying `path` from the root, evaluated against
    /// this solver's own average strategy (sparse or dense storage, both
    /// dispatch through [`Self::average_strategy`]). See the free function
    /// [`evaluate_node_actions`] for the full algorithm and estimator.
    pub fn evaluate_node_actions(
        &self,
        path: &[usize],
        samples: u64,
        seed: u64,
    ) -> Result<NodeActionEvaluation, SolverError> {
        evaluate_node_actions(
            &self.game,
            &self.sampler,
            self,
            path,
            samples,
            seed,
            self.config.max_traversal_depth,
        )
    }
}

/// Per-`(hand-group, action)` importance-sampling accumulator backing
/// [`evaluate_node_actions`]'s [`ProfileEstimate`]s. Every sample contributes
/// `(weight, value)`, where `weight` is the product of the average-strategy
/// probabilities of the path actually taken en route to the evaluated node
/// (see that function's doc comment) and `value` is the seat-of-interest's
/// utility from playing the cell's action now and the average profile
/// afterward.
#[derive(Clone, Copy, Debug, Default)]
struct NodeEvalCell {
    sum_w: f64,
    sum_wx: f64,
    sum_w2: f64,
    sum_w2x: f64,
    sum_w2x2: f64,
    count: u64,
}

impl NodeEvalCell {
    fn add(&mut self, weight: f64, value: f64) {
        let w2 = weight * weight;
        self.sum_w += weight;
        self.sum_wx += weight * value;
        self.sum_w2 += w2;
        self.sum_w2x += w2 * value;
        self.sum_w2x2 += w2 * value * value;
        self.count += 1;
    }

    /// Self-normalized importance-sampling estimate of the weighted mean
    /// `Σ w·x / Σ w` and its variance `Σ w²(x - mean)² / (Σ w)²` -- the
    /// standard ratio-estimator (delta-method) variance for a
    /// self-normalized importance-sampling mean (see e.g. Owen, *Monte Carlo
    /// theory, methods and examples*, ch. 9, for the general form). Zero
    /// total weight (every contributing sample had a zero-probability path
    /// action) reports a degenerate zero estimate rather than dividing by
    /// zero.
    fn estimate(&self) -> ProfileEstimate {
        if self.sum_w <= 0.0 {
            return ProfileEstimate {
                mean: 0.0,
                stderr: 0.0,
                ci95: [0.0, 0.0],
            };
        }
        let mean = self.sum_wx / self.sum_w;
        let variance_numerator =
            (self.sum_w2x2 - 2.0 * mean * self.sum_w2x + mean * mean * self.sum_w2).max(0.0);
        let stderr = variance_numerator.sqrt() / self.sum_w;
        let radius = 1.96 * stderr;
        ProfileEstimate {
            mean,
            stderr,
            ci95: [mean - radius, mean + radius],
        }
    }
}

/// One hand-group's accumulator while [`evaluate_node_actions`] is still
/// sampling; converted to a [`NodeActionGroupEvaluation`] once every sample
/// has been folded in.
struct NodeEvalGroup {
    weight: f64,
    samples: u64,
    /// The group's own average strategy at the evaluated node, recorded once
    /// (it is a deterministic function of the group's `InfoKey`, not of any
    /// individual sample).
    frequencies: Vec<f64>,
    cells: Vec<NodeEvalCell>,
}

/// Estimates, for each of the acting seat's hand-groups and each legal
/// action at the decision node reached by replaying `path` from `game`'s
/// root, the expected utility of "take this action now, then everyone
/// (including the actor) plays `strategy`'s average profile to the end of
/// the hand". This is the evaluator behind a GTO-Wizard-style per-hand/
/// per-action EV display.
///
/// `path` is a sequence of action indices (see
/// [`ExternalSamplingGame::node_actions`] /
/// [`ExternalSamplingGame::next_state_with`]); replaying it is entirely
/// card-independent (poker actions never depend on hole cards), so it
/// happens exactly once, up front, rather than once per sample. It is an
/// error for `path` to index past a node's legal action count, or to
/// continue past a terminal state.
///
/// Each of `samples` draws is a fresh, deterministic, seeded physical world
/// (independent of any live solver's training RNG streams -- this evaluator
/// never touches those, and never mutates `strategy` or any solver/learning
/// state; it is `&self`-only throughout, safe to call from a read-only
/// diagnostics path while a solver keeps training). Walking from the root to
/// the evaluated node, every path node's average strategy (via `strategy`,
/// falling back to uniform over that node's legal actions on a lookup miss)
/// is looked up at that node's own actor's current bucket, and the sample's
/// weight is multiplied by the probability the path actually assigns to the
/// action taken; a zero-probability path action zeroes the sample's weight,
/// and the sample is skipped from there on (it would contribute nothing
/// regardless). At the evaluated node, the acting seat ("hero")'s hand-group
/// is hero's own current-street bucket (the 169-class itself, for a
/// preflop node); for each legal action, hero's utility is sampled by
/// applying that action and then playing every subsequent decision (hero's
/// own included) from `strategy`'s average profile (again uniform on a
/// miss) to a terminal. Every action at one sample reuses the exact same
/// playout RNG stream (common random numbers), so action-to-action EV
/// differences see reduced sampling variance instead of independent noise;
/// board and hole cards are already fixed by the sample's world.
///
/// Per `(group, action)` cell, `Σ weight · value`, `Σ weight`, and the count
/// are accumulated into a [`NodeActionGroupEvaluation`] row (see
/// [`NodeEvalCell::estimate`] for the self-normalized importance-sampling
/// mean/variance estimator); the same accumulation, pooled across every
/// group, becomes each action's [`NodeActionAggregate`].
pub fn evaluate_node_actions<G: ExternalSamplingGame>(
    game: &G,
    sampler: &DealSampler,
    strategy: &impl AverageStrategyLookup,
    path: &[usize],
    samples: u64,
    seed: u64,
    max_depth: u32,
) -> Result<NodeActionEvaluation, SolverError> {
    if samples == 0 {
        return Err(SolverError::ZeroEvaluationSamples);
    }
    let num_players = game.num_players();

    // Replay `path` once, card-independently, recording each path node's
    // (history, actor, state) so every sample can re-derive that node's
    // world-dependent bucket without replaying the betting line again.
    let mut state = game.root_state();
    let mut history = HistoryKey::ROOT;
    let mut path_nodes: Vec<(HistoryKey, usize, G::State)> = Vec::with_capacity(path.len());
    for (depth, &action_index) in path.iter().enumerate() {
        let Some(actor) = game.actor(&state) else {
            return Err(SolverError::EvaluationPathTerminalEarly { depth });
        };
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = game.node_actions(&state);
        let num_actions = game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        if action_index >= num_actions {
            return Err(SolverError::EvaluationPathIndexOutOfRange {
                depth,
                index: action_index,
                actions: num_actions,
            });
        }
        path_nodes.push((history, actor, state.clone()));
        let next_state = game.next_state_with(&state, &actions, action_index);
        history = history.child(actor, action_index);
        state = next_state;
    }
    let Some(hero) = game.actor(&state) else {
        return Err(SolverError::EvaluationPathTerminalEarly { depth: path.len() });
    };
    if hero >= num_players {
        return Err(SolverError::InvalidActor {
            actor: hero,
            num_players,
        });
    }
    let target_history = history;
    let target_state = state;
    let target_actions = game.node_actions(&target_state);
    let num_target_actions = game.num_actions_of(&target_actions);
    if num_target_actions == 0 {
        return Err(SolverError::NoActions { actor: hero });
    }
    let action_labels: Vec<String> = (0..num_target_actions)
        .map(|index| game.action_label_of(&target_actions, index))
        .collect();

    let mut groups: FxHashMap<BucketId, NodeEvalGroup> = FxHashMap::default();
    let mut aggregate_cells = vec![NodeEvalCell::default(); num_target_actions];
    let mut aggregate_frequency_num = vec![0.0f64; num_target_actions];
    let mut aggregate_frequency_den = 0.0f64;
    let mut total_weight = 0.0f64;
    let mut total_deal_attempts = 0u64;

    for sample_id in 0..samples {
        let mut deal_rng = node_eval_deal_rng(seed, sample_id);
        let sample = sampler.sample_counted(&mut deal_rng)?;
        total_deal_attempts = total_deal_attempts
            .checked_add(u64::from(sample.attempts))
            .ok_or(SolverError::CounterOverflow)?;
        let world = sample.world;

        let mut weight = 1.0f64;
        for (&action_index, (node_history, node_actor, node_state)) in path.iter().zip(&path_nodes)
        {
            let node_actions = game.node_actions(node_state);
            let node_num_actions = game.num_actions_of(&node_actions);
            let private = game.bucket(node_state, &world, *node_actor);
            let key = InfoKey {
                history: *node_history,
                player: *node_actor as u8,
                street: private.street,
                active_opponents: private.active_opponents,
                bucket_path: private.bucket_path,
            };
            let probabilities = strategy
                .lookup(key)
                .filter(|probabilities| probabilities.len() == node_num_actions)
                .unwrap_or_else(|| vec![1.0 / node_num_actions as f64; node_num_actions]);
            let probability = probabilities.get(action_index).copied().unwrap_or(0.0);
            if probability <= 0.0 {
                weight = 0.0;
                break;
            }
            weight *= probability;
        }
        if weight <= 0.0 {
            continue;
        }

        let hero_private = game.bucket(&target_state, &world, hero);
        let group_id = hero_private.current_bucket();
        let target_key = InfoKey {
            history: target_history,
            player: hero as u8,
            street: hero_private.street,
            active_opponents: hero_private.active_opponents,
            bucket_path: hero_private.bucket_path,
        };
        let target_strategy = strategy
            .lookup(target_key)
            .filter(|probabilities| probabilities.len() == num_target_actions)
            .unwrap_or_else(|| vec![1.0 / num_target_actions as f64; num_target_actions]);

        let mut action_values = Vec::with_capacity(num_target_actions);
        for action_index in 0..num_target_actions {
            let next_state = game.next_state_with(&target_state, &target_actions, action_index);
            let next_history = target_history.child(hero, action_index);
            let mut playout_rng = node_eval_playout_rng(seed, sample_id);
            let value = playout(
                game,
                strategy,
                next_state,
                next_history,
                &world,
                hero,
                &mut playout_rng,
                max_depth,
            )?;
            action_values.push(value);
        }

        let group = groups.entry(group_id).or_insert_with(|| NodeEvalGroup {
            weight: 0.0,
            samples: 0,
            frequencies: target_strategy.clone(),
            cells: vec![NodeEvalCell::default(); num_target_actions],
        });
        group.weight += weight;
        group.samples += 1;
        total_weight += weight;
        for (action_index, &value) in action_values.iter().enumerate() {
            group.cells[action_index].add(weight, value);
            aggregate_cells[action_index].add(weight, value);
            aggregate_frequency_num[action_index] += weight * target_strategy[action_index];
        }
        aggregate_frequency_den += weight;
    }

    let mut group_rows: Vec<NodeActionGroupEvaluation> = groups
        .into_iter()
        .map(|(group, accumulator)| NodeActionGroupEvaluation {
            group,
            weight_share: if total_weight > 0.0 {
                accumulator.weight / total_weight
            } else {
                0.0
            },
            frequencies: accumulator.frequencies,
            actions: accumulator
                .cells
                .iter()
                .map(NodeEvalCell::estimate)
                .collect(),
            samples: accumulator.samples,
        })
        .collect();
    group_rows.sort_unstable_by_key(|row| row.group);

    let aggregate = (0..num_target_actions)
        .map(|action_index| NodeActionAggregate {
            ev: aggregate_cells[action_index].estimate(),
            frequency: if aggregate_frequency_den > 0.0 {
                aggregate_frequency_num[action_index] / aggregate_frequency_den
            } else {
                1.0 / num_target_actions as f64
            },
        })
        .collect();

    Ok(NodeActionEvaluation {
        actor: hero,
        action_labels,
        samples,
        total_deal_attempts,
        groups: group_rows,
        aggregate,
    })
}

/// Plays `state` (already past the evaluated node's chosen action) forward
/// to a terminal, sampling every decision -- including the seat-of-interest's
/// own later decisions -- from `strategy`'s average profile (uniform on a
/// lookup miss), and returns `hero`'s terminal utility.
#[allow(clippy::too_many_arguments)]
fn playout<G: ExternalSamplingGame>(
    game: &G,
    strategy: &impl AverageStrategyLookup,
    mut state: G::State,
    mut history: HistoryKey,
    world: &SampledWorld,
    hero: usize,
    rng: &mut ChaCha20Rng,
    max_depth: u32,
) -> Result<f64, SolverError> {
    let num_players = game.num_players();
    for depth in 0..=max_depth {
        let Some(actor) = game.actor(&state) else {
            let mut utilities = vec![0.0; num_players];
            game.terminal_utilities(&state, world, &mut utilities);
            if let Some((seat, &utility)) = utilities
                .iter()
                .enumerate()
                .find(|(_, utility)| !utility.is_finite())
            {
                return Err(SolverError::NonFiniteUtility { seat, utility });
            }
            return Ok(utilities[hero]);
        };
        if actor >= num_players {
            return Err(SolverError::InvalidActor { actor, num_players });
        }
        let actions = game.node_actions(&state);
        let num_actions = game.num_actions_of(&actions);
        if num_actions == 0 {
            return Err(SolverError::NoActions { actor });
        }
        let private = game.bucket(&state, world, actor);
        let key = InfoKey {
            history,
            player: actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        let probabilities = strategy
            .lookup(key)
            .filter(|probabilities| probabilities.len() == num_actions)
            .unwrap_or_else(|| vec![1.0 / num_actions as f64; num_actions]);
        let action = sample_profile_action_f64(&probabilities, rng);
        history = history.child(actor, action);
        state = game.next_state_with(&state, &actions, action);
        if depth == max_depth {
            return Err(SolverError::DepthLimit { limit: max_depth });
        }
    }
    unreachable!("depth loop returns at its upper bound")
}

fn sample_profile_action_f64(strategy: &[f64], rng: &mut ChaCha20Rng) -> usize {
    let needle = rng.gen_range(0.0..1.0);
    let mut cumulative = 0.0;
    for (action, &probability) in strategy.iter().enumerate() {
        cumulative += probability;
        if needle < cumulative {
            return action;
        }
    }
    strategy.len() - 1
}

fn node_eval_deal_rng(seed: u64, sample_id: u64) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.node-eval-deal.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

/// Derived fresh (never mutated across an evaluated node's actions) so every
/// action at one sample gets the exact same playout RNG stream -- common
/// random numbers, reducing action-to-action EV variance.
fn node_eval_playout_rng(seed: u64, sample_id: u64) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.node-eval-playout.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
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
