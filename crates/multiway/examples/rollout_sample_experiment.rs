//! Reproducible quality/runtime experiment for the rollout sample default.
//!
//! Run with:
//! `cargo run --release -p multiway --example rollout_sample_experiment`

use std::collections::BTreeMap;
use std::time::Instant;

use cards::{ALL_CARDS, Card, combo_index};
use multiway::{
    BucketContext, RolloutFeatures, RolloutKMeansAbstraction, RolloutKMeansBuilder,
    RolloutKMeansParams, Street,
};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;

const CANDIDATES: [u32; 7] = [128, 256, 512, 1_024, 2_048, 4_096, 10_000];
const CANDIDATE_SEEDS: [u64; 3] = [11, 29, 47];
const REFERENCE_SAMPLES: u32 = 32_768;
const REFERENCE_SEED: u64 = 20_260_721;
const SCENARIOS_PER_GROUP: usize = 12;
const REFERENCE_CLUSTERS: usize = 4;

// These thresholds were fixed before running the experiment. The selected
// default is the smallest candidate for which every independent seed passes.
const MAX_RMSE: f64 = 0.015;
const MAX_P95_COMPONENT_ERROR: f64 = 0.035;
const MIN_ASSIGNMENT_AGREEMENT: f64 = 0.90;

#[derive(Clone)]
struct Scenario {
    street: Street,
    board: Vec<Card>,
    combo: usize,
    opponents: u8,
}

#[derive(Default)]
struct Metrics {
    rmse: f64,
    p95_component_error: f64,
    assignment_agreement: f64,
    build_seconds: f64,
    query_micros_per_state: f64,
}

fn main() {
    let scenarios = scenarios();
    let reference_model = build_model(REFERENCE_SAMPLES, REFERENCE_SEED).0;
    let reference = evaluate(&reference_model, &scenarios).0;
    let reference_centroids = reference_centroids(&scenarios, &reference);

    println!(
        "samples,mean_rmse,worst_rmse,mean_p95_component_error,worst_p95_component_error,\
         mean_assignment_agreement,worst_assignment_agreement,mean_build_seconds,\
         mean_query_micros_per_state,all_seeds_pass"
    );

    for samples in CANDIDATES {
        let mut runs = Vec::new();
        for seed in CANDIDATE_SEEDS {
            let (model, build_seconds) = build_model(samples, seed);
            let (features, query_micros_per_state) = evaluate(&model, &scenarios);
            let mut metric = compare(&scenarios, &features, &reference, &reference_centroids);
            metric.build_seconds = build_seconds;
            metric.query_micros_per_state = query_micros_per_state;
            runs.push(metric);
        }

        let mean =
            |field: fn(&Metrics) -> f64| runs.iter().map(field).sum::<f64>() / runs.len() as f64;
        let worst_high =
            |field: fn(&Metrics) -> f64| runs.iter().map(field).fold(f64::NEG_INFINITY, f64::max);
        let worst_low =
            |field: fn(&Metrics) -> f64| runs.iter().map(field).fold(f64::INFINITY, f64::min);
        let passes = runs.iter().all(|run| {
            run.rmse <= MAX_RMSE
                && run.p95_component_error <= MAX_P95_COMPONENT_ERROR
                && run.assignment_agreement >= MIN_ASSIGNMENT_AGREEMENT
        });
        println!(
            "{samples},{:.6},{:.6},{:.6},{:.6},{:.4},{:.4},{:.3},{:.1},{passes}",
            mean(|m| m.rmse),
            worst_high(|m| m.rmse),
            mean(|m| m.p95_component_error),
            worst_high(|m| m.p95_component_error),
            mean(|m| m.assignment_agreement),
            worst_low(|m| m.assignment_agreement),
            mean(|m| m.build_seconds),
            mean(|m| m.query_micros_per_state),
        );
    }
}

fn build_model(samples: u32, seed: u64) -> (RolloutKMeansAbstraction, f64) {
    let params = RolloutKMeansParams {
        flop_buckets: 1,
        turn_buckets: 1,
        river_buckets: 1,
        rollout_samples: samples,
        seed,
    };
    let start = Instant::now();
    let model = RolloutKMeansBuilder::new(params)
        .points_per_bucket(1)
        .kmeans_iterations(1)
        .build()
        .expect("valid experiment model");
    (model, start.elapsed().as_secs_f64())
}

