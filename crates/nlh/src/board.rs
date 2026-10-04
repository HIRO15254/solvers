//! Suit-permutation-invariant board summaries for the postflop tree
//! script's board predicates.
//!
//! The tree builder merges suit-isomorphic runouts into a single branch as
//! an *exact* quotient by suit permutation (`iso_merging`). A board
//! predicate is evaluated once per merged class, so if it could tell two
//! members of that class apart it would silently build a wrong tree, with
//! no panic to catch it. [`BoardFacts`] is the entire vocabulary those
//! predicates get to read, and it is built so that reading it can never
//! break the quotient -- see the WHY comment in [`BoardFacts::new`].

use crate::card::{Card, Rank};

/// Suit-permutation-invariant summary of a dealt board, in the vocabulary
/// the tree script's board predicates read.
///
/// The six boolean textures (`paired`, `monotone`, ...) are methods rather
/// than fields: the spec defines each as a function of the counts here, and
/// storing them separately would let the two drift apart. Keeping
/// `BoardFacts` to plain counts also keeps it small and `Copy`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardFacts {
    /// Number of cards summarized: 3, 4, or 5.
    pub cards: u8,
    /// Distinct suits present on the board.
    pub suits: u8,
    /// Distinct ranks present on the board.
    pub ranks: u8,
    /// Largest number of board ranks that fall inside any 5-rank window
    /// (the ace counts both low and high), so a straight is possible with
    /// two hole cards exactly when this is at least 3. Exposed as a count,
    /// not just the `straight_possible` bool, because on the river the
    /// bool is true for most boards and finer tree conditions need the
    /// number.
    pub straight_ranks: u8,
    /// The most cards of a single suit on the board.
    pub max_suit_count: u8,
    /// The board's highest rank.
    pub high_rank: Rank,
    /// The board's lowest rank.
    pub low_rank: Rank,
}

impl BoardFacts {
    /// Summarizes a dealt board of 3 (flop), 4 (turn), or 5 (river) cards.
    ///
    /// Panics if `board` is empty, has more than 5 cards, or repeats a
    /// card.
    pub fn new(board: &[Card]) -> BoardFacts {
        assert!(!board.is_empty(), "board must have at least one card");
        assert!(board.len() <= 5, "board must have at most five cards");
        for (i, &a) in board.iter().enumerate() {
            for &b in &board[i + 1..] {
                assert!(a != b, "board repeats a card: {a:?}");
            }
        }

        // WHY: `iso_merging` merges suit-isomorphic runouts into one tree
        // branch as an *exact* quotient by suit permutation. Any board
        // predicate that could tell two members of that merged class apart
        // would silently build a wrong tree -- no panic, no failing
        // assertion. The fix is structural, not a rule someone has to
        // remember: the only thing this function reads about a card's suit
        // is a tally into a `[u8; 4]` indexed by `card.suit()`, and that
        // tally is reduced to functions of the tally *multiset* --
        // `suits` (how many entries are nonzero) and `max_suit_count` (the
        // largest entry) -- before anything leaves this function. A suit
        // permutation only permutes which slot of the array holds which
        // count; it cannot change how many slots are nonzero or what the
        // largest one is. That is the whole proof. Any future predicate
        // that needs to know *which* suit -- "flush draw in spades", "the
        // ace is suited to the flush" -- would need to read more than the
        // tally, which breaks this guarantee: it must be dropped, not
        // added.
        let mut suit_tally = [0u8; 4];
        let mut rank_mask: u16 = 0;
        let mut high_rank: Rank = 0;
        let mut low_rank: Rank = 12;
        for &card in board {
            suit_tally[card.suit() as usize] += 1;
            let rank = card.rank();
            rank_mask |= 1 << rank;
            high_rank = high_rank.max(rank);
            low_rank = low_rank.min(rank);
        }

        let suits = suit_tally.iter().filter(|&&count| count > 0).count() as u8;
        let max_suit_count = suit_tally.into_iter().max().unwrap_or(0);
        let ranks = rank_mask.count_ones() as u8;

        // 14-bit straight mask: bit 0 is the ace playing low, bit `r + 1`
        // is rank `r` present, so the ace (rank 12) sets both bit 0 and
        // bit 13. `straight_ranks` is the most set bits any 5-bit window
        // covers, checked at every window start `0..=9` (the highest start
        // that still fits a 5-bit window below bit 14).
        let mut straight_mask: u16 = rank_mask << 1;
        if rank_mask & (1 << 12) != 0 {
            straight_mask |= 1;
        }
        let straight_ranks = (0..=9)
            .map(|start| (straight_mask & (0b11111 << start)).count_ones() as u8)
            .max()
            .unwrap_or(0);

        BoardFacts {
            cards: board.len() as u8,
            suits,
            ranks,
            straight_ranks,
            max_suit_count,
            high_rank,
            low_rank,
        }
    }

