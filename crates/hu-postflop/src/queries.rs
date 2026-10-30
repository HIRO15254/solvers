//! P1 live solve and read-only node queries for viewers.
use crate::artifact::{EvSummary, StrategyProvider};
use crate::index::{GameIndex, TreeView};
use crate::{PostflopEvaluator, class_average, class_weights, range_equity};
use crate::{prepare, run};
use anyhow::Result;
use hu_engine::{F32Storage, I16Storage, MixedStorage, NodeId, Solver, Storage};
use nlh::{Card, PerPlayer, Player, combo_cards};
use std::sync::atomic::AtomicBool;
use std::time::Instant;
enum LiveSolver {
    F32(Solver<PostflopEvaluator, F32Storage>),
    I16(Solver<PostflopEvaluator, I16Storage>),
    Mixed(Solver<PostflopEvaluator, MixedStorage>),
}
/// A live solve with its viewer index and root summary.
pub struct LiveSolution {
    solver: LiveSolver,
    index: GameIndex,
    pub board: Vec<Card>,
    pub summary: run::RunSummary,
}
impl LiveSolution {
    /// Compiled tree and its index for read-only navigation.
    pub fn view(&self) -> TreeView<'_> {
        let game = match &self.solver {
            LiveSolver::F32(s) => s.game(),
            LiveSolver::I16(s) => s.game(),
            LiveSolver::Mixed(s) => s.game(),
        };
        TreeView {
            tree: &game.tree,
            index: &self.index,
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
            crate::input::Storage::I16F32Avg => {
                live::<MixedStorage>(p, iterations, target, cancel, LiveSolver::Mixed)
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
    let (solver, index, (ev, expl), profile) = run::query::<S>(p, iterations, target, cancel)?;
    let mut summary = run::summary_from_evaluation(
        &solver,
        start.elapsed(),
        run::subgame_ev(ev, p.payoff.ev_offset()),
        expl,
    );
    if let Some(report) = &profile {
        summary.frequencies.clone_from(&report.frequencies);
    }
    summary.profile = profile;
    Ok(LiveSolution {
        solver: wrap(solver),
        index,
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
            LiveSolver::Mixed(s) => s.average_strategy_at(node),
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
    view: TreeView<'_>,
    provider: &mut dyn StrategyProvider,
    current: NodeId,
) -> Result<PerPlayer<Vec<f32>>> {
    let ranges = view.root_ranges();
    let roots = PerPlayer::new(ranges[Player::P0].as_slice(), ranges[Player::P1].as_slice());
    let mut failure = None;
    let path = hu_engine::path_from_root(&view.index.parents, current);
    let reach = hu_engine::reach_on_path(view.tree, roots, &path, |id, _, out| {
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
    view: TreeView<'_>,
    current: NodeId,
    avg: &[f32],
    reach: &PerPlayer<Vec<f32>>,
) -> Vec<f64> {
    let node = view.tree.node(current);
    let sref = view.tree.storage_ref(node);
    prepare::action_frequencies(
        avg,
        &reach[node.player],
        sref.num_actions as usize,
        sref.num_hands as usize,
    )
}
/// Class-averaged frequency grid for one action, using node reach.
pub fn action_grid(
    view: TreeView<'_>,
    current: NodeId,
    pos: usize,
    avg: &[f32],
    reach: &PerPlayer<Vec<f32>>,
) -> Grid {
    let node = view.tree.node(current);
    let hands = view.tree.storage_ref(node).num_hands as usize;
    let weight = view.hands().expand(node.player, &reach[node.player]);
    let value = view
        .hands()
        .expand(node.player, &avg[pos * hands..(pos + 1) * hands]);
    Grid {
        weights: class_weights(&weight),
        values: class_average(&weight, &value).map(|v| v * 100.0),
    }
}
/// Root-range weights as percentages of the largest hand-class weight.
pub fn range_grid(view: TreeView<'_>, player: Player) -> Grid {
    let weights = class_weights(&view.hands().expand(player, &view.root_ranges()[player]));
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
    view: TreeView<'_>,
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
    let hands = view.hands();
    let global = PerPlayer::new(
        hands.expand(Player::P0, &reach[Player::P0]),
        hands.expand(Player::P1, &reach[Player::P1]),
    );
    range_equity(&board, &global)
}
/// Class-averaged OOP equity percentages against the current IP reach.
pub fn equity_grid(
    view: TreeView<'_>,
    reach: &PerPlayer<Vec<f32>>,
    equity: &PerPlayer<Vec<f32>>,
) -> Grid {
    let weight = view.hands().expand(Player::P0, &reach[Player::P0]);
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
pub fn resolve_action(actions: &[&str], arg: &str) -> Result<usize, String> {
    if let Some(p) = actions.iter().position(|a| *a == arg) {
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

/// Resolve labels by their rounded chip amount, preserving action kind.
pub(crate) fn resolve_scaled_action(
    actions: &[&str],
    arg: &str,
    scale: nlh::ChipScale,
    rounding: nlh::Rounding,
) -> Result<usize, String> {
    for prefix in ["bet ", "raise to "] {
        if let Some(wanted) = arg.strip_prefix(prefix)
            && let Some(pos) = actions.iter().position(|label| {
                label.strip_prefix(prefix).is_some_and(|actual| {
                    history_token_matches(
                        &format!("r{actual}"),
                        &format!("r{wanted}"),
                        scale,
                        rounding,
                    )
                })
            })
        {
            return Ok(pos);
        }
    }
    resolve_action(actions, arg)
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

/// Compare history actions by chip amount, rounding user-written BB amounts.
pub(crate) fn history_token_matches(
    actual: &str,
    wanted: &str,
    scale: nlh::ChipScale,
    rounding: nlh::Rounding,
) -> bool {
    if actual == wanted {
        return true;
    }
    fn amount(token: &str, scale: nlh::ChipScale, rounding: nlh::Rounding) -> Option<nlh::MwChips> {
        let text = token.strip_prefix('r')?;
        if let Some(chips) = text.strip_suffix('c') {
            chips.parse::<u64>().ok().map(nlh::MwChips)
        } else {
            scale
                .parse_bb_rounded(text.strip_suffix("bb").unwrap_or(text), rounding)
                .ok()
                .map(|(chips, _)| chips)
        }
    }
    match (
        amount(actual, scale, rounding),
        amount(wanted, scale, rounding),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Parse ambiguous `rNc` as either a chip amount or a BB amount followed by
/// a call, preferring the table's canonical spelling.
pub(crate) fn selector_history_steps(selector: &str, scale: nlh::ChipScale) -> Vec<Vec<&str>> {
    fn parse<'a>(
        rest: &'a str,
        chips_first: bool,
        steps: &mut Vec<&'a str>,
        out: &mut Vec<Vec<&'a str>>,
    ) {
        if rest.is_empty() {
            out.push(steps.clone());
            return;
        }
        let len = match rest.as_bytes()[0] {
            b'x' | b'c' | b'f' => 1,
            b'r' => {
                1 + rest[1..]
                    .bytes()
                    .take_while(|b| b.is_ascii_digit() || *b == b'.')
                    .count()
            }
            b'[' => match rest.find(']') {
                Some(end) => end + 1,
                None => return,
            },
            _ => return,
        };
        let len = if rest.starts_with('r') && rest[len..].starts_with("bb") {
            len + 2
        } else {
            len
        };
        let ambiguous = rest.starts_with('r') && rest.as_bytes().get(len) == Some(&b'c');
        for suffix in [chips_first, !chips_first] {
            if suffix && !ambiguous {
                continue;
            }
            let end = len + usize::from(suffix);
            steps.push(&rest[..end]);
            parse(&rest[end..], chips_first, steps, out);
            steps.pop();
        }
    }
    let mut out = Vec::new();
    parse(selector, !scale.is_decimal(), &mut Vec::new(), &mut out);
    out
}