fn evaluate(
    model: &RolloutKMeansAbstraction,
    scenarios: &[Scenario],
) -> (Vec<RolloutFeatures>, f64) {
    let start = Instant::now();
    let features = scenarios
        .iter()
        .map(|scenario| {
            model
                .rollout_features(BucketContext {
                    street: scenario.street,
                    board: &scenario.board,
                    combo: scenario.combo,
                    active_opponents: scenario.opponents,
                })
                .expect("valid experiment scenario")
        })
        .collect();
    let micros = start.elapsed().as_secs_f64() * 1_000_000.0 / scenarios.len() as f64;
    (features, micros)
}

fn scenarios() -> Vec<Scenario> {
    let mut rng = ChaCha20Rng::seed_from_u64(0x5a17_2026_0721);
    let mut result = Vec::new();
    for street in [Street::Flop, Street::Turn, Street::River] {
        for opponents in [1, 2, 5, 8] {
            for _ in 0..SCENARIOS_PER_GROUP {
                let mut deck: Vec<Card> = ALL_CARDS.into_iter().collect();
                deck.shuffle(&mut rng);
                let board_len = match street {
                    Street::Flop => 3,
                    Street::Turn => 4,
                    Street::River => 5,
                    Street::Preflop => unreachable!(),
                };
                result.push(Scenario {
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

type Group = (usize, u8);

fn reference_centroids(
    scenarios: &[Scenario],
    reference: &[RolloutFeatures],
) -> BTreeMap<Group, Vec<[f64; 4]>> {
    let mut groups: BTreeMap<Group, Vec<[f64; 4]>> = BTreeMap::new();
    for (scenario, feature) in scenarios.iter().zip(reference) {
        groups
            .entry((scenario.street.index(), scenario.opponents))
            .or_default()
            .push(feature.as_array());
    }
    groups
        .into_iter()
        .map(|(group, points)| (group, kmeans(&points, REFERENCE_CLUSTERS)))
        .collect()
}

fn compare(
    scenarios: &[Scenario],
    candidate: &[RolloutFeatures],
    reference: &[RolloutFeatures],
    centroids: &BTreeMap<Group, Vec<[f64; 4]>>,
) -> Metrics {
    let mut squared_error = 0.0;
    let mut component_errors = Vec::with_capacity(candidate.len() * 4);
    let mut matching_assignments = 0;
    for ((scenario, candidate), reference) in scenarios.iter().zip(candidate).zip(reference) {
        let candidate = candidate.as_array();
        let reference = reference.as_array();
        for (left, right) in candidate.into_iter().zip(reference) {
            let error = (left - right).abs();
            squared_error += error * error;
            component_errors.push(error);
        }
        let group = (scenario.street.index(), scenario.opponents);
        let group_centroids = &centroids[&group];
        matching_assignments +=
            usize::from(nearest(candidate, group_centroids) == nearest(reference, group_centroids));
    }
    component_errors.sort_by(f64::total_cmp);
    let p95_index = ((component_errors.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(component_errors.len() - 1);
    Metrics {
        rmse: (squared_error / component_errors.len() as f64).sqrt(),
        p95_component_error: component_errors[p95_index],
        assignment_agreement: matching_assignments as f64 / scenarios.len() as f64,
        ..Metrics::default()
    }
}

fn kmeans(points: &[[f64; 4]], clusters: usize) -> Vec<[f64; 4]> {
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
            for (sum, value) in sums[cluster].iter_mut().zip(point) {
                *sum += value;
            }
        }
        for cluster in 0..clusters {
            if counts[cluster] > 0 {
                for value in &mut sums[cluster] {
                    *value /= counts[cluster] as f64;
                }
                centroids[cluster] = sums[cluster];
            }
        }
    }
    centroids
}

fn nearest(point: [f64; 4], centroids: &[[f64; 4]]) -> usize {
    centroids
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            distance(point, **left).total_cmp(&distance(point, **right))
        })
        .map(|(index, _)| index)
        .expect("non-empty centroids")
}

fn nearest_distance(point: [f64; 4], centroids: &[[f64; 4]]) -> f64 {
    centroids
        .iter()
        .map(|&centroid| distance(point, centroid))
        .fold(f64::INFINITY, f64::min)
}

fn distance(left: [f64; 4], right: [f64; 4]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| (left - right).powi(2))
        .sum()
}
