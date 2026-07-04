//! Roadmap M4: rake / ICM / general-sum validation on the postflop solver.
//!
//! These tests exercise the `game::payoff` pipeline (see its unit tests for
//! the pipeline-level invariants) all the way through a real postflop
//! solve: a pure HU-ICM solve must be identical to its chip-EV twin (ICM is
//! affine in stacks for two players), a raked solve must leak exactly the
//! rake in aggregate, and rake must visibly shift both bettor and defender
//! strategies at a hand-verifiable spot.

use cards::{Card, Chips, NUM_COMBOS, PerPlayer, Player, Range};
use engine::{Dcfr, F32Storage, ParConfig, Solver};
use game::{ChipEv, Icm, NoRake, PayoffPipeline, PercentCapRake};
use holdem::{PerStreet, PostflopConfig, build_postflop_game};

fn chip_ev() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

fn parse_cards(s: &str) -> Vec<Card> {
    s.split_whitespace().map(|c| c.parse().unwrap()).collect()
}

/// No chance-node parallelism: repeated solves of the same tree accumulate
/// floats in the same order, so two pipelines that must agree exactly (the
/// ICM-vs-chip-EV invariant test) do so up to plain float noise rather than
/// reduction-order noise as well.
fn sequential() -> ParConfig {
    ParConfig {
        chance_depth: 0,
        min_children: usize::MAX,
    }
}

/// Small turn-start config shared by the ICM-agreement test below: same
/// board/ranges/bet grammar shape as
/// `iso_quotient_matches_full_tree_per_hand` in `postflop.rs`.
fn small_turn_config() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("2s 7s Ks 2h"),
        ranges: PerPlayer::new(
            "44,55".parse::<Range>().unwrap(),
            "33,66".parse::<Range>().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(20),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![0.75], vec![0.75]),
            river: PerPlayer::new(vec![1.0], vec![1.0]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 1,
            river: 1,
        },
        ..Default::default()
    }
}

/// Root strategy plus NashConv for `small_turn_config()` under `pipeline`,
/// solved sequentially for `iterations`.
fn solve_root_strategy(pipeline: PayoffPipeline<'_>, iterations: u64) -> (Vec<f32>, f64) {
    let config = small_turn_config();
    let game = build_postflop_game(&config, pipeline);
    let mut solver =
        Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(iterations));
    solver.set_par(sequential());
    solver.run(iterations);
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    (solver.average_strategy_at(0), nash_conv)
}

/// Max absolute pointwise difference between two root average strategies,
/// over every action and every in-range (nonzero P0 weight) combo.
fn max_root_strategy_diff(a: &[f32], b: &[f32], p0_range: &Range) -> f32 {
    assert_eq!(a.len(), b.len());
    let num_actions = a.len() / NUM_COMBOS;
    let mut max_diff = 0.0f32;
    for act in 0..num_actions {
        for combo in 0..NUM_COMBOS {
            if p0_range.weight(combo) == 0.0 {
                continue;
            }
            let diff = (a[act * NUM_COMBOS + combo] - b[act * NUM_COMBOS + combo]).abs();
            max_diff = max_diff.max(diff);
        }
    }
    max_diff
}

/// HU ICM is an affine, equal-slope function of chip stacks (see
/// `game::payoff::tests::hu_icm_is_affine_in_chip_ev`), and with `NoRake`
/// the total of both players' stacks after any terminal is a constant equal
/// to the starting total (nothing leaves the game). So every baked ICM
/// payoff is exactly `slope * baked chip-EV payoff` for one global positive
/// `slope = (payouts[0] - payouts[1]) / total_stacks`. Regret matching and
/// linear regret accumulation are both invariant under a uniform positive
/// rescale of every terminal payoff, so the two solves must land on
/// identical strategies at every iteration, not just at the optimum. This
/// validates the whole bake -> solve pipeline for ICM, not merely the
/// `PayoffPipeline::bake` unit tests.
#[test]
#[ignore = "slow unoptimized; CI runs it in release with --include-ignored"]
fn pure_hu_icm_postflop_solve_matches_chip_ev() {
    let icm = Icm {
        payouts: [100.0, 60.0],
    };
    let icm_pipeline = PayoffPipeline {
        rake: &NoRake,
        utility: &icm,
    };

    // Same iteration count as `iso_quotient_matches_full_tree_per_hand` in
    // `postflop.rs` (also a sequential solve of this exact tree): enough for
    // a loose NashConv bound, small enough to stay debug-runnable. The
    // strategy-agreement assertion below does not depend on how converged
    // the solves are — it holds at every iteration, converged or not, per
    // the scale-invariance argument above.
    let iterations = 64;
    let (sig_chip, conv_chip) = solve_root_strategy(chip_ev(), iterations);
    let (sig_icm, conv_icm) = solve_root_strategy(icm_pipeline, iterations);

    let p0_range: Range = "44,55".parse().unwrap();
    let max_diff = max_root_strategy_diff(&sig_chip, &sig_icm, &p0_range);
    assert!(
        max_diff < 5e-3,
        "chip-EV and ICM root strategies diverged: max diff {max_diff}"
    );

    // Loose convergence bound (a fraction of the pot): both solves are the
    // same tiny turn tree, just rescaled.
    assert!(
        conv_chip < 0.5,
        "chip-EV solve did not converge: nash_conv={conv_chip}"
    );
    assert!(
        conv_icm < 0.5,
        "ICM solve did not converge: nash_conv={conv_icm}"
    );
}

