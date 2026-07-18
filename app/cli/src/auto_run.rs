//! Pure derivation of "auto mode" run parameters from a multiway config plus
//! caller-supplied machine facts (thread count, memory budget). No machine
//! detection happens here -- a later GUI phase probes the host and passes
//! `threads`/`memory_budget_bytes` in; this module only turns those numbers
//! (plus the config) into concrete run knobs.

use anyhow::{Context, Result};
use multiway::MultiwayConfig;

/// Bucket-count ladder searched by [`derive_auto_run`], smallest first.
const BUCKET_LADDER: [u16; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

/// Result of [`derive_auto_run`]: concrete run knobs a caller can splice
/// straight into `[run]`/`[game.abstraction]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoRunDerivation {
    pub threads: usize,
    pub sweep_batch: u64,
    pub flop_buckets: u16,
    pub turn_buckets: u16,
    pub river_buckets: u16,
    /// The dense-arena estimate ([`multiway::estimate_dense_arena`]) for the
    /// chosen bucket count. May exceed `memory_budget_bytes` when even the
    /// smallest ladder rung (`64`) does not fit; callers should compare this
    /// against their budget and warn rather than assume it always fits.
    pub estimated_bytes: u64,
}

/// Derives `[run]`/`[game.abstraction]` knobs for an "auto mode" solve of
/// `config` on a machine with `threads` worker threads and a
/// `memory_budget_bytes` dense-arena budget.
///
/// - `sweep_batch = threads.div_ceil(seats)` (minimum `1`), so one drive-loop
///   batch uses every thread even though one sweep only yields `seats`
///   parallel traversals (see `MultiwaySession`'s sweep-batch hint).
/// - Bucket counts: the largest `B` from `[64, 128, 256, 512, 1024, 2048,
///   4096]` such that [`multiway::estimate_dense_arena`] with
///   `flop = turn = river = B` (and any `active_opponent_buckets` override
///   cleared, since this derivation picks one uniform count) fits
///   `memory_budget_bytes`. If even `64` does not fit, `64` is returned
///   anyway together with its (over-budget) `estimated_bytes`, so the caller
///   can surface a warning instead of the derivation simply failing.
///
/// Pure function of its inputs: calling it twice with the same arguments
/// (and an unmodified `config`) always returns the same result.
pub fn derive_auto_run(
    config: &MultiwayConfig,
    threads: usize,
    memory_budget_bytes: u64,
) -> Result<AutoRunDerivation> {
    let seats = config.seats.len().max(1) as u64;
    let sweep_batch = (threads as u64).div_ceil(seats).max(1);

    let mut fits: Option<(u16, u64)> = None;
    let mut smallest: Option<(u16, u64)> = None;
    for &bucket in &BUCKET_LADDER {
        let mut trial = config.clone();
        trial.abstraction.flop_buckets = bucket;
        trial.abstraction.turn_buckets = bucket;
        trial.abstraction.river_buckets = bucket;
        trial.abstraction.active_opponent_buckets.clear();
        let estimate = multiway::estimate_dense_arena(&trial).with_context(|| {
            format!("estimating dense-arena size for a uniform {bucket}-bucket abstraction")
        })?;
        if smallest.is_none() {
            smallest = Some((bucket, estimate.estimated_bytes));
        }
        if estimate.estimated_bytes <= memory_budget_bytes {
            fits = Some((bucket, estimate.estimated_bytes));
        } else {
            // Bucket count only grows the arena (same betting tree, more
            // columns/slots per node), so once a rung overflows the budget
            // every larger rung would too.
            break;
        }
    }
    let (buckets, estimated_bytes) = fits
        .or(smallest)
        .expect("BUCKET_LADDER is a non-empty constant");

    Ok(AutoRunDerivation {
        threads,
        sweep_batch,
        flop_buckets: buckets,
        turn_buckets: buckets,
        river_buckets: buckets,
        estimated_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiway::config::{AnteConfig, BettingConfig, BlindConfig, SeatConfig};
    use multiway::types::SeatId;

    fn tiny_config() -> MultiwayConfig {
        MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 10.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: multiway::AbstractionConfig::default(),
        }
    }

    #[test]
    fn sweep_batch_uses_ceil_division_and_a_floor_of_one() {
        let config = tiny_config();
        let derivation = derive_auto_run(&config, 8, 1024 * 1024 * 1024).unwrap();
        // 3 seats, 8 threads -> ceil(8/3) = 3.
        assert_eq!(derivation.sweep_batch, 3);

        let derivation = derive_auto_run(&config, 0, 1024 * 1024 * 1024).unwrap();
        assert_eq!(derivation.sweep_batch, 1);
    }

    #[test]
    fn tiny_config_picks_the_largest_ladder_rung_under_a_huge_budget() {
        let config = tiny_config();
        let derivation = derive_auto_run(&config, 4, u64::MAX).unwrap();
        assert_eq!(derivation.flop_buckets, 4096);
        assert_eq!(derivation.turn_buckets, 4096);
        assert_eq!(derivation.river_buckets, 4096);
    }

    #[test]
    fn bigger_budget_never_picks_a_smaller_bucket_ladder_rung() {
        let config = tiny_config();
        let budgets = [
            1024,
            64 * 1024,
            1024 * 1024,
            16 * 1024 * 1024,
            256 * 1024 * 1024,
            u64::MAX,
        ];
        let mut previous = 0u16;
        for budget in budgets {
            let derivation = derive_auto_run(&config, 4, budget).unwrap();
            assert!(
                derivation.flop_buckets >= previous,
                "budget {budget}: bucket {} regressed below previous {previous}",
                derivation.flop_buckets
            );
            previous = derivation.flop_buckets;
        }
        assert_eq!(
            previous, 4096,
            "the largest budget must reach the ladder ceiling"
        );
    }

    #[test]
    fn checkdown_style_config_fits_far_more_buckets_than_the_full_tree_config() {
        let full_tree = tiny_config();
        let mut checkdown = full_tree.clone();
        checkdown.betting.flop.max_betting_players = Some(1);
        checkdown.betting.turn.max_betting_players = Some(1);
        checkdown.betting.river.max_betting_players = Some(1);

        // Pick a budget tight enough that the full-tree config cannot reach
        // the ladder ceiling, so there is room for the checkdown config to
        // beat it.
        let budget = {
            let full_tree_4096 = {
                let mut trial = full_tree.clone();
                trial.abstraction.flop_buckets = 4096;
                trial.abstraction.turn_buckets = 4096;
                trial.abstraction.river_buckets = 4096;
                multiway::estimate_dense_arena(&trial).unwrap()
            };
            full_tree_4096.estimated_bytes / 4
        };

        let full_tree_derivation = derive_auto_run(&full_tree, 4, budget).unwrap();
        let checkdown_derivation = derive_auto_run(&checkdown, 4, budget).unwrap();
        assert!(checkdown_derivation.flop_buckets > full_tree_derivation.flop_buckets);
    }

    #[test]
    fn even_the_smallest_rung_over_budget_still_returns_ok_with_a_flag_estimate() {
        let config = tiny_config();
        let derivation = derive_auto_run(&config, 4, 1).unwrap();
        assert_eq!(derivation.flop_buckets, 64);
        assert!(derivation.estimated_bytes > 1);
    }
}
