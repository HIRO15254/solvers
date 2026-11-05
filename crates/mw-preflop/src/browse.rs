//! Saved P2 preflop browsing, independent of terminal rendering.
use crate::ExternalSamplingGame;
use crate::trunk::l0::{self, ClassProfileDocument, FlopLeaves, Profile, Tree};
use anyhow::{Context, Result, anyhow, ensure};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

/// One public decision or leaf. Missing class rows remain unknown.
pub struct Node {
    pub actor: Option<usize>,
    pub actions: Vec<String>,
    pub children: Vec<Option<usize>>,
    pub parent: Option<(usize, usize)>,
    pub probabilities: Vec<Option<Vec<f64>>>,
    pub pot_bb: Option<f64>,
    pub stacks_bb: Option<Vec<f64>>,
}
impl Node {
    fn leaf() -> Self {
        Self {
            actor: None,
            actions: Vec::new(),
            children: Vec::new(),
            parent: None,
            probabilities: vec![None; 169],
            pot_bb: None,
            stacks_bb: None,
        }
    }
}
/// In-memory index; loading and domain queries never depend on CLI state.
pub struct Solution {
    pub kind: &'static str,
    pub nodes: Vec<Node>,
    pub root: usize,
    pub positions: Vec<String>,
    pub prior_combos: Vec<Vec<f64>>,
    pub ev: Vec<crate::mwsol::MultiwaySeatResult>,
    pub ev_unit: Option<&'static str>,
    pub sweeps: Option<u64>,
    pub stop_status: Option<String>,
    histories: BTreeMap<String, usize>,
}
/// Number of physical combinations in a matrix class.
pub fn class_combos(class: usize) -> f64 {
    match (class / 13).cmp(&(class % 13)) {
        std::cmp::Ordering::Equal => 6.0,
        std::cmp::Ordering::Less => 4.0,
        std::cmp::Ordering::Greater => 12.0,
    }
}
/// A..2 row-major class label, suited above the diagonal.
pub fn class_label(class: usize) -> String {
    crate::views::hand_label(class)
}

