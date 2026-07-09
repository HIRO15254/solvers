//! The preflop trunk: config, betting-tree builder, and session wrapper.
//!
//! Mirrors `holdem::postflop`'s shape (private recursive `Builder` producing
//! `engine::TempNode`s, tag-0-is-untagged node info convention,
//! `node_by_history` lookup) with the private-state space swapped from
//! 1,326 combos to the 169 classes.

use cards::{Chips, PerPlayer, Player, Range};
use engine::{CompiledGame, NodeId};
use game::PayoffPipeline;

use crate::equity::EquityTable;
use crate::evaluator::PreflopEvaluator;
use crate::model::PostflopModel;

/// Preflop trunk description. Chips are denominated in tenths of a big
/// blind ([`crate::CHIPS_PER_BB`] = 10); use [`PreflopConfig::hu`] for the
/// standard heads-up shape.
///
/// # Betting grammar
///
/// - `open_sizes_bb`: raise-to sizes, in big blinds, available to the first
///   raiser (the SB open, or the BB's raise over a limp).
/// - `raise_factors[level - 1]`: raise-to sizes for the `level`-th raise
///   (3-bet is level 1) as multiples of the previous raise-to amount; the
///   last entry is reused for deeper levels; an empty outer list means
///   sized reraises are never offered (all-in only).
/// - Raise targets are clamped to the standard minimum (previous raise-to
///   plus the last raise increment) and to the effective stack; targets
///   that reach the stack become the all-in. Duplicates are merged.
/// - `include_allin` adds the jam to every raise level; `max_raises` caps
///   the total number of raises (the jam counts as a raise).
///
/// # Terminal streets (rake semantics)
///
/// Fold terminals are stamped `Street::Preflop`; all-in showdowns and
/// continuations are stamped `Street::Flop`, because a board is dealt in
/// both cases — this is what makes `no_flop_no_drop` rake waive folds but
/// charge showdowns, matching how the rule works in practice.
pub struct PreflopConfig {
    /// Per-player starting stack (chips, before posting blinds).
    pub effective_stack: Chips,
    /// Small blind posted by [`Player::P0`].
    pub sb: Chips,
    /// Big blind posted by [`Player::P1`].
    pub bb: Chips,
    /// Combo-weighted ranges; `[Range::full(), Range::full()]` for a normal
    /// preflop solve. P0 = SB, P1 = BB.
    pub ranges: PerPlayer<Range>,
    /// First-raise raise-to sizes in big blinds (e.g. `[2.5]`).
    pub open_sizes_bb: Vec<f64>,
    /// Reraise-to factors per raise level (see type docs).
    pub raise_factors: Vec<Vec<f64>>,
    /// Cap on the total number of raises.
    pub max_raises: u32,
    /// Offer the all-in at every raise level.
    pub include_allin: bool,
    /// Allow the SB to limp (call the big blind).
    pub allow_limp: bool,
    /// Record per-node history strings and action labels.
    pub track_node_info: bool,
}

impl PreflopConfig {
    /// Standard heads-up shape: blinds 0.5/1 bb, full ranges, 2.5x open,
    /// 3x reraises, up to 4 raises with all-in and limp available.
    pub fn hu(effective_stack_bb: f64) -> Self {
        PreflopConfig {
            effective_stack: Chips((effective_stack_bb * crate::CHIPS_PER_BB as f64).round() as u32),
            sb: Chips(crate::CHIPS_PER_BB / 2),
            bb: Chips(crate::CHIPS_PER_BB),
            ranges: PerPlayer::new(Range::full(), Range::full()),
            open_sizes_bb: vec![2.5],
            raise_factors: vec![vec![3.0]],
            max_raises: 4,
            include_allin: true,
            allow_limp: true,
            track_node_info: true,
        }
    }
}

/// Per-node history string and action labels (tag 0 is the shared
/// "untagged" sentinel, same convention as the toy and postflop builders).
///
/// History tokens: `f` fold, `c` call (or limp), `x` check,
/// `r{chips}` raise-to (the all-in is `r{stack}`), joined without
/// separators onto the parent history.
#[derive(Clone, Debug)]
pub struct PreflopNodeInfo {
    pub history: String,
    pub actions: Vec<String>,
}

/// A compiled preflop trunk plus its node metadata.
pub struct PreflopGame {
    pub game: CompiledGame<PreflopEvaluator>,
    pub node_info: Vec<PreflopNodeInfo>,
}

impl PreflopGame {
    /// Node lookup by exact history string (`""` is the root).
    pub fn node_by_history(&self, _history: &str) -> Option<NodeId> {
        todo!("implemented by trunk/evaluator agent")
    }

    /// The node's info record (untagged nodes get the sentinel entry 0).
    pub fn info(&self, _node: NodeId) -> &PreflopNodeInfo {
        todo!("implemented by trunk/evaluator agent")
    }
}

/// Pre-allocation estimate, computed by a dry run that shares the
/// action-enumeration code with the real builder.
#[derive(Clone, Copy, Debug)]
pub struct MemoryEstimate {
    pub f32_bytes: u64,
    pub i16_bytes: u64,
    pub nodes: u64,
    pub terminals: u64,
}

/// Sizes the trunk without building it.
pub fn memory_usage(_config: &PreflopConfig) -> MemoryEstimate {
    todo!("implemented by trunk/evaluator agent")
}

/// Builds the trunk through the payoff pipeline.
///
/// `zero_sum` is `pipeline.is_zero_sum() && model.preserves_zero_sum()`.
/// Panics on inconsistent configs (blinds not `sb < bb`, stack not above
/// the big blind, empty ranges, or an SB with no legal action).
pub fn build_preflop_game(
    _config: &PreflopConfig,
    _table: &EquityTable,
    _model: &dyn PostflopModel,
    _pipeline: PayoffPipeline<'_>,
) -> PreflopGame {
    todo!("implemented by trunk/evaluator agent")
}
