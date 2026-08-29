//! Shared helpers for building a postflop subgame from a [`SolveConfig`],
//! used by `solve`, `inspect`, and `report` so there is exactly one
//! implementation of board/range parsing, discount-schedule construction,
//! and the memory-estimate preflight line. The rake and utility models live
//! in [`crate::economics`], which owns the two that are shared with the
//! sampled multiway engine.

use anyhow::{Result, anyhow};
use cards::{Card, Chips, PerPlayer, Player, Range};
use engine::{CfrPlus, Dcfr, DiscountSchedule, HsDcfr, Vanilla, linear_cfr};
use game::UtilityModel;
use holdem::{MemoryEstimate, PerStreet, PostflopConfig, StreetTree};

use crate::config::AlgorithmSection;

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

/// Parses `[game] preflop_aggressor` (`"oop"` / `"ip"` / `"none"`) into the
/// side [`PostflopConfig::preflop_aggressor`] wants. `"oop"` is
/// [`Player::P0`], matching this module's own `PerPlayer::new(oop, ip)`
/// convention below.
pub fn parse_preflop_aggressor(value: &str) -> Result<Option<Player>> {
    match value {
        "none" => Ok(None),
        "oop" => Ok(Some(Player::P0)),
        "ip" => Ok(Some(Player::P1)),
        other => Err(anyhow!(
            "unknown preflop_aggressor {other:?}; expected \"oop\", \"ip\", or \"none\""
        )),
    }
}

/// Builds a [`PostflopConfig`] from the raw config fields. `streets` and
/// `min_bet` are handed over already resolved to `holdem`'s new
/// `StreetTree`-based grammar (TOML `[game.tree]` parsing lives in
/// `crate::config`/`crate::solver_config_v1`, which construct `StreetTree`
/// directly rather than an intermediate `bets`-section shape) — this
/// function only owns board/range parsing and the `PostflopConfig` literal.
#[allow(clippy::too_many_arguments)]
pub fn build_postflop_config(
    board: &str,
    oop_range: &str,
    ip_range: &str,
    pot: u32,
    effective_stack: u32,
    iso_merging: bool,
    min_bet: u32,
    streets: PerStreet<StreetTree>,
    preflop_aggressor: &str,
) -> Result<PostflopConfig> {
    let board = parse_board(board)?;
    let oop = parse_range("oop_range", oop_range)?;
    let ip = parse_range("ip_range", ip_range)?;
    let ranges = PerPlayer::new(oop, ip);
    let preflop_aggressor = parse_preflop_aggressor(preflop_aggressor)?;

    Ok(PostflopConfig {
        board,
        ranges,
        pot: Chips(pot),
        effective_stack: Chips(effective_stack),
        streets,
        min_bet: Chips(min_bet),
        iso_merging,
        track_node_info: true,
        preflop_aggressor,
    })
}

/// The constant that re-bases a postflop solver's root values on the start
/// of the subgame.
///
/// The solve measures utility from before the pot was built, which is what
/// keeps an unraked chip-EV game exactly zero-sum. This offset moves the
/// zero point to the root of the subgame — both players holding only the
/// chips behind them, with the pot dead on the table — so the reported
/// number answers "what does this player take out of this spot, net of what
/// they still have to put in". That is the basis PioSOLVER and GTO Wizard
/// report and the one `docs/validation/gto-wizard-validation.md` compares
/// against.
///
/// It goes through the utility model rather than adding chips directly,
/// because under ICM the solver's values are prize units and adding a chip
/// count to them would be a unit error. Under chip EV the model is the
/// identity and this reduces to "add your slice of the starting pot".
///
/// The internal split of the starting pot cancels either way: the solver
/// value carries `-utility(stacks_before)` and this adds it straight back,
/// leaving `utility(stacks_after) - utility(both players' behind stacks)`.
/// So the reported EV never depends on which player was credited with an
/// odd pot's extra chip.
pub fn subgame_ev_offset(config: &PostflopConfig, utility: &dyn UtilityModel) -> PerPlayer<f64> {
    let behind = config.effective_stack.as_f64();
    let before = PerPlayer::new(
        behind + config.starting_share(Player::P0).as_f64(),
        behind + config.starting_share(Player::P1).as_f64(),
    );
    let solve_baseline = utility.utility(&before);
    let subgame_baseline = utility.utility(&PerPlayer::new(behind, behind));
    PerPlayer::new(
        solve_baseline[Player::P0] - subgame_baseline[Player::P0],
        solve_baseline[Player::P1] - subgame_baseline[Player::P1],
    )
}

/// Applies [`subgame_ev_offset`] to a solver's root values.
///
/// Under chip EV without rake the two results sum to the starting pot; with
/// rake, to the pot less the expected rake. They never sum to zero.
pub fn subgame_ev(solver_ev: PerPlayer<f64>, offset: PerPlayer<f64>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver_ev[Player::P0] + offset[Player::P0],
        solver_ev[Player::P1] + offset[Player::P1],
    )
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

