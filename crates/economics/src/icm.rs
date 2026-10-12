//! Independent Chip Model evaluation for one table inside a tournament field.
//!
//! Only the table's stacks change during a hand; off-table players keep their
//! supplied stacks. Fields through 15 players use exact subset dynamic
//! programming. Larger fields evaluate the integral representation of the
//! Plackett–Luce exponential race on the fixed double-exponential grid of
//! DE-ICM (W. Inariba, "Near-Exact Computation of Independent Chip Model
//! Placement Probabilities for Thousands of Players", arXiv:2610.06028).
//!
//! Player `i` finishes ahead of `j` when its clock `T_i ~ Exp(s_i)` is
//! smaller. Conditional on `T_i = t`, the number of players ahead is
//! Poisson-binomial with generating function `∏_{j≠i} (q_j + (1 − q_j) z)`,
//! `q_j = e^{−s_j t}`, so player `i`'s equity is
//!
//! ```text
//! V_i = ∫₀^∞ s_i e^{−s_i t} Σ_r π_{r+1} C_r^{∖i}(t) dt.
//! ```
//!
//! The off-table part of that product is identical at every terminal. Its
//! prize-weighted tails `P_k(t) = Σ_j π_{j+k+1} C_j^{out}(t)` are prepared
//! once per quadrature node; a terminal then multiplies at most nine table
//! factors per node. Every term is a sum of non-negative products, so the
//! leave-one-out coefficients need no deconvolution.

use nlh::{MAX_SEATS, MwChips, SeatId, SeatVec};
use thiserror::Error;

pub const EXACT_ICM_MAX_PLAYERS: usize = 15;
pub const ICM_MAX_PLAYERS: usize = 10_000;

/// Headroom `c`: the largest possible stack (the whole field's chips) makes
/// its clock transition at `t = c`, on the logarithmic branch of the grid.
const HEADROOM: f64 = 20.0;
/// Discretization constant `κ` of the step rule (paper, Equation 12).
const STEP_KAPPA: f64 = 28.0;
/// Target of the step rule: the double-precision rounding floor.
const STEP_TARGET: f64 = 1e-15;
/// Largest step; smaller fields alias their exponential tails above it.
const MAX_STEP: f64 = 0.125;
/// Omitted density mass at each end of the integration window.
const WINDOW_TARGET: f64 = 1e-16;
/// Poisson-binomial coefficients below this probability are dropped.
const TAIL_CUTOFF: f64 = 1e-40;

/// How a field is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcmMode {
    /// Exact subset dynamic programming, fields through 15 players.
    Exact,
    /// Fixed-grid double-exponential quadrature with `nodes` nodes.
    Quadrature { nodes: usize },
}

/// Evaluation mode for `field_players` players holding `total_chips` chips,
/// without preparing the field.
pub fn icm_mode(field_players: usize, total_chips: u128) -> IcmMode {
    if field_players <= EXACT_ICM_MAX_PLAYERS {
        IcmMode::Exact
    } else {
        IcmMode::Quadrature {
            nodes: Grid::new(field_players, total_chips).nodes,
        }
    }
}

/// Reusable ICM evaluator for the many terminal stack vectors of one table.
pub struct PreparedIcm {
    starting_table: SeatVec<MwChips>,
    outside_field: Vec<MwChips>,
    payouts: Vec<f64>,
    quadrature: Option<Quadrature>,
    baseline: SeatVec<f64>,
}

impl PreparedIcm {
    /// Prepares the field and evaluates the table's starting equities.
    /// `payouts` has one entry per field player, best finish first.
    pub fn new(
        starting_table: SeatVec<MwChips>,
        outside_field: Vec<MwChips>,
        payouts: Vec<f64>,
    ) -> Result<Self, IcmError> {
        let field = starting_table.len() + outside_field.len();
        validate_payouts(field, &payouts)?;
        if starting_table
            .iter()
            .chain(&outside_field)
            .any(|stack| *stack == MwChips::ZERO)
        {
            return Err(IcmError::ZeroStack);
        }
        let quadrature = (field > EXACT_ICM_MAX_PLAYERS).then(|| {
            Quadrature::new(
                starting_table.len(),
                &outside_field,
                &payouts,
                total_chips(starting_table.as_slice(), &outside_field),
            )
        });
        let mut prepared = Self {
            baseline: SeatVec::new_unchecked(vec![0.0; starting_table.len()]),
            starting_table,
            outside_field,
            payouts,
            quadrature,
        };
        prepared.baseline = prepared.terminal_values(&prepared.starting_table)?;
        Ok(prepared)
    }

