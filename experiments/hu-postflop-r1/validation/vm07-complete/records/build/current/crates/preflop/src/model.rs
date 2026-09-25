//! Postflop continuation models — the Mode B leaf seam.
//!
//! A trunk terminal where both players still have chips behind hands the
//! rest of the hand to a [`PostflopModel`]. In this slice the model's
//! contract is deliberately narrow: it must express the terminal's
//! per-class-pair utility as an *affine function of all-in equity*
//! ([`TermCoef`]), which is what lets the evaluator collapse every terminal
//! to three scalars over three shared 169x169 tables. `SolvedFlopSubset`
//! and bucketed models will not fit this shape and will widen the seam when
//! they land (per the roadmap, M6 later slices) — the trait exists now so
//! the trunk builder is already written against it.

use cards::{PerPlayer, Player};
use game::{BakedPayoffs, Outcome, TerminalDescriptor};

/// Per-player utility of a terminal against class pair `(h, o)`, affine in
/// the pair's all-in equity:
///
/// ```text
/// u_p(h, o) = a + b * e_win(h, o) + c * e_tie(h, o)
/// ```
///
/// where `e_win`/`e_tie` are from the acting player's perspective
/// ([`crate::EquityTable`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TermCoef {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

/// Everything a model may consult about a continuation terminal. The baked
/// payoffs already carry rake and the utility model (ICM) — models blend
/// the endpoints instead of recomputing utilities.
pub struct ContinuationCtx<'a> {
    pub descriptor: &'a TerminalDescriptor,
    pub payoffs: &'a BakedPayoffs,
}

/// Continuation-value provider for trunk terminals where the hand goes on
/// postflop (call/check with chips behind).
pub trait PostflopModel: Send + Sync {
    /// Coefficients of player `p`'s utility at this terminal.
    fn continuation_coef(&self, ctx: &ContinuationCtx<'_>, p: Player) -> TermCoef;

    /// True when the model's pot shares sum to exactly 1 for every class
    /// pair, so a zero-sum payoff pipeline stays zero-sum end to end.
    fn preserves_zero_sum(&self) -> bool;
}

/// Baked-payoff endpoints oriented to player `p`: `(u_win, u_tie, u_lose)`
/// where "win" means `p` wins.
fn oriented(payoffs: &BakedPayoffs, p: Player) -> (f64, f64, f64) {
    let mine = payoffs.for_outcome(match p {
        Player::P0 => Outcome::WinP0,
        Player::P1 => Outcome::WinP1,
    })[p];
    let theirs = payoffs.for_outcome(match p {
        Player::P0 => Outcome::WinP1,
        Player::P1 => Outcome::WinP0,
    })[p];
    let tie = payoffs.for_outcome(Outcome::Tie)[p];
    (mine, tie, theirs)
}

/// Coefficients of a fold terminal: the outcome is deterministic, so the
/// baked payoffs put the same value on all three outcomes and equity is
/// irrelevant.
pub fn fold_coef(payoffs: &BakedPayoffs, p: Player) -> TermCoef {
    TermCoef {
        a: payoffs.for_outcome(Outcome::WinP0)[p],
        b: 0.0,
        c: 0.0,
    }
}

/// Coefficients of an all-in showdown terminal: the exact
/// `e_win * u_win + e_tie * u_tie + e_lose * u_lose` blend rewritten around
/// the lose endpoint.
pub fn showdown_coef(payoffs: &BakedPayoffs, p: Player) -> TermCoef {
    let (u_win, u_tie, u_lose) = oriented(payoffs, p);
    TermCoef {
        a: u_lose,
        b: u_win - u_lose,
        c: u_tie - u_lose,
    }
}

/// HRC-v1 grade continuation model: the terminal pot is split by raw all-in
/// equity scaled by a per-player realization factor.
///
/// Player `p`'s pot share for class pair `(h, o)` is
/// `r_p * (e_win + e_tie / 2)`; utility interpolates the baked win/lose
/// endpoints linearly in that share, which is exact for affine utility
/// models (ChipEv, heads-up ICM). With `realization == [1.0, 1.0]` this is
/// precisely "both players check the hand down", a zero-sum game.
pub struct EquityShowdown {
    pub realization: PerPlayer<f64>,
}

impl Default for EquityShowdown {
    fn default() -> Self {
        EquityShowdown {
            realization: PerPlayer::new(1.0, 1.0),
        }
    }
}

impl PostflopModel for EquityShowdown {
    fn continuation_coef(&self, ctx: &ContinuationCtx<'_>, p: Player) -> TermCoef {
        let (u_win, _, u_lose) = oriented(ctx.payoffs, p);
        let r = self.realization[p];
        let span = u_win - u_lose;
        TermCoef {
            a: u_lose,
            b: r * span,
            c: r * span / 2.0,
        }
    }

