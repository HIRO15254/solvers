//! Product-neutral, validated poker and operating data. No algorithm settings live here.
use crate::TreeVar;
use economics::{CompiledRake, RakeConfig, UtilityConfig};
use nlh::betting::BettingState;
use nlh::script::Script;
use nlh::{Card, MwChips, Range, SeatId, SeatVec, Street, TableSetup};
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
    /// P1 only: save the final state in addition to periodic checkpoints.
    pub final_checkpoint: bool,
}

/// Values of the legacy multiway history selectors at a postflop decision.
/// The two preflop-only predicates remain false after preflop closes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PreflopFacts {
    pub limpers: u8,
    pub flats: u8,
    pub squeeze: bool,
    pub open_cold_calls: u8,
    pub preflop_participant: bool,
    pub in_position_to_last_aggressor: bool,
    pub last_preflop_aggressor_position: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LineAction {
    pub street: Street,
    pub seat: SeatId,
    pub position: String,
    pub action: nlh::betting::Action,
    pub implicit: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StartSeat {
    pub seat: SeatId,
    pub position: String,
    pub starting_stack: MwChips,
    pub remaining_stack: MwChips,
    pub total_contribution: MwChips,
    pub folded: bool,
    pub refund: MwChips,
}

#[derive(Clone, Debug, Serialize)]
pub struct PostflopPlayer {
    pub seat: SeatId,
    pub position: String,
    pub preflop: PreflopFacts,
}

/// All monetary values use the shared 0.001 BB chip unit, before rake.
#[derive(Clone, Debug, Serialize)]
pub struct StartState {
    pub street: Street,
    #[serde(serialize_with = "serialize_board")]
    pub board: Vec<Card>,
    pub pot: MwChips,
    pub seats: Vec<StartSeat>,
    pub folded_seats: Vec<SeatId>,
    pub oop: Option<PostflopPlayer>,
    pub ip: Option<PostflopPlayer>,
    pub effective_stack: Option<MwChips>,
    pub previous_street_aggressor: Option<SeatId>,
    pub actions: Vec<LineAction>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ParamDiagnostic {
    pub name: String,
    pub kind: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TreeDiagnostics {
    pub params: Vec<ParamDiagnostic>,
    pub rules: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ValidateSummary {
    pub product: Product,
    pub start: StartState,
    pub effective_stack: Option<MwChips>,
    pub actions: Vec<LineAction>,
    pub tree: TreeDiagnostics,
    pub warnings: Vec<String>,
}

fn serialize_board<S: serde::Serializer>(board: &[Card], serializer: S) -> Result<S::Ok, S::Error> {
    board
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .serialize(serializer)
}

/// Validated game context passed to a product, at a preflop or postflop street root.
#[derive(Clone)]
pub struct Spot {
    pub meta: Meta,
    pub table: Table,
    pub economics: Economics,
    pub start: BettingState,
    /// Canonical user spelling, unchanged by normalization.
    pub line: String,
    pub board_text: Option<String>,
    pub context: StartState,
    pub ranges: SeatVec<SeatRange>,
    pub tree: Tree,
    pub run: Run,
    pub product: Product,
}
