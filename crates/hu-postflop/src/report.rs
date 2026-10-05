//! Multi-board P1 computation returning typed report rows.
use crate::prepare;
use crate::range_equity;
use crate::run;
use anyhow::Result;
use hu_engine::{F32Storage, I16Storage, Storage};
use nlh::{Card, Player};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
/// One solved board, with original root action labels and frequencies.
pub struct BoardRow {
    pub board: String,
    pub iterations: u64,
    pub wall_secs: f64,
    pub nash_conv: f64,
    pub ev: [f64; 2],
    pub oop_equity: f64,
    pub labels: Vec<String>,
    pub frequencies: Vec<f64>,
}
/// Validate all replacement boards, solve in order, and return numeric rows.
pub fn compute(
    raw: &str,
    path: &Path,
    boards: &[String],
    cancel: &AtomicBool,
    diagnostics: &mut dyn FnMut(String),
) -> Result<Vec<BoardRow>> {
    prepare::prepare(raw, path)?;
    let mut rows = Vec::new();
    // Validate every replacement before solving any board. Replaying the line
    // enforces its street count and range/card compatibility for each board.
    let prepared = boards
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
            doc["spot"]["board"] = toml_edit::value(token.clone());
            prepare::prepare(&doc.to_string(), path)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut hits = prepared.first().map(|p| p.estimate.rule_hits.clone());
    for p in &prepared {
        if let Some(hits) = &mut hits {
            prepare::merge_rule_hits(hits, &p.estimate.rule_hits);
        }
        let row = run::with_threads(prepare::threads(p)?, || match p.settings.solver.storage {
            crate::input::Storage::F32 => board_row::<F32Storage>(p, cancel),
            crate::input::Storage::I16 => board_row::<I16Storage>(p, cancel),
        })?;
        rows.push(row);
        if cancel.load(Ordering::SeqCst) {
            break;
        }
    }
    if let (Some(p), Some(hits)) = (prepared.first(), hits) {
        for warning in prepare::warnings_for_hits(p, &hits) {
            diagnostics(warning);
        }
    }
    Ok(rows)
}
fn board_row<S: Storage>(p: &prepare::Prepared, cancel: &AtomicBool) -> Result<BoardRow> {
    let start = Instant::now();
    let (solver, info) = run::query::<S>(p, None, None, cancel)?;
    let elapsed = start.elapsed();
    let tree = &solver.game().tree;
    let sref = tree.storage_ref(tree.node(0));
    let weights = &solver.game().root_ranges[Player::P0];
    let freqs = prepare::action_frequencies(
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
    let ev = run::subgame_ev(run::solver_ev(&solver), p.payoff.ev_offset());
    let expl = solver.exploitability();
    let board = p
        .config
        .board
        .iter()
        .map(Card::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(BoardRow {
        board,
        iterations: solver.iteration(),
        wall_secs: elapsed.as_secs_f64(),
        nash_conv: expl[Player::P0] + expl[Player::P1],
        ev: [ev[Player::P0], ev[Player::P1]],
        oop_equity: equity,
        labels: info[tree.tags[0] as usize].actions.clone(),
        frequencies: freqs,
    })
}
