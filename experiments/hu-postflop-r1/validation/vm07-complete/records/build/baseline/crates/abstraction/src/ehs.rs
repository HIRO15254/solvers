//! Hand-strength primitives: exact HS against a uniform opponent on a
//! complete board, and E[HS]/E[HS²] over river completions.
//!
//! All exact enumeration (no sampling): per river board the opponent's
//! 1,081 live combos are ranked once and swept in sorted order, so HS for
//! every hero combo on one board costs O(n log n) total, not O(n²).
//!
//! This module is the simple *reference* implementation — readable, exact,
//! but O(hero) per call. The bucket build in `buckets.rs` computes the same
//! quantities amortized across every hero at once (one rank sweep per river
//! board, not one per hero) and does not call these functions in its hot
//! loop.

use cards::{Card, CardSet, NUM_COMBOS, combo_cards, rank_of};

/// Hero's hand strength on a complete 5-card board against a uniform
/// random opponent: `(wins + ties / 2) / opponents`, where opponents are
/// the C(45, 2) combos disjoint from the board and the hero's hole cards.
///
/// Panics if `hole` or `board` overlap.
pub fn hand_strength(board: &[Card; 5], hole: (Card, Card)) -> f64 {
    let board_set: CardSet = board.iter().copied().collect();
    assert!(
        hole.0 != hole.1 && !board_set.contains(hole.0) && !board_set.contains(hole.1),
        "hand_strength: hole cards must be disjoint from each other and the board"
    );
    let hero_rank = rank_of(board.iter().copied().chain([hole.0, hole.1]));

    let mut dead = board_set;
    dead.insert(hole.0);
    dead.insert(hole.1);

    let mut wins = 0u32;
    let mut ties = 0u32;
    let mut opponents = 0u32;
    for combo in 0..NUM_COMBOS {
        let (a, b) = combo_cards(combo);
        if dead.contains(a) || dead.contains(b) {
            continue;
        }
        opponents += 1;
        let opp_rank = rank_of(board.iter().copied().chain([a, b]));
        match hero_rank.cmp(&opp_rank) {
            std::cmp::Ordering::Greater => wins += 1,
            std::cmp::Ordering::Equal => ties += 1,
            std::cmp::Ordering::Less => {}
        }
    }
    debug_assert_eq!(opponents, 990, "C(45, 2) live opponents");
    (wins as f64 + 0.5 * ties as f64) / opponents as f64
}

