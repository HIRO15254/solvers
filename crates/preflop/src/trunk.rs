//! The preflop trunk: config, betting-tree builder, and session wrapper.
//!
//! Mirrors `holdem::postflop`'s shape (private recursive `Builder` producing
//! `engine::TempNode`s, tag-0-is-untagged node info convention,
//! `node_by_history` lookup) with the private-state space swapped from
//! 1,326 combos to the 169 classes.

use cards::{Chips, NUM_CLASSES, PerPlayer, Player, Range, Street};
use engine::{
    CompiledGame, F32Storage, I16Storage, NodeId, PublicTree, Storage, TempNode, TreeSpec,
};
use game::{PayoffPipeline, TerminalDescriptor, TerminalKind};

use crate::classes;
use crate::equity::EquityTable;
use crate::evaluator::PreflopEvaluator;
use crate::model::{ContinuationCtx, PostflopModel, fold_coef, showdown_coef};

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

    /// The node's info record (untagged nodes get the sentinel entry 0).
    pub fn info(&self, node: NodeId) -> &PreflopNodeInfo {
        &self.node_info[self.game.tree.tags[node as usize] as usize]
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
pub fn memory_usage(config: &PreflopConfig) -> MemoryEstimate {
    let mut counting = Counting {
        config,
        nodes: 0,
        terminals: 0,
        elements: 0,
        action_nodes: 0,
    };
    counting.betting(root_line_state(config));
    MemoryEstimate {
        f32_bytes: F32Storage::bytes_for(
            counting.elements as usize,
            counting.action_nodes as usize,
        ),
        i16_bytes: I16Storage::bytes_for(
            counting.elements as usize,
            counting.action_nodes as usize,
        ),
        nodes: counting.nodes,
        terminals: counting.terminals,
    }
}

/// Builds the trunk through the payoff pipeline.
///
/// `zero_sum` is `pipeline.is_zero_sum() && model.preserves_zero_sum()`.
/// Panics on inconsistent configs (blinds not `sb < bb`, stack not above
/// the big blind, empty ranges, or an SB with no legal action).
pub fn build_preflop_game(
    config: &PreflopConfig,
    table: &EquityTable,
    model: &dyn PostflopModel,
    pipeline: PayoffPipeline<'_>,
) -> PreflopGame {
    assert!(
        config.sb < config.bb,
        "small blind ({:?}) must be less than the big blind ({:?})",
        config.sb,
        config.bb
    );
    assert!(
        config.effective_stack > config.bb,
        "effective stack ({:?}) must exceed the big blind ({:?})",
        config.effective_stack,
        config.bb
    );

    let root_ranges = PerPlayer::new(
        classes::class_mass(&config.ranges[Player::P0]),
        classes::class_mass(&config.ranges[Player::P1]),
    );
    let mass = root_ranges.as_ref().map(|r| r.iter().sum::<f32>());
    assert!(
        mass[Player::P0] > 0.0,
        "P0 (SB) range must have positive mass"
    );
    assert!(
        mass[Player::P1] > 0.0,
        "P1 (BB) range must have positive mass"
    );

    let mut builder = Builder {
        config,
        model,
        pipeline,
        evaluator: PreflopEvaluator::new(table),
        node_info: vec![PreflopNodeInfo {
            history: "<untagged>".into(),
            actions: Vec::new(),
        }],
    };

    let root = builder.betting(root_line_state(config));

    let tree = PublicTree::compile(TreeSpec {
        root,
        masks: Vec::new(),
        transitions: Vec::new(),
        root_dims: PerPlayer::new(NUM_CLASSES as u32, NUM_CLASSES as u32),
    });
    assert!(
        tree.node(0).num_children >= 2,
        "root must have at least two legal actions"
    );

    let normalizer = normalizer(&root_ranges);
    let zero_sum = builder.pipeline.is_zero_sum() && model.preserves_zero_sum();

    let Builder {
        evaluator,
        node_info,
        ..
    } = builder;

    PreflopGame {
        game: CompiledGame {
            tree,
            evaluator,
            root_ranges: root_ranges.map(|r| r.to_vec()),
            normalizer,
            zero_sum,
        },
        node_info,
    }
}

/// Betting-line state threaded through the recursion (see the type docs'
/// betting recursion spec). `contrib` starts at the blinds, `last_raise_to`
/// at `bb` (so the first raise's minimum is `2 * bb`), `prev_raise_to` at
/// zero.
#[derive(Clone)]
struct LineState {
    to_act: Player,
    contrib: PerPlayer<Chips>,
    raises_used: u32,
    last_raise_to: Chips,
    prev_raise_to: Chips,
    bb_option: bool,
    history: String,
}

fn root_line_state(config: &PreflopConfig) -> LineState {
    LineState {
        to_act: Player::P0,
        contrib: PerPlayer::new(config.sb, config.bb),
        raises_used: 0,
        last_raise_to: config.bb,
        prev_raise_to: Chips(0),
        bb_option: false,
        history: String::new(),
    }
}

/// The shape of the non-raise actions offered at `state`: shared between the
/// real builder and the memory-usage dry run so they cannot disagree about
/// which of {Check, Fold+Limp, Fold+Call} applies.
enum BaseActions {
    /// Nothing outstanding (only the BB-option node after a limp): Check.
    Check,
    /// Facing a bet as the SB's opening action with the blind not yet
    /// raised: Fold, plus Limp iff `allow_limp`.
    FoldThenLimp { allow_limp: bool },
    /// Facing a bet anywhere else: Fold, Call.
    FoldThenCall,
}

fn base_actions(state: &LineState, config: &PreflopConfig) -> BaseActions {
    let opp = state.to_act.opponent();
    let outstanding = state.contrib[opp] - state.contrib[state.to_act];
    if outstanding == Chips::ZERO {
        BaseActions::Check
    } else if state.to_act == Player::P0
        && state.raises_used == 0
        && state.contrib[opp] == config.bb
    {
        BaseActions::FoldThenLimp {
            allow_limp: config.allow_limp,
        }
    } else {
        BaseActions::FoldThenCall
    }
}

/// Legal raise-to targets at `state`, ascending and deduplicated. Shared
/// between the real builder and the memory-usage dry run (see the type
/// docs' betting recursion spec for the derivation of `min_to` and the
/// per-level sizing).
fn raise_targets(state: &LineState, config: &PreflopConfig) -> Vec<Chips> {
    let opp = state.to_act.opponent();
    if state.raises_used >= config.max_raises || state.contrib[opp] >= config.effective_stack {
        return Vec::new();
    }
    let min_to = state.last_raise_to + (state.last_raise_to - state.prev_raise_to);
    let level = state.raises_used as usize;
    let mut raw: Vec<u32> = Vec::new();
    if level == 0 {
        for &size_bb in &config.open_sizes_bb {
            raw.push((size_bb * config.bb.0 as f64).round() as u32);
        }
    } else if !config.raise_factors.is_empty() {
        let idx = (level - 1).min(config.raise_factors.len() - 1);
        for &factor in &config.raise_factors[idx] {
            raw.push((factor * state.last_raise_to.0 as f64).round() as u32);
        }
    }
    let mut targets: Vec<Chips> = raw
        .into_iter()
        .map(|r| Chips(r).max(min_to).min(config.effective_stack))
        .collect();
    if config.include_allin {
        targets.push(config.effective_stack);
    }
    targets.sort();
    targets.dedup();
    targets
}

/// `Σ_{h,o} R0(h) * R1(o) * compat(h, o)`, computed in f64 straight from
/// `compat_counts`/`class_combo_counts` (full ranges must give exactly
/// `1,624,350`, see `classes::total_disjoint_pairs`).
fn normalizer(root_ranges: &PerPlayer<[f32; NUM_CLASSES]>) -> f64 {
    let counts = classes::compat_counts();
    let combo_counts = classes::class_combo_counts();
    let mut total = 0f64;
    for h in 0..NUM_CLASSES {
        for o in 0..NUM_CLASSES {
            let compat = counts[h * NUM_CLASSES + o] as f64
                / (combo_counts[h] as f64 * combo_counts[o] as f64);
            total += root_ranges[Player::P0][h] as f64 * root_ranges[Player::P1][o] as f64 * compat;
        }
    }
    total
}

struct Builder<'a> {
    config: &'a PreflopConfig,
    model: &'a dyn PostflopModel,
    pipeline: PayoffPipeline<'a>,
    evaluator: PreflopEvaluator,
    node_info: Vec<PreflopNodeInfo>,
}

