//! Research-only structural draw split layered over an existing abstraction.
//!
//! This is a diagnostic representation, not a production abstraction or an
//! equity model. It preserves the wrapped bucket on preflop and river, and
//! splits each flop/turn bucket by two current-card structural flags.

use cards::{Card, NUM_COMBOS, combo_cards};

use crate::{BucketContext, BucketId, MultiwayAbstraction, Street};

const FINGERPRINT_DOMAIN: &[u8] = b"solvers.multiway.research.draw-aware.v1";

/// Construction failure for [`DrawAwareAbstraction`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DrawAwareAbstractionError {
    #[error("inner abstraction has zero {street:?} buckets for {active_opponents} opponents")]
    ZeroBuckets {
        street: Street,
        active_opponents: u8,
    },
    #[error("four-way {street:?} draw split overflows u32 for {active_opponents} opponents")]
    BucketCountOverflow {
        street: Street,
        active_opponents: u8,
    },
}

/// Adds structural flush- and straight-draw bits to flop/turn bucket IDs.
///
/// For flop and turn, the returned bucket is
/// `4 * inner_bucket + flush_draw + 2 * straight_draw`. A flush draw means
/// exactly four cards of one suit among board plus hole cards, with at least
/// one of those cards in the hole. A straight draw means no five-rank straight
/// is already present and some wheel/consecutive five-rank window contains
/// exactly four available ranks, including a rank contributed by the hole
/// cards that is absent from the board.
#[derive(Clone, Debug)]
pub struct DrawAwareAbstraction<A> {
    inner: A,
    fingerprint: [u8; 32],
}

impl<A: MultiwayAbstraction> DrawAwareAbstraction<A> {
    pub fn new(inner: A) -> Result<Self, DrawAwareAbstractionError> {
        for active_opponents in 0..=8 {
            for street in [Street::Preflop, Street::Flop, Street::Turn, Street::River] {
                let count = inner.num_buckets(street, active_opponents);
                if count == 0 {
                    return Err(DrawAwareAbstractionError::ZeroBuckets {
                        street,
                        active_opponents,
                    });
                }
                if matches!(street, Street::Flop | Street::Turn) && count.checked_mul(4).is_none() {
                    return Err(DrawAwareAbstractionError::BucketCountOverflow {
                        street,
                        active_opponents,
                    });
                }
            }
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(FINGERPRINT_DOMAIN);
        hasher.update(&inner.fingerprint());
        let fingerprint = *hasher.finalize().as_bytes();
        Ok(Self { inner, fingerprint })
    }

    pub fn inner(&self) -> &A {
        &self.inner
    }

    pub fn into_inner(self) -> A {
        self.inner
    }

    fn expanded_bucket(
        &self,
        street: Street,
        board: &[Card],
        combo: usize,
        active_opponents: u8,
        inner_bucket: BucketId,
    ) -> BucketId {
        let inner_count = self.inner.num_buckets(street, active_opponents);
        assert!(
            inner_bucket < inner_count,
            "inner abstraction returned bucket outside its declared range"
        );
        if !matches!(street, Street::Flop | Street::Turn) {
            return inner_bucket;
        }
        let (flush_draw, straight_draw) = draw_flags(board, combo);
        inner_bucket
            .checked_mul(4)
            .and_then(|bucket| {
                bucket.checked_add(u32::from(flush_draw) + 2 * u32::from(straight_draw))
            })
            .expect("constructor validated expanded bucket counts")
    }
}

impl<A: MultiwayAbstraction> MultiwayAbstraction for DrawAwareAbstraction<A> {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        let count = self.inner.num_buckets(street, active_opponents);
        assert!(count > 0, "inner abstraction has zero buckets");
        if matches!(street, Street::Flop | Street::Turn) {
            count
                .checked_mul(4)
                .expect("constructor validated expanded bucket counts")
        } else {
            count
        }
    }

