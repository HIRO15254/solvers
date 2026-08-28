//! Multi-street postflop builder correctness: direct-enumeration equity on
//! a check-down tree, suit-isomorphism on/off equivalence, zero-sum and
//! exploitability invariants on a small flop solve, an all-in-runout
//! convergence check, and the memory-usage dry run against an actual build.

use std::time::Instant;

use cards::{
    ALL_CARDS, Card, CardSet, Chips, NUM_COMBOS, PerPlayer, Player, Range, SizeSpec, combo_cards,
    rank_of,
};
use engine::{Dcfr, F32Storage, I16Storage, NodeKind, ParConfig, Solver};
use game::{ChipEv, NoRake, PayoffPipeline};
use holdem::{PerStreet, PostflopConfig, StreetTree, build_postflop_game, memory_usage};
use rayon::prelude::*;

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

/// A street whose bet menu and raise menu use different pot fractions —
/// `StreetTree::pot_fractions` assumes they're shared, so the tests
/// exercising `oop_raise`/`ip_raise` independently of `oop_bet`/`ip_bet`
/// build the `StreetTree` by hand instead.
fn manual_street(
    oop_bet: &[f64],
    ip_bet: &[f64],
    oop_raise: &[f64],
    ip_raise: &[f64],
    max_aggressive_actions: u32,
) -> StreetTree {
    let sizes = |fractions: &[f64]| -> Vec<SizeSpec> {
        fractions
            .iter()
            .map(|&fraction| SizeSpec::PotAfterCall { fraction })
            .collect()
    };
    let raise_levels = |fractions: &[f64]| -> Vec<Vec<SizeSpec>> {
        if fractions.is_empty() {
            Vec::new()
        } else {
            vec![sizes(fractions)]
        }
    };
    StreetTree {
        oop_bet: sizes(oop_bet),
        ip_bet: sizes(ip_bet),
        oop_raise: Some(raise_levels(oop_raise)),
        ip_raise: Some(raise_levels(ip_raise)),
        max_aggressive_actions,
        ..Default::default()
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

/// Same reference computation as [`direct_checkdown_ev`] — average the
/// pot-2 (+1/0/-1) payoff over every ordered (turn, river) completion for
/// each compatible hole-card pair, weight-average over pairs — but
/// restructured for full-range scale, where `direct_checkdown_ev`'s
/// per-pair-then-per-runout loop (1,326^2 pairs x 990 unordered runouts,
/// each running its own pair of `rank_of` calls) is far too slow.
///
/// Instead this enumerates the 49*48/2 = 1,176 unordered (turn, river)
/// completions once. For each, it ranks every live combo a single time
/// (~1,176 x 1,326 `rank_of` calls total — the only calls in the hot path)
/// into a `rank_by_combo` table plus a `live` bitmap (combos overlapping the
/// runout's 5 cards are marked dead rather than relying on any particular
/// rank value as a sentinel). It then folds `sign(rank0 vs rank1)` for every
/// live *pair* into a shared 1,326x1,326 `f32` accumulator (~7 MB), weight 2
/// per unordered runout (standing for both turn/river orderings).
///
/// A pair's accumulated total, divided by 45*44 = 1,980 (the ordered runout
/// count avoiding that pair's 4 cards — not 49*48, since both hole-card
/// pairs are already fixed), is exactly that pair's average payoff over its
/// compatible runouts, matching `direct_checkdown_ev`'s inner average. Pairs
/// that share a card with each other, or with the flop, never accumulate
/// (skipped by the live check or the flop-conflict filter below) and are
/// excluded from the weighted average exactly as the root range zeroing
/// excludes them from the tree.
///
/// Runs over `rayon` chunks (~2.3G table lookups total across the pairwise
/// fold) to stay within a `--release` test's time budget.
fn direct_checkdown_ev_full_range(board: [Card; 3], ranges: &PerPlayer<Range>) -> f64 {
    let board_set: CardSet = board.iter().copied().collect();
    let remaining: Vec<Card> = ALL_CARDS
        .into_iter()
        .filter(|&c| !board_set.contains(c))
        .collect();
    assert_eq!(remaining.len(), 49);

    // Combo -> card pair, precomputed once so the pairwise fold below never
    // re-derives it from a combo index.
    let combo_pairs: Vec<(Card, Card)> = (0..NUM_COMBOS).map(combo_cards).collect();

    let runouts: Vec<(usize, usize)> = (0..remaining.len())
        .flat_map(|i| ((i + 1)..remaining.len()).map(move |j| (i, j)))
        .collect();
    assert_eq!(runouts.len(), 49 * 48 / 2);

    let zero_acc = || vec![0f32; NUM_COMBOS * NUM_COMBOS];
    let num_chunks = rayon::current_num_threads().max(1);
    let chunk_size = runouts.len().div_ceil(num_chunks);

    // acc[h0 * NUM_COMBOS + h1] accumulates `2 * sign(rank0 vs rank1)` over
    // every unordered runout where both h0 and h1 are live.
    let acc: Vec<f32> = runouts
        .par_chunks(chunk_size.max(1))
        .map(|chunk| {
            let mut acc = zero_acc();
            let mut rank_by_combo = vec![0u16; NUM_COMBOS];
            let mut live = vec![false; NUM_COMBOS];
            for &(i, j) in chunk {
                let turn = remaining[i];
                let river = remaining[j];
                let runout_set: CardSet = [board[0], board[1], board[2], turn, river]
                    .into_iter()
                    .collect();

                live.fill(false);
                for combo in 0..NUM_COMBOS {
                    let (c1, c2) = combo_pairs[combo];
                    if runout_set.contains(c1) || runout_set.contains(c2) {
                        continue;
                    }
                    rank_by_combo[combo] =
                        rank_of(board.into_iter().chain([turn, river, c1, c2])).0;
                    live[combo] = true;
                }

                for h0 in 0..NUM_COMBOS {
                    if !live[h0] {
                        continue;
                    }
                    let (a1, a2) = combo_pairs[h0];
                    let r0 = rank_by_combo[h0];
                    let row = &mut acc[h0 * NUM_COMBOS..(h0 + 1) * NUM_COMBOS];
                    for h1 in 0..NUM_COMBOS {
                        if !live[h1] {
                            continue;
                        }
                        let (b1, b2) = combo_pairs[h1];
                        if b1 == a1 || b1 == a2 || b2 == a1 || b2 == a2 {
                            continue;
                        }
                        let payoff = match r0.cmp(&rank_by_combo[h1]) {
                            std::cmp::Ordering::Greater => 2.0,
                            std::cmp::Ordering::Equal => 0.0,
                            std::cmp::Ordering::Less => -2.0,
                        };
                        row[h1] += payoff;
                    }
                }
            }
            acc
        })
        .reduce(zero_acc, |mut a, b| {
            for (x, y) in a.iter_mut().zip(&b) {
                *x += y;
            }
            a
        });

    let mut total_weight = 0.0f64;
    let mut total_value = 0.0f64;
    for h0 in 0..NUM_COMBOS {
        let (a1, a2) = combo_pairs[h0];
        if board_set.contains(a1) || board_set.contains(a2) {
            continue;
        }
        let w0 = ranges[Player::P0].weight(h0) as f64;
        if w0 == 0.0 {
            continue;
        }
        for h1 in 0..NUM_COMBOS {
            let (b1, b2) = combo_pairs[h1];
            if board_set.contains(b1) || board_set.contains(b2) {
                continue;
            }
            if b1 == a1 || b1 == a2 || b2 == a1 || b2 == a2 {
                continue;
            }
            let w1 = ranges[Player::P1].weight(h1) as f64;
            if w1 == 0.0 {
                continue;
            }
            let pair_value = acc[h0 * NUM_COMBOS + h1] as f64 / 1980.0;
            let weight = w0 * w1;
            total_weight += weight;
            total_value += weight * pair_value;
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

/// Same check-down invariant as `no_bet_flop_value_matches_direct_enumeration`
/// but at full-range scale (Range::full() for both players), which forces
/// the reference computation onto the efficient runout-major enumeration in
/// [`direct_checkdown_ev_full_range`] rather than the naive per-pair one.
#[test]
#[ignore = "full-range enumeration is heavy; CI runs it in release"]
fn no_bet_flop_value_full_ranges() {
    let board = flop3("Qs Jh 2h");
    let ranges = PerPlayer::new(Range::full(), Range::full());
    let config = PostflopConfig {
        board: board.to_vec(),
        ranges: ranges.clone(),
        pot: Chips(2),
        effective_stack: Chips(100),
        // No bet fractions, no raises anywhere: a pure check-down tree, same
        // degenerate single-action-per-node shape as the smaller test above.
        ..Default::default()
    };
    let game = build_postflop_game(&config, chip_ev());
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(1));
    solver.run(1);
    let got = solver.expected_value(Player::P0);
    let want = direct_checkdown_ev_full_range(board, &ranges);
    assert!(
        // f32 accumulation noise at full-range scale (millions of terms
        // summed through f32 chains, on both the solver and reference
        // sides) is larger than the small-range test's tolerance.
        (got - want).abs() < 5e-4,
        "full-range check-down EV mismatch: solver={got}, direct={want}"
    );
}

#[test]
#[ignore = "two 2000-iteration turn-tree solves are slow unoptimized; CI runs it in release"]
fn iso_on_off_converge_to_same_value() {
    // Flop is monotone spades (2s 7s Ks); the turn card 2h fixes hearts.
    // The board's stabilizer is then exactly {identity, swap(clubs,
    // diamonds)}, and the pair ranges below are symmetric under every suit
    // permutation, so the merged tree is an exact per-hand quotient of the
    // full tree (see `Builder::quotient_transition`); the sharper per-hand
    // agreement is pinned by `iso_quotient_matches_full_tree_per_hand`.
    // This test keeps the coarser end-to-end check on a longer solve:
    // converged values agree within the sum of both solves' exploitability
    // bounds — for a zero-sum game, any eps-equilibrium's value is within
    // eps of the game value.
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
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[0.75], &[0.75], 1),
            river: StreetTree::pot_fractions(&[], &[], 0),
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
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[0.75], &[0.75], 1),
            river: StreetTree::pot_fractions(&[], &[], 0),
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
        ..base
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
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[], &[], 0),
            river: StreetTree::pot_fractions(&[0.5], &[0.5], 1),
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
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[], &[], 0),
            river: StreetTree::pot_fractions(&[5.0], &[5.0], 1),
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
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[], &[], 0),
            river: StreetTree::pot_fractions(&[0.5], &[0.5], 1),
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

    // Extend the cross-check to a tree that actually exercises donk menus,
    // per-level raises, `include_allin`, and `allin_threshold` together
    // (flop -> turn, an IP bet OOP can only call sets up the turn's donk
    // spot), so `memory_usage`'s `Counting` mirror and the real `Builder`
    // can never disagree about any of these features' fan-out. Single-combo
    // ranges disjoint from an asymmetric board keep the two-chance-deal tree
    // small enough to build without `#[ignore]`.
    let flop = StreetTree {
        ip_bet: vec![SizeSpec::PotAfterCall { fraction: 0.5 }],
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let turn = StreetTree {
        oop_bet: vec![SizeSpec::ToChips { value: 5.0 }],
        oop_donk: Some(vec![SizeSpec::ToChips { value: 3.0 }]),
        oop_raise: Some(vec![vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }]]),
        ip_raise: Some(vec![
            vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }],
            vec![SizeSpec::PreviousBetMultiple { factor: 2.0 }],
        ]),
        max_aggressive_actions: 3,
        include_allin: true,
        allin_threshold: Some(0.9),
        ..Default::default()
    };
    let rich_config = PostflopConfig {
        board: parse_cards("2s 7d 9h"),
        ranges: PerPlayer::new(
            "AsAh".parse::<Range>().unwrap(),
            "KdKc".parse::<Range>().unwrap(),
        ),
        pot: Chips(4),
        effective_stack: Chips(40),
        streets: PerStreet {
            flop,
            turn,
            river: StreetTree::default(),
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let rich_estimate = memory_usage(&rich_config);
    let rich_game = build_postflop_game(&rich_config, chip_ev());
    let real_terminals = rich_game
        .game
        .tree
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Terminal)
        .count() as u64;
    assert_eq!(
        rich_estimate.f32_bytes,
        rich_game.game.tree.storage_len as u64 * 2 * 4
    );
    assert_eq!(rich_estimate.nodes, rich_game.game.tree.nodes.len() as u64);
    assert_eq!(rich_estimate.terminals, real_terminals);
}

