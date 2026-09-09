use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use multiway::{
    DeviatorTrainingCoverage, ExternalSamplingGame, HistoryKey, InfoKey,
    MultiwayAbstractionBackend, MultiwaySolver, PolicyArenaAllocation, ProfileEvaluation,
    ProfileVariant, Street,
};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::Serialize;

type ProductionSolver = MultiwaySolver<multiway::HoldemGame<MultiwayAbstractionBackend>>;

/// Re-evaluate a frozen, full-policy Multiway checkpoint without resuming training.
#[derive(Debug, Parser)]
#[command(name = "mw_checkpoint_audit")]
struct Args {
    /// Original v1 config used to create the checkpoint.
    #[arg(long)]
    config: PathBuf,

    /// Atomic `.mwckpt` snapshot to restore and evaluate.
    #[arg(long)]
    checkpoint: PathBuf,

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditOutput {
    schema_version: &'static str,
    checkpoint: String,
    config: String,
    sweeps: u64,
    solver_state_version: u16,
    configuration_fingerprint: String,
    abstraction_fingerprint: String,
    policy_arena: Option<PolicyArenaAllocation>,
    evaluation_samples_per_seed: u64,
    evaluation_seeds: Vec<u64>,
    deviator_training: DeviatorTrainingOutput,
    evaluations: Vec<EvaluationOutput>,
    nodes: Vec<NodeOutput>,
    interpretation: &'static str,
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
struct NodeOutput {
    requested: String,
    history: String,
    actor: u8,
    active_opponents: u8,
    actions: Vec<String>,
    hands: Vec<HandOutput>,
    frequency: Option<NodeFrequencyOutput>,
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
    validate_parameters(
        &args.evaluation_seeds,
        args.samples,
        args.br_traversals,
        args.threads,
        args.node_frequency_samples,
    )?;
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
    let session = cli::session::build_production_multiway_session(
        &effective_config,
        Some(&args.checkpoint),
    )
    .with_context(|| {
        format!(
            "restoring full policy from {} (configuration and abstraction fingerprints must match)",
            args.checkpoint.display()
        )
    })?;
    let solver = &session.solver;

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

    let output = AuditOutput {
        schema_version: "solvers.multiway-checkpoint-audit/v1",
        checkpoint: args.checkpoint.display().to_string(),
        config: args.config.display().to_string(),
        sweeps: solver.completed_sweeps(),
        solver_state_version: multiway::solver::SOLVER_STATE_VERSION,
        configuration_fingerprint: hex(solver.configuration_fingerprint()),
        abstraction_fingerprint: hex(solver.abstraction_fingerprint()),
        policy_arena: solver.policy_arena_allocation(),
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
        interpretation: "Held-out gains cover two fixed candidate deviations per seat; the trained candidate is used where it retained an action and otherwise falls back to the main regret-greedy candidate. Deviator coverage in this document is training coverage, not held-out replay coverage. Node conditional action rates are self-normalized reach-weighted ratio estimates, are not claimed finite-sample unbiased, and represent a composite policy whenever their fallback reach-weight fraction is positive. These results are diagnostics, not a full best response, exploitability, or Nash certificate. Evaluation seeds are reported separately and are not selected or pooled.",
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
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
    let frequency = (frequency_config.samples > 0)
        .then(|| estimate_node_frequency(solver, history, &actions, frequency_config))
        .transpose()?;

    Ok(NodeOutput {
        requested: requested.to_owned(),
        history: hex(history.0),
        actor: node.actor,
        active_opponents: node.active_opponents,
        actions,
        hands,
        frequency,
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
