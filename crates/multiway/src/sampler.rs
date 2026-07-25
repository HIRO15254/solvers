//! Exact sampling of one physical, correlated card world.
//!
//! Each seat's two-card combo is drawn independently from its weighted
//! range and the whole tuple is rejected if any cards collide.  Conditioning
//! the independent product distribution this way gives every legal tuple
//! probability proportional to the product of its range weights.  Once a
//! tuple is accepted, a single five-card runout is drawn from the remaining
//! deck and shared by every branch of an external-sampling traversal.

use cards::{ALL_CARDS, Card, CardSet, NUM_COMBOS, Range, combo_cards, combo_index};
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use crate::types::Street;

/// The largest table supported by the multiway solver.
pub const MAX_PLAYERS: usize = 9;
/// A useful guard against incompatible or extremely narrow ranges.
pub const DEFAULT_MAX_DEAL_ATTEMPTS: u32 = 100_000;
/// Deterministic independent draws used to diagnose practical acceptance.
pub const DEFAULT_PILOT_SAMPLES: u32 = 4_096;
const EXACT_DFS_MAX_TOTAL_SUPPORT: usize = 512;
const EXACT_DFS_MAX_SEAT_SUPPORT: usize = 128;

/// One complete physical deal.  Folded seats deliberately remain present:
/// their cards are still dead when later streets are revealed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampledWorld {
    hole_combos: Vec<usize>,
    runout: [Card; 5],
}

/// Construction-time evidence that a range set is both feasible and
/// practical for independent-product rejection sampling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplingDiagnostics {
    /// True when the bounded narrow-range condition allowed the exact DFS
    /// to prove that at least one disjoint joint tuple exists.
    pub exact_feasibility_checked: bool,
    pub pilot_samples: u32,
    pub pilot_accepted: u32,
}

impl SamplingDiagnostics {
    pub fn pilot_acceptance_rate(self) -> f64 {
        f64::from(self.pilot_accepted) / f64::from(self.pilot_samples)
    }
}

impl SampledWorld {
    pub fn new(hole_combos: Vec<usize>, runout: [Card; 5]) -> Result<Self, SampleError> {
        validate_world(&hole_combos, &runout)?;
        Ok(Self {
            hole_combos,
            runout,
        })
    }

    pub fn num_players(&self) -> usize {
        self.hole_combos.len()
    }

    pub fn hole_combo(&self, seat: usize) -> usize {
        self.hole_combos[seat]
    }

    pub fn hole_combos(&self) -> &[usize] {
        &self.hole_combos
    }

    pub fn hole_cards(&self, seat: usize) -> (Card, Card) {
        combo_cards(self.hole_combo(seat))
    }

    pub fn runout(&self) -> &[Card; 5] {
        &self.runout
    }

    /// Public cards visible on `street`.
    pub fn board(&self, street: Street) -> &[Card] {
        let len = match street {
            Street::Preflop => 0,
            Street::Flop => 3,
            Street::Turn => 4,
            Street::River => 5,
        };
        &self.runout[..len]
    }
}

#[derive(Clone, Debug)]
struct WeightedRange {
    cumulative: Vec<f64>,
    total: f64,
    support: Vec<usize>,
}

impl WeightedRange {
    fn new(range: &Range, seat: usize) -> Result<Self, SampleError> {
        let mut cumulative = Vec::with_capacity(NUM_COMBOS);
        let mut support = Vec::new();
        let mut total = 0.0;
        for (combo, &weight) in range.weights().iter().enumerate() {
            if !weight.is_finite() || weight < 0.0 {
                return Err(SampleError::InvalidWeight {
                    seat,
                    combo,
                    weight,
                });
            }
            total += f64::from(weight);
            if weight > 0.0 {
                support.push(combo);
            }
            cumulative.push(total);
        }
        if total <= 0.0 {
            return Err(SampleError::EmptyRange { seat });
        }
        Ok(Self {
            cumulative,
            total,
            support,
        })
    }

    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> usize {
        let needle = rng.gen_range(0.0..self.total);
        // `needle < total`, so an index always exists. `partition_point`
        // also skips zero-weight plateaus correctly.
        self.cumulative.partition_point(|&x| x <= needle)
    }