/// Small river-start config shared by the rake-aggregate and
/// defend-frequency tests: same board/ranges/bet grammar as
/// `river_solve_is_zero_sum` in `river.rs`, but built through
/// `build_postflop_game` (a 5-card board starts directly on the river, no
/// chance nodes) so it shares the exact terminal/kernel path every other
/// postflop config uses.
fn small_river_config() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("2c 7d 9h Js Qs"),
        ranges: PerPlayer::new(
            "22+,A2s+,KTo+".parse::<Range>().unwrap(),
            "55-22,QJs,A5s-A2s,KQo,T9s".parse::<Range>().unwrap(),
        ),
        pot: Chips(10),
        effective_stack: Chips(50),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![], vec![]),
            river: PerPlayer::new(vec![0.5], vec![0.5]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: 2,
        },
        ..Default::default()
    }
}

/// The postflop builder must propagate `PayoffPipeline::is_zero_sum` into
/// `CompiledGame::zero_sum`, which gates the engine's exploitability fast
/// path (deriving P1's EV as -P0's). Unraked chip EV and unraked HU ICM
/// qualify; any value-taking rake must not.
#[test]
fn postflop_builder_sets_zero_sum_flag() {
    let config = small_river_config();
    assert!(build_postflop_game(&config, chip_ev()).game.zero_sum);

    let icm = Icm {
        payouts: [100.0, 60.0],
    };
    let icm_pipeline = PayoffPipeline {
        rake: &NoRake,
        utility: &icm,
    };
    assert!(build_postflop_game(&config, icm_pipeline).game.zero_sum);

    let raked = PercentCapRake {
        rate: 0.05,
        cap: 1e9,
        no_flop_no_drop: false,
    };
    let raked_pipeline = PayoffPipeline {
        rake: &raked,
        utility: &ChipEv,
    };
    assert!(!build_postflop_game(&config, raked_pipeline).game.zero_sum);
}

/// A percentage rake taken at every terminal (uncapped in practice: `cap` is
/// far above any pot this config can reach) must leak value in aggregate:
/// `ev_p0 + ev_p1` is the negative of the range-weighted average rake taken,
/// strictly below zero. The unraked twin, solved the same way, stays
/// zero-sum within solver noise — the usual invariant asserted throughout
/// `postflop.rs` and `river.rs`.
#[test]
fn raked_solve_leaks_exactly_the_rake_in_aggregate() {
    let config = small_river_config();
    let iterations = 200;

    let raked = PercentCapRake {
        rate: 0.05,
        cap: 1e9, // effectively uncapped for this config's pot sizes
        no_flop_no_drop: false,
    };
    let raked_pipeline = PayoffPipeline {
        rake: &raked,
        utility: &ChipEv,
    };
    let raked_game = build_postflop_game(&config, raked_pipeline);
    let mut raked_solver =
        Solver::<_, F32Storage>::new(raked_game.game, Box::<Dcfr>::default(), Some(iterations));
    raked_solver.run(iterations);
    let ev_p0 = raked_solver.expected_value(Player::P0);
    let ev_p1 = raked_solver.expected_value(Player::P1);
    assert!(
        ev_p0 + ev_p1 < -1e-3,
        "raked solve should leak value in aggregate: ev_p0={ev_p0} ev_p1={ev_p1} sum={}",
        ev_p0 + ev_p1
    );
    let raked_expl = raked_solver.exploitability();
    assert!(
        raked_expl[Player::P0] > -1e-3 && raked_expl[Player::P1] > -1e-3,
        "exploitability must be nonnegative: {raked_expl:?}"
    );

    let unraked_game = build_postflop_game(&config, chip_ev());
    let mut unraked_solver =
        Solver::<_, F32Storage>::new(unraked_game.game, Box::<Dcfr>::default(), Some(iterations));
    unraked_solver.run(iterations);
    let sum = unraked_solver.expected_value(Player::P0) + unraked_solver.expected_value(Player::P1);
    assert!(sum.abs() < 1e-3, "unraked solve should be zero-sum: {sum}");
}

