//! Product-neutral, validated poker and operating data. No algorithm settings live here.
use crate::TreeVar;
use economics::{CompiledRake, RakeConfig, UtilityConfig};
use nlh::betting::BettingState;
use nlh::script::Script;
use nlh::{MwChips, Range, SeatVec, TableSetup};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Product {
    HuPostflop,
    MultiwayPreflop,
}

impl Product {
    pub fn name(self) -> &'static str {
        match self {
            Self::HuPostflop => "P1 (HU Postflop)",
            Self::MultiwayPreflop => "P2 (Multiway Preflop)",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub name: Option<String>,
    pub description: Option<String>,
    pub derived_from: Option<DerivedFrom>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedFrom {
    pub run_id: Option<String>,
    pub solution_hash: Option<String>,
    pub line: Option<String>,
    pub board: Option<String>,
}

/// Seat zero is BTN; all per-seat data is clockwise, including heads-up BTN/SB.
#[derive(Clone, Debug)]
pub struct Table {
    pub positions: SeatVec<String>,
    pub stacks: SeatVec<MwChips>,
    pub sb: MwChips,
    pub ante: MwChips,
    pub bb_ante: MwChips,
    pub setup: TableSetup,
}

#[derive(Clone, Debug)]
pub struct Economics {
    pub rake: RakeConfig,
    pub compiled_rake: CompiledRake,
    pub utility: UtilityConfig,
}

/// Range text is retained for normalization alongside the parsed combo weights.
#[derive(Clone)]
pub struct SeatRange {
    pub text: String,
    pub range: Range,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaxAggressiveActions {
    pub preflop: u64,
    pub flop: u64,
    pub turn: u64,
    pub river: u64,
}

impl Default for MaxAggressiveActions {
    fn default() -> Self {
        Self {
            preflop: 4,
            flop: 3,
            turn: 3,
            river: 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub script: String,
    pub compiled: Script<TreeVar>,
    pub include_allin: bool,
    pub allin_threshold: Option<f64>,
    pub preflop_reraise_jam_above_stack: Option<Ratio>,
    pub max_aggressive_actions: MaxAggressiveActions,
    pub params: BTreeMap<String, toml::Value>,
}

/// `None` denotes the product's automatic resource choice, resolved by that product.
#[derive(Clone, Debug)]
pub struct Run {
    pub threads: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub max_time_seconds: Option<f64>,
    pub checkpoint_interval_seconds: f64,
}

/// Validated game context passed to a product. The start currently supports preflop roots.
#[derive(Clone)]
pub struct Spot {
    pub meta: Meta,
    pub table: Table,
    pub economics: Economics,
    pub start: BettingState,
    pub ranges: SeatVec<SeatRange>,
    pub tree: Tree,
    pub run: Run,
    pub product: Product,
}
