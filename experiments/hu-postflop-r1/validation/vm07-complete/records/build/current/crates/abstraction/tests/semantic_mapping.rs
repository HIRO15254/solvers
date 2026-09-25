//! Small executable contrasts for the R1 abstraction contract. These check
//! existing EHS2 mappings, not convergence or original-NLHE exploitability.

use std::collections::{BTreeMap, BTreeSet};

use abstraction::{CardAbstraction, Ehs2Abstraction, Ehs2Params};
use cards::{Card, NUM_COMBOS, Street, combo_cards, combo_index, rank_of};

fn board() -> Vec<Card> {
    "Ks Qs 7h 2d 3c"
        .split_whitespace()
        .map(|card| card.parse().unwrap())
        .collect()
}

fn build(board: &[Card], buckets: u32) -> Ehs2Abstraction {
    Ehs2Abstraction::build_for_boards(
        Ehs2Params {
            flop_buckets: buckets,
            turn_buckets: buckets,
            river_buckets: buckets,
        },
        &[board.to_vec()],
    )
}

fn live(board: &[Card], combo: usize) -> bool {
    let (a, b) = combo_cards(combo);
    !board.contains(&a) && !board.contains(&b)
}

#[test]
fn nested_river_buckets_lift_a_coarse_policy_but_are_not_lossless() {
    let board = board();
    let coarse = build(&board, 2);
    let fine = build(&board, 4);
    let mut parent = BTreeMap::new();
    let mut combos = 0;
    for combo in 0..NUM_COMBOS {
        if !live(&board, combo) {
            continue;
        }
        combos += 1;
        let coarse_id = coarse.bucket(&board, combo);
        let fine_id = fine.bucket(&board, combo);
        if let Some(previous) = parent.insert(fine_id, coarse_id) {
            assert_eq!(previous, coarse_id, "a fine bucket spans coarse buckets");
        }
    }
    assert_eq!(combos, 1081);
    assert_eq!(parent.len(), 4);
    assert_eq!(parent.values().copied().collect::<BTreeSet<_>>().len(), 2);

    // An explicit policy transport, not a claim that independently solved
    // coarse/fine policies coincide. In this finite game P0 chooses fold
    // (-1) or showdown (+1/0/-1), and P1 has no subsequent decision.
    let coarse_showdown_probability = [0.25, 0.75];
    let opponent = combo_index("Ac".parse().unwrap(), "Ad".parse().unwrap());
    let (o1, o2) = combo_cards(opponent);
    let opponent_rank = rank_of(board.iter().copied().chain([o1, o2]));
    let mut coarse_ev = 0.0;
    let mut lifted_ev = 0.0;
    let mut compatible_worlds = 0;
    let mut outcomes_by_bucket: BTreeMap<u32, BTreeSet<i8>> = BTreeMap::new();
    for combo in 0..NUM_COMBOS {
        let (a, b) = combo_cards(combo);
        if !live(&board, combo) || [a, b].iter().any(|c| *c == o1 || *c == o2) {
            continue;
        }
        compatible_worlds += 1;
        let outcome = match rank_of(board.iter().copied().chain([a, b])).cmp(&opponent_rank) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        let coarse_id = coarse.bucket(&board, combo);
        outcomes_by_bucket
            .entry(coarse_id)
            .or_default()
            .insert(outcome);
        let direct = coarse_showdown_probability[coarse_id as usize];
        let through_fine =
            coarse_showdown_probability[parent[&fine.bucket(&board, combo)] as usize];
        coarse_ev += direct * f64::from(outcome) - (1.0 - direct);
        lifted_ev += through_fine * f64::from(outcome) - (1.0 - through_fine);
    }
    assert_eq!(
        compatible_worlds, 990,
        "condition on both players' physical cards"
    );
    assert_eq!(coarse_ev / 990.0, lifted_ev / 990.0);
    assert!(
        outcomes_by_bucket
            .values()
            .any(|outcomes| outcomes.contains(&-1) && outcomes.contains(&1)),
        "same bucket can win or lose against the same opponent: this is a lossy mapping"
    );
}

#[test]
fn canonical_table_content_depends_on_coverage_not_only_bucket_counts() {
    let board = board();
    let original = build(&board, 4);
    let permuted: Vec<_> = board
        .iter()
        .map(|card| card.with_suit((card.suit() + 1) % 4))
        .collect();
    let same_domain = build(&permuted, 4);
    let different_board: Vec<Card> = "2c 3c 4c 5c 6c"
        .split_whitespace()
        .map(|card| card.parse().unwrap())
        .collect();
    let different_domain = build(&different_board, 4);
    assert_eq!(
        original.num_buckets(Street::River),
        different_domain.num_buckets(Street::River)
    );
    // Canonical table bytes are equal for a global suit rename and unequal
    // for different board coverage. This does not promote postcard bytes
    // to a version-independent semantic ID or prove game-level symmetry.
    assert_eq!(
        postcard::to_allocvec(&original).unwrap(),
        postcard::to_allocvec(&same_domain).unwrap()
    );
    assert_ne!(
        postcard::to_allocvec(&original).unwrap(),
        postcard::to_allocvec(&different_domain).unwrap()
    );
}
