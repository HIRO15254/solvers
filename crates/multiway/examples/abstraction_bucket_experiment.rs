//! Historical research-only postflop bucket-budget experiment for Multiway
//! Preflop.
//!
//! The default discovery experiment separates rollout-estimator noise from
//! bucket quantization:
//!
//! 1. evaluate 6,144 fixed physical-card states with 32,768 rollouts;
//! 2. train each discovery candidate with the then-proposed 512-rollout
//!    estimator;
//! 3. replace every state by its assigned production centroid;
//! 4. compare that centroid with the high-sample reference features.
//!
//! Run with:
//! `cargo run --release -p multiway --features research-abstractions \
//!   --example abstraction_bucket_experiment`
//!
//! The explicit holdout/robustness modes use independent physical-state and
//! reference seeds and include 1,024--4,096-rollout Cash candidates.

use std::collections::BTreeMap;
use std::time::Instant;

use cards::{ALL_CARDS, Card, combo_index};
use multiway::{
    BucketContext, MultiwayAbstraction, RolloutFeatures, RolloutKMeansAbstraction,
    RolloutKMeansBuilder, RolloutKMeansParams, Street,
};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;

const BUCKET_CANDIDATES: [u32; 12] = [8, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512];
const REFINEMENT_BUCKETS: [u32; 4] = [64, 96, 128, 192];
const REFINEMENT_ROLLOUTS: [u32; 2] = [512, 1_024];
const CANDIDATE_SEEDS: [u64; 3] = [11, 29, 47];
const HOLDOUT_CANDIDATE_SEEDS: [u64; 4] = [0, 11, 29, 47];
const CANDIDATE_ROLLOUTS: u32 = 512;
const REFERENCE_ROLLOUTS: u32 = 32_768;
const REFERENCE_SEED: u64 = 0x2026_0723_5a17;
const DISCOVERY_SCENARIO_SEED: u64 = 0x5a17_2026_0723;
const HOLDOUT_REFERENCE_SEED: u64 = 0x686f_6c64_6f75_7452;
const HOLDOUT_SCENARIO_SEED: u64 = 0x686f_6c64_6f75_7453;
const CONFIRM_REFERENCE_SEED: u64 = 0x636f_6e66_6972_6d52;
const CONFIRM_SCENARIO_SEED: u64 = 0x636f_6e66_6972_6d53;
const SCENARIOS_PER_GROUP: usize = 256;
// Four strata deliberately match the independently qualified rollout-sample
// experiment. A 16-stratum pilot made the diagnostic itself dominated by
// 512-rollout noise near adjacent stratum boundaries, even at 256 buckets.
const REFERENCE_STRATA: usize = 4;

// Fixed before looking at this experiment's results. The tournament gate
// allows bucket error comparable with (but not materially larger than) the
// already-qualified 512-rollout estimator. The deep-cash gate is stricter
// because more postflop decisions compound abstraction error.
const TOURNAMENT_GATE: QualityGate = QualityGate {
    max_rmse: 0.020,
    max_p95_component_error: 0.050,
    min_strata_agreement: 0.90,
};
const DEEP_CASH_GATE: QualityGate = QualityGate {
    max_rmse: 0.0175,
    max_p95_component_error: 0.045,
    min_strata_agreement: 0.95,
};

type Group = (usize, u8);

#[derive(Clone)]
struct Scenario {
    street: Street,
    board: Vec<Card>,
    combo: usize,
    opponents: u8,
}