    fn preserves_zero_sum(&self) -> bool {
        self.realization[Player::P0] == 1.0 && self.realization[Player::P1] == 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cards::{Chips, Street};
    use game::{ChipEv, NoRake, PayoffPipeline, TerminalKind};

    fn baked(pot: u32, contrib: (u32, u32)) -> BakedPayoffs {
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        pipeline.bake(&TerminalDescriptor {
            kind: TerminalKind::Showdown,
            street: Street::Flop,
            pot: Chips(pot),
            contrib: PerPlayer::new(Chips(contrib.0), Chips(contrib.1)),
            stacks_before: PerPlayer::new(Chips(200), Chips(200)),
        })
    }

    #[test]
    fn showdown_coef_reproduces_exact_blend() {
        let payoffs = baked(40, (20, 20));
        for p in Player::BOTH {
            let coef = showdown_coef(&payoffs, p);
            let (u_win, u_tie, u_lose) = oriented(&payoffs, p);
            for (e_win, e_tie) in [(0.0, 0.0), (1.0, 0.0), (0.3, 0.4), (0.0, 1.0)] {
                let e_lose = 1.0 - e_win - e_tie;
                let direct = e_win * u_win + e_tie * u_tie + e_lose * u_lose;
                let via = coef.a + coef.b * e_win + coef.c * e_tie;
                assert!((direct - via).abs() < 1e-12, "p={p:?} {e_win} {e_tie}");
            }
        }
    }

    #[test]
    fn equity_showdown_unit_realization_is_zero_sum_checkdown() {
        let payoffs = baked(40, (20, 20));
        let model = EquityShowdown::default();
        assert!(model.preserves_zero_sum());
        let ctx = ContinuationCtx {
            descriptor: &TerminalDescriptor {
                kind: TerminalKind::Showdown,
                street: Street::Flop,
                pot: Chips(40),
                contrib: PerPlayer::new(Chips(20), Chips(20)),
                stacks_before: PerPlayer::new(Chips(200), Chips(200)),
            },
            payoffs: &payoffs,
        };
        // With r = 1 the continuation must equal the exact showdown blend
        // for any equity (u_tie is the win/lose midpoint under ChipEv).
        for p in Player::BOTH {
            let cont = model.continuation_coef(&ctx, p);
            let show = showdown_coef(&payoffs, p);
            for (e_win, e_tie) in [(0.5, 0.0), (0.2, 0.3), (0.0, 1.0)] {
                let a = cont.a + cont.b * e_win + cont.c * e_tie;
                let b = show.a + show.b * e_win + show.c * e_tie;
                assert!((a - b).abs() < 1e-12);
            }
        }
        // And the two players' utilities must sum to zero for any split.
        let c0 = model.continuation_coef(&ctx, Player::P0);
        let c1 = model.continuation_coef(&ctx, Player::P1);
        for (e_win0, e_tie) in [(0.7, 0.1), (0.0, 0.0), (0.25, 0.5)] {
            let e_win1 = 1.0 - e_win0 - e_tie;
            let u0 = c0.a + c0.b * e_win0 + c0.c * e_tie;
            let u1 = c1.a + c1.b * e_win1 + c1.c * e_tie;
            assert!((u0 + u1).abs() < 1e-12);
        }
    }

    #[test]
    fn non_unit_realization_breaks_zero_sum_flag() {
        let model = EquityShowdown {
            realization: PerPlayer::new(0.95, 1.05),
        };
        assert!(!model.preserves_zero_sum());
    }
}