/// Perf smoke test on a realistic 3-bet-pot spot: build + solve telemetry
/// (memory estimate, node/terminal/rank-table counts, build and solve wall
/// time, NashConv at two checkpoints) printed for CI logs, plus the one
/// correctness invariant cheap enough to assert without a golden value —
/// NashConv strictly decreases as DCFR gets more iterations, and never dips
/// meaningfully negative (a zero-sum solve's exploitability floor is 0, so
/// only f32/BR noise should ever push it slightly below).
#[test]
#[ignore = "perf smoke; CI runs it in release"]
fn smoke_solve_3bet_pot() {
    let board = flop3("Qs Jh 2h");
    let ranges = PerPlayer::new(
        "22+,ATs+,KTs+,QTs+,JTs,T9s,98s,AJo+,KQo"
            .parse::<Range>()
            .unwrap(),
        "22+,ATs+,KJs+,AQo+".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board: board.to_vec(),
        ranges,
        pot: Chips(200),
        effective_stack: Chips(725),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[0.55], &[0.55], 2),
            turn: StreetTree::pot_fractions(&[0.55], &[0.55], 2),
            river: StreetTree::pot_fractions(&[0.55], &[0.55], 2),
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: false,
    };

    let estimate = memory_usage(&config);
    println!(
        "smoke_solve_3bet_pot: memory_usage f32={:.1} MB, nodes={}, terminals={}, rank_tables={}",
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.nodes,
        estimate.terminals,
        estimate.rank_tables,
    );

    let build_start = Instant::now();
    let game = build_postflop_game(&config, chip_ev());
    let build_time = build_start.elapsed();
    println!("smoke_solve_3bet_pot: build wall time = {build_time:?}");

    // Default `ParConfig` (set by `Solver::new`) — parallel over chance
    // branches.
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(200));

    let solve_start = Instant::now();
    solver.run(50);
    let first_time = solve_start.elapsed();
    let expl1 = solver.exploitability();
    let nash_conv_1 = expl1[Player::P0] + expl1[Player::P1];
    println!(
        "smoke_solve_3bet_pot: iter=50 NashConv={nash_conv_1:.4} ({:.3}% pot), wall={first_time:?}",
        nash_conv_1 / config.pot.as_f64() * 100.0,
    );

    let solve_start2 = Instant::now();
    solver.run(150);
    let second_time = solve_start2.elapsed();
    let expl2 = solver.exploitability();
    let nash_conv_2 = expl2[Player::P0] + expl2[Player::P1];
    println!(
        "smoke_solve_3bet_pot: iter=200 NashConv={nash_conv_2:.4} ({:.3}% pot), wall={second_time:?}",
        nash_conv_2 / config.pot.as_f64() * 100.0,
    );

    assert!(
        nash_conv_1.is_finite() && nash_conv_1 > -1e-3,
        "NashConv at 50 iters must be finite and non-negative (within noise): {nash_conv_1}"
    );
    assert!(
        nash_conv_2.is_finite() && nash_conv_2 > -1e-3,
        "NashConv at 200 iters must be finite and non-negative (within noise): {nash_conv_2}"
    );
    assert!(
        nash_conv_2 < nash_conv_1,
        "NashConv should decrease with more iterations: {nash_conv_1} (50 iters) -> \
         {nash_conv_2} (200 iters)"
    );
}