    fn bucket(&self, context: BucketContext<'_>) -> BucketId {
        validate_context(context);
        let inner_bucket = self.inner.bucket(context);
        self.expanded_bucket(
            context.street,
            context.board,
            context.combo,
            context.active_opponents,
            inner_bucket,
        )
    }

    fn bucket_batch(
        &self,
        street: Street,
        board: &[Card],
        active_opponents: u8,
        combos: &[usize],
    ) -> Vec<BucketId> {
        validate_public_context(street, board, active_opponents);
        for &combo in combos {
            validate_combo(board, combo);
        }
        let inner = self
            .inner
            .bucket_batch(street, board, active_opponents, combos);
        assert_eq!(
            inner.len(),
            combos.len(),
            "inner batch result length differs from input"
        );
        combos
            .iter()
            .copied()
            .zip(inner)
            .map(|(combo, inner_bucket)| {
                self.expanded_bucket(street, board, combo, active_opponents, inner_bucket)
            })
            .collect()
    }

    fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
}

fn validate_context(context: BucketContext<'_>) {
    validate_public_context(context.street, context.board, context.active_opponents);
    validate_combo(context.board, context.combo);
}

fn validate_public_context(street: Street, board: &[Card], active_opponents: u8) {
    assert!(active_opponents <= 8, "active opponent count exceeds 9-max");
    let expected_board_len = match street {
        Street::Preflop => 0,
        Street::Flop => 3,
        Street::Turn => 4,
        Street::River => 5,
    };
    assert_eq!(
        board.len(),
        expected_board_len,
        "board length does not match street"
    );

    let mut seen = 0u64;
    for &card in board {
        let bit = 1u64 << card.index();
        assert_eq!(seen & bit, 0, "board contains duplicate card");
        seen |= bit;
    }
}

fn validate_combo(board: &[Card], combo: usize) {
    assert!(combo < NUM_COMBOS, "combo index out of range");
    let (a, b) = combo_cards(combo);
    assert!(
        board.iter().all(|&card| card != a && card != b),
        "hole cards collide with board"
    );
}

fn draw_flags(board: &[Card], combo: usize) -> (bool, bool) {
    let (hi, lo) = combo_cards(combo);

    let mut suit_counts = [0u8; 4];
    let mut hole_suit_counts = [0u8; 4];
    for &card in board {
        suit_counts[card.suit() as usize] += 1;
    }
    for card in [hi, lo] {
        suit_counts[card.suit() as usize] += 1;
        hole_suit_counts[card.suit() as usize] += 1;
    }
    let flush_draw = (0..4).any(|suit| suit_counts[suit] == 4 && hole_suit_counts[suit] > 0);

    let mut board_ranks = 0u16;
    for &card in board {
        board_ranks |= 1u16 << card.rank();
    }
    let mut all_ranks = board_ranks;
    let mut hole_exclusive_ranks = 0u16;
    for card in [hi, lo] {
        let rank = 1u16 << card.rank();
        all_ranks |= rank;
        if board_ranks & rank == 0 {
            hole_exclusive_ranks |= rank;
        }
    }

    let wheel = (1u16 << 12) | 0b1111;
    let has_straight = rank_windows(wheel).any(|window| all_ranks & window == window);
    let straight_draw = !has_straight
        && rank_windows(wheel).any(|window| {
            (all_ranks & window).count_ones() == 4 && hole_exclusive_ranks & window != 0
        });
    (flush_draw, straight_draw)
}