    /// True when the board repeats a rank (fewer distinct ranks than
    /// cards).
    pub fn paired(&self) -> bool {
        self.ranks < self.cards
    }

    /// True when every card on the board shares one suit.
    pub fn monotone(&self) -> bool {
        self.suits == 1
    }

    /// True when the board shows exactly two distinct suits.
    pub fn two_tone(&self) -> bool {
        self.suits == 2
    }

    /// True when every card on the board has a different suit.
    pub fn rainbow(&self) -> bool {
        self.suits == self.cards
    }

    /// True when a flush is possible with two hole cards, i.e. the board
    /// already shows three or more cards of one suit.
    pub fn flush_possible(&self) -> bool {
        self.max_suit_count >= 3
    }

    /// True when a straight is possible with two hole cards, i.e. some
    /// 5-rank window already holds three or more board ranks.
    pub fn straight_possible(&self) -> bool {
        self.straight_ranks >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(cards: &str) -> Vec<Card> {
        cards
            .split_whitespace()
            .map(|c| c.parse().unwrap())
            .collect()
    }

    /// All 24 permutations of the four suits, generated inline rather than
    /// via `hand-index`'s permutation machinery: `cards` cannot depend on
    /// `hand-index`, and a test should not be validated by the same code it
    /// validates.
    fn all_suit_permutations() -> Vec<[u8; 4]> {
        let mut perms = Vec::with_capacity(24);
        let suits = [0u8, 1, 2, 3];
        for a in suits {
            for b in suits {
                if b == a {
                    continue;
                }
                for c in suits {
                    if c == a || c == b {
                        continue;
                    }
                    for d in suits {
                        if d == a || d == b || d == c {
                            continue;
                        }
                        perms.push([a, b, c, d]);
                    }
                }
            }
        }
        perms
    }

    fn apply_permutation(cards: &[Card], perm: &[u8; 4]) -> Vec<Card> {
        cards
            .iter()
            .map(|&c| Card::new(c.rank(), perm[c.suit() as usize]))
            .collect()
    }

    /// Calls `f` with every size-`k` combination of `0..n`, in colex order,
    /// via depth-first recursion.
    fn for_each_combination(n: u8, k: usize, f: &mut dyn FnMut(&[Card])) {
        fn helper(n: u8, k: usize, start: u8, combo: &mut Vec<Card>, f: &mut dyn FnMut(&[Card])) {
            if combo.len() == k {
                f(combo);
                return;
            }
            for i in start..n {
                combo.push(Card::from_index(i));
                helper(n, k, i + 1, combo, f);
                combo.pop();
            }
        }
        let mut combo = Vec::with_capacity(k);
        helper(n, k, 0, &mut combo, f);
    }

    fn assert_invariant_under_all_suit_permutations(board: &[Card]) {
        let facts = BoardFacts::new(board);
        for perm in all_suit_permutations() {
            let permuted = apply_permutation(board, &perm);
            assert_eq!(
                BoardFacts::new(&permuted),
                facts,
                "board {board:?} permuted by {perm:?} to {permuted:?} changed BoardFacts"
            );
        }
    }

    /// Exhaustive: every one of the `C(52,3) = 22,100` three-card boards,
    /// each checked against all 24 suit permutations. This is the module's
    /// core guarantee, so it is not sampled.
    #[test]
    fn flop_invariant_under_every_suit_permutation() {
        for_each_combination(52, 3, &mut |board| {
            assert_invariant_under_all_suit_permutations(board);
        });
    }

    /// `C(52,4) = 270,725` and `C(52,5) = 2,598,960` boards are too many to
    /// check exhaustively in a debug build on every run, so this walks them
    /// with a fixed prime stride (97): deterministic coverage across the
    /// whole space without an RNG dependency.
    #[test]
    fn turn_and_river_invariant_under_strided_suit_permutation_sweep() {
        const STRIDE: u64 = 97;
        let mut counter = 0u64;
        for_each_combination(52, 4, &mut |board| {
            if counter.is_multiple_of(STRIDE) {
                assert_invariant_under_all_suit_permutations(board);
            }
            counter += 1;
        });
        let mut counter = 0u64;
        for_each_combination(52, 5, &mut |board| {
            if counter.is_multiple_of(STRIDE) {
                assert_invariant_under_all_suit_permutations(board);
            }
            counter += 1;
        });
    }

    /// Full exhaustive sweep of turns and rivers. Too expensive for a
    /// normal debug run; CI runs `#[ignore]`d tests in release via
    /// `--include-ignored`.
    #[test]
    #[ignore]
    fn turn_and_river_invariant_under_every_suit_permutation_exhaustive() {
        for_each_combination(52, 4, &mut |board| {
            assert_invariant_under_all_suit_permutations(board);
        });
        for_each_combination(52, 5, &mut |board| {
            assert_invariant_under_all_suit_permutations(board);
        });
    }

    #[test]
    fn straight_ranks_ace_king_two_is_two() {
        let facts = BoardFacts::new(&board("Ah Kd 2c"));
        assert_eq!(facts.straight_ranks, 2);
        assert!(!facts.straight_possible());
    }

    #[test]
    fn straight_ranks_nine_seven_five_is_three() {
        let facts = BoardFacts::new(&board("9h 7d 5c"));
        assert_eq!(facts.straight_ranks, 3);
        assert!(facts.straight_possible());
    }

    /// `straight_ranks` bottoms out at 1, not 2: a board can have every
    /// rank more than four apart (`2 7 Q`), and a trip board has only one
    /// distinct rank at all. The spec table says 1..=5 for this reason --
    /// nobody should "tighten" it back to 2.
    #[test]
    fn straight_ranks_can_be_one() {
        assert_eq!(BoardFacts::new(&board("2h 7d Qc")).straight_ranks, 1);
        assert_eq!(BoardFacts::new(&board("2c 2d 2h")).straight_ranks, 1);
    }

    #[test]
    fn high_and_low_rank_are_pinned() {
        let facts = BoardFacts::new(&board("2h 7d Kc"));
        assert_eq!(facts.high_rank, 11); // King
        assert_eq!(facts.low_rank, 0); // Two
    }

    #[test]
    fn paired_flop() {
        let facts = BoardFacts::new(&board("9h 9d 2c"));
        assert_eq!(facts.cards, 3);
        assert_eq!(facts.ranks, 2);
        assert!(facts.paired());
    }

    #[test]
    fn monotone_flop() {
        let facts = BoardFacts::new(&board("2h 5h 9h"));
        assert!(facts.monotone());
        assert!(!facts.two_tone());
        assert!(!facts.rainbow());
        assert!(facts.flush_possible());
    }

    #[test]
    fn two_tone_flop() {
        let facts = BoardFacts::new(&board("2h 5h 9c"));
        assert!(!facts.monotone());
        assert!(facts.two_tone());
        assert!(!facts.rainbow());
        assert!(!facts.flush_possible());
    }

    #[test]
    fn rainbow_flop() {
        let facts = BoardFacts::new(&board("2h 5d 9c"));
        assert!(!facts.monotone());
        assert!(!facts.two_tone());
        assert!(facts.rainbow());
        assert_eq!(facts.suits, 3);
    }

    #[test]
    fn flush_possible_on_turn_with_three_of_a_suit() {
        let facts = BoardFacts::new(&board("2h 5h 9h Kd"));
        assert_eq!(facts.cards, 4);
        assert_eq!(facts.max_suit_count, 3);
        assert!(facts.flush_possible());
    }

    /// Pigeonhole: a 5-card board has only 4 suits to spread across 5
    /// cards, so `rainbow()` (`suits == cards`) is always false. Pinned
    /// explicitly so nobody "fixes" it later.
    #[test]
    fn rainbow_is_impossible_on_a_five_card_board() {
        let facts = BoardFacts::new(&board("2h 5d 9c Ks Ac"));
        assert_eq!(facts.cards, 5);
        assert!(facts.suits <= 4);
        assert!(!facts.rainbow());
    }

    #[test]
    fn river_texture_golden() {
        let facts = BoardFacts::new(&board("2h 5d 9c Ks Kc"));
        assert_eq!(facts.cards, 5);
        assert_eq!(facts.ranks, 4);
        assert!(facts.paired());
        assert_eq!(facts.high_rank, 11); // King
        assert_eq!(facts.low_rank, 0); // Two
    }

    #[test]
    #[should_panic]
    fn new_panics_on_empty_board() {
        BoardFacts::new(&[]);
    }

    #[test]
    #[should_panic]
    fn new_panics_on_too_many_cards() {
        let cards = board("2h 3h 4h 5h 6h 7h");
        BoardFacts::new(&cards);
    }

    #[test]
    #[should_panic]
    fn new_panics_on_repeated_card() {
        let cards = board("2h 2h 3h");
        BoardFacts::new(&cards);
    }
}
