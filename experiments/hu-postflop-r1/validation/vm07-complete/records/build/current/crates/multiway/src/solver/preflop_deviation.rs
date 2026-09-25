//! Independently fitted, preflop-only unilateral deviations against a frozen
//! profile. The held-out accumulator retains one bounded batch, not all worlds.

use std::time::Instant;

use super::eval::validate_purify_threshold;
use super::*;

const HELD_OUT_CHUNK_SAMPLES: u64 = 4096;

/// Diagnostic fitting only; neither mode changes the learned solver profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PreflopDeviationFitMode {
    #[default]
    LocalRegretMatching,
    /// Use the frozen baseline until a key reaches the retention threshold,
    /// while continuing to explore and update every own action.
    RetentionGated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflopDeviationConfig {
    pub fit_traversals_per_seat: u64,
    pub fit_seed: u64,
    pub held_out_samples: u64,
    pub held_out_seeds: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflopDeviationHeldOut {
    pub seed: u64,
    pub samples: u64,
    pub elapsed_secs: f64,
    pub total_deal_attempts: u64,
    pub baseline: Vec<ProfileEstimate>,
    pub deviating: Vec<ProfileEstimate>,
    /// Signed paired gain, including every sampled world and unsupported key.
    /// The approximate 95% intervals are per seat, not a simultaneous bound.
    pub gains: Vec<ProfileEstimate>,
    pub coverage: Vec<ReferenceDeviationCoverage>,
    pub candidate_policy_coverage: Vec<CandidatePolicyCoverage>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflopDeviationEvaluation {
    pub schema_version: &'static str,
    pub scope: &'static str,
    pub config: PreflopDeviationConfig,
    pub variant: ProfileVariant,
    pub fit_mode: PreflopDeviationFitMode,
    pub min_fit_visits: u32,
    pub fit_elapsed_secs: f64,
    pub fit_coverage: Vec<DeviatorTrainingCoverage>,
    /// Identity of the sorted fitted actions only, not of the frozen baseline.
    pub fit_policy_fingerprint: String,
    /// Upper bound on concurrently retained held-out sample results. It does
    /// not include fitted tables, allocator overhead, worker stacks or solver.
    pub max_buffered_samples: u64,
    pub held_out: Vec<PreflopDeviationHeldOut>,
}

impl PreflopDeviationConfig {
    pub fn validate(&self) -> Result<(), SolverError> {
        if self.fit_traversals_per_seat == 0 || self.held_out_samples < 2 {
            return Err(SolverError::InvalidState(
                "preflop deviation needs positive fitting traversals and at least two held-out samples",
            ));
        }
        if self.held_out_seeds.is_empty() || self.held_out_seeds.len() > 64 {
            return Err(SolverError::InvalidState(
                "preflop deviation needs one to 64 held-out seeds",
            ));
        }
        for (index, seed) in self.held_out_seeds.iter().enumerate() {
            if *seed == self.fit_seed || self.held_out_seeds[..index].contains(seed) {
                return Err(SolverError::InvalidState(
                    "preflop deviation seeds must be unique and separate from fitting",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Moments {
    mean: f64,
    m2: f64,
}

impl Moments {
    fn observe(&mut self, value: f64, count: u64) -> Result<(), SolverError> {
        let delta = value - self.mean;
        let mean = self.mean + delta / count as f64;
        let m2 = self.m2 + delta * (value - mean);
        if !value.is_finite() || !mean.is_finite() || !m2.is_finite() || m2 < 0.0 {
            return Err(SolverError::NumericOverflow);
        }
        self.mean = mean;
        self.m2 = m2;
        Ok(())
    }

    fn finish(self, count: u64) -> Result<ProfileEstimate, SolverError> {
        let result = profile_estimate(self.mean, self.m2, count);
        if !result.mean.is_finite()
            || !result.stderr.is_finite()
            || result.ci95.iter().any(|value| !value.is_finite())
        {
            return Err(SolverError::NumericOverflow);
        }
        Ok(result)
    }
}

struct Sample {
    deal_attempts: u64,
    baseline: Vec<f64>,
    deviating: Vec<f64>,
    coverage: Vec<ReferenceDeviationCoverage>,
    candidate_coverage: Vec<CandidatePolicyCoverage>,
}

struct Accumulator {
    count: u64,
    attempts: u64,
    baseline: Vec<Moments>,
    deviating: Vec<Moments>,
    gains: Vec<Moments>,
    coverage: Vec<ReferenceDeviationCoverage>,
    candidate_coverage: Vec<CandidatePolicyCoverage>,
}

impl Accumulator {
    fn new(players: usize) -> Self {
        Self {
            count: 0,
            attempts: 0,
            baseline: vec![Moments::default(); players],
            deviating: vec![Moments::default(); players],
            gains: vec![Moments::default(); players],
            coverage: vec![ReferenceDeviationCoverage::default(); players],
            candidate_coverage: vec![CandidatePolicyCoverage::default(); players],
        }
    }

    fn observe(&mut self, sample: Sample) -> Result<(), SolverError> {
        self.count = self
            .count
            .checked_add(1)
            .ok_or(SolverError::CounterOverflow)?;
        self.attempts = self
            .attempts
            .checked_add(sample.deal_attempts)
            .ok_or(SolverError::CounterOverflow)?;
        for seat in 0..self.baseline.len() {
            self.baseline[seat].observe(sample.baseline[seat], self.count)?;
            self.deviating[seat].observe(sample.deviating[seat], self.count)?;
            self.gains[seat].observe(sample.deviating[seat] - sample.baseline[seat], self.count)?;
            self.coverage[seat].checked_add_assign(sample.coverage[seat])?;
            self.candidate_coverage[seat].checked_add_assign(sample.candidate_coverage[seat])?;
        }
        Ok(())
    }

    fn finish(self, seed: u64, elapsed_secs: f64) -> Result<PreflopDeviationHeldOut, SolverError> {
        let estimates = |values: Vec<Moments>| {
            values
                .into_iter()
                .map(|value| value.finish(self.count))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(PreflopDeviationHeldOut {
            seed,
            samples: self.count,
            elapsed_secs,
            total_deal_attempts: self.attempts,
            baseline: estimates(self.baseline)?,
            deviating: estimates(self.deviating)?,
            gains: estimates(self.gains)?,
            coverage: self.coverage,
            candidate_policy_coverage: self.candidate_coverage,
        })
    }
}

fn validate_policies(players: usize, policies: &[DeviatorPolicy]) -> Result<(), SolverError> {
    if policies.len() != players {
        return Err(SolverError::InvalidState(
            "preflop deviators must contain one policy per seat",
        ));
    }
    for (seat, policy) in policies.iter().enumerate() {
        if policy.seat != seat
            || policy.actions.keys().any(|key| {
                key.player as usize != seat
                    || key.street != Street::Preflop as u8
                    || key.bucket_path[0] == UNREACHED_BUCKET
                    || key.bucket_path[1..]
                        .iter()
                        .any(|&bucket| bucket != UNREACHED_BUCKET)
                    || key.active_opponents == 0
                    || key.active_opponents as usize >= players
            })
        {
            return Err(SolverError::InvalidState(
                "preflop deviator contains an invalid seat or non-preflop key",
            ));
        }
    }
    Ok(())
}

fn policy_fingerprint(policies: &[DeviatorPolicy]) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(b"solvers.multiway.preflop-deviator-actions.v1\0");
    hash.update(&(policies.len() as u64).to_le_bytes());
    for policy in policies {
        hash.update(&(policy.seat as u64).to_le_bytes());
        hash.update(&(policy.actions.len() as u64).to_le_bytes());
        let mut sorted = policy.actions.iter().collect::<Vec<_>>();
        sorted.sort_unstable_by_key(|(key, _)| **key);
        for (key, action) in sorted {
            hash.update(&key.history.0);
            hash.update(&[key.player, key.street, key.active_opponents]);
            for bucket in key.bucket_path {
                hash.update(&bucket.to_le_bytes());
            }
            hash.update(&action.to_le_bytes());
        }
    }
    hash.finalize().to_hex().to_string()
}

impl<G: ExternalSamplingGame> MultiwaySolver<G> {
    /// Fit one unilateral policy per seat against this immutable baseline,
    /// changing all of that seat's preflop decisions only. Postflop and missing
    /// fitted keys replay the exact candidate baseline. Independent held-out
    /// worlds estimate signed root gains, not full BR or multiway exploitability.
    ///
    /// Fitted tables scale with visited preflop information sets; held-out
    /// scratch is bounded by 4096 samples, independent of the requested budget.
    /// All fitting and replay work is read-only with respect to the solver.
    pub fn evaluate_preflop_deviation(
        &self,
        variant: ProfileVariant,
        threads: usize,
        config: &PreflopDeviationConfig,
    ) -> Result<PreflopDeviationEvaluation, SolverError> {
        self.evaluate_preflop_deviation_with_fit_mode(
            variant,
            threads,
            config,
            PreflopDeviationFitMode::LocalRegretMatching,
        )
    }

    /// Select a research fitting rule. Retention gating prevents an ancestor
    /// from valuing a learned continuation at a key that is still unsupported.
    /// It does not guarantee a profitable final pure action table: finite fit,
    /// later policy changes and pure extraction can still produce losses.
    pub fn evaluate_preflop_deviation_with_fit_mode(
        &self,
        variant: ProfileVariant,
        threads: usize,
        config: &PreflopDeviationConfig,
        fit_mode: PreflopDeviationFitMode,
    ) -> Result<PreflopDeviationEvaluation, SolverError> {
        config.validate()?;
        validate_purify_threshold(variant.purify_threshold)?;
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        let started = Instant::now();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|error| SolverError::ThreadPoolBuild(error.to_string()))?;
        let fitted = pool.install(|| {
            (0..self.game.num_players())
                .into_par_iter()
                .map(|seat| match fit_mode {
                    PreflopDeviationFitMode::LocalRegretMatching => self
                        .train_deviator_core::<true>(
                            seat,
                            config.fit_traversals_per_seat,
                            config.fit_seed,
                            variant.purify_threshold,
                            variant.use_current_strategy,
                        ),
                    PreflopDeviationFitMode::RetentionGated => self
                        .train_deviator_core_with_retention_gate::<true, true>(
                            seat,
                            config.fit_traversals_per_seat,
                            config.fit_seed,
                            variant.purify_threshold,
                            variant.use_current_strategy,
                        ),
                })
                .collect::<Vec<_>>()
        });
        let mut policies = Vec::with_capacity(fitted.len());
        let mut fit_coverage = Vec::with_capacity(fitted.len());
        for result in fitted {
            let result = result?;
            fit_coverage.push(result.coverage);
            policies.push(result.policy);
        }
        validate_policies(self.game.num_players(), &policies)?;
        let fit_policy_fingerprint = policy_fingerprint(&policies);
        let fit_elapsed_secs = started.elapsed().as_secs_f64();
        // Drop the fit pool before constructing held-out pools. A sample batch
        // never retains an unrelated set of idle fit threads.
        drop(pool);
        let mut held_out = Vec::with_capacity(config.held_out_seeds.len());
        for &seed in &config.held_out_seeds {
            held_out.push(self.evaluate_frozen_preflop_deviators(
                config.held_out_samples,
                seed,
                &policies,
                variant,
                threads,
            )?);
        }
        Ok(PreflopDeviationEvaluation {
            schema_version: "solvers.multiway-preflop-deviation/v1",
            scope: "all-preflop-decisions-with-frozen-postflop",
            config: config.clone(),
            variant,
            fit_mode,
            min_fit_visits: MIN_DEVIATOR_POLICY_VISITS,
            fit_elapsed_secs,
            fit_coverage,
            fit_policy_fingerprint,
            max_buffered_samples: HELD_OUT_CHUNK_SAMPLES.min(config.held_out_samples),
            held_out,
        })
    }

    pub(super) fn evaluate_frozen_preflop_deviators(
        &self,
        samples: u64,
        seed: u64,
        policies: &[DeviatorPolicy],
        variant: ProfileVariant,
        threads: usize,
    ) -> Result<PreflopDeviationHeldOut, SolverError> {
        if samples < 2 {
            return Err(SolverError::InvalidState(
                "preflop deviation needs at least two held-out samples",
            ));
        }
        if threads == 0 {
            return Err(SolverError::ZeroThreads);
        }
        validate_purify_threshold(variant.purify_threshold)?;
        validate_policies(self.game.num_players(), policies)?;
        let started = Instant::now();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|error| SolverError::ThreadPoolBuild(error.to_string()))?;
        let mut moments = Accumulator::new(self.game.num_players());
        let mut start = 0;
        while start < samples {
            let end = start.saturating_add(HELD_OUT_CHUNK_SAMPLES).min(samples);
            let count = usize::try_from(end - start).expect("bounded held-out chunk fits usize");
            let results = pool.install(|| {
                (0..count)
                    .into_par_iter()
                    .map(|offset| {
                        let sample_id = start + offset as u64;
                        self.preflop_deviation_sample(sample_id, seed, policies, variant)
                    })
                    .collect::<Vec<_>>()
            });
            // Preserve sample order and floating-point reduction order across
            // thread counts and across the final short batch.
            for result in results {
                moments.observe(result?)?;
            }
            start = end;
        }
        moments.finish(seed, started.elapsed().as_secs_f64())
    }

    fn preflop_deviation_sample(
        &self,
        sample_id: u64,
        seed: u64,
        policies: &[DeviatorPolicy],
        variant: ProfileVariant,
    ) -> Result<Sample, SolverError> {
        let mut deal_rng = evaluation_deal_rng(seed, sample_id);
        let deal = self.sampler.sample_counted(&mut deal_rng)?;
        let common_rng = evaluation_action_rng(seed, sample_id, None);
        let mut baseline_rng = common_rng.clone();
        let mut candidate_coverage =
            vec![CandidatePolicyCoverage::default(); self.game.num_players()];
        let baseline = self.evaluate_world(
            &deal.world,
            &mut baseline_rng,
            None,
            None,
            variant.purify_threshold,
            variant.use_current_strategy,
            Some(&mut candidate_coverage),
            &mut [],
            None,
        )?;
        let mut deviating = Vec::with_capacity(self.game.num_players());
        let mut coverage = Vec::with_capacity(self.game.num_players());
        for seat in 0..self.game.num_players() {
            let (utilities, visits) = self.evaluate_reference_world::<true>(
                &deal.world,
                &mut common_rng.clone(),
                seat,
                policies,
                variant.purify_threshold,
                variant.use_current_strategy,
            )?;
            deviating.push(utilities[seat]);
            coverage.push(visits);
        }
        Ok(Sample {
            deal_attempts: u64::from(deal.attempts),
            baseline,
            deviating,
            coverage,
            candidate_coverage,
        })
    }
}