fn rank_windows(wheel: u16) -> impl Iterator<Item = u16> {
    std::iter::once(wheel).chain((0..=8).map(|start| 0b1_1111u16 << start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::{NUM_CLASSES, combo_index};

    #[derive(Clone, Debug)]
    struct FixedAbstraction {
        postflop_count: u32,
        fingerprint: [u8; 32],
        zero_at: Option<(Street, u8)>,
    }

    impl FixedAbstraction {
        fn normal(postflop_count: u32, fingerprint_byte: u8) -> Self {
            Self {
                postflop_count,
                fingerprint: [fingerprint_byte; 32],
                zero_at: None,
            }
        }
    }

    impl MultiwayAbstraction for FixedAbstraction {
        fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
            if self.zero_at == Some((street, active_opponents)) {
                return 0;
            }
            match street {
                Street::Preflop => NUM_CLASSES as u32,
                _ => self.postflop_count,
            }
        }

        fn bucket(&self, context: BucketContext<'_>) -> BucketId {
            match context.street {
                Street::Preflop => (context.combo % NUM_CLASSES) as u32,
                _ => (context.combo as u32) % self.postflop_count,
            }
        }

        fn fingerprint(&self) -> [u8; 32] {
            self.fingerprint
        }
    }

    fn cards(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|card| card.parse().unwrap())
            .collect()
    }

    fn combo(text: &str) -> usize {
        let cards = cards(text);
        combo_index(cards[0], cards[1])
    }

    fn split_bits(board: &str, hole: &str) -> u32 {
        let board = cards(board);
        let street = match board.len() {
            3 => Street::Flop,
            4 => Street::Turn,
            _ => panic!("test board must be flop or turn"),
        };
        let abstraction = DrawAwareAbstraction::new(FixedAbstraction::normal(1, 1)).unwrap();
        abstraction.bucket(BucketContext {
            street,
            board: &board,
            combo: combo(hole),
            active_opponents: 2,
        })
    }

    #[test]
    fn structural_flags_cover_flush_straight_combo_and_wheel_draws() {
        assert_eq!(split_bits("Ah 7h 2c", "Kh Qh"), 1);
        assert_eq!(split_bits("5c 6d Ks", "7h 8s"), 2);
        assert_eq!(split_bits("5h 6h Ks", "7h 8h"), 3);
        assert_eq!(split_bits("2c 3d Kh", "4s 5h"), 2);
    }

    #[test]
    fn made_and_board_only_structures_are_not_draw_flags() {
        assert_eq!(split_bits("Ah 7h 2h 3c", "Kh Qh"), 0);
        assert_eq!(split_bits("4c 5d 6h Ks", "7c 8d"), 0);
        assert_eq!(split_bits("2h 5h 9h Kh", "Ac Ad"), 0);
        assert_eq!(split_bits("5c 6d 7h 8s", "Ac Ad"), 0);
    }

    #[test]
    fn flags_are_invariant_under_global_suit_permutation() {
        let board = cards("5h 6h Ks");
        let hole = cards("7h 8h");
        let permuted_board: Vec<Card> = board
            .iter()
            .map(|card| card.with_suit((card.suit() + 1) % 4))
            .collect();
        let permuted_hole: Vec<Card> = hole
            .iter()
            .map(|card| card.with_suit((card.suit() + 1) % 4))
            .collect();
        assert_eq!(
            draw_flags(&board, combo_index(hole[0], hole[1])),
            draw_flags(
                &permuted_board,
                combo_index(permuted_hole[0], permuted_hole[1])
            )
        );
    }

    #[test]
    fn preflop_and_river_preserve_inner_buckets_and_counts() {
        let abstraction = DrawAwareAbstraction::new(FixedAbstraction::normal(17, 1)).unwrap();
        let hole = combo("As Kd");
        let river = cards("2c 4d 6h 8s Tc");
        assert_eq!(abstraction.num_buckets(Street::Preflop, 3), 169);
        assert_eq!(abstraction.num_buckets(Street::Flop, 3), 68);
        assert_eq!(abstraction.num_buckets(Street::Turn, 3), 68);
        assert_eq!(abstraction.num_buckets(Street::River, 3), 17);
        assert_eq!(
            abstraction.bucket(BucketContext {
                street: Street::Preflop,
                board: &[],
                combo: hole,
                active_opponents: 3,
            }),
            (hole % NUM_CLASSES) as u32
        );
        assert_eq!(
            abstraction.bucket(BucketContext {
                street: Street::River,
                board: &river,
                combo: hole,
                active_opponents: 3,
            }),
            (hole as u32) % 17
        );
    }

    #[test]
    fn constructor_checks_zero_overflow_and_fingerprint_domain() {
        let mut zero = FixedAbstraction::normal(4, 1);
        zero.zero_at = Some((Street::Turn, 7));
        assert_eq!(
            DrawAwareAbstraction::new(zero).unwrap_err(),
            DrawAwareAbstractionError::ZeroBuckets {
                street: Street::Turn,
                active_opponents: 7,
            }
        );

        assert_eq!(
            DrawAwareAbstraction::new(FixedAbstraction::normal(u32::MAX, 1)).unwrap_err(),
            DrawAwareAbstractionError::BucketCountOverflow {
                street: Street::Flop,
                active_opponents: 0,
            }
        );

        let a = DrawAwareAbstraction::new(FixedAbstraction::normal(4, 1)).unwrap();
        let b = DrawAwareAbstraction::new(FixedAbstraction::normal(4, 2)).unwrap();
        assert_ne!(a.fingerprint(), [1; 32]);
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn batch_matches_scalar_and_all_buckets_stay_in_range() {
        let abstraction = DrawAwareAbstraction::new(FixedAbstraction::normal(7, 1)).unwrap();
        let board = cards("5h 6h Ks");
        let combos = [combo("7h 8h"), combo("Ac Ad"), combo("Qc Jd")];
        let batch = abstraction.bucket_batch(Street::Flop, &board, 5, &combos);
        let scalar: Vec<_> = combos
            .iter()
            .map(|&combo| {
                abstraction.bucket(BucketContext {
                    street: Street::Flop,
                    board: &board,
                    combo,
                    active_opponents: 5,
                })
            })
            .collect();
        assert_eq!(batch, scalar);
        assert!(
            batch
                .iter()
                .all(|&bucket| bucket < abstraction.num_buckets(Street::Flop, 5))
        );
    }

    #[test]
    fn invalid_contexts_are_rejected_before_calling_inner() {
        let abstraction = DrawAwareAbstraction::new(FixedAbstraction::normal(4, 1)).unwrap();
        let bad_len = std::panic::catch_unwind(|| {
            abstraction.bucket(BucketContext {
                street: Street::Flop,
                board: &cards("2c 3d 4h 5s"),
                combo: combo("As Kd"),
                active_opponents: 1,
            })
        });
        assert!(bad_len.is_err());

        let overlap = std::panic::catch_unwind(|| {
            abstraction.bucket(BucketContext {
                street: Street::Flop,
                board: &cards("As 3d 4h"),
                combo: combo("As Kd"),
                active_opponents: 1,
            })
        });
        assert!(overlap.is_err());

        let duplicate_board = std::panic::catch_unwind(|| {
            abstraction.bucket(BucketContext {
                street: Street::Flop,
                board: &cards("2c 2c 4h"),
                combo: combo("As Kd"),
                active_opponents: 1,
            })
        });
        assert!(duplicate_board.is_err());

        let bad_opponents = std::panic::catch_unwind(|| {
            abstraction.bucket(BucketContext {
                street: Street::Preflop,
                board: &[],
                combo: combo("As Kd"),
                active_opponents: 9,
            })
        });
        assert!(bad_opponents.is_err());

        let bad_combo = std::panic::catch_unwind(|| {
            abstraction.bucket(BucketContext {
                street: Street::Preflop,
                board: &[],
                combo: NUM_COMBOS,
                active_opponents: 1,
            })
        });
        assert!(bad_combo.is_err());

        let empty_batch_bad_context = std::panic::catch_unwind(|| {
            abstraction.bucket_batch(Street::Turn, &cards("2c 3d 4h"), 9, &[])
        });
        assert!(empty_batch_bad_context.is_err());
    }
}