/// Clairvoyance spot from `river.rs`'s `clairvoyance_game_matches_closed_form`
/// (P0 perfectly polarized: AA nuts, 33 air; P1 pure bluffcatcher: QQ; pot 2,
/// stacks 2, one pot-sized bet, one raise cap, only P0 can bet), built via
/// `build_postflop_game` on a completed 5-card board rather than
/// `RiverConfig` — same tree either way, since `river::build_river_game` is
/// a thin shim over the postflop builder.
fn clairvoyance_config() -> PostflopConfig {
    PostflopConfig {
        board: parse_cards("Ks Kh Kd 2c 2d"),
        ranges: PerPlayer::new(
            "AA,33".parse::<Range>().unwrap(),
            "QQ".parse::<Range>().unwrap(),
        ),
        pot: Chips(2),
        effective_stack: Chips(2),
        bet_fractions: PerStreet {
            flop: PerPlayer::new(vec![], vec![]),
            turn: PerPlayer::new(vec![], vec![]),
            river: PerPlayer::new(vec![1.0], vec![]),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: 1,
        },
        iso_merging: false,
        track_node_info: true,
    }
}

/// QQ's call frequency at the facing-bet node ("b2": P0 bet to 2, actions
/// [fold, call]) and 33's bluffing frequency at the root (actions [check,
/// bet 2]), for the clairvoyance spot under `pipeline`.
fn clairvoyance_frequencies(pipeline: PayoffPipeline<'_>, iterations: u64) -> (f64, f64) {
    let config = clairvoyance_config();
    let game = build_postflop_game(&config, pipeline);
    let root = game.node_by_history("").expect("root history");
    let facing_bet = game.node_by_history("b2").expect("facing-bet history");
    let mut solver =
        Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(iterations));
    solver.run(iterations);

    let freq = |node: engine::NodeId, range: &str, action: usize| -> f64 {
        let range: Range = range.parse().unwrap();
        let sigma = solver.average_strategy_at(node);
        let mut total = 0.0;
        let mut hit = 0.0;
        for combo in 0..NUM_COMBOS {
            let w = range.weight(combo) as f64;
            if w > 0.0 {
                total += w;
                hit += w * sigma[action * NUM_COMBOS + combo] as f64;
            }
        }
        hit / total
    };

    let call_freq = freq(facing_bet, "QQ", 1); // [fold, call]
    let bluff_freq = freq(root, "33", 1); // [check, bet 2]
    (call_freq, bluff_freq)
}