/// `track_node_info: false` must skip building history/action-label strings
/// altogether, not just skip storing them: `node_info` stays at just the
/// build-time untagged sentinel, and every compiled node's tag is 0.
#[test]
fn untracked_node_info_stays_empty() {
    let board = parse_cards("2s 7s Ks 2h");
    let config = PostflopConfig {
        board,
        ranges: PerPlayer::new("44,55".parse().unwrap(), "33,66".parse().unwrap()),
        pot: Chips(2),
        effective_stack: Chips(20),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[0.75], &[0.75], 1),
            river: StreetTree::pot_fractions(&[], &[], 0),
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: false,
    };
    let game = build_postflop_game(&config, chip_ev());
    assert_eq!(
        game.node_info.len(),
        1,
        "untracked builds must keep only the untagged sentinel entry"
    );
    assert!(
        game.game.tree.tags.iter().all(|&t| t == 0),
        "untracked builds must tag every compiled node 0"
    );

    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(10));
    solver.run(10);
    let ev = solver.expected_value(Player::P0);
    assert!(ev.is_finite(), "expected value must be finite: {ev}");
}

/// The quotient construction is exact per hand, not just in aggregate:
/// solving the merged tree and the full tree must produce (near-)identical
/// root strategies combo by combo. Trajectories agree up to f32 summation
/// order (the transition's backward sum vs sequential branch accumulation),
/// so the tolerance is a float-noise bound, not an exploitability bound.
#[test]
#[ignore = "slow unoptimized; CI runs it in release with --include-ignored"]
fn iso_quotient_matches_full_tree_per_hand() {
    let board = parse_cards("2s 7s Ks 2h");
    let ranges = PerPlayer::new(
        "44,55".parse::<Range>().unwrap(),
        "33,66".parse::<Range>().unwrap(),
    );
    let base = PostflopConfig {
        board,
        ranges: ranges.clone(),
        pot: Chips(2),
        effective_stack: Chips(20),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[0.75], &[0.75], 1),
            river: StreetTree::pot_fractions(&[1.0], &[1.0], 1),
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
        let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(64));
        solver.set_par(sequential);
        solver.run(64);
        (
            solver.expected_value(Player::P0),
            solver.average_strategy_at(0),
        )
    };
    let (ev_on, sig_on) = run(true);
    let (ev_off, sig_off) = run(false);
    assert!(
        (ev_on - ev_off).abs() < 1e-4,
        "iso EV mismatch: {ev_on} vs {ev_off}"
    );
    assert_eq!(sig_on.len(), sig_off.len());
    let num_actions = sig_on.len() / NUM_COMBOS;
    for a in 0..num_actions {
        for combo in 0..NUM_COMBOS {
            if ranges[Player::P0].weight(combo) == 0.0 {
                continue;
            }
            let (x, y) = (
                sig_on[a * NUM_COMBOS + combo],
                sig_off[a * NUM_COMBOS + combo],
            );
            assert!(
                (x - y).abs() < 1e-4,
                "root strategy diverged: action {a} combo {combo}: merged {x} vs full {y}"
            );
        }
    }
}

