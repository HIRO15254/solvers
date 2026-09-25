use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, ValueEnum};
use multiway::{
    CandidatePolicyCoverage, DeviatorTrainingCoverage, ExternalSamplingGame, HistoryKey, InfoKey,
    MultiwayAbstractionBackend, MultiwaySolver, PolicyArenaAllocation, ProfileEvaluation,
    ProfileVariant, Street, StreetVisitCounts,
};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::Serialize;

type ProductionSolver = MultiwaySolver<multiway::HoldemGame<MultiwayAbstractionBackend>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum ConditionSampler {
    #[default]
    Root,
    PreflopProposal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum EndpointTarget {
    #[default]
    ActualPrefix,
    OpponentsPrefix,
    Both,
}

impl EndpointTarget {
    fn targets(self) -> &'static [Self] {
        match self {
            Self::ActualPrefix => &[Self::ActualPrefix],
            Self::OpponentsPrefix => &[Self::OpponentsPrefix],
            Self::Both => &[Self::ActualPrefix, Self::OpponentsPrefix],
        }
    }
}

/// Audit a frozen checkpoint or a fresh, fixed-sweep Multiway research run.
#[derive(Debug, Parser)]
#[command(name = "mw_checkpoint_audit")]
struct Args {
    /// Original v1 config used to create the checkpoint.
    #[arg(long)]
    config: PathBuf,

