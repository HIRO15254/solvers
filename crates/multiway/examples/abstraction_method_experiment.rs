//! Historical research-only feature-level comparison of EHS² percentile and
//! the retired opponent-aware multiway rollout abstraction.
//!
//! Run from the workspace root with:
//! `cargo run --release -p multiway --features research-abstractions \
//!   --example abstraction_method_experiment`
//!
//! The EHS² method loads or builds its full canonical flop/turn/river table at
//! `target/abstraction-experiment/ehs2-64.postcard`. The first cold
//! build is intentionally expensive; later runs reuse the validated cache.
//!
//! IMPORTANT: the target being predicted is a 32,768-sample *multiway rollout*
//! feature vector. That makes this a rollout-aligned surrogate comparison and
//! can favor the rollout method. It is not a solve-level strategy, EV, regret,
//! or exploitability comparison; those require separate paired solves.

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use abstraction::{Ehs2Abstraction, Ehs2Params};
use cards::{ALL_CARDS, Card, Street as CardStreet, combo_index};
use multiway::{
    BucketContext, MultiwayAbstraction, RolloutKMeansAbstraction, RolloutKMeansBuilder,
    RolloutKMeansParams, Street, TableAbstractionAdapter, ehs2_table_fingerprint,
};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha20Rng;

const BUCKETS: u32 = 64;
const ROLLOUT_SAMPLES: u32 = 512;
const ROLLOUT_SEEDS: [u64; 3] = [11, 29, 47];
const REFERENCE_SAMPLES: u32 = 32_768;
const REFERENCE_SEED: u64 = 20_260_721;
const SCENARIO_SEED: u64 = 0x5a17_2026_0723;
const CALIBRATION_PER_GROUP: usize = 256;
const TEST_PER_GROUP: usize = 128;
const STRATA: usize = 4;

type Group = (usize, u8);

#[derive(Clone)]
struct Scenario {
    street: Street,
    board: Vec<Card>,
    combo: usize,
    active_opponents: u8,
}

struct ReferencedScenario {
    scenario: Scenario,
    reference: [f64; 4],
}

#[derive(Clone, Copy, Default)]
struct FeatureSum {
    sum: [f64; 4],
    count: u64,
}

impl FeatureSum {
    fn add(&mut self, features: [f64; 4]) {
        for (sum, value) in self.sum.iter_mut().zip(features) {
            *sum += value;
        }
        self.count += 1;
    }

    fn mean(self) -> [f64; 4] {
        let count = self.count as f64;
        self.sum.map(|value| value / count)
    }
}

#[derive(Default)]
struct MetricAccumulator {
    total: u64,
    covered: u64,
    squared_error: f64,
    component_errors: Vec<f64>,
    matching_strata: u64,
}

impl MetricAccumulator {
    fn record(&mut self, prediction: Option<[f64; 4]>, reference: [f64; 4], strata: &[[f64; 4]]) {
        self.total += 1;
        let Some(prediction) = prediction else {
            return;
        };
        self.covered += 1;
        for (predicted, expected) in prediction.into_iter().zip(reference) {
            let error = (predicted - expected).abs();
            self.squared_error += error * error;
            self.component_errors.push(error);
        }
        self.matching_strata +=
            u64::from(nearest(prediction, strata) == nearest(reference, strata));
    }

