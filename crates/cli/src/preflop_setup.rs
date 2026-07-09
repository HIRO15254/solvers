//! Shared helpers for building a preflop trunk from a [`SolveConfig`]'s
//! `Preflop` section, mirroring `postflop_setup.rs`'s role for postflop
//! configs.

use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow};
use cards::{Chips, PerPlayer, Range};
use preflop::{MemoryEstimate, PreflopConfig};

/// Parses an optional preflop range spec; `None` is the full range.
fn parse_preflop_range(label: &str, spec: Option<&str>) -> Result<Range> {
    match spec {
        Some(s) => s
            .parse::<Range>()
            .map_err(|e| anyhow!("parsing {label} {s:?}: {e}")),
        None => Ok(Range::full()),
    }
}

/// Builds a [`PreflopConfig`] from the raw config fields, converting big-blind
/// sizes to chips (`bb = CHIPS_PER_BB`, others rounded to the same 0.1 bb
/// grid).
#[allow(clippy::too_many_arguments)]
pub fn build_preflop_config(
    effective_stack_bb: f64,
    sb_bb: f64,
    open_sizes_bb: Vec<f64>,
    raise_factors: Vec<Vec<f64>>,
    max_raises: u32,
    include_allin: bool,
    allow_limp: bool,
    sb_range: Option<&str>,
    bb_range: Option<&str>,
) -> Result<PreflopConfig> {
    let sb_range = parse_preflop_range("sb_range", sb_range)?;
    let bb_range = parse_preflop_range("bb_range", bb_range)?;
    let to_chips = |bb: f64| Chips((bb * preflop::CHIPS_PER_BB as f64).round() as u32);

    Ok(PreflopConfig {
        effective_stack: to_chips(effective_stack_bb),
        sb: to_chips(sb_bb),
        bb: Chips(preflop::CHIPS_PER_BB),
        ranges: PerPlayer::new(sb_range, bb_range),
        open_sizes_bb,
        raise_factors,
        max_raises,
        include_allin,
        allow_limp,
        track_node_info: true,
    })
}

/// Prints the tree-size preflight line before committing to a (possibly
/// large) real build.
pub fn print_memory_estimate(estimate: MemoryEstimate) {
    println!(
        "tree: nodes={} terminals={} storage={:.1} MiB (f32) / {:.1} MiB (i16)",
        estimate.nodes,
        estimate.terminals,
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_bytes as f64 / (1024.0 * 1024.0),
    );
}

/// Loads the exact 169x169 equity table from `cache` when present and valid,
/// otherwise computes it (printing a notice first, since the full enumeration
/// takes about a minute in release mode) and best-effort saves it back.
pub fn load_or_compute_equity_table(cache: Option<&Path>) -> preflop::EquityTable {
    if let Some(path) = cache
        && let Ok(table) = preflop::EquityTable::load(path)
    {
        println!("equity: loaded cached table from {}", path.display());
        return table;
    }
    println!("equity: computing exact 169x169-class table (~1 minute in release mode)...");
    // `EquityTable::save` writes straight into the cache path's parent
    // directory without creating it, and a failed save is silently
    // swallowed (by design, so a read-only cache dir never fails the solve)
    // -- so a missing parent directory (e.g. a fresh checkout's un-created
    // `.cache/`) would otherwise recompute the table on every run instead of
    // ever landing the cache. Create it up front so the common case actually
    // caches.
    if let Some(path) = cache
        && let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty())
    {
        let _ = std::fs::create_dir_all(dir);
    }
    let start = Instant::now();
    let table = preflop::EquityTable::load_or_compute(cache);
    println!("equity: ready in {:.2}s", start.elapsed().as_secs_f64());
    table
}
