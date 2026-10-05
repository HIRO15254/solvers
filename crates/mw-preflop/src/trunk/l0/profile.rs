use anyhow::{Context, Result, ensure};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::Tree;
use crate::{HistoryKey, mwsol::MultiwayStrategyBlock};

/// Class/action rows in legal tree order. An absent row uses the legacy
/// unvisited-infoset uniform strategy and is separately marked as defaulted.
#[derive(Clone)]
pub struct Profile {
    rows: Vec<Vec<f64>>,
    defaulted: Vec<[bool; 169]>,
    fingerprint: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassProfileDocument {
    pub format: String,
    pub version: u32,
    pub nodes: Vec<ProfileNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileNode {
    pub path: Vec<String>,
    pub actor: usize,
    pub actions: Vec<String>,
    pub probabilities: Vec<Vec<f64>>,
}

impl Profile {
    /// Explicit uniform profile; no information sets are considered missing.
    pub fn uniform(tree: &Tree) -> Self {
        Self::initial(tree, false)
    }

    /// Start a paged artifact import. Call `apply_blocks` for each page.
    pub fn missing(tree: &Tree) -> Self {
        Self::initial(tree, true)
    }

    fn initial(tree: &Tree, missing: bool) -> Self {
        Self {
            rows: tree
                .nodes
                .iter()
                .map(|n| {
                    if n.actor.is_some() {
                        vec![1.0 / n.labels.len() as f64; 169 * n.labels.len()]
                    } else {
                        Vec::new()
                    }
                })
                .collect(),
            defaulted: tree
                .nodes
                .iter()
                .map(|n| [missing && n.actor.is_some(); 169])
                .collect(),
            fingerprint: tree.game_fingerprint,
        }
    }

    pub fn row(&self, tree: &Tree, node: usize, class: usize) -> &[f64] {
        let a = tree.nodes[node].labels.len();
        &self.rows[node][class * a..(class + 1) * a]
    }

    pub fn is_defaulted(&self, node: usize, class: usize) -> bool {
        self.defaulted[node][class]
    }

    pub(crate) fn validate(&self, tree: &Tree) -> Result<()> {
        ensure!(
            self.fingerprint == tree.game_fingerprint && self.rows.len() == tree.nodes.len(),
            "profile/tree mismatch"
        );
        Ok(())
    }

    /// Label-mapped import, with omitted legal actions set to zero. Duplicate
    /// (node,class) entries are refused rather than depending on page order.
    pub fn apply_blocks(&mut self, tree: &Tree, blocks: &[MultiwayStrategyBlock]) -> Result<()> {
        self.validate(tree)?;
        let histories: FxHashMap<_, _> = tree
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.history, i))
            .collect();
        for block in blocks.iter().filter(|b| b.key.street == 0) {
            let node = *histories
                .get(&HistoryKey(block.key.history))
                .context("unknown strategy history")?;
            ensure!(
                tree.nodes[node].actor == Some(usize::from(block.key.actor)),
                "strategy actor mismatch"
            );
            let c = block.key.bucket_path[0] as usize;
            ensure!(c < 169, "preflop class outside 0..169");
            ensure!(self.defaulted[node][c], "duplicate strategy (node,class)");
            self.set_row(
                tree,
                node,
                c,
                &block.actions,
                &block
                    .probabilities
                    .iter()
                    .map(|&p| f64::from(p))
                    .collect::<Vec<_>>(),
                false,
            )?;
        }
        Ok(())
    }

    pub fn from_json(tree: &Tree, document: &ClassProfileDocument) -> Result<Self> {
        ensure!(
            document.format == "p2-class-profile" && document.version == 1,
            "expected p2-class-profile version 1"
        );
        let mut profile = Self::missing(tree);
        for entry in &document.nodes {
            let mut node = 0;
            for label in &entry.path {
                let a = tree.nodes[node]
                    .labels
                    .iter()
                    .position(|l| l == label)
                    .with_context(|| format!("unknown path label {label:?}"))?;
                node = tree.nodes[node].children[a];
            }
            ensure!(
                tree.nodes[node].actor == Some(entry.actor),
                "profile actor mismatch or terminal path"
            );
            ensure!(
                entry.probabilities.len() == 169,
                "profile needs 169 class rows"
            );
            ensure!(
                profile.defaulted[node].iter().all(|&d| d),
                "duplicate profile node"
            );
            for (c, row) in entry.probabilities.iter().enumerate() {
                profile.set_row(tree, node, c, &entry.actions, row, true)?;
            }
        }
        Ok(profile)
    }

    fn set_row(
        &mut self,
        tree: &Tree,
        node: usize,
        class: usize,
        labels: &[String],
        values: &[f64],
        exact: bool,
    ) -> Result<()> {
        let legal = &tree.nodes[node].labels;
        ensure!(
            labels.len() == values.len(),
            "action/probability length mismatch"
        );
        ensure!(
            !exact || labels.len() == legal.len(),
            "profile label-set mismatch"
        );
        let mut row = vec![0.0; legal.len()];
        let mut seen = vec![false; legal.len()];
        for (label, &v) in labels.iter().zip(values) {
            let a = legal
                .iter()
                .position(|l| l == label)
                .with_context(|| format!("unknown action label {label:?}"))?;
            ensure!(!seen[a], "duplicate action label {label:?}");
            ensure!(
                v.is_finite() && v >= 0.0,
                "negative or non-finite probability"
            );
            seen[a] = true;
            row[a] = v;
        }
        let sum: f64 = row.iter().sum();
        ensure!(
            sum.is_finite() && sum > 0.0,
            "all-zero or non-finite probability row"
        );
        let a = legal.len();
        for (slot, value) in self.rows[node][class * a..(class + 1) * a]
            .iter_mut()
            .zip(row)
        {
            *slot = value / sum;
        }
        self.defaulted[node][class] = false;
        Ok(())
    }

    pub fn export(&self, tree: &Tree) -> ClassProfileDocument {
        ClassProfileDocument {
            format: "p2-class-profile".into(),
            version: 1,
            nodes: tree
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    n.actor.map(|actor| ProfileNode {
                        path: tree.path(i),
                        actor,
                        actions: n.labels.clone(),
                        probabilities: (0..169).map(|c| self.row(tree, i, c).to_vec()).collect(),
                    })
                })
                .collect(),
        }
    }
}
