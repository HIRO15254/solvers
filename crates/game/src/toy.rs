//! Toy games (Kuhn, Leduc) compiled through the production tree pipeline.
//!
//! Private states are single cards from a small deck; betting is a generic
//! limit structure. The point is that the correctness harness (known game
//! values, exploitability decay) exercises the same `TempNode` compilation,
//! payoff baking, chance masks, and solver walks as real poker games.

use cards::{Chips, PerPlayer, Player, Street};
use engine::{CompiledGame, PublicTree, ReachMap, TempNode, TerminalEvaluator, TreeSpec};

use crate::payoff::{BakedPayoffs, Outcome, PayoffPipeline, TerminalDescriptor, TerminalKind};

/// One betting round of a limit toy game.
pub struct RoundSpec {
    pub bet: Chips,
    pub max_raises: u32,
    /// Deal one public card (from the same deck as the hands) before this
    /// round starts.
    pub deal_board: bool,
}

/// Description of a limit toy game with single-card private hands.
pub struct ToyGameSpec {
    pub name: &'static str,
    /// Physical deck size; private states are cards `0..deck`.
    pub deck: usize,
    pub ante: Chips,
    pub starting_stack: Chips,
    pub rounds: Vec<RoundSpec>,
    /// `(p0_card, p1_card, board) -> Outcome`. Only called for distinct
    /// live cards.
    #[allow(clippy::type_complexity)]
    pub showdown: Box<dyn Fn(usize, usize, Option<usize>) -> Outcome + Send + Sync>,
}

/// Terminal evaluator for toy games: per-terminal utility matrices with
/// card-removal (h == o excluded) baked in as zero entries.
pub struct ToyEvaluator {
    num_hands: usize,
    /// Per terminal, per player: `H*H` matrix, entry `[h * H + o]`.
    terminals: Vec<PerPlayer<Vec<f32>>>,
}

impl TerminalEvaluator for ToyEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        let matrix = &self.terminals[terminal as usize][p];
        let h_count = self.num_hands;
        debug_assert_eq!(opp_reach.len(), h_count);
        debug_assert_eq!(out.len(), h_count);
        for h in 0..h_count {
            let row = &matrix[h * h_count..(h + 1) * h_count];
            out[h] = row.iter().zip(opp_reach).map(|(&u, &r)| u * r).sum::<f32>();
        }
    }
}

/// Node metadata for tests, the CLI, and strategy export.
#[derive(Clone, Debug, Default)]
pub struct ToyNodeInfo {
    /// Action history from the root, e.g. `"cb"` (Leduc board cards appear
    /// as `[2]`).
    pub history: String,
    /// Action names at this node; empty for chance/terminal nodes.
    pub actions: Vec<String>,
}

pub struct ToyGame {
    pub game: CompiledGame<ToyEvaluator>,
    /// Indexed by the tags stored in `game.tree.tags`.
    pub node_info: Vec<ToyNodeInfo>,
}

impl ToyGame {
    /// Finds the action node with the given history string.
    pub fn node_by_history(&self, history: &str) -> Option<engine::NodeId> {
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

    pub fn info(&self, node: engine::NodeId) -> &ToyNodeInfo {
        &self.node_info[self.game.tree.tags[node as usize] as usize]
    }
}

struct Builder<'a> {
    spec: &'a ToyGameSpec,
    pipeline: PayoffPipeline<'a>,
    terminals: Vec<PerPlayer<Vec<f32>>>,
    masks: Vec<Vec<f32>>,
    node_info: Vec<ToyNodeInfo>,
}

#[derive(Clone)]
struct BetState {
    round: usize,
    board: Option<usize>,
    contrib: PerPlayer<Chips>,
    to_act: Player,
    outstanding: Chips,
    raises_used: u32,
    first_checked: bool,
    history: String,
}

/// Builds a [`ToyGame`] with the given rake and utility models applied
/// through the payoff pipeline.
pub fn build_toy_game(spec: &ToyGameSpec, pipeline: PayoffPipeline<'_>) -> ToyGame {
    let mut builder = Builder {
        spec,
        pipeline,
        terminals: Vec::new(),
        masks: Vec::new(),
        // Tag 0 is reserved for untagged (chance/terminal) nodes; give it a
        // history no real node can have so lookups never match it.
        node_info: vec![ToyNodeInfo {
            history: "<untagged>".into(),
            actions: Vec::new(),
        }],
    };
    let root = builder.round_start(BetState {
        round: 0,
        board: None,
        contrib: PerPlayer::new(spec.ante, spec.ante),
        to_act: Player::P0,
        outstanding: Chips::ZERO,
        raises_used: 0,
        first_checked: false,
        history: String::new(),
    });
    let deck = spec.deck as u32;
    let tree = engine::PublicTree::compile(TreeSpec {
        root,
        masks: builder.masks,
        transitions: Vec::new(),
        root_dims: PerPlayer::new(deck, deck),
    });
    let street_norm = normalizer(&tree, spec.deck);
    ToyGame {
        game: CompiledGame {
            tree,
            evaluator: ToyEvaluator {
                num_hands: spec.deck,
                terminals: builder.terminals,
            },
            root_ranges: PerPlayer::new(vec![1.0; spec.deck], vec![1.0; spec.deck]),
            normalizer: street_norm,
        },
        node_info: builder.node_info,
    }
}

