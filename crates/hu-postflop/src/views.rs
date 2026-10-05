//! Typed P1 artifact views and node-by-node comparisons.
//! Stored strategy and value blocks are read without solving. Missing river
//! blocks remain explicit errors; the interactive provider owns lazy re-solves.

use std::path::Path;

use crate::PostflopNodeInfo;
use crate::sol::{StreetsStored, dequantize_probs};
use anyhow::{Result, anyhow, bail};
use hu_engine::{NodeId, NodeKind};
use nlh::{PerPlayer, Player, Street, combo_cards};
use serde::Serialize;

/// One P1 artifact view.
pub enum ExportView {
    Summary,
    Tree,
    Actions,
    Strategy,
    Ev,
    Range,
}
/// Typed data for a selected artifact view.
#[derive(Serialize)]
#[serde(untagged)]
pub enum ExportData {
    Summary(Summary),
    Tree(Vec<TreeRow>),
    Actions(Vec<ActionRow>),
    Strategy(Vec<StrategyRow>),
    Ev(Vec<ValueRow>),
    Range(Vec<RangeRow>),
}

use crate::artifact::{Diagnostic, LoadedSol, load_sol};

/// Which nodes a view covers.
enum Selection {
    /// One node, named by history or by slash-separated action labels.
    One(NodeId),
    /// Every action node the artifact stores.
    All,
}

/// Read an artifact and return the selected view as typed data for caller-owned encoding.
pub fn export(
    path: &Path,
    view: ExportView,
    node: &str,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<ExportData> {
    let loaded = load_sol(path, 0, None, diagnostics)?;
    let selection = resolve_selection(&loaded, node)?;
    Ok(match view {
        ExportView::Summary => ExportData::Summary(summary(&loaded)),
        ExportView::Tree => ExportData::Tree(tree_rows(&loaded, &selection)),
        ExportView::Actions => ExportData::Actions(action_rows(&loaded, &selection)?),
        ExportView::Strategy => ExportData::Strategy(strategy_rows(&loaded, &selection)?),
        ExportView::Ev => ExportData::Ev(value_rows(&loaded, &selection)?),
        ExportView::Range => ExportData::Range(range_rows(&loaded)),
    })
}

/// Resolves `--node`: `all`, `root`, a betting-line history (`xr10c`), or
/// slash-separated action labels (`check/bet 10`).
fn resolve_selection(loaded: &LoadedSol, node: &str) -> Result<Selection> {
    if node == "all" {
        return Ok(Selection::All);
    }
    if node == "root" || node.is_empty() {
        return Ok(Selection::One(0));
    }
    if let Some(id) = loaded.pf_game.node_by_history(node) {
        return Ok(Selection::One(id));
    }
    // Fall back to the label path the REPL's `go` accepts, so a node the
    // user navigated to interactively can be named the same way here.
    let tree = &loaded.pf_game.game.tree;
    let mut current: NodeId = 0;
    for step in node.split('/').filter(|s| !s.trim().is_empty()) {
        let step = step.trim();
        let n = *tree.node(current);
        let children: Vec<NodeId> = tree.children(current).collect();
        let position = match n.kind {
            NodeKind::Action => {
                let info = &loaded.pf_game.node_info[tree.tags[current as usize] as usize];
                crate::queries::resolve_action(&info.actions, step)
                    .map_err(|e| anyhow!("resolving {node:?} at {:?}: {e}", info.history))?
            }
            NodeKind::Chance => (0..children.len())
                .find(|&pos| {
                    let label = crate::queries::chance_child_label(
                        tree,
                        &loaded.pf_game.node_info,
                        current,
                        pos,
                    );
                    label.eq_ignore_ascii_case(step)
                        || label.trim_end_matches('*').eq_ignore_ascii_case(step)
                })
                .ok_or_else(|| anyhow!("no dealt card {step:?} at this chance node"))?,
            NodeKind::Terminal => bail!("{node:?} walks past a terminal node"),
        };
        current = children[position];
    }
    Ok(Selection::One(current))
}

/// The action nodes a selection covers, in tree order.
fn selected_action_nodes(loaded: &LoadedSol, selection: &Selection) -> Vec<NodeId> {
    let tree = &loaded.pf_game.game.tree;
    match selection {
        Selection::One(id) => vec![*id],
        Selection::All => (0..tree.nodes.len() as NodeId)
            .filter(|id| {
                tree.node(*id).kind == NodeKind::Action
                    && loaded.blocks.contains_key(&tree.node(*id).aux)
            })
            .collect(),
    }
}

fn info_of(loaded: &LoadedSol, id: NodeId) -> &PostflopNodeInfo {
    let tag = loaded.pf_game.game.tree.tags[id as usize] as usize;
    &loaded.pf_game.node_info[tag]
}

fn street_name(street: Street) -> &'static str {
    match street {
        Street::Preflop => "preflop",
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
    }
}

