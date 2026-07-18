//! Shared helpers for building a preflop trunk from a [`SolveConfig`]'s
//! `Preflop` section, mirroring `postflop_setup.rs`'s role for postflop
//! configs.

use std::path::Path;
use std::time::Instant;

use abstraction::{BlueprintArtifacts, Ehs2Abstraction, Ehs2Params};
use anyhow::{Result, anyhow};
use cards::{Chips, PerPlayer, Range, Street};
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
    // caches (see `ensure_cache_parent_dir`, shared with the bucketed
    // abstraction/artifact caches below).
    ensure_cache_parent_dir(cache);
    let start = Instant::now();
    let table = preflop::EquityTable::load_or_compute(cache);
    println!("equity: ready in {:.2}s", start.elapsed().as_secs_f64());
    table
}

/// Creates `cache`'s parent directory up front, mirroring
/// [`load_or_compute_equity_table`]'s fix: both `Ehs2Abstraction::save` and
/// `BlueprintArtifacts::save` write straight into the cache path's parent
/// without creating it, and a failed save is silently swallowed, so a
/// missing `.cache/` directory would otherwise recompute on every run
/// instead of ever landing the cache.
fn ensure_cache_parent_dir(cache: Option<&Path>) {
    if let Some(path) = cache
        && let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty())
    {
        let _ = std::fs::create_dir_all(dir);
    }
}

/// Loads the EHS² bucket abstraction (all three postflop streets) from
/// `cache` when present and valid, otherwise builds it and best-effort
/// saves it back. Prints how long the load/build took; the caller is
/// responsible for the "this may take ~10 minutes cold" warning, since that
/// framing belongs with the rest of the bucketed-model plan printout.
pub fn load_or_build_abstraction(params: Ehs2Params, cache: Option<&Path>) -> Ehs2Abstraction {
    ensure_cache_parent_dir(cache);
    let start = Instant::now();
    let abs =
        Ehs2Abstraction::load_or_build(params, &[Street::Flop, Street::Turn, Street::River], cache);
    println!(
        "abstraction: ready in {:.2}s",
        start.elapsed().as_secs_f64()
    );
    abs
}

/// Loads the derived blueprint artifacts (T1/T2/T3 transitions plus river
/// bucket-vs-bucket equity) from `cache` when present and valid, otherwise
/// builds them from `abs` and best-effort saves them back. Same
/// cache-directory and timing conventions as
/// [`load_or_build_abstraction`].
pub fn load_or_build_artifacts(abs: &Ehs2Abstraction, cache: Option<&Path>) -> BlueprintArtifacts {
    ensure_cache_parent_dir(cache);
    let start = Instant::now();
    let artifacts = BlueprintArtifacts::load_or_build(abs, cache);
    println!("artifacts: ready in {:.2}s", start.elapsed().as_secs_f64());
    artifacts
}
