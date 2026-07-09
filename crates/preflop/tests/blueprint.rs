//! End-to-end checks for the bucketed blueprint game (roadmap M6 slice 4).
//!
//! Uses only synthetic artifacts (small dims, hand-built `TransitionTable`/
//! `BucketEquity`) — never `BlueprintArtifacts::build`, which needs a
//! multi-minute abstraction build. Internals that need private access
//! (evaluator differentials, composition invariants, the zero-sum terminal
//! identity) live in `bucketed.rs`'s own `#[cfg(test)]` module instead.

use abstraction::{BlueprintArtifacts, BucketEquity, TransitionTable};
use cards::{Chips, NUM_CLASSES, PerPlayer, Player, Range};
use engine::{Dcfr, F32Storage, Solver, Storage};
use game::{ChipEv, NoRake, PayoffPipeline};
use preflop::{
    EquityShowdown, EquityTable, PostflopBets, PreflopConfig, build_blueprint_game,
    build_preflop_game,
};

fn full_ranges() -> PerPlayer<Range> {
    PerPlayer::new(Range::full(), Range::full())
}

/// Flat symmetric equity table: irrelevant to tree-structure tests but
/// needed by every `build_*_game` call (mirrors `trunk.rs`'s `flat_table`).
fn flat_table() -> EquityTable {
    let n = NUM_CLASSES * NUM_CLASSES;
    EquityTable::from_probabilities(vec![0.5; n], vec![0.0; n])
}

/// Small synthetic artifacts (Kf=3, Kt=2, Kr=2): T1 rows sum to kappa
/// (1225/1326), T2/T3 rows sum to 1, river equity satisfies `win + tie +
/// win^T == 1` (tie symmetric). Values are otherwise arbitrary.
fn synthetic_artifacts() -> BlueprintArtifacts {
    let kappa = 1225.0 / 1326.0;
    let (kf, kt, kr) = (3usize, 2usize, 2usize);

    let mut t1 = Vec::new();
    for h in 0..NUM_CLASSES {
        let b = h % kf;
        t1.push((h as u32, b as u32, kappa as f32));
    }
    let class_to_flop = TransitionTable {
        in_dim: NUM_CLASSES as u32,
        out_dim: kf as u32,
        entries: t1,
    };

    let flop_to_turn = TransitionTable {
        in_dim: kf as u32,
        out_dim: kt as u32,
        entries: vec![
            (0, 0, 0.7),
            (0, 1, 0.3),
            (1, 0, 0.4),
            (1, 1, 0.6),
            (2, 0, 0.5),
            (2, 1, 0.5),
        ],
    };

    let turn_to_river = TransitionTable {
        in_dim: kt as u32,
        out_dim: kr as u32,
        entries: vec![(0, 0, 0.8), (0, 1, 0.2), (1, 0, 0.35), (1, 1, 0.65)],
    };

    // win + tie + win^T == 1, tie symmetric (see the inline fixture in
    // `bucketed.rs`'s tests for the derivation of these numbers).
    let win = vec![0.45, 0.6, 0.35, 0.4];
    let tie = vec![0.1, 0.05, 0.05, 0.2];
    let river_equity = BucketEquity {
        dim: kr as u32,
        win,
        tie,
    };

    BlueprintArtifacts {
        class_to_flop,
        flop_to_turn,
        turn_to_river,
        river_equity,
    }
}

fn no_bets_postflop() -> PostflopBets {
    PostflopBets {
        flop: PerPlayer::new(Vec::new(), Vec::new()),
        turn: PerPlayer::new(Vec::new(), Vec::new()),
        river: PerPlayer::new(Vec::new(), Vec::new()),
        max_raises: 0,
        include_allin: false,
    }
}