    /// Recovers `combo`'s range weight from the cumulative table (its build
    /// invariant: `cumulative[c] - cumulative[c-1] == weight(c)`, `0.0` for
    /// `c == 0`), avoiding a second, separate per-combo weight table.
    fn weight_of(&self, combo: usize) -> f64 {
        let upper = self.cumulative[combo];
        let lower = if combo == 0 {
            0.0
        } else {
            self.cumulative[combo - 1]
        };
        (upper - lower).max(0.0)
    }
}

/// A sampled world together with the rejection work required to obtain it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountedSample {
    pub world: SampledWorld,
    pub attempts: u32,
}

/// Exact joint range sampler for two through nine seats.
#[derive(Clone, Debug)]
pub struct DealSampler {
    ranges: Vec<WeightedRange>,
    range_fingerprint: [u8; 32],
    max_attempts: u32,
    uniform_full: bool,
    diagnostics: SamplingDiagnostics,
}

impl DealSampler {
    pub fn new(ranges: Vec<Range>) -> Result<Self, SampleError> {
        Self::with_max_attempts(ranges, DEFAULT_MAX_DEAL_ATTEMPTS)
    }

    pub fn with_max_attempts(ranges: Vec<Range>, max_attempts: u32) -> Result<Self, SampleError> {
        if !(2..=MAX_PLAYERS).contains(&ranges.len()) {
            return Err(SampleError::PlayerCount {
                found: ranges.len(),
            });
        }
        if max_attempts == 0 {
            return Err(SampleError::ZeroAttempts);
        }

        let uniform_full = ranges.iter().all(is_uniform_full_range);
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.ranges.v1");
        hasher.update(&(ranges.len() as u64).to_le_bytes());
        for range in &ranges {
            for &weight in range.weights() {
                hasher.update(&weight.to_bits().to_le_bytes());
            }
        }
        let range_fingerprint = *hasher.finalize().as_bytes();

        let ranges = ranges
            .iter()
            .enumerate()
            .map(|(seat, range)| WeightedRange::new(range, seat))
            .collect::<Result<Vec<_>, _>>()?;
        let exact_feasibility_checked = should_run_exact_feasibility(&ranges);
        if exact_feasibility_checked && !has_feasible_joint_tuple(&ranges) {
            return Err(SampleError::IncompatibleRanges);
        }
        let diagnostics = if uniform_full {
            SamplingDiagnostics {
                exact_feasibility_checked,
                pilot_samples: DEFAULT_PILOT_SAMPLES,
                // The physical-deck fast path never rejects.
                pilot_accepted: DEFAULT_PILOT_SAMPLES,
            }
        } else {
            pilot_acceptance(&ranges, range_fingerprint, exact_feasibility_checked)
        };
        if diagnostics.pilot_accepted == 0 {
            return Err(SampleError::PilotAcceptanceTooLow {
                samples: diagnostics.pilot_samples,
            });
        }

        Ok(Self {
            ranges,
            range_fingerprint,
            max_attempts,
            uniform_full,
            diagnostics,
        })
    }

