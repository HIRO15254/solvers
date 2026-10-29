//! Resolution of user profile selectors into compact engine constraints.
use crate::index::{GameIndex, TreeView};
use crate::{NodeId, NodeKind, PostflopConfig, PostflopGame};
use hu_engine::{NodeIncentive, NodeLock, Profile};
use nlh::iso::{all_suit_perms, permute_card, permute_combo};
use nlh::{Card, NUM_COMBOS, Range};
use spot::{HandSelector, LockStrategy, ProfileIr};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, thiserror::Error)]
#[error("NLH003: profile: {0}")]
pub struct ProfileResolveError(pub String);
fn error(message: impl Into<String>) -> ProfileResolveError {
    ProfileResolveError(message.into())
}

/// A separate constraint for each matched runout instance.
#[derive(Clone, Debug)]
pub struct FrequencyTarget {
    pub entry: usize,
    pub node: NodeId,
    pub storage_ref: u32,
    pub action: usize,
    pub hands: Vec<u32>,
    pub target: f64,
    pub pot_utility: f64,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProfileSummary {
    pub matched_nodes: usize,
    pub matched_hands: usize,
    pub iso_restricted: bool,
    pub selectors: Vec<SelectorSummary>,
    pub resolved_nodes: Vec<ResolvedNode>,
}
/// One profile selector's resolved action node (or a frozen ancestor).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ResolvedNode {
    pub node: NodeId,
    pub history: String,
    pub kind: ResolvedNodeKind,
    pub entry: Option<usize>,
    pub confidence: Option<f64>,
    /// Whether a lock selects fewer than all board-live hands.
    pub partial: Option<bool>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedNodeKind {
    Lock,
    Incentive,
    Frequency,
    Frozen,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SelectorSummary {
    pub kind: String,
    pub node: String,
    pub matched_nodes: usize,
    pub matched_hands: usize,
}
impl ProfileSummary {
    fn record(&mut self, kind: &str, node: &str, before: (usize, usize)) {
        self.selectors.push(SelectorSummary {
            kind: kind.into(),
            node: node.into(),
            matched_nodes: self.matched_nodes - before.0,
            matched_hands: self.matched_hands - before.1,
        });
    }
}
pub struct ResolvedProfile {
    pub engine: Profile,
    pub targets: Vec<FrequencyTarget>,
    pub summary: ProfileSummary,
    pub frozen_nodes: Vec<NodeId>,
}

fn range(text: &str) -> Result<Range, ProfileResolveError> {
    text.parse::<Range>().map_err(|e| error(e.to_string()))
}
fn combo(text: &str) -> Result<usize, ProfileResolveError> {
    let r = range(text)?;
    let matches: Vec<_> = (0..NUM_COMBOS).filter(|&h| r.weight(h) > 0.0).collect();
    if matches.len() != 1 {
        return Err(error(format!("{text:?} is not one explicit combo")));
    }
    Ok(matches[0])
}

/// Restrict the builder's suit group to permutations preserving every selector.
/// This is applied before both dry-run counting and actual construction.
pub fn restrict_iso(
    config: &mut PostflopConfig,
    ir: &ProfileIr,
) -> Result<(), ProfileResolveError> {
    let mut cards = Vec::<Card>::new();
    let mut ranges = Vec::<Range>::new();
    let mut combo_maps = Vec::new();
    let mut inspect = |node: &str, hands: &HandSelector| -> Result<(), ProfileResolveError> {
        for part in node.split(['[', ']', '/']) {
            let part = part.trim().trim_end_matches('*');
            if let Ok(card) = part.parse::<Card>() {
                cards.push(card);
            }
        }
        match hands {
            HandSelector::All => {}
            HandSelector::Range(text) => ranges.push(range(text)?),
            HandSelector::Category(text) => {
                nlh::category::CategoryExpr::parse(text).map_err(|e| error(e.to_string()))?;
            }
        }
        Ok(())
    };
    for lock in &ir.locks {
        inspect(&lock.node, &lock.hands)?;
        if let LockStrategy::PerCombo(entries) = &lock.strategy {
            let mut map = BTreeMap::new();
            for (text, strategy) in entries {
                map.insert(combo(text)?, strategy.clone());
            }
            combo_maps.push(map);
        }
    }
    for incentive in &ir.incentives {
        inspect(&incentive.node, &incentive.hands)?;
    }
    for frequency in &ir.frequencies {
        inspect(&frequency.node, &frequency.hands)?;
    }
    let perms = all_suit_perms()
        .into_iter()
        .filter(|p| {
            cards.iter().all(|&c| permute_card(p, c) == c)
                && ranges
                    .iter()
                    .all(|r| (0..NUM_COMBOS).all(|h| r.weight(h) == r.weight(permute_combo(p, h))))
                && combo_maps.iter().all(|m| {
                    m.iter()
                        .all(|(&h, s)| m.get(&permute_combo(p, h)) == Some(s))
                })
        })
        .collect();
    config.profile_suit_perms = Some(perms);
    Ok(())
}

fn action(view: TreeView<'_>, node: NodeId, label: &str) -> Result<usize, ProfileResolveError> {
    crate::queries::resolve_action(&view.actions(node), label)
        .map_err(|e| error(format!("at {:?}: {e}", view.history(node))))
}
fn nodes(view: TreeView<'_>, selector: &str) -> Result<Vec<NodeId>, ProfileResolveError> {
    view.profile_nodes(selector)
        .map_err(|e| error(e.to_string()))
}
fn mask(
    view: TreeView<'_>,
    node: NodeId,
    board: &[Card],
    selector: &HandSelector,
) -> Result<Vec<bool>, ProfileResolveError> {
    view.profile_hand_mask(node, board, selector)
        .map_err(|e| error(e.to_string()))
}
#[allow(clippy::too_many_arguments)]
fn add_incentive(
    profile: &mut Profile,
    s: usize,
    actions: usize,
    hands: usize,
    space: u32,
    a: usize,
    mask: &[bool],
    value: f64,
) {
    let incentive = profile.incentives[s].get_or_insert_with(|| NodeIncentive {
        bonus: vec![0.0; actions * hands],
        space,
    });
    for (h, &selected) in mask.iter().enumerate() {
        if selected {
            incentive.bonus[a * hands + h] += value as f32;
        }
    }
}

/// Resolve a profile against the actual indexed tree. Cash utilities and pots
/// are in BB. Tournament callers must reject incentives and frequency targets.
pub fn resolve_profile(
    game: &mut PostflopGame,
    config: &PostflopConfig,
    ir: &ProfileIr,
    base: Option<&dyn Fn(NodeId) -> anyhow::Result<Option<Vec<f32>>>>,
) -> Result<ResolvedProfile, ProfileResolveError> {
    let start = crate::artifact::start_street_from_board_len(config.board.len());
    let index = GameIndex::from_game(&game.game, &game.node_info, start);
    let tree = &game.game.tree;
    let view = TreeView {
        tree,
        index: &index,
    };
    let n = tree.storage_refs.len();
    let mut engine = Profile {
        locks: vec![None; n],
        incentives: vec![None; n],
    };
    let mut targets = Vec::new();
    let mut summary = ProfileSummary {
        iso_restricted: config
            .profile_suit_perms
            .as_ref()
            .is_some_and(|p| p.len() < 24),
        ..Default::default()
    };
    let mut matched = BTreeSet::new();
    for (entry_index, lock) in ir.locks.iter().enumerate() {
        let before = (summary.matched_nodes, summary.matched_hands);
        for id in nodes(view, &lock.node)? {
            let hands_before = summary.matched_hands;
            let node = tree.node(id);
            let s = node.aux as usize;
            let actions = node.num_children as usize;
            let hands = tree.storage_ref(node).num_hands as usize;
            type Selection<'a> = (Vec<bool>, &'a Vec<(String, f64)>);
            let selections: Vec<Selection<'_>> = match &lock.strategy {
                LockStrategy::Uniform(strategy) => {
                    vec![(mask(view, id, &config.board, &lock.hands)?, strategy)]
                }
                LockStrategy::PerCombo(entries) => {
                    let mut result = Vec::new();
                    for (text, strategy) in entries {
                        let global = combo(text)?;
                        let local = index.hands.local(node.player, global).ok_or_else(|| {
                            error(format!(
                                "combo {text:?} is outside actor support at {:?}",
                                view.history(id)
                            ))
                        })?;
                        let board = view.profile_board(id, &config.board);
                        let (a, b) = nlh::combo_cards(global);
                        if board.contains(&a) || board.contains(&b) {
                            continue;
                        }
                        let mut selected = vec![false; hands];
                        selected[local] = true;
                        result.push((selected, strategy));
                    }
                    result
                }
            };
            if selections
                .iter()
                .all(|(selected, _)| !selected.iter().any(|&v| v))
            {
                continue;
            }
            matched.insert(id);
            let entry = engine.locks[s].get_or_insert_with(|| NodeLock {
                fixed: vec![f32::NAN; actions * hands],
                confidence: vec![0.0; hands],
            });
            for (selected, strategy) in selections {
                let mut fixed = vec![f32::NAN; actions];
                for (label, probability) in strategy {
                    let a = action(view, id, label)?;
                    if !fixed[a].is_nan() {
                        return Err(error(format!("duplicate action {label:?}")));
                    }
                    fixed[a] = *probability as f32;
                }
                let sum: f32 = fixed.iter().filter(|v| !v.is_nan()).sum();
                if !lock.confidence.is_finite()
                    || !(0.0..=1.0).contains(&lock.confidence)
                    || lock.confidence == 0.0
                    || fixed
                        .iter()
                        .any(|x| !x.is_nan() && !(0.0..=1.0).contains(x))
                    || sum > 1.000001
                    || (lock.confidence < 1.0
                        && (fixed.iter().any(|x| x.is_nan()) || (sum - 1.0).abs() > 1e-6))
                    || (lock.confidence == 1.0
                        && sum < 1.0 - 1e-6
                        && fixed.iter().all(|x| !x.is_nan()))
                {
                    return Err(error(format!(
                        "invalid lock probabilities or confidence at {:?}, actions {:?}",
                        view.history(id),
                        view.actions(id)
                    )));
                }
                for (h, &selected) in selected.iter().enumerate() {
                    if selected {
                        if entry.confidence[h] != 0.0 {
                            return Err(error(format!(
                                "overlapping locks at {:?}",
                                view.history(id)
                            )));
                        }
                        entry.confidence[h] = lock.confidence as f32;
                        for (a, &value) in fixed.iter().enumerate() {
                            entry.fixed[a * hands + h] = value;
                        }
                        summary.matched_hands += 1;
                    }
                }
            }
            let live = mask(view, id, &config.board, &HandSelector::All)?
                .into_iter()
                .filter(|&selected| selected)
                .count();
            summary.resolved_nodes.push(ResolvedNode {
                node: id,
                history: view.history(id),
                kind: ResolvedNodeKind::Lock,
                entry: Some(entry_index),
                confidence: Some(lock.confidence),
                partial: Some(summary.matched_hands - hands_before < live),
            });
            summary.matched_nodes += 1;
        }
        if summary.matched_hands == before.1 {
            return Err(error(format!(
                "lock {:?} selects no hands at any matched node",
                lock.node
            )));
        }
        summary.record("lock", &lock.node, before);
    }
    for (entry_index, incentive) in ir.incentives.iter().enumerate() {
        let before = (summary.matched_nodes, summary.matched_hands);
        for id in nodes(view, &incentive.node)? {
            let node = tree.node(id);
            let s = node.aux as usize;
            let hands = tree.storage_ref(node).num_hands as usize;
            let selected = mask(view, id, &config.board, &incentive.hands)?;
            if !selected.iter().any(|&v| v) {
                continue;
            }
            matched.insert(id);
            let a = action(view, id, &incentive.action)?;
            let pot = f64::from(view.pot(id)) / 1000.0;
            let value = match incentive.amount {
                spot::IncentiveAmount::PotFraction(v) => v * pot,
                spot::IncentiveAmount::Bb(v) => v,
            };
            let board = view.profile_board(id, &config.board);
            let space = game.game.evaluator.register_profile_board(&board);
            add_incentive(
                &mut engine,
                s,
                node.num_children as usize,
                hands,
                space,
                a,
                &selected,
                value,
            );
            summary.matched_nodes += 1;
            summary.matched_hands += selected.iter().filter(|&&v| v).count();
            summary.resolved_nodes.push(ResolvedNode {
                node: id,
                history: view.history(id),
                kind: ResolvedNodeKind::Incentive,
                entry: Some(entry_index),
                confidence: None,
                partial: None,
            });
        }
        if summary.matched_hands == before.1 {
            return Err(error(format!(
                "incentive {:?} selects no hands at any matched node",
                incentive.node
            )));
        }
        summary.record("incentive", &incentive.node, before);
    }
    for (entry, frequency) in ir.frequencies.iter().enumerate() {
        let before = (summary.matched_nodes, summary.matched_hands);
        for id in nodes(view, &frequency.node)? {
            let node = tree.node(id);
            let s = node.aux as usize;
            let hands = tree.storage_ref(node).num_hands as usize;
            let selected = mask(view, id, &config.board, &frequency.hands)?;
            if !selected.iter().any(|&v| v) {
                continue;
            }
            matched.insert(id);
            let a = action(view, id, &frequency.action)?;
            if engine.locks[s].as_ref().is_some_and(|l| {
                selected
                    .iter()
                    .enumerate()
                    .any(|(h, &v)| v && l.confidence[h] > 0.0)
            }) {
                return Err(error(format!(
                    "frequency target overlaps lock at {:?}",
                    view.history(id)
                )));
            }
            let board = view.profile_board(id, &config.board);
            let space = game.game.evaluator.register_profile_board(&board);
            add_incentive(
                &mut engine,
                s,
                node.num_children as usize,
                hands,
                space,
                a,
                &selected,
                0.0,
            );
            let selected_hands: Vec<_> = selected
                .iter()
                .enumerate()
                .filter_map(|(h, &yes)| yes.then_some(h as u32))
                .collect();
            summary.matched_nodes += 1;
            summary.matched_hands += selected_hands.len();
            summary.resolved_nodes.push(ResolvedNode {
                node: id,
                history: view.history(id),
                kind: ResolvedNodeKind::Frequency,
                entry: Some(entry),
                confidence: None,
                partial: None,
            });
            targets.push(FrequencyTarget {
                entry,
                node: id,
                storage_ref: node.aux,
                action: a,
                hands: selected_hands,
                target: frequency.target,
                pot_utility: f64::from(view.pot(id)) / 1000.0,
            });
        }
        if summary.matched_hands == before.1 {
            return Err(error(format!(
                "frequency {:?} selects no hands at any matched node",
                frequency.node
            )));
        }
        summary.record("frequency", &frequency.node, before);
    }
    let mut frozen = BTreeSet::new();
    if ir.freeze_upstream.is_some() {
        let base =
            base.ok_or_else(|| error("freeze_upstream requires a compatible base solution"))?;
        for &id in &matched {
            let mut parent = index.parents[id as usize];
            while parent != NodeId::MAX {
                if tree.node(parent).kind == NodeKind::Action && !matched.contains(&parent) {
                    frozen.insert(parent);
                }
                parent = index.parents[parent as usize];
            }
        }
        for &id in &frozen {
            let node = tree.node(id);
            let s = node.aux as usize;
            let hands = tree.storage_ref(node).num_hands as usize;
            let fixed = base(id)
                .map_err(|e| error(format!("base solution strategy: {e:#}")))?
                .ok_or_else(|| error(format!("base solution lacks node {:?}", view.history(id))))?;
            if fixed.len() != hands * node.num_children as usize {
                return Err(error("base solution strategy shape mismatch"));
            }
            if targets
                .iter()
                .any(|target| target.storage_ref as usize == s)
            {
                return Err(error(format!(
                    "frequency target overlaps frozen ancestor {:?}",
                    view.history(id)
                )));
            }
            if engine.locks[s].is_some() {
                return Err(error(format!(
                    "explicit lock overlaps frozen ancestor {:?}",
                    view.history(id)
                )));
            }
            engine.locks[s] = Some(NodeLock {
                fixed,
                confidence: vec![1.0; hands],
            });
            summary.resolved_nodes.push(ResolvedNode {
                node: id,
                history: view.history(id),
                kind: ResolvedNodeKind::Frozen,
                entry: None,
                confidence: None,
                partial: None,
            });
        }
    }
    Ok(ResolvedProfile {
        engine,
        targets,
        summary,
        frozen_nodes: frozen.into_iter().collect(),
    })
}