#[derive(Clone, Copy)]
struct QualityGate {
    max_rmse: f64,
    max_p95_component_error: f64,
    min_strata_agreement: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Metrics {
    rmse: f64,
    p95_component_error: f64,
    strata_agreement: f64,
    occupied_fraction: f64,
    build_seconds: f64,
    query_micros_per_state: f64,
}

impl QualityGate {
    fn passes(self, metrics: Metrics) -> bool {
        metrics.rmse <= self.max_rmse
            && metrics.p95_component_error <= self.max_p95_component_error
            && metrics.strata_agreement >= self.min_strata_agreement
    }
}

fn main() {
    let refinement_only = std::env::args().any(|arg| arg == "--refinement-only");
    if std::env::args().any(|arg| arg == "--tournament-holdout") {
        run_tournament_holdout();
        return;
    }
    if std::env::args().any(|arg| arg == "--cash-holdout") {
        run_cash_holdout();
        return;
    }
    if std::env::args().any(|arg| arg == "--cash-robustness") {
        run_cash_robustness();
        return;
    }
    if std::env::args().any(|arg| arg == "--cash-bucket-extension") {
        run_cash_bucket_extension();
        return;
    }
    if std::env::args().any(|arg| arg == "--cash-confirm") {
        run_cash_confirm();
        return;
    }
    let grouped_scenarios = scenarios(DISCOVERY_SCENARIO_SEED);
    let reference_model = build_model(1, REFERENCE_ROLLOUTS, REFERENCE_SEED).0;
    let reference = reference_features(&reference_model, &grouped_scenarios);
    let strata = reference_strata(&reference);

    if !refinement_only {
        let mut runs: BTreeMap<(u32, Group), Vec<Metrics>> = BTreeMap::new();
        for buckets in BUCKET_CANDIDATES {
            for seed in CANDIDATE_SEEDS {
                let (model, build_seconds) = build_model(buckets, CANDIDATE_ROLLOUTS, seed);
                for (&group, group_scenarios) in &grouped_scenarios {
                    let start = Instant::now();
                    let mut metrics = compare_group(
                        &model,
                        group_scenarios,
                        &reference[&group],
                        &strata[&group],
                        buckets,
                    );
                    metrics.query_micros_per_state =
                        start.elapsed().as_secs_f64() * 1_000_000.0 / group_scenarios.len() as f64;
                    metrics.build_seconds = build_seconds;
                    runs.entry((buckets, group)).or_default().push(metrics);
                }
            }
        }
        print_global_frontier(&runs);
        print_selected_profiles(&runs);
    }

    run_rollout_refinement(&grouped_scenarios, &reference, &strata);
}

fn build_model(buckets: u32, rollouts: u32, seed: u64) -> (RolloutKMeansAbstraction, f64) {
    let params = RolloutKMeansParams {
        flop_buckets: buckets,
        turn_buckets: buckets,
        river_buckets: buckets,
        rollout_samples: rollouts,
        seed,
    };
    let builder = RolloutKMeansBuilder::new(params);
    let builder = if buckets == 1 {
        builder.points_per_bucket(1).kmeans_iterations(1)
    } else {
        builder
    };
    let start = Instant::now();
    let model = builder.build().expect("valid experiment model");
    (model, start.elapsed().as_secs_f64())
}

fn scenarios(seed: u64) -> BTreeMap<Group, Vec<Scenario>> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut result: BTreeMap<Group, Vec<Scenario>> = BTreeMap::new();
    for street in [Street::Flop, Street::Turn, Street::River] {
        for opponents in 1..=8 {
            for _ in 0..SCENARIOS_PER_GROUP {
                let mut deck: Vec<Card> = ALL_CARDS.into_iter().collect();
                deck.shuffle(&mut rng);
                let board_len = match street {
                    Street::Flop => 3,
                    Street::Turn => 4,
                    Street::River => 5,
                    Street::Preflop => unreachable!(),
                };
                result
                    .entry((street.index(), opponents))
                    .or_default()
                    .push(Scenario {
                        street,
                        board: deck[2..2 + board_len].to_vec(),
                        combo: combo_index(deck[0], deck[1]),
                        opponents,
                    });
            }
        }
    }
    result
}

