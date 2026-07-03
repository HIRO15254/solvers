//! Range-vs-range showdown equity, averaged over remaining runouts. Cold
//! path (display/reporting): built for correctness and reuse of the terminal
//! kernels in [`crate::kernel`], not for the solve loop.

use cards::{Card, CardSet, HandRank, NUM_COMBOS, PerPlayer, Player, combo_cards, rank_of};

use crate::kernel;

/// Every live combo's `(HandRank, combo)` pair for a completed 5-card
/// board, sorted ascending by rank — the same construction
/// `postflop::Builder::rank_table_id` uses for its showdown tables.
fn rank_table(board5: &[Card; 5]) -> Vec<(HandRank, u32)> {
    let board_set: CardSet = board5.iter().copied().collect();
    let mut sorted: Vec<(HandRank, u32)> = Vec::with_capacity(NUM_COMBOS);
    for combo in 0..NUM_COMBOS {
        let (c1, c2) = combo_cards(combo);
        if board_set.contains(c1) || board_set.contains(c2) {
            continue;
        }
        let rank = rank_of(board5.iter().copied().chain([c1, c2]));
        sorted.push((rank, combo as u32));
    }
    sorted.sort_unstable();
    sorted
}

/// Every unordered completion of `board` (3, 4, or 5 cards) to a full
/// 5-card board, as the missing cards drawn from the rest of the deck. Each
/// returned runout is equally likely.
fn runouts(board: &[Card]) -> Vec<Vec<Card>> {
    let board_set: CardSet = board.iter().copied().collect();
    let remaining: Vec<Card> = cards::ALL_CARDS
        .into_iter()
        .filter(|c| !board_set.contains(*c))
        .collect();
    match 5 - board.len() {
        0 => vec![Vec::new()],
        1 => remaining.iter().map(|&c| vec![c]).collect(),
        2 => {
            let mut out = Vec::with_capacity(remaining.len() * (remaining.len() - 1) / 2);
            for i in 0..remaining.len() {
                for j in (i + 1)..remaining.len() {
                    out.push(vec![remaining[i], remaining[j]]);
                }
            }
            out
        }
        _ => unreachable!("board must have 3, 4, or 5 cards"),
    }
}

/// Showdown equity of every combo in each player's range against the
/// other player's range, averaged uniformly over every remaining runout of
/// `board` (3, 4, or 5 cards).
///
/// For a hero combo `h`, `equity[h]` is
/// `sum(win + 0.5*tie) / sum(win + tie + lose)`, both sums taken over every
/// runout disjoint from `h`'s two cards and every opponent combo compatible
/// with both `h` and that runout, weighted by the opponent's range. Combos
/// with zero total compat across every runout (e.g. combos sharing a card
/// with `board`) get `0.0`.
pub fn range_equity(board: &[Card], ranges: &PerPlayer<Vec<f32>>) -> PerPlayer<Vec<f32>> {
    assert!(
        (3..=5).contains(&board.len()),
        "board must have 3 (flop), 4 (turn), or 5 (river) cards"
    );
    assert_eq!(ranges[Player::P0].len(), NUM_COMBOS);
    assert_eq!(ranges[Player::P1].len(), NUM_COMBOS);

    let mut numerator = PerPlayer::new(vec![0.0f64; NUM_COMBOS], vec![0.0f64; NUM_COMBOS]);
    let mut denominator = PerPlayer::new(vec![0.0f64; NUM_COMBOS], vec![0.0f64; NUM_COMBOS]);
    let mut num_out = vec![0.0f32; NUM_COMBOS];
    let mut denom_out = vec![0.0f32; NUM_COMBOS];

    for missing in runouts(board) {
        let mut board5 = [board[0]; 5];
        board5[..board.len()].copy_from_slice(board);
        board5[board.len()..].copy_from_slice(&missing);
        let sorted = rank_table(&board5);

        for p in Player::BOTH {
            let opp = p.opponent();
            num_out.fill(0.0);
            kernel::showdown_kernel(&sorted, 1.0, 0.5, 0.0, &ranges[opp], &mut num_out);
            denom_out.fill(0.0);
            kernel::fold_kernel(&sorted, 1.0, &ranges[opp], &mut denom_out);
            for combo in 0..NUM_COMBOS {
                numerator[p][combo] += num_out[combo] as f64;
                denominator[p][combo] += denom_out[combo] as f64;
            }
        }
    }

    PerPlayer::new(
        (0..NUM_COMBOS)
            .map(|combo| {
                let d = denominator[Player::P0][combo];
                if d != 0.0 {
                    (numerator[Player::P0][combo] / d) as f32
                } else {
                    0.0
                }
            })
            .collect(),
        (0..NUM_COMBOS)
            .map(|combo| {
                let d = denominator[Player::P1][combo];
                if d != 0.0 {
                    (numerator[Player::P1][combo] / d) as f32
                } else {
                    0.0
                }
            })
            .collect(),
    )
}