/// Push/fold trunk (jam-only, no limp): every continuation terminal in the
/// trunk sense is actually a preflop all-in, so no `Chance(T1)` node is ever
/// built and the bucketed streets are structurally unreachable.
fn push_fold_config(stack: Chips) -> PreflopConfig {
    PreflopConfig {
        effective_stack: stack,
        sb: Chips(5),
        bb: Chips(10),
        ranges: full_ranges(),
        open_sizes_bb: Vec::new(),
        raise_factors: Vec::new(),
        max_raises: 1,
        include_allin: true,
        allow_limp: false,
        track_node_info: true,
    }
}

fn pipeline() -> PayoffPipeline<'static> {
    PayoffPipeline {
        rake: &NoRake,
        utility: &ChipEv,
    }
}

// --- 1. reduction-to-trunk exactness ---------------------------------------

#[test]
fn push_fold_blueprint_reduces_exactly_to_the_trunk() {
    let trunk_config = push_fold_config(Chips(100));
    let post = no_bets_postflop();
    let table = flat_table();
    let artifacts = synthetic_artifacts();

    let trunk_game = build_preflop_game(
        &trunk_config,
        &table,
        &EquityShowdown::default(),
        pipeline(),
    );
    let blueprint_game = build_blueprint_game(&trunk_config, &post, &table, &artifacts, pipeline());

    assert_eq!(
        blueprint_game.game.tree.nodes.len(),
        trunk_game.game.tree.nodes.len(),
        "push/fold blueprint must have the same node count as the trunk"
    );
    assert_eq!(
        blueprint_game.node_info.len(),
        trunk_game.node_info.len(),
        "push/fold blueprint must have the same action-node count as the trunk"
    );
    let trunk_histories: Vec<&str> = trunk_game
        .node_info
        .iter()
        .map(|i| i.history.as_str())
        .collect();
    let blueprint_histories: Vec<&str> = blueprint_game
        .node_info
        .iter()
        .map(|i| i.history.as_str())
        .collect();
    assert_eq!(
        blueprint_histories, trunk_histories,
        "push/fold blueprint must resolve identical histories to the trunk"
    );

    let mut trunk_solver =
        Solver::<_, F32Storage>::new(trunk_game.game, Box::new(Dcfr::default()), Some(500));
    let mut blueprint_solver =
        Solver::<_, F32Storage>::new(blueprint_game.game, Box::new(Dcfr::default()), Some(500));
    trunk_solver.run(500);
    blueprint_solver.run(500);

    for p in Player::BOTH {
        let ev_t = trunk_solver.expected_value(p);
        let ev_b = blueprint_solver.expected_value(p);
        assert!(
            (ev_t - ev_b).abs() < 1e-9,
            "expected_value diverged for {p:?}: trunk {ev_t} vs blueprint {ev_b}"
        );
    }
    let expl_t = trunk_solver.exploitability();
    let expl_b = blueprint_solver.exploitability();
    for p in Player::BOTH {
        assert!(
            (expl_t[p] - expl_b[p]).abs() < 1e-9,
            "exploitability diverged for {p:?}: trunk {} vs blueprint {}",
            expl_t[p],
            expl_b[p]
        );
    }
}

// --- 2. structure smoke -----------------------------------------------------

fn structure_smoke_trunk() -> PreflopConfig {
    PreflopConfig {
        effective_stack: Chips(200), // 20bb at CHIPS_PER_BB=10
        sb: Chips(5),
        bb: Chips(10),
        ranges: full_ranges(),
        open_sizes_bb: vec![2.5],
        raise_factors: Vec::new(),
        max_raises: 1,
        include_allin: true,
        allow_limp: false,
        track_node_info: true,
    }
}

fn structure_smoke_postflop() -> PostflopBets {
    PostflopBets {
        flop: PerPlayer::new(vec![0.5], vec![0.5]),
        turn: PerPlayer::new(vec![0.5], vec![0.5]),
        river: PerPlayer::new(vec![0.5], vec![0.5]),
        max_raises: 1,
        include_allin: true,
    }
}

