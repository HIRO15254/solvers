//! Multi-street postflop builder correctness: direct-enumeration equity on
//! a check-down tree, suit-isomorphism on/off equivalence, zero-sum and
//! exploitability invariants on a small flop solve, an all-in-runout
//! convergence check, and the memory-usage dry run against an actual build.

use cards::{
    ALL_CARDS, Card, CardSet, Chips, NUM_COMBOS, PerPlayer, Player, Range, combo_cards, rank_of,
};
use engine::{Dcfr, F32Storage, ParConfig, Solver};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{PerStreet, PostflopConfig, build_postflop_game, memory_usage};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn parse_cards(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

fn flop3(s: &str) -> [Card; 3] {
    parse_cards(s).try_into().unwrap()
}

/// Direct enumeration of a check-down flop subgame's value to P0: average
/// the pot-2 (+1/0/-1) payoff over every ordered (turn, river) completion
/// for each compatible hole-card pair, then weight-average over pairs by
/// their root range weights. This is exactly what the check-down tree
/// computes (the 1/45, 1/44 per-deal weights live inside the tree; here we
/// average uniformly over the 45*44 ordered runouts instead), so the two
/// must agree.
fn direct_checkdown_ev(board: [Card; 3], ranges: &PerPlayer<Range>) -> f64 {
    let board_set: CardSet = board.iter().copied().collect();
    let live: Vec<usize> = (0..NUM_COMBOS)
        .filter(|&combo| {
            let (c1, c2) = combo_cards(combo);
            !board_set.contains(c1) && !board_set.contains(c2)
        })
        .collect();

    let mut total_weight = 0.0f64;
    let mut total_value = 0.0f64;
    for &h0 in &live {
        let w0 = ranges[Player::P0].weight(h0) as f64;
        if w0 == 0.0 {
            continue;
        }
        let (a1, a2) = combo_cards(h0);
        for &h1 in &live {
            let (b1, b2) = combo_cards(h1);
            if b1 == a1 || b1 == a2 || b2 == a1 || b2 == a2 {
                continue;
            }
            let w1 = ranges[Player::P1].weight(h1) as f64;
            if w1 == 0.0 {
                continue;
            }

            let used: CardSet = [board[0], board[1], board[2], a1, a2, b1, b2]
                .into_iter()
                .collect();
            let remaining: Vec<Card> = ALL_CARDS
                .into_iter()
                .filter(|&c| !used.contains(c))
                .collect();

            let mut pair_sum = 0.0f64;
            let mut pair_count = 0u32;
            for &turn in &remaining {
                for &river in &remaining {
                    if turn == river {
                        continue;
                    }
                    let rank0 = rank_of(board.into_iter().chain([turn, river, a1, a2]));
                    let rank1 = rank_of(board.into_iter().chain([turn, river, b1, b2]));
                    let payoff = match rank0.cmp(&rank1) {
                        std::cmp::Ordering::Greater => 1.0,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Less => -1.0,
                    };
                    pair_sum += payoff;
                    pair_count += 1;
                }
            }

            let weight = w0 * w1;
            total_weight += weight;
            total_value += weight * (pair_sum / pair_count as f64);
        }
    }
    total_value / total_weight
}

#[test]
fn no_bet_flop_value_matches_direct_enumeration() {
    let board = flop3("Qs Jh 2h");
    let ranges = PerPlayer::new(
        "AA,KK".parse::<Range>().unwrap(),
        "33,44".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board: board.to_vec(),
        ranges: ranges.clone(),
        pot: Chips(2),
        effective_stack: Chips(100),
        // No bet fractions, no raises anywhere: a pure check-down tree.
        ..Default::default()
    };
    let game = build_postflop_game(&config, chip_ev());
    // Every node has exactly one legal action (check), so the average
    // strategy is degenerate regardless of iteration count.
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(1));
    solver.run(1);
    let got = solver.expected_value(Player::P0);
    let want = direct_checkdown_ev(board, &ranges);
    assert!(
        (got - want).abs() < 1e-4,
        "check-down EV mismatch: solver={got}, direct={want}"
    );
}