    pub fn mode(&self) -> IcmMode {
        match &self.quadrature {
            None => IcmMode::Exact,
            Some(quadrature) => IcmMode::Quadrature {
                nodes: quadrature.times.len(),
            },
        }
    }

    /// Table equities at the starting stacks.
    pub fn baseline(&self) -> &SeatVec<f64> {
        &self.baseline
    }

    /// Table equities after a hand. Players busted in this hand finish below
    /// every survivor, ordered by their starting stacks; equal starting
    /// stacks split the affected prizes. Off-table players keep their stacks.
    pub fn terminal_values(
        &self,
        final_table: &SeatVec<MwChips>,
    ) -> Result<SeatVec<f64>, IcmError> {
        if self.starting_table.len() != final_table.len() {
            return Err(IcmError::SeatCount);
        }
        let (mut values, bottom) = busted_prizes(&self.starting_table, final_table, &self.payouts);
        let survivors: Vec<SeatId> = final_table
            .seats()
            .filter(|&seat| final_table[seat] > MwChips::ZERO)
            .collect();
        match &self.quadrature {
            None => {
                let mut stacks: Vec<MwChips> =
                    survivors.iter().map(|&seat| final_table[seat]).collect();
                stacks.extend_from_slice(&self.outside_field);
                let equities = exact_icm(&stacks, &self.payouts[..bottom]);
                for (&seat, equity) in survivors.iter().zip(equities) {
                    values[seat.index()] = equity;
                }
            }
            Some(quadrature) => quadrature.table_values(final_table, &survivors, &mut values),
        }
        Ok(SeatVec::new_unchecked(values))
    }

    /// Change of each table player's equity from the starting stacks.
    pub fn terminal_delta(&self, final_table: &SeatVec<MwChips>) -> Result<SeatVec<f64>, IcmError> {
        let terminal = self.terminal_values(final_table)?;
        Ok(SeatVec::new_unchecked(
            terminal
                .iter()
                .zip(self.baseline.iter())
                .map(|(terminal, baseline)| terminal - baseline)
                .collect(),
        ))
    }
}

fn total_chips(table: &[MwChips], outside: &[MwChips]) -> u128 {
    table
        .iter()
        .chain(outside)
        .map(|stack| u128::from(stack.raw()))
        .sum()
}

/// Trapezoidal rule on `x ∈ x_L + hℕ` after `t = exp(x − e^{−x})`.
#[derive(Debug, Clone, Copy)]
struct Grid {
    step: f64,
    left: f64,
    nodes: usize,
}

impl Grid {
    fn new(field_players: usize, total_chips: u128) -> Self {
        let n = field_players as f64;
        // Largest power-of-two step whose predicted aliasing error
        // (2/n) exp(−κ / ((n − 1) h²)) stays below the rounding floor.
        let predicted = (STEP_KAPPA / ((n - 1.0) * (2.0 / (n * STEP_TARGET)).ln())).sqrt();
        let step = predicted.min(MAX_STEP).log2().floor().exp2();
        // Omitting [0, t_L) changes a first-place probability by at most
        // (n / c) t_L, with t ≈ exp(−e^{−x}) on the left branch.
        let left = -(n / (HEADROOM * WINDOW_TARGET)).ln().ln();
        // A positive stack holds at least one chip, so every clock rate is at
        // least 1 / (c S); e^{−t_R / (c S)} ≤ ε bounds the right tail.
        let right_time = HEADROOM * total_chips as f64 * (1.0 / WINDOW_TARGET).ln();
        let mut right = right_time.ln();
        for _ in 0..4 {
            right = right_time.ln() + (-right).exp();
        }
        Self {
            step,
            left,
            nodes: ((right - left) / step).ceil() as usize,
        }
    }

    /// Node time `t` and trapezoidal weight `h φ'(x)`.
    fn node(&self, index: usize) -> (f64, f64) {
        let x = self.left + index as f64 * self.step;
        let decay = (-x).exp();
        let time = (x - decay).exp();
        (time, self.step * (1.0 + decay) * time)
    }
}

struct Quadrature {
    /// Clock rate of one chip, `1 / (c S)`.
    rate_per_chip: f64,
    times: Vec<f64>,
    weights: Vec<f64>,
    table: usize,
    /// `tails[node * table + k] = Σ_j π_{j+k+1} C_j^{out}(t_node)`: the
    /// expected prize when `k` table players and `j` off-table players
    /// finish ahead.
    tails: Vec<f64>,
}

