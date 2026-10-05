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
use hu_engine::{F32Storage, I16Storage, Storage};
use hu_postflop::range_equity;
use nlh::{Card, Player};

use crate::nlh_v1;

pub fn run(
    config_path: &Path,
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    crate::nlh_v1::prepare(&raw, config_path)?;
    run_nlh(&raw, config_path, boards_arg, boards_file, output)
}

fn run_nlh(
    raw: &str,
    path: &Path,
    boards_arg: Option<&str>,
    boards_file: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let mut rows = Vec::new();
    let mut labels = Vec::new();
    // Validate every replacement before solving any board. Replaying the line
    // enforces its street count and range/card compatibility for each board.
    let prepared = collect_board_tokens(boards_arg, boards_file)?
        .iter()
        .map(|token| {
            let mut doc: toml_edit::DocumentMut = raw.parse()?;
            if doc
                .get("spot")
                .is_none_or(|spot| spot.as_table_like().is_none())
            {
                return Err(
                    spot::SpotError::new(spot::Code::NLH002, "spot", "expected a table").into(),
                );
            }
            doc["spot"]["board"] = toml_edit::value(normalize_board_token(token));
            crate::nlh_v1::prepare(&doc.to_string(), path)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut hits = prepared.first().map(|p| p.estimate.rule_hits.clone());
    for p in &prepared {
        if let Some(hits) = &mut hits {
            nlh_v1::merge_rule_hits(hits, &p.estimate.rule_hits);
        }
        let row = nlh_v1::with_threads(crate::nlh_v1::threads(p)?, || {
            match p.settings.solver.storage {
                hu_postflop::input::Storage::F32 => nlh_row::<F32Storage>(p),
                hu_postflop::input::Storage::I16 => nlh_row::<I16Storage>(p),
            }
        })?;
        for label in &row.1 {
            if !labels.contains(label) {
                labels.push(label.clone());
            }
        }
        rows.push(row);
        if crate::CLI_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
    }
    if let (Some(p), Some(hits)) = (prepared.first(), hits) {
        for warning in crate::nlh_v1::warnings_for_hits(p, &hits) {
            eprintln!("warning: {warning}");
        }
    }
    let mut header = vec![
        "board".into(),
        "iterations".into(),
        "wall_s".into(),
        "nash_conv".into(),
        "ev_oop".into(),
        "ev_ip".into(),
        "oop_equity".into(),
    ];
    header.extend(
        labels
            .iter()
            .map(|label| format!("freq_{}", label.replace(' ', "_"))),
    );
    let mut out = header.join(",") + "\n";
    for (mut prefix, own_labels, freqs) in rows {
        prefix.extend(labels.iter().map(|label| {
            own_labels
                .iter()
                .position(|l| l == label)
                .map(|i| fmt_sig(freqs[i], 6))
                .unwrap_or_default()
        }));
        out.push_str(&(prefix.join(",") + "\n"));
    }
    if let Some(path) = output {
        std::fs::write(path, out)?;
    } else {
        print!("{out}");
    }
    Ok(())
}

fn nlh_row<S: Storage>(
    p: &crate::nlh_v1::Prepared,
) -> Result<(Vec<String>, Vec<String>, Vec<f64>)> {
    let start = Instant::now();
    let (solver, info) = crate::nlh_v1::query::<S>(p, None, None)?;
    let elapsed = start.elapsed();
    let tree = &solver.game().tree;
    let sref = tree.storage_ref(tree.node(0));
    let weights = &solver.game().root_ranges[Player::P0];
    let freqs = nlh_v1::action_frequencies(
        &solver.average_strategy_at(0),
        weights,
        sref.num_actions as usize,
        sref.num_hands as usize,
    );
    let equity = range_equity(&p.config.board, &solver.game().root_ranges);
    let total: f64 = weights.iter().map(|&w| w as f64).sum();
    let equity = if total > 0.0 {
        weights
            .iter()
            .zip(&equity[Player::P0])
            .map(|(&w, &e)| w as f64 * e as f64)
            .sum::<f64>()
            / total
    } else {
        0.0
    };
    let ev = nlh_v1::subgame_ev(crate::nlh_v1::solver_ev(&solver), p.payoff.ev_offset());
    let expl = solver.exploitability();
    let board = p
        .config
        .board
        .iter()
        .map(Card::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    Ok((
        vec![
            board,
            solver.iteration().to_string(),
            fmt_sig(elapsed.as_secs_f64(), 6),
            fmt_sig(expl[Player::P0] + expl[Player::P1], 6),
            fmt_sig(ev[Player::P0], 6),
            fmt_sig(ev[Player::P1], 6),
            fmt_sig(equity, 6),
        ],
        info[tree.tags[0] as usize].actions.clone(),
        freqs,
    ))
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