/// `(E[HS], E[HS²])` over all river completions of a 3-card (flop) or
/// 4-card (turn) board, HS as in [`hand_strength`]. For a 5-card board the
/// expectation is over the single completed board (HS, HS²).
pub fn ehs2(board: &[Card], hole: (Card, Card)) -> (f64, f64) {
    let board_set: CardSet = board.iter().copied().collect();
    assert!(
        hole.0 != hole.1 && !board_set.contains(hole.0) && !board_set.contains(hole.1),
        "ehs2: hole cards must be disjoint from each other and the board"
    );
    let mut dead = board_set;
    dead.insert(hole.0);
    dead.insert(hole.1);

    match board.len() {
        5 => {
            let board5: [Card; 5] = board.try_into().unwrap();
            let hs = hand_strength(&board5, hole);
            (hs, hs * hs)
        }
        4 => {
            let mut sum_hs = 0.0;
            let mut sum_hs2 = 0.0;
            let mut n = 0u32;
            for river in cards::ALL_CARDS {
                if dead.contains(river) {
                    continue;
                }
                let board5 = [board[0], board[1], board[2], board[3], river];
                let hs = hand_strength(&board5, hole);
                sum_hs += hs;
                sum_hs2 += hs * hs;
                n += 1;
            }
            debug_assert_eq!(n, 46, "52 - 4 board - 2 hole");
            (sum_hs / n as f64, sum_hs2 / n as f64)
        }
        3 => {
            let remaining: Vec<Card> = cards::ALL_CARDS
                .into_iter()
                .filter(|&c| !dead.contains(c))
                .collect();
            debug_assert_eq!(remaining.len(), 47, "52 - 3 board - 2 hole");
            let mut sum_hs = 0.0;
            let mut sum_hs2 = 0.0;
            let mut n = 0u32;
            for i in 0..remaining.len() {
                for j in (i + 1)..remaining.len() {
                    let board5 = [board[0], board[1], board[2], remaining[i], remaining[j]];
                    let hs = hand_strength(&board5, hole);
                    sum_hs += hs;
                    sum_hs2 += hs * hs;
                    n += 1;
                }
            }
            debug_assert_eq!(n, 1_081, "C(47, 2) turn+river completions");
            (sum_hs / n as f64, sum_hs2 / n as f64)
        }
        n => panic!("ehs2: board must be 3, 4, or 5 cards, got {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|c| c.parse().unwrap()).collect()
    }

    fn board5(s: &str) -> [Card; 5] {
        cards(s).try_into().unwrap()
    }

    fn hole(s: &str) -> (Card, Card) {
        let c = cards(s);
        (c[0], c[1])
    }

    #[test]
    fn royal_flush_is_the_strict_nuts() {
        // Board gives three hearts; hero's Jh Th completes the royal flush.
        // No other combo can tie (a second heart royal is impossible; no
        // other suit has 3+ board cards for a rival flush).
        let board = board5("Ah Kh Qh 2c 3d");
        let hs = hand_strength(&board, hole("Jh Th"));
        assert_eq!(hs, 1.0);
    }

    #[test]
    fn board_plays_for_every_live_combo() {
        // Ace-high straight (Broadway) with no suit repeated 3+ times: no
        // hero combo can make a flush, a higher straight (Ace is already
        // top), or a full house/quads (board is unpaired, only 2 hole
        // cards available). Every hero's best five is exactly the board's
        // straight, so every live combo ties every other: HS == 0.5.
        let board = board5("As Kh Qc Jd Th");
        for h in ["2c 3d", "2s 2h", "9c 8c", "4d 5c"] {
            let hs = hand_strength(&board, hole(h));
            assert_eq!(hs, 0.5, "hole {h}");
        }
    }

    #[test]
    fn ehs2_on_river_matches_hand_strength_exactly() {
        let board = cards("Ah Kh Qh 2c 3d");
        let h = hole("Jh Th");
        let hs = hand_strength(&board.clone().try_into().unwrap(), h);
        let (e_hs, e_hs2) = ehs2(&board, h);
        assert_eq!(e_hs, hs);
        assert_eq!(e_hs2, hs * hs);
    }

    #[test]
    fn ehs2_turn_matches_independent_brute_force_loop() {
        let board = cards("2c 7d Kh Jd");
        let h = hole("Ac Ad");
        let dead: CardSet = board.iter().copied().chain([h.0, h.1]).collect();
        // Independently written direct loop (not calling ehs2 internally).
        let mut sum_hs = 0.0;
        let mut sum_hs2 = 0.0;
        let mut n = 0u32;
        for river in cards::ALL_CARDS {
            if dead.contains(river) {
                continue;
            }
            let b5: [Card; 5] = [board[0], board[1], board[2], board[3], river];
            let hs = hand_strength(&b5, h);
            sum_hs += hs;
            sum_hs2 += hs * hs;
            n += 1;
        }
        let expected = (sum_hs / n as f64, sum_hs2 / n as f64);
        assert_eq!(ehs2(&board, h), expected);
    }

    #[test]
    fn ehs2_squared_expectation_never_exceeds_linear_expectation() {
        // x^2 <= x on [0, 1], so this must hold pointwise per completion
        // and hence for the average too.
        for (board_s, hole_s) in [
            ("2c 7d Kh", "Ac Ad"),
            ("Ks 7c 2d", "9h 8h"),
            ("2c 7d Kh Jd", "Ac Ad"),
        ] {
            let board = cards(board_s);
            let h = hole(hole_s);
            let (e_hs, e_hs2) = ehs2(&board, h);
            assert!(
                e_hs2 <= e_hs + 1e-12,
                "E[HS^2] = {e_hs2} > E[HS] = {e_hs} for {board_s} / {hole_s}"
            );
        }
    }

    #[test]
    fn aa_beats_72o_in_ehs2_on_a_dry_flop() {
        // Dry, disconnected, rainbow flop that shares no rank with 7 or 2
        // (a flop 7 or 2 would pair the "trash" hand into two pair, which
        // would defeat the point of the comparison).
        let board = cards("Ks 9c 4d");
        let (_, aa_hs2) = ehs2(&board, hole("Ah Ad"));
        let (_, trash_hs2) = ehs2(&board, hole("7h 2s")); // 72o, offsuit
        assert!(
            aa_hs2 > trash_hs2,
            "AA E[HS^2] = {aa_hs2} should exceed 72o E[HS^2] = {trash_hs2}"
        );
    }
}
