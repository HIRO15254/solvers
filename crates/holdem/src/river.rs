use cards::{
    Card, CardSet, Chips, HandRank, NUM_COMBOS, PerPlayer, Player, Range, Street, combo_cards,
    rank_of,
};
use engine::{CompiledGame, TempNode, TerminalEvaluator, TreeSpec};
use game::{BakedPayoffs, PayoffPipeline, TerminalDescriptor, TerminalKind};

/// A river subgame: fixed 5-card board, both ranges, existing pot
/// (contributed equally), remaining effective stacks, and a simple bet
/// grammar (pot-fraction sizes per player, raise cap, all-in clamping).
///
/// The full `BetGrammar` (per-street size grids, geometric sizing, donk
/// toggles) arrives with the multi-street builder; this is its river core.
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
    pub game: CompiledGame<RiverEvaluator>,
    pub node_info: Vec<RiverNodeInfo>,
}

impl RiverGame {
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
}

struct RiverTerminal {
    kind: TerminalKind,
    payoffs: BakedPayoffs,
}

/// Exact terminal evaluation over 1,326-combo reach vectors.
pub struct RiverEvaluator {
    terminals: Vec<RiverTerminal>,
    /// Live combos (disjoint from the board) sorted by ascending hand rank.
    sorted_combos: Vec<(HandRank, u32)>,
}

impl RiverEvaluator {
    /// Sum of `opp_reach` over combos disjoint from hand `h`, via
    /// inclusion-exclusion on h's two cards.
    fn compat_sums(&self, opp_reach: &[f32]) -> (f64, [f64; 52]) {
        let mut total = 0.0f64;
        let mut per_card = [0.0f64; 52];
        for &(_, combo) in &self.sorted_combos {
            let r = opp_reach[combo as usize] as f64;
            if r != 0.0 {
                let (c1, c2) = combo_cards(combo as usize);
                total += r;
                per_card[c1.index()] += r;
                per_card[c2.index()] += r;
            }
        }
        (total, per_card)
    }
}

impl TerminalEvaluator for RiverEvaluator {
    fn eval(&self, terminal: u32, p: Player, opp_reach: &[f32], out: &mut [f32]) {
        debug_assert_eq!(opp_reach.len(), NUM_COMBOS);
        debug_assert_eq!(out.len(), NUM_COMBOS);
        out.fill(0.0);
        let term = &self.terminals[terminal as usize];
        // Orient payoff constants to the traversing player: `u_win` is p's
        // utility when p's hand wins, etc.
        let (u_win, u_tie, u_lose) = match p {
            Player::P0 => (
                term.payoffs.win_p0[Player::P0],
                term.payoffs.tie[Player::P0],
                term.payoffs.win_p1[Player::P0],
            ),
            Player::P1 => (
                term.payoffs.win_p1[Player::P1],
                term.payoffs.tie[Player::P1],
                term.payoffs.win_p0[Player::P1],
            ),
        };
        let (all_total, all_card) = self.compat_sums(opp_reach);

        match term.kind {
            TerminalKind::Fold { .. } => {
                // Fold payoffs are outcome-independent (win_p0 == tie ==
                // win_p1); u_win == u_tie == u_lose here.
                for &(_, combo) in &self.sorted_combos {
                    let (c1, c2) = combo_cards(combo as usize);
                    let compat = all_total - all_card[c1.index()] - all_card[c2.index()]
                        + opp_reach[combo as usize] as f64;
                    out[combo as usize] = (u_win * compat) as f32;
                }
            }
            TerminalKind::Showdown => {
                // O(n + m) sorted-rank sweep: walk equal-rank groups in
                // ascending order keeping strictly-below prefix sums; ties
                // are handled with the group's own sums.
                let mut below_total = 0.0f64;
                let mut below_card = [0.0f64; 52];
                let combos = &self.sorted_combos;
                let mut group_start = 0;
                while group_start < combos.len() {
                    let rank = combos[group_start].0;
                    let mut group_end = group_start;
                    while group_end < combos.len() && combos[group_end].0 == rank {
                        group_end += 1;
                    }
                    let group = &combos[group_start..group_end];

                    let mut group_total = 0.0f64;
                    let mut group_card = [0.0f64; 52];
                    for &(_, combo) in group {
                        let r = opp_reach[combo as usize] as f64;
                        if r != 0.0 {
                            let (c1, c2) = combo_cards(combo as usize);
                            group_total += r;
                            group_card[c1.index()] += r;
                            group_card[c2.index()] += r;
                        }
                    }

                    for &(_, combo) in group {
                        let idx = combo as usize;
                        let (c1, c2) = combo_cards(idx);
                        let (i1, i2) = (c1.index(), c2.index());
                        // Inclusion-exclusion: the o == h combo is subtracted
                        // twice by the per-card sums, so groups containing h
                        // (tie, all) add its reach back once.
                        let win = below_total - below_card[i1] - below_card[i2];
                        let tie =
                            group_total - group_card[i1] - group_card[i2] + opp_reach[idx] as f64;
                        let compat =
                            all_total - all_card[i1] - all_card[i2] + opp_reach[idx] as f64;
                        let lose = compat - win - tie;
                        out[idx] = (u_win * win + u_tie * tie + u_lose * lose) as f32;
                    }

                    below_total += group_total;
                    for &(_, combo) in group {
                        let r = opp_reach[combo as usize] as f64;
                        if r != 0.0 {
                            let (c1, c2) = combo_cards(combo as usize);
                            below_card[c1.index()] += r;
                            below_card[c2.index()] += r;
                        }
                    }
                    group_start = group_end;
                }
            }
        }
    }
}