/// Closed form generalizing `river.rs`'s unraked clairvoyance game to a
/// `PercentCapRake { rate, .. }` (cap large enough to be a no-op here). Let
/// `contrib0 = contrib1 = 3` after a called bet (1 ante + 2 shove each) and
/// `= 3, 1` after a fold; the rake model in this crate taxes *any* terminal
/// pot, fold or showdown, so both branches shrink:
///
/// - Showdown pot 6 -> net `V(rate) = 6*(1-rate) - 3`: the winner's payoff
///   over baseline, whether QQ (catches a bluff) or AA (beats a value bet).
/// - Fold pot 4 -> net `F(rate) = 4*(1-rate) - 3`: P0's payoff when a bluff
///   goes through uncontested. The loser's payoff is always `-contrib`,
///   untouched by rake (rake only ever shrinks a *winner's* share).
///
/// QQ's indifference between call (`P(value)*(-3) + P(bluff)*V`, reached-node
/// odds `1 : beta` for value vs bluff) and fold (`-1`, just the ante) pins
/// down P0's bluff frequency `beta = 2 / (V + 1)`; at `rate=0` this is the
/// textbook `2/4 = 0.5`. Rake *shrinks* `V` (a caught bluff pays the bluffer
/// less), so `beta` must *rise* to keep QQ indifferent: at `rate=0.10`,
/// `V = 2.4` and `beta = 2/3.4 ≈ 0.588` — rake makes the bettor bluff more,
/// not less.
///
/// Symmetrically, 33's indifference between betting (`(1-kappa)*F -
/// kappa*3`) and checking (`-1`, always loses the checked-down showdown)
/// pins down QQ's call frequency `kappa = (F + 1) / (F + 3)`; at `rate=0`
/// this is `2/4 = 0.5`. Rake shrinks `F` too (an uncontested bluff pays less
/// after rake), so `kappa` *falls*: at `rate=0.10`, `F = 0.6` and
/// `kappa = 1.6/3.6 ≈ 0.444`. This is the pot-odds mechanism the test name
/// refers to: rake shrinks what a defender recovers by catching a bluff, so
/// she must defend less often to stay indifferent — even though, per the
/// first half of this derivation, the bettor is compensating by bluffing
/// *more* to keep her indifferent in the first place.
#[test]
fn rake_makes_defender_call_less() {
    let iterations = 4_000;
    let (call_no_rake, bluff_no_rake) = clairvoyance_frequencies(chip_ev(), iterations);

    let raked = PercentCapRake {
        rate: 0.10,
        cap: 100.0,
        no_flop_no_drop: false,
    };
    let raked_pipeline = PayoffPipeline {
        rake: &raked,
        utility: &ChipEv,
    };
    let (call_raked, bluff_raked) = clairvoyance_frequencies(raked_pipeline, iterations);

    // kappa: 0.5 -> ~0.444 (closed form above).
    assert!(
        call_no_rake - call_raked >= 0.03,
        "rake should reduce QQ's call frequency by >= 0.03: {call_no_rake} -> {call_raked}"
    );
    // beta: 0.5 -> ~0.588 (closed form above) — rake pushes bluffing *up*,
    // the opposite of the naive intuition, because the bettor must show up
    // with more bluffs to keep an increasingly rake-squeezed defender
    // indifferent at all.
    assert!(
        bluff_raked - bluff_no_rake >= 0.03,
        "rake should increase P0's bluffing (33 betting) frequency by >= 0.03: \
         {bluff_no_rake} -> {bluff_raked}"
    );
}

/// Two rake structures with the same rate but very different caps must
/// solve to materially different strategies. Reuses `small_river_config()`
/// (real ranges, real single-street bet/raise action) rather than
/// `small_turn_config()`: the latter's root turns out to be a degenerate
/// pure-strategy node (always check) at the top of the tree regardless of
/// rake — deep stacks relative to the pot make a turn bet unprofitable for
/// either range here, so there is no indifference point for a rake
/// perturbation to move. `small_river_config()`'s root genuinely mixes bet
/// vs. check for many combos (see e.g. this test's own printed diffs), so a
/// cap that changes how much of a large river pot gets raked away visibly
/// moves it. Terminal pots there range up to `10 + 2*25 = 60`-ish once
/// raises stack up, so a 5% rake ranges up to ~3 chips uncapped; `cap = 4.0`
/// never binds, `cap = 0.1` caps essentially every contested pot down to a
/// flat 0.1 chips. `#[ignore]`d: two 2000-iteration solves of this
/// two-raise river tree are release-only for the same reason as the other
/// heavier solves in `postflop.rs`.
#[test]
#[ignore = "slow unoptimized; CI runs it in release with --include-ignored"]
fn rake_cap_level_changes_strategy() {
    let config = small_river_config();
    let iterations = 2000;
    let run = |cap: f64| {
        let rake = PercentCapRake {
            rate: 0.05,
            cap,
            no_flop_no_drop: false,
        };
        let pipeline = PayoffPipeline {
            rake: &rake,
            utility: &ChipEv,
        };
        let game = build_postflop_game(&config, pipeline);
        let mut solver =
            Solver::<_, F32Storage>::new(game.game, Box::<Dcfr>::default(), Some(iterations));
        solver.run(iterations);
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        (solver.average_strategy_at(0), nash_conv)
    };

    let (sig_loose_cap, conv_loose_cap) = run(4.0);
    let (sig_tight_cap, conv_tight_cap) = run(0.1);

    let p0_range: Range = "22+,A2s+,KTo+".parse().unwrap();
    let max_diff = max_root_strategy_diff(&sig_loose_cap, &sig_tight_cap, &p0_range);
    assert!(
        max_diff > 0.02,
        "different rake cap levels should produce materially different strategies: max diff {max_diff}"
    );
    assert!(
        conv_loose_cap < 5e-3,
        "loose-cap solve did not converge: nash_conv={conv_loose_cap}"
    );
    assert!(
        conv_tight_cap < 5e-3,
        "tight-cap solve did not converge: nash_conv={conv_tight_cap}"
    );
}