#[test]
fn structure_smoke_matches_memory_usage_and_bb_acts_first() {
    let trunk_config = structure_smoke_trunk();
    let post = structure_smoke_postflop();
    let table = flat_table();
    let artifacts = synthetic_artifacts();

    let estimate = preflop::blueprint_memory_usage(&trunk_config, &post, &artifacts);
    let game = build_blueprint_game(&trunk_config, &post, &table, &artifacts, pipeline());

    let terminal_count = game
        .game
        .tree
        .nodes
        .iter()
        .filter(|n| n.kind == engine::NodeKind::Terminal)
        .count() as u64;

    assert_eq!(estimate.nodes, game.game.tree.nodes.len() as u64);
    assert_eq!(estimate.terminals, terminal_count);
    let f32_bytes = engine::F32Storage::bytes_for(
        game.game.tree.storage_len,
        game.game.tree.storage_refs.len(),
    );
    assert_eq!(estimate.f32_bytes, f32_bytes);

    // Preflop open (r25), call: continuation into the flop street (pinned
    // history convention: postflop streets separated by `/`, tokens
    // `x`/`c`/`f`/`b{to}` with `to` the actor's total street contribution).
    let flop_first = game
        .node_by_history("r25c/")
        .expect("flop's first decision node (BB to act)");
    assert_eq!(
        game.game.tree.node(flop_first).player,
        Player::P1,
        "BB must act first on the flop"
    );
    assert_eq!(
        game.info(flop_first).actions,
        vec![
            "Check".to_string(),
            "Bet 25".to_string(),
            "Bet 175".to_string()
        ],
        "flop bet sizing: 0.5 pot (25) and the all-in (175, stack behind)"
    );

    // BB bets 25 (half the 50-chip flop pot), SB calls: turn's first
    // decision node, still BB.
    let turn_first = game
        .node_by_history("r25c/b25c/")
        .expect("turn's first decision node (BB to act)");
    assert_eq!(
        game.game.tree.node(turn_first).player,
        Player::P1,
        "BB must act first on the turn"
    );
    assert_eq!(
        game.info(turn_first).actions,
        vec![
            "Check".to_string(),
            "Bet 50".to_string(),
            "Bet 150".to_string()
        ],
    );
}

// --- 6. end-to-end solve smoke ----------------------------------------------

#[test]
fn structure_smoke_game_solves_and_converges() {
    let trunk_config = structure_smoke_trunk();
    let post = structure_smoke_postflop();
    let table = flat_table();
    let artifacts = synthetic_artifacts();

    let game = build_blueprint_game(&trunk_config, &post, &table, &artifacts, pipeline());
    assert!(game.game.zero_sum, "NoRake + ChipEv must be zero-sum");

    let mut solver = Solver::<_, F32Storage>::new(game.game, Box::new(Dcfr::default()), Some(500));

    // This synthetic game is tiny (3 postflop bet sizes, Kf/Kt/Kr <= 3) and
    // DCFR converges within a handful of iterations, so the "early" mark
    // has to be very early (not e.g. 50 iterations in, which already sits
    // at the float-noise floor) for a decrease to be observable at all.
    solver.run(1);
    let early = solver.exploitability();
    let early_total = early[Player::P0] + early[Player::P1];
    assert!(early_total.is_finite());

    solver.run(499);
    let late = solver.exploitability();
    let late_total = late[Player::P0] + late[Player::P1];
    assert!(late_total.is_finite());
    assert!(
        late_total < early_total,
        "exploitability should decrease with more iterations: early={early_total}, late={late_total}"
    );
    assert!(
        late_total.abs() < 1.0,
        "exploitability should be small after 500 iterations: {late_total}"
    );

    let ev0 = solver.expected_value(Player::P0);
    let ev1 = solver.expected_value(Player::P1);
    // Scaled tolerance (trunk.rs's zero-sum test does the same): these EVs
    // sit around chip-scale magnitude (tens of chips), so an absolute 1e-6
    // is tighter than f32-accumulated solver output can reliably hit.
    let tol = 1e-4 * (1.0 + ev0.abs().max(ev1.abs()));
    assert!(
        (ev0 + ev1).abs() < tol,
        "ev0 {ev0} + ev1 {ev1} != 0 (tol {tol})"
    );
}
