//! Mode B: heads-up preflop solving on a lossless 169-class trunk.
//!
//! The preflop trunk is an ordinary betting tree compiled through the same
//! `game` payoff pipeline and `engine` solver as every other variant. What
//! makes it "Mode B" is the private-state space: instead of 1,326 combos,
//! reach vectors are indexed by the 169 preflop hand classes (pairs, suited,
//! offsuit), which is lossless preflop because every combo in a class is
//! strategically identical before any board card is dealt.
//!
//! # Class-level vectorization (derivation)
//!
//! Ground truth at combo level: `CFV_p(c_h) = Σ_{c_o disjoint from c_h}
//! reach_o(c_o) · u_p(c_h, c_o)`. With per-combo weights uniform within a
//! class (the lossless case), average over the combos `c_h` of class `h`:
//!
//! ```text
//! out[h] = Σ_o opp_reach[o] · compat(h, o) · ū_p(h, o)
//! ```
//!
//! where, with `n_h` the number of combos in class `h` and `N(h, o)` the
//! number of ordered card-disjoint combo pairs between the classes:
//!
//! - `opp_reach[o]` is the class **mass** (sum of per-combo weights), so the
//!   root range for a full class is `n_h`, not 1;
//! - `compat(h, o) = N(h, o) / (n_h · n_o)` is the probability that combos
//!   drawn from each class don't collide;
//! - `ū_p(h, o)` is the utility averaged over the disjoint combo pairs.
//!
//! The game normalizer is `Σ_{h,o} R0(h) · R1(o) · compat(h, o)`, which for
//! full ranges equals the combo-level count of ordered disjoint pairs,
//! `1326 · 1225 = 1,624,350`.
//!
//! Every terminal's `ū_p` is affine in the pair's all-in equity:
//! `ū_p(h, o) = a + b · e_win(h, o) + c · e_tie(h, o)` (see
//! [`model::TermCoef`]). Folds have `b = c = 0`; all-in showdowns and
//! equity-realization continuations pick `a, b, c` from the baked payoff
//! endpoints, which is exact because both shipped utility models (ChipEv,
//! heads-up ICM) are affine in stacks. The evaluator therefore stores just
//! three shared 169x169 tables (`compat`, `compat·e_win`, `compat·e_tie`)
//! plus three scalars per terminal per player.
//!
//! # Conventions
//!
//! - [`Player::P0`] is the small blind / button (acts first preflop),
//!   [`Player::P1`] is the big blind.
//! - Chips are denominated so that one big blind is [`CHIPS_PER_BB`] chips,
//!   giving 0.1 bb sizing granularity.
//!
//! The postflop continuation seam is [`model::PostflopModel`]; the only
//! implementation in this slice is [`model::EquityShowdown`] (HRC-v1 grade:
//! terminal pot split by raw all-in equity times an optional realization
//! factor). `SolvedFlopSubset` and bucketed MCCFR models will extend the
//! seam in later M6 slices.

mod classes;
mod equity;
mod evaluator;
mod model;
mod trunk;

pub use classes::{
    class_combo_counts, class_combos, class_label, class_mass, compat_counts, total_disjoint_pairs,
};
pub use equity::{EQUITY_BOARDS_PER_PAIR, EquityCacheError, EquityTable};
pub use evaluator::PreflopEvaluator;
pub use model::{
    ContinuationCtx, EquityShowdown, PostflopModel, TermCoef, fold_coef, showdown_coef,
};
pub use trunk::{
    MemoryEstimate, PreflopConfig, PreflopGame, PreflopNodeInfo, build_preflop_game, memory_usage,
};

/// Chips per big blind. Sizes given in big blinds are rounded to this
/// 0.1 bb grid when converted to [`cards::Chips`].
pub const CHIPS_PER_BB: u32 = 10;
