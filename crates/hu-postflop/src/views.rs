//! Typed P1 artifact views and node-by-node comparisons.
//! Stored strategy and value blocks are read without solving. A missing river
//! block is an explicit error unless the caller opts into [`RiverResolve`],
//! which re-solves the river subgame through the artifact's provider.

use std::path::Path;

use crate::index::TreeView;
use crate::sol::StreetsStored;
use anyhow::{Context, Result, anyhow, bail};
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
    /// `actions`, `strategy` and `ev` together.
    Decision,
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
    Decision(DecisionRows),
    Range(Vec<RangeRow>),
}

/// The `actions`, `strategy` and `ev` rows of the same selected nodes, read
/// from one load so a river re-solve serves all three.
#[derive(Serialize)]
pub struct DecisionRows {
    pub actions: Vec<ActionRow>,
    pub strategy: Vec<StrategyRow>,
    pub ev: Vec<ValueRow>,
}

use crate::artifact::{LoadedSol, SolProvider, StrategyProvider, load_sol};
use crate::sol::{SolMetadata, SolReader};

/// Opt-in re-solve of the river nodes a `no-rivers` artifact does not store,
/// for the per-node views. Each river subgame below one river deal is solved
/// once per export, from the reach the stored strategies give it.
#[derive(Clone, Copy, Debug)]
pub struct RiverResolve {
    /// Iteration budget of each river subgame.
    pub iterations: u64,
    /// Stop a subgame early once its NashConv drops below this.
    pub target: Option<f64>,
}

/// A selector that cannot name a node accepted by the requested view.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct NodeSelectionError(anyhow::Error);

/// Which nodes a view covers.
enum Selection {
    /// One node, named by history or by slash-separated action labels.
    One(NodeId),
    /// Every action node the artifact stores.
    All,
}

/// Read an artifact and return the selected view as typed data for
/// caller-owned encoding. Node views cover every entry of `nodes` in order,
/// from one load; `summary` and `range` ignore `nodes`. With
/// `rest_of_street`, each selected node is followed by the action nodes after
/// it on its street, as [`preview_tree`] does. `summary` reads only the
/// artifact's metadata, never its stored tree. With `rivers`, `actions`,
/// `strategy`, `ev` and `decision` answer unstored river nodes from a
/// re-solve instead of failing; other views ignore it.
pub fn export(
    path: &Path,
    view: ExportView,
    nodes: &[String],
    rest_of_street: bool,
    rivers: Option<RiverResolve>,
) -> Result<ExportData> {
    if let ExportView::Summary = view {
        let reader =
            SolReader::open(path).with_context(|| format!("reading {}", path.display()))?;
        return Ok(ExportData::Summary(summary(
            reader.metadata(),
            reader.profile(),
        )?));
    }
    let loaded = match rivers {
        Some(rivers) => load_sol(path, rivers.iterations, rivers.target)?,
        None => load_sol(path, 0, None)?,
    };
    let selections = nodes
        .iter()
        .map(|node| {
            resolve_selection(loaded.view(), node).map_err(|error| NodeSelectionError(error).into())
        })
        .collect::<Result<Vec<_>>>()?;
    let mut ids = Vec::new();
    for selection in &selections {
        ids.extend(selected_nodes(&loaded, selection));
    }
    if rest_of_street {
        ids = ids
            .into_iter()
            .flat_map(|id| street_action_nodes(loaded.view(), id))
            .collect();
    }
    let mut blocks = Blocks {
        loaded: &loaded,
        resolver: rivers.map(|_| SolProvider::new(&loaded, Box::new(|_| {}))),
    };
    Ok(match view {
        ExportView::Summary => unreachable!("answered from metadata above"),
        ExportView::Tree => ExportData::Tree(tree_rows(&loaded, &ids)),
        ExportView::Actions => ExportData::Actions(action_rows(&mut blocks, &ids)?),
        ExportView::Strategy => ExportData::Strategy(strategy_rows(&mut blocks, &ids)?),
        ExportView::Ev => ExportData::Ev(value_rows(&mut blocks, &ids)?),
        ExportView::Decision => ExportData::Decision(DecisionRows {
            actions: action_rows(&mut blocks, &ids)?,
            strategy: strategy_rows(&mut blocks, &ids)?,
            ev: value_rows(&mut blocks, &ids)?,
        }),
        ExportView::Range => ExportData::Range(range_rows(&loaded)),
    })
}

