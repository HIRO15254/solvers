//! Bounded diagnostics while a fresh research solver is still privately owned.
use std::time::Instant;

use super::*;
use crate::{BettingState, HoldemGame, MultiwayAbstraction};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AverageSamplingDiagnosticsConfig {
    /// Complete public paths, including an empty path for root; at most 64.
    pub support_paths: Vec<Vec<usize>>,
    /// Distinct preflop or postflop decision paths, including root; at most eight.
    pub endpoint_paths: Vec<Vec<usize>>,
    /// Present exactly when endpoint_paths is nonempty.
    pub endpoint: Option<EndpointDeviationConfig>,
    /// Zero disables the separate absolute-root-reach pass.
    pub root_samples: u64,
    pub root_seeds: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestSolver = MultiwaySolver<HoldemGame<crate::abstraction::FeatureHashAbstraction>>;

    fn fixture(vector: bool) -> TestSolver {
        let (game, sampler, mut config) = super::super::tests::initialization_holdem_fixture();
        config.traverser_vector = vector;
        MultiwaySolver::new_preallocated_with_threads(game, sampler, config, 2).unwrap()
    }

    fn river_path(solver: &TestSolver) -> Vec<usize> {
        let game = solver.game();
        let mut state = game.root_state();
        let mut path = Vec::new();
        while state.street != Street::River {
            let actions = game.node_actions(&state);
            let action = (0..game.num_actions_of(&actions))
                .find(|&a| {
                    let label = game.action_label_of(&actions, a);
                    label == "check" || label.starts_with("call:")
                })
                .unwrap();
            path.push(action);
            state = game.next_state_with(&state, &actions, action);
        }
        path
    }

    fn training(
        variant: AverageSamplingResearchVariant,
        threads: usize,
    ) -> AverageSamplingResearchConfig {
        AverageSamplingResearchConfig {
            variant,
            threads,
            sweeps: 16,
            histories: vec![HistoryKey::ROOT],
            evaluation_samples: 0,
            evaluation_seeds: vec![],
            coverage_samples: 0,
            coverage_prefixes: vec![],
        }
    }

    fn diagnostics(path: Vec<usize>) -> AverageSamplingDiagnosticsConfig {
        AverageSamplingDiagnosticsConfig {
            support_paths: vec![vec![], path.clone()],
            endpoint_paths: vec![path],
            endpoint: Some(EndpointDeviationConfig {
                fit_samples: 128,
                fit_seed: 601,
                held_out_samples: 128,
                held_out_seeds: vec![702, 703],
                min_fit_ess: 2.0,
            }),
            root_samples: 128,
            root_seeds: vec![801, 802],
        }
    }

    fn without_clocks(
        mut value: AverageSamplingDiagnosticsResult,
    ) -> AverageSamplingDiagnosticsResult {
        value.elapsed_secs = 0.0;
        for endpoint in &mut value.endpoints {
            endpoint.fit_elapsed_secs = 0.0;
            for held in &mut endpoint.held_out {
                held.elapsed_secs = 0.0;
            }
        }
        value
    }

    #[test]
    fn research_diagnostics_preserve_learning_and_match_all_street_evaluation() {
        for vector in [false, true] {
            let mut direct = fixture(vector);
            let config = training(AverageSamplingResearchVariant::PostflopContinuation, 2);
            let path = river_path(&direct);
            let mut diagnostic = diagnostics(path.clone());
            diagnostic
                .endpoint_paths
                .extend([vec![], path[..1].to_vec()]);
            diagnostic.support_paths.push(path[..1].to_vec());
            let ordinary = direct
                .run_average_sampling_research_inner(config.clone())
                .unwrap();
            let snapshot = direct.snapshot_state();
            let actual = fixture(vector)
                .run_average_sampling_research_with_diagnostics(config, diagnostic.clone())
                .unwrap();
            assert_eq!(ordinary.metrics, actual.result.metrics);
            assert_eq!(
                ordinary.current_regret_fingerprint,
                actual.result.current_regret_fingerprint
            );
            assert_eq!(ordinary.histories, actual.result.histories);
            for (path, support) in diagnostic
                .support_paths
                .iter()
                .zip(&actual.diagnostics.support)
            {
                assert_eq!(support, &direct.research_support(path).unwrap());
                assert_eq!(support.rows.len(), support.expected_buckets as usize);
                for row in &support.rows {
                    assert_eq!(
                        row.regrets,
                        direct.policy(row.key).map(|c| c.regrets.clone())
                    );
                }
            }
            assert_eq!(actual.diagnostics.endpoints.len(), 3);
            assert_eq!(actual.diagnostics.endpoints[1].street, Street::Preflop);
            assert_eq!(actual.diagnostics.endpoints[2].street, Street::Preflop);
            for (path, observed) in diagnostic
                .endpoint_paths
                .iter()
                .zip(&actual.diagnostics.endpoints)
            {
                let mut expected_endpoint = direct
                    .evaluate_endpoint_deviation_preflop(
                        path,
                        ProfileVariant::default(),
                        2,
                        diagnostic.endpoint.as_ref().unwrap(),
                    )
                    .unwrap();
                let mut observed_endpoint = observed.clone();
                expected_endpoint.fit_elapsed_secs = 0.0;
                observed_endpoint.fit_elapsed_secs = 0.0;
                for endpoint in [&mut expected_endpoint, &mut observed_endpoint] {
                    for held in &mut endpoint.held_out {
                        held.elapsed_secs = 0.0;
                    }
                }
                assert_eq!(expected_endpoint, observed_endpoint);
            }
            for (&seed, root) in diagnostic
                .root_seeds
                .iter()
                .zip(&actual.diagnostics.root_evaluations)
            {
                assert_eq!(
                    *root,
                    direct
                        .evaluate_profile_conditioned(
                            128,
                            seed,
                            ProfileVariant::default(),
                            2,
                            &diagnostic.endpoint_paths
                        )
                        .unwrap()
                );
            }
            assert_eq!(snapshot, direct.snapshot_state());
            let json = serde_json::to_value(actual).unwrap();
            assert!(!json.to_string().contains("strategy_sum"));
        }
    }

    #[test]
    fn postflop_research_scalar_vector_regret_controls_and_thread_determinism() {
        for vector in [false, true] {
            let mut outputs = Vec::new();
            for threads in [1, 2, 4] {
                let solver = fixture(vector);
                let diagnostic = diagnostics(river_path(&solver));
                let result = solver
                    .run_average_sampling_research_with_diagnostics(
                        training(
                            AverageSamplingResearchVariant::PostflopContinuation,
                            threads,
                        ),
                        diagnostic,
                    )
                    .unwrap();
                outputs.push(result);
            }
            for other in &outputs[1..] {
                assert_eq!(
                    outputs[0].result.current_regret_fingerprint,
                    other.result.current_regret_fingerprint
                );
                assert_eq!(outputs[0].result.histories, other.result.histories);
                assert_eq!(outputs[0].result.metrics, other.result.metrics);
                assert_eq!(
                    without_clocks(outputs[0].diagnostics.clone()),
                    without_clocks(other.diagnostics.clone())
                );
            }
            let uniform = fixture(vector)
                .run_average_sampling_research(training(
                    AverageSamplingResearchVariant::UniformOne,
                    2,
                ))
                .unwrap();
            assert_eq!(
                uniform.current_regret_fingerprint,
                outputs[0].result.current_regret_fingerprint
            );
            assert_eq!(uniform.metrics.sweeps, outputs[0].result.metrics.sweeps);
            assert_eq!(
                uniform.metrics.traversals,
                outputs[0].result.metrics.traversals
            );
            assert_eq!(
                uniform.metrics.total_deal_attempts,
                outputs[0].result.metrics.total_deal_attempts
            );
            assert_eq!(
                uniform.metrics.hand_updates,
                outputs[0].result.metrics.hand_updates
            );
        }
    }

    #[test]
    fn postflop_research_diagnostics_reject_malformed_requests_before_training() {
        let solver = fixture(false);
        let baseline = solver.snapshot_state();
        let base = diagnostics(river_path(&solver));
        let mut invalid = Vec::new();
        let mut c = base.clone();
        c.support_paths.push(vec![usize::MAX]);
        invalid.push(c);
        let mut c = base.clone();
        c.support_paths.push(vec![]);
        invalid.push(c);
        let mut c = base.clone();
        c.support_paths = vec![vec![]; 65];
        invalid.push(c);
        let mut c = base.clone();
        c.endpoint_paths = vec![base.endpoint_paths[0].clone(); 9];
        invalid.push(c);
        let mut c = base.clone();
        c.endpoint_paths.push(base.endpoint_paths[0].clone());
        invalid.push(c);
        let mut c = base.clone();
        c.endpoint_paths[0] = vec![usize::MAX];
        invalid.push(c);
        let mut c = base.clone();
        c.endpoint = None;
        invalid.push(c);
        let mut c = base.clone();
        c.endpoint.as_mut().unwrap().held_out_seeds.push(601);
        invalid.push(c);
        let mut c = base.clone();
        c.root_samples = 1;
        invalid.push(c);
        let mut c = base.clone();
        c.root_samples = 0;
        invalid.push(c);
        let mut c = base.clone();
        c.root_seeds = vec![];
        invalid.push(c);
        let mut c = base.clone();
        c.root_seeds = vec![702];
        invalid.push(c);
        let mut c = base.clone();
        c.root_seeds = vec![801, 801];
        invalid.push(c);
        for c in invalid {
            assert!(solver.validate_average_diagnostics(&c, 2).is_err());
            assert_eq!(solver.snapshot_state(), baseline);
            assert!(
                fixture(false)
                    .run_average_sampling_research_with_diagnostics(
                        training(AverageSamplingResearchVariant::PostflopContinuation, 2),
                        c
                    )
                    .is_err()
            );
        }
        assert!(solver.validate_average_diagnostics(&base, 0).is_err());
        assert!(
            solver
                .validate_average_diagnostics(&AverageSamplingDiagnosticsConfig::default(), 2)
                .is_ok()
        );
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResearchPolicySupportRow {
    pub key: InfoKey,
    /// None means missing, while an all-zero vector means stored zero regrets.
    pub regrets: Option<Vec<f32>>,
    /// Normalized average only; None means missing or zero average mass.
    pub average_strategy: Option<Vec<f32>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResearchPolicySupport {
    pub action_indices: Vec<usize>,
    pub history: HistoryKey,
    pub actor: u8,
    pub street: Street,
    pub active_opponents: u8,
    pub bucket_active_opponents: u8,
    pub expected_buckets: u32,
    pub action_labels: Vec<String>,
    /// Every configured key. Experimental raw average masses are never exposed.
    pub rows: Vec<ResearchPolicySupportRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AverageSamplingDiagnosticsResult {
    pub config: AverageSamplingDiagnosticsConfig,
    /// Diagnostics only, excluding sweeps and the ordinary research result.
    pub elapsed_secs: f64,
    pub support: Vec<ResearchPolicySupport>,
    pub endpoints: Vec<EndpointDeviationEvaluation>,
    pub root_evaluations: Vec<ConditionalProfileEvaluation>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AverageSamplingWithDiagnostics {
    pub result: AverageSamplingResearchResult,
    pub diagnostics: AverageSamplingDiagnosticsResult,
}

impl<A: MultiwayAbstraction> MultiwaySolver<HoldemGame<A>> {
    /// Consumes a fresh solver, runs the average-only experiment, then reads
    /// support and endpoint diagnostics before disposal. Neither a solver
    /// handle, checkpoint nor raw experimental average masses are returned.
    pub fn run_average_sampling_research_with_diagnostics(
        mut self,
        training: AverageSamplingResearchConfig,
        diagnostics: AverageSamplingDiagnosticsConfig,
    ) -> Result<AverageSamplingWithDiagnostics, SolverError> {
        self.validate_average_diagnostics(&diagnostics, training.threads)?;
        let threads = training.threads;
        let result = self.run_average_sampling_research_inner(training)?;
        let started = Instant::now();
        let support = diagnostics
            .support_paths
            .iter()
            .map(|path| self.research_support(path))
            .collect::<Result<Vec<_>, _>>()?;
        let mut endpoints = Vec::new();
        if let Some(config) = &diagnostics.endpoint {
            for path in &diagnostics.endpoint_paths {
                endpoints.push(self.evaluate_endpoint_deviation_preflop(
                    path,
                    ProfileVariant::default(),
                    threads,
                    config,
                )?);
            }
        }
        let mut root_evaluations = Vec::new();
        for &seed in &diagnostics.root_seeds {
            root_evaluations.push(self.evaluate_profile_conditioned(
                diagnostics.root_samples,
                seed,
                ProfileVariant::default(),
                threads,
                &diagnostics.endpoint_paths,
            )?);
        }
        Ok(AverageSamplingWithDiagnostics {
            result,
            diagnostics: AverageSamplingDiagnosticsResult {
                config: diagnostics,
                elapsed_secs: started.elapsed().as_secs_f64(),
                support,
                endpoints,
                root_evaluations,
            },
        })
    }

    fn validate_average_diagnostics(
        &self,
        config: &AverageSamplingDiagnosticsConfig,
        threads: usize,
    ) -> Result<(), SolverError> {
        if self.game.recall_mode() != RecallMode::Street || threads == 0 {
            return Err(SolverError::InvalidState(
                "average diagnostics require current-street recall and positive threads",
            ));
        }
        if config.support_paths.len() > 64
            || config.endpoint_paths.len() > 8
            || config.endpoint.is_some() == config.endpoint_paths.is_empty()
        {
            return Err(SolverError::InvalidState(
                "average diagnostics require <=64 support paths and <=8 endpoint paths with an explicit endpoint budget",
            ));
        }
        for (i, path) in config.support_paths.iter().enumerate() {
            if config.support_paths[..i].contains(path) {
                return Err(SolverError::InvalidState("duplicate research support path"));
            }
            self.research_support_definition(path)?;
        }
        if let Some(endpoint) = &config.endpoint {
            for (i, path) in config.endpoint_paths.iter().enumerate() {
                if config.endpoint_paths[..i].contains(path) {
                    return Err(SolverError::InvalidState(
                        "duplicate research endpoint path",
                    ));
                }
                self.validate_research_endpoint(path, endpoint, threads)?;
            }
        }
        if config.root_samples == 0 {
            if !config.root_seeds.is_empty() {
                return Err(SolverError::InvalidState(
                    "root seeds require a root budget",
                ));
            }
        } else if config.root_samples < 2
            || config.endpoint_paths.is_empty()
            || config.root_seeds.is_empty()
            || config.root_seeds.len() > 64
            || config.root_seeds.iter().enumerate().any(|(i, seed)| {
                config.root_seeds[..i].contains(seed)
                    || config.endpoint.as_ref().is_some_and(|endpoint| {
                        endpoint.fit_seed == *seed || endpoint.held_out_seeds.contains(seed)
                    })
            })
        {
            return Err(SolverError::InvalidState(
                "root diagnostics require >=2 worlds and 1..=64 unique seeds separate from endpoint fit/held-out",
            ));
        }
        Ok(())
    }

    fn research_support_definition(
        &self,
        path: &[usize],
    ) -> Result<(BettingState, ResearchPolicySupport), SolverError> {
        if path.len() > self.config.max_traversal_depth as usize {
            return Err(SolverError::DepthLimit {
                limit: self.config.max_traversal_depth,
            });
        }
        let mut state = self.game.root_state();
        let mut history = HistoryKey::ROOT;
        for &action in path {
            let actor = self
                .game
                .actor(&state)
                .ok_or(SolverError::InvalidState("terminal support path"))?;
            let menu = self.game.node_actions(&state);
            if action >= self.game.num_actions_of(&menu) {
                return Err(SolverError::InvalidState("invalid support path action"));
            }
            state = self.game.next_state_with(&state, &menu, action);
            history = history.child(actor, action);
        }
        let actor = self
            .game
            .actor(&state)
            .ok_or(SolverError::InvalidState("terminal support endpoint"))?;
        let context = self.game.dense_node_context(&state);
        let expected_buckets = self
            .game
            .bucket_count(context.street, context.bucket_active_opponents);
        if expected_buckets == 0 {
            return Err(SolverError::InvalidState("support endpoint has no buckets"));
        }
        let actions = self.game.node_actions(&state);
        let action_labels = (0..self.game.num_actions_of(&actions))
            .map(|action| self.game.action_label_of(&actions, action))
            .collect::<Vec<_>>();
        support::validate_action_labels(&action_labels)?;
        Ok((
            state,
            ResearchPolicySupport {
                action_indices: path.to_vec(),
                history,
                actor: actor as u8,
                street: context.street,
                active_opponents: context.active_opponents,
                bucket_active_opponents: context.bucket_active_opponents,
                expected_buckets,
                action_labels,
                rows: Vec::new(),
            },
        ))
    }

    fn research_support(&self, path: &[usize]) -> Result<ResearchPolicySupport, SolverError> {
        let (_, mut result) = self.research_support_definition(path)?;
        for bucket in 0..result.expected_buckets {
            let private =
                PrivateInfo::from_current_bucket(result.street, result.active_opponents, bucket);
            let key = InfoKey {
                history: result.history,
                player: result.actor,
                street: private.street,
                active_opponents: private.active_opponents,
                bucket_path: private.bucket_path,
            };
            let (regrets, average_strategy) = match self.policy(key) {
                None => (None, None),
                Some(column) => {
                    if column.action_labels != result.action_labels {
                        return Err(SolverError::ActionLabelsChanged { key });
                    }
                    if column.regrets.len() != result.action_labels.len()
                        || column.strategy_sum.len() != result.action_labels.len()
                        || column.regrets.iter().any(|v| !v.is_finite())
                        || column
                            .strategy_sum
                            .iter()
                            .any(|v| !v.is_finite() || *v < 0.0)
                    {
                        return Err(SolverError::NumericOverflow);
                    }
                    let average = column
                        .strategy_sum
                        .iter()
                        .any(|v| *v > 0.0)
                        .then(|| column.average_strategy());
                    if average
                        .as_ref()
                        .is_some_and(|v| v.iter().any(|p| !p.is_finite()))
                    {
                        return Err(SolverError::NumericOverflow);
                    }
                    (Some(column.regrets.clone()), average)
                }
            };
            result.rows.push(ResearchPolicySupportRow {
                key,
                regrets,
                average_strategy,
            });
        }
        Ok(result)
    }
}