    fn summary(&mut self) -> MetricSummary {
        self.component_errors.sort_by(f64::total_cmp);
        let coverage = if self.total == 0 {
            f64::NAN
        } else {
            self.covered as f64 / self.total as f64
        };
        if self.covered == 0 {
            return MetricSummary {
                total: self.total,
                covered: 0,
                coverage,
                rmse: f64::NAN,
                p95_component_error: f64::NAN,
                strata_agreement: f64::NAN,
            };
        }
        let p95_index = ((self.component_errors.len() as f64 * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(self.component_errors.len() - 1);
        MetricSummary {
            total: self.total,
            covered: self.covered,
            coverage,
            rmse: (self.squared_error / (self.covered * 4) as f64).sqrt(),
            p95_component_error: self.component_errors[p95_index],
            strata_agreement: self.matching_strata as f64 / self.covered as f64,
        }
    }
}

struct MetricSummary {
    total: u64,
    covered: u64,
    coverage: f64,
    rmse: f64,
    p95_component_error: f64,
    strata_agreement: f64,
}

fn main() -> Result<(), Box<dyn Error>> {
    let requested_cache_path = ehs2_cache_path();
    let cache_file_name = requested_cache_path
        .file_name()
        .expect("EHS2 cache path has a file name");
    let cache_parent = requested_cache_path
        .parent()
        .expect("EHS2 cache path has a parent");
    std::fs::create_dir_all(cache_parent)?;
    let cache_path = cache_parent.canonicalize()?.join(cache_file_name);

    println!(
        "# WARNING: target=32768-sample multiway rollout features; this rollout-aligned surrogate \
         may favor the rollout method and is not a solve-level comparison"
    );
    println!("# EHS2 is opponent-agnostic; rollout k-means is active-opponent-aware");
    println!("# ehs2_cache={}", cache_path.display());
    println!(
        "# calibration_per_street_opponents={CALIBRATION_PER_GROUP},\
         test_per_street_opponents={TEST_PER_GROUP},reference_samples={REFERENCE_SAMPLES},\
         rollout_samples={ROLLOUT_SAMPLES},rollout_seeds=11|29|47"
    );
    println!(
        "method,seed,scope,street,active_opponents,calibration_scenarios,test_scenarios,\
         covered_test_scenarios,coverage,rmse,p95_component_abs_error,\
         four_strata_agreement,build_seconds,query_micros_per_state,opponent_aware"
    );
    std::io::stdout().flush()?;

    let params = Ehs2Params {
        flop_buckets: BUCKETS,
        turn_buckets: BUCKETS,
        river_buckets: BUCKETS,
    };
    let cache_existed = cache_path.exists();
    eprintln!(
        "EHS2: {} full canonical F/T/R cache at {}",
        if cache_existed {
            "loading or validating"
        } else {
            "building"
        },
        cache_path.display()
    );
    let start = Instant::now();
    let ehs2 = Ehs2Abstraction::load_or_build(
        params,
        &[CardStreet::Flop, CardStreet::Turn, CardStreet::River],
        Some(&cache_path),
    );
    let ehs2_build_seconds = start.elapsed().as_secs_f64();
    let ehs2 = TableAbstractionAdapter::new(ehs2, ehs2_table_fingerprint(params));
    eprintln!("EHS2 ready in {ehs2_build_seconds:.3}s");

    let (calibration, test) = physical_scenarios();
    eprintln!(
        "reference: evaluating {} calibration and {} test street/opponent scenarios",
        calibration.len(),
        test.len()
    );
    let start = Instant::now();
    let reference_model = build_rollout(1, REFERENCE_SAMPLES, REFERENCE_SEED, 1, 1)?;
    let calibration = attach_reference(&reference_model, calibration)?;
    let test = attach_reference(&reference_model, test)?;
    let reference_seconds = start.elapsed().as_secs_f64();
    eprintln!("reference ready in {reference_seconds:.3}s");

    // Four-strata agreement uses deterministic 4-means centroids trained only
    // on calibration reference features within each (street, opponents)
    // group. Test scenarios never influence either the bucket conditional mean
    // or the strata boundaries.
    let strata = reference_strata(&calibration);
    evaluate_method(
        "ehs2-percentile",
        "none",
        false,
        &ehs2,
        ehs2_build_seconds,
        &calibration,
        &test,
        &strata,
    );

    for seed in ROLLOUT_SEEDS {
        eprintln!("rollout-kmeans seed {seed}: building");
        let start = Instant::now();
        let rollout = build_rollout(BUCKETS, ROLLOUT_SAMPLES, seed, 8, 20)?;
        let build_seconds = start.elapsed().as_secs_f64();
        eprintln!("rollout-kmeans seed {seed}: ready in {build_seconds:.3}s");
        evaluate_method(
            "multiway-rollout",
            &seed.to_string(),
            true,
            &rollout,
            build_seconds,
            &calibration,
            &test,
            &strata,
        );
    }

    Ok(())
}

fn ehs2_cache_path() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("abstraction-experiment/ehs2-64.postcard")
}

fn build_rollout(
    buckets: u32,
    samples: u32,
    seed: u64,
    points_per_bucket: u32,
    kmeans_iterations: u32,
) -> Result<RolloutKMeansAbstraction, Box<dyn Error>> {
    let params = RolloutKMeansParams {
        flop_buckets: buckets,
        turn_buckets: buckets,
        river_buckets: buckets,
        rollout_samples: samples,
        seed,
    };
    Ok(RolloutKMeansBuilder::new(params)
        .points_per_bucket(points_per_bucket)
        .kmeans_iterations(kmeans_iterations)
        .build()?)
}

fn physical_scenarios() -> (Vec<Scenario>, Vec<Scenario>) {
    let mut rng = ChaCha20Rng::seed_from_u64(SCENARIO_SEED);
    let mut calibration = Vec::with_capacity(3 * 8 * CALIBRATION_PER_GROUP);
    let mut test = Vec::with_capacity(3 * 8 * TEST_PER_GROUP);
    for street in [Street::Flop, Street::Turn, Street::River] {
        let board_len = match street {
            Street::Flop => 3,
            Street::Turn => 4,
            Street::River => 5,
            Street::Preflop => unreachable!(),
        };
        for index in 0..(CALIBRATION_PER_GROUP + TEST_PER_GROUP) {
            let mut deck: Vec<Card> = ALL_CARDS.into_iter().collect();
            deck.shuffle(&mut rng);
            let board = deck[2..2 + board_len].to_vec();
            let combo = combo_index(deck[0], deck[1]);
            for active_opponents in 1..=8 {
                let scenario = Scenario {
                    street,
                    board: board.clone(),
                    combo,
                    active_opponents,
                };
                if index < CALIBRATION_PER_GROUP {
                    calibration.push(scenario);
                } else {
                    test.push(scenario);
                }
            }
        }
    }
    (calibration, test)
}

fn attach_reference(
    model: &RolloutKMeansAbstraction,
    scenarios: Vec<Scenario>,
) -> Result<Vec<ReferencedScenario>, Box<dyn Error>> {
    scenarios
        .into_iter()
        .enumerate()
        .map(|(index, scenario)| {
            if index > 0 && index % 1_024 == 0 {
                eprintln!("reference: {index} scenarios complete");
            }
            let features = model.rollout_features(context(&scenario))?.as_array();
            Ok(ReferencedScenario {
                scenario,
                reference: features,
            })
        })
        .collect()
}

fn reference_strata(calibration: &[ReferencedScenario]) -> BTreeMap<Group, Vec<[f64; 4]>> {
    let mut points: BTreeMap<Group, Vec<[f64; 4]>> = BTreeMap::new();
    for scenario in calibration {
        points
            .entry(group(&scenario.scenario))
            .or_default()
            .push(scenario.reference);
    }
    points
        .into_iter()
        .map(|(group, points)| (group, kmeans(&points, STRATA)))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn evaluate_method<A: MultiwayAbstraction>(
    method: &str,
    seed: &str,
    opponent_aware: bool,
    abstraction: &A,
    build_seconds: f64,
    calibration: &[ReferencedScenario],
    test: &[ReferencedScenario],
    strata: &BTreeMap<Group, Vec<[f64; 4]>>,
) {
    let query_start = Instant::now();
    let mut conditional_sums: BTreeMap<(Group, u32), FeatureSum> = BTreeMap::new();
    let mut calibration_counts: BTreeMap<Group, u64> = BTreeMap::new();
    for scenario in calibration {
        let group = group(&scenario.scenario);
        let bucket = abstraction.bucket(context(&scenario.scenario));
        conditional_sums
            .entry((group, bucket))
            .or_default()
            .add(scenario.reference);
        *calibration_counts.entry(group).or_default() += 1;
    }
    let conditional_means: BTreeMap<_, _> = conditional_sums
        .into_iter()
        .map(|(key, sum)| (key, sum.mean()))
        .collect();

    let mut groups: BTreeMap<Group, MetricAccumulator> = BTreeMap::new();
    let mut global = MetricAccumulator::default();
    for scenario in test {
        let group = group(&scenario.scenario);
        let bucket = abstraction.bucket(context(&scenario.scenario));
        let prediction = conditional_means.get(&(group, bucket)).copied();
        let centroids = &strata[&group];
        groups
            .entry(group)
            .or_default()
            .record(prediction, scenario.reference, centroids);
        global.record(prediction, scenario.reference, centroids);
    }
    let query_seconds = query_start.elapsed().as_secs_f64();
    let queries = calibration.len() + test.len();
    let query_micros_per_state = query_seconds * 1_000_000.0 / queries as f64;

    for (group, metrics) in &mut groups {
        let summary = metrics.summary();
        print_row(
            method,
            seed,
            "street-opponents",
            street_name(group.0),
            &group.1.to_string(),
            calibration_counts[group],
            &summary,
            build_seconds,
            query_micros_per_state,
            opponent_aware,
        );
    }
    let summary = global.summary();
    print_row(
        method,
        seed,
        "global",
        "all",
        "all",
        calibration.len() as u64,
        &summary,
        build_seconds,
        query_micros_per_state,
        opponent_aware,
    );
    let _ = std::io::stdout().flush();
}

#[allow(clippy::too_many_arguments)]
fn print_row(
    method: &str,
    seed: &str,
    scope: &str,
    street: &str,
    active_opponents: &str,
    calibration_scenarios: u64,
    summary: &MetricSummary,
    build_seconds: f64,
    query_micros_per_state: f64,
    opponent_aware: bool,
) {
    println!(
        "{method},{seed},{scope},{street},{active_opponents},{calibration_scenarios},{},{},\
         {:.6},{:.6},{:.6},{:.6},{build_seconds:.3},{query_micros_per_state:.3},\
         {opponent_aware}",
        summary.total,
        summary.covered,
        summary.coverage,
        summary.rmse,
        summary.p95_component_error,
        summary.strata_agreement,
    );
}

fn context(scenario: &Scenario) -> BucketContext<'_> {
    BucketContext {
        street: scenario.street,
        board: &scenario.board,
        combo: scenario.combo,
        active_opponents: scenario.active_opponents,
    }
}

fn group(scenario: &Scenario) -> Group {
    (scenario.street.index(), scenario.active_opponents)
}

fn street_name(index: usize) -> &'static str {
    match index {
        1 => "flop",
        2 => "turn",
        3 => "river",
        _ => unreachable!("experiment only contains postflop streets"),
    }
}

