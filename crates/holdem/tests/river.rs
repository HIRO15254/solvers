//! River-slice correctness: the sorted-rank showdown kernel against a naive
//! O(n^2) reference, and a full solve against the closed-form solution of
//! the clairvoyance (polarized-vs-bluffcatcher) game.

use cards::{Card, CardSet, Chips, NUM_COMBOS, PerPlayer, Player, Range, combo_cards, rank_of};
use engine::{Dcfr, F32Storage, Solver, TerminalEvaluator};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{RiverConfig, RiverGame, build_river_game};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn board(s: &str) -> [Card; 5] {
    let cards: Vec<Card> = s.split_whitespace().map(|c| c.parse().unwrap()).collect();
    cards.try_into().unwrap()
}

fn check_check_game(board_str: &str) -> RiverGame {
    build_river_game(
        &RiverConfig {
            board: board(board_str),
            ranges: PerPlayer::new(Range::full(), Range::full()),
            pot: Chips(2),
            effective_stack: Chips(100),
            bet_fractions: PerPlayer::new(vec![], vec![]),
            max_raises: 0,
        },
        chip_ev(),
    )
}

/// Naive reference: direct double loop over combos with fresh hand
/// evaluations, no prefix sums.
fn naive_showdown(board_cards: [Card; 5], p: Player, opp_reach: &[f32]) -> Vec<f32> {
    let board_set: CardSet = board_cards.iter().copied().collect();
    let live: Vec<usize> = (0..NUM_COMBOS)
        .filter(|&combo| {
            let (c1, c2) = combo_cards(combo);
            !board_set.contains(c1) && !board_set.contains(c2)
        })
        .collect();
    let rank = |combo: usize| {
        let (c1, c2) = combo_cards(combo);
        rank_of(board_cards.iter().copied().chain([c1, c2]))
    };
    let ranks: std::collections::HashMap<usize, cards::HandRank> =
        live.iter().map(|&c| (c, rank(c))).collect();
    // Check-check terminal of a pot-2 game: win +1, tie 0, lose -1 for
    // either player by symmetry.
    let mut out = vec![0.0f32; NUM_COMBOS];
    let _ = p;
    for &h in &live {
        let (h1, h2) = combo_cards(h);
        let mut v = 0.0f64;
        for &o in &live {
            if o == h {
                continue;
            }
            let (o1, o2) = combo_cards(o);
            if o1 == h1 || o1 == h2 || o2 == h1 || o2 == h2 {
                continue;
            }
            let r = opp_reach[o] as f64;
            if r == 0.0 {
                continue;
            }
            v += r * match ranks[&h].cmp(&ranks[&o]) {
                std::cmp::Ordering::Greater => 1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Less => -1.0,
            };
        }
        out[h] = v as f32;
    }
    out
}

fn pseudo_random_reach() -> Vec<f32> {
    (0..NUM_COMBOS)
        .map(|combo| ((combo.wrapping_mul(2654435761) >> 16) & 0xFF) as f32 / 255.0)
        .collect()
}

fn assert_kernel_matches_naive(board_str: &str) {
    let game = check_check_game(board_str);
    let board_cards = board(board_str);
    let board_set: CardSet = board_cards.iter().copied().collect();
    let mut reach = pseudo_random_reach();
    for (combo, w) in reach.iter_mut().enumerate() {
        let (c1, c2) = combo_cards(combo);
        if board_set.contains(c1) || board_set.contains(c2) {
            *w = 0.0;
        }
    }
    // The check-check showdown is the last terminal created; find it by
    // evaluating all and matching against any nonzero — instead, terminal 0
    // is the first terminal in build order, which for a no-bet tree is the
    // single check-check showdown.
    for p in Player::BOTH {
        let mut out = vec![0.0f32; NUM_COMBOS];
        game.game.evaluator.eval(0, p, &reach, &mut out);
        let expected = naive_showdown(board_cards, p, &reach);
        for combo in 0..NUM_COMBOS {
            assert!(
                (out[combo] - expected[combo]).abs() < 1e-3,
                "kernel mismatch on {board_str} combo {combo}: {} vs {}",
                out[combo],
                expected[combo]
            );
        }
    }
}

#[test]
fn showdown_kernel_matches_naive_mixed_board() {
    assert_kernel_matches_naive("2c 7d 9h Js Qs");
}

