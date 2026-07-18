//! Later streets may be abstracted arbitrarily coarsely: for a
//! preflop-focused blueprint the river's fidelity barely feeds back into
//! preflop strategy, so extreme bucket counts (down to k = 1) must degrade
//! gracefully, never panic or emit out-of-range buckets.

use abstraction::{CardAbstraction, Ehs2Abstraction, Ehs2Params};
use cards::{Card, NUM_COMBOS, Street, combo_cards};

fn boards() -> Vec<Vec<Card>> {
    let parse =
        |s: &str| -> Vec<Card> { s.split_whitespace().map(|c| c.parse().unwrap()).collect() };
    vec![
        parse("2c 7d Kh"),
        parse("As Ks Qs"),
        parse("2c 7d Kh Jd"),
        parse("2c 7d Kh Jd 3s"),
        parse("As Ks Qs Js Ts"),
    ]
}

#[test]
fn extreme_small_bucket_counts_degrade_gracefully() {
    for (kf, kt, kr) in [(1, 1, 1), (2, 1, 1), (4, 2, 1), (8, 4, 2)] {
        let params = Ehs2Params {
            flop_buckets: kf,
            turn_buckets: kt,
            river_buckets: kr,
        };
        let abs = Ehs2Abstraction::build_for_boards(params, &boards());
        for board in boards() {
            let street = match board.len() {
                3 => Street::Flop,
                4 => Street::Turn,
                _ => Street::River,
            };
            let k = abs.num_buckets(street);
            assert!(k >= 1);
            let dead: cards::CardSet = board.iter().copied().collect();
            for combo in 0..NUM_COMBOS {
                let (a, b) = combo_cards(combo);
                if dead.contains(a) || dead.contains(b) {
                    continue;
                }
                let bucket = abs.bucket(&board, combo);
                assert!(
                    bucket < k,
                    "bucket {bucket} out of range for k={k} on {board:?} (params {kf}/{kt}/{kr})"
                );
            }
        }
    }
}

/// k = 1 collapses every hand into one bucket: the whole street carries no
/// hand information, which is exactly the "maximally extreme" end a
/// preflop-focused config may choose for the river.
#[test]
fn k1_river_is_a_single_information_free_bucket() {
    let params = Ehs2Params {
        flop_buckets: 4,
        turn_buckets: 2,
        river_buckets: 1,
    };
    let abs = Ehs2Abstraction::build_for_boards(params, &boards());
    let river: Vec<Card> = "2c 7d Kh Jd 3s"
        .split_whitespace()
        .map(|c| c.parse().unwrap())
        .collect();
    let dead: cards::CardSet = river.iter().copied().collect();
    for combo in 0..NUM_COMBOS {
        let (a, b) = combo_cards(combo);
        if dead.contains(a) || dead.contains(b) {
            continue;
        }
        assert_eq!(abs.bucket(&river, combo), 0);
    }
}
