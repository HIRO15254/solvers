use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use cards::{Card, Chips, NUM_COMBOS, PerPlayer, Player, Range, combo_cards};
use engine::{
    CfrPlus, Dcfr, DiscountSchedule, F32Storage, HsDcfr, NodeId, NodeKind, ParConfig, Solver,
    TerminalEvaluator, Vanilla, linear_cfr,
};
use game::{
    ChipEv, GgPreflopRake, Icm, NoRake, PayoffPipeline, PercentCapRake, RakeModel, UtilityModel,
};
use holdem::{PerStreet, PostflopConfig, PostflopEvaluator, build_postflop_game, memory_usage};
use serde::Serialize;

use crate::config::{
    AlgorithmSection, BetsSection, GameSection, RakeSection, RunSection, SolveConfig,
    UtilitySection,
};

pub fn run(config_path: &Path, output: Option<&Path>, histories: &[String]) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config: SolveConfig = toml::from_str(&raw).context("parsing config")?;

    let rake: Box<dyn RakeModel> = match config.rake {
        RakeSection::None => Box::new(NoRake),
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => Box::new(PercentCapRake {
            rate,
            cap,
            no_flop_no_drop,
        }),
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => Box::new(GgPreflopRake {
            rate,
            cap,
            exempt_pot: Chips(exempt_pot),
        }),
    };
    let utility: Box<dyn UtilityModel> = match config.utility {
        UtilitySection::ChipEv => Box::new(ChipEv),
        UtilitySection::Icm { payouts } => Box::new(Icm { payouts }),
    };
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };

    let schedule: Box<dyn DiscountSchedule> = match config.algorithm {
        AlgorithmSection::Vanilla => Box::new(Vanilla),
        AlgorithmSection::CfrPlus => Box::new(CfrPlus),
        AlgorithmSection::Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        } => Box::new(Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        }),
        AlgorithmSection::LinearCfr => Box::new(linear_cfr()),
        AlgorithmSection::HsDcfr { gamma0 } => Box::new(HsDcfr { gamma0 }),
    };
    let schedule_name = schedule.name();

    match config.game {
        GameSection::Kuhn => solve_toy(
            "kuhn",
            game::kuhn(pipeline),
            schedule,
            schedule_name,
            &config.run,
            output,
        ),
        GameSection::Leduc => solve_toy(
            "leduc",
            game::leduc(pipeline),
            schedule,
            schedule_name,
            &config.run,
            output,
        ),
        GameSection::Postflop {
            board,
            oop_range,
            ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
        } => solve_postflop(
            pipeline,
            &board,
            &oop_range,
            &ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
            schedule,
            schedule_name,
            &config.run,
            output,
            histories,
        ),
    }
}

/// Convergence loop shared by every game: run a chunk of iterations, report
/// exploitability, and stop early once `target_nash_conv` is hit. Generic
/// over the terminal evaluator so both toy games and postflop subgames reuse
/// it unchanged.
fn run_loop<E: TerminalEvaluator>(solver: &mut Solver<E, F32Storage>, run: &RunSection) {
    let mut remaining = run.iterations;
    while remaining > 0 {
        let chunk = run.check_every.min(remaining);
        solver.run(chunk);
        remaining -= chunk;
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        println!(
            "iter={:>8} expl_p0={:.3e} expl_p1={:.3e} nash_conv={:.3e}",
            solver.iteration(),
            expl[Player::P0],
            expl[Player::P1],
            nash_conv,
        );
        if let Some(target) = run.target_nash_conv
            && nash_conv < target
        {
            println!("target nash_conv {target:.3e} reached");
            break;
        }
    }
}

/// Final convergence summary, shared by every game.
fn print_done<E: TerminalEvaluator>(solver: &Solver<E, F32Storage>, elapsed: Duration) {
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    let value = solver.expected_value(Player::P0);
    println!(
        "done: iterations={} wall={:.2}s value_p0={:.6} nash_conv={:.3e}",
        solver.iteration(),
        elapsed.as_secs_f64(),
        value,
        nash_conv,
    );
}