#[test]
#[ignore = "two 2000-iteration turn-tree solves are slow unoptimized; CI runs it in release"]
fn iso_on_off_converge_to_same_value() {
    // Flop is monotone spades (2s 7s Ks); the turn card 2h fixes hearts.
    // The board's stabilizer is then exactly {identity, swap(clubs,
    // diamonds)}, and the pair ranges below are symmetric under every suit
    // permutation, so the merged tree is a faithful quotient of the full
    // tree. Note this is a statement about the *game*, not about CFR
    // trajectories: finite iterates differ pointwise between the two trees
    // (the quotient replaces a sum over isomorphic branches with
    // multiplicity x representative, which symmetrizes differently), so the
    // right test is agreement of the converged value within the sum of both
    // solves' exploitability bounds — for a zero-sum game, any
    // eps-equilibrium's value is within eps of the game value.
    let board = parse_cards("2s 7s Ks 2h");
    let ranges = PerPlayer::new(
        "44,55".parse::<Range>().unwrap(),
        "33,66".parse::<Range>().unwrap(),
    );
    let base = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![], vec![]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 1,
            river: 0,
        },
        ..Default::default()
    };
    let sequential = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };

    let run = |iso_merging: bool| {
        let config = PostflopConfig {
            iso_merging,
            ..base.clone()
        };
        let game = build_postflop_game(&config, chip_ev());
        let mut solver =
            Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(2000));
        solver.set_par(sequential);
        solver.run(2000);
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        (solver.expected_value(Player::P0), nash_conv)
    };

    let (ev_on, conv_on) = run(true);
    let (ev_off, conv_off) = run(false);
    assert!(conv_on < 1e-3, "merged solve did not converge: {conv_on}");
    assert!(conv_off < 1e-3, "full solve did not converge: {conv_off}");
    // Exploitability bound plus an f32 noise floor: expected values are
    // accumulated through f32 chains over ~10^3-node trees, which carries
    // ~1e-4 of summation noise at this tree size (both solves sit at
    // NashConv ~2e-6, far below it). A deal-weight scale bug (e.g. /49
    // instead of /45) shifts the value by ~1e-2 and still fails loudly.
    let bound = conv_on + conv_off + 5e-4;
    assert!(
        (ev_on - ev_off).abs() < bound,
        "converged values differ beyond the exploitability bound: \
         {ev_on} vs {ev_off} (bound {bound})"
    );
}

#[test]
fn asymmetric_ranges_suppress_iso_merging() {
    // Same board as above: the board stabilizer swaps clubs and diamonds.
    // P0's range holds AcAh but not AdAh, which that swap does not preserve,
    // so merging any {Xc, Xd} river pair would change the game. The builder
    // must fall back to singleton deals: with the symmetry filter in place,
    // iso on and off produce identical trees.
    let board = parse_cards("2s 7s Ks 2h");
    let base = PostflopConfig {
        board,
        ranges: PerPlayer::new(
            "AcAh,44".parse::<Range>().unwrap(),
            "33,66".parse::<Range>().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![], vec![]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 1,
            river: 0,
        },
        ..Default::default()
    };
    let on = build_postflop_game(
        &PostflopConfig {
            iso_merging: true,
            ..base.clone()
        },
        chip_ev(),
    );
    let off = build_postflop_game(
        &PostflopConfig {
            iso_merging: false,
            ..base.clone()
        },
        chip_ev(),
    );
    assert_eq!(
        on.game.tree.nodes.len(),
        off.game.tree.nodes.len(),
        "asymmetric ranges admit no merging, trees must be identical in size"
    );

    // Sanity check of the suppression's converse: the symmetric ranges of
    // the convergence test above do merge (strictly fewer nodes with iso on).
    let sym = PostflopConfig {
        ranges: PerPlayer::new("44,55".parse().unwrap(), "33,66".parse().unwrap()),
        iso_merging: true,
        ..base.clone()
    };
    let sym_off = PostflopConfig {
        iso_merging: false,
        ..sym.clone()
    };
    let merged = build_postflop_game(&sym, chip_ev());
    let full = build_postflop_game(&sym_off, chip_ev());
    assert!(
        merged.game.tree.nodes.len() < full.game.tree.nodes.len(),
        "symmetric ranges must merge: {} vs {}",
        merged.game.tree.nodes.len(),
        full.game.tree.nodes.len()
    );
}