impl Quadrature {
    fn new(table: usize, outside: &[MwChips], payouts: &[f64], total_chips: u128) -> Self {
        let grid = Grid::new(table + outside.len(), total_chips);
        let rate_per_chip = 1.0 / (HEADROOM * total_chips as f64);
        let paid = payouts
            .iter()
            .rposition(|&payout| payout != 0.0)
            .map_or(0, |place| place + 1);
        // Coefficients with j ≥ paid only meet zero prizes.
        let kept = (outside.len() + 1).min(paid);
        let outside_rates: Vec<f64> = outside
            .iter()
            .map(|stack| stack.raw() as f64 * rate_per_chip)
            .collect();
        let mut times = Vec::with_capacity(grid.nodes);
        let mut weights = Vec::with_capacity(grid.nodes);
        let mut tails = vec![0.0; grid.nodes * table];
        let mut coefficients = vec![0.0; kept];
        for (node, node_tails) in tails.chunks_exact_mut(table).enumerate() {
            let (time, weight) = grid.node(node);
            times.push(time);
            weights.push(weight);
            // Poisson-binomial distribution of off-table players ahead,
            // truncated to the paid places. Only the band [low, high] holds
            // coefficients of at least `TAIL_CUTOFF`; trimmed coefficients
            // and the mass they would carry total at most
            // n · paid · TAIL_CUTOFF per node, far below the rounding floor.
            coefficients.fill(0.0);
            coefficients[0] = 1.0;
            let (mut low, mut high) = (0, 0);
            for &rate in &outside_rates {
                let behind = (-rate * time).exp();
                let ahead = -(-rate * time).exp_m1();
                let top = (high + 1).min(kept - 1);
                for index in (low.max(1)..=top).rev() {
                    coefficients[index] =
                        coefficients[index] * behind + coefficients[index - 1] * ahead;
                }
                if low == 0 {
                    coefficients[0] *= behind;
                }
                high = top;
                while high > low && coefficients[high] < TAIL_CUTOFF {
                    coefficients[high] = 0.0;
                    high -= 1;
                }
                while low < high && coefficients[low] < TAIL_CUTOFF {
                    coefficients[low] = 0.0;
                    low += 1;
                }
            }
            for (k, tail) in node_tails.iter_mut().enumerate() {
                *tail = coefficients[low..=high]
                    .iter()
                    .zip(&payouts[low + k..])
                    .map(|(coefficient, payout)| coefficient * payout)
                    .sum();
            }
        }
        Self {
            rate_per_chip,
            times,
            weights,
            table,
            tails,
        }
    }

    /// Equities of the surviving table players. Busted players occupy the
    /// bottom places, which the off-table and surviving players never reach.
    fn table_values(&self, stacks: &SeatVec<MwChips>, survivors: &[SeatId], values: &mut [f64]) {
        let count = survivors.len();
        let mut rates = [0.0; MAX_SEATS];
        for (rate, &seat) in rates.iter_mut().zip(survivors) {
            *rate = stacks[seat].raw() as f64 * self.rate_per_chip;
        }
        let mut behind = [0.0; MAX_SEATS];
        let mut ahead = [0.0; MAX_SEATS];
        let mut others = [0.0; MAX_SEATS];
        let mut sums = [0.0; MAX_SEATS];
        for (node, (&time, &weight)) in self.times.iter().zip(&self.weights).enumerate() {
            for player in 0..count {
                behind[player] = (-rates[player] * time).exp();
                ahead[player] = -(-rates[player] * time).exp_m1();
            }
            let tails = &self.tails[node * self.table..][..count];
            for player in 0..count {
                if behind[player] == 0.0 {
                    continue;
                }
                // Distribution of the other survivors finishing ahead.
                others[0] = 1.0;
                let mut degree = 0;
                for other in (0..count).filter(|&other| other != player) {
                    others[degree + 1] = others[degree] * ahead[other];
                    for index in (1..=degree).rev() {
                        others[index] =
                            others[index] * behind[other] + others[index - 1] * ahead[other];
                    }
                    others[0] *= behind[other];
                    degree += 1;
                }
                let prize: f64 = others[..=degree]
                    .iter()
                    .zip(tails)
                    .map(|(probability, tail)| probability * tail)
                    .sum();
                sums[player] += weight * behind[player] * prize;
            }
        }
        for (player, &seat) in survivors.iter().enumerate() {
            values[seat.index()] = rates[player] * sums[player];
        }
    }
}