    pub fn num_players(&self) -> usize {
        self.ranges.len()
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub fn range_fingerprint(&self) -> [u8; 32] {
        self.range_fingerprint
    }

    pub fn uses_uniform_fast_path(&self) -> bool {
        self.uniform_full
    }

    pub fn diagnostics(&self) -> SamplingDiagnostics {
        self.diagnostics
    }

    pub fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Result<SampledWorld, SampleError> {
        self.sample_counted(rng).map(|sample| sample.world)
    }

    pub fn sample_counted<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
    ) -> Result<CountedSample, SampleError> {
        if self.uniform_full {
            return Ok(CountedSample {
                world: self.sample_uniform_world(rng),
                attempts: 1,
            });
        }

        let mut hole_combos = vec![0; self.ranges.len()];
        for attempts in 1..=self.max_attempts {
            for (combo, range) in hole_combos.iter_mut().zip(&self.ranges) {
                *combo = range.sample(rng);
            }

            let mut dead = CardSet::EMPTY;
            let mut collision = false;
            for &combo in &hole_combos {
                let (a, b) = combo_cards(combo);
                if dead.contains(a) || dead.contains(b) {
                    collision = true;
                    break;
                }
                dead.insert(a);
                dead.insert(b);
            }
            if collision {
                continue;
            }

            let runout = draw_runout(dead, rng);
            return Ok(CountedSample {
                world: SampledWorld {
                    hole_combos,
                    runout,
                },
                attempts,
            });
        }

        Err(SampleError::AttemptsExhausted {
            attempts: self.max_attempts,
        })
    }

    /// Feasible hole-combo set for `seat` in `world`, used by both the dense
    /// and full-recall sparse vector-traverser paths: every positive-weight
    /// combo in `seat`'s configured range that does not intersect any other
    /// seat's sampled hole cards nor the sampled runout. `world`'s own dealt
    /// combo for `seat` is deliberately not special-cased (it may or may not
    /// appear in the returned set on its own merits); the deal stream that
    /// produced `world` is otherwise unchanged by this query. Returned in
    /// the range's deterministic support order, as `(combo, weight)` pairs.
    pub fn feasible_combos(&self, seat: usize, world: &SampledWorld) -> Vec<(usize, f64)> {
        let mut dead = CardSet::EMPTY;
        for other in 0..world.num_players() {
            if other == seat {
                continue;
            }
            let (a, b) = world.hole_cards(other);
            dead.insert(a);
            dead.insert(b);
        }
        for &card in world.runout() {
            dead.insert(card);
        }
        let range = &self.ranges[seat];
        range
            .support
            .iter()
            .filter_map(|&combo| {
                let (a, b) = combo_cards(combo);
                if dead.contains(a) || dead.contains(b) {
                    None
                } else {
                    Some((combo, range.weight_of(combo)))
                }
            })
            .collect()
    }

    fn sample_uniform_world<R: Rng + ?Sized>(&self, rng: &mut R) -> SampledWorld {
        let mut deck: Vec<Card> = ALL_CARDS.into_iter().collect();
        deck.shuffle(rng);
        let mut offset = 0;
        let mut hole_combos = Vec::with_capacity(self.num_players());
        for _ in 0..self.num_players() {
            hole_combos.push(combo_index(deck[offset], deck[offset + 1]));
            offset += 2;
        }
        let runout = deck[offset..offset + 5].try_into().expect("five cards");
        SampledWorld {
            hole_combos,
            runout,
        }
    }
}

fn should_run_exact_feasibility(ranges: &[WeightedRange]) -> bool {
    ranges
        .iter()
        .all(|range| range.support.len() <= EXACT_DFS_MAX_SEAT_SUPPORT)
        && ranges
            .iter()
            .map(|range| range.support.len())
            .sum::<usize>()
            <= EXACT_DFS_MAX_TOTAL_SUPPORT
}

fn has_feasible_joint_tuple(ranges: &[WeightedRange]) -> bool {
    fn search(ranges: &[WeightedRange], remaining: u16, dead: u64) -> bool {
        if remaining == 0 {
            return true;
        }
        let mut chosen_seat = 0usize;
        let mut chosen_legal = usize::MAX;
        for (seat, range) in ranges.iter().enumerate() {
            if remaining & (1 << seat) == 0 {
                continue;
            }
            let legal = range
                .support
                .iter()
                .filter(|&&combo| combo_card_mask(combo) & dead == 0)
                .count();
            if legal == 0 {
                return false;
            }
            if legal < chosen_legal {
                chosen_seat = seat;
                chosen_legal = legal;
            }
        }
        let next_remaining = remaining & !(1 << chosen_seat);
        ranges[chosen_seat].support.iter().any(|&combo| {
            let mask = combo_card_mask(combo);
            mask & dead == 0 && search(ranges, next_remaining, dead | mask)
        })
    }

    search(ranges, (1u16 << ranges.len()) - 1, 0)
}