fn seat_name(player: Player) -> &'static str {
    match player {
        Player::P0 => "oop",
        Player::P1 => "ip",
    }
}

// --- summary ----------------------------------------------------------------

#[derive(Serialize)]
/// Artifact metadata and rebuilt-tree coverage.
pub struct Summary {
    pub board: String,
    pub pot: serde_json::Number,
    pub effective_stack: serde_json::Number,
    pub min_bet: serde_json::Number,
    pub iterations: u64,
    pub ev_oop: f64,
    pub ev_ip: f64,
    pub expl_oop: f64,
    pub expl_ip: f64,
    pub nash_conv: f64,
    pub storage: String,
    pub wall_secs: f64,
    /// `full` or `no-rivers`: which streets carry stored strategies and
    /// values.
    pub streets_stored: String,
    pub nodes: usize,
    pub stored_nodes: usize,
}

fn summary(loaded: &LoadedSol) -> Summary {
    Summary {
        board: loaded
            .board
            .iter()
            .map(nlh::Card::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        pot: amount(loaded, loaded.config.pot.0),
        effective_stack: amount(loaded, loaded.config.effective_stack.0),
        min_bet: amount(loaded, loaded.config.min_bet.0),
        iterations: loaded.meta.iterations,
        ev_oop: loaded.meta.ev[0],
        ev_ip: loaded.meta.ev[1],
        expl_oop: loaded.meta.expl[0],
        expl_ip: loaded.meta.expl[1],
        nash_conv: loaded.meta.nash_conv,
        storage: loaded.meta.storage.clone(),
        wall_secs: loaded.meta.wall_secs,
        streets_stored: match loaded.mode {
            StreetsStored::Full => "full".into(),
            StreetsStored::NoRivers => "no-rivers".into(),
        },
        nodes: loaded.pf_game.game.tree.nodes.len(),
        stored_nodes: loaded.blocks.len(),
    }
}

fn amount(_loaded: &LoadedSol, value: u32) -> serde_json::Number {
    crate::prepare::bb(value as u64).parse().expect("BB number")
}

// --- tree -------------------------------------------------------------------

#[derive(Serialize)]
/// One action node in tree order.
pub struct TreeRow {
    pub history: String,
    pub street: String,
    pub actor: String,
    pub pot: serde_json::Number,
    pub actions: Vec<String>,
    pub stored: bool,
}

fn tree_rows(loaded: &LoadedSol, selection: &Selection) -> Vec<TreeRow> {
    selected_action_nodes(loaded, selection)
        .into_iter()
        .map(|id| {
            let node = loaded.pf_game.game.tree.node(id);
            let info = info_of(loaded, id);
            TreeRow {
                history: info.history.clone(),
                street: street_name(info.street).to_string(),
                actor: seat_name(node.player).to_string(),
                pot: amount(
                    loaded,
                    (info.contrib[Player::P0] + info.contrib[Player::P1]).0,
                ),
                actions: info.actions.clone(),
                stored: loaded.blocks.contains_key(&node.aux),
            }
        })
        .collect()
}

// --- shared per-node lookup -------------------------------------------------

/// A node whose stored blocks are present, with everything a per-hand view
/// needs.
struct StoredNode<'a> {
    id: NodeId,
    actor: Player,
    info: &'a PostflopNodeInfo,
    num_actions: usize,
    num_hands: usize,
    strategy: Vec<f32>,
    values: &'a [f32],
}

fn stored_node<'a>(loaded: &'a LoadedSol, id: NodeId) -> Result<StoredNode<'a>> {
    let tree = &loaded.pf_game.game.tree;
    let node = *tree.node(id);
    if node.kind != NodeKind::Action {
        bail!("node is not an action node; only action nodes carry strategies");
    }
    let sref = tree.storage_ref(&node);
    let info = info_of(loaded, id);
    let probs = loaded.blocks.get(&node.aux).ok_or_else(|| {
        anyhow!(
            "this artifact stores no strategy for {:?}: it was written with \
             [output] solution_streets = \"no-rivers\", which drops river nodes. \
             Re-solve with solution_streets = \"full\" to read river nodes from the artifact",
            info.history
        )
    })?;
    let values = loaded
        .values
        .get(&node.aux)
        .expect("value blocks cover the same nodes as strategy blocks");
    Ok(StoredNode {
        id,
        actor: node.player,
        info,
        num_actions: sref.num_actions as usize,
        num_hands: sref.num_hands as usize,
        strategy: dequantize_probs(probs, sref.num_actions as usize, sref.num_hands as usize)?,
        values,
    })
}