/// Where the node views read strategy and value blocks: the stored blocks,
/// and with a resolver also the re-solved blocks of unstored river nodes.
struct Blocks<'a> {
    loaded: &'a LoadedSol,
    resolver: Option<SolProvider<'a>>,
}

impl Blocks<'_> {
    fn strategy(&mut self, id: NodeId) -> Result<Vec<f32>> {
        match &mut self.resolver {
            Some(resolver) => resolver.average_strategy(id),
            None => self
                .loaded
                .stored_strategy(id)?
                .ok_or_else(|| unstored(self.loaded, id)),
        }
    }

    /// Per-hand values, OOP's support then IP's.
    fn values(&mut self, id: NodeId) -> Result<Vec<f32>> {
        match &mut self.resolver {
            Some(resolver) => resolver.values(id),
            None => self
                .loaded
                .stored_values(id)?
                .ok_or_else(|| unstored(self.loaded, id)),
        }
    }
}

fn unstored(loaded: &LoadedSol, id: NodeId) -> anyhow::Error {
    anyhow!(
        "this artifact stores no strategy or values for {:?}: it was written with \
         [output] solution_streets = \"no-rivers\", which drops river nodes. \
         Re-solve them on demand (export --resolve-rivers) or solve with \
         solution_streets = \"full\" to read river nodes from the artifact",
        loaded.view().history(id)
    )
}