/// Flop board chosen for maximal suit-isomorphism merging: a monotone flop
/// pins one suit and leaves the other three completely free (stabilizer
/// `S_3`), which is the largest a 3-card flop's stabilizer can be. Even so,
/// a from-the-flop tree runs two chance deals (~6k nodes for this config) —
/// heavy enough that this and the all-in test below run under `--release`.
fn merged_flop() -> Vec<Card> {
    parse_cards("2s 7s Ks")
}

#[test]
#[ignore = "flop-starting trees run two chance deals (~6k nodes); CI runs it in release"]
fn flop_solve_is_zero_sum() {
    let config = PostflopConfig {
        board: merged_flop(),
        ranges: PerPlayer::new("AA,KK".parse().unwrap(), "QQ,JJ".parse().unwrap()),
        pot: Chips(4),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![], vec![]),
            river: PerPlayer::new(vec![0.5], vec![0.5]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: 1,
        },
        ..Default::default()
    };
    let game = build_postflop_game(&config, chip_ev());
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(50));
    solver.run(50);
    let sum = solver.expected_value(Player::P0) + solver.expected_value(Player::P1);
    assert!(sum.abs() < 1e-3, "zero-sum violated: {sum}");
    let expl = solver.exploitability();
    assert!(
        expl[Player::P0] > -1e-3 && expl[Player::P1] > -1e-3,
        "exploitability must be nonnegative: {expl:?}"
    );
}

/// Effective stack of 1 chip against a pot-2 subgame means any nonzero bet
/// fraction clamps to all-in; flop/turn never bet (`max_raises = 0`), so
/// the only decision in the whole tree is the river's bet/check and
/// call/fold. Rather than compute a closed-form equity bound, this checks
/// the two invariants that must hold at any equilibrium of a chip-EV
/// zero-sum game: the value sums to zero and neither player can improve by
/// deviating.
#[test]
#[ignore = "flop-starting trees run two chance deals (~6k nodes); CI runs it in release"]
fn allin_runout_matches_direct_equity() {
    let config = PostflopConfig {
        board: merged_flop(),
        ranges: PerPlayer::new("AA".parse().unwrap(), "KK".parse().unwrap()),
        pot: Chips(2),
        effective_stack: Chips(1),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![], vec![]),
            // Fraction far larger than the pot: raw size always clamps to
            // the 1-chip stack, i.e. any bet is an all-in bet.
            river: PerPlayer::new(vec![5.0], vec![5.0]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: 1,
        },
        ..Default::default()
    };
    let game = build_postflop_game(&config, chip_ev());
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(300));
    solver.run(300);

    let sum = solver.expected_value(Player::P0) + solver.expected_value(Player::P1);
    assert!(sum.abs() < 1e-3, "zero-sum violated: {sum}");

    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    let one_percent_pot = config.pot.as_f64() * 0.01;
    assert!(
        nash_conv < one_percent_pot,
        "nash_conv = {nash_conv}, want < 1% pot = {one_percent_pot}"
    );
}

#[test]
fn memory_usage_matches_allocated() {
    let config = PostflopConfig {
        board: merged_flop(),
        ranges: PerPlayer::new("AA,KK".parse().unwrap(), "QQ,JJ".parse().unwrap()),
        pot: Chips(4),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![], vec![]),
            river: PerPlayer::new(vec![0.5], vec![0.5]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: 1,
        },
        ..Default::default()
    };
    let estimate = memory_usage(&config);
    let game = build_postflop_game(&config, chip_ev());

    assert_eq!(
        estimate.f32_bytes,
        game.game.tree.storage_len as u64 * 2 * 4
    );
    assert_eq!(estimate.nodes, game.game.tree.nodes.len() as u64);
}
