//! Read-only importance-weighted diagnostics inside rare public branches.
use super::eval::validate_purify_threshold;
use super::support::*;
use super::*;

/// A ratio-of-means estimate with a first-order, trajectory-clustered delta
/// standard error. Finite-sample unbiasedness and reliable low-ESS intervals
/// are not claimed. Zero denominator is represented by None at the call site.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeightedEstimate {
    pub mean: f64,
    pub stderr: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConditionalProfileEvaluation {
    pub samples: u64,
    pub seed: u64,
    /// Physical worlds are shared across prefixes and counted only once.
    pub total_deal_attempts: u64,
    pub prefixes: Vec<ConditionalPrefixEvaluation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConditionalPrefixEvaluation {
    pub history: HistoryKey,
    pub action_indices: Vec<usize>,
    pub positive_weight_samples: u64,
    pub reach_probability: WeightedEstimate,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConditionalStreetCoverage {
    /// Unweighted counts exclude zero-reach worlds; they are not ordinary
    /// baseline reaches and must not be used as a binomial sample size.
    pub positive_weight_decision_visits: u64,
    pub positive_weight_trajectory_visits: u64,
    pub trajectory_probability: Option<WeightedEstimate>,
    pub decisions_per_prefix_trajectory: Option<WeightedEstimate>,
    /// Concentration of (prefix reach * number of decisions) per trajectory.
    pub decision_weight_effective_sample_size: f64,
    pub average_fraction: Option<WeightedEstimate>,
    pub current_fraction: Option<WeightedEstimate>,
    pub regret_fallback_fraction: Option<WeightedEstimate>,
    pub uniform_fallback_fraction: Option<WeightedEstimate>,
}

pub(super) struct ForcedPrefixReplay<'a> {
    pub actions: &'a [usize],
    /// Change only the first decision after `actions`, without weighting it.
    pub endpoint_action: Option<usize>,
    pub weight: f64,
    pub skip_weight_actions: usize,
    pub sources: [bool; 3],
}

/// The existing sampler clips cumulative f32 probabilities at one and sends
/// residual mass to the last action. Mirror those intervals (including f32
/// normalization roundoff), rather than changing the baseline distribution.
pub(super) fn profile_action_probability(strategy: &[f32], action: usize) -> f64 {
    let before = strategy[..action]
        .iter()
        .map(|&p| f64::from(p))
        .sum::<f64>()
        .min(1.0);
    let after = if action + 1 == strategy.len() {
        1.0
    } else {
        (before + f64::from(strategy[action])).min(1.0)
    };
    after - before
}

// Joint Welford moments keep numerator/denominator covariance and avoid
// subtracting two large raw second moments. One observation is one world and
// one sampled continuation, not one visited decision.
#[derive(Clone, Copy, Default)]
pub(super) struct RatioMoments {
    n: u64,
    x: f64,
    y: f64,
    xx: f64,
    yy: f64,
    xy: f64,
    // A separately centered numerator is a fallback when covariance
    // subtraction spuriously goes negative for nearly proportional x/y.
    anchor: f64,
    z: f64,
    zz: f64,
    zy: f64,
}

impl RatioMoments {
    pub(super) fn observe(&mut self, x: f64, y: f64) {
        if self.n == 0 && y != 0.0 {
            self.anchor = x / y;
        }
        let z = x - self.anchor * y;
        self.n += 1;
        let dx = x - self.x;
        let dy = y - self.y;
        let dz = z - self.z;
        self.x += dx / self.n as f64;
        self.y += dy / self.n as f64;
        self.z += dz / self.n as f64;
        self.xx += dx * (x - self.x);
        self.yy += dy * (y - self.y);
        self.xy += dx * (y - self.y);
        self.zz += dz * (z - self.z);
        self.zy += dz * (y - self.y);
    }

    pub(super) fn ratio(self) -> Result<Option<WeightedEstimate>, SolverError> {
        if self.y == 0.0 {
            return Ok(None);
        }
        let mut mean = self.x / self.y;
        let terms = [self.xx, -2.0 * mean * self.xy, mean * mean * self.yy];
        let mut variance = terms.iter().sum::<f64>();
        let scale = terms.iter().map(|v| v.abs()).sum::<f64>();
        if !variance.is_finite() || !scale.is_finite() {
            return Err(SolverError::NumericOverflow);
        }
        if variance < -128.0 * f64::EPSILON * scale {
            // x = anchor*y + z leaves the exact residual variance invariant.
            // For constant gains and almost equal weights, z is near zero
            // and avoids subtracting three inaccurate, almost equal moments.
            // Keep the historical arithmetic for every previously valid
            // estimate; this branch replaces only a numeric-error result.
            let delta = self.z / self.y;
            mean = self.anchor + delta;
            let terms = [self.zz, -2.0 * delta * self.zy, delta * delta * self.yy];
            variance = terms.iter().sum::<f64>();
            let scale = terms.iter().map(|v| v.abs()).sum::<f64>();
            if !variance.is_finite()
                || !scale.is_finite()
                || variance < -128.0 * f64::EPSILON * scale
            {
                return Err(SolverError::NumericOverflow);
            }
        }
        let stderr = (variance.max(0.0) / (self.n - 1) as f64 / self.n as f64).sqrt() / self.y;
        if !mean.is_finite() || !stderr.is_finite() {
            return Err(SolverError::NumericOverflow);
        }
        Ok(Some(WeightedEstimate { mean, stderr }))
    }

    pub(super) fn denominator_ess(self) -> f64 {
        if self.y == 0.0 {
            return 0.0;
        }
        let sum = self.n as f64 * self.y;
        let squares = self.yy + self.n as f64 * self.y * self.y;
        (sum / squares.sqrt()).powi(2).clamp(0.0, self.n as f64)
    }

    pub(super) fn numerator_ess(self) -> f64 {
        Self {
            y: self.x,
            yy: self.xx,
            ..self
        }
        .denominator_ess()
    }
}

#[derive(Default)]
pub(super) struct StreetMoments {
    visits: u64,
    trajectories: u64,
    trajectory: RatioMoments,
    decisions: RatioMoments,
    sources: [RatioMoments; 4],
}

impl StreetMoments {
    pub(super) fn observe(
        &mut self,
        weight: f64,
        coverage: CandidatePolicyCoverage,
        street: Street,
    ) -> Result<(), SolverError> {
        let decisions = coverage.decision_visits_by_street.get(street);
        let trajectory = u64::from(decisions > 0);
        if weight > 0.0 {
            self.visits = self
                .visits
                .checked_add(decisions)
                .ok_or(SolverError::CounterOverflow)?;
            self.trajectories = self
                .trajectories
                .checked_add(trajectory)
                .ok_or(SolverError::CounterOverflow)?;
        }
        self.trajectory.observe(weight * trajectory as f64, weight);
        self.decisions.observe(weight * decisions as f64, weight);
        for (moments, count) in self.sources.iter_mut().zip([
            coverage.average_strategy_visits_by_street.get(street),
            coverage.current_strategy_visits_by_street.get(street),
            coverage.regret_fallback_visits_by_street.get(street),
            coverage.uniform_fallback_visits_by_street.get(street),
        ]) {
            moments.observe(weight * count as f64, weight * decisions as f64);
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<ConditionalStreetCoverage, SolverError> {
        Ok(ConditionalStreetCoverage {
            positive_weight_decision_visits: self.visits,
            positive_weight_trajectory_visits: self.trajectories,
            trajectory_probability: self.trajectory.ratio()?,
            decisions_per_prefix_trajectory: self.decisions.ratio()?,
            decision_weight_effective_sample_size: self.sources[0].denominator_ess(),
            average_fraction: self.sources[0].ratio()?,
            current_fraction: self.sources[1].ratio()?,
            regret_fallback_fraction: self.sources[2].ratio()?,
            uniform_fallback_fraction: self.sources[3].ratio()?,
        })
    }
}

struct PrefixMoments {
    history: HistoryKey,
    positive: u64,
    max_weight: f64,
    reach: RatioMoments,
    prefix_sources: [RatioMoments; 3],
    seats: Vec<RatioMoments>,
    streets: [StreetMoments; 4],
    seat_streets: Vec<[StreetMoments; 4]>,
}

struct ConditionalSample {
    attempts: u32,
    prefixes: Vec<ConditionalPrefixSample>,
}

struct ConditionalPrefixSample {
    weight: f64,
    utilities: Vec<f64>,
    coverage: Vec<CandidatePolicyCoverage>,
    prefix_sources: [bool; 3],
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    /// Force each action-index prefix on held-out physical worlds and sample
    /// baseline continuations. Weight by the product of baseline action
    /// probabilities on the forced path. Ratios target the conditional
    /// baseline distribution; they do not measure unconditional root coverage
    /// or a best response. No training state or ordinary RNG stream changes.
    ///
    /// Accepts 1..=64 unique, legal, nonterminal prefixes, including an empty
    /// root prefix, and at least two worlds. Invalid paths fail before sampling.
    /// Same worlds/action substreams are reused for each prefix, independent
    /// of list order. Indexed chunks preserve bit-identical thread results and
    /// bound sample-result storage near 8 MiB, independently of sample count.
    /// Positive reach whose squared weight underflows fails explicitly.
    pub fn evaluate_profile_conditioned(
        &self,
        samples: u64,
        seed: u64,
        variant: ProfileVariant,
        threads: usize,
        prefixes: &[Vec<usize>],
    ) -> Result<ConditionalProfileEvaluation, SolverError> {
        self.evaluate_conditioned_core(samples, seed, variant, threads, prefixes, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn evaluate_conditioned_core(
        &self,
        samples: u64,
        seed: u64,
        variant: ProfileVariant,
        threads: usize,
        prefixes: &[Vec<usize>],
        proposal: Option<&super::preflop_proposal::PreparedPreflopProposal>,
    ) -> Result<ConditionalProfileEvaluation, SolverError> {
        validate_purify_threshold(variant.purify_threshold)?;
        if samples < 2 {
            return Err(SolverError::InvalidState(
                "conditional evaluation requires at least two samples",
            ));
        }
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        if prefixes.is_empty() || prefixes.len() > 64 {
            return Err(SolverError::InvalidState(
                "conditional evaluation requires 1..=64 prefixes",
            ));
        }
        let players = self.game.num_players();
        let mut moments = Vec::with_capacity(prefixes.len());
        for (index, actions) in prefixes.iter().enumerate() {
            if prefixes[..index].contains(actions) {
                return Err(SolverError::InvalidState(
                    "conditional prefixes must be unique",
                ));
            }
            let mut state = self.game.root_state();
            let mut history = HistoryKey::ROOT;
            if actions.len() >= self.config.max_traversal_depth as usize {
                return Err(SolverError::DepthLimit {
                    limit: self.config.max_traversal_depth,
                });
            }
            for &action in actions {
                let actor = self.game.actor(&state).ok_or(SolverError::InvalidState(
                    "conditional prefix passes a terminal state",
                ))?;
                if actor >= players {
                    return Err(SolverError::InvalidActor {
                        actor,
                        num_players: players,
                    });
                }
                let menu = self.game.node_actions(&state);
                if action >= self.game.num_actions_of(&menu) {
                    return Err(SolverError::InvalidState(
                        "conditional prefix action is out of bounds",
                    ));
                }
                state = self.game.next_state_with(&state, &menu, action);
                history = history.child(actor, action);
            }
            let endpoint_actor = self.game.actor(&state);
            if endpoint_actor.is_none() {
                return Err(SolverError::InvalidState(
                    "conditional prefix must end at a decision",
                ));
            }
            if let Some(actor) = endpoint_actor
                && actor >= players
            {
                return Err(SolverError::InvalidActor {
                    actor,
                    num_players: players,
                });
            }
            moments.push(PrefixMoments {
                history,
                positive: 0,
                max_weight: 0.0,
                reach: RatioMoments::default(),
                prefix_sources: Default::default(),
                seats: vec![RatioMoments::default(); players],
                streets: Default::default(),
                seat_streets: (0..players).map(|_| Default::default()).collect(),
            });
        }
        let sample_bytes = size_of::<ConditionalSample>()
            + prefixes.len()
                * (size_of::<ConditionalPrefixSample>()
                    + players * (size_of::<f64>() + size_of::<CandidatePolicyCoverage>()));
        let chunk_size = (8 * 1024 * 1024 / sample_bytes).clamp(1, 4096) as u64;
        let pool = (threads > 1)
            .then(|| rayon::ThreadPoolBuilder::new().num_threads(threads).build())
            .transpose()
            .map_err(|e| SolverError::ThreadPoolBuild(e.to_string()))?;
        let mut total_deal_attempts = 0_u64;
        let mut start = 0;
        while start < samples {
            let end = start
                .saturating_add(if threads == 1 { 1 } else { chunk_size })
                .min(samples);
            let evaluate = |id| self.conditioned_sample(id, seed, variant, prefixes, proposal);
            let results = if let Some(pool) = &pool {
                pool.install(|| {
                    (0..(end - start) as usize)
                        .into_par_iter()
                        .map(|offset| evaluate(start + offset as u64))
                        .collect::<Vec<_>>()
                })
            } else {
                vec![evaluate(start)]
            };
            for result in results {
                let result = result?;
                total_deal_attempts = total_deal_attempts
                    .checked_add(u64::from(result.attempts))
                    .ok_or(SolverError::CounterOverflow)?;
                for (acc, item) in moments.iter_mut().zip(result.prefixes) {
                    let ConditionalPrefixSample {
                        weight,
                        utilities,
                        coverage,
                        prefix_sources,
                    } = item;
                    for (moment, flag) in acc.prefix_sources.iter_mut().zip(prefix_sources) {
                        moment.observe(weight * f64::from(flag), weight);
                    }
                    acc.positive += u64::from(weight > 0.0);
                    acc.max_weight = acc.max_weight.max(weight);
                    acc.reach.observe(weight, 1.0);
                    for (seat, utility) in acc.seats.iter_mut().zip(utilities) {
                        seat.observe(weight * utility, weight);
                    }
                    let mut total = CandidatePolicyCoverage::default();
                    for (seat, coverage) in acc.seat_streets.iter_mut().zip(coverage) {
                        total.checked_add_assign(coverage)?;
                        for (street, stat) in Street::ALL.into_iter().zip(seat) {
                            stat.observe(weight, coverage, street)?;
                        }
                    }
                    for (street, stat) in Street::ALL.into_iter().zip(&mut acc.streets) {
                        stat.observe(weight, total, street)?;
                    }
                }
            }
            start = end;
        }
        let finish_streets =
            |streets: [StreetMoments; 4]| -> Result<[ConditionalStreetCoverage; 4], SolverError> {
                let [a, b, c, d] = streets;
                Ok([a.finish()?, b.finish()?, c.finish()?, d.finish()?])
            };
        let prefixes = moments
            .into_iter()
            .zip(prefixes)
            .map(|(m, actions)| {
                // Reverse x/y to reuse the denominator-weight ESS formula.
                let weights = RatioMoments {
                    y: m.reach.x,
                    yy: m.reach.xx,
                    ..m.reach
                };
                Ok(ConditionalPrefixEvaluation {
                    history: m.history,
                    action_indices: actions.clone(),
                    positive_weight_samples: m.positive,
                    reach_probability: m.reach.ratio()?.expect("unit denominator"),
                    effective_sample_size: weights.denominator_ess(),
                    max_normalized_weight: if m.reach.x == 0.0 {
                        0.0
                    } else {
                        m.max_weight / (samples as f64 * m.reach.x)
                    },
                    prefix_current_fraction: m.prefix_sources[0].ratio()?,
                    prefix_regret_fallback_fraction: m.prefix_sources[1].ratio()?,
                    prefix_uniform_fallback_fraction: m.prefix_sources[2].ratio()?,
                    seats: m
                        .seats
                        .into_iter()
                        .map(RatioMoments::ratio)
                        .collect::<Result<_, _>>()?,
                    coverage_by_street: finish_streets(m.streets)?,
                    coverage_by_seat: m
                        .seat_streets
                        .into_iter()
                        .map(finish_streets)
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<_, SolverError>>()?;
        Ok(ConditionalProfileEvaluation {
            samples,
            seed,
            total_deal_attempts,
            prefixes,
        })
    }

    fn conditioned_sample(
        &self,
        id: u64,
        seed: u64,
        variant: ProfileVariant,
        prefixes: &[Vec<usize>],
        proposal: Option<&super::preflop_proposal::PreparedPreflopProposal>,
    ) -> Result<ConditionalSample, SolverError> {
        let sampler = proposal.map_or(&self.sampler, |p| &p.sampler);
        let sample = sampler.sample_counted(&mut evaluation_deal_rng(seed, id))?;
        let weight = proposal.map_or(Ok(1.0), |p| p.correction(&sample.world))?;
        let mut results = Vec::with_capacity(prefixes.len());
        for actions in prefixes {
            let mut forced = ForcedPrefixReplay {
                actions,
                endpoint_action: None,
                weight,
                skip_weight_actions: proposal.map_or(0, |p| p.actions.len()),
                sources: [false; 3],
            };
            let mut coverage = vec![CandidatePolicyCoverage::default(); self.game.num_players()];
            let utilities = self.evaluate_world(
                &sample.world,
                &mut evaluation_action_rng(seed, id, None),
                None,
                None,
                variant.purify_threshold,
                variant.use_current_strategy,
                Some(&mut coverage),
                &mut [],
                Some(&mut forced),
            )?;
            results.push(ConditionalPrefixSample {
                weight: forced.weight,
                utilities,
                coverage,
                prefix_sources: forced.sources,
            });
        }
        Ok(ConditionalSample {
            attempts: sample.attempts,
            prefixes: results,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_uses_paired_world_residuals_and_decision_weights() {
        let data = [
            (0.1, 3.0, 1.0),
            (0.4, 1.0, 1.0),
            (0.0, 8.0, 2.0),
            (0.2, 2.0, 0.0),
        ];
        let mut m = RatioMoments::default();
        for (w, visits, average) in data {
            m.observe(w * average, w * visits);
        }
        let denominator = data.iter().map(|(w, v, _)| w * v).sum::<f64>();
        let expected = data.iter().map(|(w, _, a)| w * a).sum::<f64>() / denominator;
        let residual = data
            .iter()
            .map(|(w, v, a)| (w * a - expected * w * v).powi(2))
            .sum::<f64>();
        let answer = m.ratio().unwrap().unwrap();
        assert!((answer.mean - expected).abs() < 1e-14);
        assert!((answer.stderr - (4.0 * residual / 3.0).sqrt() / denominator).abs() < 1e-14);
        assert!(
            (m.denominator_ess()
                - denominator.powi(2) / data.iter().map(|(w, v, _)| (w * v).powi(2)).sum::<f64>())
            .abs()
                < 1e-14
        );
        let mut zero = RatioMoments::default();
        zero.observe(0.0, 0.0);
        zero.observe(0.0, 0.0);
        assert_eq!(zero.ratio().unwrap(), None);
        assert_eq!(zero.denominator_ess(), 0.0);
    }

    #[test]
    fn constant_gain_with_nearly_equal_proposal_weights_has_zero_error() {
        // Reduced from the trained preflop endpoint integration test. Joint
        // Welford covariance subtraction alone reports a negative variance.
        for gain in [5.5, -5.5] {
            let mut moments = RatioMoments::default();
            for weight in [1.0, 0.999_999_971_578_907_9] {
                moments.observe(weight * gain, weight);
            }
            let estimate = moments.ratio().unwrap().unwrap();
            assert_eq!(estimate.mean, gain);
            assert_eq!(estimate.stderr, 0.0);
            assert!((moments.denominator_ess() - 2.0).abs() < 1e-14);
        }
    }

    #[test]
    fn ratio_rejects_nonfinite_data_after_residual_fallback_addition() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut moments = RatioMoments::default();
            moments.observe(value, 1.0);
            moments.observe(2.0, 1.0);
            assert!(matches!(moments.ratio(), Err(SolverError::NumericOverflow)));
        }
        let mut moments = RatioMoments::default();
        moments.observe(1e200, 1e200);
        moments.observe(2e200, 2e200);
        // Finite inputs can still overflow second moments. NaN variance
        // must not become a spurious zero through f64::max(0.0).
        assert!(matches!(moments.ratio(), Err(SolverError::NumericOverflow)));
    }

    #[test]
    fn forced_probability_matches_sampler_roundoff_intervals() {
        for strategy in [
            vec![1.0 / 3.0; 3],
            vec![0.1, 0.2, 0.7],
            vec![0.0, 1.0],
            vec![0.7, 0.4, 0.0],
        ] {
            let probabilities = (0..strategy.len())
                .map(|a| profile_action_probability(&strategy, a))
                .collect::<Vec<_>>();
            assert_eq!(probabilities.iter().sum::<f64>(), 1.0);
            assert!(probabilities.iter().all(|p| (0.0..=1.0).contains(p)));
        }
        assert_eq!(
            profile_action_probability(&[1.0 / 3.0; 3], 2),
            1.0 - 2.0 * f64::from(1.0_f32 / 3.0)
        );
    }
}
