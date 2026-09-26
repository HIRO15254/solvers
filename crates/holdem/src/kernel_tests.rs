//! Compare the compact kernels both with the historical accumulation order
//! and with an independent, quadratic hand-pair calculation.

use super::{
    RankEntry, fold_kernel, fold_kernel_compact, showdown_kernel, showdown_kernel_compact,
};
use crate::hands::PostflopHands;
use cards::{Card, HandRank, NUM_COMBOS, PerPlayer, Player, Range, combo_cards, rank_of};

type SortedRanks = Vec<(HandRank, u32)>;

fn board(text: &str) -> Vec<Card> {
    text.split_whitespace()
        .map(|card| card.parse().unwrap())
        .collect()
}

fn blocked(combo: usize, board: &[Card]) -> bool {
    let (a, b) = combo_cards(combo);
    board.contains(&a) || board.contains(&b)
}

fn tables(hands: &PostflopHands, board: &[Card]) -> (SortedRanks, SortedRanks) {
    let union: Vec<_> = (0..NUM_COMBOS)
        .filter(|&combo| {
            Player::BOTH
                .into_iter()
                .any(|p| hands.index(p, combo).is_some())
        })
        .collect();
    let fold = union
        .iter()
        .map(|&combo| (HandRank(0), combo as u32))
        .collect();
    let mut showdown: Vec<_> = union
        .into_iter()
        .filter(|&combo| !blocked(combo, board))
        .map(|combo| {
            let (a, b) = combo_cards(combo);
            (rank_of(board.iter().copied().chain([a, b])), combo as u32)
        })
        .collect();
    showdown.sort_unstable();
    (showdown, fold)
}

fn compact_table(sorted: &[(HandRank, u32)], hands: &PostflopHands) -> Vec<RankEntry> {
    sorted
        .iter()
        .map(|&(rank, combo)| RankEntry::new(rank, combo, hands))
        .collect()
}

// Precompute only physical cards and individual strengths. The reference
// below does not use the production sorted table or inclusion-exclusion.
fn pairwise_masses(
    hands: &PostflopHands,
    player: Player,
    board: &[Card],
    reach: &[f32],
) -> (Vec<[f64; 3]>, Vec<f64>) {
    let describe = |p| {
        hands
            .combos(p)
            .iter()
            .map(|&combo| {
                let (a, b) = combo_cards(combo as usize);
                let live = !board.contains(&a) && !board.contains(&b);
                let rank = live.then(|| rank_of(board.iter().copied().chain([a, b])));
                (a, b, rank)
            })
            .collect::<Vec<_>>()
    };
    let own = describe(player);
    let opponents = describe(player.opponent());
    let mut outcomes = vec![[0.0; 3]; own.len()];
    let mut compatible = vec![0.0; own.len()];
    for (i, &(a, b, own_rank)) in own.iter().enumerate() {
        for (&(c, d, opp_rank), &weight) in opponents.iter().zip(reach) {
            if weight == 0.0 || a == c || a == d || b == c || b == d {
                continue;
            }
            // A later public card must already have masked opponent reach.
            assert!(opp_rank.is_some());
            compatible[i] += weight as f64;
            if let Some(own_rank) = own_rank {
                let outcome = match own_rank.cmp(&opp_rank.unwrap()) {
                    std::cmp::Ordering::Greater => 0,
                    std::cmp::Ordering::Equal => 1,
                    std::cmp::Ordering::Less => 2,
                };
                outcomes[i][outcome] += weight as f64;
            }
        }
    }
    (outcomes, compatible)
}

fn assert_bits(actual: &[f32], full: &[f32], hands: &PostflopHands, player: Player) {
    for (local, &combo) in hands.combos(player).iter().enumerate() {
        assert_eq!(
            actual[local].to_bits(),
            full[combo as usize].to_bits(),
            "player={player:?}, local={local}, global={combo}"
        );
    }
}

fn compare(root_board: &[Card], final_board: &[Card], ranges: PerPlayer<Range>) {
    let hands = PostflopHands::from_ranges(root_board, &ranges);
    let (showdown, fold) = tables(&hands, final_board);
    let compact_showdown = compact_table(&showdown, &hands);
    let compact_fold = compact_table(&fold, &hands);
    for player in Player::BOTH {
        // Non-dyadic, sparse, all-zero, and a single supported opponent hand.
        for pattern in 0..4 {
            let reach: Vec<_> = hands
                .combos(player.opponent())
                .iter()
                .enumerate()
                .map(|(local, &combo)| {
                    if blocked(combo as usize, final_board)
                        || pattern == 2
                        || (pattern == 1 && local.is_multiple_of(3))
                        || (pattern == 3 && local != 0)
                    {
                        0.0
                    } else {
                        ((combo as usize * 17 + 13) % 997 + 1) as f32 / 997.0
                    }
                })
                .collect();
            let global_reach = hands.expand(player.opponent(), &reach);
            let (masses, compatible) = pairwise_masses(&hands, player, final_board, &reach);
            // A zero tie utility can hide matching mistakes in tie and total
            // self-combo correction. Test all three outcome masses separately.
            for utilities in [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [2.25, -0.75, -4.125],
            ] {
                // eval, not the kernel, owns output clearing. In particular,
                // board-dead own hands are absent from this showdown table.
                let mut actual = vec![0.0; hands.len(player)];
                let mut old = vec![0.0; NUM_COMBOS];
                showdown_kernel_compact(&compact_showdown, utilities, player, &reach, &mut actual);
                showdown_kernel(
                    &showdown,
                    utilities[0],
                    utilities[1],
                    utilities[2],
                    &global_reach,
                    &mut old,
                );
                assert_bits(&actual, &old, &hands, player);
                for (local, (&value, mass)) in actual.iter().zip(&masses).enumerate() {
                    let expected = (utilities[0] * mass[0]
                        + utilities[1] * mass[1]
                        + utilities[2] * mass[2]) as f32;
                    assert!(
                        (value - expected).abs() < 1e-3,
                        "showdown player={player:?} pattern={pattern} local={local} utilities={utilities:?}: {value} != {expected}"
                    );
                    if blocked(hands.combo(player, local), final_board) {
                        assert_eq!(value.to_bits(), 0.0f32.to_bits());
                    }
                }
            }
            for utility in [1.0, -2.75, 0.0] {
                let mut actual = vec![0.0; hands.len(player)];
                let mut old = vec![0.0; NUM_COMBOS];
                fold_kernel_compact(&compact_fold, utility, player, &reach, &mut actual);
                fold_kernel(&fold, utility, &global_reach, &mut old);
                assert_bits(&actual, &old, &hands, player);
                for (&value, &mass) in actual.iter().zip(&compatible) {
                    assert!((value - (utility * mass) as f32).abs() < 1e-3);
                }
            }
        }
    }
}