/// Reach-weighted overall frequency of each action at an action node: for
/// action `a`, `sum_combo(weight[combo] * avg_strategy[a][combo]) /
/// sum_combo(weight[combo])`.
///
/// `weight` is the acting player's reach *at that node* (see
/// `engine::reach_at`), not their root range. The two agree at the root and
/// diverge below it: a hand that folded upstream, or that card removal has
/// made impossible, still carries root weight but no reach, and counting it
/// would report a frequency over hands that could not be there.
///
/// Returns one frequency per action, and `0.0` for every action when the
/// node is unreachable (total weight zero).
pub fn action_frequencies(
    avg_strategy: &[f32],
    weight: &[f32],
    num_actions: usize,
    num_hands: usize,
) -> Vec<f64> {
    let total: f64 = weight.iter().map(|&w| w as f64).sum();
    if total <= 0.0 {
        return vec![0.0; num_actions];
    }
    (0..num_actions)
        .map(|a| {
            let row = &avg_strategy[a * num_hands..(a + 1) * num_hands];
            let sum: f64 = weight
                .iter()
                .zip(row)
                .map(|(&w, &s)| w as f64 * s as f64)
                .sum();
            sum / total
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use game::{ChipEv, Icm};
    use holdem::PerStreet;

    fn config(pot: u32) -> PostflopConfig {
        PostflopConfig {
            pot: Chips(pot),
            effective_stack: Chips(100),
            streets: PerStreet::default(),
            ..PostflopConfig::default()
        }
    }

    /// Under chip EV the re-basing is exactly "add your slice of the
    /// starting pot", and the two slices are the whole pot.
    #[test]
    fn chip_ev_offset_is_the_starting_pot_split() {
        for pot in [10, 11] {
            let offset = subgame_ev_offset(&config(pot), &ChipEv);
            assert_eq!(offset[Player::P0], (pot / 2) as f64);
            assert_eq!(offset[Player::P1], (pot - pot / 2) as f64);
            assert_eq!(offset[Player::P0] + offset[Player::P1], pot as f64);
        }
    }

    /// The reported EV must not depend on which player was credited with an
    /// odd pot's extra chip: the solver value carries `-share` and the
    /// offset adds it straight back. Simulated here by checking that a
    /// solver value expressed relative to either split re-bases to the same
    /// number.
    #[test]
    fn the_starting_split_cancels_out_of_the_reported_ev() {
        let config = config(11);
        let offset = subgame_ev_offset(&config, &ChipEv);
        // "OOP takes the whole pot, invests nothing more": the solver value
        // is the pot minus OOP's own share, whichever share that is.
        let solver_ev = PerPlayer::new(11.0 - offset[Player::P0], -(11.0 - offset[Player::P0]));
        let reported = subgame_ev(solver_ev, offset);
        assert!((reported[Player::P0] - 11.0).abs() < 1e-12);
        assert!((reported[Player::P1] - 0.0).abs() < 1e-12);
    }

    /// The offset goes through the utility model, so under ICM it is a
    /// difference of prize-unit values, never a chip count. Two-player ICM
    /// depends only on the stack ratio, so crediting both players with a
    /// symmetric slice of the pot changes nothing and the offset is zero —
    /// which is exactly the reading that catches a regression to "add the
    /// chips", where it would be the pot split instead.
    #[test]
    fn the_two_player_icm_offset_is_zero_not_a_chip_count() {
        let icm = Icm {
            payouts: [100.0, 60.0],
        };
        let offset = subgame_ev_offset(&config(20), &icm);
        assert!(offset[Player::P0].abs() < 1e-12, "{offset:?}");
        assert!(offset[Player::P1].abs() < 1e-12, "{offset:?}");
    }

    /// With an outside field the pair's own stacks are no longer the whole
    /// tournament, so the pot does move their equity and the offset is
    /// nonzero — but still in prize units, orders of magnitude away from
    /// the chip split it would be under a unit error.
    #[test]
    fn the_tournament_icm_offset_is_nonzero_but_still_prize_units() {
        let icm = crate::economics::TournamentIcm::new(
            vec![1000.0, 600.0, 400.0],
            vec![300.0, 400.0],
            100_000,
            0,
        )
        .expect("valid tournament ICM");
        let offset = subgame_ev_offset(&config(20), &icm);
        assert!(offset[Player::P0] > 0.0, "{offset:?}");
        assert!(
            offset[Player::P0] < 20.0,
            "a prize-unit offset must not look like the chip split: {offset:?}"
        );
        assert!(
            (offset[Player::P0] - offset[Player::P1]).abs() < 1e-9,
            "an even pot splits symmetrically: {offset:?}"
        );
    }
}