fn run_tournament_holdout() {
    // These profiles were frozen from the discovery split before this
    // independent physical-state/reference-seed run was inspected.
    const PROFILE_BUCKETS: [u32; 7] = [24, 32, 48, 64, 96, 128, 256];

    let grouped_scenarios = scenarios(HOLDOUT_SCENARIO_SEED);
    let reference_model = build_model(1, REFERENCE_ROLLOUTS, HOLDOUT_REFERENCE_SEED).0;
    let reference = reference_features(&reference_model, &grouped_scenarios);
    let strata = reference_strata(&reference);
    let mut runs: BTreeMap<(u32, Group), Vec<Metrics>> = BTreeMap::new();

    for buckets in PROFILE_BUCKETS {
        for seed in HOLDOUT_CANDIDATE_SEEDS {
            let (model, build_seconds) = build_model(buckets, CANDIDATE_ROLLOUTS, seed);
            for (&group, group_scenarios) in &grouped_scenarios {
                let start = Instant::now();
                let mut metrics = compare_group(
                    &model,
                    group_scenarios,
                    &reference[&group],
                    &strata[&group],
                    buckets,
                );
                metrics.query_micros_per_state =
                    start.elapsed().as_secs_f64() * 1_000_000.0 / group_scenarios.len() as f64;
                metrics.build_seconds = build_seconds;
                runs.entry((buckets, group)).or_default().push(metrics);
            }
        }
    }

    println!(
        "HOLDOUT street,opponents,frozen_buckets,pass,worst_rmse,worst_p95,\
         worst_agreement"
    );
    for street in [Street::Flop, Street::Turn, Street::River] {
        for opponents in 1..=8 {
            let group = (street.index(), opponents);
            let buckets = frozen_tournament_buckets(street, opponents);
            let metrics = worst_metrics(&runs[&(buckets, group)]);
            println!(
                "HOLDOUT {},{opponents},{buckets},{},{:.6},{:.6},{:.4}",
                street_name(street),
                TOURNAMENT_GATE.passes(metrics),
                metrics.rmse,
                metrics.p95_component_error,
                metrics.strata_agreement,
            );
        }
    }

    let uniform_metrics: Vec<_> = runs
        .iter()
        .filter(|((buckets, _), _)| *buckets == 256)
        .flat_map(|(_, values)| values.iter().copied())
        .collect();
    let uniform = worst_metrics(&uniform_metrics);
    println!(
        "HOLDOUT_UNIFORM 256,{},{:.6},{:.6},{:.4}",
        TOURNAMENT_GATE.passes(uniform),
        uniform.rmse,
        uniform.p95_component_error,
        uniform.strata_agreement,
    );
}

fn run_cash_holdout() {
    const CASH_BUCKETS: [u32; 2] = [128, 192];

    let grouped_scenarios = scenarios(HOLDOUT_SCENARIO_SEED);
    let reference_model = build_model(1, REFERENCE_ROLLOUTS, HOLDOUT_REFERENCE_SEED).0;
    let reference = reference_features(&reference_model, &grouped_scenarios);
    let strata = reference_strata(&reference);

    println!("CASH_HOLDOUT buckets,pass,worst_rmse,worst_p95,worst_agreement");
    for buckets in CASH_BUCKETS {
        let mut values = Vec::new();
        for seed in HOLDOUT_CANDIDATE_SEEDS {
            let (model, _) = build_model(buckets, 1_024, seed);
            for (&group, group_scenarios) in &grouped_scenarios {
                values.push(compare_group(
                    &model,
                    group_scenarios,
                    &reference[&group],
                    &strata[&group],
                    buckets,
                ));
            }
        }
        let metrics = worst_metrics(&values);
        println!(
            "CASH_HOLDOUT {buckets},{},{:.6},{:.6},{:.4}",
            DEEP_CASH_GATE.passes(metrics),
            metrics.rmse,
            metrics.p95_component_error,
            metrics.strata_agreement,
        );
    }
}

fn run_cash_robustness() {
    const CANDIDATES: [(u32, u32); 5] = [
        (128, 1_024),
        (96, 2_048),
        (128, 2_048),
        (192, 2_048),
        (128, 4_096),
    ];
    run_cash_robust_candidates("CASH_ROBUST", &CANDIDATES);
}

fn run_cash_bucket_extension() {
    const CANDIDATES: [(u32, u32); 2] = [(256, 2_048), (384, 2_048)];
    run_cash_robust_candidates("CASH_BUCKET_EXT", &CANDIDATES);
}

