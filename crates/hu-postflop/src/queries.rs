//! P1 live solve and read-only node queries for viewers.
use crate::artifact::{EvSummary, StrategyProvider};
use crate::{PostflopEvaluator, PostflopNodeInfo, class_average, class_weights, range_equity};
use crate::{prepare, run};
use anyhow::Result;
use hu_engine::{
    CompiledGame, F32Storage, I16Storage, NodeId, NodeKind, ReachMap, Solver, Storage,
};
use nlh::{Card, PerPlayer, Player, combo_cards};
use std::sync::atomic::AtomicBool;
use std::time::Instant;
enum LiveSolver {
    F32(Solver<PostflopEvaluator, F32Storage>),
    I16(Solver<PostflopEvaluator, I16Storage>),
}
/// A live solve with its display metadata and root summary.
pub struct LiveSolution {
    solver: LiveSolver,
    pub node_info: Vec<PostflopNodeInfo>,
    pub board: Vec<Card>,
    pub summary: run::RunSummary,
}
impl LiveSolution {
    /// Compiled tree and root ranges for read-only navigation.
    pub fn game(&self) -> &CompiledGame<PostflopEvaluator> {
        match &self.solver {
            LiveSolver::F32(s) => s.game(),
            LiveSolver::I16(s) => s.game(),
        }
    }
}
/// Solve P1 and lend its typed session to a viewer within the configured local pool.
/// Returns the viewer's result after applying the configured storage and stop settings.
pub fn with_live<T: Send>(
    p: &prepare::Prepared,
    iterations: Option<u64>,
    target: Option<f64>,
    cancel: &AtomicBool,
    viewer: impl FnOnce(&LiveSolution) -> Result<T> + Send,
) -> Result<T> {
    run::with_threads(prepare::threads(p)?, || {
        let solved = match p.settings.solver.storage {
            crate::input::Storage::F32 => {
                live::<F32Storage>(p, iterations, target, cancel, LiveSolver::F32)
            }
            crate::input::Storage::I16 => {
                live::<I16Storage>(p, iterations, target, cancel, LiveSolver::I16)
            }
        }?;
        viewer(&solved)
    })
}
fn live<S: Storage>(
    p: &prepare::Prepared,
    iterations: Option<u64>,
    target: Option<f64>,
    cancel: &AtomicBool,
    wrap: impl FnOnce(Solver<PostflopEvaluator, S>) -> LiveSolver,
) -> Result<LiveSolution> {
    let start = Instant::now();
    let (solver, node_info) = run::query::<S>(p, iterations, target, cancel)?;
    let summary = run::summarize(
        &solver,
        start.elapsed(),
        run::subgame_ev(run::solver_ev(&solver), p.payoff.ev_offset()),
    );
    Ok(LiveSolution {
        solver: wrap(solver),
        node_info,
        summary,
        board: p.config.board.clone(),
    })
}
/// Strategy source borrowing an already solved live session.
pub struct LiveProvider<'a>(pub &'a LiveSolution);
impl StrategyProvider for LiveProvider<'_> {
    fn average_strategy(&mut self, node: NodeId) -> Result<Vec<f32>> {
        Ok(match &self.0.solver {
            LiveSolver::F32(s) => s.average_strategy_at(node),
            LiveSolver::I16(s) => s.average_strategy_at(node),
        })
    }
    fn ev_summary(&self) -> EvSummary {
        let s = &self.0.summary;
        EvSummary {
            ev: [s.ev[Player::P0], s.ev[Player::P1]],
            expl: [s.expl_p0, s.expl_p1],
            nash_conv: s.nash_conv,
            iterations: s.iterations,
            at_export: false,
        }
    }
}
/// Both players' reach at a node, using the selected strategy source.
pub fn reach_here(
    game: &CompiledGame<PostflopEvaluator>,
    provider: &mut dyn StrategyProvider,
    current: NodeId,
) -> Result<PerPlayer<Vec<f32>>> {
    let tree = &game.tree;
    let roots = PerPlayer::new(
        game.root_ranges[Player::P0].as_slice(),
        game.root_ranges[Player::P1].as_slice(),
    );
    let mut failure = None;
    let reach = hu_engine::reach_at(tree, roots, current, |id, _, out| {
        match provider.average_strategy(id) {
            Ok(avg) => out.copy_from_slice(&avg),
            Err(error) => failure = Some(error),
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(reach),
    }
}
/// Values and weights of a 13x13 hand-class grid.
pub struct Grid {
    pub weights: [f64; 169],
    pub values: [f64; 169],
}
/// Reach-weighted frequencies for every action at this node.
pub fn action_frequencies(
    game: &CompiledGame<PostflopEvaluator>,
    current: NodeId,
    avg: &[f32],
    reach: &PerPlayer<Vec<f32>>,
) -> Vec<f64> {
    let node = game.tree.node(current);
    let sref = game.tree.storage_ref(node);
    prepare::action_frequencies(
        avg,
        &reach[node.player],
        sref.num_actions as usize,
        sref.num_hands as usize,
    )
}
/// Class-averaged frequency grid for one action, using node reach.
pub fn action_grid(
    game: &CompiledGame<PostflopEvaluator>,
    current: NodeId,
    pos: usize,
    avg: &[f32],
    reach: &PerPlayer<Vec<f32>>,
) -> Grid {
    let node = game.tree.node(current);
    let hands = game.tree.storage_ref(node).num_hands as usize;
    let weight = game
        .evaluator
        .hands
        .expand(node.player, &reach[node.player]);
    let value = game
        .evaluator
        .hands
        .expand(node.player, &avg[pos * hands..(pos + 1) * hands]);
    Grid {
        weights: class_weights(&weight),
        values: class_average(&weight, &value).map(|v| v * 100.0),
    }
}
/// Root-range weights as percentages of the largest hand-class weight.
pub fn range_grid(game: &CompiledGame<PostflopEvaluator>, player: Player) -> Grid {
    let weights = class_weights(
        &game
            .evaluator
            .hands
            .expand(player, &game.root_ranges[player]),
    );
    let max = weights.iter().cloned().fold(0.0, f64::max);
    let values = weights.map(|w| {
        if max > 0.0 && w > 0.0 {
            w / max * 100.0
        } else {
            0.0
        }
    });
    Grid { weights, values }
}
/// Current-node equity; the viewer may retain the result until navigation changes.
pub fn equity(
    game: &CompiledGame<PostflopEvaluator>,
    board: &[Card],
    history: &str,
    reach: &PerPlayer<Vec<f32>>,
) -> PerPlayer<Vec<f32>> {
    let mut board = board.to_vec();
    for token in history.split('[').skip(1) {
        if let Some(card) = token.split(']').next().and_then(|s| s.parse::<Card>().ok()) {
            board.push(card);
        }
    }
    let hands = &game.evaluator.hands;
    let global = PerPlayer::new(
        hands.expand(Player::P0, &reach[Player::P0]),
        hands.expand(Player::P1, &reach[Player::P1]),
    );
    range_equity(&board, &global)
}
/// Class-averaged OOP equity percentages against the current IP reach.
pub fn equity_grid(
    game: &CompiledGame<PostflopEvaluator>,
    reach: &PerPlayer<Vec<f32>>,
    equity: &PerPlayer<Vec<f32>>,
) -> Grid {
    let weight = game.evaluator.hands.expand(Player::P0, &reach[Player::P0]);
    Grid {
        weights: class_weights(&weight),
        values: class_average(&weight, &equity[Player::P0]).map(|v| v * 100.0),
    }
}
/// One concrete combo's per-action probabilities.
pub struct ComboRow {
    pub combo: String,
    pub probabilities: Vec<f32>,
}
/// Select combos by the viewer's range expression and return their probabilities.
pub struct ComboClass(nlh::Range);
/// Parse the viewer's hand-class expression before reading a strategy.
pub fn parse_class(arg: &str) -> Result<ComboClass, String> {
    arg.parse::<nlh::Range>()
        .map(ComboClass)
        .map_err(|e| format!("parsing {arg:?}: {e}"))
}
/// Probabilities for the concrete combos selected by a parsed expression.
pub fn combo_rows(
    class: &ComboClass,
    hands: &crate::PostflopHands,
    player: Player,
    actions: usize,
    avg: &[f32],
) -> Vec<ComboRow> {
    let num_hands = hands.len(player);
    hands
        .combos(player)
        .iter()
        .enumerate()
        .filter(|&(_, &c)| class.0.weight(c as usize) > 0.0)
        .map(|(local, &combo)| {
            let (hi, lo) = combo_cards(combo as usize);
            ComboRow {
                combo: format!("{hi}{lo}"),
                probabilities: (0..actions).map(|a| avg[a * num_hands + local]).collect(),
            }
        })
        .collect()
}
/// Resolve a typed action label or numeric index.
pub fn resolve_action(actions: &[String], arg: &str) -> Result<usize, String> {
    if let Some(p) = actions.iter().position(|a| a == arg) {
        return Ok(p);
    }
    match arg.parse::<usize>() {
        Ok(idx) if idx < actions.len() => Ok(idx),
        Ok(idx) => Err(format!(
            "action index {idx} out of range (0..{})",
            actions.len()
        )),
        Err(_) => Err(format!("unknown action {arg:?}")),
    }
}

/// Label for a chance node's `pos`-th child: the dealt card. For a `Mask`
/// deal (the common unmerged case) this comes straight from the card-removal
/// builder metadata. For a `Transition` deal (an iso-merged class:
/// several structurally-equivalent cards folded into one branch) there is no
/// single mask to read a card from, so instead this reads the representative
/// card straight out of the child action node's own recorded history -- its
/// last bracketed `[Xy]` token is exactly the card the builder dealt to
/// reach it -- and appends `*` to mark that the branch stands in for a whole
/// merged class (see `cmd_help`'s iso-merging note). Falls back to a bare
/// `dealN` when neither source applies (e.g. an `Identity` map, which this
/// builder never uses at a chance node, or an untagged/non-action child).
pub fn chance_child_label(
    game: &CompiledGame<PostflopEvaluator>,
    node_info: &[PostflopNodeInfo],
    node_id: NodeId,
    pos: usize,
) -> String {
    let tree = &game.tree;
    let node = tree.node(node_id);
    let deal = tree.deal(node, pos);
    match deal.maps[Player::P0] {
        ReachMap::Mask(m) => game.evaluator.mask_cards[m as usize].to_string(),
        ReachMap::Transition(_) => tree
            .children(node_id)
            .nth(pos)
            .filter(|&child_id| tree.node(child_id).kind == NodeKind::Action)
            .and_then(|child_id| {
                let tag = tree.tags[child_id as usize] as usize;
                last_bracketed_card(&node_info[tag].history)
            })
            .map(|card| format!("{card}*"))
            .unwrap_or_else(|| format!("deal{pos}")),
        ReachMap::Identity => format!("deal{pos}"),
    }
}

/// Extracts the card inside the last `[Xy]` token in a history string (e.g.
/// `"xx[9d]"` -> `Some("9d")`), or `None` if there is no bracketed token.
fn last_bracketed_card(history: &str) -> Option<&str> {
    let start = history.rfind('[')?;
    let end = history[start..].find(']')? + start;
    Some(&history[start + 1..end])
}

/// Convert a displayed action label into its tree-history token.
pub fn history_token(label: &str) -> String {
    match label {
        "check" => "x".to_string(),
        "fold" => "f".to_string(),
        "call" => "c".to_string(),
        _ => {
            let amount = label.rsplit(' ').next().unwrap_or(label);
            format!("r{amount}")
        }
    }
}
