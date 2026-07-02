//! Suit-isomorphism canonicalization for boards and chance deals.
//!
//! Two boards are strategically identical if one maps onto the other by a
//! permutation of the four suits. This crate provides:
//!
//! - [`canonicalize_board`]: the canonical representative of a board,
//! - [`canonical_flops`]: the 1,755 canonical flops with multiplicities,
//! - [`deal_groups`]: candidate next cards grouped into isomorphism classes
//!   under the suit permutations that stabilize the current board — the
//!   building block for merging turn/river chance branches in the holdem
//!   tree builder.
//!
//! A board is treated as an unordered flop plus ordered later streets; the
//! canonical form minimizes the (sorted flop, turn, river) card-index tuple
//! over all 24 suit permutations. The full Waugh perfect hand indexing (for
//! abstraction cache keys) is planned for a later milestone; this module
//! only handles boards, which is all the exact postflop solver needs.

use cards::{Card, Suit};

/// A permutation of the four suits; `perm[s]` is the image of suit `s`.
pub type SuitPerm = [Suit; 4];

/// All 24 suit permutations.
pub fn all_suit_perms() -> [SuitPerm; 24] {
    let mut perms = [[0; 4]; 24];
    let mut i = 0;
    for a in 0..4u8 {
        for b in 0..4u8 {
            if b == a {
                continue;
            }
            for c in 0..4u8 {
                if c == a || c == b {
                    continue;
                }
                let d = 6 - a - b - c;
                perms[i] = [a, b, c, d];
                i += 1;
            }
        }
    }
    perms
}

fn apply_perm(perm: &SuitPerm, card: Card) -> Card {
    card.with_suit(perm[card.suit() as usize])
}

/// A board: unordered flop, then zero or more ordered later cards
/// (turn, river, or stud upcards).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Board {
    /// Flop cards sorted ascending by card index.
    pub flop: Vec<Card>,
    /// Later streets in deal order.
    pub later: Vec<Card>,
}

impl Board {
    pub fn new(flop: &[Card], later: &[Card]) -> Self {
        let mut flop = flop.to_vec();
        flop.sort_unstable();
        Board {
            flop,
            later: later.to_vec(),
        }
    }

    pub fn cards(&self) -> impl Iterator<Item = Card> + '_ {
        self.flop.iter().chain(self.later.iter()).copied()
    }

    fn permuted(&self, perm: &SuitPerm) -> Board {
        let mut flop: Vec<Card> = self.flop.iter().map(|&c| apply_perm(perm, c)).collect();
        flop.sort_unstable();
        Board {
            flop,
            later: self.later.iter().map(|&c| apply_perm(perm, c)).collect(),
        }
    }
}

/// Returns the canonical representative of `board` and the permutation that
/// produced it.
pub fn canonicalize_board(board: &Board) -> (Board, SuitPerm) {
    all_suit_perms()
        .iter()
        .map(|perm| (board.permuted(perm), *perm))
        .min()
        .unwrap()
}

/// The suit permutations that map `board` to itself (flop as a set, later
/// streets pointwise).
pub fn stabilizer(board: &Board) -> Vec<SuitPerm> {
    all_suit_perms()
        .iter()
        .filter(|perm| &board.permuted(perm) == board)
        .copied()
        .collect()
}

/// One isomorphism class of candidate next cards on `board`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DealGroup {
    /// Lowest-index member, used as the dealt representative.
    pub representative: Card,
    /// All members of the class, ascending. `members.len()` is the chance
    /// multiplicity folded into the deal weight by tree builders.
    pub members: Vec<Card>,
}

/// Groups the cards not on `board` (nor in `dead`) into isomorphism classes
/// under the stabilizer of `board`.
pub fn deal_groups(board: &Board, dead: &[Card]) -> Vec<DealGroup> {
    let stab = stabilizer(board);
    let used: Vec<Card> = board.cards().chain(dead.iter().copied()).collect();
    let mut groups: Vec<DealGroup> = Vec::new();
    let mut assigned = [false; 52];
    for card in cards::ALL_CARDS {
        if assigned[card.index()] || used.contains(&card) {
            continue;
        }
        let mut members: Vec<Card> = stab.iter().map(|perm| apply_perm(perm, card)).collect();
        members.sort_unstable();
        members.dedup();
        // With dead cards present, the true symmetry group is the stabilizer
        // of board + dead; dropping dead members from an orbit would merge
        // cards that are no longer interchangeable. Recompute conservatively:
        // if any orbit member is dead, split the orbit into singletons.
        if members.iter().any(|m| used.contains(m)) {
            members = vec![card];
        }
        for &m in &members {
            assigned[m.index()] = true;
        }
        groups.push(DealGroup {
            representative: members[0],
            members,
        });
    }
    groups
}

/// All canonical flops with their multiplicities (how many of the 22,100 raw
/// flops map to each). Multiplicities sum to 22,100; the list has 1,755
/// entries.
pub fn canonical_flops() -> Vec<(Board, u32)> {
    let mut counts: std::collections::BTreeMap<Board, u32> = std::collections::BTreeMap::new();
    for a in 0..52u8 {
        for b in 0..a {
            for c in 0..b {
                let board = Board::new(
                    &[
                        Card::from_index(a),
                        Card::from_index(b),
                        Card::from_index(c),
                    ],
                    &[],
                );
                let (canon, _) = canonicalize_board(&board);
                *counts.entry(canon).or_insert(0) += 1;
            }
        }
    }
    counts.into_iter().collect()
}