/// Resolves `--node`: `all`, `root`, a betting-line history (`xr10c`), or
/// slash-separated action labels (`check/bet 10`).
fn resolve_selection(view: TreeView<'_>, node: &str) -> Result<Selection> {
    if node == "all" {
        return Ok(Selection::All);
    }
    if node == "root" || node.is_empty() {
        return Ok(Selection::One(0));
    }
    if let Some(id) = view.node_by_history_any(node) {
        return Ok(Selection::One(id));
    }
    // Fall back to the label path the REPL's `go` accepts, so a node the
    // user navigated to interactively can be named the same way here.
    let tree = view.tree;
    let mut current: NodeId = 0;
    for step in node.split('/').filter(|s| !s.trim().is_empty()) {
        let step = step.trim();
        let n = *tree.node(current);
        let children: Vec<NodeId> = tree.children(current).collect();
        let position = match n.kind {
            NodeKind::Action => crate::queries::resolve_action(&view.actions(current), step)
                .map_err(|e| anyhow!("resolving {node:?} at {:?}: {e}", view.history(current)))?,
            NodeKind::Chance => (0..children.len())
                .find(|&pos| {
                    let label = view.chance_label(current, pos);
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

/// The selected nodes; `all` retains stored action nodes in tree order.
fn selected_nodes(loaded: &LoadedSol, selection: &Selection) -> Vec<NodeId> {
    match selection {
        Selection::One(id) => vec![*id],
        Selection::All => (0..loaded.game.tree.nodes.len() as NodeId)
            .filter(|&id| loaded.is_stored(id))
            .collect(),
    }
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
/// Artifact metadata and stored-tree coverage.
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
    /// Resolved profile metadata; absent for profile-free solutions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Box<crate::run::ProfileReport>>,
    /// Normalized profile input from the solution's embedded configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_config: Option<Box<toml::Value>>,
}

fn summary(metadata: &SolMetadata, profile: Option<&crate::run::ProfileReport>) -> Result<Summary> {
    crate::prepare::require_artifact_config(&metadata.config_toml)?;
    let config = crate::prepare::lower_config(&metadata.config_toml, Path::new("embedded.toml"))?;
    let meta = &metadata.meta;
    let profile_config = if profile.is_some() {
        metadata
            .config_toml
            .parse::<toml::Value>()?
            .get("profile")
            .cloned()
            .map(Box::new)
    } else {
        None
    };
    Ok(Summary {
        board: config
            .board
            .iter()
            .map(nlh::Card::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        pot: amount(config.pot.0),
        effective_stack: amount(config.effective_stack.0),
        min_bet: amount(config.min_bet.0),
        iterations: meta.iterations,
        ev_oop: meta.ev[0],
        ev_ip: meta.ev[1],
        expl_oop: meta.expl[0],
        expl_ip: meta.expl[1],
        nash_conv: meta.nash_conv,
        storage: meta.storage.clone(),
        wall_secs: meta.wall_secs,
        streets_stored: match metadata.mode {
            StreetsStored::Full => "full".into(),
            StreetsStored::NoRivers => "no-rivers".into(),
        },
        nodes: metadata.tree_nodes as usize,
        stored_nodes: metadata.stored_nodes as usize,
        profile: profile.cloned().map(Box::new),
        profile_config,
    })
}

fn amount(value: u32) -> serde_json::Number {
    crate::prepare::bb(value as u64).parse().expect("BB number")
}

// --- tree -------------------------------------------------------------------

#[derive(Serialize)]
/// One selected node, with its outgoing edges in tree order.
pub struct TreeRow {
    pub history: String,
    pub kind: String,
    pub street: String,
    pub actor: Option<String>,
    pub pot: Option<serde_json::Number>,
    pub actions: Vec<String>,
    pub children: Vec<String>,
    pub child_kinds: Vec<String>,
    pub stored: bool,
}

/// One selected node of a config's unsolved tree, without artifact storage state.
#[derive(Serialize)]
pub struct PreviewTreeRow {
    pub history: String,
    pub kind: String,
    pub street: String,
    pub actor: Option<String>,
    pub pot: Option<serde_json::Number>,
    pub actions: Vec<String>,
    pub children: Vec<String>,
    pub child_kinds: Vec<String>,
}

/// Build the same indexed public tree used by solving, without solver storage.
/// Selectors have the artifact tree view's grammar; `all` selects every action node.
/// With `rest_of_street`, each selected node is followed by the action nodes
/// after it on its street, in tree order, stopping at chance and terminal nodes.
pub fn preview_tree(
    raw: &str,
    path: &Path,
    nodes: &[String],
    rest_of_street: bool,
) -> Result<Vec<PreviewTreeRow>> {
    let prepared = crate::prepare::prepare(raw, path)?;
    let (game, index) = crate::run::build_indexed(&prepared)?;
    let view = TreeView {
        tree: &game.tree,
        index: &index,
    };
    let mut ids = Vec::new();
    if nodes.is_empty() {
        ids.push(0);
    }
    for node in nodes {
        match resolve_selection(view, node).map_err(NodeSelectionError)? {
            Selection::One(id) => ids.push(id),
            Selection::All => ids.extend(
                (0..game.tree.nodes.len() as NodeId)
                    .filter(|&id| game.tree.node(id).kind == NodeKind::Action),
            ),
        }
    }
    if rest_of_street {
        ids = ids
            .into_iter()
            .flat_map(|id| street_action_nodes(view, id))
            .collect();
    }
    Ok(ids.iter().map(|&id| tree_row(view, id)).collect())
}

/// `id`, then the action nodes below it before the next chance or terminal node, in preorder.
fn street_action_nodes(view: TreeView<'_>, id: NodeId) -> Vec<NodeId> {
    let mut out = vec![id];
    if view.tree.node(id).kind != NodeKind::Action {
        return out;
    }
    let mut stack: Vec<NodeId> = view.tree.children(id).collect();
    stack.reverse();
    while let Some(next) = stack.pop() {
        if view.tree.node(next).kind != NodeKind::Action {
            continue;
        }
        out.push(next);
        let start = stack.len();
        stack.extend(view.tree.children(next));
        stack[start..].reverse();
    }
    out
}

fn tree_rows(loaded: &LoadedSol, ids: &[NodeId]) -> Vec<TreeRow> {
    let view = loaded.view();
    ids.iter()
        .map(|&id| {
            let row = tree_row(view, id);
            TreeRow {
                history: row.history,
                kind: row.kind,
                street: row.street,
                actor: row.actor,
                pot: row.pot,
                actions: row.actions,
                children: row.children,
                child_kinds: row.child_kinds,
                stored: loaded.is_stored(id),
            }
        })
        .collect()
}

fn tree_row(view: TreeView<'_>, id: NodeId) -> PreviewTreeRow {
    let node = view.tree.node(id);
    let street = match (node.kind, view.street(id)) {
        (NodeKind::Chance, Street::Flop) => Street::Turn,
        (NodeKind::Chance, Street::Turn) => Street::River,
        (NodeKind::Chance, _) => unreachable!("chance only follows flop or turn"),
        (_, street) => street,
    };
    let actions = match node.kind {
        NodeKind::Action => view.actions(id).into_iter().map(str::to_string).collect(),
        NodeKind::Chance => (0..node.num_children as usize)
            .map(|pos| view.chance_label(id, pos))
            .collect(),
        NodeKind::Terminal => Vec::new(),
    };
    let mut children = Vec::with_capacity(node.num_children as usize);
    let mut child_kinds = Vec::with_capacity(node.num_children as usize);
    for child in view.tree.children(id) {
        children.push(view.history(child));
        child_kinds.push(kind_name(view.tree.node(child).kind).to_string());
    }
    PreviewTreeRow {
        history: view.history(id),
        kind: kind_name(node.kind).to_string(),
        street: street_name(street).to_string(),
        actor: (node.kind == NodeKind::Action).then(|| seat_name(node.player).to_string()),
        pot: (node.kind == NodeKind::Action).then(|| amount(view.pot(id))),
        actions,
        children,
        child_kinds,
    }
}

fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Action => "action",
        NodeKind::Chance => "chance",
        NodeKind::Terminal => "terminal",
    }
}

// --- shared per-node lookup -------------------------------------------------

/// An action node's acting seat, history and strategy shape.
struct ActionNode {
    actor: Player,
    history: String,
    num_actions: usize,
    num_hands: usize,
}

fn action_node(loaded: &LoadedSol, id: NodeId) -> ActionNode {
    let view = loaded.view();
    let node = *view.tree.node(id);
    let sref = view.tree.storage_ref(&node);
    ActionNode {
        actor: node.player,
        history: view.history(id),
        num_actions: sref.num_actions as usize,
        num_hands: sref.num_hands as usize,
    }
}

/// Visits the requested nodes in one forward reach pass. Ancestors are marked
/// once; branches with no requested descendants are never decoded.
fn reached_rows<'a, R: Clone>(
    blocks: &mut Blocks<'a>,
    ids: &[NodeId],
    rows_of: impl Fn(&mut Blocks<'a>, NodeId, &PerPlayer<Vec<f32>>) -> Result<Vec<R>>,
) -> Result<Vec<R>> {
    let loaded = blocks.loaded;
    for &id in ids {
        if loaded.game.tree.node(id).kind != NodeKind::Action {
            bail!(NodeSelectionError(anyhow!(
                "node is not an action node; only action nodes carry strategies"
            )));
        }
    }
    let mut needed = vec![false; loaded.game.tree.nodes.len()];
    let mut requested = vec![false; needed.len()];
    let mut remaining = vec![0usize; needed.len()];
    for &id in ids {
        requested[id as usize] = true;
        remaining[id as usize] += 1;
        let mut current = id;
        while current != NodeId::MAX && !needed[current as usize] {
            needed[current as usize] = true;
            current = loaded.game.index.parents[current as usize];
        }
    }
    let mut parts = std::collections::HashMap::new();
    fn visit<'a, R>(
        blocks: &mut Blocks<'a>,
        id: NodeId,
        reach: &PerPlayer<Vec<f32>>,
        needed: &[bool],
        requested: &[bool],
        rows_of: &impl Fn(&mut Blocks<'a>, NodeId, &PerPlayer<Vec<f32>>) -> Result<Vec<R>>,
        parts: &mut std::collections::HashMap<NodeId, Vec<R>>,
    ) -> Result<()> {
        if requested[id as usize] {
            parts.insert(id, rows_of(blocks, id, reach)?);
        }
        let loaded = blocks.loaded;
        let tree = &loaded.game.tree;
        let node = *tree.node(id);
        if !tree.children(id).any(|child| needed[child as usize]) {
            return Ok(());
        }
        let strategy = if node.kind == NodeKind::Action {
            Some(blocks.strategy(id)?)
        } else {
            None
        };
        for child in tree.children(id).filter(|&child| needed[child as usize]) {
            let position = (child - node.first_child) as usize;
            let mut next = reach.clone();
            match node.kind {
                NodeKind::Action => {
                    let hands = tree.storage_ref(&node).num_hands as usize;
                    let column = &strategy.as_ref().expect("action strategy")
                        [position * hands..(position + 1) * hands];
                    for (r, &probability) in next[node.player].iter_mut().zip(column) {
                        *r *= probability;
                    }
                }
                NodeKind::Chance => {
                    let deal = tree.deal(&node, position);
                    for seat in Player::BOTH {
                        let len = match deal.maps[seat] {
                            hu_engine::ReachMap::Transition(t) => {
                                tree.transitions[t as usize].out_dim as usize
                            }
                            _ => reach[seat].len(),
                        };
                        next[seat].resize(len, 0.0);
                        tree.map_reach_into(deal.maps[seat], &reach[seat], &mut next[seat]);
                    }
                }
                NodeKind::Terminal => unreachable!("terminal has no children"),
            }
            visit(blocks, child, &next, needed, requested, rows_of, parts)?;
        }
        Ok(())
    }
    if needed.first() == Some(&true) {
        visit(
            blocks,
            0,
            &loaded.game.index.root_ranges,
            &needed,
            &requested,
            &rows_of,
            &mut parts,
        )?;
    }
    let mut rows = Vec::new();
    // Repeated selections repeat rows, just like the original per-node export.
    for &id in ids {
        remaining[id as usize] -= 1;
        if remaining[id as usize] == 0 {
            rows.extend(parts.remove(&id).expect("requested node visited"));
        } else {
            rows.extend(parts[&id].iter().cloned());
        }
    }
    Ok(rows)
}

fn combo_label(index: usize) -> String {
    let (hi, lo) = combo_cards(index);
    format!("{hi}{lo}")
}

// --- actions ----------------------------------------------------------------

#[derive(Clone, Serialize)]
/// One reach-weighted action frequency.
pub struct ActionRow {
    pub history: String,
    pub street: String,
    pub actor: String,
    pub action: String,
    pub frequency: f64,
}

fn action_rows(blocks: &mut Blocks<'_>, ids: &[NodeId]) -> Result<Vec<ActionRow>> {
    let loaded = blocks.loaded;
    let view = loaded.view();
    reached_rows(blocks, ids, |blocks, id, reach| {
        let node = action_node(loaded, id);
        let freqs = crate::prepare::action_frequencies(
            &blocks.strategy(id)?,
            &reach[node.actor],
            node.num_actions,
            node.num_hands,
        );
        let street = street_name(view.street(id));
        Ok(view
            .actions(id)
            .into_iter()
            .zip(freqs)
            .map(|(label, frequency)| ActionRow {
                history: node.history.clone(),
                street: street.to_string(),
                actor: seat_name(node.actor).to_string(),
                action: label.to_string(),
                frequency,
            })
            .collect())
    })
}

// --- strategy ---------------------------------------------------------------

#[derive(Clone, Serialize)]
/// One reachable concrete combo and its action probabilities.
pub struct StrategyRow {
    pub history: String,
    pub actor: String,
    pub combo: String,
    pub weight: f32,
    pub probabilities: Vec<f32>,
}

fn strategy_rows(blocks: &mut Blocks<'_>, ids: &[NodeId]) -> Result<Vec<StrategyRow>> {
    let loaded = blocks.loaded;
    let hands = &loaded.game.index.hands;
    reached_rows(blocks, ids, |blocks, id, reach| {
        let node = action_node(loaded, id);
        let strategy = blocks.strategy(id)?;
        let mut rows = Vec::new();
        for hand in 0..node.num_hands {
            let weight = reach[node.actor][hand];
            if weight <= 0.0 {
                continue;
            }
            rows.push(StrategyRow {
                history: node.history.clone(),
                actor: seat_name(node.actor).to_string(),
                combo: combo_label(hands.combos(node.actor)[hand] as usize),
                weight,
                probabilities: (0..node.num_actions)
                    .map(|a| strategy[a * node.num_hands + hand])
                    .collect(),
            });
        }
        Ok(rows)
    })
}

// --- ev ---------------------------------------------------------------------

#[derive(Clone, Serialize)]
/// One reachable concrete combo and its utility.
pub struct ValueRow {
    pub history: String,
    pub seat: String,
    pub combo: String,
    pub weight: f32,
    /// Expected utility relative to the original subgame start, including
    /// wagers already made. Chips for chip EV, prize units for ICM.
    pub ev: f32,
}

fn value_rows(blocks: &mut Blocks<'_>, ids: &[NodeId]) -> Result<Vec<ValueRow>> {
    let loaded = blocks.loaded;
    let hands = &loaded.game.index.hands;
    reached_rows(blocks, ids, |blocks, id, reach| {
        let history = loaded.view().history(id);
        let values = blocks.values(id)?;
        let mut rows = Vec::new();
        for seat in Player::BOTH {
            let base = match seat {
                Player::P0 => 0,
                Player::P1 => hands.len(Player::P0),
            };
            for hand in 0..hands.len(seat) {
                let weight = reach[seat][hand];
                if weight <= 0.0 {
                    continue;
                }
                rows.push(ValueRow {
                    history: history.clone(),
                    seat: seat_name(seat).to_string(),
                    combo: combo_label(hands.combos(seat)[hand] as usize),
                    weight,
                    ev: values[base + hand],
                });
            }
        }
        Ok(rows)
    })
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
    let index = &loaded.game.index;
    let mut rows = Vec::new();
    for seat in Player::BOTH {
        for (hand, &weight) in index.root_ranges[seat].iter().enumerate() {
            if weight <= 0.0 {
                continue;
            }
            rows.push(RangeRow {
                seat: seat_name(seat).to_string(),
                combo: combo_label(index.hands.combos(seat)[hand] as usize),
                weight,
            });
        }
    }
    rows
}

/// Compares two postflop artifacts node by node.
///
/// Both store the tree they were solved on, so the same tree shape means
/// corresponding `sref`s — the only basis on which strategies and values
/// can be lined up. Artifacts of the same shape but a different spot are
/// refused unless the caller says otherwise with `--cross-game`.
pub fn compare(left_path: &Path, right_path: &Path, cross_game: bool) -> Result<ComparisonReport> {
    let left = load_sol(left_path, 0, None)?;
    let right = crate::artifact::load_sol_sharing(right_path, &left)?;
    let left_ids = selected_nodes(&left, &Selection::All);
    let right_ids = selected_nodes(&right, &Selection::All);
    if left.game.tree.nodes.len() != right.game.tree.nodes.len() || left_ids != right_ids {
        bail!(
            "artifacts describe different trees ({} vs {} nodes, {} vs {} stored nodes); \
             only solves of the same tree can be compared node by node",
            left.game.tree.nodes.len(),
            right.game.tree.nodes.len(),
            left_ids.len(),
            right_ids.len(),
        );
    }
    if !cross_game {
        for (name, differs) in [
            ("board", left.config.board != right.config.board),
            ("pot", left.config.pot != right.config.pot),
            (
                "effective stack",
                left.config.effective_stack != right.config.effective_stack,
            ),
        ] {
            if differs {
                bail!("artifacts have different {name}; pass --cross-game to compare them anyway");
            }
        }
        for seat in Player::BOTH {
            let a = left.game.index.hands.combos(seat);
            let b = right.game.index.hands.combos(seat);
            if a != b {
                let hand = a
                    .iter()
                    .zip(b)
                    .position(|(a, b)| a != b)
                    .unwrap_or(a.len().min(b.len()));
                bail!(
                    "artifacts have different {} hand lists at combo {hand}; pass --cross-game to compare them anyway",
                    seat_name(seat)
                );
            }
        }
        for id in 0..left.game.tree.nodes.len() as NodeId {
            let a = left.game.tree.node(id).kind;
            let b = right.game.tree.node(id).kind;
            if (a == NodeKind::Action || b == NodeKind::Action)
                && (a != b
                    || !left
                        .view()
                        .action_labels(id)
                        .eq(right.view().action_labels(id)))
            {
                bail!(
                    "node {id} ({:?}) has different action labels (including sizes); pass --cross-game to compare them anyway",
                    left.view().history(id)
                );
            }
        }
    }

    // Per-node deltas run in parallel; the fold below stays in node order so
    // sums and first-maximum ties match a serial walk.
    use rayon::prelude::*;
    let deltas: Vec<Result<_>> = left_ids
        .par_iter()
        .map_init(
            || (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
            |(a_strategy, b_strategy, a_values, b_values), &id| {
                let a_node = left.game.tree.node(id);
                let b_node = right.game.tree.node(id);
                let a = left.game.tree.storage_ref(a_node);
                let b = right.game.tree.storage_ref(b_node);
                let a_len =
                    left.game.index.hands.len(Player::P0) + left.game.index.hands.len(Player::P1);
                let b_len =
                    right.game.index.hands.len(Player::P0) + right.game.index.hands.len(Player::P1);
                if a.len() != b.len() || a_len != b_len {
                    bail!(
                        "node {:?} has different shapes in the two artifacts",
                        left.view().history(id)
                    );
                }
                for (loaded, sref, strategy, values, len) in [
                    (&left, a, &mut *a_strategy, &mut *a_values, a_len),
                    (&right, b, &mut *b_strategy, &mut *b_values, b_len),
                ] {
                    let aux = loaded.game.tree.node(id).aux;
                    loaded
                        .store
                        .strategy_into(
                            aux,
                            sref.num_actions as usize,
                            sref.num_hands as usize,
                            strategy,
                        )?
                        .ok_or_else(|| anyhow!("artifact has no strategy for sref {aux}"))?;
                    loaded
                        .store
                        .values_into(aux, len, values)?
                        .ok_or_else(|| anyhow!("artifact has no values for sref {aux}"))?;
                }
                // Per-hand L1 over the action distribution, averaged over hands:
                // the usual "how differently does this node play" number, bounded
                // by 2 and independent of how many actions the node offers.
                let mut node_l1 = 0.0f64;
                for hand in 0..a.num_hands as usize {
                    for action in 0..a.num_actions as usize {
                        let index = action * a.num_hands as usize + hand;
                        node_l1 += (a_strategy[index] - b_strategy[index]).abs() as f64;
                    }
                }
                let node_l1 = node_l1 / a.num_hands.max(1) as f64;
                let node_ev = a_values
                    .iter()
                    .zip(b_values.iter())
                    .map(|(x, y)| (x - y).abs() as f64)
                    .fold(0.0, f64::max);
                Ok((id, node_l1, node_ev))
            },
        )
        .collect();

    let view = left.view();
    let nodes = deltas.len();
    let mut strategy_total = 0.0f64;
    let mut strategy_max = 0.0f64;
    let mut worst_strategy = None;
    let mut ev_total = 0.0f64;
    let mut ev_max = 0.0f64;
    let mut worst_ev = None;
    for delta in deltas {
        let (id, node_l1, node_ev) = delta?;
        strategy_total += node_l1;
        if node_l1 > strategy_max {
            strategy_max = node_l1;
            worst_strategy = Some(id);
        }
        ev_total += node_ev;
        if node_ev > ev_max {
            ev_max = node_ev;
            worst_ev = Some(id);
        }
    }

    let report = ComparisonReport {
        nodes,
        mean_strategy_l1: strategy_total / nodes.max(1) as f64,
        max_strategy_l1: strategy_max,
        max_strategy_node: worst_strategy
            .map(|id| view.history(id))
            .unwrap_or_default(),
        mean_max_ev_delta: ev_total / nodes.max(1) as f64,
        max_ev_delta: ev_max,
        max_ev_node: worst_ev.map(|id| view.history(id)).unwrap_or_default(),
        ev_oop: [left.meta.ev[0], right.meta.ev[0]],
        ev_ip: [left.meta.ev[1], right.meta.ev[1]],
        nash_conv: [left.meta.nash_conv, right.meta.nash_conv],
        profile_conditions: (left.profile_toml.is_some() || right.profile_toml.is_some())
            .then(|| [left.profile_toml.clone(), right.profile_toml.clone()]),
        profile_changed: left.nlh.0.profile != right.nlh.0.profile,
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
    /// Whether normalized profile conditions differ.
    #[serde(skip_serializing_if = "is_false")]
    pub profile_changed: bool,
    /// Both normalized profile sections, with None for an unprofiled side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_conditions: Option<[Option<String>; 2]>,
}

fn is_false(value: &bool) -> bool {
    !value
}
