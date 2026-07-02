//! River-only shim over the multi-street postflop builder: a thin adapter
//! that constructs a 5-card [`PostflopConfig`] and delegates to
//! [`build_postflop_game`]. `iso_merging` is irrelevant here (a 5-card
//! starting board never deals — there's no next street), and every history
//! token / node lookup behaves exactly as it did when this module owned the
//! whole builder.

use cards::{Card, Chips, PerPlayer, Range};
use engine::{CompiledGame, NodeId};
use game::PayoffPipeline;

use crate::postflop::{
    PerStreet, PostflopConfig, PostflopEvaluator, PostflopGame, build_postflop_game,
};

/// A river subgame: fixed 5-card board, both ranges, existing pot
/// (contributed equally), remaining effective stacks, and a simple bet
/// grammar (pot-fraction sizes per player, raise cap, all-in clamping).
pub struct RiverConfig {
    pub board: [Card; 5],
    pub ranges: PerPlayer<Range>,
    /// Pot at the start of the river; must be even (equal contributions).
    pub pot: Chips,
    /// Chips behind for each player.
    pub effective_stack: Chips,
    /// Bet/raise sizes as fractions of the current pot, per player.
    pub bet_fractions: PerPlayer<Vec<f64>>,
    /// Maximum number of bets+raises on the river.
    pub max_raises: u32,
}

/// Node metadata mirroring the toy games' scheme.
#[derive(Clone, Debug, Default)]
pub struct RiverNodeInfo {
    pub history: String,
    pub actions: Vec<String>,
}

pub struct RiverGame {
    pub game: CompiledGame<PostflopEvaluator>,
    pub node_info: Vec<RiverNodeInfo>,
}

impl RiverGame {
    pub fn node_by_history(&self, history: &str) -> Option<NodeId> {
        let tag = self
            .node_info
            .iter()
            .position(|info| info.history == history)? as u32;
        self.game
            .tree
            .tags
            .iter()
            .position(|&t| t == tag)
            .map(|id| id as u32)
    }
}

/// Builds a river subgame through the payoff pipeline.
pub fn build_river_game(config: &RiverConfig, pipeline: PayoffPipeline<'_>) -> RiverGame {
    let postflop_config = PostflopConfig {
        board: config.board.to_vec(),
        ranges: config.ranges.clone(),
        pot: config.pot,
        effective_stack: config.effective_stack,
        bet_fractions: PerStreet {
            flop: PerPlayer::new(Vec::new(), Vec::new()),
            turn: PerPlayer::new(Vec::new(), Vec::new()),
            river: config.bet_fractions.clone(),
        },
        max_raises: PerStreet {
            flop: 0,
            turn: 0,
            river: config.max_raises,
        },
        iso_merging: true,
        track_node_info: true,
    };
    let PostflopGame { game, node_info } = build_postflop_game(&postflop_config, pipeline);
    RiverGame {
        game,
        node_info: node_info
            .into_iter()
            .map(|info| RiverNodeInfo {
                history: info.history,
                actions: info.actions,
            })
            .collect(),
    }
}