#[test]
fn showdown_kernel_matches_naive_tie_heavy_board() {
    // Broadway straight on board, no flush possible: almost everything ties.
    assert_kernel_matches_naive("Ah Kh Qd Jc Ts");
}

/// Clairvoyance game: P0 is perfectly polarized (AA nuts, 33 air, 50/50),
/// P1 holds pure bluffcatchers (QQ). Pot 2, stacks 2, pot-sized bet only.
/// Closed form: P0 bets all AA and half of 33, P1 calls half; P0's value
/// is +0.5 chips.
#[test]
#[ignore = "20k iterations is slow unoptimized; CI runs it in release"]
fn clairvoyance_game_matches_closed_form() {
    let config = RiverConfig {
        board: board("Ks Kh Kd 2c 2d"),
        ranges: PerPlayer::new("AA,33".parse().unwrap(), "QQ".parse().unwrap()),
        pot: Chips(2),
        effective_stack: Chips(2),
        bet_fractions: PerPlayer::new(vec![1.0], vec![]),
        max_raises: 1,
    };
    let game = build_river_game(&config, chip_ev());
    let root = game.node_by_history("").unwrap();
    let facing_bet = game.node_by_history("b2").unwrap();
    let node_info = game.node_info.clone();
    let tree_tags = game.game.tree.tags.clone();

    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(20_000));
    solver.run(20_000);

    let value = solver.expected_value(Player::P0);
    assert!(
        (value - 0.5).abs() < 0.02,
        "clairvoyance value should be +0.5, got {value}"
    );
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    assert!(nash_conv < 5e-3, "nash_conv = {nash_conv}");

    // Root: action 0 = check, action 1 = bet (see node_info actions).
    let info = &node_info[tree_tags[root as usize] as usize];
    assert_eq!(info.actions, vec!["check".to_string(), "bet 2".to_string()]);
    let sigma = solver.average_strategy_at(root);
    let bet_freq = |range: &str| -> f64 {
        let range: Range = range.parse().unwrap();
        let mut total = 0.0;
        let mut bet = 0.0;
        for combo in 0..NUM_COMBOS {
            let w = range.weight(combo) as f64;
            if w > 0.0 {
                total += w;
                bet += w * sigma[NUM_COMBOS + combo] as f64;
            }
        }
        bet / total
    };
    let aa = bet_freq("AA");
    let threes = bet_freq("33");
    assert!(aa > 0.97, "AA should always bet, got {aa}");
    assert!(
        (threes - 0.5).abs() < 0.05,
        "33 should bluff half the time, got {threes}"
    );

    // Facing the bet, QQ calls half the time.
    let info = &node_info[tree_tags[facing_bet as usize] as usize];
    assert_eq!(info.actions[0], "fold");
    assert_eq!(info.actions[1], "call");
    let sigma = solver.average_strategy_at(facing_bet);
    let qq: Range = "QQ".parse().unwrap();
    let mut total = 0.0;
    let mut call = 0.0;
    for combo in 0..NUM_COMBOS {
        let w = qq.weight(combo) as f64;
        if w > 0.0 {
            total += w;
            call += w * sigma[NUM_COMBOS + combo] as f64;
        }
    }
    let call_freq = call / total;
    assert!(
        (call_freq - 0.5).abs() < 0.05,
        "QQ should call half the time, got {call_freq}"
    );
}

#[test]
fn river_solve_is_zero_sum() {
    let config = RiverConfig {
        board: board("2c 7d 9h Js Qs"),
        ranges: PerPlayer::new(
            "22+,A2s+,KTo+".parse().unwrap(),
            "55-22,QJs,A5s-A2s,KQo,T9s".parse().unwrap(),
        ),
        pot: Chips(10),
        effective_stack: Chips(50),
        bet_fractions: PerPlayer::new(vec![0.5], vec![0.5]),
        max_raises: 2,
    };
    let game = build_river_game(&config, chip_ev());
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(200));
    solver.run(200);
    let sum = solver.expected_value(Player::P0) + solver.expected_value(Player::P1);
    assert!(sum.abs() < 1e-3, "zero-sum violated: {sum}");
    let expl = solver.exploitability();
    assert!(
        expl[Player::P0] > -1e-3 && expl[Player::P1] > -1e-3,
        "exploitability must be nonnegative: {expl:?}"
    );
}