/// Exact equities by dynamic programming over the set of finished players.
fn exact_icm(stacks: &[MwChips], payouts: &[f64]) -> Vec<f64> {
    let n = stacks.len();
    let states = 1usize << n;
    let total: u128 = stacks.iter().map(|stack| stack.raw() as u128).sum();
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
        for player in 0..n {
            if mask & (1usize << player) != 0 {
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

/// Prizes of players busted in this hand and the number of places left
/// above them.
fn busted_prizes(
    starting_table: &SeatVec<MwChips>,
    final_table: &SeatVec<MwChips>,
    payouts: &[f64],
) -> (Vec<f64>, usize) {
    let mut values = vec![0.0; starting_table.len()];
    let mut busted: Vec<SeatId> = starting_table
        .seats()
        .filter(|&seat| final_table[seat] == MwChips::ZERO)
        .collect();
    busted.sort_unstable_by_key(|&seat| (starting_table[seat], seat));
    let mut bottom = payouts.len();
    let mut index = 0;
    while index < busted.len() {
        let starting_stack = starting_table[busted[index]];
        let mut end = index + 1;
        while end < busted.len() && starting_table[busted[end]] == starting_stack {
            end += 1;
        }
        let group = end - index;
        let first_prize = bottom - group;
        let split = payouts[first_prize..bottom].iter().sum::<f64>() / group as f64;
        for &seat in &busted[index..end] {
            values[seat.index()] = split;
        }
        bottom = first_prize;
        index = end;
    }
    (values, bottom)
}

fn validate_payouts(players: usize, payouts: &[f64]) -> Result<(), IcmError> {
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

#[derive(Debug, Error)]
pub enum IcmError {
    #[error("ICM field must contain 2 through 10000 players, got {0}")]
    FieldSize(usize),
    #[error("ICM payouts length must be {expected}, got {actual}")]
    PayoutCount { expected: usize, actual: usize },
    #[error("payout {0} must be finite and non-negative")]
    InvalidPayout(usize),
    #[error("payouts must be ordered highest to lowest")]
    PayoutOrder,
    #[error("ICM starting stacks must be positive")]
    ZeroStack,
    #[error("table stack vectors have different seat counts")]
    SeatCount,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats(stacks: &[u64]) -> SeatVec<MwChips> {
        SeatVec::try_new(stacks.iter().copied().map(MwChips).collect()).unwrap()
    }

    fn chips(stacks: &[u64]) -> Vec<MwChips> {
        stacks.iter().copied().map(MwChips).collect()
    }

    fn assert_relative(actual: &[f64], expected: &[f64], tolerance: f64) {
        for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
            let error = (actual - expected).abs() / expected.abs().max(f64::MIN_POSITIVE);
            assert!(
                error <= tolerance,
                "player {index}: {actual} vs {expected}, relative error {error:e}"
            );
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
            let total: u64 = remaining.iter().map(|&player| stacks[player].raw()).sum();
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
            let prepared = PreparedIcm::new(
                SeatVec::try_new(stacks[..2].to_vec()).unwrap(),
                stacks[2..].to_vec(),
                payouts,
            )
            .unwrap();
            assert_eq!(prepared.mode(), IcmMode::Exact);
            assert_relative(prepared.baseline().as_slice(), &expected[..2], 1e-12);
        }
    }

    #[test]
    fn mode_switches_to_quadrature_above_fifteen_players() {
        let payouts = |players: usize| (0..players).rev().map(|place| place as f64).collect();
        let exact = PreparedIcm::new(seats(&[100; 9]), chips(&[100; 6]), payouts(15)).unwrap();
        assert_eq!(exact.mode(), IcmMode::Exact);
        let quadrature = PreparedIcm::new(seats(&[100; 9]), chips(&[100; 7]), payouts(16)).unwrap();
        assert!(matches!(quadrature.mode(), IcmMode::Quadrature { .. }));
        assert_eq!(quadrature.mode(), icm_mode(16, 1_600));
    }

    /// Quadrature against the exact program on fields small enough for both,
    /// including busts, uneven stacks and partially paid places.
    #[test]
    fn quadrature_matches_exact_dynamic_programming() {
        let starting = seats(&[2_500, 400, 9_000, 1_234, 50, 7_777]);
        let outside = chips(&[
            3_000, 3_000, 120, 15_000, 800, 2_222, 999, 4_321, 60, 10_000, 5,
        ]);
        let mut payouts = vec![0.0; 17];
        payouts[..8].copy_from_slice(&[400.0, 250.0, 160.0, 100.0, 70.0, 50.0, 40.0, 30.0]);
        let prepared =
            PreparedIcm::new(starting.clone(), outside.clone(), payouts.clone()).unwrap();
        assert!(matches!(prepared.mode(), IcmMode::Quadrature { .. }));

        let mut field = starting.as_slice().to_vec();
        field.extend_from_slice(&outside);
        assert_relative(
            prepared.baseline().as_slice(),
            &exact_icm(&field, &payouts)[..starting.len()],
            1e-12,
        );

        // Seats 1 and 4 bust; seat 1 started larger, so it finishes 16th and
        // seat 4 17th. Survivors split the first 15 places with the field.
        let final_table = seats(&[2_000, 0, 10_000, 1_234, 0, 8_177]);
        let terminal = prepared.terminal_values(&final_table).unwrap();
        let mut survivors = chips(&[2_000, 10_000, 1_234, 8_177]);
        survivors.extend_from_slice(&outside);
        let exact = exact_icm(&survivors, &payouts[..15]);
        assert_relative(
            &[
                terminal[SeatId(0)],
                terminal[SeatId(2)],
                terminal[SeatId(3)],
                terminal[SeatId(5)],
            ],
            &exact[..4],
            1e-12,
        );
        assert_eq!(terminal[SeatId(1)], payouts[15]);
        assert_eq!(terminal[SeatId(4)], payouts[16]);
    }

    /// Winner-take-all equity is the chip share in any field size.
    #[test]
    fn quadrature_winner_take_all_is_chip_share_in_large_field() {
        let starting = seats(&[
            1, 40_000, 777_777, 25_000, 25_000, 3, 1_000_000, 12_345, 5_000,
        ]);
        let outside: Vec<_> = (0..4_000u64)
            .map(|i| MwChips(1 + (i * i * 7_919) % 90_000))
            .collect();
        let mut payouts = vec![0.0; starting.len() + outside.len()];
        payouts[0] = 1.0;
        let prepared = PreparedIcm::new(starting.clone(), outside.clone(), payouts).unwrap();
        let total = total_chips(starting.as_slice(), &outside) as f64;
        let expected: Vec<f64> = starting.iter().map(|s| s.raw() as f64 / total).collect();
        assert_relative(prepared.baseline().as_slice(), &expected, 1e-13);
    }

    /// Equal stacks give every player the average prize.
    #[test]
    fn quadrature_equal_stacks_receive_average_prize() {
        let players = 300;
        let payouts: Vec<f64> = (0..players)
            .map(|place| 1.0 / (place as f64 + 1.0))
            .collect();
        let average = payouts.iter().sum::<f64>() / players as f64;
        let prepared = PreparedIcm::new(seats(&[2_000; 9]), chips(&[2_000; 291]), payouts).unwrap();
        assert_relative(prepared.baseline().as_slice(), &[average; 9], 1e-13);
    }

    #[test]
    fn unchanged_stacks_have_exactly_zero_delta() {
        let starting = seats(&[800, 1_200]);
        let mut payouts = vec![0.0; 40];
        payouts[..3].copy_from_slice(&[100.0, 60.0, 40.0]);
        let prepared = PreparedIcm::new(starting.clone(), chips(&[1_000; 38]), payouts).unwrap();
        assert_eq!(
            prepared.terminal_delta(&starting).unwrap().as_slice(),
            &[0.0, 0.0]
        );
    }

    #[test]
    fn simultaneous_busts_use_starting_stack_and_split_ties() {
        let starting = seats(&[10, 10, 50]);
        let prepared =
            PreparedIcm::new(starting, chips(&[30]), vec![100.0, 50.0, 20.0, 0.0]).unwrap();
        let terminal = prepared.terminal_values(&seats(&[0, 0, 70])).unwrap();
        assert_eq!(terminal[SeatId(0)], 10.0);
        assert_eq!(terminal[SeatId(1)], 10.0);
        assert!(terminal[SeatId(2)] > 50.0);
    }

    #[test]
    fn lone_survivor_takes_first_prize() {
        let prepared = PreparedIcm::new(seats(&[10, 20]), Vec::new(), vec![100.0, 0.0]).unwrap();
        let terminal = prepared.terminal_values(&seats(&[0, 30])).unwrap();
        assert_eq!(terminal.as_slice(), &[0.0, 100.0]);
    }

    #[test]
    fn invalid_fields_are_rejected() {
        assert!(matches!(
            PreparedIcm::new(seats(&[1, 1]), chips(&[1; ICM_MAX_PLAYERS - 1]), vec![0.0; ICM_MAX_PLAYERS + 1]),
            Err(IcmError::FieldSize(size)) if size == ICM_MAX_PLAYERS + 1
        ));
        assert!(matches!(
            PreparedIcm::new(seats(&[0, 1]), Vec::new(), vec![1.0, 0.0]),
            Err(IcmError::ZeroStack)
        ));
        assert!(matches!(
            PreparedIcm::new(seats(&[1, 1]), Vec::new(), vec![0.0, 1.0]),
            Err(IcmError::PayoutOrder)
        ));
    }
}
