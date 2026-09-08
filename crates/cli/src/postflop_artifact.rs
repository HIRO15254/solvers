//! Stable JSON/CSV views over a postflop `.sol` artifact.
//!
//! The multiway counterpart is [`crate::multiway_artifact`]; the two share
//! `ExportView`/`ExportFormat` and their view names mean the same thing on
//! both sides, so a caller that knows one knows the other. What differs is
//! what a postflop node is: two seats, 1,326 concrete combos, and a betting
//! line rather than a seat-indexed public history.
//!
//! Everything here reads the artifact. Strategies and per-hand values are
//! both stored (see `formats::ValueBlock`), so no view has to re-solve —
//! except a river node in a `no-rivers` artifact, which carries neither and
//! is reported as such rather than silently recomputed.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use cards::{PerPlayer, Player, Street, combo_cards};
use engine::{NodeId, NodeKind};
use formats::{StreetsStored, dequantize_probs};
use holdem::PostflopNodeInfo;
use serde::Serialize;

use crate::multiway_artifact::{ExportFormat, ExportView};
use crate::sol::{LoadedSol, load_sol};

/// Which nodes a view covers.
enum Selection {
    /// One node, named by history or by slash-separated action labels.
    One(NodeId),
    /// Every action node the artifact stores.
    All,
}

