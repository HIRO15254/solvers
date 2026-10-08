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
const FIT_LANES: usize = 16;

/// Play the profile's mixed row at this decision cell.
pub const FOLLOW: u8 = 254;

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
pub struct ResponseEstimate {
    pub value: Estimate,
    pub gain: Estimate,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RealSeat {
    pub seat: usize,
    pub name: String,
    pub value: Estimate,
    pub value_by_active_count: Vec<Estimate>,
    pub responses: Vec<ResponseEstimate>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RealEvaluation {
    pub seats: Vec<RealSeat>,
    pub value_sum: Estimate,
    /// Per-set, per-deal sum over seats of unilateral response gains.
    pub response_gain_sums: Vec<Estimate>,
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

/// Cards and ranks shared by evaluation and fitting, without heap allocation.
struct Deal {
    classes: [usize; MAX_SEATS],
    ranks: [u16; MAX_SEATS],
    winners: [usize; 1 << MAX_SEATS],
}

impl Deal {
    fn new(tree: &Tree, world: &SampledWorld) -> Self {
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
        Self {
            classes,
            ranks,
            winners,
        }
    }

    fn payoff<'a>(
        &self,
        tree: &'a Tree,
        prepared: &Prepared,
        z: usize,
        utility: &'a mut [f64; MAX_SEATS],
    ) -> &'a [f64] {
        let t = tree.nodes[z].terminal.as_ref().unwrap();
        let ranks = &self.ranks;
        let winners = &self.winners;
        let k = t.active.len();
        match k {
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
        }
    }
}

pub(super) struct Scratch {
    reach: Vec<f64>,
    slots: usize,
    pub(super) values: [f64; MAX_SEATS],
    pub(super) by_count: [[f64; MAX_SEATS + 1]; MAX_SEATS],
    pub(super) best: Vec<[f64; MAX_SEATS]>,
}

impl Scratch {
    pub(super) fn new(tree: &Tree, sets: usize) -> Self {
        let slots = 1 + tree.seats * sets;
        Self {
            reach: vec![0.0; slots * tree.nodes.len()],
            slots,
            values: [0.0; MAX_SEATS],
            by_count: [[0.0; MAX_SEATS + 1]; MAX_SEATS],
            best: vec![[0.0; MAX_SEATS]; sets],
        }
    }