impl Builder<'_> {
    fn extend_history(&self, base: &str, token: &str) -> String {
        if self.config.track_node_info {
            format!("{base}{token}")
        } else {
            String::new()
        }
    }

    fn action_label(&self, make: impl FnOnce() -> String) -> String {
        if self.config.track_node_info {
            make()
        } else {
            String::new()
        }
    }

    fn betting(&mut self, state: LineState) -> TempNode {
        let actor = state.to_act;
        let opp = actor.opponent();
        let mut actions: Vec<(String, TempNode)> = Vec::new();

        match base_actions(&state, self.config) {
            BaseActions::Check => {
                debug_assert!(
                    state.bb_option,
                    "outstanding == 0 should only happen at the BB-option node after a limp"
                );
                let history = self.extend_history(&state.history, "x");
                let label = self.action_label(|| "Check".into());
                let next = LineState {
                    history,
                    ..state.clone()
                };
                actions.push((label, self.showdown_or_continuation(&next)));
            }
            BaseActions::FoldThenLimp { allow_limp } => {
                actions.push((
                    self.action_label(|| "Fold".into()),
                    self.fold_terminal(&state, actor),
                ));
                if allow_limp {
                    let history = self.extend_history(&state.history, "c");
                    let label = self.action_label(|| "Limp".into());
                    let mut contrib = state.contrib;
                    contrib[actor] = self.config.bb;
                    let next = LineState {
                        contrib,
                        to_act: opp,
                        bb_option: true,
                        history,
                        ..state.clone()
                    };
                    actions.push((label, self.betting(next)));
                }
            }
            BaseActions::FoldThenCall => {
                actions.push((
                    self.action_label(|| "Fold".into()),
                    self.fold_terminal(&state, actor),
                ));
                let history = self.extend_history(&state.history, "c");
                let label = self.action_label(|| "Call".into());
                let mut contrib = state.contrib;
                contrib[actor] = state.contrib[opp];
                let next = LineState {
                    contrib,
                    history,
                    ..state.clone()
                };
                actions.push((label, self.showdown_or_continuation(&next)));
            }
        }

        for t in raise_targets(&state, self.config) {
            let history = self.extend_history(&state.history, &format!("r{}", t.0));
            let label = self.action_label(|| {
                let x = t.0 as f64 / self.config.bb.0 as f64;
                if t == self.config.effective_stack {
                    format!("All-in {x}bb")
                } else {
                    format!("Raise {x}bb")
                }
            });
            let mut contrib = state.contrib;
            contrib[actor] = t;
            let next = LineState {
                contrib,
                raises_used: state.raises_used + 1,
                prev_raise_to: state.last_raise_to,
                last_raise_to: t,
                to_act: opp,
                bb_option: false,
                history,
            };
            actions.push((label, self.betting(next)));
        }

        self.finish_action_node(actor, state.history, actions)
    }

    fn finish_action_node(
        &mut self,
        actor: Player,
        history: String,
        actions: Vec<(String, TempNode)>,
    ) -> TempNode {
        let tag = if self.config.track_node_info {
            let tag = self.node_info.len() as u32;
            self.node_info.push(PreflopNodeInfo {
                history,
                actions: actions.iter().map(|(name, _)| name.clone()).collect(),
            });
            tag
        } else {
            0
        };
        TempNode::Action {
            player: actor,
            children: actions.into_iter().map(|(_, child)| child).collect(),
            tag,
        }
    }

    fn fold_terminal(&mut self, state: &LineState, folder: Player) -> TempNode {
        let descriptor = TerminalDescriptor {
            kind: TerminalKind::Fold { folder },
            street: Street::Preflop,
            pot: state.contrib[Player::P0] + state.contrib[Player::P1],
            contrib: state.contrib,
            stacks_before: PerPlayer::new(self.config.effective_stack, self.config.effective_stack),
        };
        let baked = self.pipeline.bake(&descriptor);
        let coef = PerPlayer::new(fold_coef(&baked, Player::P0), fold_coef(&baked, Player::P1));
        let id = self.evaluator.push_terminal(coef);
        TempNode::Terminal { id, tag: 0 }
    }

    /// All-in showdown when both contributions already equal the effective
    /// stack, otherwise a postflop continuation. Both are stamped
    /// `Street::Flop` (see the type docs on rake semantics).
    fn showdown_or_continuation(&mut self, state: &LineState) -> TempNode {
        let contrib = state.contrib;
        let descriptor = TerminalDescriptor {
            kind: TerminalKind::Showdown,
            street: Street::Flop,
            pot: contrib[Player::P0] + contrib[Player::P1],
            contrib,
            stacks_before: PerPlayer::new(self.config.effective_stack, self.config.effective_stack),
        };
        let baked = self.pipeline.bake(&descriptor);
        let all_in = contrib[Player::P0] == self.config.effective_stack
            || contrib[Player::P1] == self.config.effective_stack;
        let coef = if all_in {
            PerPlayer::new(
                showdown_coef(&baked, Player::P0),
                showdown_coef(&baked, Player::P1),
            )
        } else {
            let ctx = ContinuationCtx {
                descriptor: &descriptor,
                payoffs: &baked,
            };
            PerPlayer::new(
                self.model.continuation_coef(&ctx, Player::P0),
                self.model.continuation_coef(&ctx, Player::P1),
            )
        };
        let id = self.evaluator.push_terminal(coef);
        TempNode::Terminal { id, tag: 0 }
    }
}