fn pilot_acceptance(
    ranges: &[WeightedRange],
    range_fingerprint: [u8; 32],
    exact_feasibility_checked: bool,
) -> SamplingDiagnostics {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.deal-pilot.v1");
    hasher.update(&range_fingerprint);
    let mut rng = ChaCha20Rng::from_seed(*hasher.finalize().as_bytes());
    let mut combos = vec![0; ranges.len()];
    let mut accepted = 0;
    for _ in 0..DEFAULT_PILOT_SAMPLES {
        for (combo, range) in combos.iter_mut().zip(ranges) {
            *combo = range.sample(&mut rng);
        }
        accepted += u32::from(joint_tuple_is_legal(&combos));
    }
    SamplingDiagnostics {
        exact_feasibility_checked,
        pilot_samples: DEFAULT_PILOT_SAMPLES,
        pilot_accepted: accepted,
    }
}

fn joint_tuple_is_legal(combos: &[usize]) -> bool {
    let mut dead = 0u64;
    for &combo in combos {
        let mask = combo_card_mask(combo);
        if mask & dead != 0 {
            return false;
        }
        dead |= mask;
    }
    true
}

fn combo_card_mask(combo: usize) -> u64 {
    let (a, b) = combo_cards(combo);
    (1u64 << a.index()) | (1u64 << b.index())
}

fn is_uniform_full_range(range: &Range) -> bool {
    let Some(&first) = range.weights().first() else {
        return false;
    };
    first > 0.0 && range.weights().iter().all(|&weight| weight == first)
}

fn draw_runout<R: Rng + ?Sized>(dead: CardSet, rng: &mut R) -> [Card; 5] {
    let mut deck: Vec<Card> = ALL_CARDS
        .into_iter()
        .filter(|&card| !dead.contains(card))
        .collect();
    deck.shuffle(rng);
    deck[..5].try_into().expect("at least five live cards")
}