/// In the *unmerged* tree, two isomorphic river branches must converge to
/// suit-permuted copies of one strategy — the symmetry the quotient encodes
/// structurally. Uses the clubs<->diamonds swap that stabilizes the board.
#[test]
#[ignore = "slow unoptimized; CI runs it in release with --include-ignored"]
fn member_branch_matches_suit_permuted_rep_branch() {
    let board = parse_cards("2s 7s Ks 2h");
    let ranges = PerPlayer::new(
        "44,55".parse::<Range>().unwrap(),
        "33,66".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board,
        ranges: ranges.clone(),
        pot: Chips(2),
        effective_stack: Chips(20),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[], &[], 0),
            river: StreetTree::pot_fractions(&[1.0], &[1.0], 1),
        },
        iso_merging: false,
        ..Default::default()
    };
    let game = build_postflop_game(&config, chip_ev());
    let node_info = game.node_info.clone();
    let tags = game.game.tree.tags.clone();
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(64));
    solver.set_par(ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    });
    solver.run(64);

    let find = |history: &str| -> engine::NodeId {
        let tag = node_info
            .iter()
            .position(|i| i.history == history)
            .unwrap_or_else(|| panic!("history {history:?} not found")) as u32;
        tags.iter().position(|&t| t == tag).unwrap() as u32
    };
    // Turn checks through; compare the 8c and 8d river branches under the
    // clubs<->diamonds swap ([1, 0, 2, 3] in suit order c, d, h, s).
    let perm: hand_index::SuitPerm = [1, 0, 2, 3];
    let node_c = find("xx[8c]");
    let node_d = find("xx[8d]");
    let sig_c = solver.average_strategy_at(node_c);
    let sig_d = solver.average_strategy_at(node_d);
    let num_actions = sig_c.len() / NUM_COMBOS;
    for a in 0..num_actions {
        for combo in 0..NUM_COMBOS {
            if ranges[Player::P0].weight(combo) == 0.0 {
                continue;
            }
            let x = sig_c[a * NUM_COMBOS + combo];
            let y = sig_d[a * NUM_COMBOS + hand_index::permute_combo(&perm, combo)];
            assert!(
                (x - y).abs() < 1e-3,
                "branches not suit-symmetric: action {a} combo {combo}: {x} vs {y}"
            );
        }
    }
}

/// The quantized `I16Storage` backend must solve a real postflop tree (not
/// just toy games) to essentially the same equilibrium as `F32Storage`: a
/// small turn-start spot (same board/ranges/turn-only betting shape as
/// `iso_on_off_converge_to_same_value` above, run for far fewer iterations)
/// solved sequentially with each backend must agree on both the expected
/// value and NashConv.
#[test]
#[ignore = "slow unoptimized; CI runs it in release with --include-ignored"]
fn i16_storage_matches_f32_on_small_turn_spot() {
    let board = parse_cards("2s 7s Ks 2h");
    let ranges = PerPlayer::new(
        "44,55".parse::<Range>().unwrap(),
        "33,66".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(20),
        streets: PerStreet {
            flop: StreetTree::pot_fractions(&[], &[], 0),
            turn: StreetTree::pot_fractions(&[0.75], &[0.75], 1),
            river: StreetTree::pot_fractions(&[], &[], 0),
        },
        ..Default::default()
    };
    let sequential = ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    };
    let iters = 100;

    let game_f32 = build_postflop_game(&config, chip_ev());
    let mut solver_f32 =
        Solver::<_, F32Storage>::new(game_f32.game, Box::<Dcfr>::default(), Some(iters));
    solver_f32.set_par(sequential);
    solver_f32.run(iters);

    let game_i16 = build_postflop_game(&config, chip_ev());
    let mut solver_i16 =
        Solver::<_, I16Storage>::new(game_i16.game, Box::<Dcfr>::default(), Some(iters));
    solver_i16.set_par(sequential);
    solver_i16.run(iters);

    let ev_f32 = solver_f32.expected_value(Player::P0);
    let ev_i16 = solver_i16.expected_value(Player::P0);
    assert!(
        (ev_f32 - ev_i16).abs() < 2e-3,
        "expected_value diverged: f32={ev_f32} i16={ev_i16}"
    );

    let expl_f32 = solver_f32.exploitability();
    let expl_i16 = solver_i16.exploitability();
    let nash_conv_f32 = expl_f32[Player::P0] + expl_f32[Player::P1];
    let nash_conv_i16 = expl_i16[Player::P0] + expl_i16[Player::P1];
    assert!(
        (nash_conv_f32 - nash_conv_i16).abs() < 2e-3,
        "NashConv diverged: f32={nash_conv_f32} i16={nash_conv_i16}"
    );
}