/// Renders `view` over the artifact at `path` and writes it to `output` (or
/// stdout).
pub fn export(
    path: &Path,
    view: ExportView,
    format: ExportFormat,
    node: &str,
    output: Option<&Path>,
) -> Result<()> {
    let loaded = load_sol(path, 0, None)?;
    let selection = resolve_selection(&loaded, node)?;

    let rendered = match (view, format) {
        (ExportView::Summary, ExportFormat::Json) => to_json(&summary(&loaded))?,
        (ExportView::Summary, ExportFormat::Csv) => summary_csv(&summary(&loaded)),
        (ExportView::Tree, ExportFormat::Json) => to_json(&tree_rows(&loaded, &selection))?,
        (ExportView::Tree, ExportFormat::Csv) => tree_csv(&tree_rows(&loaded, &selection)),
        (ExportView::Actions, ExportFormat::Json) => to_json(&action_rows(&loaded, &selection)?)?,
        (ExportView::Actions, ExportFormat::Csv) => action_csv(&action_rows(&loaded, &selection)?),
        (ExportView::Strategy, ExportFormat::Json) => {
            to_json(&strategy_rows(&loaded, &selection)?)?
        }
        (ExportView::Strategy, ExportFormat::Csv) => {
            strategy_csv(&strategy_rows(&loaded, &selection)?)
        }
        (ExportView::Ev, ExportFormat::Json) => to_json(&value_rows(&loaded, &selection)?)?,
        (ExportView::Ev, ExportFormat::Csv) => value_csv(&value_rows(&loaded, &selection)?),
        (ExportView::Range, ExportFormat::Json) => to_json(&range_rows(&loaded))?,
        (ExportView::Range, ExportFormat::Csv) => range_csv(&range_rows(&loaded)),
    };

    match output {
        Some(path) => {
            std::fs::write(path, rendered)
                .with_context(|| format!("writing {}", path.display()))?;
            println!("wrote {}", path.display());
        }
        None => print!("{rendered}"),
    }
    Ok(())
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)? + "\n")
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
                crate::inspect::resolve_action(&info.actions, step)
                    .map_err(|e| anyhow!("resolving {node:?} at {:?}: {e}", info.history))?
            }
            NodeKind::Chance => (0..children.len())
                .find(|&pos| {
                    let label = crate::inspect::chance_child_label(
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
struct Summary {
    board: String,
    pot: u32,
    effective_stack: u32,
    min_bet: u32,
    iterations: u64,
    ev_oop: f64,
    ev_ip: f64,
    expl_oop: f64,
    expl_ip: f64,
    nash_conv: f64,
    storage: String,
    wall_secs: f64,
    /// `full` or `no-rivers`: which streets carry stored strategies and
    /// values.
    streets_stored: String,
    nodes: usize,
    stored_nodes: usize,
}

fn summary(loaded: &LoadedSol) -> Summary {
    Summary {
        board: loaded
            .board
            .iter()
            .map(cards::Card::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        pot: loaded.config.pot.0,
        effective_stack: loaded.config.effective_stack.0,
        min_bet: loaded.config.min_bet.0,
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

fn summary_csv(summary: &Summary) -> String {
    let mut out = String::from(
        "board,pot,effective_stack,min_bet,iterations,ev_oop,ev_ip,expl_oop,expl_ip,\
         nash_conv,storage,wall_secs,streets_stored,nodes,stored_nodes\n",
    );
    out.push_str(&format!(
        "{},{},{},{},{},{:.6},{:.6},{:.3e},{:.3e},{:.3e},{},{:.3},{},{},{}\n",
        summary.board,
        summary.pot,
        summary.effective_stack,
        summary.min_bet,
        summary.iterations,
        summary.ev_oop,
        summary.ev_ip,
        summary.expl_oop,
        summary.expl_ip,
        summary.nash_conv,
        summary.storage,
        summary.wall_secs,
        summary.streets_stored,
        summary.nodes,
        summary.stored_nodes,
    ));
    out
}

// --- tree -------------------------------------------------------------------

#[derive(Serialize)]
struct TreeRow {
    history: String,
    street: String,
    actor: String,
    pot: u32,
    actions: Vec<String>,
    stored: bool,
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
                pot: (info.contrib[Player::P0] + info.contrib[Player::P1]).0,
                actions: info.actions.clone(),
                stored: loaded.blocks.contains_key(&node.aux),
            }
        })
        .collect()
}

fn tree_csv(rows: &[TreeRow]) -> String {
    let mut out = String::from("history,street,actor,pot,stored,actions\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            row.history,
            row.street,
            row.actor,
            row.pot,
            row.stored,
            row.actions.join("|"),
        ));
    }
    out
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
             --sol-streets no-rivers, which drops river nodes. Re-solve with \
             --sol-streets full to read river nodes from the artifact",
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
    let reach = engine::reach_at(tree, root_slices, id, |node_id, sref, out| {
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
struct ActionRow {
    history: String,
    street: String,
    actor: String,
    action: String,
    frequency: f64,
}

fn action_rows(loaded: &LoadedSol, selection: &Selection) -> Result<Vec<ActionRow>> {
    let mut rows = Vec::new();
    for id in selected_action_nodes(loaded, selection) {
        let node = stored_node(loaded, id)?;
        let reach = reach_at(loaded, id)?;
        let freqs = crate::postflop_setup::action_frequencies(
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

fn action_csv(rows: &[ActionRow]) -> String {
    let mut out = String::from("history,street,actor,action,frequency\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{:.6}\n",
            row.history, row.street, row.actor, row.action, row.frequency
        ));
    }
    out
}

// --- strategy ---------------------------------------------------------------

#[derive(Serialize)]
struct StrategyRow {
    history: String,
    actor: String,
    combo: String,
    weight: f32,
    probabilities: Vec<f32>,
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

fn strategy_csv(rows: &[StrategyRow]) -> String {
    let mut out = String::from("history,actor,combo,weight,probabilities\n");
    for row in rows {
        let probs: Vec<String> = row
            .probabilities
            .iter()
            .map(|p| format!("{p:.6}"))
            .collect();
        out.push_str(&format!(
            "{},{},{},{:.6},{}\n",
            row.history,
            row.actor,
            row.combo,
            row.weight,
            probs.join("|"),
        ));
    }
    out
}

// --- ev ---------------------------------------------------------------------

#[derive(Serialize)]
struct ValueRow {
    history: String,
    seat: String,
    combo: String,
    weight: f32,
    /// Expected utility relative to the original subgame start, including
    /// wagers already made. Chips for chip EV, prize units for ICM.
    ev: f32,
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

fn value_csv(rows: &[ValueRow]) -> String {
    let mut out = String::from("history,seat,combo,weight,ev\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{:.6},{:.6}\n",
            row.history, row.seat, row.combo, row.weight, row.ev
        ));
    }
    out
}

// --- range ------------------------------------------------------------------

#[derive(Serialize)]
struct RangeRow {
    seat: String,
    combo: String,
    weight: f32,
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

fn range_csv(rows: &[RangeRow]) -> String {
    let mut out = String::from("seat,combo,weight\n");
    for row in rows {
        out.push_str(&format!("{},{},{:.6}\n", row.seat, row.combo, row.weight));
    }
    out
}

/// Compares two postflop artifacts node by node.
///
/// Both embed the config that produced them, so the same tree shape means
/// corresponding `sref`s — the only basis on which strategies and values
/// can be lined up. Artifacts of the same shape but a different spot are
/// refused unless the caller says otherwise with `--cross-game`.
pub fn compare(left_path: &Path, right_path: &Path, cross_game: bool) -> Result<()> {
    let left = load_sol(left_path, 0, None)?;
    let right = load_sol(right_path, 0, None)?;

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
    print!("{}", to_json(&report)?);
    Ok(())
}

#[derive(Serialize)]
struct ComparisonReport {
    nodes: usize,
    /// Mean over nodes of the per-hand-averaged L1 distance between the two
    /// action distributions. `0` is identical play, `2` is disjoint.
    mean_strategy_l1: f64,
    max_strategy_l1: f64,
    max_strategy_node: String,
    /// Mean over nodes of that node's largest per-hand EV difference, in
    /// chips.
    mean_max_ev_delta: f64,
    max_ev_delta: f64,
    max_ev_node: String,
    ev_oop: [f64; 2],
    ev_ip: [f64; 2],
    nash_conv: [f64; 2],
}