fn run_cash_confirm() {
    const BUCKETS: u32 = 256;
    const ROLLOUTS: u32 = 2_048;

    let scenarios = scenarios(CONFIRM_SCENARIO_SEED);
    let reference_model = build_model(1, REFERENCE_ROLLOUTS, CONFIRM_REFERENCE_SEED).0;
    let reference = reference_features(&reference_model, &scenarios);
    let strata = reference_strata(&reference);
    let mut values = Vec::new();
    for seed in HOLDOUT_CANDIDATE_SEEDS {
        let (model, _) = build_model(BUCKETS, ROLLOUTS, seed);
        for (&group, group_scenarios) in &scenarios {
            values.push(compare_group(
                &model,
                group_scenarios,
                &reference[&group],
                &strata[&group],
                BUCKETS,
            ));
        }
    }
    let metrics = worst_metrics(&values);
    println!(
        "CASH_CONFIRM {BUCKETS},{ROLLOUTS},{},{:.6},{:.6},{:.4}",
        DEEP_CASH_GATE.passes(metrics),
        metrics.rmse,
        metrics.p95_component_error,
        metrics.strata_agreement,
    );
}

fn run_cash_robust_candidates(label: &str, candidates: &[(u32, u32)]) {
    let discovery_scenarios = scenarios(DISCOVERY_SCENARIO_SEED);
    let discovery_reference_model = build_model(1, REFERENCE_ROLLOUTS, REFERENCE_SEED).0;
    let discovery_reference = reference_features(&discovery_reference_model, &discovery_scenarios);
    let discovery_strata = reference_strata(&discovery_reference);

    let holdout_scenarios = scenarios(HOLDOUT_SCENARIO_SEED);
    let holdout_reference_model = build_model(1, REFERENCE_ROLLOUTS, HOLDOUT_REFERENCE_SEED).0;
    let holdout_reference = reference_features(&holdout_reference_model, &holdout_scenarios);
    let holdout_strata = reference_strata(&holdout_reference);

    println!("{label} buckets,rollouts,pass,worst_rmse,worst_p95,worst_agreement");
    for &(buckets, rollouts) in candidates {
        let mut values = Vec::new();
        for seed in HOLDOUT_CANDIDATE_SEEDS {
            let (model, _) = build_model(buckets, rollouts, seed);
            for (&group, group_scenarios) in &discovery_scenarios {
                values.push(compare_group(
                    &model,
                    group_scenarios,
                    &discovery_reference[&group],
                    &discovery_strata[&group],
                    buckets,
                ));
            }
            for (&group, group_scenarios) in &holdout_scenarios {
                values.push(compare_group(
                    &model,
                    group_scenarios,
                    &holdout_reference[&group],
                    &holdout_strata[&group],
                    buckets,
                ));
            }
        }
        let metrics = worst_metrics(&values);
        println!(
            "{label} {buckets},{rollouts},{},{:.6},{:.6},{:.4}",
            DEEP_CASH_GATE.passes(metrics),
            metrics.rmse,
            metrics.p95_component_error,
            metrics.strata_agreement,
        );
    }
}

fn frozen_tournament_buckets(street: Street, opponents: u8) -> u32 {
    let index = usize::from(opponents - 1);
    match street {
        Street::Flop => [128, 48, 48, 32, 32, 24, 32, 24][index],
        Street::Turn => [256, 96, 48, 48, 48, 48, 64, 32][index],
        Street::River => [96, 48, 48, 96, 32, 64, 48, 24][index],
        Street::Preflop => unreachable!(),
    }
}

fn reference_features(
    model: &RolloutKMeansAbstraction,
    scenarios: &BTreeMap<Group, Vec<Scenario>>,
) -> BTreeMap<Group, Vec<RolloutFeatures>> {
    let start = Instant::now();
    let mut result = BTreeMap::new();
    for (&group, values) in scenarios {
        let features = values
            .iter()
            .map(|scenario| {
                model
                    .rollout_features(BucketContext {
                        street: scenario.street,
                        board: &scenario.board,
                        combo: scenario.combo,
                        active_opponents: scenario.opponents,
                    })
                    .expect("valid reference scenario")
            })
            .collect();
        result.insert(group, features);
    }
    eprintln!(
        "reference_states={} reference_rollouts={} elapsed_seconds={:.3}",
        scenarios.values().map(Vec::len).sum::<usize>(),
        REFERENCE_ROLLOUTS,
        start.elapsed().as_secs_f64(),
    );
    result
}