/// Joint weight of compatible root hand pairs with unit ranges.
fn normalizer(_tree: &PublicTree, deck: usize) -> f64 {
    (deck * (deck - 1)) as f64
}

impl Builder<'_> {
    /// Entry point of a betting round: deals a board card first if the
    /// round asks for one.
    fn round_start(&mut self, state: BetState) -> TempNode {
        let round = &self.spec.rounds[state.round];
        if round.deal_board && state.board.is_none() {
            return self.deal_board(state);
        }
        self.betting(state)
    }

    fn deal_board(&mut self, state: BetState) -> TempNode {
        // Uniform conditional probability: with both hands hidden, every
        // (h, o) pair sees the same number of live board candidates.
        let live = self.spec.deck - 2;
        let weight = 1.0 / live as f32;
        let deals = (0..self.spec.deck)
            .map(|card| {
                let mask_id = self.board_mask(card);
                let maps = PerPlayer::new(ReachMap::Mask(mask_id), ReachMap::Mask(mask_id));
                let child_state = BetState {
                    board: Some(card),
                    history: format!("{}[{}]", state.history, card),
                    ..state.clone()
                };
                let child = self.betting(child_state);
                (weight, maps, child)
            })
            .collect();
        TempNode::Chance { deals, tag: 0 }
    }

    fn board_mask(&mut self, card: usize) -> u32 {
        let mut mask = vec![1.0f32; self.spec.deck];
        mask[card] = 0.0;
        self.masks.push(mask);
        (self.masks.len() - 1) as u32
    }

    fn betting(&mut self, state: BetState) -> TempNode {
        let round = &self.spec.rounds[state.round];
        let actor = state.to_act;
        let mut actions: Vec<(String, TempNode)> = Vec::new();

        if state.outstanding == Chips::ZERO {
            // Check.
            if state.first_checked {
                let next = BetState {
                    history: format!("{}c", state.history),
                    ..state.clone()
                };
                actions.push(("check".into(), self.end_round(next)));
            } else {
                let next = BetState {
                    to_act: actor.opponent(),
                    first_checked: true,
                    history: format!("{}c", state.history),
                    ..state.clone()
                };
                actions.push(("check".into(), self.betting(next)));
            }
            // Bet.
            if state.raises_used < round.max_raises {
                let mut contrib = state.contrib;
                contrib[actor] += round.bet;
                let next = BetState {
                    to_act: actor.opponent(),
                    outstanding: round.bet,
                    raises_used: state.raises_used + 1,
                    contrib,
                    history: format!("{}b", state.history),
                    ..state.clone()
                };
                actions.push(("bet".into(), self.betting(next)));
            }
        } else {
            // Fold.
            let fold_state = BetState {
                history: format!("{}f", state.history),
                ..state.clone()
            };
            actions.push((
                "fold".into(),
                self.terminal(&fold_state, TerminalKind::Fold { folder: actor }),
            ));
            // Call.
            {
                let mut contrib = state.contrib;
                contrib[actor] += state.outstanding;
                let next = BetState {
                    contrib,
                    outstanding: Chips::ZERO,
                    history: format!("{}k", state.history),
                    ..state.clone()
                };
                actions.push(("call".into(), self.end_round(next)));
            }
            // Raise.
            if state.raises_used < round.max_raises {
                let mut contrib = state.contrib;
                contrib[actor] += state.outstanding + round.bet;
                let next = BetState {
                    to_act: actor.opponent(),
                    outstanding: round.bet,
                    raises_used: state.raises_used + 1,
                    contrib,
                    history: format!("{}r", state.history),
                    ..state.clone()
                };
                actions.push(("raise".into(), self.betting(next)));
            }
        }

        let tag = self.node_info.len() as u32;
        self.node_info.push(ToyNodeInfo {
            history: state.history.clone(),
            actions: actions.iter().map(|(name, _)| name.clone()).collect(),
        });
        TempNode::Action {
            player: actor,
            children: actions.into_iter().map(|(_, child)| child).collect(),
            tag,
        }
    }

    fn end_round(&mut self, state: BetState) -> TempNode {
        if state.round + 1 < self.spec.rounds.len() {
            let next = BetState {
                round: state.round + 1,
                to_act: Player::P0,
                outstanding: Chips::ZERO,
                raises_used: 0,
                first_checked: false,
                ..state
            };
            self.round_start(next)
        } else {
            self.terminal(&state, TerminalKind::Showdown)
        }
    }

    fn terminal(&mut self, state: &BetState, kind: TerminalKind) -> TempNode {
        let last_round = state.round + 1 == self.spec.rounds.len();
        let street = if last_round {
            Street::River
        } else {
            Street::Preflop
        };
        let descriptor = TerminalDescriptor {
            kind,
            street,
            pot: state.contrib[Player::P0] + state.contrib[Player::P1],
            contrib: state.contrib,
            stacks_before: PerPlayer::new(self.spec.starting_stack, self.spec.starting_stack),
        };
        let payoffs = self.pipeline.bake(&descriptor);
        let id = self.terminals.len() as u32;
        self.terminals
            .push(self.utility_matrices(state, kind, &payoffs));
        TempNode::Terminal { id, tag: 0 }
    }

    /// Bakes per-player `H*H` utility matrices with card removal as zeros.
    fn utility_matrices(
        &self,
        state: &BetState,
        kind: TerminalKind,
        payoffs: &BakedPayoffs,
    ) -> PerPlayer<Vec<f32>> {
        let h_count = self.spec.deck;
        let mut matrices = PerPlayer::new(
            vec![0.0f32; h_count * h_count],
            vec![0.0f32; h_count * h_count],
        );
        for p0_card in 0..h_count {
            for p1_card in 0..h_count {
                if p0_card == p1_card
                    || Some(p0_card) == state.board
                    || Some(p1_card) == state.board
                {
                    continue;
                }
                let outcome = match kind {
                    TerminalKind::Fold { folder: Player::P0 } => Outcome::WinP1,
                    TerminalKind::Fold { folder: Player::P1 } => Outcome::WinP0,
                    TerminalKind::Showdown => (self.spec.showdown)(p0_card, p1_card, state.board),
                };
                let utility = payoffs.for_outcome(outcome);
                matrices[Player::P0][p0_card * h_count + p1_card] = utility[Player::P0] as f32;
                matrices[Player::P1][p1_card * h_count + p0_card] = utility[Player::P1] as f32;
            }
        }
        matrices
    }
}