    /// Atomic `.mwckpt` snapshot to restore and evaluate.
    #[arg(
        long,
        required_unless_present = "fresh_sweeps",
        conflicts_with = "fresh_sweeps"
    )]
    checkpoint: Option<PathBuf>,

    /// Train a fresh solver for this many sweeps before the audit. No resume,
    /// checkpoint or solution writes; replaces --checkpoint. Config stop/time
    /// schedules do not drive this bounded research run.
    #[arg(long, conflicts_with = "checkpoint", value_parser = clap::value_parser!(u64).range(1..))]
    fresh_sweeps: Option<u64>,

    /// Research-only regret sampling: enumerate the first opponent response
    /// after a preflop raise on each path. Requires a fresh range-vector run,
    /// zero opponent exploration, no pruning and research-regret-sampling.
    #[arg(long, requires = "fresh_sweeps", conflicts_with = "checkpoint")]
    enumerate_raised_preflop: bool,

    /// Independent held-out evaluation seeds, comma-delimited.
    #[arg(long, value_delimiter = ',', default_value = "1")]
    evaluation_seeds: Vec<u64>,

    /// Held-out worlds evaluated for each seed (minimum 2).
    #[arg(long, default_value_t = 8_192)]
    samples: u64,

    /// External-sampling traversals used to train each fixed seat deviator.
    #[arg(long, default_value_t = 100_000)]
    br_traversals: u64,

    /// Seed used once to train the fixed deviator set.
    #[arg(long, default_value_t = 0x6272_2d61_7564_6974)]
    br_seed: u64,

    /// Physical card worlds used for each node's range-wide frequency
    /// estimate. Zero omits these estimates; positive values must be >= 2.
    #[arg(long, default_value_t = 8_192)]
    node_frequency_samples: u64,

    /// Physical-world seed for range-wide node frequency estimates.
    #[arg(long, default_value_t = 0x6e6f_6465_2d66_7265)]
    node_frequency_seed: u64,

    /// Override threads for restoration, deviator training, and held-out evaluation.
    #[arg(long)]
    threads: Option<usize>,

    /// Override the policy-arena budget, for example `48GiB`.
    #[arg(long)]
    memory: Option<String>,

    /// Override the machine-local EHS2 cache directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Preflop public node to export: `root`, a 32-digit history key, or a
    /// slash-separated sequence of action labels/indices. Repeatable.
    #[arg(long, default_value = "root")]
    node: Vec<String>,

    /// Public decision prefix whose baseline descendants are counted by
    /// street/seat. Same path syntax as --node; all streets allowed. Up to 64.
    #[arg(long)]
    coverage_prefix: Vec<String>,

    /// Baseline-only worlds per evaluation seed for prefix coverage. Defaults
    /// to --samples; skips deviation replays and reports no deviation gain.
    #[arg(long, requires = "coverage_prefix", value_parser = clap::value_parser!(u64).range(2..))]
    coverage_samples: Option<u64>,

    /// Public decision prefix to force before reach-weighted baseline rollouts.
    /// Same path syntax as --node; all streets allowed. Repeatable, up to 64.
    #[arg(long)]
    condition_prefix: Vec<String>,

    /// Physical worlds per evaluation seed for forced-prefix diagnostics
    /// (per preflop trunk when using preflop-proposal).
    /// Zero disables them; enabled values must be >= 2 and require prefixes.
    #[arg(long, default_value_t = 0)]
    condition_samples: u64,

    /// Physical-world proposal for conditional diagnostics. Preflop-proposal
    /// accepts postflop endpoints and samples separately for each preflop trunk.
    #[arg(long, value_enum, default_value = "root", requires_all = ["condition_prefix", "condition_samples"])]
    condition_sampler: ConditionSampler,

    /// Export stored/missing support and raw regret/average columns at an
    /// exact public decision node, including postflop buckets. Up to 64.
    #[arg(long)]
    support_node: Vec<String>,

    /// Census every materialized preflop decision, including untouched nodes.
    /// Compact raw-state hashes and support counts are not convergence metrics.
    #[arg(long)]
    preflop_support_census: bool,

    /// Preflop or postflop decisions (including root) at which to independently
    /// fit frozen own-information action tables. Repeatable, up to eight.
    #[arg(long, requires_all = ["endpoint_fit_samples", "endpoint_samples", "endpoint_fit_seed", "endpoint_seeds"])]
    endpoint_prefix: Vec<String>,

    /// Prefix target for the endpoint diagnostic. Opponents-prefix excludes
    /// the endpoint actor's own earlier actions and supports preflop only.
    /// Both fits the two populations independently on this same frozen state.
    #[arg(
        long,
        value_enum,
        default_value = "actual-prefix",
        requires = "endpoint_prefix"
    )]
    endpoint_target: EndpointTarget,

    /// Accepted proposal worlds used once to fit the endpoint action table.
    #[arg(long, requires = "endpoint_prefix", value_parser = clap::value_parser!(u64).range(2..))]
    endpoint_fit_samples: Option<u64>,

    /// Accepted proposal worlds for each endpoint held-out seed.
    #[arg(long, requires = "endpoint_prefix", value_parser = clap::value_parser!(u64).range(2..))]
    endpoint_samples: Option<u64>,

    /// Fit seed, distinct from every --endpoint-seeds value.
    #[arg(long, requires = "endpoint_prefix")]
    endpoint_fit_seed: Option<u64>,

    /// One to 64 unique held-out endpoint seeds, comma-delimited.
    #[arg(long, requires = "endpoint_prefix", value_delimiter = ',')]
    endpoint_seeds: Vec<u64>,

    /// Minimum fit weight ESS per key before retaining a positive-gain action.
    /// This evidence threshold is not a confidence guarantee.
    #[arg(long, requires = "endpoint_prefix", default_value_t = 64.0)]
    endpoint_min_fit_ess: f64,

    /// Fit a separate unilateral policy per seat that may change every own
    /// preflop decision, with frozen postflop continuation and baseline fallback.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), requires_all = ["preflop_deviation_fit_seed", "preflop_deviation_samples", "preflop_deviation_seeds"])]
    preflop_deviation_fit_traversals: Option<u64>,

    /// Seed for the preflop-only fitting pass, separate from held-out seeds.
    #[arg(long, requires = "preflop_deviation_fit_traversals")]
    preflop_deviation_fit_seed: Option<u64>,

    /// Held-out physical worlds per seed for signed whole-preflop root gains.
    #[arg(long, requires = "preflop_deviation_fit_traversals", value_parser = clap::value_parser!(u64).range(2..))]
    preflop_deviation_samples: Option<u64>,

    /// One to 64 unique held-out seeds, distinct from the preflop-only fit seed.
    #[arg(
        long,
        requires = "preflop_deviation_fit_traversals",
        value_delimiter = ','
    )]
    preflop_deviation_seeds: Vec<u64>,

    /// Keep under-supported own preflop continuations on the baseline during fit.
    #[arg(long, requires = "preflop_deviation_fit_traversals")]
    preflop_deviation_retention_gate: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditOutput {
    schema_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkpoint: Option<String>,
    config: String,
    sweeps: u64,
    solver_state_version: u16,
    configuration_fingerprint: String,
    abstraction_fingerprint: String,
    policy_arena: Option<PolicyArenaAllocation>,
    construction_elapsed_secs: f64,
    evaluation_samples_per_seed: u64,
    evaluation_seeds: Vec<u64>,
    deviator_training: DeviatorTrainingOutput,
    evaluations: Vec<EvaluationOutput>,
    nodes: Vec<NodeOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    coverage_evaluations: Vec<CoverageEvaluationOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    conditional_evaluations: Vec<ConditionalEvaluationOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    preflop_conditional_evaluations: Vec<PreflopConditionalEvaluationOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fresh_training: Option<FreshTrainingOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    policy_support: Vec<PolicySupportNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preflop_support_census: Option<PreflopSupportCensusOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preflop_deviation: Option<multiway::PreflopDeviationEvaluation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint_deviation: Option<EndpointDeviationOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint_counterfactual_deviation: Option<CounterfactualEndpointDeviationOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    endpoint_deviations: Vec<EndpointDeviationOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    endpoint_counterfactual_deviations: Vec<CounterfactualEndpointDeviationOutput>,
    interpretation: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FreshTrainingOutput {
    requested_sweeps: u64,
    solve_elapsed_secs: f64,
    effective_config_blake3: String,
    solver_config: multiway::SolverConfig,
    metrics: multiway::SolverMetrics,
    #[cfg(feature = "research-regret-sampling")]
    #[serde(skip_serializing_if = "Option::is_none")]
    regret_sampling_research: Option<multiway::RaisedPreflopResearchWork>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflopSupportCensusOutput {
    elapsed_secs: f64,
    result: multiway::PreflopSupportCensus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EndpointDeviationOutput {
    context: ConditionalPrefixContext,
    elapsed_secs: f64,
    result: multiway::EndpointDeviationEvaluation,
    interpretation: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CounterfactualEndpointDeviationOutput {
    context: ConditionalPrefixContext,
    elapsed_secs: f64,
    result: multiway::CounterfactualEndpointDeviationEvaluation,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicySupportNode {
    context: ConditionalPrefixContext,
    bucket_active_opponents: u8,
    expected_buckets: u32,
    stored_buckets: usize,
    nonzero_regret_buckets: usize,
    positive_regret_buckets: usize,
    average_buckets: usize,
    average_and_nonzero_regret_buckets: usize,
    action_labels: Vec<String>,
    rows: Vec<PolicySupportRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicySupportRow {
    bucket: u32,
    status: &'static str,
    regrets: Option<Vec<f32>>,
    strategy_sum: Option<Vec<f32>>,
    strategy_mass: Option<f64>,
    current_strategy: Option<Vec<f32>>,
    // Missing positive average mass stays null, not a normalized fallback.
    average_strategy: Option<Vec<f32>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviatorTrainingOutput {
    seed: u64,
    traversals_per_seat: u64,
    elapsed_secs: f64,
    coverage: Vec<DeviatorTrainingCoverage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationOutput {
    seed: u64,
    elapsed_secs: f64,
    result: ProfileEvaluation,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CoverageEvaluationOutput {
    seed: u64,
    elapsed_secs: f64,
    result: ProfileEvaluation,
    prefixes: Vec<PrefixCoverageOutput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrefixCoverageOutput {
    requested: String,
    history: String,
    reached_samples: u64,
    reached_fraction: f64,
    trajectory_visits_by_street: StreetVisitCounts,
    candidate_policy_coverage: Vec<CandidatePolicyCoverage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConditionalEvaluationOutput {
    seed: u64,
    prefixes: Vec<ConditionalPrefixContext>,
    result: multiway::ConditionalProfileEvaluation,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflopConditionalEvaluationOutput {
    seed: u64,
    prefixes: Vec<ConditionalPrefixContext>,
    result: multiway::PreflopConditionalProfileEvaluation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConditionalPrefixContext {
    requested: String,
    history: String,
    action_indices: Vec<usize>,
    action_labels: Vec<String>,
    actor: u8,
    street: Street,
    active_opponents: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeOutput {
    requested: String,
    history: String,
    actor: u8,
    active_opponents: u8,
    actions: Vec<String>,
    hands: Vec<HandOutput>,
    frequency: Option<NodeFrequencyOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_elapsed_secs: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HandOutput {
    hand: String,
    bucket: u32,
    status: &'static str,
    strategy_mass: f64,
    strategy: Option<BTreeMap<String, f32>>,
}

#[derive(Clone, Copy)]
struct NodeFrequencyConfig {
    samples: u64,
    seed: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeFrequencyOutput {
    sample_count: u64,
    seed: u64,
    total_deal_attempts: u64,
    conditional_action_rates: Option<BTreeMap<String, Estimate>>,
    reach_probability_estimate: Estimate,
    effective_sample_size: f64,
    fallback_reach_weight_fraction: FallbackReachWeightFraction,
    estimator: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Estimate {
    estimate: f64,
    standard_error: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FallbackReachWeightFraction {
    any: Option<f64>,
    current_regret: Option<f64>,
    uniform: Option<f64>,
    categories_may_overlap: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FallbackFlags {
    current_regret: bool,
    uniform: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_research_build(&args)?;
    validate_parameters(
        &args.evaluation_seeds,
        args.samples,
        args.br_traversals,
        args.threads,
        args.node_frequency_samples,
    )?;
    if args.coverage_prefix.len() > 64 {
        bail!("at most 64 coverage prefixes are supported");
    }
    if args.support_node.len() > 64 {
        bail!("at most 64 support nodes are supported");
    }
    validate_condition_parameters(
        &args.condition_prefix,
        args.condition_samples,
        args.condition_sampler,
    )?;
    let endpoint_config = endpoint_config(&args)?;
    let preflop_deviation_config = preflop_deviation_config(&args)?;
    if let Some(cache_dir) = args.cache_dir.as_deref() {
        cli::cache::set_root_override(cache_dir);
    }

    let raw_config = fs::read_to_string(&args.config)
        .with_context(|| format!("reading {}", args.config.display()))?;
    let effective_config = cli::multiway_v1::apply_solve_overrides(
        &raw_config,
        args.threads,
        args.memory.as_deref(),
        None,
        Some(&args.config),
    )?;
    let construction_started = Instant::now();
    let mut session = cli::session::build_production_multiway_session(
        &effective_config,
        args.checkpoint.as_deref(),
    )
    .context("constructing audit solver (restored checkpoint fingerprints must match)")?;
    let construction_elapsed_secs = construction_started.elapsed().as_secs_f64();
    let solver = &session.solver;
    // Validate prefixes before node-frequency estimation or deviator training.
    let coverage_histories = resolve_coverage_prefixes(solver, &args.coverage_prefix)?;
    let condition_contexts = resolve_condition_prefixes(solver, &args.condition_prefix)?;
    let support_contexts = resolve_condition_prefixes(solver, &args.support_node)?;
    let endpoint_contexts = resolve_endpoints(solver, &args.endpoint_prefix, args.endpoint_target)?;
    let condition_paths = condition_contexts
        .iter()
        .map(|context| context.action_indices.clone())
        .collect::<Vec<_>>();
    let condition_groups = if args.condition_sampler == ConditionSampler::PreflopProposal {
        group_preflop_condition_prefixes(solver, &condition_contexts)?
    } else {
        Vec::new()
    };

    // Validate every requested node before spending the fresh training budget.
    for requested in &args.node {
        let history = resolve_node(solver, requested)?;
        let node = solver
            .public_node_view(history)
            .context("unknown audit node")?;
        ensure_preflop(node.street, requested)?;
    }
    let fresh_training = if let Some(sweeps) = args.fresh_sweeps {
        let started = Instant::now();
        #[cfg(feature = "research-regret-sampling")]
        let regret_sampling_research = if args.enumerate_raised_preflop {
            Some(
                session
                    .solver
                    .run_raised_preflop_research(sweeps, session.threads)?,
            )
        } else {
            session
                .solver
                .run_sweeps_with_threads(sweeps, session.threads)?;
            None
        };
        #[cfg(not(feature = "research-regret-sampling"))]
        session
            .solver
            .run_sweeps_with_threads(sweeps, session.threads)?;
        let solve_elapsed_secs = started.elapsed().as_secs_f64();
        if session.solver.completed_sweeps() != sweeps {
            bail!("fresh solver did not complete the requested sweep budget");
        }
        Some(FreshTrainingOutput {
            requested_sweeps: sweeps,
            solve_elapsed_secs,
            effective_config_blake3: blake3::hash(effective_config.as_bytes())
                .to_hex()
                .to_string(),
            solver_config: session.solver.config(),
            metrics: session.solver.metrics(),
            #[cfg(feature = "research-regret-sampling")]
            regret_sampling_research,
        })
    } else {
        None
    };
    let solver = &session.solver;
    let policy_support = support_contexts
        .into_iter()
        .map(|context| export_policy_support(solver, context))
        .collect::<Result<Vec<_>>>()?;

    let preflop_support_census = if args.preflop_support_census {
        let started = Instant::now();
        Some(PreflopSupportCensusOutput {
            result: solver.preflop_support_census()?,
            elapsed_secs: started.elapsed().as_secs_f64(),
        })
    } else {
        None
    };

    let nodes = args
        .node
        .iter()
        .map(|requested| {
            export_preflop_node(
                solver,
                requested,
                NodeFrequencyConfig {
                    samples: args.node_frequency_samples,
                    seed: args.node_frequency_seed,
                },
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let training_started = Instant::now();
    let reports = cli::session::train_deviators_with_reports_parallel(
        solver,
        session.game_config.seats.len(),
        session.threads,
        args.br_traversals,
        args.br_seed,
        ProfileVariant::default(),
    )?;
    let training_elapsed = training_started.elapsed().as_secs_f64();
    let coverage = reports.iter().map(|report| report.coverage).collect();
    let deviators = reports
        .into_iter()
        .map(|report| report.policy)
        .collect::<Vec<_>>();

    let mut evaluations = Vec::with_capacity(args.evaluation_seeds.len());
    for &seed in &args.evaluation_seeds {
        let started = Instant::now();
        let result = solver.evaluate_profile_with_threads(
            args.samples,
            seed,
            Some(&deviators),
            ProfileVariant::default(),
            session.threads,
        )?;
        evaluations.push(EvaluationOutput {
            seed,
            elapsed_secs: started.elapsed().as_secs_f64(),
            result,
        });
    }

    let mut coverage_evaluations = Vec::new();
    if !coverage_histories.is_empty() {
        let samples = args.coverage_samples.unwrap_or(args.samples);
        for &seed in &args.evaluation_seeds {
            let started = Instant::now();
            let result = solver.evaluate_profile_coverage(
                samples,
                seed,
                ProfileVariant::default(),
                session.threads,
                &coverage_histories,
            )?;
            let prefixes = result
                .prefixes
                .into_iter()
                .zip(&args.coverage_prefix)
                .map(|(coverage, requested)| PrefixCoverageOutput {
                    requested: requested.clone(),
                    history: hex(coverage.history.0),
                    reached_samples: coverage.reached_samples,
                    reached_fraction: coverage.reached_samples as f64 / samples as f64,
                    trajectory_visits_by_street: coverage.trajectory_visits_by_street,
                    candidate_policy_coverage: coverage.candidate_policy_coverage,
                })
                .collect();
            coverage_evaluations.push(CoverageEvaluationOutput {
                seed,
                elapsed_secs: started.elapsed().as_secs_f64(),
                result: result.evaluation,
                prefixes,
            });
        }
    }

    let mut conditional_evaluations = Vec::new();
    if args.condition_sampler == ConditionSampler::Root && !condition_paths.is_empty() {
        for &seed in &args.evaluation_seeds {
            let result = solver.evaluate_profile_conditioned(
                args.condition_samples,
                seed,
                ProfileVariant::default(),
                session.threads,
                &condition_paths,
            )?;
            conditional_evaluations.push(ConditionalEvaluationOutput {
                seed,
                prefixes: condition_contexts.clone(),
                result,
            });
        }
    }
    let mut preflop_conditional_evaluations = Vec::new();
    for &seed in &args.evaluation_seeds {
        for contexts in &condition_groups {
            let paths = contexts
                .iter()
                .map(|context| context.action_indices.clone())
                .collect::<Vec<_>>();
            let result = solver.evaluate_profile_conditioned_preflop(
                args.condition_samples,
                seed,
                ProfileVariant::default(),
                session.threads,
                &paths,
            )?;
            preflop_conditional_evaluations.push(PreflopConditionalEvaluationOutput {
                seed,
                prefixes: contexts.clone(),
                result,
            });
        }
    }

    let mut endpoint_deviations = Vec::new();
    let mut endpoint_counterfactual_deviations = Vec::new();
    for context in endpoint_contexts {
        let config = endpoint_config.as_ref().expect("validated endpoint budget");
        for &target in args.endpoint_target.targets() {
            let started = Instant::now();
            match target {
                EndpointTarget::ActualPrefix => {
                    let result = solver.evaluate_endpoint_deviation_preflop(
                        &context.action_indices,
                        ProfileVariant::default(),
                        session.threads,
                        config,
                    )?;
                    endpoint_deviations.push(EndpointDeviationOutput {
                    context: context.clone(),
                    elapsed_secs: started.elapsed().as_secs_f64(),
                    result,
                    interpretation: "One endpoint-only action table fitted by own information key on independent worlds and frozen before held-out evaluation. Later decisions of every seat remain baseline. Signed conditional gain includes unsupported keys in the full prefix-weight denominator. Read retained-key weight coverage and ESS beside gain; low coverage cannot establish a strong baseline. This separate diagnostic is not a full best response or global exploitability measurement.",
                });
                }
                EndpointTarget::OpponentsPrefix => {
                    let result = solver.evaluate_endpoint_deviation_preflop_counterfactual(
                        &context.action_indices,
                        ProfileVariant::default(),
                        session.threads,
                        config,
                    )?;
                    endpoint_counterfactual_deviations.push(CounterfactualEndpointDeviationOutput {
                    context: context.clone(),
                    elapsed_secs: started.elapsed().as_secs_f64(),
                    result,
                    interpretation: "Chance and opponents' prefix target; the endpoint actor's own earlier action probabilities are excluded. This independently fitted table, every fit/held-out weight and its signed gain use the counterfactual population, including zero-own-reach keys and unsupported-key weight. Actual-prefix gains and relative-weight means describe a different population and cannot be ranked directly against these aggregates. Only the first endpoint action changes; all later decisions remain baseline. A self-normalized read-only diagnostic, not an unbiased CFR update, root reach, full best response or equilibrium certificate.",
                });
                }
                EndpointTarget::Both => unreachable!("combined target expanded before fitting"),
            }
        }
    }

    let preflop_deviation = preflop_deviation_config
        .as_ref()
        .map(|config| {
            let mode = if args.preflop_deviation_retention_gate {
                multiway::PreflopDeviationFitMode::RetentionGated
            } else {
                multiway::PreflopDeviationFitMode::LocalRegretMatching
            };
            solver.evaluate_preflop_deviation_with_fit_mode(
                ProfileVariant::default(),
                session.threads,
                config,
                mode,
            )
        })
        .transpose()?;

    // Preserve the complete legacy single-endpoint JSON shape. Multiple
    // endpoints share this restored solver and use separately identified rows.
    let endpoint_deviation = if endpoint_deviations.len() == 1 {
        endpoint_deviations.pop()
    } else {
        None
    };
    let endpoint_counterfactual_deviation = if endpoint_counterfactual_deviations.len() == 1 {
        endpoint_counterfactual_deviations.pop()
    } else {
        None
    };

    let has_preflop_deviation = preflop_deviation.is_some();
    let output = AuditOutput {
        schema_version: "solvers.multiway-checkpoint-audit/v1",
        checkpoint: args.checkpoint.map(|path| path.display().to_string()),
        config: args.config.display().to_string(),
        sweeps: solver.completed_sweeps(),
        solver_state_version: multiway::solver::SOLVER_STATE_VERSION,
        configuration_fingerprint: hex(solver.configuration_fingerprint()),
        abstraction_fingerprint: hex(solver.abstraction_fingerprint()),
        policy_arena: solver.policy_arena_allocation(),
        construction_elapsed_secs,
        evaluation_samples_per_seed: args.samples,
        evaluation_seeds: args.evaluation_seeds,
        deviator_training: DeviatorTrainingOutput {
            seed: args.br_seed,
            traversals_per_seat: args.br_traversals,
            elapsed_secs: training_elapsed,
            coverage,
        },
        evaluations,
        nodes,
        coverage_evaluations,
        conditional_evaluations,
        preflop_conditional_evaluations,
        fresh_training,
        policy_support,
        preflop_support_census,
        preflop_deviation,
        endpoint_deviation,
        endpoint_counterfactual_deviation,
        endpoint_deviations,
        endpoint_counterfactual_deviations,
        interpretation: if has_preflop_deviation {
            "The ordinary evaluations array covers two fixed candidate deviations per seat with regret-greedy fallback, and deviatorTraining reports fitting coverage. The separate preflopDeviation object fits all own preflop decisions with frozen postflop continuation and baseline fallback; it reports signed paired gains and held-out replay coverage for every seat and seed. Its pointwise intervals are not simultaneous guarantees. Node conditional action rates are self-normalized reach-weighted ratio estimates and can describe a composite fallback policy. These diagnostics are not a full best response, exploitability, or Nash certificate. Evaluation seeds are reported separately and are not selected or pooled."
        } else {
            "Held-out gains cover two fixed candidate deviations per seat; the trained candidate is used where it retained an action and otherwise falls back to the main regret-greedy candidate. Deviator coverage in this document is training coverage, not held-out replay coverage. Node conditional action rates are self-normalized reach-weighted ratio estimates, are not claimed finite-sample unbiased, and represent a composite policy whenever their fallback reach-weight fraction is positive. These results are diagnostics, not a full best response, exploitability, or Nash certificate. Evaluation seeds are reported separately and are not selected or pooled."
        },
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn validate_research_build(args: &Args) -> Result<()> {
    if args.enumerate_raised_preflop && !cfg!(feature = "research-regret-sampling") {
        bail!("--enumerate-raised-preflop requires the research-regret-sampling build feature");
    }
    Ok(())
}

fn resolve_endpoints(
    solver: &ProductionSolver,
    requested: &[String],
    target: EndpointTarget,
) -> Result<Vec<ConditionalPrefixContext>> {
    if requested.len() > 8 {
        bail!("at most eight --endpoint-prefix values are supported");
    }
    let mut histories = BTreeSet::new();
    requested
        .iter()
        .map(|path| {
            let context = resolve_endpoint(solver, path)?;
            validate_endpoint_target(&context, target)?;
            if !histories.insert(context.history.clone()) {
                bail!("duplicate resolved endpoint history");
            }
            Ok(context)
        })
        .collect()
}

fn validate_endpoint_target(
    context: &ConditionalPrefixContext,
    target: EndpointTarget,
) -> Result<()> {
    if target != EndpointTarget::ActualPrefix && context.street != Street::Preflop {
        bail!("--endpoint-target opponents-prefix/both requires a preflop decision");
    }
    Ok(())
}

fn preflop_deviation_config(args: &Args) -> Result<Option<multiway::PreflopDeviationConfig>> {
    let Some(fit_traversals_per_seat) = args.preflop_deviation_fit_traversals else {
        return Ok(None);
    };
    let config = multiway::PreflopDeviationConfig {
        fit_traversals_per_seat,
        fit_seed: args
            .preflop_deviation_fit_seed
            .context("--preflop-deviation-fit-seed is required")?,
        held_out_samples: args
            .preflop_deviation_samples
            .context("--preflop-deviation-samples is required")?,
        held_out_seeds: args.preflop_deviation_seeds.clone(),
    };
    config.validate()?;
    Ok(Some(config))
}

fn endpoint_config(args: &Args) -> Result<Option<multiway::EndpointDeviationConfig>> {
    if args.endpoint_prefix.is_empty() {
        return Ok(None);
    }
    if args.endpoint_prefix.len() > 8 {
        bail!("at most eight --endpoint-prefix values are supported");
    }
    let fit_seed = args
        .endpoint_fit_seed
        .context("--endpoint-fit-seed is required")?;
    let unique = args.endpoint_seeds.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != args.endpoint_seeds.len()
        || unique.is_empty()
        || unique.len() > 64
        || unique.contains(&fit_seed)
    {
        bail!("--endpoint-seeds requires 1..=64 unique seeds distinct from --endpoint-fit-seed");
    }
    if !args.endpoint_min_fit_ess.is_finite() || args.endpoint_min_fit_ess < 2.0 {
        bail!("--endpoint-min-fit-ess must be finite and at least 2");
    }
    Ok(Some(multiway::EndpointDeviationConfig {
        fit_samples: args
            .endpoint_fit_samples
            .context("--endpoint-fit-samples is required")?,
        fit_seed,
        held_out_samples: args
            .endpoint_samples
            .context("--endpoint-samples is required")?,
        held_out_seeds: args.endpoint_seeds.clone(),
        min_fit_ess: args.endpoint_min_fit_ess,
    }))
}

fn resolve_endpoint(
    solver: &ProductionSolver,
    requested: &str,
) -> Result<ConditionalPrefixContext> {
    let context = resolve_condition_prefixes(solver, &[requested.to_owned()])?
        .pop()
        .context("missing endpoint context")?;
    if context.action_indices.len() >= solver.config().max_traversal_depth as usize {
        bail!("--endpoint-prefix exceeds the configured traversal depth");
    }
    let node = solver
        .public_node_view(resolve_node(solver, requested)?)
        .context("missing endpoint public node")?;
    if !(1..=8).contains(&node.actions.len()) {
        bail!("--endpoint-prefix requires a menu of 1..=8 actions");
    }
    if solver.game().recall_mode() != multiway::RecallMode::Street {
        bail!("--endpoint-prefix requires current-street recall");
    }
    Ok(context)
}

fn resolve_coverage_prefixes(
    solver: &ProductionSolver,
    requested: &[String],
) -> Result<Vec<HistoryKey>> {
    let mut histories = Vec::with_capacity(requested.len());
    for path in requested {
        let history = resolve_node(solver, path)?;
        if solver.public_node_view(history).is_none() {
            bail!("coverage prefix {path:?} is not a known public decision node");
        }
        if histories.contains(&history) {
            bail!("coverage prefixes resolve to the same public history: {path:?}");
        }
        histories.push(history);
    }
    Ok(histories)
}

fn validate_condition_parameters(
    prefixes: &[String],
    samples: u64,
    sampler: ConditionSampler,
) -> Result<()> {
    if prefixes.len() > 64 {
        bail!("at most 64 condition prefixes are supported");
    }
    if sampler == ConditionSampler::PreflopProposal && prefixes.is_empty() {
        bail!(
            "--condition-sampler preflop-proposal requires --condition-prefix and --condition-samples"
        );
    }
    match (prefixes.is_empty(), samples) {
        (true, 0) => Ok(()),
        (true, _) => bail!("--condition-samples requires at least one --condition-prefix"),
        (false, 0 | 1) => bail!("--condition-prefix requires --condition-samples of at least 2"),
        (false, _) => Ok(()),
    }
}

fn resolve_condition_prefixes(
    solver: &ProductionSolver,
    requested: &[String],
) -> Result<Vec<ConditionalPrefixContext>> {
    let mut contexts = Vec::with_capacity(requested.len());
    let mut histories = BTreeSet::new();
    for requested in requested {
        let history = resolve_node(solver, requested)?;
        let node = solver.public_node_view(history).ok_or_else(|| {
            anyhow!("condition prefix {requested:?} is not a known public decision node")
        })?;
        if !histories.insert(history) {
            bail!("condition prefixes resolve to the same public history: {requested:?}");
        }
        let path = target_path(solver, history)?;
        contexts.push(ConditionalPrefixContext {
            requested: requested.clone(),
            history: hex(history.0),
            action_indices: path.iter().map(|step| step.action_index).collect(),
            action_labels: path.into_iter().map(|step| step.action_label).collect(),
            actor: node.actor,
            street: node.street,
            active_opponents: node.active_opponents,
        });
    }
    Ok(contexts)
}

fn export_policy_support(
    solver: &ProductionSolver,
    context: ConditionalPrefixContext,
) -> Result<PolicySupportNode> {
    let game = solver.game();
    if game.recall_mode() != multiway::RecallMode::Street {
        bail!("policy support requires current-street dense storage");
    }
    let mut state = game.root_state();
    let mut history = HistoryKey::ROOT;
    for &action in &context.action_indices {
        let actor = game
            .actor(&state)
            .context("support path reached a terminal")?;
        let actions = game.node_actions(&state);
        if action >= game.num_actions_of(&actions) {
            bail!("support path contains an invalid action");
        }
        state = game.next_state_with(&state, &actions, action);
        history = history.child(actor, action);
    }
    let actor = game.actor(&state).context("support endpoint is terminal")?;
    let dense_context = game.dense_node_context(&state);
    if hex(history.0) != context.history
        || actor != usize::from(context.actor)
        || dense_context.street != context.street
        || dense_context.active_opponents != context.active_opponents
    {
        bail!("support context does not match the replayed state");
    }
    let expected_buckets = game.bucket_count(context.street, dense_context.bucket_active_opponents);
    let actions = game.node_actions(&state);
    let action_labels = action_labels(game, &actions);
    let mut output = PolicySupportNode {
        bucket_active_opponents: dense_context.bucket_active_opponents,
        expected_buckets,
        stored_buckets: 0,
        nonzero_regret_buckets: 0,
        positive_regret_buckets: 0,
        average_buckets: 0,
        average_and_nonzero_regret_buckets: 0,
        action_labels,
        rows: Vec::new(),
        context,
    };
    for bucket in 0..expected_buckets {
        let mut bucket_path = [multiway::solver::UNREACHED_BUCKET; 4];
        bucket_path[output.context.street as usize] = bucket;
        let key = InfoKey {
            history,
            player: output.context.actor,
            street: output.context.street as u8,
            active_opponents: output.context.active_opponents,
            bucket_path,
        };
        let column = solver.policy(key);
        let row = support_row(bucket, column.as_ref(), &output.action_labels)?;
        if let Some(regrets) = &row.regrets {
            let nonzero = regrets.iter().any(|&value| value != 0.0);
            let positive = regrets.iter().any(|&value| value > 0.0);
            let average = row.strategy_mass.is_some_and(|mass| mass > 0.0);
            output.stored_buckets += 1;
            output.nonzero_regret_buckets += usize::from(nonzero);
            output.positive_regret_buckets += usize::from(positive);
            output.average_buckets += usize::from(average);
            output.average_and_nonzero_regret_buckets += usize::from(average && nonzero);
        }
        output.rows.push(row);
    }
    Ok(output)
}

fn support_row(
    bucket: u32,
    column: Option<&multiway::PolicyColumn>,
    labels: &[String],
) -> Result<PolicySupportRow> {
    let Some(column) = column else {
        return Ok(PolicySupportRow {
            bucket,
            status: "missing",
            regrets: None,
            strategy_sum: None,
            strategy_mass: None,
            current_strategy: None,
            average_strategy: None,
        });
    };
    if column.action_labels != labels
        || column.regrets.len() != labels.len()
        || column.strategy_sum.len() != labels.len()
        || column
            .regrets
            .iter()
            .chain(&column.strategy_sum)
            .any(|value| !value.is_finite())
        || column.strategy_sum.iter().any(|&value| value < 0.0)
    {
        bail!("invalid stored policy support column");
    }
    let mass = column
        .strategy_sum
        .iter()
        .map(|&value| f64::from(value))
        .sum::<f64>();
    let current = column.current_strategy();
    let average = (mass > 0.0).then(|| column.average_strategy());
    for probabilities in std::iter::once(&current).chain(average.iter()) {
        if probabilities
            .iter()
            .any(|&value| !value.is_finite() || value < 0.0)
            || !probabilities.iter().any(|&value| value > 0.0)
        {
            bail!("stored policy support normalization overflowed");
        }
    }
    Ok(PolicySupportRow {
        bucket,
        status: if column.regrets.iter().any(|&value| value > 0.0) {
            "stored-positive-regrets"
        } else if column.regrets.iter().any(|&value| value != 0.0) {
            "stored-nonpositive-regrets"
        } else {
            "stored-zero-regrets"
        },
        regrets: Some(column.regrets.clone()),
        strategy_sum: Some(column.strategy_sum.clone()),
        strategy_mass: Some(mass),
        current_strategy: Some(current),
        average_strategy: average,
    })
}

fn group_preflop_condition_prefixes(
    solver: &ProductionSolver,
    contexts: &[ConditionalPrefixContext],
) -> Result<Vec<Vec<ConditionalPrefixContext>>> {
    let game = solver.game();
    let mut group_indices = BTreeMap::new();
    let mut groups: Vec<Vec<ConditionalPrefixContext>> = Vec::new();
    for context in contexts {
        if context.street == Street::Preflop {
            bail!(
                "--condition-sampler preflop-proposal requires postflop decision prefixes: {:?}",
                context.requested
            );
        }
        let mut state = game.root_state();
        let mut trunk = Vec::new();
        for &action in &context.action_indices {
            if state.street != Street::Preflop {
                break;
            }
            if game.actor(&state).is_none() {
                bail!("condition prefix reaches a terminal before postflop");
            }
            let actions = game.node_actions(&state);
            if action >= game.num_actions_of(&actions) {
                bail!("condition prefix has an invalid preflop action index");
            }
            state = game.next_state_with(&state, &actions, action);
            trunk.push(action);
        }
        if state.street == Street::Preflop || game.actor(&state).is_none() {
            bail!("condition prefix does not reach a postflop decision");
        }
        let next_group = groups.len();
        let group = *group_indices.entry(trunk).or_insert(next_group);
        if group == next_group {
            groups.push(Vec::new());
        }
        groups[group].push(context.clone());
    }
    Ok(groups)
}

fn validate_parameters(
    evaluation_seeds: &[u64],
    samples: u64,
    br_traversals: u64,
    threads: Option<usize>,
    node_frequency_samples: u64,
) -> Result<()> {
    if samples < 2 {
        bail!("--samples must be at least 2 to estimate sampling uncertainty");
    }
    if br_traversals == 0 {
        bail!("--br-traversals must be positive");
    }
    if threads == Some(0) {
        bail!("--threads must be positive");
    }
    if node_frequency_samples == 1 {
        bail!("--node-frequency-samples must be zero or at least 2");
    }
    if evaluation_seeds.is_empty() {
        bail!("at least one --evaluation-seeds value is required");
    }
    let unique = evaluation_seeds.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != evaluation_seeds.len() {
        bail!("--evaluation-seeds must not contain duplicates");
    }
    Ok(())
}

fn export_preflop_node(
    solver: &ProductionSolver,
    requested: &str,
    frequency_config: NodeFrequencyConfig,
) -> Result<NodeOutput> {
    let history = resolve_node(solver, requested)?;
    let node = solver
        .public_node_view(history)
        .ok_or_else(|| anyhow!("{} does not resolve to a decision node", requested))?;
    ensure_preflop(node.street, requested)?;

    let mut by_bucket = BTreeMap::new();
    for (key, labels, probabilities, mass) in solver.strategies_at_with_mass(history) {
        if key.player != node.actor || key.street != Street::Preflop.index() as u8 {
            continue;
        }
        let previous = by_bucket.insert(key.bucket_path[0], (labels, probabilities, mass));
        if previous.is_some() {
            bail!(
                "duplicate preflop bucket {} at {}",
                key.bucket_path[0],
                requested
            );
        }
    }

    let hands = (0..cards::NUM_CLASSES)
        .map(|class| -> Result<HandOutput> {
            let bucket = class as u32;
            match by_bucket.get(&bucket) {
                Some((labels, probabilities, mass)) => {
                    let (status, strategy) = export_average_strategy(labels, probabilities, *mass)?;
                    Ok(HandOutput {
                        hand: preflop::class_label(class),
                        bucket,
                        status,
                        strategy_mass: *mass,
                        strategy,
                    })
                }
                None => Ok(HandOutput {
                    hand: preflop::class_label(class),
                    bucket,
                    status: "unvisited",
                    strategy_mass: 0.0,
                    strategy: None,
                }),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let actions = node
        .actions
        .into_iter()
        .map(|action| action.label)
        .collect::<Vec<_>>();
    let frequency_started = Instant::now();
    let frequency = (frequency_config.samples > 0)
        .then(|| estimate_node_frequency(solver, history, &actions, frequency_config))
        .transpose()?;
    let frequency_elapsed_secs = frequency
        .as_ref()
        .map(|_| frequency_started.elapsed().as_secs_f64());

    Ok(NodeOutput {
        requested: requested.to_owned(),
        history: hex(history.0),
        actor: node.actor,
        active_opponents: node.active_opponents,
        actions,
        hands,
        frequency,
        frequency_elapsed_secs,
    })
}

fn export_average_strategy(
    labels: &[String],
    probabilities: &[f32],
    mass: f64,
) -> Result<(&'static str, Option<BTreeMap<String, f32>>)> {
    if !mass.is_finite() || mass < 0.0 {
        bail!("invalid average-strategy mass {mass}");
    }
    if mass == 0.0 {
        // `strategies_at_with_mass` exposes the current regret-matched
        // fallback for a touched zero-mass column. It is useful to a live UI,
        // but it must not be presented as the checkpoint's average policy.
        return Ok(("current-regret-fallback-omitted", None));
    }
    if labels.len() != probabilities.len() {
        bail!(
            "strategy label/probability length mismatch: {} labels, {} probabilities",
            labels.len(),
            probabilities.len()
        );
    }
    let strategy = labels
        .iter()
        .cloned()
        .zip(probabilities.iter().copied())
        .collect::<BTreeMap<_, _>>();
    if strategy.len() != labels.len() {
        bail!("strategy contains duplicate action labels");
    }
    Ok(("average-observed", Some(strategy)))
}

fn estimate_node_frequency(
    solver: &ProductionSolver,
    target: HistoryKey,
    target_labels: &[String],
    config: NodeFrequencyConfig,
) -> Result<NodeFrequencyOutput> {
    if config.samples < 2 {
        bail!("node-frequency estimation requires at least two samples");
    }
    let prepared = prepare_frequency_path(solver, target, target_labels)?;
    let game = solver.game();
    let mut total_deal_attempts = 0u64;
    let mut moments = NodeFrequencyMoments::new(target_labels.len());

    for sample_id in 0..config.samples {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.node-frequency-audit.v1");
        hasher.update(&config.seed.to_le_bytes());
        hasher.update(&sample_id.to_le_bytes());
        let mut rng = ChaCha20Rng::from_seed(*hasher.finalize().as_bytes());
        let sample = solver.sampler().sample_counted(&mut rng)?;
        total_deal_attempts = total_deal_attempts
            .checked_add(u64::from(sample.attempts))
            .context("node-frequency deal-attempt counter overflow")?;
        let world = sample.world;
        let mut reach = 1.0;
        let mut fallbacks = FallbackFlags::default();

        // Keep the original path multiplication and moment-observation order.
        // Only public state/menu reconstruction and immutable normalization
        // have moved out of the physical-world loop.
        for (node, action_index) in &prepared.steps {
            let (strategy, source) = node.strategy_for_world(game, &world);
            fallbacks.merge(source);
            reach *= strategy[*action_index];
        }
        let (strategy, source) = prepared.target.strategy_for_world(game, &world);
        fallbacks.merge(source);
        moments.observe(reach, strategy, fallbacks)?;
    }

    moments.finish(
        target_labels,
        config.samples,
        config.seed,
        total_deal_attempts,
    )
}

struct PreparedFrequencyPolicy {
    probabilities: Vec<f64>,
    source: FallbackFlags,
}

struct PreparedFrequencyNode {
    state: multiway::BettingState,
    history: HistoryKey,
    actor: usize,
    policies: BTreeMap<InfoKey, PreparedFrequencyPolicy>,
    uniform: Vec<f64>,
}

impl PreparedFrequencyNode {
    fn strategy_for_world(
        &self,
        game: &multiway::HoldemGame<MultiwayAbstractionBackend>,
        world: &multiway::SampledWorld,
    ) -> (&[f64], FallbackFlags) {
        let private = game.bucket(&self.state, world, self.actor);
        let key = InfoKey {
            history: self.history,
            player: self.actor as u8,
            street: private.street,
            active_opponents: private.active_opponents,
            bucket_path: private.bucket_path,
        };
        match self.policies.get(&key) {
            Some(policy) => (&policy.probabilities, policy.source),
            None => (
                &self.uniform,
                FallbackFlags {
                    uniform: true,
                    ..FallbackFlags::default()
                },
            ),
        }
    }
}

struct PreparedFrequencyPath {
    steps: Vec<(PreparedFrequencyNode, usize)>,
    target: PreparedFrequencyNode,
}

fn prepare_frequency_node(
    solver: &ProductionSolver,
    stored_policies: &StoredPolicies,
    state: multiway::BettingState,
    history: HistoryKey,
    actor: usize,
    labels: &[String],
) -> Result<PreparedFrequencyNode> {
    let mut policies = BTreeMap::new();
    if let Some(rows) = stored_policies.get(&history) {
        for (&key, row) in rows {
            if row.labels != labels {
                bail!(
                    "checkpoint action labels differ at history {}",
                    hex(history.0)
                );
            }
            // Normalize the exact same f32 source as the replayed estimator,
            // once per immutable key rather than once per sampled world.
            let probabilities = if row.mass > 0.0 {
                normalize_probabilities(&row.probabilities)?
            } else {
                let current = solver.current_strategy(key).ok_or_else(|| {
                    anyhow!("touched policy disappeared at history {}", hex(history.0))
                })?;
                normalize_probabilities(&current)?
            };
            policies.insert(
                key,
                PreparedFrequencyPolicy {
                    probabilities,
                    source: FallbackFlags {
                        current_regret: row.mass == 0.0,
                        uniform: false,
                    },
                },
            );
        }
    }
    let probability = 1.0 / labels.len() as f64;
    Ok(PreparedFrequencyNode {
        state,
        history,
        actor,
        policies,
        uniform: vec![probability; labels.len()],
    })
}

fn prepare_frequency_path(
    solver: &ProductionSolver,
    target: HistoryKey,
    target_labels: &[String],
) -> Result<PreparedFrequencyPath> {
    let path = target_path(solver, target)?;
    let stored_policies = stored_policies_for_path(solver, &path, target)?;
    let game = solver.game();
    let mut state = game.root_state();
    let mut history = HistoryKey::ROOT;
    let mut steps = Vec::with_capacity(path.len());

    // Holdem's action transitions depend only on public betting state. Cards
    // enter through bucket(state, world, actor), which remains inside the
    // sample loop; no board-specific private information is cached here.
    for step in &path {
        if history != step.parent {
            bail!("target path parent metadata is inconsistent");
        }
        let actor = game.actor(&state).ok_or_else(|| {
            anyhow!(
                "target path reaches a terminal state before action {:?}",
                step.action_label
            )
        })?;
        if actor != step.actor {
            bail!(
                "target path actor changed at history {}: stored {}, runtime {}",
                hex(history.0),
                step.actor,
                actor
            );
        }
        let actions = game.node_actions(&state);
        let labels = action_labels(game, &actions);
        if step.action_index >= labels.len() || labels[step.action_index] != step.action_label {
            bail!(
                "target action metadata changed at history {}",
                hex(history.0)
            );
        }
        let node =
            prepare_frequency_node(solver, &stored_policies, state, history, actor, &labels)?;
        state = game.next_state_with(&node.state, &actions, step.action_index);
        steps.push((node, step.action_index));
        history = history.child(actor, step.action_index);
        if history != step.child {
            bail!("target path child metadata is inconsistent");
        }
    }
    if history != target {
        bail!(
            "resolved target path ended at {}, expected {}",
            hex(history.0),
            hex(target.0)
        );
    }
    let actor = game
        .actor(&state)
        .ok_or_else(|| anyhow!("target {} is terminal", hex(target.0)))?;
    let actions = game.node_actions(&state);
    let labels = action_labels(game, &actions);
    if labels != target_labels {
        bail!("target action labels changed while replaying physical worlds");
    }
    let target = prepare_frequency_node(solver, &stored_policies, state, history, actor, &labels)?;
    Ok(PreparedFrequencyPath { steps, target })
}

// The original per-world replay is a differential oracle for the prepared
// estimator. It deliberately retains its allocations and normalization path.
#[cfg(test)]
fn estimate_node_frequency_replayed(
    solver: &ProductionSolver,
    target: HistoryKey,
    target_labels: &[String],
    config: NodeFrequencyConfig,
) -> Result<NodeFrequencyOutput> {
    if config.samples < 2 {
        bail!("node-frequency estimation requires at least two samples");
    }
    let path = target_path(solver, target)?;
    let stored_policies = stored_policies_for_path(solver, &path, target)?;
    let game = solver.game();
    let mut total_deal_attempts = 0u64;
    let mut moments = NodeFrequencyMoments::new(target_labels.len());

    for sample_id in 0..config.samples {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.node-frequency-audit.v1");
        hasher.update(&config.seed.to_le_bytes());
        hasher.update(&sample_id.to_le_bytes());
        let mut rng = ChaCha20Rng::from_seed(*hasher.finalize().as_bytes());
        let sample = solver.sampler().sample_counted(&mut rng)?;
        total_deal_attempts = total_deal_attempts
            .checked_add(u64::from(sample.attempts))
            .context("node-frequency deal-attempt counter overflow")?;
        let world = sample.world;
        let mut state = game.root_state();
        let mut history = HistoryKey::ROOT;
        let mut reach = 1.0;
        let mut fallbacks = FallbackFlags::default();

        for step in &path {
            if history != step.parent {
                bail!("target path parent metadata is inconsistent");
            }
            let actor = game.actor(&state).ok_or_else(|| {
                anyhow!(
                    "target path reaches a terminal state before action {:?}",
                    step.action_label
                )
            })?;
            if actor != step.actor {
                bail!(
                    "target path actor changed at history {}: stored {}, runtime {}",
                    hex(history.0),
                    step.actor,
                    actor
                );
            }
            let actions = game.node_actions(&state);
            let labels = action_labels(game, &actions);
            if step.action_index >= labels.len() || labels[step.action_index] != step.action_label {
                bail!(
                    "target action metadata changed at history {}",
                    hex(history.0)
                );
            }
            let (strategy, source) = average_policy_for_world(
                solver,
                &stored_policies,
                &state,
                &world,
                history,
                actor,
                &labels,
            )?;
            fallbacks.merge(source);
            reach *= strategy[step.action_index];
            state = game.next_state_with(&state, &actions, step.action_index);
            history = history.child(actor, step.action_index);
            if history != step.child {
                bail!("target path child metadata is inconsistent");
            }
        }
        if history != target {
            bail!(
                "resolved target path ended at {}, expected {}",
                hex(history.0),
                hex(target.0)
            );
        }

        let actor = game
            .actor(&state)
            .ok_or_else(|| anyhow!("target {} is terminal", hex(target.0)))?;
        let actions = game.node_actions(&state);
        let labels = action_labels(game, &actions);
        if labels != target_labels {
            bail!("target action labels changed while replaying physical worlds");
        }
        let (strategy, source) = average_policy_for_world(
            solver,
            &stored_policies,
            &state,
            &world,
            history,
            actor,
            &labels,
        )?;
        fallbacks.merge(source);
        moments.observe(reach, &strategy, fallbacks)?;
    }

    moments.finish(
        target_labels,
        config.samples,
        config.seed,
        total_deal_attempts,
    )
}

#[derive(Clone)]
struct TargetPathStep {
    parent: HistoryKey,
    child: HistoryKey,
    actor: usize,
    action_index: usize,
    action_label: String,
}

fn target_path(solver: &ProductionSolver, target: HistoryKey) -> Result<Vec<TargetPathStep>> {
    let mut child = target;
    let mut reversed = Vec::new();
    while child != HistoryKey::ROOT {
        let entry = solver
            .history_entry(child)
            .ok_or_else(|| anyhow!("cannot resolve target history {}", hex(child.0)))?;
        let actor = usize::from(entry.actor);
        let action_index = usize::try_from(entry.action_index)
            .context("target action index does not fit usize")?;
        if entry.parent.child(actor, action_index) != child {
            bail!("target history metadata fails child-key validation");
        }
        reversed.push(TargetPathStep {
            parent: entry.parent,
            child,
            actor,
            action_index,
            action_label: entry.action_label,
        });
        child = entry.parent;
    }
    reversed.reverse();
    Ok(reversed)
}

#[derive(Clone)]
struct StoredAveragePolicy {
    labels: Vec<String>,
    probabilities: Vec<f32>,
    mass: f64,
}

type StoredPolicies = BTreeMap<HistoryKey, BTreeMap<InfoKey, StoredAveragePolicy>>;

fn stored_policies_for_path(
    solver: &ProductionSolver,
    path: &[TargetPathStep],
    target: HistoryKey,
) -> Result<StoredPolicies> {
    let mut histories = path.iter().map(|step| step.parent).collect::<BTreeSet<_>>();
    histories.insert(target);
    let mut result = BTreeMap::new();
    for history in histories {
        let mut policies = BTreeMap::new();
        for (key, labels, probabilities, mass) in solver.strategies_at_with_mass(history) {
            if !mass.is_finite() || mass < 0.0 {
                bail!(
                    "invalid average-strategy mass {mass} at history {}",
                    hex(history.0)
                );
            }
            let previous = policies.insert(
                key,
                StoredAveragePolicy {
                    labels,
                    probabilities,
                    mass,
                },
            );
            if previous.is_some() {
                bail!("duplicate policy key at history {}", hex(history.0));
            }
        }
        result.insert(history, policies);
    }
    Ok(result)
}

fn action_labels<G: ExternalSamplingGame>(game: &G, actions: &G::Actions) -> Vec<String> {
    (0..game.num_actions_of(actions))
        .map(|index| game.action_label_of(actions, index))
        .collect()
}

#[cfg(test)]
fn average_policy_for_world(
    solver: &ProductionSolver,
    stored_policies: &StoredPolicies,
    state: &multiway::BettingState,
    world: &multiway::SampledWorld,
    history: HistoryKey,
    actor: usize,
    labels: &[String],
) -> Result<(Vec<f64>, FallbackFlags)> {
    let game = solver.game();
    let private = game.bucket(state, world, actor);
    let key = InfoKey {
        history,
        player: actor as u8,
        street: private.street,
        active_opponents: private.active_opponents,
        bucket_path: private.bucket_path,
    };
    let row = stored_policies
        .get(&history)
        .and_then(|policies| policies.get(&key));
    let Some(row) = row else {
        let probability = 1.0 / labels.len() as f64;
        return Ok((
            vec![probability; labels.len()],
            FallbackFlags {
                uniform: true,
                ..FallbackFlags::default()
            },
        ));
    };
    if row.labels != labels {
        bail!(
            "checkpoint action labels differ at history {}",
            hex(history.0)
        );
    }
    let probabilities = if row.mass > 0.0 {
        row.probabilities.clone()
    } else {
        solver
            .current_strategy(key)
            .ok_or_else(|| anyhow!("touched policy disappeared at history {}", hex(history.0)))?
    };
    let probabilities = normalize_probabilities(&probabilities)?;
    Ok((
        probabilities,
        FallbackFlags {
            current_regret: row.mass == 0.0,
            uniform: false,
        },
    ))
}

fn normalize_probabilities(probabilities: &[f32]) -> Result<Vec<f64>> {
    let mut normalized = Vec::with_capacity(probabilities.len());
    let mut total = 0.0;
    for &probability in probabilities {
        if !probability.is_finite() || probability < 0.0 {
            bail!("invalid policy probability {probability}");
        }
        let probability = f64::from(probability);
        total += probability;
        normalized.push(probability);
    }
    if !total.is_finite() || total <= 0.0 {
        bail!("policy probability mass must be finite and positive");
    }
    for probability in &mut normalized {
        *probability /= total;
    }
    Ok(normalized)
}

impl FallbackFlags {
    fn merge(&mut self, other: Self) {
        self.current_regret |= other.current_regret;
        self.uniform |= other.uniform;
    }

    fn any(self) -> bool {
        self.current_regret || self.uniform
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let next = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.correction += (self.sum - next) + value;
        } else {
            self.correction += (value - next) + self.sum;
        }
        self.sum = next;
    }

    fn total(self) -> f64 {
        self.sum + self.correction
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ActionMoments {
    numerator: CompensatedSum,
    numerator_squared: CompensatedSum,
    numerator_times_reach: CompensatedSum,
}

struct NodeFrequencyMoments {
    observations: u64,
    reach: CompensatedSum,
    reach_squared: CompensatedSum,
    actions: Vec<ActionMoments>,
    any_fallback_reach: CompensatedSum,
    current_regret_reach: CompensatedSum,
    uniform_reach: CompensatedSum,
}

impl NodeFrequencyMoments {
    fn new(actions: usize) -> Self {
        Self {
            observations: 0,
            reach: CompensatedSum::default(),
            reach_squared: CompensatedSum::default(),
            actions: vec![ActionMoments::default(); actions],
            any_fallback_reach: CompensatedSum::default(),
            current_regret_reach: CompensatedSum::default(),
            uniform_reach: CompensatedSum::default(),
        }
    }

    fn observe(
        &mut self,
        reach: f64,
        probabilities: &[f64],
        fallbacks: FallbackFlags,
    ) -> Result<()> {
        if !reach.is_finite() || !(0.0..=1.0).contains(&reach) {
            bail!("invalid public-history reach weight {reach}");
        }
        if probabilities.len() != self.actions.len() {
            bail!("target strategy length changed across physical worlds");
        }
        self.observations = self
            .observations
            .checked_add(1)
            .context("node-frequency sample counter overflow")?;
        self.reach.add(reach);
        self.reach_squared.add(reach * reach);
        for (moments, &probability) in self.actions.iter_mut().zip(probabilities) {
            if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
                bail!("invalid target action probability {probability}");
            }
            let numerator = reach * probability;
            moments.numerator.add(numerator);
            moments.numerator_squared.add(numerator * numerator);
            moments.numerator_times_reach.add(numerator * reach);
        }
        if fallbacks.any() {
            self.any_fallback_reach.add(reach);
        }
        if fallbacks.current_regret {
            self.current_regret_reach.add(reach);
        }
        if fallbacks.uniform {
            self.uniform_reach.add(reach);
        }
        Ok(())
    }

    fn finish(
        self,
        labels: &[String],
        samples: u64,
        seed: u64,
        total_deal_attempts: u64,
    ) -> Result<NodeFrequencyOutput> {
        if self.observations != samples {
            bail!("node-frequency observation count mismatch");
        }
        let n = samples as f64;
        let denominator = self.reach.total();
        let reach_squared = self.reach_squared.total();
        let reach_variance_numerator = nonnegative_roundoff(
            reach_squared - denominator * denominator / n,
            reach_squared + denominator * denominator / n,
        )?;
        let reach_standard_error = (reach_variance_numerator / ((n - 1.0) * n)).sqrt();
        let reach_probability_estimate = Estimate {
            estimate: (denominator / n).clamp(0.0, 1.0),
            standard_error: reach_standard_error,
        };
        let effective_sample_size = if reach_squared == 0.0 {
            0.0
        } else {
            (denominator * denominator / reach_squared).clamp(0.0, n)
        };

        let conditional_action_rates = if denominator == 0.0 {
            None
        } else {
            let mut rates = BTreeMap::new();
            for (label, moments) in labels.iter().cloned().zip(self.actions) {
                let numerator = moments.numerator.total();
                let estimate = (numerator / denominator).clamp(0.0, 1.0);
                let numerator_squared = moments.numerator_squared.total();
                let numerator_times_reach = moments.numerator_times_reach.total();
                let residual_squared = nonnegative_roundoff(
                    numerator_squared - 2.0 * estimate * numerator_times_reach
                        + estimate * estimate * reach_squared,
                    numerator_squared
                        + 2.0 * estimate.abs() * numerator_times_reach.abs()
                        + estimate * estimate * reach_squared,
                )?;
                let standard_error = (n * residual_squared / (n - 1.0)).sqrt() / denominator;
                if rates
                    .insert(
                        label,
                        Estimate {
                            estimate,
                            standard_error,
                        },
                    )
                    .is_some()
                {
                    bail!("target contains duplicate action labels");
                }
            }
            Some(rates)
        };

        let fraction = |sum: CompensatedSum| {
            (denominator > 0.0).then(|| (sum.total() / denominator).clamp(0.0, 1.0))
        };
        Ok(NodeFrequencyOutput {
            sample_count: samples,
            seed,
            total_deal_attempts,
            conditional_action_rates,
            reach_probability_estimate,
            effective_sample_size,
            fallback_reach_weight_fraction: FallbackReachWeightFraction {
                any: fraction(self.any_fallback_reach),
                current_regret: fraction(self.current_regret_reach),
                uniform: fraction(self.uniform_reach),
                categories_may_overlap: true,
            },
            estimator: "self-normalized reach-weighted ratio; action-rate standard errors use a first-order delta method and the ratio estimate is not claimed unbiased",
        })
    }
}

fn nonnegative_roundoff(value: f64, scale: f64) -> Result<f64> {
    if value >= 0.0 {
        return Ok(value);
    }
    let tolerance = 64.0 * f64::EPSILON * scale.max(1.0);
    if value >= -tolerance {
        return Ok(0.0);
    }
    bail!("negative second-moment residual {value} exceeds roundoff tolerance {tolerance}")
}

fn resolve_node(solver: &ProductionSolver, requested: &str) -> Result<HistoryKey> {
    let requested = requested.trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case("root") {
        return Ok(HistoryKey::ROOT);
    }
    if !requested.contains('/') && requested.len() == 32 {
        return parse_history_key(requested);
    }

    let mut current = HistoryKey::ROOT;
    for segment in requested.split('/').filter(|segment| !segment.is_empty()) {
        let node = solver.public_node_view(current).ok_or_else(|| {
            anyhow!(
                "history {} has no decision node before segment {segment:?}",
                hex(current.0)
            )
        })?;
        let labels = node
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>();
        let action = select_action(segment, &labels)?;
        current = current.child(node.actor as usize, action);
    }
    Ok(current)
}

fn select_action(segment: &str, labels: &[&str]) -> Result<usize> {
    if let Ok(index) = segment.parse::<usize>() {
        if index < labels.len() {
            return Ok(index);
        }
        bail!("action index {index} is outside 0..{}", labels.len());
    }
    labels
        .iter()
        .position(|label| *label == segment)
        .ok_or_else(|| anyhow!("unknown action {segment:?}; expected one of {labels:?}"))
}

fn ensure_preflop(street: Street, requested: &str) -> Result<()> {
    if street != Street::Preflop {
        bail!(
            "node {requested:?} is on {street:?}; hand export currently supports preflop nodes only"
        );
    }
    Ok(())
}

fn parse_history_key(raw: &str) -> Result<HistoryKey> {
    if raw.len() != 32 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("history keys must contain exactly 32 hexadecimal digits");
    }
    let mut key = [0u8; 16];
    for (index, chunk) in raw.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(chunk).expect("hex input is ASCII");
        key[index] = u8::from_str_radix(pair, 16)?;
    }
    Ok(HistoryKey(key))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflop_deviation_cli_requires_complete_disjoint_budgets() {
        let base = [
            "audit",
            "--config",
            "unused.toml",
            "--checkpoint",
            "unused.mwckpt",
        ];
        let parse =
            |extra: &[&str]| Args::try_parse_from(base.into_iter().chain(extra.iter().copied()));
        assert!(
            preflop_deviation_config(&parse(&[]).unwrap())
                .unwrap()
                .is_none()
        );
        let complete = [
            "--preflop-deviation-fit-traversals",
            "128",
            "--preflop-deviation-fit-seed",
            "601",
            "--preflop-deviation-samples",
            "4097",
            "--preflop-deviation-seeds",
            "701,702",
        ];
        for missing in 0..4 {
            let incomplete = complete
                .iter()
                .enumerate()
                .filter_map(|(index, value)| (index / 2 != missing).then_some(*value))
                .collect::<Vec<_>>();
            assert!(parse(&incomplete).is_err());
            assert!(parse(&complete[missing * 2..missing * 2 + 2]).is_err());
        }
        let config = preflop_deviation_config(&parse(&complete).unwrap())
            .unwrap()
            .unwrap();
        assert!(!parse(&complete).unwrap().preflop_deviation_retention_gate);
        assert!(parse(&["--preflop-deviation-retention-gate"]).is_err());
        let with_gate = complete
            .into_iter()
            .chain(["--preflop-deviation-retention-gate"])
            .collect::<Vec<_>>();
        assert!(parse(&with_gate).unwrap().preflop_deviation_retention_gate);
        assert_eq!(
            preflop_deviation_config(&parse(&with_gate).unwrap()).unwrap(),
            Some(config.clone())
        );
        assert_eq!(
            config,
            multiway::PreflopDeviationConfig {
                fit_traversals_per_seat: 128,
                fit_seed: 601,
                held_out_samples: 4097,
                held_out_seeds: vec![701, 702],
            }
        );
        for (index, value) in [(1, "0"), (5, "0"), (5, "1"), (5, "18446744073709551616")] {
            let mut invalid = complete;
            invalid[index] = value;
            assert!(parse(&invalid).is_err());
        }
        for seeds in ["601", "701,701"] {
            let mut invalid = complete;
            invalid[7] = seeds;
            assert!(preflop_deviation_config(&parse(&invalid).unwrap()).is_err());
        }
        let many_seeds = (700..765)
            .map(|seed| seed.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut invalid = complete;
        invalid[7] = &many_seeds;
        assert!(preflop_deviation_config(&parse(&invalid).unwrap()).is_err());
    }

    #[test]
    fn endpoint_both_expands_targets_without_splitting_explicit_budgets() {
        let parse = |target| {
            Args::try_parse_from([
                "audit",
                "--config",
                "unused.toml",
                "--checkpoint",
                "unused.mwckpt",
                "--endpoint-prefix",
                "root",
                "--endpoint-fit-samples",
                "64",
                "--endpoint-fit-seed",
                "501",
                "--endpoint-samples",
                "96",
                "--endpoint-seeds",
                "502,503",
                "--endpoint-min-fit-ess",
                "2",
                "--endpoint-target",
                target,
            ])
            .unwrap()
        };
        let actual = parse("actual-prefix");
        let opponents = parse("opponents-prefix");
        let both = parse("both");
        assert_eq!(
            both.endpoint_target.targets(),
            &[
                EndpointTarget::ActualPrefix,
                EndpointTarget::OpponentsPrefix
            ]
        );
        assert_eq!(
            actual.endpoint_target.targets(),
            &[EndpointTarget::ActualPrefix]
        );
        assert_eq!(
            opponents.endpoint_target.targets(),
            &[EndpointTarget::OpponentsPrefix]
        );
        let config = endpoint_config(&both).unwrap().unwrap();
        assert_eq!(endpoint_config(&actual).unwrap().unwrap(), config);
        assert_eq!(endpoint_config(&opponents).unwrap().unwrap(), config);
        assert_eq!(config.fit_samples, 64);
        assert_eq!(config.held_out_samples, 96);
        assert_eq!(config.fit_seed, 501);
        assert_eq!(config.held_out_seeds, [502, 503]);
        assert!(
            Args::try_parse_from([
                "audit",
                "--config",
                "unused.toml",
                "--checkpoint",
                "unused.mwckpt",
                "--endpoint-target",
                "both",
            ])
            .is_err()
        );
    }

    #[test]
    fn endpoint_both_replays_each_independent_target_and_preserves_solver_state() {
        fn without_clocks(
            mut result: multiway::EndpointDeviationEvaluation,
        ) -> multiway::EndpointDeviationEvaluation {
            result.fit_elapsed_secs = 0.0;
            for held in &mut result.held_out {
                held.elapsed_secs = 0.0;
            }
            result
        }
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let mut solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        solver.run_sweeps_with_threads(4, 2).unwrap();
        // Fold, SB raise, BB reraise: the endpoint actor has already acted.
        // Thus the counterfactual call exercises actual own-factor exclusion,
        // beyond the identical targets at root or an unopened first action.
        let game = solver.game();
        let mut state = game.root_state();
        let mut path_labels = vec![];
        let mut own_prefix_actor = None;
        for step in 0..3 {
            let actions = game.node_actions(&state);
            let labels = action_labels(game, &actions);
            let action = labels
                .iter()
                .position(|label| {
                    if step == 0 {
                        label == "fold"
                    } else {
                        label.starts_with("raise-to:") && (step == 2 || !label.ends_with(":all-in"))
                    }
                })
                .expect("small fixture has a non-all-in open and a reraise");
            if step == 1 {
                own_prefix_actor = game.actor(&state);
            }
            path_labels.push(labels[action].clone());
            state = game.next_state_with(&state, &actions, action);
        }
        assert_eq!(game.actor(&state), own_prefix_actor);
        let context = resolve_endpoints(&solver, &[path_labels.join("/")], EndpointTarget::Both)
            .unwrap()
            .remove(0);
        let config = multiway::EndpointDeviationConfig {
            fit_samples: 64,
            fit_seed: 501,
            held_out_samples: 96,
            held_out_seeds: vec![502, 503],
            min_fit_ess: 2.0,
        };
        let original_config = config.clone();
        let before = solver.snapshot_state();
        let expected_actual = without_clocks(
            solver
                .evaluate_endpoint_deviation_preflop(
                    &context.action_indices,
                    ProfileVariant::default(),
                    1,
                    &config,
                )
                .unwrap(),
        );
        let mut expected_opponents = solver
            .evaluate_endpoint_deviation_preflop_counterfactual(
                &context.action_indices,
                ProfileVariant::default(),
                1,
                &config,
            )
            .unwrap();
        expected_opponents.evaluation = without_clocks(expected_opponents.evaluation);
        assert!(
            expected_opponents
                .own_prefix_probability_by_bucket
                .iter()
                .any(|&value| value < 1.0)
        );
        for &target in EndpointTarget::Both.targets() {
            let result = match target {
                EndpointTarget::ActualPrefix => {
                    let result = without_clocks(
                        solver
                            .evaluate_endpoint_deviation_preflop(
                                &context.action_indices,
                                ProfileVariant::default(),
                                1,
                                &config,
                            )
                            .unwrap(),
                    );
                    assert_eq!(result, expected_actual);
                    result
                }
                EndpointTarget::OpponentsPrefix => {
                    let mut result = solver
                        .evaluate_endpoint_deviation_preflop_counterfactual(
                            &context.action_indices,
                            ProfileVariant::default(),
                            1,
                            &config,
                        )
                        .unwrap();
                    result.evaluation = without_clocks(result.evaluation);
                    assert_eq!(result, expected_opponents);
                    result.evaluation
                }
                EndpointTarget::Both => panic!("combined target was not expanded"),
            };
            assert_eq!(result.config, original_config);
            assert_eq!(result.fit.sampling.samples, config.fit_samples);
            assert_eq!(result.fit.sampling.seed, config.fit_seed);
            assert_eq!(result.held_out.len(), config.held_out_seeds.len());
            for (held, &seed) in result.held_out.iter().zip(&config.held_out_seeds) {
                assert_eq!(held.sampling.samples, config.held_out_samples);
                assert_eq!(held.sampling.seed, seed);
            }
        }
        assert_eq!(config, original_config);
        assert_eq!(solver.snapshot_state(), before);
    }

    #[test]
    fn endpoint_cli_requires_explicit_disjoint_fit_and_held_out_budgets() {
        let base = [
            "audit",
            "--config",
            "unused.toml",
            "--checkpoint",
            "unused.mwckpt",
        ];
        let parse =
            |extra: &[&str]| Args::try_parse_from(base.into_iter().chain(extra.iter().copied()));
        assert!(endpoint_config(&parse(&[]).unwrap()).unwrap().is_none());
        assert_eq!(
            parse(&[]).unwrap().endpoint_target,
            EndpointTarget::ActualPrefix
        );
        assert!(parse(&["--endpoint-target", "opponents-prefix"]).is_err());
        assert!(parse(&["--endpoint-target", "actual-prefix"]).is_err());
        for option in [
            "--endpoint-fit-samples",
            "--endpoint-samples",
            "--endpoint-fit-seed",
            "--endpoint-seeds",
            "--endpoint-min-fit-ess",
        ] {
            assert!(parse(&[option, "64"]).is_err(), "{option}");
        }
        assert!(parse(&["--endpoint-prefix", "check"]).is_err());
        let complete = [
            "--endpoint-prefix",
            "check",
            "--endpoint-fit-samples",
            "4096",
            "--endpoint-samples",
            "8192",
            "--endpoint-fit-seed",
            "601",
            "--endpoint-seeds",
            "701,702",
        ];
        let args = parse(&complete).unwrap();
        for (name, expected) in [
            ("actual-prefix", EndpointTarget::ActualPrefix),
            ("opponents-prefix", EndpointTarget::OpponentsPrefix),
        ] {
            let args = Args::try_parse_from(
                base.into_iter()
                    .chain(complete)
                    .chain(["--endpoint-target", name]),
            )
            .unwrap();
            assert_eq!(args.endpoint_target, expected);
        }
        assert!(
            Args::try_parse_from(
                base.into_iter()
                    .chain(complete)
                    .chain(["--endpoint-target", "counterfactual"])
            )
            .is_err()
        );
        assert_eq!(args.endpoint_prefix, ["check"]);
        let extra = Args::try_parse_from(
            base.into_iter()
                .chain(complete)
                .chain(["--endpoint-prefix", "root"]),
        )
        .unwrap();
        assert_eq!(extra.endpoint_prefix, ["check", "root"]);
        assert!(endpoint_config(&extra).is_ok());
        let excessive = Args::try_parse_from(
            base.into_iter()
                .chain(complete)
                .chain(std::iter::repeat_n(["--endpoint-prefix", "root"], 8).flatten()),
        )
        .unwrap();
        assert!(endpoint_config(&excessive).is_err());
        let config = endpoint_config(&args).unwrap().unwrap();
        assert_eq!(config.fit_samples, 4096);
        assert_eq!(config.held_out_samples, 8192);
        assert_eq!(config.fit_seed, 601);
        assert_eq!(config.held_out_seeds, [701, 702]);
        assert_eq!(config.min_fit_ess, 64.0);
        for (index, value) in [(3, "0"), (5, "1")] {
            let mut invalid = complete;
            invalid[index] = value;
            assert!(parse(&invalid).is_err());
        }
        for seeds in ["601", "701,701"] {
            let mut invalid = complete;
            invalid[9] = seeds;
            assert!(endpoint_config(&parse(&invalid).unwrap()).is_err());
        }
        let too_many = (700..765)
            .map(|seed| seed.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut invalid = complete;
        invalid[9] = &too_many;
        assert!(endpoint_config(&parse(&invalid).unwrap()).is_err());
        for ess in ["NaN", "inf", "1.999"] {
            let args = Args::try_parse_from(
                base.into_iter()
                    .chain(complete)
                    .chain(["--endpoint-min-fit-ess", ess]),
            )
            .unwrap();
            assert!(endpoint_config(&args).is_err());
        }
    }

    #[test]
    fn endpoint_resolves_all_decision_streets_without_mutating_solver() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        let before = solver.snapshot_state();
        assert_eq!(
            resolve_endpoint(&solver, "root").unwrap().street,
            Street::Preflop
        );
        assert!(resolve_endpoint(&solver, "99999").is_err());
        let root_context = resolve_endpoint(&solver, "root").unwrap();
        assert!(
            resolve_endpoints(&solver, &["root".into()], EndpointTarget::OpponentsPrefix).is_ok()
        );
        assert!(
            resolve_endpoints(
                &solver,
                &["root".into(), root_context.history],
                EndpointTarget::ActualPrefix
            )
            .is_err()
        );
        assert!(
            resolve_endpoints(
                &solver,
                &vec!["root".into(); 9],
                EndpointTarget::ActualPrefix
            )
            .is_err()
        );
        let game = solver.game();
        let mut state = game.root_state();
        let mut labels = Vec::new();
        while game.actor(&state).is_some() {
            {
                let requested = if labels.is_empty() {
                    "root".to_owned()
                } else {
                    labels.join("/")
                };
                let context = resolve_endpoint(&solver, &requested).unwrap();
                assert_eq!(context.street, state.street);
                assert!(validate_endpoint_target(&context, EndpointTarget::ActualPrefix).is_ok());
                assert_eq!(
                    validate_endpoint_target(&context, EndpointTarget::OpponentsPrefix).is_ok(),
                    state.street == Street::Preflop
                );
                assert_eq!(
                    validate_endpoint_target(&context, EndpointTarget::Both).is_ok(),
                    state.street == Street::Preflop
                );
                assert_eq!(usize::from(context.actor), game.actor(&state).unwrap());
                assert_eq!(context.active_opponents, 2);
                assert_eq!(
                    resolve_endpoint(&solver, &context.history).unwrap().history,
                    context.history
                );
            }
            let actions = game.node_actions(&state);
            let menu = action_labels(game, &actions);
            let index = menu
                .iter()
                .position(|label| label == "check" || label.starts_with("call:"))
                .unwrap();
            labels.push(menu[index].clone());
            state = game.next_state_with(&state, &actions, index);
        }
        assert!(resolve_endpoint(&solver, &labels.join("/")).is_err());
        assert_eq!(solver.snapshot_state(), before);
    }

    #[test]
    fn fresh_budget_and_checkpoint_are_exclusive_and_required() {
        let base = ["audit", "--config", "unused.toml"];
        let parse =
            |extra: &[&str]| Args::try_parse_from(base.into_iter().chain(extra.iter().copied()));
        assert!(parse(&[]).is_err());
        assert!(parse(&["--fresh-sweeps", "0"]).is_err());
        assert!(parse(&["--fresh-sweeps", "4", "--checkpoint", "unused.mwckpt"]).is_err());
        let fresh = parse(&["--fresh-sweeps", "4", "--support-node", "root"]).unwrap();
        assert_eq!(fresh.fresh_sweeps, Some(4));
        assert!(fresh.checkpoint.is_none());
        assert_eq!(fresh.support_node, ["root"]);
        let restored = parse(&["--checkpoint", "unused.mwckpt"]).unwrap();
        assert!(restored.fresh_sweeps.is_none());
        assert!(restored.support_node.is_empty());
    }

    #[test]
    fn raised_preflop_is_explicit_fresh_and_feature_gated() {
        let parse = |extra: &[&str]| {
            Args::try_parse_from(
                ["audit", "--config", "unused.toml"]
                    .into_iter()
                    .chain(extra.iter().copied()),
            )
        };
        assert!(
            parse(&[
                "--checkpoint",
                "unused.mwckpt",
                "--enumerate-raised-preflop"
            ])
            .is_err()
        );
        assert!(parse(&["--enumerate-raised-preflop"]).is_err());
        let ordinary = parse(&["--fresh-sweeps", "8", "--preflop-support-census"]).unwrap();
        assert!(ordinary.preflop_support_census);
        assert!(!ordinary.enumerate_raised_preflop);
        validate_research_build(&ordinary).unwrap();
        let candidate = parse(&["--fresh-sweeps", "8", "--enumerate-raised-preflop"]).unwrap();
        assert_eq!(
            validate_research_build(&candidate).is_ok(),
            cfg!(feature = "research-regret-sampling")
        );
    }

    #[test]
    fn support_distinguishes_missing_zero_and_nonpositive_regrets_from_average_mass() {
        let labels = vec!["check".to_owned(), "bet:1000".to_owned()];
        let missing = support_row(0, None, &labels).unwrap();
        assert_eq!(missing.status, "missing");
        assert!(missing.regrets.is_none() && missing.strategy_mass.is_none());
        let mut column = multiway::PolicyColumn {
            action_labels: labels.clone(),
            regrets: vec![0.0, 0.0],
            strategy_sum: vec![0.0, 0.0],
        };
        let zero = support_row(0, Some(&column), &labels).unwrap();
        assert_eq!(zero.status, "stored-zero-regrets");
        assert_eq!(zero.current_strategy, Some(vec![0.5, 0.5]));
        assert!(zero.average_strategy.is_none());
        column.regrets = vec![-7.0, 0.0];
        let nonpositive = support_row(0, Some(&column), &labels).unwrap();
        assert_eq!(nonpositive.status, "stored-nonpositive-regrets");
        assert_eq!(zero.current_strategy, nonpositive.current_strategy);
        column.regrets = vec![3.0, 3.0];
        let equal_positive = support_row(0, Some(&column), &labels).unwrap();
        assert_eq!(equal_positive.status, "stored-positive-regrets");
        assert_eq!(zero.current_strategy, equal_positive.current_strategy);
        column.regrets = vec![0.0, 3.0];
        column.strategy_sum = vec![1.0, 3.0];
        let positive = support_row(0, Some(&column), &labels).unwrap();
        assert_eq!(positive.status, "stored-positive-regrets");
        assert_eq!(positive.current_strategy, Some(vec![0.0, 1.0]));
        assert_eq!(positive.average_strategy, Some(vec![0.25, 0.75]));
        assert_eq!(positive.strategy_mass, Some(4.0));
        // An average walk can give positive mass while regret updates were
        // zero: observing an average column is not evidence of regret learning.
        column.regrets = vec![0.0, 0.0];
        let average_only = support_row(0, Some(&column), &labels).unwrap();
        assert_eq!(average_only.status, "stored-zero-regrets");
        assert!(average_only.average_strategy.is_some());
        for invalid in [f32::NAN, f32::INFINITY, -1.0] {
            column.strategy_sum[0] = invalid;
            assert!(support_row(0, Some(&column), &labels).is_err());
        }
        column.strategy_sum = vec![f32::MAX, f32::MAX];
        assert!(support_row(0, Some(&column), &labels).is_err());
    }

    #[test]
    fn support_export_matches_stored_columns_across_streets_without_mutation() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let mut solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        solver.run_sweeps_with_threads(4, 2).unwrap();
        let before = solver.snapshot_state();
        let mut history = HistoryKey::ROOT;
        let mut streets = BTreeSet::new();
        while let Some(node) = solver.public_node_view(history) {
            let context = resolve_condition_prefixes(&solver, &[hex(history.0)])
                .unwrap()
                .remove(0);
            let support = export_policy_support(&solver, context).unwrap();
            let columns = before
                .policies
                .iter()
                .filter(|entry| entry.key.history == history)
                .collect::<Vec<_>>();
            assert_eq!(support.stored_buckets, columns.len());
            assert_eq!(support.rows.len(), support.expected_buckets as usize);
            for entry in &columns {
                let row = &support.rows[entry.key.bucket_path[node.street as usize] as usize];
                assert_eq!(row.regrets.as_ref(), Some(&entry.column.regrets));
                assert_eq!(row.strategy_sum.as_ref(), Some(&entry.column.strategy_sum));
            }
            assert_eq!(
                support.nonzero_regret_buckets,
                columns
                    .iter()
                    .filter(|entry| entry.column.regrets.iter().any(|&v| v != 0.0))
                    .count()
            );
            streets.insert(node.street as u8);
            let next = node
                .actions
                .iter()
                .position(|action| action.label == "check" || action.label.starts_with("call:"))
                .unwrap();
            history = history.child(usize::from(node.actor), next);
        }
        assert_eq!(streets, BTreeSet::from([0, 1, 2, 3]));
        assert_eq!(before, solver.snapshot_state());
        assert!(
            resolve_condition_prefixes(&solver, &["root".to_owned(), hex(HistoryKey::ROOT.0)])
                .is_err()
        );
    }

    #[test]
    fn validation_rejects_duplicate_seeds_and_degenerate_sample_counts() {
        assert!(validate_parameters(&[1, 2, 1], 128, 10, Some(1), 2).is_err());
        assert!(validate_parameters(&[1], 1, 10, Some(1), 2).is_err());
        assert!(validate_parameters(&[1], 128, 0, Some(1), 2).is_err());
        assert!(validate_parameters(&[1], 128, 10, Some(0), 2).is_err());
        assert!(validate_parameters(&[1], 128, 10, Some(1), 1).is_err());
        validate_parameters(&[0, u64::MAX], 2, 1, None, 0).unwrap();
    }

    #[test]
    fn clap_rejects_seed_outside_u64_range() {
        let parsed = Args::try_parse_from([
            "mw_checkpoint_audit",
            "--config",
            "config.toml",
            "--checkpoint",
            "checkpoint.mwckpt",
            "--evaluation-seeds",
            "18446744073709551616",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn action_selection_accepts_labels_and_in_range_indices() {
        let labels = ["fold", "call", "raise-to:2500"];
        assert_eq!(select_action("call", &labels).unwrap(), 1);
        assert_eq!(select_action("2", &labels).unwrap(), 2);
        assert!(select_action("3", &labels).is_err());
        assert!(select_action("raise", &labels).is_err());
    }

    #[test]
    fn history_hex_round_trips_and_rejects_bad_input() {
        let expected = HistoryKey([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 255]);
        assert_eq!(parse_history_key(&hex(expected.0)).unwrap(), expected);
        assert!(parse_history_key("00").is_err());
        assert!(parse_history_key("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn postflop_hand_export_is_explicitly_rejected() {
        assert!(ensure_preflop(Street::Flop, "check/call").is_err());
        ensure_preflop(Street::Preflop, "root").unwrap();
    }

    #[test]
    fn zero_mass_regret_fallback_is_not_exported_as_an_average_strategy() {
        let labels = vec!["fold".to_owned(), "call".to_owned()];
        let fallback = vec![0.25, 0.75];
        let (status, strategy) = export_average_strategy(&labels, &fallback, 0.0).unwrap();
        assert_eq!(status, "current-regret-fallback-omitted");
        assert_eq!(strategy, None);

        let (status, strategy) = export_average_strategy(&labels, &fallback, 4.0).unwrap();
        assert_eq!(status, "average-observed");
        assert_eq!(strategy.unwrap()["call"], 0.75);
        assert!(export_average_strategy(&labels, &fallback, f64::NAN).is_err());
        assert!(export_average_strategy(&labels, &fallback, -1.0).is_err());
    }

    #[test]
    fn prepared_frequency_matches_replayed_worlds_across_streets_and_fallbacks() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let mut solver = ProductionSolver::new(
            game,
            sampler,
            multiway::SolverConfig {
                seed: 811,
                max_memory_bytes: 1 << 25,
                traverser_vector: true,
                ..multiway::SolverConfig::default()
            },
        )
        .unwrap();

        // With no stored keys every lookup must retain the original uniform
        // fallback. The public dense tree still resolves every target path.
        let labels = action_labels(
            solver.game(),
            &solver.game().node_actions(&solver.game().root_state()),
        );
        let config = NodeFrequencyConfig {
            samples: 17,
            seed: 73,
        };
        assert_eq!(
            serde_json::to_value(
                estimate_node_frequency(&solver, HistoryKey::ROOT, &labels, config).unwrap()
            )
            .unwrap(),
            serde_json::to_value(
                estimate_node_frequency_replayed(&solver, HistoryKey::ROOT, &labels, config)
                    .unwrap()
            )
            .unwrap()
        );

        solver.run_sweeps(2).unwrap();
        let mut state = solver.snapshot_state();
        // Construct a valid frozen checkpoint with all three policy sources.
        // Non-binary f32 weights exercise the original f64 renormalization.
        state
            .policies
            .retain(|entry| entry.key.bucket_path[entry.key.street as usize] % 3 != 2);
        for entry in &mut state.policies {
            let bucket = entry.key.bucket_path[entry.key.street as usize];
            for (index, (regret, average)) in entry
                .column
                .regrets
                .iter_mut()
                .zip(&mut entry.column.strategy_sum)
                .enumerate()
            {
                *regret = 0.13 * (index + 1) as f32;
                *average = if bucket % 3 == 1 {
                    0.0
                } else {
                    0.1 * (index + 1) as f32
                };
            }
        }
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let solver = ProductionSolver::from_state(game, sampler, state).unwrap();
        let before = solver.snapshot_state();
        let mut state = solver.game().root_state();
        let mut history = HistoryKey::ROOT;
        let mut streets = BTreeSet::new();
        let mut saw_average = false;
        let mut saw_regret = false;
        let mut tested_nodes = 0;
        while let Some(actor) = solver.game().actor(&state) {
            streets.insert(state.street as u8);
            let actions = solver.game().node_actions(&state);
            let labels = action_labels(solver.game(), &actions);
            let prepared = prepare_frequency_path(&solver, history, &labels).unwrap();
            for policy in prepared.target.policies.values() {
                saw_average |= !policy.source.current_regret;
                saw_regret |= policy.source.current_regret;
            }
            for seed in [0, u64::MAX] {
                let config = NodeFrequencyConfig { samples: 257, seed };
                let expected =
                    estimate_node_frequency_replayed(&solver, history, &labels, config).unwrap();
                let actual = estimate_node_frequency(&solver, history, &labels, config).unwrap();
                assert!(actual.fallback_reach_weight_fraction.uniform.unwrap() > 0.0);
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    serde_json::to_value(expected).unwrap(),
                    "prepared frequencies changed at {} with seed {seed}",
                    hex(history.0)
                );
            }
            tested_nodes += 1;
            let action_index = labels
                .iter()
                .position(|label| label.starts_with("call:") || label == "check")
                .unwrap();
            state = solver
                .game()
                .next_state_with(&state, &actions, action_index);
            history = history.child(actor, action_index);
        }
        assert_eq!(streets.len(), 4);
        assert!(tested_nodes >= 6);
        assert!(saw_average && saw_regret);
        assert_eq!(solver.snapshot_state(), before);
    }

    fn frequency_test_game() -> multiway::HoldemGame<MultiwayAbstractionBackend> {
        let mut config = multiway::MultiwayConfig {
            seats: (0..3)
                .map(|_| multiway::SeatConfig {
                    name: None,
                    stack_bb: 6.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: multiway::SeatId(0),
            blinds: multiway::config::BlindConfig::default(),
            ante: multiway::config::AnteConfig::None,
            betting: multiway::BettingConfig::default(),
            forced_bets: None,
            abstraction: multiway::AbstractionConfig::default(),
        };
        config.abstraction.recall = multiway::RecallMode::Street;
        config.betting.preflop.max_aggressive_actions = 2;
        for street in [
            &mut config.betting.flop,
            &mut config.betting.turn,
            &mut config.betting.river,
        ] {
            street.bet_sizes.clear();
            street.raise_sizes.clear();
            street.include_allin = false;
            street.max_aggressive_actions = 1;
        }
        multiway::HoldemGame::new(
            &config,
            &multiway::UtilityConfig::ChipEv,
            &multiway::config::RakeConfig::None,
            MultiwayAbstractionBackend::FeatureHash(
                multiway::FeatureHashAbstraction::new(multiway::FeatureHashParams {
                    flop_buckets: 4,
                    turn_buckets: 4,
                    river_buckets: 4,
                })
                .unwrap(),
            ),
        )
        .unwrap()
    }

    #[test]
    fn prepared_frequency_rejects_changed_target_menu_before_sampling() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        let mut labels = action_labels(
            solver.game(),
            &solver.game().node_actions(&solver.game().root_state()),
        );
        labels.swap(0, 1);
        let error = match prepare_frequency_path(&solver, HistoryKey::ROOT, &labels) {
            Ok(_) => panic!("changed action menu must fail during preparation"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("target action labels changed"));
    }

    #[test]
    fn coverage_cli_requires_prefix_and_non_degenerate_samples() {
        let base = [
            "audit",
            "--config",
            "unused.toml",
            "--checkpoint",
            "unused.mwckpt",
        ];
        let parse =
            |extra: &[&str]| Args::try_parse_from(base.into_iter().chain(extra.iter().copied()));
        assert!(parse(&["--coverage-samples", "2"]).is_err());
        assert!(parse(&["--coverage-prefix", "root", "--coverage-samples", "1"]).is_err());
        let args = parse(&["--coverage-prefix", "root", "--coverage-samples", "65536"]).unwrap();
        assert_eq!(args.coverage_prefix, vec!["root"]);
        assert_eq!(args.coverage_samples, Some(65536));
        let ordinary = parse(&[]).unwrap();
        assert!(ordinary.coverage_prefix.is_empty());
        assert!(ordinary.coverage_samples.is_none());
    }

    #[test]
    fn coverage_prefix_aliases_and_unknown_keys_fail_before_evaluation() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let mut solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        solver.run_sweeps(1).unwrap();
        let root = "root".to_owned();
        assert_eq!(
            resolve_coverage_prefixes(&solver, std::slice::from_ref(&root)).unwrap(),
            vec![HistoryKey::ROOT]
        );
        assert!(
            resolve_coverage_prefixes(&solver, &[root, hex(HistoryKey::ROOT.0)])
                .unwrap_err()
                .to_string()
                .contains("same public history")
        );
        assert!(
            resolve_coverage_prefixes(&solver, &["f".repeat(32)])
                .unwrap_err()
                .to_string()
                .contains("not a known public decision")
        );
    }

    #[test]
    fn condition_cli_requires_explicit_samples_and_bounded_prefixes() {
        let base = [
            "audit",
            "--config",
            "unused.toml",
            "--checkpoint",
            "unused.mwckpt",
        ];
        let parse =
            |extra: &[&str]| Args::try_parse_from(base.into_iter().chain(extra.iter().copied()));
        for extra in [vec![], vec!["--condition-samples", "0"]] {
            let args = parse(&extra).unwrap();
            assert_eq!(args.condition_samples, 0);
            assert_eq!(args.condition_sampler, ConditionSampler::Root);
            assert!(args.condition_prefix.is_empty());
            validate_condition_parameters(
                &args.condition_prefix,
                args.condition_samples,
                args.condition_sampler,
            )
            .unwrap();
        }
        for extra in [
            vec!["--condition-samples", "2"],
            vec!["--condition-prefix", "root"],
            vec!["--condition-prefix", "root", "--condition-samples", "0"],
            vec!["--condition-prefix", "root", "--condition-samples", "1"],
        ] {
            let args = parse(&extra).unwrap();
            assert!(
                validate_condition_parameters(
                    &args.condition_prefix,
                    args.condition_samples,
                    args.condition_sampler
                )
                .is_err()
            );
        }
        let args = parse(&[
            "--condition-prefix",
            "root",
            "--condition-prefix",
            "fold",
            "--condition-samples",
            "2",
        ])
        .unwrap();
        assert_eq!(args.condition_prefix, vec!["root", "fold"]);
        validate_condition_parameters(
            &args.condition_prefix,
            args.condition_samples,
            args.condition_sampler,
        )
        .unwrap();
        let prefixes = (0..64).map(|index| index.to_string()).collect::<Vec<_>>();
        validate_condition_parameters(&prefixes, 2, ConditionSampler::Root).unwrap();
        let mut too_many = prefixes;
        too_many.push("root".to_owned());
        assert!(validate_condition_parameters(&too_many, 2, ConditionSampler::Root).is_err());
        assert!(parse(&["--condition-samples", "18446744073709551616"]).is_err());
    }

    #[test]
    fn condition_sampler_selection_requires_an_enabled_budget_and_known_mode() {
        let base = [
            "audit",
            "--config",
            "unused.toml",
            "--checkpoint",
            "unused.mwckpt",
        ];
        let parse =
            |extra: &[&str]| Args::try_parse_from(base.into_iter().chain(extra.iter().copied()));
        for mode in ["root", "preflop-proposal"] {
            assert!(parse(&["--condition-sampler", mode]).is_err());
            assert!(parse(&["--condition-sampler", mode, "--condition-prefix", "root"]).is_err());
            assert!(parse(&["--condition-sampler", mode, "--condition-samples", "2"]).is_err());
            for samples in ["0", "1"] {
                let args = parse(&[
                    "--condition-sampler",
                    mode,
                    "--condition-prefix",
                    "root",
                    "--condition-samples",
                    samples,
                ])
                .unwrap();
                assert!(
                    validate_condition_parameters(
                        &args.condition_prefix,
                        args.condition_samples,
                        args.condition_sampler,
                    )
                    .is_err()
                );
            }
            let args = parse(&[
                "--condition-sampler",
                mode,
                "--condition-prefix",
                "root",
                "--condition-samples",
                "2",
            ])
            .unwrap();
            validate_condition_parameters(
                &args.condition_prefix,
                args.condition_samples,
                args.condition_sampler,
            )
            .unwrap();
            assert_eq!(
                args.condition_sampler == ConditionSampler::Root,
                mode == "root"
            );
        }
        assert!(
            parse(&[
                "--condition-sampler",
                "unknown",
                "--condition-prefix",
                "root",
                "--condition-samples",
                "2",
            ])
            .is_err()
        );
        assert!(validate_condition_parameters(&[], 0, ConditionSampler::PreflopProposal).is_err());
    }

    #[test]
    fn condition_proposal_groups_public_trunks_in_first_request_order() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        let before = solver.snapshot_state();
        let postflop_paths = |fold_first: bool| {
            let game = solver.game();
            let mut state = game.root_state();
            let mut labels = Vec::new();
            let mut paths = BTreeMap::new();
            while game.actor(&state).is_some() {
                if state.street != Street::Preflop {
                    paths
                        .entry(state.street as u8)
                        .or_insert_with(|| labels.join("/"));
                }
                let actions = game.node_actions(&state);
                let menu = action_labels(game, &actions);
                let index = menu
                    .iter()
                    .position(|label| {
                        if fold_first && labels.is_empty() {
                            label == "fold"
                        } else {
                            label.starts_with("call:") || label == "check"
                        }
                    })
                    .unwrap();
                labels.push(menu[index].clone());
                state = game.next_state_with(&state, &actions, index);
            }
            [Street::Flop, Street::Turn, Street::River].map(|street| paths[&(street as u8)].clone())
        };
        let three = postflop_paths(false);
        let heads_up = postflop_paths(true);
        // Interleave different trunks and use a hex alias for one request.
        let first = hex(resolve_node(&solver, &heads_up[2]).unwrap().0);
        let requested = vec![
            first,
            three[0].clone(),
            heads_up[0].clone(),
            three[2].clone(),
            three[1].clone(),
        ];
        let contexts = resolve_condition_prefixes(&solver, &requested).unwrap();
        let groups = group_preflop_condition_prefixes(&solver, &contexts).unwrap();
        assert_eq!(
            groups,
            vec![
                vec![contexts[0].clone(), contexts[2].clone()],
                vec![
                    contexts[1].clone(),
                    contexts[3].clone(),
                    contexts[4].clone()
                ],
            ]
        );
        assert!(
            groups[0]
                .iter()
                .all(|context| context.active_opponents == 1)
        );
        assert!(
            groups[1]
                .iter()
                .all(|context| context.active_opponents == 2)
        );
        for path in ["root", "fold"] {
            let contexts = resolve_condition_prefixes(&solver, &[path.to_owned()]).unwrap();
            assert!(
                group_preflop_condition_prefixes(&solver, &contexts)
                    .unwrap_err()
                    .to_string()
                    .contains("postflop decision prefixes")
            );
        }
        assert_eq!(solver.snapshot_state(), before);
    }

    #[test]
    fn condition_prefixes_recover_indices_for_labels_and_hex_on_every_street() {
        let game = frequency_test_game();
        let sampler = game.deal_sampler().unwrap();
        let solver =
            ProductionSolver::new(game, sampler, multiway::SolverConfig::default()).unwrap();
        let before = solver.snapshot_state();
        let mut state = solver.game().root_state();
        let mut history = HistoryKey::ROOT;
        let mut indices = Vec::new();
        let mut labels = Vec::new();
        let mut streets = BTreeSet::new();
        while let Some(actor) = solver.game().actor(&state) {
            let label_path = if labels.is_empty() {
                "root".to_owned()
            } else {
                labels.join("/")
            };
            let index_path = if indices.is_empty() {
                "root".to_owned()
            } else {
                indices
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join("/")
            };
            for requested in [label_path, index_path, hex(history.0)] {
                let contexts =
                    resolve_condition_prefixes(&solver, std::slice::from_ref(&requested)).unwrap();
                let context = &contexts[0];
                assert_eq!(context.requested, requested);
                assert_eq!(context.history, hex(history.0));
                assert_eq!(context.action_indices, indices);
                assert_eq!(context.action_labels, labels);
                assert_eq!(context.actor as usize, actor);
                assert_eq!(context.street, state.street);
                streets.insert(context.street as u8);
            }
            let actions = solver.game().node_actions(&state);
            let menu = action_labels(solver.game(), &actions);
            let index = menu
                .iter()
                .position(|label| label.starts_with("call:") || label == "check")
                .unwrap();
            indices.push(index);
            labels.push(menu[index].clone());
            state = solver.game().next_state_with(&state, &actions, index);
            history = history.child(actor, index);
        }
        assert_eq!(streets.len(), 4);
        assert!(
            resolve_condition_prefixes(&solver, &[labels.join("/")])
                .unwrap_err()
                .to_string()
                .contains("not a known public decision")
        );
        assert!(
            resolve_condition_prefixes(&solver, &["root".to_owned(), hex(HistoryKey::ROOT.0)])
                .unwrap_err()
                .to_string()
                .contains("same public history")
        );
        assert!(resolve_condition_prefixes(&solver, &["f".repeat(32)]).is_err());
        assert!(resolve_condition_prefixes(&solver, &["99999".to_owned()]).is_err());
        assert_eq!(solver.snapshot_state(), before);
    }

    #[test]
    fn node_frequency_ratio_matches_toy_weighted_math_and_normalizes() {
        let mut moments = NodeFrequencyMoments::new(2);
        moments
            .observe(
                1.0,
                &[1.0, 0.0],
                FallbackFlags {
                    current_regret: true,
                    uniform: false,
                },
            )
            .unwrap();
        moments
            .observe(
                0.5,
                &[0.0, 1.0],
                FallbackFlags {
                    current_regret: false,
                    uniform: true,
                },
            )
            .unwrap();
        let labels = vec!["fold".to_owned(), "call".to_owned()];
        let output = moments.finish(&labels, 2, 99, 2).unwrap();
        let rates = output.conditional_action_rates.unwrap();
        assert!((rates["fold"].estimate - 2.0 / 3.0).abs() < 1e-12);
        assert!((rates["call"].estimate - 1.0 / 3.0).abs() < 1e-12);
        assert!((rates.values().map(|rate| rate.estimate).sum::<f64>() - 1.0).abs() < 1e-12);
        assert!((output.reach_probability_estimate.estimate - 0.75).abs() < 1e-12);
        assert!((output.effective_sample_size - 1.8).abs() < 1e-12);
        assert_eq!(output.fallback_reach_weight_fraction.any, Some(1.0));
        assert!(
            (output
                .fallback_reach_weight_fraction
                .current_regret
                .unwrap()
                - 2.0 / 3.0)
                .abs()
                < 1e-12
        );
        assert!((output.fallback_reach_weight_fraction.uniform.unwrap() - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn zero_node_reach_has_no_conditional_rates() {
        let mut moments = NodeFrequencyMoments::new(2);
        moments
            .observe(0.0, &[0.25, 0.75], FallbackFlags::default())
            .unwrap();
        moments
            .observe(0.0, &[0.75, 0.25], FallbackFlags::default())
            .unwrap();
        let labels = vec!["fold".to_owned(), "call".to_owned()];
        let output = moments.finish(&labels, 2, 7, 2).unwrap();
        assert!(output.conditional_action_rates.is_none());
        assert_eq!(output.reach_probability_estimate.estimate, 0.0);
        assert_eq!(output.effective_sample_size, 0.0);
        assert_eq!(output.fallback_reach_weight_fraction.any, None);
    }

    #[test]
    fn policy_normalization_rejects_bad_values() {
        let normalized = normalize_probabilities(&[0.2, 0.3]).unwrap();
        assert!((normalized[0] - 0.4).abs() < 1e-7);
        assert!((normalized[1] - 0.6).abs() < 1e-7);
        assert!(normalize_probabilities(&[0.0, 0.0]).is_err());
        assert!(normalize_probabilities(&[-0.1, 1.1]).is_err());
        assert!(normalize_probabilities(&[f32::NAN, 1.0]).is_err());
    }

    #[test]
    fn checked_checkpoint_load_rejects_both_fingerprint_mismatches() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audit.mwckpt");
        let state = multiway::SolverState {
            schema_version: multiway::solver::SOLVER_STATE_VERSION,
            config: multiway::SolverConfig::default(),
            traversals: 0,
            completed_sweeps: 0,
            next_sample_id: 0,
            total_deal_attempts: 0,
            terminal_evaluations: 0,
            hand_updates: 0,
            histories: Vec::new(),
            policies: Vec::new(),
        };
        multiway::MultiwayCheckpoint::new(state, [3; 32], [7; 32])
            .write_atomic(&path)
            .unwrap();

        assert!(matches!(
            multiway::MultiwayCheckpoint::load(&path, [4; 32], [7; 32]),
            Err(multiway::CheckpointError::ConfigurationMismatch)
        ));
        assert!(matches!(
            multiway::MultiwayCheckpoint::load(&path, [3; 32], [8; 32]),
            Err(multiway::CheckpointError::AbstractionMismatch)
        ));
    }
}