/// Counting-only mirror of [`Builder`]'s recursion, sharing [`base_actions`]
/// and [`raise_targets`] so node/terminal/storage counts can never disagree
/// with the real builder.
struct Counting<'a> {
    config: &'a PreflopConfig,
    nodes: u64,
    terminals: u64,
    elements: u64,
    action_nodes: u64,
}

impl Counting<'_> {
    fn betting(&mut self, state: LineState) {
        self.nodes += 1;
        self.action_nodes += 1;
        let actor = state.to_act;
        let opp = actor.opponent();
        let mut num_actions: u64 = 0;

        match base_actions(&state, self.config) {
            BaseActions::Check => {
                num_actions += 1;
                self.terminal();
            }
            BaseActions::FoldThenLimp { allow_limp } => {
                num_actions += 1;
                self.terminal(); // fold
                if allow_limp {
                    num_actions += 1;
                    let mut contrib = state.contrib;
                    contrib[actor] = self.config.bb;
                    let next = LineState {
                        contrib,
                        to_act: opp,
                        bb_option: true,
                        history: String::new(),
                        ..state.clone()
                    };
                    self.betting(next);
                }
            }
            BaseActions::FoldThenCall => {
                num_actions += 2;
                self.terminal(); // fold
                self.terminal(); // call
            }
        }

        let raises = raise_targets(&state, self.config);
        num_actions += raises.len() as u64;
        for t in raises {
            let mut contrib = state.contrib;
            contrib[actor] = t;
            let next = LineState {
                contrib,
                raises_used: state.raises_used + 1,
                prev_raise_to: state.last_raise_to,
                last_raise_to: t,
                to_act: opp,
                bb_option: false,
                history: String::new(),
            };
            self.betting(next);
        }

        self.elements += num_actions * NUM_CLASSES as u64;
    }

    fn terminal(&mut self) {
        self.nodes += 1;
        self.terminals += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{NodeKind, TerminalEvaluator};
    use game::{ChipEv, NoRake};
    use rand::{Rng, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    use crate::model::EquityShowdown;

    fn full_ranges() -> PerPlayer<Range> {
        PerPlayer::new(Range::full(), Range::full())
    }

    /// Equity values are irrelevant to tree-structure/memory tests, so a
    /// flat symmetric table keeps them simple.
    fn flat_table() -> EquityTable {
        let n = NUM_CLASSES * NUM_CLASSES;
        EquityTable::from_probabilities(vec![0.5; n], vec![0.0; n])
    }

    fn push_fold_config(stack: Chips) -> PreflopConfig {
        PreflopConfig {
            effective_stack: stack,
            sb: Chips(5),
            bb: Chips(10),
            ranges: full_ranges(),
            open_sizes_bb: Vec::new(),
            raise_factors: Vec::new(),
            max_raises: 1,
            include_allin: true,
            allow_limp: false,
            track_node_info: true,
        }
    }

    fn limp_config() -> PreflopConfig {
        PreflopConfig {
            effective_stack: Chips(1000),
            sb: Chips(5),
            bb: Chips(10),
            ranges: full_ranges(),
            open_sizes_bb: vec![2.5],
            raise_factors: Vec::new(),
            max_raises: 2,
            include_allin: true,
            allow_limp: true,
            track_node_info: true,
        }
    }

    fn min_raise_config() -> PreflopConfig {
        PreflopConfig {
            effective_stack: Chips(200),
            sb: Chips(5),
            bb: Chips(10),
            ranges: full_ranges(),
            open_sizes_bb: vec![2.5],
            raise_factors: vec![vec![3.0]],
            max_raises: 4,
            include_allin: true,
            allow_limp: false,
            track_node_info: true,
        }
    }

    fn terminal_count(tree: &PublicTree) -> u64 {
        tree.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Terminal)
            .count() as u64
    }

    #[test]
    fn push_fold_tree_structure() {
        let config = push_fold_config(Chips(100));
        let table = flat_table();
        let model = EquityShowdown::default();
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let game = build_preflop_game(&config, &table, &model, pipeline);

        let root = game.node_by_history("").expect("root");
        assert_eq!(root, 0, "root must be node 0");
        let root_info = game.info(root);
        assert_eq!(
            root_info.actions,
            vec!["Fold".to_string(), "All-in 10bb".to_string()]
        );

        let jam = game.node_by_history("r100").expect("jam node");
        let jam_info = game.info(jam);
        assert_eq!(
            jam_info.actions,
            vec!["Fold".to_string(), "Call".to_string()]
        );

        assert_eq!(game.game.tree.nodes.len(), 5, "expected 5 nodes total");
        assert_eq!(
            terminal_count(&game.game.tree),
            3,
            "expected 3 terminals total"
        );

        // Terminals are untagged, so history strings ending in a terminal
        // never resolve through node_by_history.
        assert!(game.node_by_history("f").is_none());
    }

    #[test]
    fn limp_mechanics() {
        let config = limp_config();
        let table = flat_table();
        let model = EquityShowdown::default();
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let game = build_preflop_game(&config, &table, &model, pipeline);

        let limp_node = game
            .node_by_history("c")
            .expect("bb-option node after limp");
        let info = game.info(limp_node);
        assert!(
            !info.actions.contains(&"Fold".to_string()),
            "nothing outstanding after a limp, so Fold must not be offered: {:?}",
            info.actions
        );
        assert!(info.actions.contains(&"Check".to_string()));
        assert!(
            info.actions.contains(&"Raise 2.5bb".to_string()),
            "BB's isoraise size must be 2.5bb: {:?}",
            info.actions
        );

        // Checking back ends the round: terminal, not in node_by_history.
        assert!(game.node_by_history("cx").is_none());
    }

    #[test]
    fn min_raise_and_dedupe() {
        let config = min_raise_config();
        let table = flat_table();
        let model = EquityShowdown::default();
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let game = build_preflop_game(&config, &table, &model, pipeline);

        assert!(game.node_by_history("r25").is_some(), "2.5bb open");
        let three_bet = game
            .node_by_history("r25r75")
            .expect("3x reraise to 75 (3 * 25)");

        let info = game.info(three_bet);
        let raise_labels: Vec<&String> = info.actions.iter().filter(|a| a.contains("bb")).collect();
        assert_eq!(
            raise_labels.len(),
            1,
            "the would-be 225 4-bet must clamp to the stack and dedupe with \
             include_allin into a single raise child: {:?}",
            info.actions
        );
        assert_eq!(raise_labels[0], "All-in 20bb");
        assert!(game.node_by_history("r25r75r200").is_some());
    }

    #[test]
    fn memory_usage_matches_real_build() {
        for config in [push_fold_config(Chips(100)), min_raise_config()] {
            let table = flat_table();
            let model = EquityShowdown::default();
            let pipeline = PayoffPipeline {
                rake: &NoRake,
                utility: &ChipEv,
            };
            let estimate = memory_usage(&config);
            let game = build_preflop_game(&config, &table, &model, pipeline);

            assert_eq!(estimate.nodes, game.game.tree.nodes.len() as u64);
            assert_eq!(estimate.terminals, terminal_count(&game.game.tree));
            let f32_bytes = F32Storage::bytes_for(
                game.game.tree.storage_len,
                game.game.tree.storage_refs.len(),
            );
            assert_eq!(estimate.f32_bytes, f32_bytes);
        }
    }

    #[test]
    fn zero_sum_terminal_identity() {
        let mut rng = ChaCha8Rng::seed_from_u64(123);
        let n = NUM_CLASSES * NUM_CLASSES;
        // The zero-sum identity relies on `win`/`tie` being a genuine
        // probability split (`win(h, o) + win(o, h) + tie(h, o) == 1`,
        // `tie` symmetric) — otherwise a showdown's two orientations aren't
        // complementary and there's nothing for the two players' baked
        // utilities to cancel against.
        let mut win = vec![0.0f64; n];
        let mut tie = vec![0.0f64; n];
        for h in 0..NUM_CLASSES {
            // Diagonal is its own reciprocal (`o == h`), so it needs an
            // exact 50/50 win split rather than an independent `frac` (a
            // plain `frac`/`1 - frac` pair would collide: both `win[h, o]`
            // and `win[o, h]` address the same cell when `o == h`).
            let t_diag = rng.gen_range(0.0..1.0);
            tie[h * NUM_CLASSES + h] = t_diag;
            win[h * NUM_CLASSES + h] = (1.0 - t_diag) / 2.0;
            for o in (h + 1)..NUM_CLASSES {
                let t = rng.gen_range(0.0..1.0);
                let frac = rng.gen_range(0.0..1.0);
                tie[h * NUM_CLASSES + o] = t;
                tie[o * NUM_CLASSES + h] = t;
                win[h * NUM_CLASSES + o] = frac * (1.0 - t);
                win[o * NUM_CLASSES + h] = (1.0 - frac) * (1.0 - t);
            }
        }
        let table = EquityTable::from_probabilities(win, tie);

        let model = EquityShowdown::default();
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let config = PreflopConfig::hu(100.0);
        let game = build_preflop_game(&config, &table, &model, pipeline);

        let r0: Vec<f32> = (0..NUM_CLASSES).map(|_| rng.gen_range(0.0..1.0)).collect();
        let r1: Vec<f32> = (0..NUM_CLASSES).map(|_| rng.gen_range(0.0..1.0)).collect();

        for t in 0..game.game.evaluator.num_terminals() as u32 {
            let mut out0 = vec![0f32; NUM_CLASSES];
            let mut out1 = vec![0f32; NUM_CLASSES];
            game.game.evaluator.eval(t, Player::P0, &r1, &mut out0);
            game.game.evaluator.eval(t, Player::P1, &r0, &mut out1);
            let sum0: f64 = out0
                .iter()
                .zip(&r0)
                .map(|(&v, &r)| v as f64 * r as f64)
                .sum();
            let sum1: f64 = out1
                .iter()
                .zip(&r1)
                .map(|(&v, &r)| v as f64 * r as f64)
                .sum();
            // Random unnormalized reaches over the full 169x169 grid can
            // push these sums into the hundreds of thousands, at which
            // scale `eval`'s f32 output rounding (it's built from an f64
            // accumulator, but the trait contract is f32 in/out) costs more
            // than a bare 1e-3 absolute — so scale the tolerance by the
            // magnitude involved; 1e-3 remains the floor for small sums.
            let tol = 1e-3 * (1.0 + sum0.abs().max(sum1.abs()));
            assert!(
                (sum0 + sum1).abs() < tol,
                "terminal {t}: p0 {sum0} + p1 {sum1} != 0 (tol {tol})"
            );
        }
    }

    /// Independently reproduces both players' best-response values from
    /// first principles (see the type docs) and checks them against the
    /// solver's own `best_response_value`, for a push/fold game against a
    /// deterministic, asymmetric, tie-free synthetic equity table.
    #[test]
    fn best_response_matches_independent_oracle() {
        let stack: u32 = 100;
        let sb: u32 = 5;
        let bb: u32 = 10;
        let config = push_fold_config(Chips(stack));

        let s = |x: usize| (x + 1) as f64;
        let mut win = vec![0.0f64; NUM_CLASSES * NUM_CLASSES];
        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                win[h * NUM_CLASSES + o] = 0.25 + 0.5 * s(h) / (s(h) + s(o));
            }
        }
        let tie = vec![0.0f64; NUM_CLASSES * NUM_CLASSES];
        let table = EquityTable::from_probabilities(win.clone(), tie);

        let model = EquityShowdown::default();
        let pipeline = PayoffPipeline {
            rake: &NoRake,
            utility: &ChipEv,
        };
        let pf = build_preflop_game(&config, &table, &model, pipeline);

        let root = pf.node_by_history("").expect("root");
        let jam = pf
            .node_by_history(&format!("r{stack}"))
            .expect("bb's response to the jam");

        let mut solver = engine::Solver::<_, engine::F32Storage>::new(
            pf.game,
            Box::new(engine::Dcfr::default()),
            Some(2000),
        );
        solver.run(2000);

        // Action order per the builder: root = [Fold, All-in], jam node =
        // [Fold, Call] (see `base_actions`/`betting`), so index 1 in each
        // action-major block is the "aggressive" column.
        let root_strategy = solver.average_strategy_at(root);
        let jam_strategy = solver.average_strategy_at(jam);
        let sigma_j: Vec<f64> = root_strategy[NUM_CLASSES..2 * NUM_CLASSES]
            .iter()
            .map(|&x| x as f64)
            .collect();
        let sigma_c: Vec<f64> = jam_strategy[NUM_CLASSES..2 * NUM_CLASSES]
            .iter()
            .map(|&x| x as f64)
            .collect();

        let counts = classes::compat_counts();
        let combo_counts = classes::class_combo_counts();
        let compat = |h: usize, o: usize| {
            counts[h * NUM_CLASSES + o] as f64 / (combo_counts[h] as f64 * combo_counts[o] as f64)
        };
        let r: Vec<f64> = combo_counts.iter().map(|&c| c as f64).collect();

        let mut z = 0f64;
        for h in 0..NUM_CLASSES {
            for o in 0..NUM_CLASSES {
                z += r[h] * r[o] * compat(h, o);
            }
        }

        let s_stack = stack as f64;
        let sb_f = sb as f64;
        let bb_f = bb as f64;

        // BB's best-response value.
        let mut bb_br = 0f64;
        for o in 0..NUM_CLASSES {
            let mut fold_leg = 0f64;
            let mut bb_fold_val = 0f64;
            let mut bb_call_val = 0f64;
            for h in 0..NUM_CLASSES {
                let w = r[h] * compat(h, o);
                fold_leg += w * (1.0 - sigma_j[h]) * sb_f;
                bb_fold_val += w * sigma_j[h] * (-bb_f);
                bb_call_val += w
                    * sigma_j[h]
                    * s_stack
                    * (win[o * NUM_CLASSES + h] - win[h * NUM_CLASSES + o]);
            }
            bb_br += r[o] * (fold_leg + bb_fold_val.max(bb_call_val));
        }
        bb_br /= z;

        // SB's best-response value. Both alternatives at SB's own (root)
        // decision node must be compared on the engine's unnormalized
        // per-hand scale: `Σ_o opp_reach[o] * compat(h, o) * payoff(h, o)`.
        // The jam branch already has that shape; the fold branch's payoff
        // is o-independent (`-sb`), but the terminal evaluator still dilutes
        // it by the same opponent-compatible mass `m_h = Σ_o R1[o] *
        // C(h, o)` (which happens to be the constant 1,225 for full ranges,
        // see `classes::total_disjoint_pairs`), so the fold alternative must
        // be `-sb * m_h`, not a bare `-sb` — comparing `-sb` directly against
        // the jam branch compares numbers on different scales and always
        // favors jamming.
        let mut sb_br = 0f64;
        for h in 0..NUM_CLASSES {
            let mut jam_val = 0f64;
            let mut m_h = 0f64;
            for o in 0..NUM_CLASSES {
                let w = r[o] * compat(h, o);
                m_h += w;
                jam_val += w
                    * (sigma_c[o]
                        * s_stack
                        * (win[h * NUM_CLASSES + o] - win[o * NUM_CLASSES + h])
                        + (1.0 - sigma_c[o]) * bb_f);
            }
            sb_br += r[h] * jam_val.max(-sb_f * m_h);
        }
        sb_br /= z;

        let br0 = solver.best_response_value(Player::P0);
        let br1 = solver.best_response_value(Player::P1);
        assert!(
            (br1 - bb_br).abs() < 1e-2,
            "BB best-response mismatch: solver {br1}, oracle {bb_br}"
        );
        assert!(
            (br0 - sb_br).abs() < 1e-2,
            "SB best-response mismatch: solver {br0}, oracle {sb_br}"
        );

        let expl = solver.exploitability();
        assert!(expl[Player::P0] >= -1e-6, "expl[P0] = {}", expl[Player::P0]);
        assert!(expl[Player::P1] >= -1e-6, "expl[P1] = {}", expl[Player::P1]);
        assert!(
            expl[Player::P0] + expl[Player::P1] < 0.5,
            "exploitability sum too large: {} + {}",
            expl[Player::P0],
            expl[Player::P1]
        );

        let ev0 = solver.expected_value(Player::P0);
        let ev1 = solver.expected_value(Player::P1);
        assert!((ev0 + ev1).abs() < 1e-6, "ev0 {ev0} + ev1 {ev1} != 0");
    }
}