/// Kuhn poker: 3 cards, ante 1, one round, bet 1, one raise max.
/// Known game value for the first player: -1/18.
pub fn kuhn(pipeline: PayoffPipeline<'_>) -> ToyGame {
    let spec = ToyGameSpec {
        name: "kuhn",
        deck: 3,
        ante: Chips(1),
        starting_stack: Chips(10),
        rounds: vec![RoundSpec {
            bet: Chips(1),
            max_raises: 1,
            deal_board: false,
        }],
        showdown: Box::new(|p0, p1, _| {
            if p0 > p1 {
                Outcome::WinP0
            } else {
                Outcome::WinP1
            }
        }),
    };
    build_toy_game(&spec, pipeline)
}

/// Leduc hold'em: 6 cards (3 ranks x 2 suits), ante 1, two rounds with bets
/// 2 and 4 and at most two raises each, one community card before round 2.
/// Known game value for the first player: about -0.0856.
pub fn leduc(pipeline: PayoffPipeline<'_>) -> ToyGame {
    let rank = |card: usize| card / 2;
    let spec = ToyGameSpec {
        name: "leduc",
        deck: 6,
        ante: Chips(1),
        starting_stack: Chips(20),
        rounds: vec![
            RoundSpec {
                bet: Chips(2),
                max_raises: 2,
                deal_board: false,
            },
            RoundSpec {
                bet: Chips(4),
                max_raises: 2,
                deal_board: true,
            },
        ],
        showdown: Box::new(move |p0, p1, board| {
            let board_rank = rank(board.expect("leduc showdown has a board"));
            let p0_pair = rank(p0) == board_rank;
            let p1_pair = rank(p1) == board_rank;
            match (p0_pair, p1_pair) {
                (true, false) => Outcome::WinP0,
                (false, true) => Outcome::WinP1,
                _ => match rank(p0).cmp(&rank(p1)) {
                    std::cmp::Ordering::Greater => Outcome::WinP0,
                    std::cmp::Ordering::Less => Outcome::WinP1,
                    std::cmp::Ordering::Equal => Outcome::Tie,
                },
            }
        }),
    };
    build_toy_game(&spec, pipeline)
}