/// `raise_fractions` sizes a raise off the pot after a call, independently
/// of `bet_fractions` (which only sizes a bet with no outstanding bet to
/// face) -- reads the action amounts straight off `node_info`, the same
/// history/action-label pattern `clairvoyance_game_matches_closed_form` in
/// `river.rs` uses. Pot 4, `bet_fractions = [0.5]` sizes the root bet to
/// `0.5 * 4 = 2`; facing that bet, `raise_fractions = [1.0]` sizes the
/// raise's additional amount to `1.0 * (pot + 2*bet) = 1.0 * (4 + 4) = 8`,
/// landing the total commitment at `2 + 8 = 10`.
#[test]
fn raise_fractions_size_independently_of_bet_fractions() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(4),
        effective_stack: Chips(100),
        streets: PerStreet {
            flop: manual_street(&[], &[], &[], &[], 0),
            turn: manual_street(&[], &[], &[], &[], 0),
            river: manual_street(&[0.5], &[0.5], &[1.0], &[1.0], 2),
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());

    let root = game.node_by_history("").expect("root history");
    let root_tag = game.game.tree.tags[root as usize] as usize;
    assert!(
        game.node_info[root_tag]
            .actions
            .contains(&"bet 2".to_string()),
        "root actions: {:?}",
        game.node_info[root_tag].actions
    );

    let facing_bet = game.node_by_history("r2").expect("facing-bet history");
    let facing_tag = game.game.tree.tags[facing_bet as usize] as usize;
    assert!(
        game.node_info[facing_tag]
            .actions
            .contains(&"raise to 10".to_string()),
        "facing-bet actions: {:?}",
        game.node_info[facing_tag].actions
    );
}

/// When `raise_fractions` equals `bet_fractions`, the tree is identical
/// (same node count, same action amounts) to the pre-split single-list
/// behaviour, since the raise then reuses the bet's own fraction: additional
/// = `0.5 * (pot + 2*bet) = 0.5 * (4 + 4) = 4`, landing the total commitment
/// at `2 + 4 = 6`. A distinct raise fraction (`1.0`, as in the test above)
/// changes that amount but not the tree's shape -- node count depends only
/// on how many distinct fractions each list holds, not their values.
#[test]
fn an_unset_raise_menu_reuses_the_bet_menu() {
    // The pre-`StreetTree` grammar had `oop_raise`/`ip_raise` fall back to
    // `oop`/`ip`, so a config that names only bet sizes still raises with
    // them. Losing that fallback would silently shrink every such tree, so
    // pin it: `None` and an explicit copy of the bet menu must build the
    // same tree, and an explicit empty menu must not.
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let bets = vec![SizeSpec::PotAfterCall { fraction: 0.5 }];
    let build = |oop_raise: Option<Vec<Vec<SizeSpec>>>| {
        let river = StreetTree {
            oop_bet: bets.clone(),
            ip_bet: bets.clone(),
            oop_raise: oop_raise.clone(),
            ip_raise: oop_raise,
            ..StreetTree::default()
        };
        holdem::memory_usage(&PostflopConfig {
            board: board.clone(),
            ranges: ranges.clone(),
            pot: Chips(10),
            effective_stack: Chips(50),
            streets: PerStreet {
                river,
                ..PerStreet::default()
            },
            ..PostflopConfig::default()
        })
    };

    let fallback = build(None);
    let explicit = build(Some(vec![bets.clone()]));
    assert_eq!(
        (fallback.nodes, fallback.terminals),
        (explicit.nodes, explicit.terminals),
        "an unset raise menu must build the same tree as an explicit copy of the bet menu"
    );

    let never = build(Some(Vec::new()));
    assert!(
        never.nodes < fallback.nodes,
        "an explicitly empty raise menu must remove the raise branches: \
         {} vs {}",
        never.nodes,
        fallback.nodes
    );
}