/// Number of canonical unordered k-card boards. Allocation-free
/// enumeration so the count-pinning tests stay fast.
#[doc(hidden)]
pub fn canonical_unordered_board_count(k: usize) -> usize {
    assert!((3..=5).contains(&k));
    let perms = all_suit_perms();
    let mut seen = std::collections::BTreeSet::new();
    let mut combo = vec![0u8; k];
    // Enumerate ascending card-index combinations of size k.
    fn rec(
        start: u8,
        depth: usize,
        combo: &mut Vec<u8>,
        perms: &[SuitPerm; 24],
        seen: &mut std::collections::BTreeSet<[u8; 5]>,
    ) {
        let k = combo.len();
        if depth == k {
            let mut best = [u8::MAX; 5];
            for perm in perms {
                let mut mapped = [u8::MAX; 5];
                for (i, &c) in combo.iter().enumerate() {
                    mapped[i] = 4 * (c / 4) + perm[(c % 4) as usize];
                }
                mapped[..k].sort_unstable();
                if mapped < best {
                    best = mapped;
                }
            }
            seen.insert(best);
            return;
        }
        for c in start..52 {
            combo[depth] = c;
            rec(c + 1, depth + 1, combo, perms, seen);
        }
    }
    rec(0, 0, &mut combo, &perms, &mut seen);
    seen.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(flop: &str, later: &str) -> Board {
        let parse = |s: &str| -> Vec<Card> {
            s.split_whitespace()
                .map(|c| c.parse::<Card>().unwrap())
                .collect()
        };
        Board::new(&parse(flop), &parse(later))
    }

    #[test]
    fn canonical_flop_count_is_1755() {
        let flops = canonical_flops();
        assert_eq!(flops.len(), 1755);
        assert_eq!(flops.iter().map(|(_, w)| *w).sum::<u32>(), 22_100);
        assert_eq!(canonical_unordered_board_count(3), 1755);
    }

    #[test]
    fn canonical_turn_count_is_16432() {
        assert_eq!(canonical_unordered_board_count(4), 16_432);
    }

    #[test]
    #[ignore = "expensive (2.6M boards x 24 perms); CI runs it in release"]
    fn canonical_river_count_is_134459() {
        assert_eq!(canonical_unordered_board_count(5), 134_459);
    }

    #[test]
    fn monotone_flop_merges_offsuit_turns() {
        // On Ks Qs Js the three non-spade suits are interchangeable:
        // 2c/2d/2h form one class, 2s its own.
        let b = board("Ks Qs Js", "");
        let groups = deal_groups(&b, &[]);
        let two_offsuit = groups
            .iter()
            .find(|g| g.members.contains(&"2c".parse().unwrap()))
            .unwrap();
        assert_eq!(two_offsuit.members.len(), 3);
        let two_spades = groups
            .iter()
            .find(|g| g.members.contains(&"2s".parse().unwrap()))
            .unwrap();
        assert_eq!(two_spades.members.len(), 1);
        // Total turn multiplicity must cover all 49 live cards.
        assert_eq!(groups.iter().map(|g| g.members.len()).sum::<usize>(), 49);
    }

    #[test]
    fn rainbow_flop_has_trivial_stabilizer_beyond_identity() {
        // Kc Qd Jh pins clubs/diamonds/hearts; only identity stabilizes
        // (spades has nowhere to go).
        let b = board("Kc Qd Jh", "");
        assert_eq!(stabilizer(&b).len(), 1);
        let groups = deal_groups(&b, &[]);
        assert!(groups.iter().all(|g| g.members.len() == 1));
    }

    #[test]
    fn paired_suits_merge() {
        // Kc Kd 2h: swapping clubs and diamonds stabilizes the board.
        let b = board("Kc Kd 2h", "");
        assert_eq!(stabilizer(&b).len(), 2);
        let groups = deal_groups(&b, &[]);
        let threes = groups
            .iter()
            .find(|g| g.members.contains(&"3c".parse().unwrap()))
            .unwrap();
        assert_eq!(
            threes.members,
            vec!["3c".parse().unwrap(), "3d".parse().unwrap()]
        );
    }

    #[test]
    fn canonicalization_is_idempotent_and_isomorphism_invariant() {
        let b = board("Ah Kh 7d", "2c");
        let (canon, _) = canonicalize_board(&b);
        assert_eq!(canonicalize_board(&canon).0, canon);
        for perm in all_suit_perms() {
            assert_eq!(canonicalize_board(&b.permuted(&perm)).0, canon);
        }
    }

    #[test]
    fn dead_cards_split_groups() {
        let b = board("Ks Qs Js", "");
        let dead: Vec<Card> = vec!["2c".parse().unwrap()];
        let groups = deal_groups(&b, &dead);
        assert_eq!(groups.iter().map(|g| g.members.len()).sum::<usize>(), 48);
        assert!(groups.iter().all(|g| !g.members.contains(&dead[0])));
        // 2d and 2h were in 2c's orbit; with 2c dead they are kept as
        // singletons rather than being merged unsoundly.
        let two_d = groups
            .iter()
            .find(|g| g.members.contains(&"2d".parse().unwrap()))
            .unwrap();
        assert_eq!(two_d.members.len(), 1);
    }
}
