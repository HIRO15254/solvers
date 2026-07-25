//! Historical research-only rollout-k-means training-hyperparameter
//! experiment.
//!
//! This experiment holds two representative anchors fixed and sweeps only the
//! amount of centroid-training data and the k-means iteration cap:
//!
//! - Tournament: 64 buckets and 512 rollouts;
//! - Cash: 128 buckets and 1,024 rollouts;
//! - points per bucket: 4, 8, or 16;
//! - k-means iterations: 10, 20, or 40.
//!
//! Every candidate is evaluated on the same 6,144 physical-card states
//! (flop/turn/river x 1..=8 opponents x 256 states) against a deterministic
//! 32,768-rollout reference. Three independent training/assignment seeds are
//! aggregated, and four reference-feature strata provide a coarse assignment
//! stability diagnostic.
//!
//! Run with:
//! `cargo run --release -p multiway --features research-abstractions \
//!   --example abstraction_training_experiment`
//!
//! Caveats:
//!
//! - The reference is a rollout-feature surrogate, not solve EV,
//!   exploitability, ICM utility, or raked-cash utility.
//! - Evaluation states are uniformly sampled physical deals. They are not
//!   weighted by ranges, reach probability, rake, payouts, or stack depth.
//! - The reference favors rollout-feature abstractions by construction; use a
//!   solve-level comparison before changing production defaults.
//! - Build/query timings are machine-dependent. Query timings measure cold
//!   first assignments, including rollout estimation.
//! - The builder may converge before its iteration cap, so equal results at
//!   20 and 40 iterations are expected and meaningful.
//! - The Tournament anchor does not cover the final uniform-256 recommendation
//!   (nor the provisional 24..=256 opponent-specific profile). It tests
//!   whether the production 8/20 training defaults should change at one
//!   representative budget, not a universal optimum for every bucket count.

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

const PROFILES: [Profile; 2] = [
    Profile {
        name: "tournament",
        buckets: 64,
        rollout_samples: 512,
    },
    Profile {
        name: "cash",
        buckets: 128,
        rollout_samples: 1_024,
    },
];
const POINTS_PER_BUCKET: [u32; 3] = [4, 8, 16];
const KMEANS_ITERATIONS: [u32; 3] = [10, 20, 40];
const CANDIDATE_SEEDS: [u64; 3] = [11, 29, 47];
const REFERENCE_ROLLOUTS: u32 = 32_768;
const REFERENCE_SEED: u64 = 0x2026_0723_5a17;
const SCENARIO_SEED: u64 = 0x5a17_2026_0723;
const SCENARIOS_PER_GROUP: usize = 256;
const REFERENCE_STRATA: usize = 4;

type Group = (usize, u8);

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    buckets: u32,
    rollout_samples: u32,
}

