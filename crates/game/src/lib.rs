//! Variant-independent game layer.
//!
//! Home of the terminal payoff pipeline — the seam through which rake
//! models and ICM/payout structures plug in without touching the engine —
//! plus toy games (Kuhn, Leduc) compiled through the production
//! tree-building path so the correctness harness exercises real code.

mod payoff;
mod toy;

pub use payoff::{
    BakedPayoffs, ChipEv, GgPreflopRake, Icm, NoRake, Outcome, PayoffPipeline, PercentCapRake,
    RakeModel, TerminalDescriptor, TerminalKind, UtilityModel,
};
pub use toy::{
    RoundSpec, ToyEvaluator, ToyGame, ToyGameSpec, ToyNodeInfo, build_toy_game, kuhn, leduc,
};