#[test]
fn matching_raise_and_bet_fractions_reproduce_shared_size_tree() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    // Shared-size street: `oop_raise`/`ip_raise` reuse the same 0.5 pot
    // fraction as `oop_bet`/`ip_bet` (what `StreetTree::pot_fractions`
    // always builds).
    let shared_river = StreetTree::pot_fractions(&[0.5], &[0.5], 2);
    let shared_config = PostflopConfig {
        board: board.clone(),
        ranges: ranges.clone(),
        pot: Chips(4),
        effective_stack: Chips(100),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river: shared_river,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let shared_game = build_postflop_game(&shared_config, chip_ev());

    // Distinct-size street: same 0.5 bet menu, but a 1.0 raise menu.
    let distinct_river = manual_street(&[0.5], &[0.5], &[1.0], &[1.0], 2);
    let distinct_config = PostflopConfig {
        board,
        ranges,
        pot: Chips(4),
        effective_stack: Chips(100),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river: distinct_river,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let distinct_game = build_postflop_game(&distinct_config, chip_ev());

    assert_eq!(
        shared_game.game.tree.nodes.len(),
        distinct_game.game.tree.nodes.len(),
        "raise fraction value must not change tree shape"
    );

    let facing_bet = shared_game
        .node_by_history("r2")
        .expect("facing-bet history");
    let facing_tag = shared_game.game.tree.tags[facing_bet as usize] as usize;
    assert!(
        shared_game.node_info[facing_tag]
            .actions
            .contains(&"raise to 6".to_string()),
        "facing-bet actions: {:?}",
        shared_game.node_info[facing_tag].actions
    );
}

// --- New size-literal grammar: raise levels, donk, all-in handling --------

/// `oop_raise`/`ip_raise` index by raise level (index 0 = the street's
/// first raise, deeper levels reuse the last entry): a `"3x"` at level 0
/// must size off the bet it faces, and a distinct `"2.5x"` at level 1 must
/// size off the *raise* it faces, not repeat level 0's multiple.
#[test]
fn raise_levels_apply_distinct_multiples_per_level() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let river = StreetTree {
        oop_bet: vec![SizeSpec::ToChips { value: 10.0 }],
        oop_raise: Some(vec![
            vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }],
            vec![SizeSpec::PreviousBetMultiple { factor: 2.5 }],
        ]),
        ip_raise: Some(vec![
            vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }],
            vec![SizeSpec::PreviousBetMultiple { factor: 2.5 }],
        ]),
        max_aggressive_actions: 3,
        ..Default::default()
    };
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(4),
        effective_stack: Chips(1000),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());

    // Level 0 (the street's first raise, facing OOP's opening 10-chip bet):
    // "3x" of the bet, i.e. 30.
    let facing_bet = game.node_by_history("r10").expect("facing-bet history");
    let tag = game.game.tree.tags[facing_bet as usize] as usize;
    assert!(
        game.node_info[tag]
            .actions
            .contains(&"raise to 30".to_string()),
        "level 0 should apply 3x the bet: {:?}",
        game.node_info[tag].actions
    );

    // Level 1 (the second raise, facing IP's 30-chip raise): "2.5x" of the
    // raise (75), not level 0's "3x" (which would give 90).
    let facing_reraise = game
        .node_by_history("r10r30")
        .expect("facing-reraise history");
    let tag = game.game.tree.tags[facing_reraise as usize] as usize;
    assert!(
        game.node_info[tag]
            .actions
            .contains(&"raise to 75".to_string()),
        "level 1 should apply 2.5x the raise it faces: {:?}",
        game.node_info[tag].actions
    );
    assert!(
        !game.node_info[tag]
            .actions
            .contains(&"raise to 90".to_string()),
        "level 1 must not reuse level 0's 3x multiple: {:?}",
        game.node_info[tag].actions
    );
}

/// `oop_donk` gates OOP's opening menu on a street whose previous street's
/// last aggressor was IP: an empty donk menu (`Some(vec![])`) removes OOP's
/// bet from that turn node, while the same turn node (reached after the flop
/// checked through instead) keeps OOP's ordinary `oop_bet` menu, since
/// `previous_aggressor` is `None` there.
#[test]
fn donk_menu_controls_oop_opening_action_after_a_called_ip_bet() {
    let board = parse_cards("2s 7d 9h");
    let ranges = PerPlayer::new(
        "AsAh".parse::<Range>().unwrap(),
        "KdKc".parse::<Range>().unwrap(),
    );
    let flop = StreetTree {
        ip_bet: vec![SizeSpec::PotAfterCall { fraction: 0.5 }],
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let turn = StreetTree {
        oop_bet: vec![SizeSpec::ToChips { value: 5.0 }],
        oop_donk: Some(Vec::new()),
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(100),
        streets: PerStreet {
            flop,
            turn,
            river: StreetTree::default(),
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());

    // IP bet the flop (root check "x", IP bets to 1, OOP calls: "xr1c"),
    // then a turn card deals: `previous_aggressor = Some(IP)`, so the empty
    // donk menu applies and OOP's turn node must offer only "check". The
    // history must be exactly "xr1c[Xy]" (8 chars) -- a bare prefix/suffix
    // match would also catch a deeper river-entry node reached after the
    // turn itself checks through (e.g. "xr1c[Xy]xx[Ab]"), which has its own
    // (empty, default) menu and would make this assertion pass for the
    // wrong reason.
    let donked_node = game
        .node_info
        .iter()
        .find(|info| info.history.starts_with("xr1c[") && info.history.len() == 8)
        .unwrap_or_else(|| {
            panic!(
                "expected a turn node after a called IP flop bet: {:?}",
                game.node_info
                    .iter()
                    .map(|i| &i.history)
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        donked_node.actions,
        vec!["check".to_string()],
        "an empty donk menu must remove OOP's bet: {:?}",
        donked_node.actions
    );

    // Flop checked through ("xx"): `previous_aggressor = None`, so OOP's
    // turn node keeps its ordinary bet menu. Same exact-length guard as
    // above: "xx[Xy]" is 6 chars.
    let checked_node = game
        .node_info
        .iter()
        .find(|info| info.history.starts_with("xx[") && info.history.len() == 6)
        .unwrap_or_else(|| panic!("expected a turn node after a flop check-check"));
    assert!(
        checked_node.actions.contains(&"bet 5".to_string()),
        "a checked-through flop must not suppress OOP's turn bet menu: {:?}",
        checked_node.actions
    );
}

/// `include_allin` adds exactly one extra all-in action alongside a sized
/// menu that has room below the maximum, but does not duplicate the all-in
/// target when a sized entry already resolves to it.
#[test]
fn include_allin_adds_one_action_without_duplicating_an_already_allin_size() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let with_room = StreetTree {
        oop_bet: vec![SizeSpec::StackFraction { fraction: 0.5 }],
        include_allin: true,
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let config_a = PostflopConfig {
        board: board.clone(),
        ranges: ranges.clone(),
        pot: Chips(2),
        effective_stack: Chips(10),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river: with_room,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game_a = build_postflop_game(&config_a, chip_ev());
    let root_a = game_a.node_by_history("").expect("root history");
    let tag_a = game_a.game.tree.tags[root_a as usize] as usize;
    assert_eq!(
        game_a.node_info[tag_a].actions,
        vec![
            "check".to_string(),
            "bet 5".to_string(),
            "bet 10".to_string(),
        ],
        "include_allin should add exactly one extra all-in action: {:?}",
        game_a.node_info[tag_a].actions
    );

    let already_allin = StreetTree {
        oop_bet: vec![SizeSpec::StackFraction { fraction: 1.0 }],
        include_allin: true,
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let config_b = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(10),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river: already_allin,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game_b = build_postflop_game(&config_b, chip_ev());
    let root_b = game_b.node_by_history("").expect("root history");
    let tag_b = game_b.game.tree.tags[root_b as usize] as usize;
    assert_eq!(
        game_b.node_info[tag_b].actions,
        vec!["check".to_string(), "bet 10".to_string()],
        "an already-all-in size must not be duplicated by include_allin: {:?}",
        game_b.node_info[tag_b].actions
    );
}

/// `allin_threshold` collapses a size landing at or above that fraction of
/// the actor's maximum target into the all-in target itself, even without
/// `include_allin`.
#[test]
fn allin_threshold_merges_a_near_max_size_into_allin() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let river = StreetTree {
        oop_bet: vec![SizeSpec::StackFraction { fraction: 0.9 }],
        allin_threshold: Some(0.8),
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(10),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());
    let root = game.node_by_history("").expect("root history");
    let tag = game.game.tree.tags[root as usize] as usize;
    assert_eq!(
        game.node_info[tag].actions,
        vec!["check".to_string(), "bet 10".to_string()],
        "a 90% target above the 80% threshold must merge into all-in: {:?}",
        game.node_info[tag].actions
    );
}

/// A raise proposal that resolves under the minimum full raise is bumped up
/// to it rather than standing as an illegal short raise.
#[test]
fn sub_minimum_raise_is_bumped_to_the_minimum_full_raise() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    // OOP opens for 2 (a literal `ToChips`, so `min_bet`'s default of 1 has
    // no effect on the open itself): the facing node's `last_full_raise` is
    // then 2, and the minimum full raise is `bet_to_match(2) +
    // last_full_raise(2) = 4`. IP's raise menu proposes a 20% pot-after-call
    // raise, which resolves to 3 -- under the minimum -- so it must be
    // bumped up to 4 rather than standing as its own action.
    let river = StreetTree {
        oop_bet: vec![SizeSpec::ToChips { value: 2.0 }],
        ip_raise: Some(vec![vec![SizeSpec::PotAfterCall { fraction: 0.2 }]]),
        max_aggressive_actions: 2,
        ..Default::default()
    };
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(100),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river,
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());
    let facing_bet = game.node_by_history("r2").expect("facing-bet history");
    let tag = game.game.tree.tags[facing_bet as usize] as usize;
    assert!(
        game.node_info[tag]
            .actions
            .contains(&"raise to 4".to_string()),
        "a sub-minimum raise proposal must be bumped to the minimum full raise: {:?}",
        game.node_info[tag].actions
    );
    assert!(
        !game.node_info[tag]
            .actions
            .contains(&"raise to 3".to_string()),
        "the sub-minimum raw target must not survive alongside the bumped one: {:?}",
        game.node_info[tag].actions
    );
}

/// `min_bet` is the smallest legal opening bet: a config with `min_bet =
/// Chips(5)` bumps a 3-chip opening proposal up to 5 rather than opening
/// for less.
#[test]
fn min_bet_refuses_to_open_below_its_own_value() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let river = StreetTree {
        oop_bet: vec![SizeSpec::ToChips { value: 3.0 }],
        max_aggressive_actions: 1,
        ..Default::default()
    };
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(100),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::default(),
            river,
        },
        min_bet: Chips(5),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());
    let root = game.node_by_history("").expect("root history");
    let tag = game.game.tree.tags[root as usize] as usize;
    assert_eq!(
        game.node_info[tag].actions,
        vec!["check".to_string(), "bet 5".to_string()],
        "an opening bet below min_bet must be bumped up to min_bet: {:?}",
        game.node_info[tag].actions
    );
}

