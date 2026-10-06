//! Physical-deal evaluation with exact action expectations and common random numbers.
use anyhow::{Result, ensure};
use nlh::rank_of;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use serde::Serialize;

use super::{Profile, Tree};
use crate::trunk::classes::{Ordering3, class};
use crate::{ExternalSamplingGame, FeatureHashAbstraction, HoldemGame, SampledWorld, SeatId};

const MAX_SEATS: usize = 9;
const CHUNK_DEALS: u64 = 4096;

#[derive(Debug, Clone, Copy)]
pub struct RealOptions {
    pub deals: u64,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Estimate {
    pub mean: f64,
    pub stderr: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RealSeat {
    pub seat: usize,
    pub name: String,
    pub value: Estimate,
    pub value_by_active_count: Vec<Estimate>,
    pub l0_best_response_value: Estimate,
    pub l0_best_response_gain: Estimate,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RealEvaluation {
    pub seats: Vec<RealSeat>,
    pub value_sum: Estimate,
    /// Per-deal sum over seats of the L0 best responses' gains.
    pub l0_best_response_gain_sum: Estimate,
    pub deals: u64,
    pub seed: u64,
    pub mean_deal_attempts: f64,
}

/// Precomputed integer awards for each possible tied winner subset of a pot.
/// This is a compact ordering table: an ordering selects the strongest eligible
/// subset independently in every side pot, including button-relative odd chips.
struct PotAwards {
    eligible: usize,
    shares: Vec<[u64; MAX_SEATS]>,
}

struct Showdown {
    floor: [u64; MAX_SEATS],
    starting: [u64; MAX_SEATS],
    pots: Vec<PotAwards>,
}

pub(super) struct Prepared {
    ends: Vec<usize>,
    showdowns: Vec<Option<Showdown>>,
}

impl Prepared {
    pub(super) fn new(tree: &Tree, game: &HoldemGame<FeatureHashAbstraction>) -> Result<Self> {
        let mut ends: Vec<_> = (1..=tree.nodes.len()).collect();
        for z in (0..tree.nodes.len()).rev() {
            if let Some(&last) = tree.nodes[z].children.last() {
                ends[z] = ends[last];
            }
        }
        let mut showdowns = Vec::with_capacity(tree.nodes.len());
        for node in &tree.nodes {
            let Some(t) = node.terminal.as_ref().filter(|t| t.active.len() >= 4) else {
                showdowns.push(None);
                continue;
            };
            let shape = game.l0_rated_pots(&t.state)?;
            let mut floor = [0; MAX_SEATS];
            let mut starting = [0; MAX_SEATS];
            for i in 0..tree.seats {
                let seat = SeatId(i as u8);
                floor[i] = t.state.seats[seat].remaining.raw() + shape.refunds[seat].raw();
                starting[i] = game.config().seats[seat].starting_stack.raw();
            }
            let pots = shape
                .pots
                .iter()
                .map(|pot| {
                    let eligible = pot
                        .eligible
                        .iter()
                        .fold(0, |mask, seat| mask | (1 << seat.index()));
                    let mut shares = vec![[0; MAX_SEATS]; 1 << tree.seats];
                    for (winners, awards) in shares.iter_mut().enumerate().skip(1) {
                        if winners & !eligible != 0 {
                            continue;
                        }
                        let count = winners.count_ones() as u64;
                        let share = pot.net.raw() / count;
                        let mut odd = pot.net.raw() % count;
                        for step in 1..=tree.seats {
                            let i = (t.state.button.index() + step) % tree.seats;
                            if winners & (1 << i) != 0 {
                                awards[i] = share + u64::from(odd > 0);
                                odd = odd.saturating_sub(1);
                            }
                        }
                    }
                    PotAwards { eligible, shares }
                })
                .collect();
            showdowns.push(Some(Showdown {
                floor,
                starting,
                pots,
            }));
        }
        Ok(Self { ends, showdowns })
    }
}

pub(super) struct Scratch {
    reach: Vec<[f64; MAX_SEATS + 1]>,
    pub(super) values: [f64; MAX_SEATS],
    pub(super) by_count: [[f64; MAX_SEATS + 1]; MAX_SEATS],
    pub(super) best: [f64; MAX_SEATS],
}

impl Scratch {
    pub(super) fn new(tree: &Tree) -> Self {
        Self {
            reach: vec![[0.0; MAX_SEATS + 1]; tree.nodes.len()],
            values: [0.0; MAX_SEATS],
            by_count: [[0.0; MAX_SEATS + 1]; MAX_SEATS],
            best: [0.0; MAX_SEATS],
        }
    }

    /// All reaches are joint path probabilities: slot zero is the profile,
    /// slot i+1 replaces just seat i's factors with its pure response.
    pub(super) fn deal(
        &mut self,
        tree: &Tree,
        profile: &Profile,
        responses: &[Vec<u8>],
        prepared: &Prepared,
        world: &SampledWorld,
    ) {
        self.values.fill(0.0);
        self.by_count.fill([0.0; MAX_SEATS + 1]);
        self.best.fill(0.0);
        self.reach[0].fill(1.0);
        let mut classes = [0; MAX_SEATS];
        let mut ranks = [0; MAX_SEATS];
        for i in 0..tree.seats {
            classes[i] = class(world.hole_combo(i));
            let (a, b) = world.hole_cards(i);
            ranks[i] = rank_of(world.runout().iter().copied().chain([a, b])).0;
        }
        // Strongest tied subset for every eligible mask. A low-bit recurrence
        // avoids comparing the same ranks anew at thousands of terminals.
        let mut winners = [0_usize; 1 << MAX_SEATS];
        let mut strongest = [0; 1 << MAX_SEATS];
        for mask in 1_usize..1 << tree.seats {
            let i = mask.trailing_zeros() as usize;
            let bit = 1 << i;
            let rest = mask ^ bit;
            strongest[mask] = ranks[i].max(strongest[rest]);
            winners[mask] = match ranks[i].cmp(&strongest[rest]) {
                std::cmp::Ordering::Greater => bit,
                std::cmp::Ordering::Equal => bit | winners[rest],
                std::cmp::Ordering::Less => winners[rest],
            };
        }
        let mut z = 0;
        while z < tree.nodes.len() {
            let reach = self.reach[z];
            if reach[..=tree.seats].iter().all(|&p| p == 0.0) {
                z = prepared.ends[z];
                continue;
            }
            let node = &tree.nodes[z];
            if let Some(t) = &node.terminal {
                let k = t.active.len();
                let mut utility = [0.0; MAX_SEATS];
                let payoff = match k {
                    1 => &t.payoffs[0],
                    2 => {
                        let slot = match ranks[t.active[0]].cmp(&ranks[t.active[1]]) {
                            std::cmp::Ordering::Greater => 0,
                            std::cmp::Ordering::Equal => 1,
                            std::cmp::Ordering::Less => 2,
                        };
                        &t.payoffs[slot]
                    }
                    3 => {
                        &t.payoffs[Ordering3::from_ranks(
                            ranks[t.active[0]],
                            ranks[t.active[1]],
                            ranks[t.active[2]],
                        )
                        .index()]
                    }
                    _ => {
                        let showdown = prepared.showdowns[z].as_ref().unwrap();
                        let mut chips = showdown.floor;
                        for pot in &showdown.pots {
                            let awards = &pot.shares[winners[pot.eligible]];
                            for i in 0..tree.seats {
                                chips[i] += awards[i];
                            }
                        }
                        for i in 0..tree.seats {
                            utility[i] = (chips[i] as f64 - showdown.starting[i] as f64)
                                / crate::types::CHIPS_PER_BB as f64;
                        }
                        &utility[..tree.seats]
                    }
                };
                for i in 0..tree.seats {
                    let value = reach[0] * payoff[i];
                    self.values[i] += value;
                    self.by_count[i][k] += value;
                    self.best[i] += reach[i + 1] * payoff[i];
                }
            } else {
                let actor = node.actor.unwrap();
                let c = classes[actor];
                let row = profile.row(tree, z, c);
                let action = usize::from(responses[actor][z * 169 + c]);
                for (a, &child) in node.children.iter().enumerate() {
                    let mut next = [0.0; MAX_SEATS + 1];
                    for i in 0..=tree.seats {
                        next[i] = reach[i]
                            * if i == actor + 1 {
                                f64::from(a == action)
                            } else {
                                row[a]
                            };
                    }
                    self.reach[child] = next;
                }
            }
            z += 1;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Moments {
    mean: f64,
    m2: f64,
}

impl Moments {
    fn add(&mut self, x: f64, n: u64) {
        let delta = x - self.mean;
        self.mean += delta / n as f64;
        self.m2 += delta * (x - self.mean);
    }

    fn merge(&mut self, other: Self, n: u64, count: u64) {
        let delta = other.mean - self.mean;
        let total = (n + count) as f64;
        self.mean += delta * (count as f64 / total);
        self.m2 += other.m2 + delta * delta * (n as f64 * count as f64 / total);
    }

    fn estimate(self, n: u64) -> Estimate {
        Estimate {
            mean: self.mean,
            stderr: (self.m2.max(0.0) / (n - 1) as f64 / n as f64).sqrt(),
        }
    }
}

struct Accumulator {
    n: u64,
    attempts: u64,
    values: [Moments; MAX_SEATS],
    by_count: [[Moments; MAX_SEATS + 1]; MAX_SEATS],
    best: [Moments; MAX_SEATS],
    gains: [Moments; MAX_SEATS],
    sum: Moments,
    gain_sum: Moments,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            n: 0,
            attempts: 0,
            values: [Moments::default(); MAX_SEATS],
            by_count: [[Moments::default(); MAX_SEATS + 1]; MAX_SEATS],
            best: [Moments::default(); MAX_SEATS],
            gains: [Moments::default(); MAX_SEATS],
            sum: Moments::default(),
            gain_sum: Moments::default(),
        }
    }

    fn add(&mut self, scratch: &Scratch, seats: usize, attempts: u32) {
        self.n += 1;
        self.attempts += u64::from(attempts);
        for i in 0..seats {
            self.values[i].add(scratch.values[i], self.n);
            self.best[i].add(scratch.best[i], self.n);
            self.gains[i].add(scratch.best[i] - scratch.values[i], self.n);
            for k in 1..=seats {
                self.by_count[i][k].add(scratch.by_count[i][k], self.n);
            }
        }
        self.sum.add(scratch.values[..seats].iter().sum(), self.n);
        self.gain_sum.add(
            (0..seats)
                .map(|i| scratch.best[i] - scratch.values[i])
                .sum(),
            self.n,
        );
    }

    fn merge(&mut self, other: Self, seats: usize) {
        for i in 0..seats {
            self.values[i].merge(other.values[i], self.n, other.n);
            self.best[i].merge(other.best[i], self.n, other.n);
            self.gains[i].merge(other.gains[i], self.n, other.n);
            for k in 1..=seats {
                self.by_count[i][k].merge(other.by_count[i][k], self.n, other.n);
            }
        }
        self.sum.merge(other.sum, self.n, other.n);
        self.gain_sum.merge(other.gain_sum, self.n, other.n);
        self.n += other.n;
        self.attempts += other.attempts;
    }
}

/// Sample only physical cards; integrate every profile and unilateral response
/// action path exactly. Fixed consecutive chunks and ordered moment merges make
/// every reported bit independent of Rayon scheduling and thread count.
pub fn evaluate_real(
    tree: &Tree,
    profile: &Profile,
    best_responses: &[Vec<u8>],
    game: &HoldemGame<FeatureHashAbstraction>,
    options: RealOptions,
) -> Result<RealEvaluation> {
    profile.validate(tree)?;
    game.require_l0_chip_ev()?;
    ensure!(
        tree.game_fingerprint == game.game_fingerprint(),
        "game/tree mismatch"
    );
    ensure!(
        options.deals >= 2,
        "real evaluation needs at least two deals"
    );
    ensure!(
        best_responses.len() == tree.seats,
        "expected one BR vector per seat"
    );
    for (i, response) in best_responses.iter().enumerate() {
        ensure!(
            response.len() == tree.nodes.len() * 169,
            "BR action length mismatch for seat {i}"
        );
        for (z, node) in tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.actor == Some(i))
        {
            ensure!(
                node.children.len() <= 254,
                "L0 nodes may have at most 254 actions"
            );
            ensure!(
                response[z * 169..(z + 1) * 169]
                    .iter()
                    .all(|&a| usize::from(a) < node.children.len()),
                "invalid BR action at node {z}"
            );
        }
    }
    let sampler = game.deal_sampler()?;
    let prepared = Prepared::new(tree, game)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.p2.trunk.l0.real.v1");
    hasher.update(&options.seed.to_le_bytes());
    let key = *hasher.finalize().as_bytes();
    let chunks = options.deals.div_ceil(CHUNK_DEALS);
    let results: Vec<_> = (0..chunks)
        .into_par_iter()
        .map_init(
            || Scratch::new(tree),
            |scratch, chunk| -> Result<Accumulator> {
                let mut result = Accumulator::new();
                let start = chunk * CHUNK_DEALS;
                for d in start..start + CHUNK_DEALS.min(options.deals - start) {
                    let mut rng = ChaCha8Rng::from_seed(key);
                    rng.set_stream(d);
                    let sample = sampler.sample_counted(&mut rng)?;
                    scratch.deal(tree, profile, best_responses, &prepared, &sample.world);
                    result.add(scratch, tree.seats, sample.attempts);
                }
                Ok(result)
            },
        )
        .collect::<Result<_>>()?;
    let mut total = Accumulator::new();
    for result in results {
        total.merge(result, tree.seats);
    }
    let seats = (0..tree.seats)
        .map(|i| RealSeat {
            seat: i,
            name: game.config().seats[SeatId(i as u8)]
                .name
                .clone()
                .unwrap_or_else(|| format!("seat {i}")),
            value: total.values[i].estimate(total.n),
            value_by_active_count: total.by_count[i][..=tree.seats]
                .iter()
                .map(|m| m.estimate(total.n))
                .collect(),
            l0_best_response_value: total.best[i].estimate(total.n),
            l0_best_response_gain: total.gains[i].estimate(total.n),
        })
        .collect();
    Ok(RealEvaluation {
        seats,
        value_sum: total.sum.estimate(total.n),
        l0_best_response_gain_sum: total.gain_sum.estimate(total.n),
        deals: options.deals,
        seed: options.seed,
        mean_deal_attempts: total.attempts as f64 / total.n as f64,
    })
}