#[derive(Clone)]
struct BetState {
    to_act: Player,
    /// River bets so far (excluding the pre-river pot halves).
    contrib: PerPlayer<Chips>,
    outstanding: Chips,
    raises_used: u32,
    first_checked: bool,
    history: String,
}

struct Builder<'a> {
    config: &'a RiverConfig,
    pipeline: PayoffPipeline<'a>,
    terminals: Vec<RiverTerminal>,
    node_info: Vec<RiverNodeInfo>,
}

/// Builds a river subgame through the payoff pipeline.
pub fn build_river_game(config: &RiverConfig, pipeline: PayoffPipeline<'_>) -> RiverGame {
    assert!(config.pot.0.is_multiple_of(2), "river pot must be even");
    let board: CardSet = config.board.iter().copied().collect();
    assert_eq!(board.len(), 5, "board must have 5 distinct cards");

    // Rank every live combo once; the evaluator never touches the hand
    // evaluator again.
    let mut sorted_combos: Vec<(HandRank, u32)> = Vec::new();
    for combo in 0..NUM_COMBOS {
        let (c1, c2) = combo_cards(combo);
        if board.contains(c1) || board.contains(c2) {
            continue;
        }
        let rank = rank_of(config.board.iter().copied().chain([c1, c2]));
        sorted_combos.push((rank, combo as u32));
    }
    sorted_combos.sort_unstable();

    let mut builder = Builder {
        config,
        pipeline,
        terminals: Vec::new(),
        node_info: vec![RiverNodeInfo {
            history: "<untagged>".into(),
            actions: Vec::new(),
        }],
    };
    let root = builder.betting(BetState {
        to_act: Player::P0,
        contrib: PerPlayer::new(Chips::ZERO, Chips::ZERO),
        outstanding: Chips::ZERO,
        raises_used: 0,
        first_checked: false,
        history: String::new(),
    });

    // Root ranges as 1,326-weight vectors with board conflicts zeroed.
    let range_vec = |p: Player| -> Vec<f32> {
        (0..NUM_COMBOS)
            .map(|combo| {
                let (c1, c2) = combo_cards(combo);
                if board.contains(c1) || board.contains(c2) {
                    0.0
                } else {
                    config.ranges[p].weight(combo)
                }
            })
            .collect()
    };
    let ranges = PerPlayer::new(range_vec(Player::P0), range_vec(Player::P1));

    // Joint compatible weight, via the same inclusion-exclusion the fold
    // kernel uses.
    let evaluator = RiverEvaluator {
        terminals: builder.terminals,
        sorted_combos,
    };
    let (all_total, all_card) = evaluator.compat_sums(&ranges[Player::P1]);
    let normalizer: f64 = evaluator
        .sorted_combos
        .iter()
        .map(|&(_, combo)| {
            let idx = combo as usize;
            let (c1, c2) = combo_cards(idx);
            ranges[Player::P0][idx] as f64
                * (all_total - all_card[c1.index()] - all_card[c2.index()]
                    + ranges[Player::P1][idx] as f64)
        })
        .sum();
    assert!(normalizer > 0.0, "ranges share no compatible combos");

    let tree = engine::PublicTree::compile(TreeSpec {
        root,
        masks: Vec::new(),
        transitions: Vec::new(),
        root_dims: PerPlayer::new(NUM_COMBOS as u32, NUM_COMBOS as u32),
    });

    RiverGame {
        game: CompiledGame {
            tree,
            evaluator,
            root_ranges: ranges,
            normalizer,
        },
        node_info: builder.node_info,
    }
}

