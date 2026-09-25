//! Frozen, own-information one-step deviations at public decision endpoints.
use std::time::Instant;

use super::conditioned::{ForcedPrefixReplay, RatioMoments, StreetMoments};
use super::eval::validate_purify_threshold;
use super::preflop_proposal::PreparedPreflopProposal;
use super::support::{evaluation_action_rng, evaluation_deal_rng, validate_private_info};
use super::*;
use crate::{BettingState, HoldemGame, MultiwayAbstraction};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndpointDeviationConfig {
    pub fit_samples: u64,
    pub fit_seed: u64,
    pub held_out_samples: u64,
    pub held_out_seeds: Vec<u64>,
    pub min_fit_ess: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EndpointDeviationEvaluation {
    pub config: EndpointDeviationConfig,
    pub variant: ProfileVariant,
    pub configuration_fingerprint: [u8; 32],
    pub abstraction_fingerprint: [u8; 32],
    pub history: HistoryKey,
    pub action_indices: Vec<usize>,
    pub actor: u8,
    pub street: Street,
    pub active_opponents: u8,
    pub bucket_active_opponents: u8,
    pub action_labels: Vec<String>,
    pub expected_buckets: u32,
    pub proposal: PreflopProposalMetadata,
    pub fit_elapsed_secs: f64,
    pub fit: EndpointDeviationFit,
    pub held_out: Vec<EndpointDeviationHeldOut>,
}

/// A preflop-only diagnostic under chance and opponents' prefix reach.
/// The nested evaluation uses this target for every weight, fit and gain.
/// It is a self-normalized diagnostic, not an unbiased CFR update or root EV.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CounterfactualEndpointDeviationEvaluation {
    pub target: &'static str,
    pub proposal_kind: &'static str,
    pub excluded_actor: u8,
    /// All path states and the endpoint were checked against the 169 classes.
    pub validated_class_contexts: usize,
    /// The excluded own-prefix factor, in endpoint class-index order.
    /// It is constant within each class, including zero-reach classes.
    pub own_prefix_probability_by_bucket: Vec<f64>,
    pub evaluation: EndpointDeviationEvaluation,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EndpointDeviationFit {
    pub sampling: EndpointDeviationSampling,
    pub retained_buckets: u32,
    /// Every configured bucket, including unobserved or unsupported keys.
    pub rows: Vec<EndpointDeviationRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EndpointDeviationRow {
    pub key: InfoKey,
    pub baseline_source: String,
    pub positive_weight_samples: u64,
    /// Unnormalized proposal weight, not root reach or an update count.
    pub relative_weight_sum: f64,
    pub effective_sample_size: f64,
    pub max_normalized_weight: f64,
    /// Fit-only paired action gains. Fewer than two positive worlds is None.
    pub action_gains: Vec<Option<WeightedEstimate>>,
    /// None keeps the exact baseline policy, without any greedy fallback.
    pub selected_action: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EndpointDeviationHeldOut {
    pub elapsed_secs: f64,
    pub sampling: EndpointDeviationSampling,
    /// Signed held-out paired gain over ALL prefix weight, not retained keys.
    pub gain: Option<WeightedEstimate>,
    pub retained_key_weight_fraction: Option<WeightedEstimate>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EndpointDeviationSampling {
    pub seed: u64,
    pub samples: u64,
    pub total_deal_attempts: u64,
    /// Completed baseline/candidate terminal replays; zero-weight tails skip.
    pub terminal_replays: u64,
    pub positive_weight_samples: u64,
    /// Mean unnormalized proposal weight. This is NOT absolute root reach.
    pub relative_weight_mean: WeightedEstimate,
    pub effective_sample_size: f64,
    pub max_normalized_weight: f64,
    pub prefix_current_fraction: Option<WeightedEstimate>,
    pub prefix_regret_fallback_fraction: Option<WeightedEstimate>,
    pub prefix_uniform_fallback_fraction: Option<WeightedEstimate>,
    pub baseline_seats: Vec<Option<WeightedEstimate>>,
    /// Baseline suffix only; candidate replays never enter source coverage.
    pub coverage_by_street: [ConditionalStreetCoverage; 4],
    pub coverage_by_seat: Vec<[ConditionalStreetCoverage; 4]>,
}

struct Endpoint {
    state: BettingState,
    path: Vec<usize>,
    history: HistoryKey,
    actor: usize,
    context: DenseNodeContext,
    labels: Vec<String>,
    buckets: u32,
}

impl Endpoint {
    fn key(&self, bucket: BucketId) -> InfoKey {
        let private = PrivateInfo::from_current_bucket(
            self.context.street,
            self.context.active_opponents,
            bucket,
        );
        InfoKey {
            history: self.history,
            player: self.actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        }
    }
}

struct EndpointSample {
    bucket: BucketId,
    weight: f64,
    baseline: Vec<f64>,
    gains: [f64; 8],
    selected: bool,
    sources: [bool; 3],
    coverage: Vec<CandidatePolicyCoverage>,
    attempts: u32,
    terminal_replays: u64,
}

struct SamplingMoments {
    samples: u64,
    attempts: u64,
    terminal_replays: u64,
    positive: u64,
    max_weight: f64,
    reach: RatioMoments,
    sources: [RatioMoments; 3],
    seats: Vec<RatioMoments>,
    streets: [StreetMoments; 4],
    seat_streets: Vec<[StreetMoments; 4]>,
}

#[derive(Default)]
struct HeldOutMoments {
    gain: RatioMoments,
    retained: RatioMoments,
}

impl HeldOutMoments {
    fn observe(&mut self, sample: &EndpointSample) -> Result<(), SolverError> {
        // Unsupported keys have zero paired gain but retain their full weight.
        observe_ratio(
            &mut self.gain,
            sample.weight * sample.gains[0],
            sample.weight,
        )?;
        observe_ratio(
            &mut self.retained,
            sample.weight * f64::from(sample.selected),
            sample.weight,
        )
    }
}

fn observe_ratio(moment: &mut RatioMoments, x: f64, y: f64) -> Result<(), SolverError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(SolverError::NumericOverflow);
    }
    moment.observe(x, y);
    Ok(())
}

fn finish_streets(
    streets: [StreetMoments; 4],
) -> Result<[ConditionalStreetCoverage; 4], SolverError> {
    let [a, b, c, d] = streets;
    Ok([a.finish()?, b.finish()?, c.finish()?, d.finish()?])
}

impl SamplingMoments {
    fn new(players: usize) -> Self {
        Self {
            samples: 0,
            attempts: 0,
            terminal_replays: 0,
            positive: 0,
            max_weight: 0.0,
            reach: RatioMoments::default(),
            sources: Default::default(),
            seats: vec![RatioMoments::default(); players],
            streets: Default::default(),
            seat_streets: (0..players).map(|_| Default::default()).collect(),
        }
    }

    fn observe(&mut self, sample: &EndpointSample) -> Result<(), SolverError> {
        self.samples = self
            .samples
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        self.attempts = self
            .attempts
            .checked_add(u64::from(sample.attempts))
            .ok_or(SolverError::CounterOverflow)?;
        self.terminal_replays = self
            .terminal_replays
            .checked_add(sample.terminal_replays)
            .ok_or(SolverError::CounterOverflow)?;
        self.positive += u64::from(sample.weight > 0.0);
        self.max_weight = self.max_weight.max(sample.weight);
        observe_ratio(&mut self.reach, sample.weight, 1.0)?;
        for (moment, flag) in self.sources.iter_mut().zip(sample.sources) {
            observe_ratio(moment, sample.weight * f64::from(flag), sample.weight)?;
        }
        for (moment, utility) in self.seats.iter_mut().zip(&sample.baseline) {
            observe_ratio(moment, sample.weight * utility, sample.weight)?;
        }
        let mut total = CandidatePolicyCoverage::default();
        for (seat, &coverage) in self.seat_streets.iter_mut().zip(&sample.coverage) {
            total.checked_add_assign(coverage)?;
            for (street, stat) in Street::ALL.into_iter().zip(seat) {
                stat.observe(sample.weight, coverage, street)?;
            }
        }
        for (street, stat) in Street::ALL.into_iter().zip(&mut self.streets) {
            stat.observe(sample.weight, total, street)?;
        }
        Ok(())
    }

    fn finish(self, seed: u64) -> Result<EndpointDeviationSampling, SolverError> {
        let mean = self.reach.ratio()?.expect("unit denominator");
        let ess = self.reach.numerator_ess();
        let max_normalized_weight = if mean.mean > 0.0 {
            self.max_weight / (self.samples as f64 * mean.mean)
        } else {
            0.0
        };
        if !ess.is_finite() || !max_normalized_weight.is_finite() {
            return Err(SolverError::NumericOverflow);
        }
        Ok(EndpointDeviationSampling {
            seed,
            samples: self.samples,
            total_deal_attempts: self.attempts,
            terminal_replays: self.terminal_replays,
            positive_weight_samples: self.positive,
            relative_weight_mean: mean,
            effective_sample_size: ess,
            max_normalized_weight,
            prefix_current_fraction: self.sources[0].ratio()?,
            prefix_regret_fallback_fraction: self.sources[1].ratio()?,
            prefix_uniform_fallback_fraction: self.sources[2].ratio()?,
            baseline_seats: self
                .seats
                .into_iter()
                .map(RatioMoments::ratio)
                .collect::<Result<_, _>>()?,
            coverage_by_street: finish_streets(self.streets)?,
            coverage_by_seat: self
                .seat_streets
                .into_iter()
                .map(finish_streets)
                .collect::<Result<_, _>>()?,
        })
    }
}

#[derive(Default)]
struct FitBucket {
    positive: u64,
    sum_weight: f64,
    max_weight: f64,
    gains: [RatioMoments; 8],
}

impl FitBucket {
    fn observe(&mut self, weight: f64, gains: &[f64]) -> Result<(), SolverError> {
        if weight == 0.0 {
            return Ok(());
        }
        self.positive = self
            .positive
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        self.sum_weight += weight;
        self.max_weight = self.max_weight.max(weight);
        if !self.sum_weight.is_finite() {
            return Err(SolverError::NumericOverflow);
        }
        for (moment, gain) in self.gains.iter_mut().zip(gains) {
            observe_ratio(moment, weight * gain, weight)?;
        }
        Ok(())
    }

    fn finish(
        self,
        key: InfoKey,
        source: String,
        actions: usize,
        min_ess: f64,
    ) -> Result<EndpointDeviationRow, SolverError> {
        let ess = self.gains[0].denominator_ess();
        if !ess.is_finite() {
            return Err(SolverError::NumericOverflow);
        }
        let gains = self.gains[..actions]
            .iter()
            .map(|m| {
                if self.positive < 2 {
                    Ok(None)
                } else {
                    m.ratio()
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut selected_action = None;
        let mut best_gain = 0.0;
        if ess >= min_ess {
            for (action, gain) in gains.iter().enumerate() {
                if let Some(gain) = gain
                    && gain.mean > best_gain
                {
                    best_gain = gain.mean;
                    selected_action = Some(action as u16);
                }
            }
        }
        Ok(EndpointDeviationRow {
            key,
            baseline_source: source,
            positive_weight_samples: self.positive,
            relative_weight_sum: self.sum_weight,
            effective_sample_size: ess,
            max_normalized_weight: if self.sum_weight > 0.0 {
                self.max_weight / self.sum_weight
            } else {
                0.0
            },
            action_gains: gains,
            selected_action,
        })
    }
}

pub(super) fn validate_config(
    config: &EndpointDeviationConfig,
    threads: usize,
) -> Result<(), SolverError> {
    if threads == 0
        || config.fit_samples < 2
        || config.held_out_samples < 2
        || !config.min_fit_ess.is_finite()
        || config.min_fit_ess < 2.0
        || config.held_out_seeds.is_empty()
        || config.held_out_seeds.len() > 64
        || config
            .held_out_seeds
            .iter()
            .enumerate()
            .any(|(i, &seed)| seed == config.fit_seed || config.held_out_seeds[..i].contains(&seed))
    {
        return Err(SolverError::InvalidState(
            "endpoint deviation requires >=2 worlds, finite ESS>=2, positive threads and 1..=64 distinct held-out seeds separate from fit",
        ));
    }
    Ok(())
}

impl<A: MultiwayAbstraction> MultiwaySolver<HoldemGame<A>> {
    /// Fit one legal endpoint action per own-information key, freeze the table,
    /// then evaluate signed paired gains on separate worlds. Prefix policy and
    /// every later decision stay baseline. No solver state is changed.
    /// Root and preflop endpoints use their empty or partial preflop prefix;
    /// postflop endpoints use the complete preflop trunk. Proposal weights are
    /// relative weights, including at root, rather than absolute reach.
    pub fn evaluate_endpoint_deviation_preflop(
        &self,
        path: &[usize],
        variant: ProfileVariant,
        threads: usize,
        config: &EndpointDeviationConfig,
    ) -> Result<EndpointDeviationEvaluation, SolverError> {
        validate_config(config, threads)?;
        validate_purify_threshold(variant.purify_threshold)?;
        let (endpoint, trunk) = self.endpoint_definition(path)?;
        let proposal = self.prepare_preflop_proposal(trunk, variant)?;
        self.evaluate_endpoint_with_proposal(endpoint, proposal, variant, threads, config)
    }

    /// Independently fit and evaluate a one-step deviation using chance and
    /// opponents' prefix reach, excluding the endpoint actor's earlier actions.
    /// Only preflop decisions with verified 169-class mapping are supported.
    /// Zero-own-reach hands still reach the suffix. All fitted/held-out weights
    /// belong to this counterfactual target; compare neither their raw means
    /// nor their aggregate gains directly to an actual-prefix population.
    pub fn evaluate_endpoint_deviation_preflop_counterfactual(
        &self,
        path: &[usize],
        variant: ProfileVariant,
        threads: usize,
        config: &EndpointDeviationConfig,
    ) -> Result<CounterfactualEndpointDeviationEvaluation, SolverError> {
        validate_config(config, threads)?;
        validate_purify_threshold(variant.purify_threshold)?;
        let (endpoint, trunk) = self.endpoint_definition(path)?;
        if endpoint.context.street != Street::Preflop || trunk != path {
            return Err(SolverError::InvalidState(
                "counterfactual endpoint requires a preflop decision",
            ));
        }
        let actor = endpoint.actor;
        let prepared = self.prepare_counterfactual_preflop_proposal(trunk, variant, actor)?;
        let evaluation = self.evaluate_endpoint_with_proposal(
            endpoint,
            prepared.proposal,
            variant,
            threads,
            config,
        )?;
        Ok(CounterfactualEndpointDeviationEvaluation {
            target: "opponents-prefix",
            proposal_kind: "preflop-opponents-proposal",
            excluded_actor: actor as u8,
            validated_class_contexts: prepared.validated_class_contexts,
            own_prefix_probability_by_bucket: prepared.own_prefix_probability_by_bucket,
            evaluation,
        })
    }

    fn evaluate_endpoint_with_proposal(
        &self,
        endpoint: Endpoint,
        proposal: PreparedPreflopProposal,
        variant: ProfileVariant,
        threads: usize,
        config: &EndpointDeviationConfig,
    ) -> Result<EndpointDeviationEvaluation, SolverError> {
        let buckets = endpoint.buckets;
        let history = endpoint.history;
        let actor = endpoint.actor;
        let context = endpoint.context;
        let pool = (threads > 1)
            .then(|| rayon::ThreadPoolBuilder::new().num_threads(threads).build())
            .transpose()
            .map_err(|error| SolverError::ThreadPoolBuild(error.to_string()))?;
        let started = Instant::now();
        let mut sampling = SamplingMoments::new(self.game.num_players());
        let mut fit_buckets = (0..buckets)
            .map(|_| FitBucket::default())
            .collect::<Vec<_>>();
        self.endpoint_samples(
            &endpoint,
            &proposal,
            variant,
            config.fit_samples,
            config.fit_seed,
            None,
            pool.as_ref(),
            |sample| {
                sampling.observe(&sample)?;
                fit_buckets[sample.bucket as usize]
                    .observe(sample.weight, &sample.gains[..endpoint.labels.len()])
            },
        )?;
        let rows = fit_buckets
            .into_iter()
            .enumerate()
            .map(|(bucket, moments)| {
                let key = endpoint.key(bucket as BucketId);
                let source = match self.evaluation_policy(key) {
                    None => "uniform-fallback",
                    Some(policy) => {
                        if policy.action_labels != endpoint.labels {
                            return Err(SolverError::ActionLabelsChanged { key });
                        }
                        match policy.strategy(variant.use_current_strategy)?.1 {
                            CandidatePolicySource::Average => "average",
                            CandidatePolicySource::Current => "current",
                            CandidatePolicySource::RegretFallback => "regret-fallback",
                            CandidatePolicySource::UniformFallback => unreachable!(),
                        }
                    }
                };
                moments.finish(
                    key,
                    source.into(),
                    endpoint.labels.len(),
                    config.min_fit_ess,
                )
            })
            .collect::<Result<Vec<_>, SolverError>>()?;
        let selected = rows
            .iter()
            .map(|row| row.selected_action)
            .collect::<Vec<_>>();
        let fit = EndpointDeviationFit {
            sampling: sampling.finish(config.fit_seed)?,
            retained_buckets: selected.iter().filter(|value| value.is_some()).count() as u32,
            rows,
        };
        let fit_elapsed_secs = started.elapsed().as_secs_f64();
        let mut held_out = Vec::with_capacity(config.held_out_seeds.len());
        for &seed in &config.held_out_seeds {
            let started = Instant::now();
            let mut sampling = SamplingMoments::new(self.game.num_players());
            let mut moments = HeldOutMoments::default();
            self.endpoint_samples(
                &endpoint,
                &proposal,
                variant,
                config.held_out_samples,
                seed,
                Some(&selected),
                pool.as_ref(),
                |sample| {
                    sampling.observe(&sample)?;
                    moments.observe(&sample)
                },
            )?;
            let sampling = sampling.finish(seed)?;
            let gain = moments.gain.ratio()?;
            let retained_key_weight_fraction = moments.retained.ratio()?;
            held_out.push(EndpointDeviationHeldOut {
                elapsed_secs: started.elapsed().as_secs_f64(),
                sampling,
                gain,
                retained_key_weight_fraction,
            });
        }
        Ok(EndpointDeviationEvaluation {
            config: config.clone(),
            variant,
            configuration_fingerprint: self.configuration_fingerprint(),
            abstraction_fingerprint: self.abstraction_fingerprint(),
            history,
            action_indices: endpoint.path,
            actor: actor as u8,
            street: context.street,
            active_opponents: context.active_opponents,
            bucket_active_opponents: context.bucket_active_opponents,
            action_labels: endpoint.labels,
            expected_buckets: buckets,
            proposal: proposal.metadata,
            fit_elapsed_secs,
            fit,
            held_out,
        })
    }

    #[cfg(feature = "research-average-sampling")]
    pub(super) fn validate_research_endpoint(
        &self,
        path: &[usize],
        config: &EndpointDeviationConfig,
        threads: usize,
    ) -> Result<(), SolverError> {
        validate_config(config, threads)?;
        self.endpoint_definition(path).map(|_| ())
    }

    fn endpoint_definition(&self, path: &[usize]) -> Result<(Endpoint, Vec<usize>), SolverError> {
        if self.game.recall_mode() != RecallMode::Street {
            return Err(SolverError::InvalidState(
                "endpoint deviation requires current-street recall",
            ));
        }
        let trunk = self.endpoint_preflop_trunk(path)?;
        let mut state = self.game.root_state();
        let mut history = HistoryKey::ROOT;
        for &action in path {
            let actor = self
                .game
                .actor(&state)
                .ok_or(SolverError::InvalidState("terminal endpoint prefix"))?;
            let actions = self.game.node_actions(&state);
            state = self.game.next_state_with(&state, &actions, action);
            history = history.child(actor, action);
        }
        let actor = self
            .game
            .actor(&state)
            .ok_or(SolverError::InvalidState("terminal endpoint"))?;
        if actor >= self.game.num_players() {
            return Err(SolverError::InvalidActor {
                actor,
                num_players: self.game.num_players(),
            });
        }
        let actions = self.game.node_actions(&state);
        let labels = (0..self.game.num_actions_of(&actions))
            .map(|a| self.game.action_label_of(&actions, a))
            .collect::<Vec<_>>();
        if !(1..=8).contains(&labels.len()) {
            return Err(SolverError::InvalidState(
                "endpoint deviation supports 1..=8 legal endpoint actions",
            ));
        }
        support::validate_action_labels(&labels)?;
        let context = self.game.dense_node_context(&state);
        let buckets = self
            .game
            .bucket_count(context.street, context.bucket_active_opponents);
        if buckets == 0 {
            return Err(SolverError::InvalidState(
                "endpoint has no configured buckets",
            ));
        }
        let endpoint = Endpoint {
            state,
            path: path.to_vec(),
            history,
            actor,
            context,
            labels,
            buckets,
        };
        Ok((endpoint, trunk))
    }

    #[allow(clippy::too_many_arguments)]
    fn endpoint_samples(
        &self,
        endpoint: &Endpoint,
        proposal: &PreparedPreflopProposal,
        variant: ProfileVariant,
        samples: u64,
        seed: u64,
        selected: Option<&[Option<u16>]>,
        pool: Option<&rayon::ThreadPool>,
        mut observe: impl FnMut(EndpointSample) -> Result<(), SolverError>,
    ) -> Result<(), SolverError> {
        let bytes = size_of::<EndpointSample>()
            + self.game.num_players() * (size_of::<f64>() + size_of::<CandidatePolicyCoverage>());
        let chunk = (8 * 1024 * 1024 / bytes).clamp(1, 4096) as u64;
        let mut start = 0;
        while start < samples {
            let end = start
                .saturating_add(if pool.is_some() { chunk } else { 1 })
                .min(samples);
            let evaluate = |offset| {
                self.endpoint_sample(
                    endpoint,
                    proposal,
                    variant,
                    seed,
                    start + offset as u64,
                    selected,
                )
            };
            let results = if let Some(pool) = pool {
                pool.install(|| {
                    (0..(end - start) as usize)
                        .into_par_iter()
                        .map(evaluate)
                        .collect::<Vec<_>>()
                })
            } else {
                vec![evaluate(0)]
            };
            for result in results {
                observe(result?)?;
            }
            start = end;
        }
        Ok(())
    }

    fn endpoint_sample(
        &self,
        endpoint: &Endpoint,
        proposal: &PreparedPreflopProposal,
        variant: ProfileVariant,
        seed: u64,
        id: u64,
        selected: Option<&[Option<u16>]>,
    ) -> Result<EndpointSample, SolverError> {
        let sample = proposal
            .sampler
            .sample_counted(&mut evaluation_deal_rng(seed, id))?;
        let private = self
            .game
            .bucket(&endpoint.state, &sample.world, endpoint.actor);
        validate_private_info(private, self.game.num_players(), RecallMode::Street)?;
        let bucket = private.current_bucket();
        if bucket >= endpoint.buckets
            || private.street != endpoint.context.street.index() as u8
            || private.active_opponents != endpoint.context.active_opponents
        {
            return Err(SolverError::InvalidState(
                "endpoint private/public context mismatch",
            ));
        }
        let weight = proposal.correction(&sample.world)?;
        let replay = |action| ForcedPrefixReplay {
            actions: &endpoint.path,
            endpoint_action: action,
            weight,
            skip_weight_actions: proposal.actions.len(),
            sources: [false; 3],
        };
        let mut forced = replay(None);
        let mut coverage = vec![CandidatePolicyCoverage::default(); self.game.num_players()];
        let baseline = self.evaluate_world(
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
        let mut result = EndpointSample {
            bucket,
            weight: forced.weight,
            baseline,
            gains: [0.0; 8],
            selected: false,
            sources: forced.sources,
            coverage,
            attempts: sample.attempts,
            terminal_replays: u64::from(forced.weight > 0.0),
        };
        if result.weight == 0.0 {
            return Ok(result);
        }
        let actions = match selected {
            None => (0..endpoint.labels.len()).collect::<Vec<_>>(),
            Some(table) => table[bucket as usize]
                .map(|a| vec![a as usize])
                .unwrap_or_default(),
        };
        result.selected = selected.is_some() && !actions.is_empty();
        for action in actions {
            let mut candidate_prefix = replay(Some(action));
            let utility = self.evaluate_world(
                &sample.world,
                &mut evaluation_action_rng(seed, id, None),
                None,
                None,
                variant.purify_threshold,
                variant.use_current_strategy,
                None,
                &mut [],
                Some(&mut candidate_prefix),
            )?;
            if candidate_prefix.weight.to_bits() != result.weight.to_bits()
                || candidate_prefix.sources != result.sources
            {
                return Err(SolverError::InvalidState(
                    "endpoint deviation changed baseline prefix weight or source",
                ));
            }
            let gain = utility[endpoint.actor] - result.baseline[endpoint.actor];
            if !gain.is_finite() {
                return Err(SolverError::NumericOverflow);
            }
            result.gains[if selected.is_some() { 0 } else { action }] = gain;
            result.terminal_replays += 1;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    type TestGame = HoldemGame<crate::abstraction::FeatureHashAbstraction>;

    fn check_through(game: &TestGame, street: Street) -> (Vec<usize>, BettingState) {
        let mut state = game.root_state();
        let mut path = Vec::new();
        while state.street != street {
            assert!(path.len() < 20);
            let actions = game.node_actions(&state);
            let action = (0..game.num_actions_of(&actions))
                .find(|&a| {
                    let label = game.action_label_of(&actions, a);
                    label.starts_with("call:") || label == "check"
                })
                .unwrap();
            state = game.next_state_with(&state, &actions, action);
            path.push(action);
        }
        assert!(game.actor(&state).is_some());
        (path, state)
    }

    fn fixture(dense: bool) -> MultiwaySolver<TestGame> {
        let (game, sampler, config) = super::super::tests::initialization_holdem_fixture();
        let mut solver = if dense {
            MultiwaySolver::new_preallocated_with_threads(game, sampler, config, 2).unwrap()
        } else {
            MultiwaySolver::new(game, sampler, config).unwrap()
        };
        solver.run_sweeps(64).unwrap();
        solver
    }

    pub(super) fn config() -> EndpointDeviationConfig {
        EndpointDeviationConfig {
            fit_samples: 128,
            fit_seed: 908,
            held_out_samples: 129,
            held_out_seeds: vec![909, 910],
            min_fit_ess: 2.0,
        }
    }

    pub(super) fn preflop_game(deep: bool) -> TestGame {
        use crate::abstraction::{FeatureHashAbstraction, FeatureHashParams};
        use crate::config::{
            AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, MultiwayConfig, RakeConfig,
            SeatConfig, SizeSpec, UtilityConfig,
        };
        let mut betting = BettingConfig::default();
        betting.preflop.max_aggressive_actions = if deep { 5 } else { 1 };
        let sizes = if deep {
            vec![SizeSpec::PreviousBetMultiple { factor: 2.0 }]
        } else {
            vec![]
        };
        betting.preflop.bet_sizes = sizes.clone();
        betting.preflop.raise_sizes = sizes;
        for street in [&mut betting.flop, &mut betting.turn, &mut betting.river] {
            street.bet_sizes.clear();
            street.raise_sizes.clear();
            street.include_allin = false;
        }
        let game = MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: if deep { 100.0 } else { 4.0 },
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: crate::SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting,
            forced_bets: None,
            abstraction: AbstractionConfig {
                recall: RecallMode::Street,
                ..AbstractionConfig::default()
            },
        };
        HoldemGame::new(
            &game,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            FeatureHashAbstraction::new(FeatureHashParams {
                flop_buckets: 4,
                turn_buckets: 4,
                river_buckets: 4,
            })
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn endpoint_deviation_preflop_root_partial_and_five_bet_paths_are_read_only_and_deterministic()
    {
        let game = preflop_game(true);
        let sampler = game.deal_sampler().unwrap();
        let solver = MultiwaySolver::new(
            game,
            sampler,
            SolverConfig {
                max_memory_bytes: 1 << 27,
                max_traversal_depth: 64,
                ..SolverConfig::default()
            },
        )
        .unwrap();
        let before = solver.snapshot_state();
        let mut path = Vec::new();
        let mut state = solver.game().root_state();
        let mut paths = vec![path.clone()];
        let menu = solver.game().node_actions(&state);
        let fold = (0..solver.game().num_actions_of(&menu))
            .find(|&a| solver.game().action_label_of(&menu, a) == "fold")
            .unwrap();
        state = solver.game().next_state_with(&state, &menu, fold);
        path.push(fold);
        paths.push(path.clone());
        // A 2bet, 3bet, 4bet, then 5bet: all are preflop decision endpoints,
        // Current-street bucket and information-key counts update after the
        // fold; the folded seat's cards still block the physical deal.
        for _ in 0..4 {
            let menu = solver.game().node_actions(&state);
            let raise = (0..solver.game().num_actions_of(&menu))
                .find(|&a| {
                    let label = solver.game().action_label_of(&menu, a);
                    label.starts_with("raise-to:") && !label.ends_with(":all-in")
                })
                .unwrap();
            state = solver.game().next_state_with(&state, &menu, raise);
            path.push(raise);
            paths.push(path.clone());
        }
        assert_eq!(state.street, Street::Preflop);
        let mut config = config();
        config.fit_samples = 16;
        config.held_out_samples = 17;
        config.min_fit_ess = 1e6;
        for path in paths {
            assert_eq!(solver.endpoint_preflop_trunk(&path).unwrap(), path);
            assert!(solver.preflop_trunk(&path).is_err());
            assert!(
                solver
                    .evaluate_profile_conditioned_preflop(
                        2,
                        0,
                        ProfileVariant::default(),
                        1,
                        std::slice::from_ref(&path)
                    )
                    .is_err()
            );
            let one = without_timings(
                solver
                    .evaluate_endpoint_deviation_preflop(
                        &path,
                        ProfileVariant::default(),
                        1,
                        &config,
                    )
                    .unwrap(),
            );
            assert_eq!(one.street, Street::Preflop);
            assert_eq!(one.expected_buckets, 169);
            assert_eq!(one.fit.rows.len(), 169);
            assert_eq!(one.proposal.preflop_actions, path);
            assert_eq!(one.proposal.preflop_history, one.history);
            assert_eq!(
                one.bucket_active_opponents,
                if path.is_empty() { 2 } else { 1 }
            );
            assert_eq!(one.active_opponents, if path.is_empty() { 2 } else { 1 });
            assert_eq!(one.fit.retained_buckets, 0);
            for row in &one.fit.rows {
                assert_eq!(row.key.active_opponents, one.active_opponents);
                assert_eq!(&row.key.bucket_path[1..], &[UNREACHED_BUCKET; 3]);
            }
            for held in &one.held_out {
                assert_eq!(
                    held.sampling.positive_weight_samples,
                    config.held_out_samples
                );
                assert_eq!(held.gain.unwrap().mean, 0.0);
                assert_eq!(held.gain.unwrap().stderr, 0.0);
            }
            for threads in [2, 8] {
                assert_eq!(
                    one,
                    without_timings(
                        solver
                            .evaluate_endpoint_deviation_preflop(
                                &path,
                                ProfileVariant::default(),
                                threads,
                                &config
                            )
                            .unwrap()
                    )
                );
            }
        }
        assert_eq!(solver.snapshot_state(), before);
    }

    #[test]
    fn endpoint_deviation_preflop_small_support_action_and_folded_blocker_oracle() {
        use cards::{CardSet, Range, combo_cards, combo_index};
        let combo =
            |text: &str| combo_index(text[..2].parse().unwrap(), text[2..].parse().unwrap());
        let holes = [
            [combo("AsQs"), combo("KdQd")],
            [combo("AsAh"), combo("2c3c")],
            [combo("KsKh"), combo("4d5d")],
        ];
        // A controlled prior supplied to the real Holdem adapter. Folded seat
        // 0 can block the actor's aces; its forced fold probabilities differ.
        let priors = [[0.25, 0.75], [0.5, 0.5], [0.25, 0.75]];
        let ranges = holes
            .iter()
            .zip(priors)
            .map(|(hands, weights)| {
                let mut range = Range::default();
                for (&hand, weight) in hands.iter().zip(weights) {
                    range.set_weight(hand, weight);
                }
                range
            })
            .collect();
        let game = preflop_game(false);
        let mut solver = MultiwaySolver::new(
            game,
            DealSampler::new(ranges).unwrap(),
            SolverConfig {
                max_memory_bytes: 1 << 25,
                max_traversal_depth: 64,
                ..SolverConfig::default()
            },
        )
        .unwrap();
        let root = solver.game().root_state();
        assert_eq!(solver.game().actor(&root), Some(0));
        let menu = solver.game().node_actions(&root);
        let fold = (0..solver.game().num_actions_of(&menu))
            .find(|&a| solver.game().action_label_of(&menu, a) == "fold")
            .unwrap();
        let path = vec![fold];
        let board = ["7h", "9d", "Jh", "Qc", "2s"].map(|c| c.parse().unwrap());
        let example =
            SampledWorld::new(vec![holes[0][1], holes[1][0], holes[2][0]], board).unwrap();
        let root_bucket = solver.game().bucket(&root, &example, 0).current_bucket();
        let weak_world =
            SampledWorld::new(vec![holes[0][0], holes[1][1], holes[2][1]], board).unwrap();
        let weak_bb_bucket = solver.game().bucket(&root, &weak_world, 2).current_bucket();
        let dense = solver.dense.as_mut().unwrap();
        for (id, node) in dense.tree.nodes.iter().enumerate() {
            let labels = &node.action_labels;
            for bucket in 0..dense.arena.bucket_count_of(id as NodeId) {
                let range = dense.arena.slot_range(id as NodeId, bucket).unwrap();
                let mut policy = vec![0.0; labels.len()];
                if id == 0 {
                    let p = if bucket == root_bucket { 0.75 } else { 0.25 };
                    policy[fold] = p;
                    policy[labels.iter().position(|s| s.starts_with("call:")).unwrap()] = 1.0 - p;
                } else {
                    let chosen = if node.history == HistoryKey::ROOT.child(0, fold) {
                        labels.iter().position(|s| s == "fold")
                    } else {
                        labels
                            .iter()
                            .position(|s| s == "check")
                            .or_else(|| {
                                (node.street == Street::Preflop
                                    && node.actor == 2
                                    && bucket == weak_bb_bucket)
                                    .then(|| labels.iter().position(|s| s == "fold"))
                                    .flatten()
                            })
                            .or_else(|| labels.iter().position(|s| s.starts_with("call:")))
                            .or_else(|| labels.iter().position(|s| s == "fold"))
                    }
                    .unwrap();
                    policy[chosen] = 1.0;
                }
                dense.arena.strategy_sum[range].copy_from_slice(&policy);
                dense
                    .arena
                    .touched_set(dense.arena.column_id(id as NodeId, bucket).unwrap());
            }
        }
        let before = solver.snapshot_state();
        let (endpoint, trunk) = solver.endpoint_definition(&path).unwrap();
        assert_eq!(endpoint.actor, 1);
        assert_eq!(endpoint.context.active_opponents, 1);
        assert_eq!(endpoint.context.bucket_active_opponents, 1);
        assert_eq!(
            endpoint.labels,
            ["fold", "call:500", "raise-to:4000:all-in"]
        );
        let proposal = solver
            .prepare_preflop_proposal(trunk, ProfileVariant::default())
            .unwrap();
        let replay = |world: &SampledWorld, action: Option<usize>| {
            let mut forced = ForcedPrefixReplay {
                actions: &path,
                endpoint_action: action,
                weight: proposal.correction(world).unwrap(),
                skip_weight_actions: path.len(),
                sources: [false; 3],
            };
            let utility = solver
                .evaluate_world(
                    world,
                    &mut evaluation_action_rng(17, 0, None),
                    None,
                    None,
                    0.0,
                    false,
                    None,
                    &mut [],
                    Some(&mut forced),
                )
                .unwrap();
            (utility[1], forced.weight)
        };
        let mut moments = BTreeMap::<BucketId, FitBucket>::new();
        let mut expected = [[0.0; 3]; 2];
        let mut target_mass = [0.0; 2];
        let mut feasible = 0;
        for folded in 0..2 {
            for own in 0..2 {
                for opponent in 0..2 {
                    let hands = [holes[0][folded], holes[1][own], holes[2][opponent]];
                    let mut used = CardSet::EMPTY;
                    let mut valid = true;
                    for hand in hands {
                        let (a, b) = combo_cards(hand);
                        if used.contains(a) || used.contains(b) {
                            valid = false;
                        }
                        used.insert(a);
                        used.insert(b);
                    }
                    if !valid {
                        continue;
                    }
                    feasible += 1;
                    let world = SampledWorld::new(hands.to_vec(), board).unwrap();
                    let key = solver
                        .game()
                        .bucket(&endpoint.state, &world, 1)
                        .current_bucket();
                    let (baseline, weight) = replay(&world, None);
                    assert_eq!(baseline, -0.5);
                    // Independent chip arithmetic on this fixed board:
                    // AA beats KK; 23 loses to KK and beats 45. Calling risks
                    // 1bb, jamming risks 4bb; a BB fold awards its posted 1bb.
                    let gains = [
                        0.0,
                        if own == 1 && opponent == 0 { -0.5 } else { 1.5 },
                        if opponent == 1 {
                            1.5
                        } else if own == 0 {
                            4.5
                        } else {
                            -3.5
                        },
                    ];
                    for (action, &gain) in gains.iter().enumerate() {
                        let (utility, candidate_weight) = replay(&world, Some(action));
                        assert_eq!(candidate_weight.to_bits(), weight.to_bits());
                        assert_eq!(utility - baseline, gain);
                    }
                    let target = priors[0][folded] as f64
                        * priors[1][own] as f64
                        * priors[2][opponent] as f64
                        * [0.25, 0.75][folded];
                    let q = hands
                        .iter()
                        .enumerate()
                        .map(|(seat, &hand)| proposal.sampler.evaluation_combo_weight(seat, hand))
                        .product::<f64>();
                    let scale = proposal
                        .metadata
                        .target_scale_by_seat
                        .iter()
                        .product::<f64>();
                    assert!((q * weight * scale - target).abs() < 1e-14);
                    for _ in 0..4 {
                        moments
                            .entry(key)
                            .or_default()
                            .observe(q * weight, &gains)
                            .unwrap();
                    }
                    target_mass[own] += target;
                    for action in 0..3 {
                        expected[own][action] += target * gains[action];
                    }
                }
            }
        }
        assert_eq!(feasible, 6, "folded cards must still block the actor");
        assert!((target_mass[0] / target_mass.iter().sum::<f64>() - 9.0 / 19.0).abs() < 1e-14);
        let mut selected = BTreeMap::new();
        for own in 0..2 {
            let world =
                SampledWorld::new(vec![holes[0][1], holes[1][own], holes[2][0]], board).unwrap();
            let bucket = solver
                .game()
                .bucket(&endpoint.state, &world, 1)
                .current_bucket();
            let row = moments
                .remove(&bucket)
                .unwrap()
                .finish(endpoint.key(bucket), "average".into(), 3, 2.0)
                .unwrap();
            assert_eq!(row.selected_action, Some(if own == 0 { 2 } else { 1 }));
            for (gain, expected_gain) in row.action_gains.iter().zip(expected[own]) {
                assert!((gain.unwrap().mean - expected_gain / target_mass[own]).abs() < 1e-12);
            }
            selected.insert(bucket, row.selected_action.unwrap() as usize);
        }
        // Freeze AA's fit-selected jam, then a different board makes it lose.
        // Board/opponent information cannot revise a preflop own-key action.
        let losing_board = ["Kc", "7h", "9d", "Jh", "2s"].map(|c| c.parse().unwrap());
        let losing =
            SampledWorld::new(vec![holes[0][1], holes[1][0], holes[2][0]], losing_board).unwrap();
        let bucket = solver
            .game()
            .bucket(&endpoint.state, &losing, 1)
            .current_bucket();
        let (baseline, _) = replay(&losing, None);
        let (candidate, _) = replay(&losing, Some(selected[&bucket]));
        assert_eq!(candidate - baseline, -3.5);
        assert_eq!(solver.snapshot_state(), before);
    }

    pub(super) fn without_timings(
        mut value: EndpointDeviationEvaluation,
    ) -> EndpointDeviationEvaluation {
        value.fit_elapsed_secs = 0.0;
        for held in &mut value.held_out {
            held.elapsed_secs = 0.0;
        }
        value
    }

    fn assert_baseline_equal(
        actual: &EndpointDeviationSampling,
        expected: &PreflopConditionalProfileEvaluation,
    ) {
        assert_eq!(actual.seed, expected.seed);
        assert_eq!(actual.samples, expected.samples);
        assert_eq!(actual.total_deal_attempts, expected.total_deal_attempts);
        let old = &expected.prefixes[0];
        assert_eq!(actual.positive_weight_samples, old.positive_weight_samples);
        assert_eq!(actual.relative_weight_mean, old.relative_weight_mean);
        assert_eq!(actual.effective_sample_size, old.effective_sample_size);
        assert_eq!(actual.max_normalized_weight, old.max_normalized_weight);
        assert_eq!(actual.prefix_current_fraction, old.prefix_current_fraction);
        assert_eq!(
            actual.prefix_regret_fallback_fraction,
            old.prefix_regret_fallback_fraction
        );
        assert_eq!(
            actual.prefix_uniform_fallback_fraction,
            old.prefix_uniform_fallback_fraction
        );
        assert_eq!(actual.baseline_seats, old.seats);
        assert_eq!(actual.coverage_by_street, old.coverage_by_street);
        assert_eq!(actual.coverage_by_seat, old.coverage_by_seat);
    }

    #[test]
    fn endpoint_deviation_real_holdem_baseline_and_thread_results_are_exact() {
        for dense in [false, true] {
            let solver = fixture(dense);
            let before = solver.snapshot_state();
            let (path, _) = check_through(solver.game(), Street::Turn);
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
                let config = config();
                let one = without_timings(
                    solver
                        .evaluate_endpoint_deviation_preflop(&path, variant, 1, &config)
                        .unwrap(),
                );
                assert_eq!(one.fit.rows.len(), one.expected_buckets as usize);
                assert_eq!(
                    one.fit.sampling.terminal_replays,
                    one.fit.sampling.positive_weight_samples * (one.action_labels.len() as u64 + 1)
                );
                for sampling in std::iter::once(&one.fit.sampling)
                    .chain(one.held_out.iter().map(|h| &h.sampling))
                {
                    let old = solver
                        .evaluate_profile_conditioned_preflop(
                            sampling.samples,
                            sampling.seed,
                            variant,
                            1,
                            std::slice::from_ref(&path),
                        )
                        .unwrap();
                    assert_eq!(one.proposal, old.proposal);
                    assert_baseline_equal(sampling, &old);
                }
                for threads in [2, 8] {
                    let parallel = without_timings(
                        solver
                            .evaluate_endpoint_deviation_preflop(&path, variant, threads, &config)
                            .unwrap(),
                    );
                    assert_eq!(one, parallel);
                }
                let mut reordered = config;
                reordered.held_out_seeds.reverse();
                let reversed = without_timings(
                    solver
                        .evaluate_endpoint_deviation_preflop(&path, variant, 2, &reordered)
                        .unwrap(),
                );
                assert_eq!(one.fit, reversed.fit);
                assert_eq!(one.held_out[0], reversed.held_out[1]);
                assert_eq!(one.held_out[1], reversed.held_out[0]);
            }
            assert_eq!(solver.snapshot_state(), before);
        }
    }

    #[test]
    fn endpoint_deviation_unsupported_keys_preserve_baseline_and_all_weight() {
        let solver = fixture(true);
        let (path, _) = check_through(solver.game(), Street::Flop);
        let mut config = config();
        config.min_fit_ess = 1e20;
        let result = solver
            .evaluate_endpoint_deviation_preflop(&path, ProfileVariant::default(), 2, &config)
            .unwrap();
        assert_eq!(result.fit.retained_buckets, 0);
        assert!(
            result
                .fit
                .rows
                .iter()
                .all(|row| row.selected_action.is_none())
        );
        for held in result.held_out {
            assert!(held.sampling.positive_weight_samples > 0);
            assert_eq!(
                held.sampling.terminal_replays,
                held.sampling.positive_weight_samples
            );
            assert_eq!(
                held.gain,
                Some(WeightedEstimate {
                    mean: 0.0,
                    stderr: 0.0
                })
            );
            assert_eq!(
                held.retained_key_weight_fraction,
                Some(WeightedEstimate {
                    mean: 0.0,
                    stderr: 0.0
                })
            );
        }
    }

    fn toy_key() -> InfoKey {
        InfoKey {
            history: HistoryKey::ROOT,
            player: 0,
            street: Street::River.index() as u8,
            active_opponents: 2,
            bucket_path: [UNREACHED_BUCKET, UNREACHED_BUCKET, UNREACHED_BUCKET, 0],
        }
    }

    #[test]
    fn endpoint_deviation_fit_cannot_use_hidden_world_argmax() {
        let (game, _, _) = super::super::tests::initialization_holdem_fixture();
        let (path, state) = check_through(&game, Street::River);
        let actor = game.actor(&state).unwrap();
        let combo = |a: &str, b: &str| cards::combo_index(a.parse().unwrap(), b.parse().unwrap());
        let mut holes = vec![combo("As", "Ah"), combo("Ks", "Kh"), combo("Qs", "Qh")];
        let board = ["2c", "3d", "4h", "5s", "6c"].map(|c| c.parse().unwrap());
        let first = SampledWorld::new(holes.clone(), board).unwrap();
        holes[(actor + 1) % 3] = combo("Js", "Jh");
        let second = SampledWorld::new(holes, board).unwrap();
        let own = game.bucket(&state, &first, actor);
        assert_eq!(own, game.bucket(&state, &second, actor));
        let context = game.dense_node_context(&state);
        let endpoint = Endpoint {
            state,
            path,
            history: HistoryKey::ROOT,
            actor,
            context,
            labels: vec![],
            buckets: 4,
        };
        let key = endpoint.key(own.current_bucket());
        // Same own information, opposite hidden-world payoffs. A per-world
        // maximum would falsely claim +3; aggregation admits no improvement.
        let mut moments = FitBucket::default();
        for _ in 0..2 {
            moments.observe(1.0, &[3.0, -3.0]).unwrap();
            moments.observe(1.0, &[-3.0, 3.0]).unwrap();
        }
        let row = moments.finish(key, "average".into(), 2, 2.0).unwrap();
        assert_eq!(row.selected_action, None);
        assert!(row.action_gains.iter().all(|g| g.unwrap().mean == 0.0));
        let mut moments = FitBucket::default();
        for _ in 0..2 {
            moments.observe(1.0, &[4.0, -3.0]).unwrap();
            moments.observe(1.0, &[-2.0, 3.0]).unwrap();
        }
        let row = moments.finish(key, "average".into(), 2, 2.0).unwrap();
        assert_eq!(row.selected_action, Some(0));
        // One frozen action for both worlds, including the world where it loses.
        let table = BTreeMap::from([(row.key, row.selected_action)]);
        for world in [&first, &second] {
            assert_eq!(
                table[&endpoint.key(game.bucket(&endpoint.state, world, actor).current_bucket())],
                Some(0)
            );
        }
    }

    #[test]
    fn endpoint_deviation_fit_support_singletons_and_ties_are_explicit() {
        for n in [0, 1, 4] {
            let mut moments = FitBucket::default();
            for _ in 0..n {
                moments.observe(1.0, &[2.0, 2.0]).unwrap();
            }
            moments.observe(0.0, &[100.0, 200.0]).unwrap();
            let row = moments
                .finish(toy_key(), "uniform-fallback".into(), 2, 2.0)
                .unwrap();
            assert_eq!(row.positive_weight_samples, n);
            assert_eq!(row.effective_sample_size, n as f64);
            if n < 2 {
                assert_eq!(row.action_gains, vec![None, None]);
                assert_eq!(row.selected_action, None);
            } else {
                assert_eq!(row.selected_action, Some(0));
            }
        }
    }

    #[test]
    fn endpoint_deviation_signed_held_out_ratio_matches_residual_oracle() {
        let mut fit = FitBucket::default();
        for _ in 0..2 {
            fit.observe(1.0, &[2.0, -1.0]).unwrap();
            fit.observe(1.0, &[4.0, -1.0]).unwrap();
        }
        let frozen = fit.finish(toy_key(), "average".into(), 2, 2.0).unwrap();
        assert_eq!(frozen.selected_action, Some(0));
        // Fit supports action 0, but new worlds lose. Unsupported keys carry
        // substantial reach, and the zero-weight world is still an observation.
        let observations = [
            (1.0, -2.0, true),
            (3.0, -4.0, true),
            (6.0, 0.0, false),
            (0.0, 0.0, false),
        ];
        let mut moments = HeldOutMoments::default();
        for (weight, gain, selected) in observations {
            let mut gains = [0.0; 8];
            gains[0] = gain;
            moments
                .observe(&EndpointSample {
                    bucket: 0,
                    weight,
                    baseline: vec![],
                    gains,
                    selected,
                    sources: [false; 3],
                    coverage: vec![],
                    attempts: 1,
                    terminal_replays: 0,
                })
                .unwrap();
        }
        let sum_w = observations.iter().map(|v| v.0).sum::<f64>();
        let mean = observations.iter().map(|v| v.0 * v.1).sum::<f64>() / sum_w;
        let residual = observations
            .iter()
            .map(|v| (v.0 * (v.1 - mean)).powi(2))
            .sum::<f64>();
        let stderr = (4.0 / 3.0 * residual).sqrt() / sum_w;
        let actual = moments.gain.ratio().unwrap().unwrap();
        assert!((actual.mean - mean).abs() < 1e-14);
        assert!((actual.stderr - stderr).abs() < 1e-14);
        assert!(actual.mean < 0.0);
        assert!((moments.retained.ratio().unwrap().unwrap().mean - 0.4).abs() < 1e-14);
        assert_eq!(frozen.selected_action, Some(0));
        let mut zero = RatioMoments::default();
        zero.observe(0.0, 0.0);
        zero.observe(0.0, 0.0);
        assert_eq!(zero.ratio().unwrap(), None);
    }

    #[test]
    fn endpoint_deviation_rejects_invalid_budgets_seeds_and_paths() {
        let solver = fixture(false);
        let (path, _) = check_through(solver.game(), Street::Flop);
        let valid = config();
        let mut invalid = Vec::new();
        for samples in [0, 1] {
            let mut c = valid.clone();
            c.fit_samples = samples;
            invalid.push(c);
            let mut c = valid.clone();
            c.held_out_samples = samples;
            invalid.push(c);
        }
        for min in [0.0, 1.9, f64::NAN, f64::INFINITY] {
            let mut c = valid.clone();
            c.min_fit_ess = min;
            invalid.push(c);
        }
        for seeds in [
            vec![],
            vec![valid.fit_seed],
            vec![909, 909],
            (1000..1065).collect(),
        ] {
            let mut c = valid.clone();
            c.held_out_seeds = seeds;
            invalid.push(c);
        }
        for c in invalid {
            assert!(
                solver
                    .evaluate_endpoint_deviation_preflop(&path, ProfileVariant::default(), 1, &c)
                    .is_err()
            );
        }
        assert!(
            solver
                .evaluate_endpoint_deviation_preflop(&path, ProfileVariant::default(), 0, &valid)
                .is_err()
        );
        for p in [vec![usize::MAX], {
            let mut p = path.clone();
            p.push(usize::MAX);
            p
        }] {
            assert!(
                solver
                    .evaluate_endpoint_deviation_preflop(&p, ProfileVariant::default(), 1, &valid)
                    .is_err()
            );
        }
        let (mut terminal, mut state) = check_through(solver.game(), Street::River);
        while solver.game().actor(&state).is_some() {
            let menu = solver.game().node_actions(&state);
            let check = (0..solver.game().num_actions_of(&menu))
                .find(|&a| solver.game().action_label_of(&menu, a) == "check")
                .unwrap();
            state = solver.game().next_state_with(&state, &menu, check);
            terminal.push(check);
        }
        assert!(
            solver
                .evaluate_endpoint_deviation_preflop(
                    &terminal,
                    ProfileVariant::default(),
                    1,
                    &valid
                )
                .is_err()
        );
        assert!(
            solver
                .evaluate_endpoint_deviation_preflop(
                    &path,
                    ProfileVariant {
                        purify_threshold: f32::NAN,
                        use_current_strategy: false
                    },
                    1,
                    &valid
                )
                .is_err()
        );
    }
}

#[cfg(test)]
#[path = "endpoint_counterfactual_tests.rs"]
mod counterfactual_tests;
