//! `report`: solve the same postflop config across multiple boards and
//! write a CSV report (one row per board).
//!
//! The original design sketch restricted this to 3-card (flop) boards via
//! `--flops`/`--flops-file` flags. That's changed here to `--boards`/
//! `--boards-file`, accepting boards of any starting-street length (3, 4,
//! or 5 cards): a fast debug-build integration test needs river-only (5
//! card) boards, which solve in seconds even in a debug build, whereas flop
//! boards take minutes.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use cards::{Card, Player};
use engine::{F32Storage, I16Storage, Solver, Storage};
use game::PayoffPipeline;
use holdem::range_equity;

use crate::config::{GameSection, SolveConfig, StorageKind};
use crate::postflop_setup;

pub fn run(
    config_path: &Path,
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config =
        crate::config::parse_solve_config_at(&raw, config_path).context("parsing config")?;

    postflop_setup::with_threads(config.run.threads, || match config.run.storage {
        StorageKind::F32 => run_config::<F32Storage>(config, boards_arg, boards_file, output),
        StorageKind::I16 => run_config::<I16Storage>(config, boards_arg, boards_file, output),
    })
}

fn run_config<S: Storage>(
    config: SolveConfig,
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let (oop_range, ip_range, pot, effective_stack, iso_merging, min_bet, preflop_aggressor, tree) =
        match config.game {
            GameSection::Postflop {
                board,
                oop_range,
                ip_range,
                pot,
                effective_stack,
                iso_merging,
                min_bet,
                preflop_aggressor,
                tree,
            } => {
                eprintln!("note: config board {board:?} ignored; using --boards");
                (
                    oop_range,
                    ip_range,
                    pot,
                    effective_stack,
                    iso_merging,
                    min_bet,
                    preflop_aggressor,
                    tree,
                )
            }
            GameSection::Preflop { .. } => {
                return Err(anyhow!(
                    "report does not support preflop configs yet (kind = \"preflop\")"
                ));
            }
            _ => {
                return Err(anyhow!(
                    "report only supports schema = \"solvers.postflop/v1\" configs"
                ));
            }
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

    let rake = crate::economics::build_rake(&config.rake)?;
    let utility = crate::economics::build_utility(&config.utility)?;

    // The header is the union of every board's root action labels, not one
    // fixed menu: a script's board predicate (`when paired { ... }`) can
    // legitimately give two boards different root actions, and sweeping
    // boards is `report`'s whole job. `union_labels` collects that union in
    // first-seen order -- the order boards were given in `--boards`/
    // `--boards-file`, which is the user's own input order and therefore
    // stable and reproducible across runs over the same board list -- rather
    // than, say, sorting alphabetically, which would scramble a
    // deliberately-ordered menu like "check, bet 33, bet 75". Each board's
    // per-label frequencies are kept in a map and only projected onto
    // `union_labels` once every board has been solved and the union is
    // final; a board missing a label in the union gets an empty cell there,
    // not `0` -- "this action did not exist here" and "this action existed
    // and was never taken" are different facts.
    let mut union_labels: Vec<String> = Vec::new();
    let mut seen_labels: HashSet<String> = HashSet::new();
    let mut board_rows: Vec<(Vec<String>, HashMap<String, String>)> = Vec::new();

    // A rule's dead-rule warning (see `postflop_setup::warn_unmatched_rules`)
    // is accumulated OR-wise across every board in the sweep, then reported
    // once at the end -- not per board, which would be noise for a rule that
    // legitimately only matches some boards (a paired-board-only rule on a
    // sweep of unpaired boards, say). `tree_streets` is the same
    // `PerStreet<StreetTree>` on every board (only the board itself varies
    // per iteration), so any one board's copy names every rule correctly.
    let mut rule_hits_acc: Option<holdem::RuleHits> = None;
    let mut tree_streets: Option<holdem::PerStreet<holdem::StreetTree>> = None;

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
            min_bet,
            tree.lower()?,
            &preflop_aggressor,
        )?;

        if tree_streets.is_none() {
            tree_streets = Some(pf_config.streets.clone());
        }

        let estimate = holdem::memory_usage(&pf_config);
        match rule_hits_acc.as_mut() {
            Some(acc) => postflop_setup::merge_rule_hits(acc, &estimate.rule_hits),
            None => rule_hits_acc = Some(estimate.rule_hits.clone()),
        }
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
        let mut solver = Solver::<_, S>::new(pf_game.game, schedule, Some(config.run.iterations));

        let start = Instant::now();
        // Keep convergence progress on stderr so stdout remains valid CSV.
        postflop_setup::configure_solver(&mut solver, &config.run);
        let mut hooks = crate::solve::RunHooks::none();
        hooks.quiet = true;
        hooks.cancel = Some(&crate::CLI_CANCEL);
        crate::solve::run_loop(&mut solver, &config.run, &mut hooks)?;
        let elapsed = start.elapsed();

        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        eprintln!(
            "board={board_str} iterations={} wall={:.2}s nash_conv={:.3e}",
            solver.iteration(),
            elapsed.as_secs_f64(),
            nash_conv,
        );

        let ev = postflop_setup::subgame_ev(
            crate::solve::solver_ev(&solver),
            postflop_setup::subgame_ev_offset(&pf_config, utility.as_ref()),
        );
        let (ev_oop, ev_ip) = (ev[Player::P0], ev[Player::P1]);

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
        for label in labels {
            if seen_labels.insert(label.clone()) {
                union_labels.push(label.clone());
            }
        }
        let freq_by_label: HashMap<String, String> = labels
            .iter()
            .zip(&freqs)
            .map(|(label, f)| (label.clone(), fmt_sig(*f, 6)))
            .collect();

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

        let prefix = vec![
            board_str.clone(),
            solver.iteration().to_string(),
            fmt_sig(elapsed.as_secs_f64(), 6),
            fmt_sig(nash_conv, 6),
            fmt_sig(ev_oop, 6),
            fmt_sig(ev_ip, 6),
            fmt_sig(oop_equity, 6),
        ];
        board_rows.push((prefix, freq_by_label));
        if hooks.canceled {
            break;
        }
    }

    // Once, after every board -- see the accumulator's own doc comment
    // above the loop for why per-board would be noise.
    if let (Some(streets), Some(rule_hits)) = (&tree_streets, &rule_hits_acc) {
        postflop_setup::warn_unmatched_rules(streets, rule_hits);
    }

    let mut header_fields = vec![
        "board".to_string(),
        "iterations".to_string(),
        "wall_s".to_string(),
        "nash_conv".to_string(),
        "ev_oop".to_string(),
        "ev_ip".to_string(),
        "oop_equity".to_string(),
    ];
    for label in &union_labels {
        header_fields.push(format!("freq_{}", label.replace(' ', "_")));
    }

    let mut out = String::new();
    out.push_str(&header_fields.join(","));
    out.push('\n');
    for (prefix, freq_by_label) in &board_rows {
        let mut row = prefix.clone();
        // A label absent from this board's own action list -- because a
        // script's board predicate gave a different menu here than on some
        // other board -- gets an empty cell, not `0`: this board never had
        // the action to take, as opposed to having it and never taking it.
        for label in &union_labels {
            row.push(freq_by_label.get(label).cloned().unwrap_or_default());
        }
        out.push_str(&row.join(","));
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