/// An odd starting pot builds and solves like any other; its chip-EV
/// terminal payoffs match the even pot one chip larger exactly (both round
/// the shared floor/ceil split the same way once the extra chip is
/// absorbed), and exceed the even pot one chip smaller by exactly 1.
#[test]
fn odd_pot_terminal_payoffs_match_the_neighboring_even_pots() {
    // Deterministic showdown, no betting at all: P0 (the full house board's
    // Aces) always wins, so its chip-EV payoff is exactly `pot -
    // oop_share`, i.e. the amount contributed by IP. `oop_share = pot / 2`
    // (floor) regardless of parity, so an odd pot and the even pot one chip
    // *larger* both give IP the same ceil share -- while the even pot one
    // chip *smaller* gives IP one less chip, dropping P0's EV by exactly 1.
    let board = parse_cards("Ks Kh Kd 2c 2d");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "QQ".parse::<Range>().unwrap(),
    );
    let ev_for_pot = |pot: u32| -> f64 {
        let config = PostflopConfig {
            board: board.clone(),
            ranges: ranges.clone(),
            pot: Chips(pot),
            effective_stack: Chips(100),
            streets: PerStreet {
                flop: StreetTree::default(),
                turn: StreetTree::default(),
                river: StreetTree::default(),
            },
            min_bet: Chips(1),
            iso_merging: true,
            track_node_info: true,
        };
        let game = build_postflop_game(&config, chip_ev());
        let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(1));
        solver.run(1);
        solver.expected_value(Player::P0)
    };

    let ev_odd = ev_for_pot(7);
    let ev_smaller_even = ev_for_pot(6);
    let ev_larger_even = ev_for_pot(8);

    assert!(
        (ev_odd - ev_larger_even).abs() < 1e-6,
        "odd pot 7 (OOP=3/IP=4) should match even pot 8's EV (OOP=4/IP=4): \
         {ev_odd} vs {ev_larger_even}"
    );
    assert!(
        (ev_odd - ev_smaller_even - 1.0).abs() < 1e-6,
        "odd pot 7 should exceed even pot 6's EV (OOP=3/IP=3) by exactly 1 chip: \
         {ev_odd} vs {ev_smaller_even}"
    );
}