impl Solution {
    /// Detect by magic/content, including files without a conventional extension.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_config(path, None)
    }
    /// Optional trunk configuration supplies context and must match every decision.
    pub fn load_with_config(path: &Path, config: Option<&Path>) -> Result<Self> {
        let mut magic = [0; 8];
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let length = file.read(&mut magic)?;
        if length == 8 && &magic == b"SLVRMWSL" {
            ensure!(
                config.is_none(),
                "--config is only supported for p2-class-profile JSON, not .mwsol"
            );
            return Self::mwsol(path);
        }
        let document: ClassProfileDocument = serde_json::from_reader(BufReader::with_capacity(
            1024 * 1024,
            File::open(path)?,
        ))
        .context(
            "browse expects a P2 .mwsol or p2-class-profile version 1 JSON; P1 .sol is unsupported",
        )?;
        if let Some(config) = config {
            Self::configured_profile(document, config)
        } else {
            Self::profile(document)
        }
    }
    fn profile(document: ClassProfileDocument) -> Result<Self> {
        ensure!(
            document.format == "p2-class-profile" && document.version == 1,
            "expected p2-class-profile version 1"
        );
        let mut paths = BTreeMap::new();
        let mut nodes = Vec::new();
        let mut seats = 0;
        for entry in &document.nodes {
            ensure!(entry.actor < 9, "profile actor outside P2 seats 0..9");
            ensure!(!entry.actions.is_empty(), "profile decision has no actions");
            ensure!(
                entry
                    .actions
                    .iter()
                    .all(|a| !a.is_empty() && !a.contains('/')),
                "invalid action label"
            );
            let unique: std::collections::BTreeSet<_> = entry.actions.iter().collect();
            ensure!(
                unique.len() == entry.actions.len(),
                "duplicate action label"
            );
            ensure!(
                entry.probabilities.len() == 169,
                "profile needs 169 class rows"
            );
            for row in &entry.probabilities {
                ensure!(
                    row.len() == entry.actions.len()
                        && row.iter().all(|p| p.is_finite() && *p >= 0.0)
                        && (row.iter().sum::<f64>() - 1.0).abs() <= 1e-6,
                    "invalid profile probability row"
                );
            }
            ensure!(
                paths.insert(entry.path.clone(), nodes.len()).is_none(),
                "duplicate profile node"
            );
            seats = seats.max(entry.actor + 1);
            nodes.push(Node {
                actor: Some(entry.actor),
                actions: entry.actions.clone(),
                children: vec![None; entry.actions.len()],
                probabilities: entry.probabilities.iter().cloned().map(Some).collect(),
                ..Node::leaf()
            });
        }
        let root = *paths.get(&Vec::new()).context("profile has no root node")?;
        for (path, &child) in &paths {
            if path.is_empty() {
                continue;
            }
            let parent = *paths
                .get(&path[..path.len() - 1])
                .context("profile path has missing parent")?;
            let action = nodes[parent]
                .actions
                .iter()
                .position(|a| a == path.last().unwrap())
                .context("unknown profile path action")?;
            nodes[parent].children[action] = Some(child);
            nodes[child].parent = Some((parent, action));
        }
        // Exported profiles contain decisions only. Missing child paths are terminal leaves.
        let decisions = nodes.len();
        for parent in 0..decisions {
            for action in 0..nodes[parent].actions.len() {
                if nodes[parent].children[action].is_none() {
                    let child = nodes.len();
                    nodes.push(Node {
                        parent: Some((parent, action)),
                        ..Node::leaf()
                    });
                    nodes[parent].children[action] = Some(child);
                }
            }
        }
        Ok(Self {
            kind: "p2-class-profile v1 (experimental; assumed full starting ranges)",
            nodes,
            root,
            positions: Vec::new(),
            prior_combos: vec![(0..169).map(class_combos).collect(); seats],
            ev: Vec::new(),
            ev_unit: None,
            sweeps: None,
            stop_status: None,
            histories: BTreeMap::new(),
        })
    }
    fn config_prior(prepared: &crate::prepare::Prepared) -> Result<Vec<Vec<f64>>> {
        let mut prior_combos = Vec::new();
        for seat in &prepared.lowered.game.seats {
            let range: nlh::Range = if seat.range.trim().is_empty() {
                nlh::Range::full()
            } else {
                seat.range.parse()?
            };
            let mut classes = vec![0.0; 169];
            for combo in 0..nlh::NUM_COMBOS {
                let (a, b) = nlh::combo_cards(combo);
                let (a, b) = if a.rank() >= b.rank() { (a, b) } else { (b, a) };
                classes[nlh::class_index(a.rank(), b.rank(), a.suit() == b.suit())] +=
                    f64::from(range.weight(combo));
            }
            prior_combos.push(classes);
        }
        Ok(prior_combos)
    }
    fn configured_profile(document: ClassProfileDocument, config: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(config)?;
        let game = l0::game_from_config(&raw, config)?;
        let tree = Tree::build_with(&game, FlopLeaves::L1)?;
        ensure!(
            document.nodes.len() == tree.nodes.iter().filter(|n| n.actor.is_some()).count(),
            "profile decision node count does not match config tree"
        );
        let profile = Profile::from_json(&tree, &document)?;
        // Also retain the standalone loader's strict probability-row validation.
        Self::profile(document)?;
        let prepared = crate::prepare::prepare(&raw, config)?;
        let mut states = vec![game.root_state()];
        let mut nodes = Vec::with_capacity(tree.nodes.len());
        for (i, node) in tree.nodes.iter().enumerate() {
            let state = if let Some((parent, action)) = node.parent {
                let actions = game.node_actions(&states[parent]);
                game.next_state_with(&states[parent], &actions, action)
            } else {
                game.root_state()
            };
            if i == 0 {
                states[0] = state.clone();
            } else {
                states.push(state.clone());
            }
            nodes.push(Node {
                actor: node.actor,
                actions: node.labels.clone(),
                children: node.children.iter().copied().map(Some).collect(),
                parent: node.parent,
                probabilities: (0..169)
                    .map(|c| node.actor.map(|_| profile.row(&tree, i, c).to_vec()))
                    .collect(),
                pot_bb: Some(state.pot_size().0 as f64 / 1000.0),
                stacks_bb: Some(
                    state
                        .seats
                        .iter()
                        .map(|s| s.remaining.0 as f64 / 1000.0)
                        .collect(),
                ),
            });
        }
        Ok(Self {
            kind: "p2-class-profile v1 (experimental; config starting ranges)",
            nodes,
            root: 0,
            positions: prepared
                .document
                .spot
                .table
                .positions
                .iter()
                .cloned()
                .collect(),
            prior_combos: Self::config_prior(&prepared)?,
            ev: Vec::new(),
            ev_unit: None,
            sweeps: None,
            stop_status: None,
            histories: BTreeMap::new(),
        })
    }
    fn mwsol(path: &Path) -> Result<Self> {
        let (metadata, blocks) = crate::views::read_solution(path)?;
        let prepared = crate::prepare::prepare(&metadata.config_toml, Path::new("embedded.toml"))?;
        let positions = prepared
            .document
            .spot
            .table
            .positions
            .iter()
            .cloned()
            .collect();
        let ev_unit = Some(crate::prepare::utility_unit(&prepared));
        let prior_combos = Self::config_prior(&prepared)?;
        let mut ids = BTreeMap::new();
        let mut histories = BTreeMap::new();
        let mut nodes = Vec::new();
        let bb_chips = metadata.bb_chips as f64;
        for state in metadata.public_states.iter().filter(|s| s.street == 0) {
            ids.insert(state.history, nodes.len());
            histories.insert(
                state.history.iter().map(|b| format!("{b:02x}")).collect(),
                nodes.len(),
            );
            nodes.push(Node {
                actor: state.actor.map(usize::from),
                actions: state.legal_actions.iter().map(|a| a.label()).collect(),
                children: vec![None; state.legal_actions.len()],
                pot_bb: Some(state.pot_chips as f64 / bb_chips),
                stacks_bb: Some(
                    state
                        .remaining_stacks_chips
                        .iter()
                        .map(|s| *s as f64 / bb_chips)
                        .collect(),
                ),
                ..Node::leaf()
            });
        }
        let root = *ids.get(&[0; 16]).context("artifact has no preflop root")?;
        for edge in &metadata.histories {
            if let Some(&parent) = ids.get(&edge.parent) {
                let action = edge.action_index as usize;
                ensure!(
                    action < nodes[parent].actions.len(),
                    "inconsistent artifact action"
                );
                let child = match ids.get(&edge.key) {
                    Some(&child) => child,
                    None => {
                        let child = nodes.len();
                        nodes.push(Node::leaf());
                        child
                    }
                };
                nodes[parent].children[action] = Some(child);
                nodes[child].parent = Some((parent, action));
            }
        }
        for block in blocks.iter().filter(|b| b.key.street == 0) {
            if let Some(&node) = ids.get(&block.key.history) {
                let class = block.key.bucket_path[0] as usize;
                ensure!(
                    class < 169 && nodes[node].actor == Some(usize::from(block.key.actor)),
                    "inconsistent artifact strategy"
                );
                let row = nodes[node]
                    .actions
                    .iter()
                    .map(|a| {
                        block
                            .actions
                            .iter()
                            .position(|b| b == a)
                            .map_or(0.0, |i| f64::from(block.probabilities[i]))
                    })
                    .collect();
                nodes[node].probabilities[class] = Some(row);
            }
        }
        Ok(Self {
            kind: "P2 .mwsol (approximate average)",
            nodes,
            root,
            positions,
            prior_combos,
            sweeps: Some(metadata.sweeps),
            stop_status: Some(metadata.stop_status),
            ev: metadata.seats,
            ev_unit,
            histories,
        })
    }
    pub fn action(&self, node: usize, selector: &str) -> Result<usize> {
        let actions = &self.nodes[node].actions;
        selector
            .parse::<usize>()
            .ok()
            .filter(|i| *i < actions.len())
            .or_else(|| actions.iter().position(|a| a == selector))
            .ok_or_else(|| anyhow!("unknown action {selector:?}; use show to list actions"))
    }
    pub fn child(&self, node: usize, action: usize) -> Result<usize> {
        self.nodes[node].children[action].context("child is not saved in this artifact")
    }
    pub fn select(&self, selector: &str) -> Result<usize> {
        if selector.is_empty() || selector == "root" {
            return Ok(self.root);
        }
        if let Some(&node) = self.histories.get(&selector.to_ascii_lowercase()) {
            return Ok(node);
        }
        let mut node = self.root;
        for part in selector.split('/').filter(|s| !s.is_empty()) {
            node = self.child(node, self.action(node, part)?)?;
        }
        Ok(node)
    }
    pub fn path(&self, mut node: usize) -> Vec<(usize, usize)> {
        let mut path = Vec::new();
        while let Some((parent, action)) = self.nodes[node].parent {
            path.push((parent, action));
            node = parent;
        }
        path.reverse();
        path
    }
    pub fn selector(&self, node: usize) -> String {
        let path = self
            .path(node)
            .iter()
            .map(|(_, a)| a.to_string())
            .collect::<Vec<_>>()
            .join("/");
        if path.is_empty() { "root".into() } else { path }
    }
    /// Seat's prior combo mass times only its own action probabilities; unknown stays unknown.
    pub fn range(&self, node: usize, seat: usize) -> Result<Vec<Option<f64>>> {
        let prior = self
            .prior_combos
            .get(seat)
            .context("seat outside saved table")?;
        let mut range: Vec<_> = prior.iter().copied().map(Some).collect();
        for (parent, action) in self.path(node) {
            if self.nodes[parent].actor != Some(seat) {
                continue;
            }
            for (class, weight) in range.iter_mut().enumerate() {
                *weight = match (*weight, &self.nodes[parent].probabilities[class]) {
                    (Some(0.0), _) => Some(0.0),
                    (Some(w), Some(row)) => Some(w * row[action]),
                    _ => None,
                };
            }
        }
        Ok(range)
    }
    /// Frequencies conditional on known actor strategy and reach; coverage is explicit.
    pub fn frequencies(&self, node: usize) -> Result<(Vec<f64>, f64)> {
        let n = &self.nodes[node];
        let Some(actor) = n.actor else {
            return Ok((Vec::new(), 0.0));
        };
        let range = self.range(node, actor)?;
        let mut frequencies = vec![0.0; n.actions.len()];
        let mut covered = 0.0;
        for (weight, row) in range.iter().zip(&n.probabilities) {
            if let (Some(w), Some(row)) = (weight, row) {
                covered += w;
                for (f, p) in frequencies.iter_mut().zip(row) {
                    *f += w * p;
                }
            }
        }
        if covered > 0.0 {
            for f in &mut frequencies {
                *f /= covered;
            }
        }
        Ok((frequencies, covered))
    }
}