    /// All reaches are joint path probabilities: slot zero is the profile,
    /// slot 1+s*seats+i replaces just seat i's factors with response set s.
    pub(super) fn deal(
        &mut self,
        tree: &Tree,
        profile: &Profile,
        responses: &[Vec<Vec<u8>>],
        prepared: &Prepared,
        world: &SampledWorld,
    ) {
        self.values.fill(0.0);
        self.by_count.fill([0.0; MAX_SEATS + 1]);
        self.best.fill([0.0; MAX_SEATS]);
        self.reach[..self.slots].fill(1.0);
        let deal = Deal::new(tree, world);
        let classes = &deal.classes;
        let mut z = 0;
        while z < tree.nodes.len() {
            let reach = &self.reach[z * self.slots..(z + 1) * self.slots];
            if reach.iter().all(|&p| p == 0.0) {
                z = prepared.ends[z];
                continue;
            }
            let node = &tree.nodes[z];
            if let Some(t) = &node.terminal {
                let k = t.active.len();
                let mut utility = [0.0; MAX_SEATS];
                let payoff = deal.payoff(tree, prepared, z, &mut utility);
                for i in 0..tree.seats {
                    let value = reach[0] * payoff[i];
                    self.values[i] += value;
                    self.by_count[i][k] += value;
                    for (s, best) in self.best.iter_mut().enumerate() {
                        best[i] += reach[1 + s * tree.seats + i] * payoff[i];
                    }
                }
            } else {
                let actor = node.actor.unwrap();
                let c = classes[actor];
                let row = profile.row(tree, z, c);
                // Copy the parent slots to stack storage before writing children.
                let mut parent = [0.0; 1 + MAX_SEATS * 6];
                parent[..self.slots].copy_from_slice(reach);
                for (a, &child) in node.children.iter().enumerate() {
                    let next = &mut self.reach[child * self.slots..(child + 1) * self.slots];
                    next[0] = parent[0] * row[a];
                    for (s, responses) in responses.iter().enumerate() {
                        let action = usize::from(responses[actor][z * 169 + c]);
                        for i in 0..tree.seats {
                            let slot = 1 + s * tree.seats + i;
                            next[slot] = parent[slot]
                                * if i == actor && action != usize::from(FOLLOW) {
                                    f64::from(a == action)
                                } else {
                                    row[a]
                                };
                        }
                    }
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
    best: Vec<[Moments; MAX_SEATS]>,
    gains: Vec<[Moments; MAX_SEATS]>,
    sum: Moments,
    gain_sums: Vec<Moments>,
}

impl Accumulator {
    fn new(sets: usize) -> Self {
        Self {
            n: 0,
            attempts: 0,
            values: [Moments::default(); MAX_SEATS],
            by_count: [[Moments::default(); MAX_SEATS + 1]; MAX_SEATS],
            best: vec![[Moments::default(); MAX_SEATS]; sets],
            gains: vec![[Moments::default(); MAX_SEATS]; sets],
            sum: Moments::default(),
            gain_sums: vec![Moments::default(); sets],
        }
    }

    fn add(&mut self, scratch: &Scratch, seats: usize, attempts: u32) {
        self.n += 1;
        self.attempts += u64::from(attempts);
        for i in 0..seats {
            self.values[i].add(scratch.values[i], self.n);
            for s in 0..self.best.len() {
                self.best[s][i].add(scratch.best[s][i], self.n);
                self.gains[s][i].add(scratch.best[s][i] - scratch.values[i], self.n);
            }
            for k in 1..=seats {
                self.by_count[i][k].add(scratch.by_count[i][k], self.n);
            }
        }
        self.sum.add(scratch.values[..seats].iter().sum(), self.n);
        for s in 0..self.best.len() {
            self.gain_sums[s].add(
                (0..seats)
                    .map(|i| scratch.best[s][i] - scratch.values[i])
                    .sum(),
                self.n,
            );
        }
    }

    fn merge(&mut self, other: Self, seats: usize) {
        for i in 0..seats {
            self.values[i].merge(other.values[i], self.n, other.n);
            for s in 0..self.best.len() {
                self.best[s][i].merge(other.best[s][i], self.n, other.n);
                self.gains[s][i].merge(other.gains[s][i], self.n, other.n);
            }
            for k in 1..=seats {
                self.by_count[i][k].merge(other.by_count[i][k], self.n, other.n);
            }
        }
        self.sum.merge(other.sum, self.n, other.n);
        for s in 0..self.best.len() {
            self.gain_sums[s].merge(other.gain_sums[s], self.n, other.n);
        }
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
    responses: &[Vec<Vec<u8>>],
    game: &HoldemGame<FeatureHashAbstraction>,
    options: RealOptions,
) -> Result<RealEvaluation> {
    validate(tree, profile, game, options)?;
    ensure!(
        responses.len() <= 6,
        "at most six response sets are supported"
    );
    for best_responses in responses {
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
                        .all(|&a| a == FOLLOW || usize::from(a) < node.children.len()),
                    "invalid BR action at node {z}"
                );
            }
        }
    }
    let sampler = game.deal_sampler()?;
    let prepared = Prepared::new(tree, game)?;
    let key = deal_key(options.seed);
    let chunks = options.deals.div_ceil(CHUNK_DEALS);
    let results: Vec<_> = (0..chunks)
        .into_par_iter()
        .map_init(
            || Scratch::new(tree, responses.len()),
            |scratch, chunk| -> Result<Accumulator> {
                let mut result = Accumulator::new(responses.len());
                let start = chunk * CHUNK_DEALS;
                for d in start..start + CHUNK_DEALS.min(options.deals - start) {
                    let mut rng = ChaCha8Rng::from_seed(key);
                    rng.set_stream(d);
                    let sample = sampler.sample_counted(&mut rng)?;
                    scratch.deal(tree, profile, responses, &prepared, &sample.world);
                    result.add(scratch, tree.seats, sample.attempts);
                }
                Ok(result)
            },
        )
        .collect::<Result<_>>()?;
    let mut total = Accumulator::new(responses.len());
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
            responses: (0..responses.len())
                .map(|s| ResponseEstimate {
                    value: total.best[s][i].estimate(total.n),
                    gain: total.gains[s][i].estimate(total.n),
                })
                .collect(),
        })
        .collect();
    Ok(RealEvaluation {
        seats,
        value_sum: total.sum.estimate(total.n),
        response_gain_sums: total
            .gain_sums
            .iter()
            .map(|m| m.estimate(total.n))
            .collect(),
        deals: options.deals,
        seed: options.seed,
        mean_deal_attempts: total.attempts as f64 / total.n as f64,
    })
}

fn validate(
    tree: &Tree,
    profile: &Profile,
    game: &HoldemGame<FeatureHashAbstraction>,
    options: RealOptions,
) -> Result<()> {
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
    Ok(())
}

fn deal_key(seed: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.p2.trunk.l0.real.v1");
    hasher.update(&seed.to_le_bytes());
    *hasher.finalize().as_bytes()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FittedSeat {
    pub seat: usize,
    pub name: String,
    pub value: f64,
    pub best_response: f64,
    pub gain: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GatedSeat {
    pub seat: usize,
    /// In-sample value of the gated response on the fitting deals.
    pub best_response: f64,
    /// Best response minus the seat's in-sample profile value.
    pub gain: f64,
    /// Own decision node/class cells where the response deviates from the profile.
    pub deviations: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GatedResponse {
    pub threshold: f64,
    pub seats: Vec<GatedSeat>,
    /// Per seat, node * 169 + class: action or FOLLOW; u8::MAX elsewhere.
    #[serde(skip)]
    pub actions: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FittedResponses {
    /// In-sample values on the fitting deals; maximization biases the BR upward.
    pub seats: Vec<FittedSeat>,
    /// Per-seat pure actions in node * 169 + class layout.
    #[serde(skip)]
    pub actions: Vec<Vec<u8>>,
    pub deals: u64,
    pub seed: u64,
    /// Significance-gated responses, in requested threshold order.
    pub gated: Vec<GatedResponse>,
}

struct FitScratch {
    reach: Vec<[f64; MAX_SEATS]>,
    weights: Vec<f64>,
}

impl FitScratch {
    fn new(tree: &Tree, terminals: usize) -> Self {
        Self {
            reach: vec![[0.0; MAX_SEATS]; tree.nodes.len()],
            weights: vec![0.0; tree.seats * 169 * terminals],
        }
    }

    fn deal(
        &mut self,
        tree: &Tree,
        profile: &Profile,
        prepared: &Prepared,
        terminal_indices: &[usize],
        terminals: usize,
        world: &SampledWorld,
    ) {
        let deal = Deal::new(tree, world);
        self.reach[0].fill(1.0);
        let offsets: [usize; MAX_SEATS] =
            std::array::from_fn(|i| (i * 169 + deal.classes[i]) * terminals);
        let mut z = 0;
        while z < tree.nodes.len() {
            let reach = self.reach[z];
            if reach[..tree.seats].iter().all(|&r| r == 0.0) {
                z = prepared.ends[z];
                continue;
            }
            let node = &tree.nodes[z];
            if node.terminal.is_some() {
                let mut utility = [0.0; MAX_SEATS];
                let payoff = deal.payoff(tree, prepared, z, &mut utility);
                for i in 0..tree.seats {
                    if reach[i] != 0.0 {
                        self.weights[offsets[i] + terminal_indices[z]] += reach[i] * payoff[i];
                    }
                }
            } else {
                let actor = node.actor.unwrap();
                let row = profile.row(tree, z, deal.classes[actor]);
                for (a, &child) in node.children.iter().enumerate() {
                    for (i, &r) in reach.iter().enumerate().take(tree.seats) {
                        self.reach[child][i] = r * if i == actor { 1.0 } else { row[a] };
                    }
                }
            }
            z += 1;
        }
    }
}

/// Fit class-level pure best responses on physical input-game deals. The fixed
/// sixteen lanes accumulate opponent-reach-weighted terminal payoffs in chunk
/// order, then merge in lane order, independently of Rayon scheduling.
/// Evaluate these fixed actions on an independent seed for a lower-bound value.
/// Lane standard errors are meaningful only when every lane holds deals
/// (at least 16 * 4096 deals).
pub fn fit_real_responses(
    tree: &Tree,
    profile: &Profile,
    game: &HoldemGame<FeatureHashAbstraction>,
    options: RealOptions,
    thresholds: &[f64],
) -> Result<FittedResponses> {
    validate(tree, profile, game, options)?;
    ensure!(
        thresholds.len() <= 4,
        "at most four fit thresholds are supported"
    );
    ensure!(
        thresholds.iter().all(|z| z.is_finite() && *z >= 0.0),
        "fit thresholds must be finite and nonnegative"
    );
    for node in tree.nodes.iter().filter(|n| n.actor.is_some()) {
        ensure!(
            node.children.len() <= 254,
            "L0 nodes may have at most 254 actions"
        );
    }
    let prepared = Prepared::new(tree, game)?;
    let mut terminals = 0;
    let terminal_indices: Vec<_> = tree
        .nodes
        .iter()
        .map(|n| {
            if n.terminal.is_some() {
                let index = terminals;
                terminals += 1;
                index
            } else {
                usize::MAX
            }
        })
        .collect();
    let sampler = game.deal_sampler()?;
    let key = deal_key(options.seed);
    let chunks = options.deals.div_ceil(CHUNK_DEALS);
    let lanes = (0..FIT_LANES)
        .into_par_iter()
        .map(|lane| -> Result<Vec<f64>> {
            let mut scratch = FitScratch::new(tree, terminals);
            for chunk in (lane as u64..chunks).step_by(FIT_LANES) {
                let start = chunk * CHUNK_DEALS;
                for d in start..start + CHUNK_DEALS.min(options.deals - start) {
                    let mut rng = ChaCha8Rng::from_seed(key);
                    rng.set_stream(d);
                    let sample = sampler.sample_counted(&mut rng)?;
                    scratch.deal(
                        tree,
                        profile,
                        &prepared,
                        &terminal_indices,
                        terminals,
                        &sample.world,
                    );
                }
            }
            Ok(scratch.weights)
        })
        .collect::<Result<Vec<_>>>()?;
    // Preserve all replicates; copy lane zero to keep the original merge order.
    let mut weights = lanes[0].clone();
    for lane in &lanes[1..] {
        for (w, x) in weights.iter_mut().zip(lane) {
            *w += x;
        }
    }
    let mut actions = vec![vec![u8::MAX; tree.nodes.len() * 169]; tree.seats];
    let mut own_reach = vec![0.0; tree.nodes.len()];
    let mut best = vec![0.0; tree.nodes.len()];
    let mut seats = Vec::with_capacity(tree.seats);
    for (i, actions) in actions.iter_mut().enumerate() {
        let mut value = 0.0;
        let mut best_response = 0.0;
        for c in 0..169 {
            let row = &weights[(i * 169 + c) * terminals..(i * 169 + c + 1) * terminals];
            own_reach[0] = 1.0;
            let mut class_value = 0.0;
            for (z, node) in tree.nodes.iter().enumerate() {
                if node.terminal.is_some() {
                    class_value += own_reach[z] * row[terminal_indices[z]];
                } else {
                    for (a, &child) in node.children.iter().enumerate() {
                        own_reach[child] = own_reach[z]
                            * if node.actor == Some(i) {
                                profile.row(tree, z, c)[a]
                            } else {
                                1.0
                            };
                    }
                }
            }
            value += class_value;
            for (z, node) in tree.nodes.iter().enumerate().rev() {
                best[z] = if node.terminal.is_some() {
                    row[terminal_indices[z]]
                } else if node.actor == Some(i) {
                    let mut maximum = f64::NEG_INFINITY;
                    let mut action = 0;
                    for (a, &child) in node.children.iter().enumerate() {
                        if best[child] > maximum {
                            maximum = best[child];
                            action = a;
                        }
                    }
                    actions[z * 169 + c] = action as u8;
                    maximum
                } else {
                    node.children.iter().map(|&child| best[child]).sum()
                };
            }
            best_response += best[0];
        }
        let value = value / options.deals as f64;
        let best_response = best_response / options.deals as f64;
        seats.push(FittedSeat {
            seat: i,
            name: game.config().seats[SeatId(i as u8)]
                .name
                .clone()
                .unwrap_or_else(|| format!("seat {i}")),
            value,
            best_response,
            gain: best_response - value,
        });
    }
    let mut gated = Vec::with_capacity(thresholds.len());
    let mut pass = GatedPass::new(tree, profile, &terminal_indices);
    for &threshold in thresholds {
        let mut actions = vec![vec![u8::MAX; tree.nodes.len() * 169]; tree.seats];
        let mut gated_seats = Vec::with_capacity(tree.seats);
        for (i, actions) in actions.iter_mut().enumerate() {
            let mut best_response = 0.0;
            let mut deviations = 0;
            for c in 0..169 {
                let start = (i * 169 + c) * terminals;
                let end = start + terminals;
                let rows = std::array::from_fn(|l| &lanes[l][start..end]);
                let (value, count) = pass.run(i, c, &weights[start..end], rows, threshold, actions);
                best_response += value;
                deviations += count;
            }
            let best_response = best_response / options.deals as f64;
            gated_seats.push(GatedSeat {
                seat: i,
                best_response,
                gain: best_response - seats[i].value,
                deviations,
            });
        }
        gated.push(GatedResponse {
            threshold,
            seats: gated_seats,
            actions,
        });
    }
    Ok(FittedResponses {
        seats,
        actions,
        deals: options.deals,
        seed: options.seed,
        gated,
    })
}

/// Reusable backward-pass storage for one seat/class and its sixteen replicates.
pub(super) struct GatedPass<'a> {
    tree: &'a Tree,
    profile: &'a Profile,
    terminal_indices: &'a [usize],
    values: Vec<f64>,
    lane_values: Vec<[f64; FIT_LANES]>,
}

impl<'a> GatedPass<'a> {
    pub(super) fn new(tree: &'a Tree, profile: &'a Profile, terminal_indices: &'a [usize]) -> Self {
        Self {
            tree,
            profile,
            terminal_indices,
            values: vec![0.0; tree.nodes.len()],
            lane_values: vec![[0.0; FIT_LANES]; tree.nodes.len()],
        }
    }

    pub(super) fn run(
        &mut self,
        seat: usize,
        class: usize,
        total: &[f64],
        lanes: [&[f64]; FIT_LANES],
        threshold: f64,
        actions: &mut [u8],
    ) -> (f64, u64) {
        let mut deviations = 0;
        for (z, node) in self.tree.nodes.iter().enumerate().rev() {
            if node.terminal.is_some() {
                let t = self.terminal_indices[z];
                self.values[z] = total[t];
                self.lane_values[z] = std::array::from_fn(|l| lanes[l][t]);
            } else if node.actor != Some(seat) {
                self.values[z] = node.children.iter().map(|&child| self.values[child]).sum();
                self.lane_values[z] = std::array::from_fn(|l| {
                    node.children
                        .iter()
                        .map(|&child| self.lane_values[child][l])
                        .sum()
                });
            } else {
                let mut maximum = f64::NEG_INFINITY;
                let mut action = 0;
                let mut follow = 0.0;
                let mut lane_follow = [0.0; FIT_LANES];
                let row = self.profile.row(self.tree, z, class);
                for (a, &child) in node.children.iter().enumerate() {
                    if self.values[child] > maximum {
                        maximum = self.values[child];
                        action = a;
                    }
                    follow += row[a] * self.values[child];
                    for (l, f) in lane_follow.iter_mut().enumerate() {
                        *f += row[a] * self.lane_values[child][l];
                    }
                }
                let child = node.children[action];
                let advantages: [f64; FIT_LANES] =
                    std::array::from_fn(|l| self.lane_values[child][l] - lane_follow[l]);
                let mean = advantages.iter().sum::<f64>() / FIT_LANES as f64;
                let se = (FIT_LANES as f64 / (FIT_LANES - 1) as f64
                    * advantages.iter().map(|a| (a - mean).powi(2)).sum::<f64>())
                .sqrt();
                if maximum - follow > threshold * se {
                    actions[z * 169 + class] = action as u8;
                    self.values[z] = maximum;
                    self.lane_values[z] = self.lane_values[child];
                    deviations += 1;
                } else {
                    actions[z * 169 + class] = FOLLOW;
                    self.values[z] = follow;
                    self.lane_values[z] = lane_follow;
                }
            }
        }
        (self.values[0], deviations)
    }
}