fn kmeans(points: &[[f64; 4]], clusters: usize) -> Vec<[f64; 4]> {
    assert!(points.len() >= clusters);
    let mut centroids = vec![points[0]];
    while centroids.len() < clusters {
        centroids.push(
            *points
                .iter()
                .max_by(|left, right| {
                    nearest_distance(**left, &centroids)
                        .total_cmp(&nearest_distance(**right, &centroids))
                })
                .expect("calibration group is non-empty"),
        );
    }
    for _ in 0..20 {
        let mut sums = vec![[0.0; 4]; clusters];
        let mut counts = vec![0usize; clusters];
        for &point in points {
            let cluster = nearest(point, &centroids);
            for (sum, value) in sums[cluster].iter_mut().zip(point) {
                *sum += value;
            }
            counts[cluster] += 1;
        }
        for cluster in 0..clusters {
            if counts[cluster] > 0 {
                let count = counts[cluster] as f64;
                centroids[cluster] = sums[cluster].map(|value| value / count);
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
            squared_distance(point, **left).total_cmp(&squared_distance(point, **right))
        })
        .map(|(index, _)| index)
        .expect("strata centroids are non-empty")
}

fn nearest_distance(point: [f64; 4], centroids: &[[f64; 4]]) -> f64 {
    centroids
        .iter()
        .map(|&centroid| squared_distance(point, centroid))
        .fold(f64::INFINITY, f64::min)
}

fn squared_distance(left: [f64; 4], right: [f64; 4]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| {
            let delta = left - right;
            delta * delta
        })
        .sum()
}