#[test]
fn compact_kernel_rank_entry_stays_eight_bytes() {
    assert_eq!(std::mem::size_of::<RankEntry>(), 8);
}

#[test]
fn compact_kernels_match_old_bits_and_pairwise_on_asymmetric_overlapping_support() {
    let board = board("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AsAh,AcAd,KsKh,TcTd,AhKh,2c3c".parse().unwrap(),
        "AsAh,AhKh,KdKc,TsTh,7d8d".parse().unwrap(),
    );
    let hands = PostflopHands::from_ranges(&board, &ranges);
    assert_eq!((hands.len(Player::P0), hands.len(Player::P1)), (5, 4));
    compare(&board, &board, ranges);
}

#[test]
fn compact_kernels_match_old_bits_and_pairwise_with_full_support() {
    for text in ["2c 7d 9h Js Qs", "Ah Kh Qd Jc Ts"] {
        let board = board(text);
        compare(&board, &board, PerPlayer::new(Range::full(), Range::full()));
    }
}

#[test]
fn compact_kernels_remove_identical_and_shared_card_only_opponents() {
    let board = board("2c 7d 9h Js Qs");
    // There is no compatible hand pair. This isolates the own-combo
    // add-back and one-card exclusion from contributions by other hands.
    compare(
        &board,
        &board,
        PerPlayer::new("AsAh".parse().unwrap(), "AsAh,AhKh".parse().unwrap()),
    );
}

#[test]
fn compact_kernels_match_old_bits_and_pairwise_after_public_card_masks() {
    let root = board("2c 7d 9h");
    let final_board = board("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AsAh,JsJh,QsQh,TcTd,AcAd".parse().unwrap(),
        "AsAh,AhKh,JsTs,QsKs".parse().unwrap(),
    );
    let hands = PostflopHands::from_ranges(&root, &ranges);
    for player in Player::BOTH {
        assert!(
            hands
                .combos(player)
                .iter()
                .any(|&combo| blocked(combo as usize, &final_board))
        );
        assert!(
            hands
                .combos(player)
                .iter()
                .any(|&combo| !blocked(combo as usize, &final_board))
        );
    }
    compare(&root, &final_board, ranges);
}

#[test]
fn compact_kernels_preserve_historical_tiny_reach_rounding_bits() {
    // This is intentionally a compatibility test, not a claim that the old
    // inclusion-exclusion avoids cancellation at extreme weight ratios.
    let board = board("2c 7d 9h Js Qs");
    let hands = PostflopHands::from_ranges(
        &board,
        &PerPlayer::new(
            "AsAh,AcAd,KsKh,AhKh".parse().unwrap(),
            "AsAh,AhKh,KdKc".parse().unwrap(),
        ),
    );
    let (showdown, fold) = tables(&hands, &board);
    for player in Player::BOTH {
        let reach: Vec<_> = (0..hands.len(player.opponent()))
            .map(|i| match i % 3 {
                0 => f32::MIN_POSITIVE,
                1 => f32::from_bits(1),
                _ => 0.7,
            })
            .collect();
        let global_reach = hands.expand(player.opponent(), &reach);
        let mut actual = vec![0.0; hands.len(player)];
        let mut old = vec![0.0; NUM_COMBOS];
        let utilities = [2.25, -0.75, -4.125];
        showdown_kernel_compact(
            &compact_table(&showdown, &hands),
            utilities,
            player,
            &reach,
            &mut actual,
        );
        showdown_kernel(
            &showdown,
            utilities[0],
            utilities[1],
            utilities[2],
            &global_reach,
            &mut old,
        );
        assert_bits(&actual, &old, &hands, player);
        actual.fill(0.0);
        old.fill(0.0);
        fold_kernel_compact(
            &compact_table(&fold, &hands),
            -2.75,
            player,
            &reach,
            &mut actual,
        );
        fold_kernel(&fold, -2.75, &global_reach, &mut old);
        assert_bits(&actual, &old, &hands, player);
    }
}