fn solve_toy(
    kind: &str,
    toy: game::ToyGame,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
) -> Result<()> {
    let node_info = toy.node_info.clone();
    let mut solver = Solver::<_, F32Storage>::new(toy.game, schedule, Some(run.iterations));

    println!(
        "game={} schedule={} iterations={}",
        kind, schedule_name, run.iterations
    );
    let start = Instant::now();
    run_loop(&mut solver, run);
    let elapsed = start.elapsed();
    print_done(&solver, elapsed);

    if let Some(path) = output {
        let report = export_toy(kind, &node_info, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn solve_postflop(
    pipeline: PayoffPipeline<'_>,
    board: &str,
    oop_range: &str,
    ip_range: &str,
    pot: u32,
    effective_stack: u32,
    iso_merging: bool,
    bets: BetsSection,
    schedule: Box<dyn DiscountSchedule>,
    schedule_name: &str,
    run: &RunSection,
    output: Option<&Path>,
    histories: &[String],
) -> Result<()> {
    let board: Vec<Card> = board
        .split_whitespace()
        .map(|token| {
            token
                .parse::<Card>()
                .map_err(|_| anyhow!("invalid card {token:?} in board (expected e.g. \"Ks\")"))
        })
        .collect::<Result<Vec<Card>>>()?;
    let oop = oop_range
        .parse::<Range>()
        .map_err(|e| anyhow!("parsing oop_range {oop_range:?}: {e}"))?;
    let ip = ip_range
        .parse::<Range>()
        .map_err(|e| anyhow!("parsing ip_range {ip_range:?}: {e}"))?;
    let ranges = PerPlayer::new(oop, ip);

    let bet_fractions = PerStreet {
        flop: PerPlayer::new(bets.flop.oop, bets.flop.ip),
        turn: PerPlayer::new(bets.turn.oop, bets.turn.ip),
        river: PerPlayer::new(bets.river.oop, bets.river.ip),
    };
    let max_raises = PerStreet {
        flop: bets.flop.max_raises,
        turn: bets.turn.max_raises,
        river: bets.river.max_raises,
    };

    let config = PostflopConfig {
        board,
        ranges,
        pot: Chips(pot),
        effective_stack: Chips(effective_stack),
        bet_fractions,
        max_raises,
        iso_merging,
        track_node_info: true,
    };

    // Cheap dry run before committing to the (possibly very large) real
    // build, so an oversized config fails fast with a size estimate instead
    // of silently eating memory.
    let estimate = memory_usage(&config);
    println!(
        "tree: nodes={} terminals={} rank_tables={} storage={:.1} MiB (f32) / {:.1} MiB (i16)",
        estimate.nodes,
        estimate.terminals,
        estimate.rank_tables,
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_bytes as f64 / (1024.0 * 1024.0),
    );

    let pf_game = build_postflop_game(&config, pipeline);

    // Resolve the requested export histories to node ids (and their
    // player/action labels) while the built tree and node_info are still
    // both in hand; `pf_game.game` moves into the solver right after.
    let mut resolved = Vec::new();
    for history in histories {
        match pf_game.node_by_history(history) {
            Some(node_id) => {
                let tag = pf_game.game.tree.tags[node_id as usize] as usize;
                let info = &pf_game.node_info[tag];
                let player = pf_game.game.tree.node(node_id).player.index();
                resolved.push(ResolvedHistory {
                    history: history.clone(),
                    node_id,
                    player,
                    actions: info.actions.clone(),
                });
            }
            None => {
                eprintln!("warning: unknown history {history:?}, skipping");
            }
        }
    }

    if let Some(n) = run.threads {
        // Ignore "already initialized": tests and repeated calls within one
        // process may have set the global pool already.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    let mut solver = Solver::<_, F32Storage>::new(pf_game.game, schedule, Some(run.iterations));
    solver.set_par(ParConfig {
        chance_depth: run.par_chance_depth.unwrap_or(2),
        min_children: run.par_min_children.unwrap_or(12),
    });

    println!(
        "game=postflop schedule={} iterations={}",
        schedule_name, run.iterations
    );
    let start = Instant::now();
    run_loop(&mut solver, run);
    let elapsed = start.elapsed();
    print_done(&solver, elapsed);

    if let Some(path) = output {
        let report = export_postflop(&resolved, &solver);
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!("strategy written to {}", path.display());
    }
    Ok(())
}

/// A requested `--history` resolved against the built tree, before the tree
/// moves into the solver.
struct ResolvedHistory {
    history: String,
    node_id: NodeId,
    player: usize,
    actions: Vec<String>,
}

#[derive(Serialize)]
struct StrategyReport {
    game: String,
    iterations: u64,
    expected_value_p0: f64,
    exploitability: [f64; 2],
    nodes: Vec<NodeReport>,
}

#[derive(Serialize)]
struct NodeReport {
    history: String,
    player: usize,
    actions: Vec<String>,
    /// Per hand: action distribution (indexed like `actions`).
    strategy: Vec<Vec<f32>>,
}

fn export_toy(
    game_kind: &str,
    node_info: &[game::ToyNodeInfo],
    solver: &Solver<game::ToyEvaluator, F32Storage>,
) -> StrategyReport {
    let tree = &solver.game().tree;
    let mut nodes = Vec::new();
    for node_id in 0..tree.nodes.len() as u32 {
        let node = tree.node(node_id);
        if node.kind != NodeKind::Action {
            continue;
        }
        let info = &node_info[tree.tags[node_id as usize] as usize];
        let sigma = solver.average_strategy_at(node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        let strategy = (0..num_hands)
            .map(|h| (0..num_actions).map(|a| sigma[a * num_hands + h]).collect())
            .collect();
        nodes.push(NodeReport {
            history: info.history.clone(),
            player: node.player.index(),
            actions: info.actions.clone(),
            strategy,
        });
    }
    let expl = solver.exploitability();
    StrategyReport {
        game: game_kind.to_string(),
        iterations: solver.iteration(),
        expected_value_p0: solver.expected_value(Player::P0),
        exploitability: [expl[Player::P0], expl[Player::P1]],
        nodes,
    }
}

#[derive(Serialize)]
struct PostflopReport {
    game: String,
    iterations: u64,
    expected_value_p0: f64,
    exploitability: [f64; 2],
    entries: Vec<HistoryEntry>,
}

#[derive(Serialize)]
struct HistoryEntry {
    history: String,
    player: usize,
    actions: Vec<String>,
    /// One row per combo with non-zero range weight for the acting player:
    /// `[combo_index, "AhKs", [action probabilities...]]`.
    strategy: Vec<(usize, String, Vec<f32>)>,
}

fn export_postflop(
    entries: &[ResolvedHistory],
    solver: &Solver<PostflopEvaluator, F32Storage>,
) -> PostflopReport {
    let tree = &solver.game().tree;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let node = tree.node(entry.node_id);
        let sref = tree.storage_ref(node);
        let (num_actions, num_hands) = (sref.num_actions as usize, sref.num_hands as usize);
        debug_assert_eq!(num_hands, NUM_COMBOS);
        let sigma = solver.average_strategy_at(entry.node_id);
        // The game's own root ranges, not the raw user-specified `Range`:
        // these are already zeroed for combos that conflict with the
        // starting board, so board-blocked hands (e.g. a pocket pair whose
        // rank sits on the board) never leak into the export with
        // meaningless untouched storage values.
        let range = &solver.game().root_ranges[Player::from_index(entry.player)];

        let mut strategy = Vec::new();
        for combo in 0..num_hands {
            if range[combo] <= 0.0 {
                continue;
            }
            let (hi, lo) = combo_cards(combo);
            let probs: Vec<f32> = (0..num_actions)
                .map(|a| sigma[a * num_hands + combo])
                .collect();
            strategy.push((combo, format!("{hi}{lo}"), probs));
        }
        out.push(HistoryEntry {
            history: entry.history.clone(),
            player: entry.player,
            actions: entry.actions.clone(),
            strategy,
        });
    }
    let expl = solver.exploitability();
    PostflopReport {
        game: "postflop".to_string(),
        iterations: solver.iteration(),
        expected_value_p0: solver.expected_value(Player::P0),
        exploitability: [expl[Player::P0], expl[Player::P1]],
        entries: out,
    }
}
