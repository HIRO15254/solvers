//! Variant-independent game layer.
//!
//! Home of the terminal payoff pipeline — the seam through which rake
//! models and ICM/payout structures plug in without touching the engine —
//! plus test-support toy games (Kuhn, Leduc) compiled through the production
//! tree-building path so the correctness harness exercises real code. Toy
//! games and the percentage/cap fixture rake require `test-support` outside
//! unit tests.

mod payoff;
#[cfg(any(test, feature = "test-support"))]
mod toy;

#[cfg(any(test, feature = "test-support"))]
pub use payoff::PercentCapRake;
pub use payoff::{
    BakedPayoffs, ChipEv, Icm, NoRake, Outcome, PayoffPipeline, RakeModel, TerminalDescriptor,
    TerminalKind, UtilityModel,
};
#[cfg(any(test, feature = "test-support"))]
pub use toy::{
    RoundSpec, ToyEvaluator, ToyGame, ToyGameSpec, ToyNodeInfo, build_toy_game, kuhn, leduc,
};