/// A history round trip through `viewer::river_entry_state` using the `r`
/// compact-history token (not the pre-rename `b`): a bet *and* a raise both
/// emit `r{to}` tokens, and replaying that exact history reconstructs the
/// same pot/effective-stack the trunk itself carried into the river.
#[test]
fn history_round_trip_uses_r_tokens_through_river_entry_state() {
    let board = parse_cards("2s 7s Ks 2h");
    let ranges = PerPlayer::new(
        "44,55".parse::<Range>().unwrap(),
        "33,66".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(2),
        effective_stack: Chips(20),
        streets: PerStreet {
            flop: StreetTree::default(),
            turn: StreetTree::pot_fractions(&[0.75], &[0.75], 2),
            river: StreetTree::default(),
        },
        min_bet: Chips(1),
        iso_merging: true,
        track_node_info: true,
    };
    let game = build_postflop_game(&config, chip_ev());

    // OOP bets 2 ("r2"), IP raises to 7 ("r7"), OOP calls ("c"): the
    // river-entry history is "r2r7c[Xy]" for whichever card the chance node
    // deals -- both the opening bet and the raise answering it use `r`.
    let entry = game
        .node_info
        .iter()
        .find(|info| info.history.starts_with("r2r7c[") && info.history.ends_with(']'))
        .unwrap_or_else(|| {
            panic!(
                "expected a river-entry node at \"r2r7c[..]\": {:?}",
                game.node_info
                    .iter()
                    .map(|i| &i.history)
                    .collect::<Vec<_>>()
            )
        });

    let state = holdem::river_entry_state(&config, &entry.history)
        .unwrap_or_else(|e| panic!("replay failed for history {:?}: {e}", entry.history));

    // c = 7 (both players' turn-street contribution once the raise is
    // called): pot' = trunk.pot(2) + 2*7 = 16, eff' = trunk.stack(20) - 7 = 13.
    assert_eq!(state.pot, Chips(16));
    assert_eq!(state.effective_stack, Chips(13));
    assert_eq!(state.board.len(), 5);
    assert!(
        game.node_by_history(&entry.history).is_some(),
        "the replayed history must resolve back to a real node in the trunk"
    );
}

/// The root is a node like any other: values read at node 0 against the
/// root ranges must aggregate to exactly what `expected_value` reports.
/// This is what lets `export ev` answer any node with the same primitive
/// the root EV already uses.
#[test]
fn per_node_values_at_the_root_agree_with_the_root_expected_value() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "22+,A2s+".parse::<Range>().unwrap(),
        "22+,A2s+".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(20),
        effective_stack: Chips(80),
        streets: PerStreet {
            river: StreetTree::pot_fractions(&[0.5], &[0.5], 2),
            ..PerStreet::default()
        },
        ..PostflopConfig::default()
    };
    let pipeline = PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    };
    let game = build_postflop_game(&config, pipeline);
    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(64));
    solver.run(64);

    let root_ranges = &solver.game().root_ranges;
    for player in Player::BOTH {
        let reach = PerPlayer::new(
            root_ranges[Player::P0].as_slice(),
            root_ranges[Player::P1].as_slice(),
        );
        let values = solver.expected_values_at(0, player, reach);
        let own = &root_ranges[player];
        let total: f64 = own
            .iter()
            .zip(&values)
            .map(|(&r, &v)| r as f64 * v as f64)
            .sum();
        let aggregated = total / solver.game().normalizer;
        let expected = solver.expected_value(player);
        assert!(
            (aggregated - expected).abs() < 1e-6,
            "player {player:?}: per-node {aggregated} vs root {expected}"
        );
    }

    // And a best response is never worse than the profile it answers.
    let reach = PerPlayer::new(
        root_ranges[Player::P0].as_slice(),
        root_ranges[Player::P1].as_slice(),
    );
    let ev = solver.expected_values_at(0, Player::P0, reach);
    let br = solver.best_response_values_at(0, Player::P0, reach);
    for (hand, (&value, &best)) in ev.iter().zip(&br).enumerate() {
        assert!(
            best >= value - 1e-4,
            "hand {hand}: br {best} below profile value {value}"
        );
    }
}

/// A node's recorded contribution is what re-bases its counterfactual
/// values onto the subgame-start basis, so it has to be the real total —
/// starting-pot share included — and the two sides have to sum to the pot
/// at that node.
#[test]
fn node_info_records_the_pot_contribution_at_each_node() {
    let board = parse_cards("2c 7d 9h Js Qs");
    let ranges = PerPlayer::new(
        "AA".parse::<Range>().unwrap(),
        "KK".parse::<Range>().unwrap(),
    );
    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(20),
        effective_stack: Chips(80),
        streets: PerStreet {
            river: StreetTree::pot_fractions(&[0.5], &[0.5], 2),
            ..PerStreet::default()
        },
        ..PostflopConfig::default()
    };
    let pipeline = PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    };
    let game = build_postflop_game(&config, pipeline);

    let root = game
        .node_info
        .iter()
        .find(|info| info.history.is_empty())
        .expect("root node info");
    assert_eq!(root.contrib[Player::P0], Chips(10));
    assert_eq!(root.contrib[Player::P1], Chips(10));
    assert_eq!(root.street, cards::Street::River);

    // After OOP bets 10 (50% of a 20 pot) the pot at that node is 30.
    let faced = game
        .node_info
        .iter()
        .find(|info| info.history == "r10")
        .expect("node after a 10 bet");
    assert_eq!(faced.contrib[Player::P0], Chips(20));
    assert_eq!(faced.contrib[Player::P1], Chips(10));

    for info in &game.node_info {
        if info.history == "<untagged>" {
            continue;
        }
        let pot = info.contrib[Player::P0] + info.contrib[Player::P1];
        assert!(
            pot >= config.pot,
            "history {:?}: pot {pot} below the starting pot",
            info.history
        );
    }
}