/// Reach of both players at `id`, for weighting per-node aggregates.
fn reach_at(loaded: &LoadedSol, id: NodeId) -> Result<PerPlayer<Vec<f32>>> {
    let tree = &loaded.pf_game.game.tree;
    let roots = &loaded.pf_game.game.root_ranges;
    let root_slices = PerPlayer::new(roots[Player::P0].as_slice(), roots[Player::P1].as_slice());
    let mut failure = None;
    let reach = hu_engine::reach_at(tree, root_slices, id, |node_id, sref, out| {
        let aux = tree.node(node_id).aux;
        match loaded.blocks.get(&aux) {
            Some(probs) => {
                match dequantize_probs(probs, sref.num_actions as usize, sref.num_hands as usize) {
                    Ok(avg) => out.copy_from_slice(&avg),
                    Err(error) => failure = Some(anyhow!(error)),
                }
            }
            None => {
                failure = Some(anyhow!(
                    "the path to this node crosses a river node the artifact does not store"
                ))
            }
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(reach),
    }
}

fn combo_label(index: usize) -> String {
    let (hi, lo) = combo_cards(index);
    format!("{hi}{lo}")
}

// --- actions ----------------------------------------------------------------

#[derive(Serialize)]
/// One reach-weighted action frequency.
pub struct ActionRow {
    pub history: String,
    pub street: String,
    pub actor: String,
    pub action: String,
    pub frequency: f64,
}

fn action_rows(loaded: &LoadedSol, selection: &Selection) -> Result<Vec<ActionRow>> {
    let mut rows = Vec::new();
    for id in selected_action_nodes(loaded, selection) {
        let node = stored_node(loaded, id)?;
        let reach = reach_at(loaded, id)?;
        let freqs = crate::prepare::action_frequencies(
            &node.strategy,
            &reach[node.actor],
            node.num_actions,
            node.num_hands,
        );
        for (label, frequency) in node.info.actions.iter().zip(freqs) {
            rows.push(ActionRow {
                history: node.info.history.clone(),
                street: street_name(node.info.street).to_string(),
                actor: seat_name(node.actor).to_string(),
                action: label.clone(),
                frequency,
            });
        }
    }
    Ok(rows)
}

// --- strategy ---------------------------------------------------------------

#[derive(Serialize)]
/// One reachable concrete combo and its action probabilities.
pub struct StrategyRow {
    pub history: String,
    pub actor: String,
    pub combo: String,
    pub weight: f32,
    pub probabilities: Vec<f32>,
}

fn strategy_rows(loaded: &LoadedSol, selection: &Selection) -> Result<Vec<StrategyRow>> {
    let mut rows = Vec::new();
    for id in selected_action_nodes(loaded, selection) {
        let node = stored_node(loaded, id)?;
        let reach = reach_at(loaded, id)?;
        for hand in 0..node.num_hands {
            let weight = reach[node.actor][hand];
            if weight <= 0.0 {
                continue;
            }
            rows.push(StrategyRow {
                history: node.info.history.clone(),
                actor: seat_name(node.actor).to_string(),
                combo: combo_label(hand),
                weight,
                probabilities: (0..node.num_actions)
                    .map(|a| node.strategy[a * node.num_hands + hand])
                    .collect(),
            });
        }
    }
    Ok(rows)
}

// --- ev ---------------------------------------------------------------------

#[derive(Serialize)]
/// One reachable concrete combo and its stored utility.
pub struct ValueRow {
    pub history: String,
    pub seat: String,
    pub combo: String,
    pub weight: f32,
    /// Expected utility relative to the original subgame start, including
    /// wagers already made. Chips for chip EV, prize units for ICM.
    pub ev: f32,
}

fn value_rows(loaded: &LoadedSol, selection: &Selection) -> Result<Vec<ValueRow>> {
    let mut rows = Vec::new();
    for id in selected_action_nodes(loaded, selection) {
        let node = stored_node(loaded, id)?;
        let reach = reach_at(loaded, id)?;
        for seat in Player::BOTH {
            let base = match seat {
                Player::P0 => 0,
                Player::P1 => node.num_hands,
            };
            for hand in 0..node.num_hands {
                let weight = reach[seat][hand];
                if weight <= 0.0 {
                    continue;
                }
                rows.push(ValueRow {
                    history: node.info.history.clone(),
                    seat: seat_name(seat).to_string(),
                    combo: combo_label(hand),
                    weight,
                    ev: node.values[base + hand],
                });
            }
        }
        let _ = node.id;
    }
    Ok(rows)
}

// --- range ------------------------------------------------------------------

#[derive(Serialize)]
/// One concrete combo in a starting range.
pub struct RangeRow {
    pub seat: String,
    pub combo: String,
    pub weight: f32,
}

fn range_rows(loaded: &LoadedSol) -> Vec<RangeRow> {
    let mut rows = Vec::new();
    for seat in Player::BOTH {
        let range = &loaded.pf_game.game.root_ranges[seat];
        for (hand, &weight) in range.iter().enumerate() {
            if weight <= 0.0 {
                continue;
            }
            rows.push(RangeRow {
                seat: seat_name(seat).to_string(),
                combo: combo_label(hand),
                weight,
            });
        }
    }
    rows
}

/// Compares two postflop artifacts node by node.
///
/// Both embed the config that produced them, so the same tree shape means
/// corresponding `sref`s — the only basis on which strategies and values
/// can be lined up. Artifacts of the same shape but a different spot are
/// refused unless the caller says otherwise with `--cross-game`.
pub fn compare(
    left_path: &Path,
    right_path: &Path,
    cross_game: bool,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<ComparisonReport> {
    let left = load_sol(left_path, 0, None, diagnostics)?;
    let right = load_sol(right_path, 0, None, diagnostics)?;
    let mut left_srefs: Vec<u32> = left.blocks.keys().copied().collect();
    let mut right_srefs: Vec<u32> = right.blocks.keys().copied().collect();
    left_srefs.sort_unstable();
    right_srefs.sort_unstable();
    if left.pf_game.game.tree.nodes.len() != right.pf_game.game.tree.nodes.len()
        || left_srefs != right_srefs
    {
        bail!(
            "artifacts describe different trees ({} vs {} nodes, {} vs {} stored nodes); \
             only solves of the same tree can be compared node by node",
            left.pf_game.game.tree.nodes.len(),
            right.pf_game.game.tree.nodes.len(),
            left.blocks.len(),
            right.blocks.len(),
        );
    }
    if !cross_game
        && (left.config.board != right.config.board
            || left.config.pot != right.config.pot
            || left.config.effective_stack != right.config.effective_stack)
    {
        bail!(
            "artifacts share a tree shape but not a spot (board, pot, or stack differ); \
             pass --cross-game to compare them anyway"
        );
    }

    let mut nodes = 0usize;
    let mut strategy_total = 0.0f64;
    let mut strategy_max = 0.0f64;
    let mut worst_strategy = String::new();
    let mut ev_total = 0.0f64;
    let mut ev_max = 0.0f64;
    let mut worst_ev = String::new();

    for id in selected_action_nodes(&left, &Selection::All) {
        let a = stored_node(&left, id)?;
        let b = stored_node(&right, id)?;
        if a.strategy.len() != b.strategy.len() || a.values.len() != b.values.len() {
            bail!(
                "node {:?} has different shapes in the two artifacts",
                a.info.history
            );
        }
        nodes += 1;

        // Per-hand L1 over the action distribution, averaged over hands:
        // the usual "how differently does this node play" number, bounded
        // by 2 and independent of how many actions the node offers.
        let mut node_l1 = 0.0f64;
        for hand in 0..a.num_hands {
            for action in 0..a.num_actions {
                let index = action * a.num_hands + hand;
                node_l1 += (a.strategy[index] - b.strategy[index]).abs() as f64;
            }
        }
        let node_l1 = node_l1 / a.num_hands.max(1) as f64;
        strategy_total += node_l1;
        if node_l1 > strategy_max {
            strategy_max = node_l1;
            worst_strategy = a.info.history.clone();
        }

        let node_ev = a
            .values
            .iter()
            .zip(b.values)
            .map(|(x, y)| (x - y).abs() as f64)
            .fold(0.0, f64::max);
        ev_total += node_ev;
        if node_ev > ev_max {
            ev_max = node_ev;
            worst_ev = a.info.history.clone();
        }
    }

    let report = ComparisonReport {
        nodes,
        mean_strategy_l1: strategy_total / nodes.max(1) as f64,
        max_strategy_l1: strategy_max,
        max_strategy_node: worst_strategy,
        mean_max_ev_delta: ev_total / nodes.max(1) as f64,
        max_ev_delta: ev_max,
        max_ev_node: worst_ev,
        ev_oop: [left.meta.ev[0], right.meta.ev[0]],
        ev_ip: [left.meta.ev[1], right.meta.ev[1]],
        nash_conv: [left.meta.nash_conv, right.meta.nash_conv],
    };
    Ok(report)
}

#[derive(Serialize)]
/// Node-by-node strategy and utility differences between compatible artifacts.
pub struct ComparisonReport {
    pub nodes: usize,
    /// Mean over nodes of the per-hand-averaged L1 distance between the two
    /// action distributions. `0` is identical play, `2` is disjoint.
    pub mean_strategy_l1: f64,
    pub max_strategy_l1: f64,
    pub max_strategy_node: String,
    /// Mean over nodes of that node's largest per-hand EV difference, in
    /// chips.
    pub mean_max_ev_delta: f64,
    pub max_ev_delta: f64,
    pub max_ev_node: String,
    pub ev_oop: [f64; 2],
    pub ev_ip: [f64; 2],
    pub nash_conv: [f64; 2],
}