fn reference_strata(
    reference: &BTreeMap<Group, Vec<RolloutFeatures>>,
) -> BTreeMap<Group, Vec<RolloutFeatures>> {
    reference
        .iter()
        .map(|(&group, points)| (group, kmeans(points, REFERENCE_STRATA)))
        .collect()
}

fn compare_group(
    model: &RolloutKMeansAbstraction,
    scenarios: &[Scenario],
    reference: &[RolloutFeatures],
    strata: &[RolloutFeatures],
    bucket_count: u32,
) -> Metrics {
    let mut squared_error = 0.0;
    let mut component_errors = Vec::with_capacity(reference.len() * 4);
    let mut matching_strata = 0usize;
    let mut occupied = vec![false; bucket_count as usize];
    let centroids = model
        .centroids(scenarios[0].street, scenarios[0].opponents)
        .expect("builder creates every group");

    for (scenario, &truth) in scenarios.iter().zip(reference) {
        let bucket = model.bucket(BucketContext {
            street: scenario.street,
            board: &scenario.board,
            combo: scenario.combo,
            active_opponents: scenario.opponents,
        }) as usize;
        occupied[bucket] = true;
        let prediction = centroids[bucket];
        for (left, right) in prediction.as_array().into_iter().zip(truth.as_array()) {
            let error = (left - right).abs();
            squared_error += error * error;
            component_errors.push(error);
        }
        matching_strata += usize::from(nearest(prediction, strata) == nearest(truth, strata));
    }

    component_errors.sort_by(f64::total_cmp);
    let p95_index = ((component_errors.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(component_errors.len() - 1);
    Metrics {
        rmse: (squared_error / component_errors.len() as f64).sqrt(),
        p95_component_error: component_errors[p95_index],
        strata_agreement: matching_strata as f64 / scenarios.len() as f64,
        occupied_fraction: occupied.iter().filter(|&&value| value).count() as f64
            / occupied.len() as f64,
        ..Metrics::default()
    }
}

fn print_global_frontier(runs: &BTreeMap<(u32, Group), Vec<Metrics>>) {
    println!(
        "GLOBAL candidate_buckets,worst_rmse,worst_p95_component_error,\
         worst_strata_agreement,mean_occupied_fraction,mean_build_seconds,\
         mean_query_micros_per_state"
    );
    for buckets in BUCKET_CANDIDATES {
        let values: Vec<Metrics> = runs
            .iter()
            .filter(|((candidate, _), _)| *candidate == buckets)
            .flat_map(|(_, values)| values.iter().copied())
            .collect();
        println!(
            "GLOBAL {buckets},{:.6},{:.6},{:.4},{:.4},{:.3},{:.1}",
            values
                .iter()
                .map(|metrics| metrics.rmse)
                .fold(f64::NEG_INFINITY, f64::max),
            values
                .iter()
                .map(|metrics| metrics.p95_component_error)
                .fold(f64::NEG_INFINITY, f64::max),
            values
                .iter()
                .map(|metrics| metrics.strata_agreement)
                .fold(f64::INFINITY, f64::min),
            mean(values.iter().map(|metrics| metrics.occupied_fraction)),
            mean(values.iter().map(|metrics| metrics.build_seconds)),
            mean(values.iter().map(|metrics| metrics.query_micros_per_state)),
        );
    }
}

fn print_selected_profiles(runs: &BTreeMap<(u32, Group), Vec<Metrics>>) {
    println!(
        "SELECT street,opponents,tournament_buckets,tournament_pass,\
         tournament_worst_rmse,tournament_worst_p95,tournament_worst_agreement,\
         cash_buckets,cash_pass,cash_worst_rmse,cash_worst_p95,cash_worst_agreement"
    );
    for street in [Street::Flop, Street::Turn, Street::River] {
        for opponents in 1..=8 {
            let group = (street.index(), opponents);
            let (tournament_buckets, tournament_metrics, tournament_pass) =
                select(runs, group, TOURNAMENT_GATE);
            let (cash_buckets, cash_metrics, cash_pass) = select(runs, group, DEEP_CASH_GATE);
            println!(
                "SELECT {},{opponents},{tournament_buckets},{tournament_pass},\
                 {:.6},{:.6},{:.4},{cash_buckets},{cash_pass},{:.6},{:.6},{:.4}",
                street_name(street),
                tournament_metrics.rmse,
                tournament_metrics.p95_component_error,
                tournament_metrics.strata_agreement,
                cash_metrics.rmse,
                cash_metrics.p95_component_error,
                cash_metrics.strata_agreement,
            );
        }
    }
}

fn run_rollout_refinement(
    scenarios: &BTreeMap<Group, Vec<Scenario>>,
    reference: &BTreeMap<Group, Vec<RolloutFeatures>>,
    strata: &BTreeMap<Group, Vec<RolloutFeatures>>,
) {
    let mut runs: BTreeMap<(u32, u32, Group), Vec<Metrics>> = BTreeMap::new();
    for rollouts in REFINEMENT_ROLLOUTS {
        for buckets in REFINEMENT_BUCKETS {
            for seed in CANDIDATE_SEEDS {
                let (model, build_seconds) = build_model(buckets, rollouts, seed);
                for (&group, group_scenarios) in scenarios {
                    let start = Instant::now();
                    let mut metrics = compare_group(
                        &model,
                        group_scenarios,
                        &reference[&group],
                        &strata[&group],
                        buckets,
                    );
                    metrics.query_micros_per_state =
                        start.elapsed().as_secs_f64() * 1_000_000.0 / group_scenarios.len() as f64;
                    metrics.build_seconds = build_seconds;
                    runs.entry((buckets, rollouts, group))
                        .or_default()
                        .push(metrics);
                }
            }
        }
    }

    println!(
        "REFINE buckets,rollouts,worst_rmse,worst_p95_component_error,\
         worst_strata_agreement,mean_build_seconds,mean_query_micros_per_state"
    );
    for rollouts in REFINEMENT_ROLLOUTS {
        for buckets in REFINEMENT_BUCKETS {
            let values: Vec<Metrics> = runs
                .iter()
                .filter(|((candidate_buckets, candidate_rollouts, _), _)| {
                    *candidate_buckets == buckets && *candidate_rollouts == rollouts
                })
                .flat_map(|(_, values)| values.iter().copied())
                .collect();
            println!(
                "REFINE {buckets},{rollouts},{:.6},{:.6},{:.4},{:.3},{:.1}",
                values
                    .iter()
                    .map(|metrics| metrics.rmse)
                    .fold(f64::NEG_INFINITY, f64::max),
                values
                    .iter()
                    .map(|metrics| metrics.p95_component_error)
                    .fold(f64::NEG_INFINITY, f64::max),
                values
                    .iter()
                    .map(|metrics| metrics.strata_agreement)
                    .fold(f64::INFINITY, f64::min),
                mean(values.iter().map(|metrics| metrics.build_seconds)),
                mean(values.iter().map(|metrics| metrics.query_micros_per_state)),
            );
        }
    }

    println!(
        "REFINE_SELECT street,opponents,buckets,rollouts,pass,worst_rmse,\
         worst_p95,worst_agreement"
    );
    for street in [Street::Flop, Street::Turn, Street::River] {
        for opponents in 1..=8 {
            let group = (street.index(), opponents);
            let mut candidates = Vec::new();
            for rollouts in REFINEMENT_ROLLOUTS {
                for buckets in REFINEMENT_BUCKETS {
                    candidates.push((
                        u64::from(buckets) * u64::from(rollouts),
                        buckets,
                        rollouts,
                        worst_metrics(&runs[&(buckets, rollouts, group)]),
                    ));
                }
            }
            candidates.sort_by_key(|&(cost, buckets, rollouts, _)| (cost, buckets, rollouts));
            let selected = candidates
                .iter()
                .copied()
                .find(|&(_, _, _, metrics)| DEEP_CASH_GATE.passes(metrics))
                .unwrap_or_else(|| *candidates.last().expect("non-empty refinement grid"));
            let (_, buckets, rollouts, metrics) = selected;
            println!(
                "REFINE_SELECT {},{opponents},{buckets},{rollouts},{},{:.6},{:.6},{:.4}",
                street_name(street),
                DEEP_CASH_GATE.passes(metrics),
                metrics.rmse,
                metrics.p95_component_error,
                metrics.strata_agreement,
            );
        }
    }
}

fn select(
    runs: &BTreeMap<(u32, Group), Vec<Metrics>>,
    group: Group,
    gate: QualityGate,
) -> (u32, Metrics, bool) {
    for buckets in BUCKET_CANDIDATES {
        let worst = worst_metrics(&runs[&(buckets, group)]);
        if gate.passes(worst) {
            return (buckets, worst, true);
        }
    }
    let fallback = *BUCKET_CANDIDATES.last().expect("non-empty candidates");
    (fallback, worst_metrics(&runs[&(fallback, group)]), false)
}

fn worst_metrics(values: &[Metrics]) -> Metrics {
    Metrics {
        rmse: values
            .iter()
            .map(|metrics| metrics.rmse)
            .fold(f64::NEG_INFINITY, f64::max),
        p95_component_error: values
            .iter()
            .map(|metrics| metrics.p95_component_error)
            .fold(f64::NEG_INFINITY, f64::max),
        strata_agreement: values
            .iter()
            .map(|metrics| metrics.strata_agreement)
            .fold(f64::INFINITY, f64::min),
        occupied_fraction: values
            .iter()
            .map(|metrics| metrics.occupied_fraction)
            .fold(f64::INFINITY, f64::min),
        build_seconds: mean(values.iter().map(|metrics| metrics.build_seconds)),
        query_micros_per_state: mean(values.iter().map(|metrics| metrics.query_micros_per_state)),
    }
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.collect();
    values.iter().sum::<f64>() / values.len() as f64
}

fn street_name(street: Street) -> &'static str {
    match street {
        Street::Preflop => "preflop",
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
    }
}

fn kmeans(points: &[RolloutFeatures], clusters: usize) -> Vec<RolloutFeatures> {
    let mut centroids = vec![points[0]];
    while centroids.len() < clusters {
        let point = *points
            .iter()
            .max_by(|left, right| {
                nearest_distance(**left, &centroids)
                    .total_cmp(&nearest_distance(**right, &centroids))
            })
            .expect("non-empty group");
        centroids.push(point);
    }
    for _ in 0..20 {
        let mut sums = vec![[0.0; 4]; clusters];
        let mut counts = vec![0usize; clusters];
        for &point in points {
            let cluster = nearest(point, &centroids);
            counts[cluster] += 1;
            for (sum, value) in sums[cluster].iter_mut().zip(point.as_array()) {
                *sum += value;
            }
        }
        for cluster in 0..clusters {
            if counts[cluster] > 0 {
                for value in &mut sums[cluster] {
                    *value /= counts[cluster] as f64;
                }
                centroids[cluster] = RolloutFeatures {
                    expected_pot_share: sums[cluster][0],
                    expected_share_squared: sums[cluster][1],
                    scoop_probability: sums[cluster][2],
                    tie_probability: sums[cluster][3],
                };
            }
        }
    }
    centroids
}

fn nearest(point: RolloutFeatures, centroids: &[RolloutFeatures]) -> usize {
    centroids
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            distance(point, **left).total_cmp(&distance(point, **right))
        })
        .map(|(index, _)| index)
        .expect("non-empty centroids")
}

fn nearest_distance(point: RolloutFeatures, centroids: &[RolloutFeatures]) -> f64 {
    centroids
        .iter()
        .map(|&centroid| distance(point, centroid))
        .fold(f64::INFINITY, f64::min)
}

fn distance(left: RolloutFeatures, right: RolloutFeatures) -> f64 {
    left.as_array()
        .into_iter()
        .zip(right.as_array())
        .map(|(left, right)| (left - right).powi(2))
        .sum()
}