impl Builder<'_> {
    fn betting(&mut self, state: BetState) -> TempNode {
        let actor = state.to_act;
        let pot_now = self.config.pot + state.contrib[Player::P0] + state.contrib[Player::P1];
        let behind = self.config.effective_stack - state.contrib[actor];
        let mut actions: Vec<(String, TempNode)> = Vec::new();

        if state.outstanding == Chips::ZERO {
            if state.first_checked {
                let next = BetState {
                    history: format!("{}x", state.history),
                    ..state.clone()
                };
                actions.push(("check".into(), self.showdown(&next)));
            } else {
                let next = BetState {
                    to_act: actor.opponent(),
                    first_checked: true,
                    history: format!("{}x", state.history),
                    ..state.clone()
                };
                actions.push(("check".into(), self.betting(next)));
            }
        } else {
            let fold_state = BetState {
                history: format!("{}f", state.history),
                ..state.clone()
            };
            actions.push((
                "fold".into(),
                self.terminal(&fold_state, TerminalKind::Fold { folder: actor }),
            ));
            let mut contrib = state.contrib;
            contrib[actor] += state.outstanding;
            let call_state = BetState {
                contrib,
                outstanding: Chips::ZERO,
                history: format!("{}c", state.history),
                ..state.clone()
            };
            actions.push(("call".into(), self.showdown(&call_state)));
        }

        // Bets and raises share sizing logic: `f * pot after call`.
        if state.raises_used < self.config.max_raises && behind > state.outstanding {
            let mut seen_amounts: Vec<Chips> = Vec::new();
            for &fraction in &self.config.bet_fractions[actor] {
                let pot_after_call = pot_now + state.outstanding;
                let raw = (fraction * pot_after_call.as_f64()).round() as u32;
                let extra = Chips(raw.max(1)).min(behind - state.outstanding);
                let additional = state.outstanding + extra;
                if seen_amounts.contains(&additional) {
                    continue;
                }
                seen_amounts.push(additional);
                let mut contrib = state.contrib;
                contrib[actor] += additional;
                let to = contrib[actor];
                let verb = if state.outstanding == Chips::ZERO {
                    format!("bet {to}")
                } else {
                    format!("raise to {to}")
                };
                let next = BetState {
                    to_act: actor.opponent(),
                    contrib,
                    outstanding: extra,
                    raises_used: state.raises_used + 1,
                    first_checked: state.first_checked,
                    history: format!("{}b{}", state.history, to),
                };
                actions.push((verb, self.betting(next)));
            }
        }

        let tag = self.node_info.len() as u32;
        self.node_info.push(RiverNodeInfo {
            history: state.history.clone(),
            actions: actions.iter().map(|(name, _)| name.clone()).collect(),
        });
        TempNode::Action {
            player: actor,
            children: actions.into_iter().map(|(_, child)| child).collect(),
            tag,
        }
    }

    fn showdown(&mut self, state: &BetState) -> TempNode {
        self.terminal(state, TerminalKind::Showdown)
    }

    fn terminal(&mut self, state: &BetState, kind: TerminalKind) -> TempNode {
        let half_pot = Chips(self.config.pot.0 / 2);
        let contrib = PerPlayer::new(
            half_pot + state.contrib[Player::P0],
            half_pot + state.contrib[Player::P1],
        );
        let descriptor = TerminalDescriptor {
            kind,
            street: Street::River,
            pot: contrib[Player::P0] + contrib[Player::P1],
            contrib,
            stacks_before: PerPlayer::new(
                self.config.effective_stack + half_pot,
                self.config.effective_stack + half_pot,
            ),
        };
        let payoffs = self.pipeline.bake(&descriptor);
        let id = self.terminals.len() as u32;
        self.terminals.push(RiverTerminal { kind, payoffs });
        TempNode::Terminal { id, tag: 0 }
    }
}