fn validate_world(hole_combos: &[usize], runout: &[Card; 5]) -> Result<(), SampleError> {
    if !(2..=MAX_PLAYERS).contains(&hole_combos.len()) {
        return Err(SampleError::PlayerCount {
            found: hole_combos.len(),
        });
    }
    let mut seen = CardSet::EMPTY;
    for (seat, &combo) in hole_combos.iter().enumerate() {
        if combo >= NUM_COMBOS {
            return Err(SampleError::InvalidCombo { seat, combo });
        }
        let (a, b) = combo_cards(combo);
        if seen.contains(a) || seen.contains(b) {
            return Err(SampleError::CardCollision);
        }
        seen.insert(a);
        seen.insert(b);
    }
    for &card in runout {
        if seen.contains(card) {
            return Err(SampleError::CardCollision);
        }
        seen.insert(card);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SampleError {
    #[error("multiway deals require 2..={MAX_PLAYERS} players, found {found}")]
    PlayerCount { found: usize },
    #[error("seat {seat} has an empty range")]
    EmptyRange { seat: usize },
    #[error("seat {seat} combo {combo} has invalid weight {weight}")]
    InvalidWeight {
        seat: usize,
        combo: usize,
        weight: f32,
    },
    #[error("maximum deal attempts must be positive")]
    ZeroAttempts,
    #[error("failed to draw a collision-free joint hand tuple after {attempts} attempts")]
    AttemptsExhausted { attempts: u32 },
    #[error("seat ranges have no collision-free joint hand tuple")]
    IncompatibleRanges,
    #[error("no legal joint tuple appeared in {samples} deterministic pilot draws")]
    PilotAcceptanceTooLow { samples: u32 },
    #[error("seat {seat} has out-of-range combo index {combo}")]
    InvalidCombo { seat: usize, combo: usize },
    #[error("sampled world contains duplicate cards")]
    CardCollision,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn uniform_nine_way_world_has_twenty_three_unique_cards() {
        let sampler = DealSampler::new(vec![Range::full(); 9]).unwrap();
        assert!(sampler.uses_uniform_fast_path());
        let mut rng = ChaCha20Rng::seed_from_u64(7);
        let sample = sampler.sample_counted(&mut rng).unwrap();
        assert_eq!(sample.attempts, 1);

        let mut cards = CardSet::EMPTY;
        for seat in 0..9 {
            let (a, b) = sample.world.hole_cards(seat);
            cards.insert(a);
            cards.insert(b);
        }
        for &card in sample.world.runout() {
            cards.insert(card);
        }
        assert_eq!(cards.len(), 23);
        assert_eq!(sample.world.board(Street::Preflop).len(), 0);
        assert_eq!(sample.world.board(Street::Flop).len(), 3);
        assert_eq!(sample.world.board(Street::Turn).len(), 4);
        assert_eq!(sample.world.board(Street::River).len(), 5);
    }

    #[test]
    fn seeded_sampling_is_reproducible() {
        let ranges = vec!["AA,AKs:0.25".parse().unwrap(); 3];
        let sampler = DealSampler::new(ranges).unwrap();
        assert!(sampler.diagnostics().pilot_accepted > 0);
        let mut a = ChaCha20Rng::seed_from_u64(99);
        let mut b = ChaCha20Rng::seed_from_u64(99);
        for _ in 0..20 {
            assert_eq!(
                sampler.sample_counted(&mut a),
                sampler.sample_counted(&mut b)
            );
        }
    }

    #[test]
    fn incompatible_ranges_fail_exact_preflight() {
        let aces = combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
        let mut only_aces = Range::default();
        only_aces.set_weight(aces, 1.0);
        assert_eq!(
            DealSampler::with_max_attempts(vec![only_aces.clone(), only_aces], 17).unwrap_err(),
            SampleError::IncompatibleRanges
        );
    }

    #[test]
    fn feasible_but_impractical_ranges_fail_deterministic_pilot() {
        let ace_spades: Card = "As".parse().unwrap();
        let mut mostly_blocked = Range::default();
        for combo in 0..NUM_COMBOS {
            let (a, b) = combo_cards(combo);
            if a == ace_spades || b == ace_spades {
                mostly_blocked.set_weight(combo, 1.0);
            }
        }
        let rare = combo_index("Kd".parse().unwrap(), "Qd".parse().unwrap());
        mostly_blocked.set_weight(rare, 1.0e-6);
        let blocked = combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
        let mut blocker = Range::default();
        blocker.set_weight(blocked, 1.0);

        assert_eq!(
            DealSampler::new(vec![mostly_blocked, blocker]).unwrap_err(),
            SampleError::PilotAcceptanceTooLow {
                samples: DEFAULT_PILOT_SAMPLES
            }
        );
    }

    #[test]
    fn validates_manually_constructed_worlds() {
        let aces = combo_index("As".parse().unwrap(), "Ah".parse().unwrap());
        let kings = combo_index("Ks".parse().unwrap(), "Kh".parse().unwrap());
        let runout = ["2c", "3d", "4h", "5s", "6c"].map(|s| s.parse().unwrap());
        assert!(SampledWorld::new(vec![aces, kings], runout).is_ok());

        let bad_runout = ["As", "3d", "4h", "5s", "6c"].map(|s| s.parse().unwrap());
        assert_eq!(
            SampledWorld::new(vec![aces, kings], bad_runout),
            Err(SampleError::CardCollision)
        );
    }
}
