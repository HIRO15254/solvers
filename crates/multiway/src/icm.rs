//! Hybrid Independent Chip Model evaluation.
//!
//! Fields through 15 players use exact subset dynamic programming.  Larger
//! fields use deterministic weighted finish-order sampling with a Fenwick
//! tree, keeping each sample at `O(n log n)` through the 100-player limit.

use rand::seq::SliceRandom;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{MwChips, SeatId, SeatVec};

pub const EXACT_ICM_MAX_PLAYERS: usize = 15;
pub const ICM_MAX_PLAYERS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IcmMode {
    Exact,
    Sampled { samples: u64, seed: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IcmEstimate {
    pub values: Vec<f64>,
    pub standard_errors: Vec<f64>,
    pub ci95: Vec<[f64; 2]>,
    pub mode: IcmMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IcmDeltaEstimate {
    pub deltas: SeatVec<f64>,
    pub standard_errors: SeatVec<f64>,
    pub ci95: SeatVec<[f64; 2]>,
    pub baseline_values: SeatVec<f64>,
    pub terminal_values: SeatVec<f64>,
}

/// Automatically selects exact DP for fields through 15 and deterministic
/// Monte Carlo for fields 16 through 100.
pub fn estimate_icm(
    stacks: &[MwChips],
    payouts: &[f64],
    samples: u64,
    seed: u64,
) -> Result<IcmEstimate, IcmError> {
    validate_inputs(stacks, payouts)?;
    if stacks.len() <= EXACT_ICM_MAX_PLAYERS {
        let values = exact_icm(stacks, payouts);
        let ci95 = values.iter().map(|&value| [value, value]).collect();
        Ok(IcmEstimate {
            values,
            standard_errors: vec![0.0; stacks.len()],
            ci95,
            mode: IcmMode::Exact,
        })
    } else {
        if samples < 2 {
            return Err(IcmError::TooFewSamples(samples));
        }
        sampled_icm(stacks, payouts, samples, seed)
    }
}

fn exact_icm(stacks: &[MwChips], payouts: &[f64]) -> Vec<f64> {
    let n = stacks.len();
    let states = 1usize << n;
    let total: u128 = stacks.iter().map(|stack| stack.raw() as u128).sum();
    if total == 0 {
        let equal = payouts.iter().sum::<f64>() / n as f64;
        return vec![equal; n];
    }
    let mut selected_weight = vec![0u128; states];
    for mask in 1..states {
        let bit = mask.trailing_zeros() as usize;
        selected_weight[mask] = selected_weight[mask & (mask - 1)] + stacks[bit].raw() as u128;
    }
    let mut probability = vec![0.0f64; states];
    let mut values = vec![0.0f64; n];
    probability[0] = 1.0;
    for mask in 0..states {
        let path_probability = probability[mask];
        if path_probability == 0.0 {
            continue;
        }
        let place = mask.count_ones() as usize;
        if place == n {
            continue;
        }
        let remaining = total - selected_weight[mask];
        if remaining == 0 {
            let average = payouts[place..].iter().sum::<f64>() / (n - place) as f64;
            for (player, value) in values.iter_mut().enumerate() {
                if mask & (1usize << player) == 0 {
                    *value += path_probability * average;
                }
            }
            continue;
        }
        for player in 0..n {
            if mask & (1usize << player) != 0 || stacks[player] == MwChips::ZERO {
                continue;
            }
            let choice = stacks[player].raw() as f64 / remaining as f64;
            let transition = path_probability * choice;
            values[player] += transition * payouts[place];
            probability[mask | (1usize << player)] += transition;
        }
    }
    values
}

fn sampled_icm(
    stacks: &[MwChips],
    payouts: &[f64],
    samples: u64,
    seed: u64,
) -> Result<IcmEstimate, IcmError> {
    let n = stacks.len();
    let weights: Vec<u128> = stacks.iter().map(|stack| stack.raw() as u128).collect();
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut means = vec![0.0f64; n];
    let mut m2 = vec![0.0f64; n];
    let mut result = vec![0.0f64; n];
    for sample_index in 1..=samples {
        result.fill(0.0);
        let mut tree = Fenwick::new(&weights);
        let mut place = 0usize;
        while tree.total > 0 {
            let target = random_below(&mut rng, tree.total);
            let player = tree.find(target);
            result[player] = payouts[place];
            tree.remove(player, weights[player]);
            place += 1;
        }
        if place < n {
            let mut zero_players: Vec<usize> =
                (0..n).filter(|&player| weights[player] == 0).collect();
            zero_players.shuffle(&mut rng);
            for player in zero_players {
                result[player] = payouts[place];
                place += 1;
            }
        }
        let count = sample_index as f64;
        for player in 0..n {
            let delta = result[player] - means[player];
            means[player] += delta / count;
            m2[player] += delta * (result[player] - means[player]);
        }
    }
    let standard_errors = if samples > 1 {
        m2.into_iter()
            .map(|sum| (sum / (samples - 1) as f64 / samples as f64).sqrt())
            .collect()
    } else {
        vec![0.0; n]
    };
    let ci95 = means
        .iter()
        .zip(&standard_errors)
        .map(|(&mean, &stderr)| [mean - 1.96 * stderr, mean + 1.96 * stderr])
        .collect();
    Ok(IcmEstimate {
        values: means,
        standard_errors,
        ci95,
        mode: IcmMode::Sampled { samples, seed },
    })
}

/// Returns table-player ICM changes for a terminal chip result.  Players
/// busted in the same hand are ordered by their start-of-hand stacks; equal
/// stacks split the affected finish prizes.  Off-table players never bust in
/// this hand and retain their supplied stacks.
pub fn terminal_icm_delta(
    starting_table: &SeatVec<MwChips>,
    final_table: &SeatVec<MwChips>,
    outside_field: &[MwChips],
    payouts: &[f64],
    samples: u64,
    seed: u64,
) -> Result<IcmDeltaEstimate, IcmError> {
    let mut baseline_stacks = starting_table.as_slice().to_vec();
    baseline_stacks.extend_from_slice(outside_field);
    let baseline = estimate_icm(&baseline_stacks, payouts, samples, seed)?;
    terminal_icm_delta_with_baseline(
        starting_table,
        final_table,
        outside_field,
        payouts,
        samples,
        seed,
        &baseline,
    )
}

/// Terminal ICM delta using a start-of-hand estimate computed once by the
/// caller. This keeps the baseline out of the MCCFR terminal hot path.
pub(crate) fn terminal_icm_delta_with_baseline(
    starting_table: &SeatVec<MwChips>,
    final_table: &SeatVec<MwChips>,
    outside_field: &[MwChips],
    payouts: &[f64],
    samples: u64,
    seed: u64,
    baseline: &IcmEstimate,
) -> Result<IcmDeltaEstimate, IcmError> {
    if starting_table.len() != final_table.len() {
        return Err(IcmError::SeatCount);
    }
    let table_len = starting_table.len();
    let field_len = table_len + outside_field.len();
    if baseline.values.len() != field_len
        || baseline.standard_errors.len() != field_len
        || baseline.ci95.len() != field_len
    {
        return Err(IcmError::BaselineCount {
            expected: field_len,
            actual: baseline.values.len(),
        });
    }

    let mut terminal_values = vec![0.0f64; table_len];
    let mut terminal_errors = vec![0.0f64; table_len];
    let mut busted: Vec<SeatId> = starting_table
        .seats()
        .filter(|&seat| final_table[seat] == MwChips::ZERO)
        .collect();
    busted.sort_unstable_by_key(|&seat| (starting_table[seat], seat));

    let mut bottom = payouts.len();
    let mut index = 0usize;
    while index < busted.len() {
        let starting_stack = starting_table[busted[index]];
        let mut end = index + 1;
        while end < busted.len() && starting_table[busted[end]] == starting_stack {
            end += 1;
        }
        let group = end - index;
        let first_prize = bottom.checked_sub(group).ok_or(IcmError::PayoutCount {
            expected: field_len,
            actual: payouts.len(),
        })?;
        let split = payouts[first_prize..bottom].iter().sum::<f64>() / group as f64;
        for &seat in &busted[index..end] {
            terminal_values[seat.index()] = split;
        }
        bottom = first_prize;
        index = end;
    }

    let survivors: Vec<SeatId> = final_table
        .seats()
        .filter(|&seat| final_table[seat] > MwChips::ZERO)
        .collect();
    let mut survivor_stacks: Vec<MwChips> =
        survivors.iter().map(|&seat| final_table[seat]).collect();
    survivor_stacks.extend_from_slice(outside_field);
    if survivor_stacks.len() == 1 {
        if let Some(&seat) = survivors.first() {
            terminal_values[seat.index()] = payouts[0];
        }
    } else if !survivor_stacks.is_empty() {
        let survivor_estimate = estimate_icm(
            &survivor_stacks,
            &payouts[..bottom],
            samples,
            seed ^ 0x9e37_79b9_7f4a_7c15,
        )?;
        for (position, &seat) in survivors.iter().enumerate() {
            terminal_values[seat.index()] = survivor_estimate.values[position];
            terminal_errors[seat.index()] = survivor_estimate.standard_errors[position];
        }
    }
    let baseline_values = baseline.values[..table_len].to_vec();
    let deltas: Vec<f64> = (0..table_len)
        .map(|player| terminal_values[player] - baseline_values[player])
        .collect();
    let errors: Vec<f64> = (0..table_len)
        .map(|player| baseline.standard_errors[player].hypot(terminal_errors[player]))
        .collect();
    let ci95: Vec<[f64; 2]> = deltas
        .iter()
        .zip(&errors)
        .map(|(&delta, &stderr)| [delta - 1.96 * stderr, delta + 1.96 * stderr])
        .collect();
    Ok(IcmDeltaEstimate {
        deltas: SeatVec::new_unchecked(deltas),
        standard_errors: SeatVec::new_unchecked(errors),
        ci95: SeatVec::new_unchecked(ci95),
        baseline_values: SeatVec::new_unchecked(baseline_values),
        terminal_values: SeatVec::new_unchecked(terminal_values),
    })
}

fn validate_inputs(stacks: &[MwChips], payouts: &[f64]) -> Result<(), IcmError> {
    let players = stacks.len();
    if !(2..=ICM_MAX_PLAYERS).contains(&players) {
        return Err(IcmError::FieldSize(players));
    }
    if payouts.len() != players {
        return Err(IcmError::PayoutCount {
            expected: players,
            actual: payouts.len(),
        });
    }
    for (index, payout) in payouts.iter().copied().enumerate() {
        if !payout.is_finite() || payout < 0.0 {
            return Err(IcmError::InvalidPayout(index));
        }
        if index > 0 && payouts[index - 1] < payout {
            return Err(IcmError::PayoutOrder);
        }
    }
    Ok(())
}

struct Fenwick {
    tree: Vec<u128>,
    total: u128,
}

impl Fenwick {
    fn new(weights: &[u128]) -> Self {
        let mut result = Self {
            tree: vec![0; weights.len() + 1],
            total: 0,
        };
        for (index, &weight) in weights.iter().enumerate() {
            result.add(index, weight);
            result.total += weight;
        }
        result
    }

    fn add(&mut self, index: usize, value: u128) {
        let mut node = index + 1;
        while node < self.tree.len() {
            self.tree[node] += value;
            node += node & node.wrapping_neg();
        }
    }

    fn remove(&mut self, index: usize, value: u128) {
        let mut node = index + 1;
        while node < self.tree.len() {
            self.tree[node] -= value;
            node += node & node.wrapping_neg();
        }
        self.total -= value;
    }

    /// Finds the zero-based item containing cumulative offset `target`.
    fn find(&self, target: u128) -> usize {
        debug_assert!(target < self.total);
        let mut index = 0usize;
        let mut accumulated = 0u128;
        let mut step = 1usize;
        while step < self.tree.len() {
            step <<= 1;
        }
        let mut bit = step >> 1;
        while bit != 0 {
            let next = index + bit;
            if next < self.tree.len() && accumulated + self.tree[next] <= target {
                index = next;
                accumulated += self.tree[next];
            }
            bit >>= 1;
        }
        index
    }
}

fn random_below(rng: &mut impl RngCore, upper: u128) -> u128 {
    debug_assert!(upper > 0);
    let zone = u128::MAX - (u128::MAX % upper);
    loop {
        let value = ((rng.next_u64() as u128) << 64) | rng.next_u64() as u128;
        if value < zone {
            return value % upper;
        }
    }
}

#[derive(Debug, Error)]
pub enum IcmError {
    #[error("ICM field must contain 2 through 100 players, got {0}")]
    FieldSize(usize),
    #[error("ICM payouts length must be {expected}, got {actual}")]
    PayoutCount { expected: usize, actual: usize },
    #[error("payout {0} must be finite and non-negative")]
    InvalidPayout(usize),
    #[error("payouts must be ordered highest to lowest")]
    PayoutOrder,
    #[error("sampled ICM requires at least two samples, got {0}")]
    TooFewSamples(u64),
    #[error("table stack vectors have different seat counts")]
    SeatCount,
    #[error("ICM baseline length must be {expected}, got {actual}")]
    BaselineCount { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_heads_up_matches_chip_fraction() {
        let estimate = estimate_icm(&[MwChips(100), MwChips(300)], &[100.0, 0.0], 1, 0).unwrap();
        assert_eq!(estimate.mode, IcmMode::Exact);
        assert!((estimate.values[0] - 25.0).abs() < 1e-12);
        assert!((estimate.values[1] - 75.0).abs() < 1e-12);
        assert_eq!(estimate.ci95, vec![[25.0, 25.0], [75.0, 75.0]]);
    }

    #[test]
    fn exact_zero_stacks_split_remaining_places() {
        let estimate = estimate_icm(
            &[MwChips(100), MwChips::ZERO, MwChips::ZERO],
            &[60.0, 30.0, 10.0],
            1,
            0,
        )
        .unwrap();
        assert_eq!(estimate.values, vec![60.0, 20.0, 20.0]);
    }

    #[test]
    fn sampled_mode_is_deterministic_and_reports_error() {
        let stacks = vec![MwChips(1_000); 16];
        let mut payouts = vec![0.0; 16];
        payouts[0] = 100.0;
        let first = estimate_icm(&stacks, &payouts, 5_000, 77).unwrap();
        let second = estimate_icm(&stacks, &payouts, 5_000, 77).unwrap();
        assert_eq!(first, second);
        assert!(matches!(first.mode, IcmMode::Sampled { .. }));
        assert!(first.standard_errors.iter().any(|error| *error > 0.0));
        assert!((first.values.iter().sum::<f64>() - 100.0).abs() < 1e-9);
        for ((&mean, interval), &stderr) in first
            .values
            .iter()
            .zip(&first.ci95)
            .zip(&first.standard_errors)
        {
            assert!(interval[0] <= mean && mean <= interval[1]);
            assert!((interval[1] - interval[0] - 3.92 * stderr).abs() < 1e-10);
        }
    }

    fn enumerate_finish_orders(stacks: &[MwChips], payouts: &[f64]) -> Vec<f64> {
        fn walk(
            stacks: &[MwChips],
            payouts: &[f64],
            remaining: &mut Vec<usize>,
            place: usize,
            probability: f64,
            values: &mut [f64],
        ) {
            if remaining.is_empty() {
                return;
            }
            let total: u64 = remaining.iter().map(|&player| stacks[player].raw()).sum();
            if total == 0 {
                let average = payouts[place..].iter().sum::<f64>() / remaining.len() as f64;
                for &player in remaining.iter() {
                    values[player] += probability * average;
                }
                return;
            }
            for index in (0..remaining.len()).rev() {
                let player = remaining.remove(index);
                let choice = stacks[player].raw() as f64 / total as f64;
                values[player] += probability * choice * payouts[place];
                walk(
                    stacks,
                    payouts,
                    remaining,
                    place + 1,
                    probability * choice,
                    values,
                );
                remaining.insert(index, player);
            }
        }

        let mut values = vec![0.0; stacks.len()];
        let mut remaining: Vec<_> = (0..stacks.len()).collect();
        walk(stacks, payouts, &mut remaining, 0, 1.0, &mut values);
        values
    }

    #[test]
    fn exact_three_through_seven_match_finish_order_enumeration() {
        for players in 3..=7 {
            let stacks: Vec<_> = (0..players)
                .map(|player| MwChips((player as u64 + 1) * 137))
                .collect();
            let payouts: Vec<_> = (0..players)
                .map(|place| ((players - place) * 10) as f64)
                .collect();
            let expected = enumerate_finish_orders(&stacks, &payouts);
            let actual = estimate_icm(&stacks, &payouts, 1, 0).unwrap();
            assert_eq!(actual.mode, IcmMode::Exact);
            for (left, right) in actual.values.iter().zip(expected) {
                assert!((left - right).abs() < 1e-9, "players={players}");
            }
        }
    }

    #[test]
    fn exact_boundary_is_fifteen_players() {
        let payouts_15: Vec<_> = (0..15).rev().map(|place| place as f64).collect();
        let exact = estimate_icm(&[MwChips(100); 15], &payouts_15, 8, 5).unwrap();
        assert_eq!(exact.mode, IcmMode::Exact);
        let payouts_16: Vec<_> = (0..16).rev().map(|place| place as f64).collect();
        let sampled = estimate_icm(&[MwChips(100); 16], &payouts_16, 8, 5).unwrap();
        assert!(matches!(sampled.mode, IcmMode::Sampled { .. }));
    }

    #[test]
    fn sampled_mode_rejects_fewer_than_two_samples() {
        let stacks = [MwChips(100); 16];
        let payouts: Vec<_> = (0..16).rev().map(|place| place as f64).collect();
        for samples in [0, 1] {
            assert!(matches!(
                estimate_icm(&stacks, &payouts, samples, 5),
                Err(IcmError::TooFewSamples(actual)) if actual == samples
            ));
        }
    }

    #[test]
    fn exact_mode_does_not_require_monte_carlo_samples() {
        let estimate = estimate_icm(&[MwChips(100), MwChips(300)], &[100.0, 0.0], 0, 5).unwrap();
        assert_eq!(estimate.mode, IcmMode::Exact);
        assert_eq!(estimate.ci95, vec![[25.0, 25.0], [75.0, 75.0]]);
    }

    #[test]
    fn one_hundred_player_sample_smoke_test() {
        let stacks = vec![MwChips(1_000); 100];
        let payouts: Vec<f64> = (0..100).rev().map(|value| value as f64).collect();
        let estimate = estimate_icm(&stacks, &payouts, 32, 9).unwrap();
        assert_eq!(estimate.values.len(), 100);
    }

    #[test]
    fn simultaneous_busts_use_starting_stack_and_split_ties() {
        let starting = SeatVec::try_new(vec![MwChips(10), MwChips(10), MwChips(50)]).unwrap();
        let final_stacks =
            SeatVec::try_new(vec![MwChips::ZERO, MwChips::ZERO, MwChips(70)]).unwrap();
        let delta = terminal_icm_delta(
            &starting,
            &final_stacks,
            &[MwChips(30)],
            &[100.0, 50.0, 20.0, 0.0],
            1_000,
            1,
        )
        .unwrap();
        assert_eq!(delta.terminal_values[SeatId(0)], 10.0);
        assert_eq!(delta.terminal_values[SeatId(1)], 10.0);
        assert!(delta.terminal_values[SeatId(2)] > 50.0);
    }

    #[test]
    fn cached_baseline_matches_direct_delta_and_outside_stack_matters() {
        let starting = SeatVec::try_new(vec![MwChips(100), MwChips(200)]).unwrap();
        let final_stacks = SeatVec::try_new(vec![MwChips(250), MwChips(50)]).unwrap();
        let payouts = [100.0, 60.0, 0.0];
        let outside = [MwChips(300)];
        let mut baseline_stacks = starting.as_slice().to_vec();
        baseline_stacks.extend_from_slice(&outside);
        let baseline = estimate_icm(&baseline_stacks, &payouts, 1, 11).unwrap();
        let cached = terminal_icm_delta_with_baseline(
            &starting,
            &final_stacks,
            &outside,
            &payouts,
            1,
            11,
            &baseline,
        )
        .unwrap();
        let direct =
            terminal_icm_delta(&starting, &final_stacks, &outside, &payouts, 1, 11).unwrap();
        assert_eq!(cached, direct);

        let deep_outside = [MwChips(3_000)];
        let deep =
            terminal_icm_delta(&starting, &final_stacks, &deep_outside, &payouts, 1, 11).unwrap();
        assert_ne!(direct.deltas, deep.deltas);
    }

    #[test]
    fn terminal_icm_handles_one_remaining_player() {
        let starting = SeatVec::try_new(vec![MwChips(10), MwChips(20)]).unwrap();
        let final_stacks = SeatVec::try_new(vec![MwChips::ZERO, MwChips(30)]).unwrap();
        let delta =
            terminal_icm_delta(&starting, &final_stacks, &[], &[100.0, 0.0], 10, 4).unwrap();
        assert_eq!(delta.terminal_values[SeatId(0)], 0.0);
        assert_eq!(delta.terminal_values[SeatId(1)], 100.0);
        assert_eq!(delta.standard_errors[SeatId(1)], 0.0);
    }
}