#[derive(Clone)]
struct Scenario {
    street: Street,
    board: Vec<Card>,
    combo: usize,
    opponents: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct GroupMetrics {
    rmse: f64,
    p95_component_error: f64,
    strata_agreement: f64,
    occupied_fraction: f64,
}

#[derive(Clone, Copy, Debug)]
struct ModelRun {
    worst_rmse: f64,
    worst_p95_component_error: f64,
    worst_strata_agreement: f64,
    mean_occupied_fraction: f64,
    build_seconds: f64,
    query_micros_per_state: f64,
}

fn main() {
    eprintln!(
        "caveat=rollout-feature surrogate on uniform physical deals; \
         not a solve-level, reach-weighted, ICM, or rake-aware comparison"
    );
    eprintln!(
        "caveat=timings are machine-dependent and query timing includes cold rollout assignments"
    );

    let grouped_scenarios = scenarios();
    let reference_model = build_reference_model();
    let reference = reference_features(&reference_model, &grouped_scenarios);
    let strata = reference_strata(&reference);

    println!(
        "GLOBAL profile,buckets,rollouts,points_per_bucket,kmeans_iterations,\
         worst_rmse,worst_p95_component_error,worst_strata_agreement,\
         mean_occupied_fraction,mean_build_seconds,mean_query_micros_per_state"
    );

    for profile in PROFILES {
        for points_per_bucket in POINTS_PER_BUCKET {
            for kmeans_iterations in KMEANS_ITERATIONS {
                let runs: Vec<ModelRun> = CANDIDATE_SEEDS
                    .into_iter()
                    .map(|seed| {
                        evaluate_model(
                            profile,
                            points_per_bucket,
                            kmeans_iterations,
                            seed,
                            &grouped_scenarios,
                            &reference,
                            &strata,
                        )
                    })
                    .collect();

                println!(
                    "GLOBAL {},{},{},{points_per_bucket},{kmeans_iterations},\
                     {:.6},{:.6},{:.4},{:.4},{:.3},{:.1}",
                    profile.name,
                    profile.buckets,
                    profile.rollout_samples,
                    runs.iter()
                        .map(|run| run.worst_rmse)
                        .fold(f64::NEG_INFINITY, f64::max),
                    runs.iter()
                        .map(|run| run.worst_p95_component_error)
                        .fold(f64::NEG_INFINITY, f64::max),
                    runs.iter()
                        .map(|run| run.worst_strata_agreement)
                        .fold(f64::INFINITY, f64::min),
                    mean(runs.iter().map(|run| run.mean_occupied_fraction)),
                    mean(runs.iter().map(|run| run.build_seconds)),
                    mean(runs.iter().map(|run| run.query_micros_per_state)),
                );
            }
        }
    }
}

fn build_reference_model() -> RolloutKMeansAbstraction {
    let params = RolloutKMeansParams {
        flop_buckets: 1,
        turn_buckets: 1,
        river_buckets: 1,
        rollout_samples: REFERENCE_ROLLOUTS,
        seed: REFERENCE_SEED,
    };
    RolloutKMeansBuilder::new(params)
        .points_per_bucket(1)
        .kmeans_iterations(1)
        .build()
        .expect("valid reference model")
}

fn build_candidate(
    profile: Profile,
    points_per_bucket: u32,
    kmeans_iterations: u32,
    seed: u64,
) -> (RolloutKMeansAbstraction, f64) {
    let params = RolloutKMeansParams {
        flop_buckets: profile.buckets,
        turn_buckets: profile.buckets,
        river_buckets: profile.buckets,
        rollout_samples: profile.rollout_samples,
        seed,
    };
    let start = Instant::now();
    let model = RolloutKMeansBuilder::new(params)
        .points_per_bucket(points_per_bucket)
        .kmeans_iterations(kmeans_iterations)
        .build()
        .expect("valid candidate model");
    (model, start.elapsed().as_secs_f64())
}

fn evaluate_model(
    profile: Profile,
    points_per_bucket: u32,
    kmeans_iterations: u32,
    seed: u64,
    scenarios: &BTreeMap<Group, Vec<Scenario>>,
    reference: &BTreeMap<Group, Vec<RolloutFeatures>>,
    strata: &BTreeMap<Group, Vec<RolloutFeatures>>,
) -> ModelRun {
    let (model, build_seconds) =
        build_candidate(profile, points_per_bucket, kmeans_iterations, seed);
    let query_start = Instant::now();
    let metrics: Vec<GroupMetrics> = scenarios
        .iter()
        .map(|(&group, group_scenarios)| {
            compare_group(
                &model,
                group_scenarios,
                &reference[&group],
                &strata[&group],
                profile.buckets,
            )
        })
        .collect();
    let scenario_count = scenarios.values().map(Vec::len).sum::<usize>();

    ModelRun {
        worst_rmse: metrics
            .iter()
            .map(|metrics| metrics.rmse)
            .fold(f64::NEG_INFINITY, f64::max),
        worst_p95_component_error: metrics
            .iter()
            .map(|metrics| metrics.p95_component_error)
            .fold(f64::NEG_INFINITY, f64::max),
        worst_strata_agreement: metrics
            .iter()
            .map(|metrics| metrics.strata_agreement)
            .fold(f64::INFINITY, f64::min),
        mean_occupied_fraction: mean(metrics.iter().map(|metrics| metrics.occupied_fraction)),
        build_seconds,
        query_micros_per_state: query_start.elapsed().as_secs_f64() * 1_000_000.0
            / scenario_count as f64,
    }
}

fn scenarios() -> BTreeMap<Group, Vec<Scenario>> {
    let mut rng = ChaCha20Rng::seed_from_u64(SCENARIO_SEED);
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
) -> GroupMetrics {
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
    GroupMetrics {
        rmse: (squared_error / component_errors.len() as f64).sqrt(),
        p95_component_error: component_errors[p95_index],
        strata_agreement: matching_strata as f64 / scenarios.len() as f64,
        occupied_fraction: occupied.iter().filter(|&&value| value).count() as f64
            / occupied.len() as f64,
    }
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.collect();
    values.iter().sum::<f64>() / values.len() as f64
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
