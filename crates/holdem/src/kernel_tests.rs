//! Compare the compact kernels both with the historical accumulation order
//! and with an independent, quadratic hand-pair calculation.

use super::{
    RankEntry, fold_kernel, fold_kernel_compact, showdown_kernel, showdown_kernel_compact,
};
use crate::hands::PostflopHands;
use crate::mass::{MassWidth, classify_integer_mass, f64_mass_is_exact};
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
fn compact_and_global_kernels_agree_with_extreme_reach() {
    let board = board("2c 7d 9h Js Qs");
    let hands = PostflopHands::from_ranges(
        &board,
        &PerPlayer::new(
            "AsAh,AcAd,KsKh,AhKh".parse().unwrap(),
            "AsAh,AhKh,KdKc".parse().unwrap(),
        ),
    );
    let (showdown, fold) = tables(&hands, &board);
    for (low, expected_width) in [
        (2.0_f32.powi(-30), MassWidth::U64),
        (2.0_f32.powi(-70), MassWidth::U128),
        (f32::from_bits(1), MassWidth::Wide),
    ] {
        for player in Player::BOTH {
            let reach: Vec<_> = (0..hands.len(player.opponent()))
                .map(|i| match i % 3 {
                    0 => low,
                    1 => low * 2.0,
                    _ => 0.7,
                })
                .collect();
            let global_reach = hands.expand(player.opponent(), &reach);
            assert!(!f64_mass_is_exact(&reach));
            assert_eq!(classify_integer_mass(&reach).0, expected_width);
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
}

#[test]
fn tiny_legal_mass_survives_blocked_weights_for_each_outcome_and_both_seats() {
    let board = board("2c 7d 9h Js Qs");
    for (own, blocked_hand, legal_hand, outcome) in [
        ("AsAh", "AsKh", "KcKd", 0),
        ("AcAd", "AcKh", "AsAh", 1),
        ("8c8d", "Ac8c", "KcKd", 2),
    ] {
        for player in Player::BOTH {
            let opponent: Range = format!("{blocked_hand},{legal_hand}").parse().unwrap();
            let own: Range = own.parse().unwrap();
            let ranges = if player == Player::P0 {
                PerPlayer::new(own, opponent)
            } else {
                PerPlayer::new(opponent, own)
            };
            let hands = PostflopHands::from_ranges(&board, &ranges);
            let legal: Range = legal_hand.parse().unwrap();
            let (showdown, fold) = tables(&hands, &board);
            for tiny in [2.0_f32.powi(-30), 1e-20f32, f32::from_bits(1)] {
                let reach: Vec<_> = hands
                    .combos(player.opponent())
                    .iter()
                    .map(|&combo| {
                        if legal.weights()[combo as usize] > 0.0 {
                            tiny
                        } else {
                            1.0
                        }
                    })
                    .collect();
                let (masses, compatible) = pairwise_masses(&hands, player, &board, &reach);
                assert_eq!(compatible, vec![tiny as f64]);
                let mut actual = vec![0.0; hands.len(player)];
                for selected in 0..3 {
                    let mut utilities = [0.0; 3];
                    utilities[selected] = 1.0;
                    showdown_kernel_compact(
                        &compact_table(&showdown, &hands),
                        utilities,
                        player,
                        &reach,
                        &mut actual,
                    );
                    let expected = if selected == outcome { tiny } else { 0.0 };
                    assert_eq!(masses[0][selected] as f32, expected);
                    assert_eq!(actual[0].to_bits(), expected.to_bits());
                }
                fold_kernel_compact(
                    &compact_table(&fold, &hands),
                    1.0,
                    player,
                    &reach,
                    &mut actual,
                );
                assert_eq!(actual[0].to_bits(), tiny.to_bits());
                assert_eq!(
                    crate::compatible_reach(&hands, player, &reach),
                    vec![tiny as f64]
                );
            }
        }
    }
}

#[test]
fn losing_mass_is_subtracted_before_float_conversion() {
    let board = board("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new("KsKh".parse().unwrap(), "AcAd,TcTd".parse().unwrap());
    let hands = PostflopHands::from_ranges(&board, &ranges);
    let aces: Range = "AcAd".parse().unwrap();
    let (showdown, _) = tables(&hands, &board);
    for tiny in [2.0_f32.powi(-30), 1e-20f32, f32::from_bits(1)] {
        let reach: Vec<_> = hands
            .combos(Player::P1)
            .iter()
            .map(|&combo| {
                if aces.weights()[combo as usize] > 0.0 {
                    tiny
                } else {
                    1.0
                }
            })
            .collect();
        let mut out = vec![0.0; 1];
        showdown_kernel_compact(
            &compact_table(&showdown, &hands),
            [0.0, 0.0, 1.0],
            Player::P0,
            &reach,
            &mut out,
        );
        assert_eq!(out[0].to_bits(), tiny.to_bits());
    }
}
