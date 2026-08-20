//! Shared helpers for building a postflop subgame from a [`SolveConfig`],
//! used by `solve`, `inspect`, and `report` so there is exactly one
//! implementation of board/range parsing, rake/utility/schedule
//! construction, and the memory-estimate preflight line.

use anyhow::{Result, anyhow};
use cards::{Card, Chips, PerPlayer, Range};
use engine::{CfrPlus, Dcfr, DiscountSchedule, HsDcfr, Vanilla, linear_cfr};
use game::{ChipEv, GgPreflopRake, Icm, NoRake, PercentCapRake, RakeModel, UtilityModel};
use holdem::{MemoryEstimate, PerStreet, PostflopConfig};

use crate::config::{AlgorithmSection, BetsSection, RakeSection, UtilitySection};

/// Parses a whitespace-separated board string ("Ks 7h 2d") into cards.
pub fn parse_board(board: &str) -> Result<Vec<Card>> {
    board
        .split_whitespace()
        .map(|token| {
            token
                .parse::<Card>()
                .map_err(|_| anyhow!("invalid card {token:?} in board (expected e.g. \"Ks\")"))
        })
        .collect()
}

/// Parses a range string with a label for error messages
/// (`"oop_range"`/`"ip_range"`).
pub fn parse_range(label: &str, spec: &str) -> Result<Range> {
    spec.parse::<Range>()
        .map_err(|e| anyhow!("parsing {label} {spec:?}: {e}"))
}

/// Builds a [`PostflopConfig`] from the raw config fields.
#[allow(clippy::too_many_arguments)]
pub fn build_postflop_config(
    board: &str,
    oop_range: &str,
    ip_range: &str,
    pot: u32,
    effective_stack: u32,
    iso_merging: bool,
    bets: BetsSection,
) -> Result<PostflopConfig> {
    let board = parse_board(board)?;
    let oop = parse_range("oop_range", oop_range)?;
    let ip = parse_range("ip_range", ip_range)?;
    let ranges = PerPlayer::new(oop, ip);

    // Raise sizes fall back to the matching bet sizes when the TOML omits
    // `oop_raise`/`ip_raise`, so pre-existing configs keep the classic
    // shared-size tree unchanged.
    let raise_fractions = PerStreet {
        flop: PerPlayer::new(
            bets.flop
                .oop_raise
                .clone()
                .unwrap_or_else(|| bets.flop.oop.clone()),
            bets.flop
                .ip_raise
                .clone()
                .unwrap_or_else(|| bets.flop.ip.clone()),
        ),
        turn: PerPlayer::new(
            bets.turn
                .oop_raise
                .clone()
                .unwrap_or_else(|| bets.turn.oop.clone()),
            bets.turn
                .ip_raise
                .clone()
                .unwrap_or_else(|| bets.turn.ip.clone()),
        ),
        river: PerPlayer::new(
            bets.river
                .oop_raise
                .clone()
                .unwrap_or_else(|| bets.river.oop.clone()),
            bets.river
                .ip_raise
                .clone()
                .unwrap_or_else(|| bets.river.ip.clone()),
        ),
    };
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

    Ok(PostflopConfig {
        board,
        ranges,
        pot: Chips(pot),
        effective_stack: Chips(effective_stack),
        bet_fractions,
        raise_fractions,
        max_raises,
        iso_merging,
        track_node_info: true,
    })
}

/// Builds the rake trait object from its config section.
pub fn build_rake(rake: &RakeSection) -> Box<dyn RakeModel> {
    match rake {
        RakeSection::None => Box::new(NoRake),
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => Box::new(PercentCapRake {
            rate: *rate,
            cap: *cap,
            no_flop_no_drop: *no_flop_no_drop,
        }),
        RakeSection::Generic { .. } => panic!("generic rake is only valid in Multiway Preflop v1"),
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => Box::new(GgPreflopRake {
            rate: *rate,
            cap: *cap,
            exempt_pot: Chips(*exempt_pot),
        }),
    }
}

/// Builds the utility trait object from its config section.
pub fn build_utility(utility: &UtilitySection) -> Box<dyn UtilityModel> {
    match utility {
        UtilitySection::ChipEv => Box::new(ChipEv),
        UtilitySection::Icm { payouts } => Box::new(Icm { payouts: *payouts }),
        UtilitySection::TournamentIcm { .. } => {
            unreachable!("tournament ICM is handled by the multiway solve path")
        }
    }
}

/// Builds the discount schedule from its config section. Matches by
/// reference so callers that solve repeatedly (e.g. `report`, once per
/// board) can call this fresh each time from the same `&AlgorithmSection`.
pub fn build_schedule(algorithm: &AlgorithmSection) -> Box<dyn DiscountSchedule> {
    match algorithm {
        AlgorithmSection::Vanilla => Box::new(Vanilla),
        AlgorithmSection::CfrPlus => Box::new(CfrPlus),
        AlgorithmSection::Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        } => Box::new(Dcfr {
            alpha: *alpha,
            beta: *beta,
            gamma: *gamma,
            pow4_reset: *pow4_reset,
        }),
        AlgorithmSection::LinearCfr => Box::new(linear_cfr()),
        AlgorithmSection::HsDcfr { gamma0 } => Box::new(HsDcfr { gamma0: *gamma0 }),
        AlgorithmSection::ExternalSamplingMccfr { .. } => {
            unreachable!("external-sampling MCCFR is handled by the multiway solve path")
        }
    }
}

/// Prints the tree-size preflight line before committing to a (possibly
/// very large) real build.
pub fn print_memory_estimate(estimate: MemoryEstimate) {
    println!(
        "tree: nodes={} terminals={} rank_tables={} storage={:.1} MiB (f32) / {:.1} MiB (i16)",
        estimate.nodes,
        estimate.terminals,
        estimate.rank_tables,
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_bytes as f64 / (1024.0 * 1024.0),
    );
}

/// Range-weighted overall frequency of each action at an action node: for
/// action `a`, `sum_combo(root_range[combo] * avg_strategy[a][combo]) /
/// sum_combo(root_range[combo])`, using the acting player's ROOT range
/// weights (not the node's actual reach, which may differ deeper in the
/// tree after card removal — this is a deliberate simplification for
/// reporting purposes, not a solve-path quantity). Returns one frequency per
/// action, `0.0` for every action if the root range has zero total weight.
pub fn action_frequencies(
    avg_strategy: &[f32],
    root_range: &[f32],
    num_actions: usize,
    num_hands: usize,
) -> Vec<f64> {
    let total: f64 = root_range.iter().map(|&w| w as f64).sum();
    if total <= 0.0 {
        return vec![0.0; num_actions];
    }
    (0..num_actions)
        .map(|a| {
            let row = &avg_strategy[a * num_hands..(a + 1) * num_hands];
            let sum: f64 = root_range
                .iter()
                .zip(row)
                .map(|(&w, &s)| w as f64 * s as f64)
                .sum();
            sum / total
        })
        .collect()
}
