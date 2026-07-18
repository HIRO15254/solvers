//! `report`: solve the same postflop config across multiple boards and
//! write a CSV report (one row per board).
//!
//! The original design sketch restricted this to 3-card (flop) boards via
//! `--flops`/`--flops-file` flags. That's changed here to `--boards`/
//! `--boards-file`, accepting boards of any starting-street length (3, 4,
//! or 5 cards): a fast debug-build integration test needs river-only (5
//! card) boards, which solve in seconds even in a debug build, whereas flop
//! boards take minutes.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use cards::{Card, Player};
use engine::{F32Storage, Solver, TerminalEvaluator};
use game::PayoffPipeline;
use holdem::range_equity;

use crate::config::{GameSection, RunSection};
use crate::postflop_setup;

pub fn run(
    config_path: &Path,
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config: crate::config::SolveConfig = toml::from_str(&raw).context("parsing config")?;

    let (oop_range, ip_range, pot, effective_stack, iso_merging, bets) = match config.game {
        GameSection::Postflop {
            board,
            oop_range,
            ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
        } => {
            eprintln!("note: config board {board:?} ignored; using --boards");
            (oop_range, ip_range, pot, effective_stack, iso_merging, bets)
        }
        GameSection::Preflop { .. } => {
            return Err(anyhow!(
                "report does not support preflop configs yet (kind = \"preflop\")"
            ));
        }
        _ => return Err(anyhow!("report only supports kind = \"postflop\" configs")),
    };

    let raw_boards = collect_board_tokens(boards_arg, boards_file)?;
    let boards: Vec<Vec<Card>> = raw_boards
        .iter()
        .map(|token| {
            let normalized = normalize_board_token(token);
            let cards = postflop_setup::parse_board(&normalized)?;
            if !(3..=5).contains(&cards.len()) {
                return Err(anyhow!(
                    "board {token:?} must have 3, 4, or 5 cards, got {}",
                    cards.len()
                ));
            }
            Ok(cards)
        })
        .collect::<Result<Vec<_>>>()?;

    let rake = postflop_setup::build_rake(&config.rake);
    let utility = postflop_setup::build_utility(&config.utility);

    let mut header_labels: Option<Vec<String>> = None;
    let mut csv_rows: Vec<String> = Vec::new();

    for (i, board_cards) in boards.iter().enumerate() {
        let board_str: String = board_cards
            .iter()
            .map(Card::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        let pf_config = postflop_setup::build_postflop_config(
            &board_str,
            &oop_range,
            &ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets.clone(),
        )?;

        let estimate = holdem::memory_usage(&pf_config);
        if i == 0 {
            // Same fields as `postflop_setup::print_memory_estimate`, but
            // routed to stderr: stdout is reserved for the CSV report alone
            // (the `report_smoke` test reads stdout as pure CSV), whereas
            // `solve`/`inspect` intentionally print this to stdout as part
            // of their normal operational output.
            eprintln!(
                "tree: nodes={} terminals={} rank_tables={} storage={:.1} MiB (f32) / {:.1} MiB (i16)",
                estimate.nodes,
                estimate.terminals,
                estimate.rank_tables,
                estimate.f32_bytes as f64 / (1024.0 * 1024.0),
                estimate.i16_bytes as f64 / (1024.0 * 1024.0),
            );
        }

        let pipeline = PayoffPipeline {
            rake: rake.as_ref(),
            utility: utility.as_ref(),
        };
        let pf_game = holdem::build_postflop_game(&pf_config, pipeline);
        let node_info = pf_game.node_info.clone();

        let schedule = postflop_setup::build_schedule(&config.algorithm);
        let mut solver =
            Solver::<_, F32Storage>::new(pf_game.game, schedule, Some(config.run.iterations));

        let start = Instant::now();
        // Deliberately not `crate::solve::run_loop`: that prints its
        // per-chunk convergence trace to stdout, which would interleave
        // with (and break parsing of) the CSV this command writes to
        // stdout. Same chunking/early-stop semantics, progress on stderr.
        run_loop_quiet(&mut solver, &config.run);
        let elapsed = start.elapsed();

        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        eprintln!(
            "board={board_str} iterations={} wall={:.2}s nash_conv={:.3e}",
            solver.iteration(),
            elapsed.as_secs_f64(),
            nash_conv,
        );

        let ev_oop = solver.expected_value(Player::P0);
        let ev_ip = solver.expected_value(Player::P1);

        let tree = &solver.game().tree;
        let root_node = *tree.node(0);
        let root_tag = tree.tags[0] as usize;
        let root_info = &node_info[root_tag];
        let sref = tree.storage_ref(&root_node);
        let avg_root = solver.average_strategy_at(0);
        let root_range_oop = &solver.game().root_ranges[Player::P0];
        let freqs = postflop_setup::action_frequencies(
            &avg_root,
            root_range_oop,
            sref.num_actions as usize,
            sref.num_hands as usize,
        );
        let labels = &root_info.actions;

        match &header_labels {
            None => header_labels = Some(labels.clone()),
            Some(h) => {
                if h != labels {
                    return Err(anyhow!(
                        "board {board_str:?} produced a different action tree than the first board \
                         (labels {labels:?} vs {h:?}); all boards must share the same betting structure"
                    ));
                }
            }
        }

        // Range-weighted mean of the OOP equity vector: a simple
        // weight-average over combos, not a joint-compatible-weight
        // normalization (a small simplification for reporting purposes).
        let equity = range_equity(board_cards, &solver.game().root_ranges);
        let oop_equity_vec = &equity[Player::P0];
        let total_weight: f64 = root_range_oop.iter().map(|&w| w as f64).sum();
        let oop_equity = if total_weight > 0.0 {
            root_range_oop
                .iter()
                .zip(oop_equity_vec)
                .map(|(&w, &e)| w as f64 * e as f64)
                .sum::<f64>()
                / total_weight
        } else {
            0.0
        };

        let mut row = vec![
            board_str.clone(),
            solver.iteration().to_string(),
            fmt_sig(elapsed.as_secs_f64(), 6),
            fmt_sig(nash_conv, 6),
            fmt_sig(ev_oop, 6),
            fmt_sig(ev_ip, 6),
            fmt_sig(oop_equity, 6),
        ];
        for f in &freqs {
            row.push(fmt_sig(*f, 6));
        }
        csv_rows.push(row.join(","));
    }

    let labels = header_labels.unwrap_or_default();
    let mut header_fields = vec![
        "board".to_string(),
        "iterations".to_string(),
        "wall_s".to_string(),
        "nash_conv".to_string(),
        "ev_oop".to_string(),
        "ev_ip".to_string(),
        "oop_equity".to_string(),
    ];
    for label in &labels {
        header_fields.push(format!("freq_{}", label.replace(' ', "_")));
    }

    let mut out = String::new();
    out.push_str(&header_fields.join(","));
    out.push('\n');
    for row in &csv_rows {
        out.push_str(row);
        out.push('\n');
    }

    match output {
        Some(path) => {
            std::fs::write(path, &out).with_context(|| format!("writing {}", path.display()))?;
            eprintln!("report written to {}", path.display());
        }
        None => print!("{out}"),
    }
    Ok(())
}

/// Same chunking and early-stop semantics as [`crate::solve::run_loop`], but
/// reports progress on stderr instead of stdout — see the comment at its
/// call site for why.
fn run_loop_quiet<E: TerminalEvaluator>(solver: &mut Solver<E, F32Storage>, run: &RunSection) {
    let mut remaining = run.iterations;
    while remaining > 0 {
        let chunk = run.check_every.min(remaining);
        solver.run(chunk);
        remaining -= chunk;
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        eprintln!(
            "iter={:>8} expl_p0={:.3e} expl_p1={:.3e} nash_conv={:.3e}",
            solver.iteration(),
            expl[Player::P0],
            expl[Player::P1],
            nash_conv,
        );
        if let Some(target) = run.target_nash_conv
            && nash_conv < target
        {
            eprintln!("target nash_conv {target:.3e} reached");
            break;
        }
    }
}

/// Collects raw board tokens from exactly one of `--boards`/`--boards-file`.
fn collect_board_tokens(
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
) -> Result<Vec<String>> {
    match (boards_arg, boards_file) {
        (Some(_), Some(_)) => Err(anyhow!("pass exactly one of --boards or --boards-file")),
        (None, None) => Err(anyhow!("pass exactly one of --boards or --boards-file")),
        (Some(s), None) => Ok(s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()),
        (None, Some(path)) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            Ok(text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect())
        }
    }
}

/// Normalizes a board token: a whitespace-free even-length token (e.g.
/// "Ks7h2d") is split into 2-char card chunks; anything else (e.g. "Ks 7h
/// 2d") passes through unchanged.
fn normalize_board_token(token: &str) -> String {
    if !token.is_empty() && !token.contains(char::is_whitespace) && token.len().is_multiple_of(2) {
        token
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        token.to_string()
    }
}

/// Formats `x` to `sig` significant figures (fixed-point, not exponential).
fn fmt_sig(x: f64, sig: usize) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let d = sig as i32 - 1 - x.abs().log10().floor() as i32;
    let d = d.max(0) as usize;
    format!("{:.*}", d, x)
}
