//! Typed P2 artifact queries, comparisons and profile evaluation.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::solver::{
    HistoryEntry, HistoryKey, InfoKey, PolicyColumn, PolicyEntry, ProfileVariant,
    SOLVER_STATE_VERSION, SolverState,
};
use crate::{ExternalSamplingGame, HoldemGame, MultiwayAbstractionBackend};
use anyhow::{Context, Result, anyhow, bail};
use rand::{SeedableRng, rngs::StdRng};
use serde::Serialize;

use crate::session;

/// One configured starting range for export.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
pub struct RangeRow {
    pub seat: usize,
    pub range: String,
}

/// Public-tree summary for an inspected artifact.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectSummary {
    pub format_version: u16,
    pub schema_version: u16,
    pub sweeps: u64,
    pub profile_type: String,
    pub visited_infosets: usize,
    pub public_nodes: usize,
    pub public_history_edges: usize,
    pub typed_action_entries: usize,
    pub seats: Vec<crate::mwsol::MultiwaySeatResult>,
}

/// One inspected public node, with actions and child history keys.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub history: String,
    pub path: Option<Vec<crate::mwsol::MultiwayHistoryAction>>,
    pub street: u8,
    pub actor: Option<u8>,
    pub pot_milli_bb: u64,
    pub pot_bb: f64,
    pub remaining_stacks_milli_bb: Vec<u64>,
    pub legal_actions: Vec<crate::mwsol::MultiwayPublicAction>,
    pub children: Vec<NodeChild>,
}

/// One child action of an inspected node.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeChild {
    pub history: String,
    pub action_index: u32,
    pub action: String,
}

/// One preflop class strategy; absent strategy means an unvisited class.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
pub struct StrategyCell {
    pub hand: String,
    pub bucket: usize,
    pub status: String,
    pub weight: f64,
    pub strategy: Option<BTreeMap<String, f32>>,
}

/// One class reach weight; unknown reach remains distinct from zero.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeCell {
    pub hand: String,
    pub bucket: usize,
    pub status: String,
    pub reach_weight: Option<f64>,
    pub normalized_weight: Option<f64>,
}

/// Conditional range grid for one seat.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
pub struct SeatRange {
    pub seat: usize,
    pub grid: Vec<Vec<RangeCell>>,
}

/// Typed result of an artifact node query, independent of CLI formatting.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum InspectData {
    Node {
        summary: InspectSummary,
        node: NodeView,
        #[serde(rename = "strategy13x13")]
        strategy: Option<Vec<Vec<StrategyCell>>>,
        #[serde(rename = "ranges13x13")]
        ranges: Vec<SeatRange>,
    },
    Strategy {
        node: NodeView,
        actor: Option<u8>,
        grid: Option<Vec<Vec<StrategyCell>>>,
    },
    Range {
        node: NodeView,
        ranges: Vec<SeatRange>,
    },
    Summary(InspectSummary),
    Ev(InspectEv),
    /// Preserve the existing cache reader's opaque payload and extension fields.
    #[serde(skip_deserializing)]
    CachedEv(serde_json::Value),
}

/// A fresh or cached held-out evaluation of the artifact profile at a node.
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectEv {
    pub scope: String,
    pub history: String,
    pub solution_fingerprint: String,
    pub samples: u64,
    pub seed: u64,
    pub br_traversals: u64,
    pub cache_hit: bool,
    pub evaluation: crate::ProfileEvaluation,
}

fn parse_solution_config(config_toml: &str) -> Result<crate::input::Lowered> {
    crate::prepare::require_artifact_config(config_toml)?;
    Ok(crate::prepare::prepare(config_toml, Path::new("embedded.toml"))?.lowered)
}

fn build_solution_session(
    config_toml: &str,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<session::MultiwaySession> {
    crate::prepare::require_artifact_config(config_toml)?;
    crate::prepare::build_session(config_toml, None, cache_root, on_ready)
}
fn key_hex(key: [u8; 16]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_solution(
    path: &Path,
) -> Result<(
    crate::mwsol::MultiwaySolutionMetadata,
    Vec<crate::mwsol::MultiwayStrategyBlock>,
)> {
    let mut reader = crate::mwsol::MwSolReader::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let metadata = reader.metadata().clone();
    crate::prepare::require_artifact_config(&metadata.config_toml)?;
    let mut strategies = Vec::with_capacity(reader.strategy_count());
    let mut cursor = 0;
    while cursor < reader.strategy_count() {
        let page = reader.read_strategy_page(cursor, crate::mwsol::MWSOL_MAX_PAGE_LIMIT)?;
        cursor += page.strategies.len();
        strategies.extend(page.strategies);
    }
    Ok((metadata, strategies))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Available P2 node queries.
pub enum InspectView {
    Node,
    Summary,
    Strategy,
    Range,
    Ev,
}

/// Node selector and evaluation settings for one artifact query.
pub struct InspectRequest<'a> {
    pub history: &'a str,
    pub view: InspectView,
    pub actor: Option<u8>,
    pub samples: u64,
    pub seed: u64,
    pub br_traversals: u64,
}

fn parse_history(
    metadata: &crate::mwsol::MultiwaySolutionMetadata,
    requested: &str,
) -> Result<[u8; 16]> {
    if requested.is_empty() || requested == "root" {
        return Ok([0; 16]);
    }
    if requested.len() == 32 && requested.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let mut key = [0; 16];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&requested[index * 2..index * 2 + 2], 16)
                .context("invalid history hex")?;
        }
        return Ok(key);
    }

    let mut current = [0; 16];
    for segment in requested.split('/').filter(|segment| !segment.is_empty()) {
        let candidates: Vec<_> = metadata
            .histories
            .iter()
            .filter(|edge| edge.parent == current)
            .collect();
        let edge = if let Ok(index) = segment.parse::<u32>() {
            candidates
                .into_iter()
                .find(|edge| edge.action_index == index)
        } else {
            candidates.into_iter().find(|edge| edge.action == segment)
        }
        .ok_or_else(|| {
            anyhow!(
                "history segment {segment:?} is not a child of {}",
                key_hex(current)
            )
        })?;
        current = edge.key;
    }
    Ok(current)
}

fn parse_runtime_range(raw: &str) -> Result<nlh::Range> {
    if raw.trim().is_empty() {
        Ok(nlh::Range::full())
    } else {
        Ok(raw.parse()?)
    }
}

fn hand_label(index: usize) -> String {
    const RANKS: [char; 13] = [
        'A', 'K', 'Q', 'J', 'T', '9', '8', '7', '6', '5', '4', '3', '2',
    ];
    let row = index / 13;
    let column = index % 13;
    if row == column {
        format!("{}{}", RANKS[row], RANKS[column])
    } else if row < column {
        format!("{}{}s", RANKS[row], RANKS[column])
    } else {
        format!("{}{}o", RANKS[column], RANKS[row])
    }
}

fn matrix(mut cells: Vec<serde_json::Value>) -> Vec<Vec<serde_json::Value>> {
    (0..13).map(|_| cells.drain(..13).collect()).collect()
}

fn strategy_matrix(
    strategies: &[crate::mwsol::MultiwayStrategyBlock],
    weights: &[crate::mwsol::MultiwayStrategyWeight],
    history: [u8; 16],
    actor: u8,
) -> Vec<Vec<serde_json::Value>> {
    let weight_by_key: BTreeMap<_, _> = weights
        .iter()
        .map(|entry| (entry.key, entry.weight))
        .collect();
    let cells = (0..nlh::NUM_CLASSES)
        .map(|bucket| {
            let block = strategies.iter().find(|block| {
                block.key.history == history
                    && block.key.actor == actor
                    && block.key.street == 0
                    && block.key.bucket_path[0] == bucket as u32
            });
            match block {
                Some(block) => serde_json::json!({
                    "hand": hand_label(bucket),
                    "bucket": bucket,
                    "status": "visited",
                    "weight": weight_by_key.get(&block.key).copied().unwrap_or(0.0),
                    "strategy": block.actions.iter().cloned()
                        .zip(block.probabilities.iter().copied())
                        .collect::<BTreeMap<_, _>>(),
                }),
                None => serde_json::json!({
                    "hand": hand_label(bucket),
                    "bucket": bucket,
                    "status": "unvisited",
                    "weight": 0.0,
                    "strategy": null,
                }),
            }
        })
        .collect();
    matrix(cells)
}

fn range_matrix(
    metadata: &crate::mwsol::MultiwaySolutionMetadata,
    strategies: &[crate::mwsol::MultiwayStrategyBlock],
    target: [u8; 16],
) -> Result<Vec<serde_json::Value>> {
    let config = parse_solution_config(&metadata.config_toml)?;
    let game = config.game;
    let mut ranges = game
        .seats
        .iter()
        .map(|seat| {
            let parsed = parse_runtime_range(&seat.range)?;
            let mut classes = vec![Some(0.0_f64); nlh::NUM_CLASSES];
            for combo in 0..nlh::NUM_COMBOS {
                let (first, second) = nlh::combo_cards(combo);
                let (hi, lo) = if first.rank() >= second.rank() {
                    (first, second)
                } else {
                    (second, first)
                };
                let class = nlh::class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit());
                *classes[class].as_mut().expect("initialized") += f64::from(parsed.weight(combo));
            }
            Ok::<_, anyhow::Error>(classes)
        })
        .collect::<Result<Vec<_>>>()?;

    let path = metadata
        .resolve_history(target)
        .ok_or_else(|| anyhow!("history {} is absent from the public tree", key_hex(target)))?;
    let mut current = [0; 16];
    for step in path {
        for (bucket, reach) in ranges[step.actor as usize].iter_mut().enumerate() {
            let probability = strategies
                .iter()
                .find(|block| {
                    block.key.history == current
                        && block.key.actor == step.actor
                        && block.key.street == 0
                        && block.key.bucket_path[0] == bucket as u32
                })
                .and_then(|block| block.probabilities.get(step.action_index as usize))
                .copied();
            *reach = match (*reach, probability) {
                (Some(reach), Some(probability)) => Some(reach * f64::from(probability)),
                _ => None,
            };
        }
        current = metadata
            .histories
            .iter()
            .find(|edge| {
                edge.parent == current
                    && edge.actor == step.actor
                    && edge.action_index == step.action_index
            })
            .map(|edge| edge.key)
            .ok_or_else(|| anyhow!("public history is internally inconsistent"))?;
    }

    Ok(ranges
        .into_iter()
        .enumerate()
        .map(|(seat, values)| {
            let total: f64 = values.iter().flatten().sum();
            let cells = values
                .into_iter()
                .enumerate()
                .map(|(bucket, reach)| serde_json::json!({
                    "hand": hand_label(bucket),
                    "bucket": bucket,
                    "status": if reach.is_some() { "known" } else { "unvisited" },
                    "reachWeight": reach,
                    "normalizedWeight": reach.filter(|_| total > 0.0).map(|value| value / total),
                }))
                .collect();
            serde_json::json!({ "seat": seat, "grid": matrix(cells) })
        })
        .collect())
}

/// Read a P2 artifact and return the requested typed node view or held-out EV estimate.
pub fn inspect(
    path: &Path,
    request: InspectRequest<'_>,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<InspectData> {
    let InspectRequest {
        history: requested_history,
        view,
        actor: actor_override,
        samples,
        seed,
        br_traversals,
    } = request;
    if matches!(view, InspectView::Ev) {
        return inspect_ev(
            path,
            requested_history,
            samples,
            seed,
            br_traversals,
            cache_root,
            on_ready,
        );
    }
    let (metadata, strategies) = read_solution(path)?;
    let history = parse_history(&metadata, requested_history)?;
    let state = metadata
        .public_states
        .binary_search_by_key(&history, |state| state.history)
        .ok()
        .map(|index| &metadata.public_states[index])
        .ok_or_else(|| {
            anyhow!(
                "history {} is absent from the public tree",
                key_hex(history)
            )
        })?;
    let actor = actor_override.or(state.actor);
    let summary = serde_json::json!({
        "formatVersion": crate::mwsol::MWSOL_FORMAT_VERSION,
        "schemaVersion": metadata.schema_version,
        "sweeps": metadata.sweeps,
        "profileType": "approximate-average",
        "visitedInfosets": strategies.len(),
        "publicNodes": metadata.public_states.len(),
        "publicHistoryEdges": metadata.histories.len(),
        "typedActionEntries": metadata.public_states.iter().map(|state| state.legal_actions.len()).sum::<usize>(),
        "seats": metadata.seats,
    });
    let node = serde_json::json!({
        "history": key_hex(history),
        "path": metadata.resolve_history(history),
        "street": state.street,
        "actor": state.actor,
        "potMilliBb": state.pot_millibb,
        "potBb": state.pot_millibb as f64 / 1000.0,
        "remainingStacksMilliBb": state.remaining_stacks_millibb,
        "legalActions": state.legal_actions,
        "children": metadata.histories.iter()
            .filter(|edge| edge.parent == history)
            .map(|edge| serde_json::json!({
                "history": key_hex(edge.key),
                "actionIndex": edge.action_index,
                "action": edge.action,
            }))
            .collect::<Vec<_>>(),
    });
    let rendered = match view {
        InspectView::Summary => summary,
        InspectView::Node => serde_json::json!({
            "summary": summary,
            "node": node,
            "strategy13x13": actor.map(|actor| strategy_matrix(
                &strategies,
                &metadata.strategy_weights,
                history,
                actor,
            )),
            "ranges13x13": range_matrix(&metadata, &strategies, history)?,
        }),
        InspectView::Strategy => serde_json::json!({
            "node": node,
            "actor": actor,
            "grid": actor.map(|actor| strategy_matrix(
                &strategies,
                &metadata.strategy_weights,
                history,
                actor,
            )),
        }),
        InspectView::Range => serde_json::json!({
            "node": node,
            "ranges": range_matrix(&metadata, &strategies, history)?,
        }),
        InspectView::Ev => unreachable!(),
    };
    Ok(serde_json::from_value(rendered)?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Available P2 artifact exports.
pub enum ExportView {
    Strategy,
    Actions,
    Range,
    Ev,
    Tree,
    Summary,
}

/// Raw typed artifact data for export; callers choose their own output encoding.
pub struct ExportData {
    pub metadata: crate::mwsol::MultiwaySolutionMetadata,
    pub strategies: Vec<crate::mwsol::MultiwayStrategyBlock>,
    pub ranges: Vec<RangeRow>,
}

/// Read one export view, validating embedded input before returning typed data.
pub fn export(path: &Path, view: ExportView) -> Result<ExportData> {
    let (metadata, strategies) = read_solution(path)?;
    let ranges = if view == ExportView::Range {
        parse_solution_config(&metadata.config_toml)?
            .game
            .seats
            .into_iter()
            .enumerate()
            .map(|(seat, value)| RangeRow {
                seat,
                range: value.range,
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(ExportData {
        metadata,
        strategies,
        ranges,
    })
}

fn comparison_mapping(config_toml: &str) -> Result<(usize, &'static str)> {
    let config = parse_solution_config(config_toml)?;
    let game = &config.game;
    let unit = match &config.utility {
        crate::UtilityConfig::ChipEv => "bb",
        crate::UtilityConfig::TournamentIcm { .. } => "prize",
    };
    Ok((game.seats.len(), unit))
}

fn comparison_recall(config_toml: &str) -> Result<crate::RecallMode> {
    let config = parse_solution_config(config_toml)?;
    let game = config.game;
    Ok(game.abstraction.recall)
}

const CROSS_ABSTRACTION_SAMPLES: u64 = 1_024;
const CROSS_ABSTRACTION_SEED: u64 = 0x6d77_636f_6d70_6172;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// Metric differences for one seat in an artifact comparison.
pub struct SeatComparison {
    pub seat: u8,
    pub profile_ev_mean_delta: Option<f64>,
    pub profile_ev_stderr_delta: Option<f64>,
    pub average_positive_regret_delta: f64,
    pub strategy_drift_l1_delta: f64,
    pub deviation_gain_lower_bound_mean_delta: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// Strategy and seat-metric differences between two artifacts.
pub struct Comparison {
    pub basis: &'static str,
    pub shared_infosets: usize,
    pub only_left: usize,
    pub only_right: usize,
    pub real_card_samples: Option<u64>,
    pub mean_strategy_l1: f64,
    pub max_strategy_l1: f64,
    pub sweep_delta: i128,
    pub left_stop_status: String,
    pub right_stop_status: String,
    pub seats: Vec<SeatComparison>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComparisonBasis {
    BucketInfoset,
    SharedRealCardSample,
}

impl ComparisonBasis {
    fn for_artifacts(
        left_fingerprint: [u8; 32],
        right_fingerprint: [u8; 32],
        left_recall: crate::RecallMode,
        right_recall: crate::RecallMode,
    ) -> Self {
        // Recall is checked independently for compatibility with artifacts
        // written before recall entered the abstraction fingerprint. Two
        // legacy current-street/full solutions can carry the same backend
        // fingerprint even though their infoset keys are not comparable.
        if left_fingerprint == right_fingerprint && left_recall == right_recall {
            Self::BucketInfoset
        } else {
            Self::SharedRealCardSample
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::BucketInfoset => "bucket-infoset",
            Self::SharedRealCardSample => "shared-real-card-sample",
        }
    }
}

fn strategy_l1(
    key: crate::mwsol::MultiwayStrategyKey,
    left: &crate::mwsol::MultiwayStrategyBlock,
    right: &crate::mwsol::MultiwayStrategyBlock,
) -> Result<f64> {
    if left.actions != right.actions || left.probabilities.len() != right.probabilities.len() {
        bail!("action table differs at shared infoset {key:?}");
    }
    Ok(left
        .probabilities
        .iter()
        .zip(&right.probabilities)
        .map(|(a, b)| f64::from((a - b).abs()))
        .sum())
}

fn replay_public_state(
    game: &HoldemGame<MultiwayAbstractionBackend>,
    metadata: &crate::mwsol::MultiwaySolutionMetadata,
    history: [u8; 16],
) -> Result<crate::BettingState> {
    let mut state = game.root_state();
    let path = metadata.resolve_history(history).ok_or_else(|| {
        anyhow!(
            "history {} is absent from the public tree",
            key_hex(history)
        )
    })?;
    for step in path {
        if game.actor(&state) != Some(step.actor as usize) {
            bail!(
                "public history actor differs while rebuilding {}",
                key_hex(history)
            );
        }
        let actions = game.node_actions(&state);
        let action_index = usize::try_from(step.action_index)
            .map_err(|_| anyhow!("public history action index exceeds platform width"))?;
        if action_index >= game.num_actions_of(&actions)
            || game.action_label_of(&actions, action_index) != step.action
        {
            bail!(
                "public history action differs while rebuilding {}",
                key_hex(history)
            );
        }
        state = game.next_state_with(&state, &actions, action_index);
    }
    Ok(state)
}

fn compare_same_abstraction(
    left: &BTreeMap<crate::mwsol::MultiwayStrategyKey, crate::mwsol::MultiwayStrategyBlock>,
    right: &BTreeMap<crate::mwsol::MultiwayStrategyKey, crate::mwsol::MultiwayStrategyBlock>,
) -> Result<(usize, usize, usize, f64, f64)> {
    let mut sum = 0.0;
    let mut maximum = 0.0_f64;
    let mut shared = 0;
    for (key, left_block) in left {
        let Some(right_block) = right.get(key) else {
            continue;
        };
        let l1 = strategy_l1(*key, left_block, right_block)?;
        sum += l1;
        maximum = maximum.max(l1);
        shared += 1;
    }
    Ok((
        shared,
        left.len() - shared,
        right.len() - shared,
        if shared == 0 {
            0.0
        } else {
            sum / shared as f64
        },
        maximum,
    ))
}

fn compare_shared_real_cards(
    left_meta: &crate::mwsol::MultiwaySolutionMetadata,
    right_meta: &crate::mwsol::MultiwaySolutionMetadata,
    left: &BTreeMap<crate::mwsol::MultiwayStrategyKey, crate::mwsol::MultiwayStrategyBlock>,
    right: &BTreeMap<crate::mwsol::MultiwayStrategyKey, crate::mwsol::MultiwayStrategyBlock>,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<(usize, usize, usize, f64, f64)> {
    let left_session = build_solution_session(&left_meta.config_toml, cache_root, on_ready)
        .context("rebuilding the left abstraction")?;
    let right_session = build_solution_session(&right_meta.config_toml, cache_root, on_ready)
        .context("rebuilding the right abstraction")?;
    let (left_game, sampler, _) = left_session.solver.into_components();
    let (right_game, _, _) = right_session.solver.into_components();

    let mut states = Vec::new();
    for left_public in &left_meta.public_states {
        let Some(actor) = left_public.actor else {
            continue;
        };
        let Some(right_public) = right_meta
            .public_states
            .binary_search_by_key(&left_public.history, |state| state.history)
            .ok()
            .map(|index| &right_meta.public_states[index])
        else {
            continue;
        };
        if right_public.actor != Some(actor) || right_public.street != left_public.street {
            continue;
        }
        states.push((
            left_public.history,
            actor,
            replay_public_state(&left_game, left_meta, left_public.history)?,
            replay_public_state(&right_game, right_meta, right_public.history)?,
        ));
    }

    let mut rng = StdRng::seed_from_u64(CROSS_ABSTRACTION_SEED);
    let mut sum = 0.0;
    let mut maximum = 0.0_f64;
    let mut shared = 0;
    let mut only_left = 0;
    let mut only_right = 0;
    for _ in 0..CROSS_ABSTRACTION_SAMPLES {
        let world = sampler.sample(&mut rng)?;
        for (history, actor, left_state, right_state) in &states {
            let left_private = left_game.bucket(left_state, &world, *actor as usize);
            let right_private = right_game.bucket(right_state, &world, *actor as usize);
            let left_key = crate::mwsol::MultiwayStrategyKey {
                history: *history,
                actor: *actor,
                street: left_private.street,
                active_opponents: left_private.active_opponents,
                bucket_path: left_private.bucket_path,
            };
            let right_key = crate::mwsol::MultiwayStrategyKey {
                history: *history,
                actor: *actor,
                street: right_private.street,
                active_opponents: right_private.active_opponents,
                bucket_path: right_private.bucket_path,
            };
            match (left.get(&left_key), right.get(&right_key)) {
                (Some(left_block), Some(right_block)) => {
                    let l1 = strategy_l1(left_key, left_block, right_block)?;
                    sum += l1;
                    maximum = maximum.max(l1);
                    shared += 1;
                }
                (Some(_), None) => only_left += 1,
                (None, Some(_)) => only_right += 1,
                (None, None) => {}
            }
        }
    }
    if shared == 0 {
        bail!("different abstractions had no shared visited real-card observations");
    }
    Ok((shared, only_left, only_right, sum / shared as f64, maximum))
}

/// Compare two artifact profiles using matching infosets or shared real-card samples.
pub fn compare(
    left_path: &Path,
    right_path: &Path,
    cross_game: bool,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<Comparison> {
    let (left_meta, left_blocks) = read_solution(left_path)?;
    let (right_meta, right_blocks) = read_solution(right_path)?;
    let left_mapping = comparison_mapping(&left_meta.config_toml)?;
    let right_mapping = comparison_mapping(&right_meta.config_toml)?;
    let left_recall = comparison_recall(&left_meta.config_toml)?;
    let right_recall = comparison_recall(&right_meta.config_toml)?;
    if !cross_game && left_meta.game_fingerprint != right_meta.game_fingerprint {
        bail!(
            "solutions have different game fingerprints; pass --cross-game to compare explicitly"
        );
    }
    if cross_game && left_mapping != right_mapping {
        bail!("cross-game comparison requires identical seat mapping and utility units");
    }
    let left: BTreeMap<_, _> = left_blocks
        .into_iter()
        .map(|block| (block.key, block))
        .collect();
    let right: BTreeMap<_, _> = right_blocks
        .into_iter()
        .map(|block| (block.key, block))
        .collect();
    let basis = ComparisonBasis::for_artifacts(
        left_meta.abstraction_fingerprint,
        right_meta.abstraction_fingerprint,
        left_recall,
        right_recall,
    );
    let (shared, only_left, only_right, mean, maximum) = match basis {
        ComparisonBasis::BucketInfoset => compare_same_abstraction(&left, &right)?,
        ComparisonBasis::SharedRealCardSample => {
            compare_shared_real_cards(&left_meta, &right_meta, &left, &right, cache_root, on_ready)?
        }
    };
    let seats = left_meta
        .seats
        .iter()
        .map(|left_seat| {
            let right_seat = right_meta
                .seats
                .iter()
                .find(|seat| seat.seat == left_seat.seat)
                .ok_or_else(|| anyhow!("right solution is missing seat {}", left_seat.seat))?;
            Ok(SeatComparison {
                seat: left_seat.seat,
                profile_ev_mean_delta: left_seat
                    .profile_ev
                    .as_ref()
                    .zip(right_seat.profile_ev.as_ref())
                    .map(|(left, right)| right.mean - left.mean),
                profile_ev_stderr_delta: left_seat
                    .profile_ev
                    .as_ref()
                    .zip(right_seat.profile_ev.as_ref())
                    .map(|(left, right)| right.stderr - left.stderr),
                average_positive_regret_delta: right_seat.average_positive_regret
                    - left_seat.average_positive_regret,
                strategy_drift_l1_delta: right_seat.strategy_drift_l1 - left_seat.strategy_drift_l1,
                deviation_gain_lower_bound_mean_delta: left_seat
                    .deviation_gain_lower_bound
                    .as_ref()
                    .zip(right_seat.deviation_gain_lower_bound.as_ref())
                    .map(|(left, right)| right.mean - left.mean),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let result = Comparison {
        basis: basis.label(),
        shared_infosets: shared,
        only_left,
        only_right,
        real_card_samples: (basis == ComparisonBasis::SharedRealCardSample)
            .then_some(CROSS_ABSTRACTION_SAMPLES),
        mean_strategy_l1: mean,
        max_strategy_l1: maximum,
        sweep_delta: i128::from(right_meta.sweeps) - i128::from(left_meta.sweeps),
        left_stop_status: left_meta.stop_status,
        right_stop_status: right_meta.stop_status,
        seats,
    };
    Ok(result)
}

fn evaluate_prepared(
    build: impl FnOnce() -> Result<session::MultiwaySession>,
    metadata: &crate::mwsol::MultiwaySolutionMetadata,
    blocks: Vec<crate::mwsol::MultiwayStrategyBlock>,
    samples: u64,
    seed: u64,
    br_traversals: u64,
    train_deviators: bool,
) -> Result<crate::ProfileEvaluation> {
    if samples == 0 || (train_deviators && br_traversals == 0) {
        bail!("evaluation requires positive samples and deviation traversals");
    }
    let mut mw_session = build().context("rebuilding the solution game")?;
    let num_players = mw_session.game_config.seats.len();
    let traversals = metadata.sweeps.saturating_mul(num_players as u64);
    let mut needed_histories: BTreeSet<_> = blocks.iter().map(|block| block.key.history).collect();
    let mut frontier: Vec<_> = needed_histories.iter().copied().collect();
    while let Some(history) = frontier.pop() {
        if history == [0; 16] {
            continue;
        }
        let edge = metadata
            .histories
            .binary_search_by_key(&history, |edge| edge.key)
            .ok()
            .map(|index| &metadata.histories[index])
            .ok_or_else(|| anyhow!("strategy history is absent from the public tree"))?;
        if edge.parent != [0; 16] && needed_histories.insert(edge.parent) {
            frontier.push(edge.parent);
        }
    }
    let state = SolverState {
        schema_version: SOLVER_STATE_VERSION,
        config: mw_session.solver.config(),
        traversals,
        completed_sweeps: metadata.sweeps,
        next_sample_id: traversals,
        total_deal_attempts: 0,
        terminal_evaluations: 0,
        hand_updates: traversals,
        histories: metadata
            .histories
            .iter()
            .filter(|entry| needed_histories.contains(&entry.key))
            .map(|entry| HistoryEntry {
                key: HistoryKey(entry.key),
                parent: HistoryKey(entry.parent),
                actor: entry.actor,
                action_index: entry.action_index,
                action_label: entry.action.clone(),
            })
            .collect(),
        policies: blocks
            .into_iter()
            .map(|block| PolicyEntry {
                key: InfoKey {
                    history: HistoryKey(block.key.history),
                    player: block.key.actor,
                    street: block.key.street,
                    active_opponents: block.key.active_opponents,
                    bucket_path: block.key.bucket_path,
                },
                column: PolicyColumn {
                    regrets: vec![0.0; block.probabilities.len()],
                    strategy_sum: block.probabilities,
                    action_labels: block.actions,
                },
            })
            .collect(),
    };
    let (game, sampler, config) = mw_session.solver.into_components();
    let restored = crate::MultiwaySolver::from_state_with_config_preallocated_with_threads(
        game,
        sampler,
        state,
        config,
        mw_session.threads,
    );
    mw_session.solver = restored.context("restoring the formal average profile")?;
    let deviators = if train_deviators {
        Some(session::train_deviators_parallel(
            &mw_session.solver,
            num_players,
            mw_session.threads,
            br_traversals,
            seed ^ 0x6576_616c,
            ProfileVariant::default(),
        )?)
    } else {
        None
    };
    let evaluation = mw_session.solver.evaluate_profile(
        samples,
        seed,
        deviators.as_deref(),
        ProfileVariant::default(),
    )?;
    Ok(evaluation)
}

/// Restore the saved average profile and return a held-out evaluation with trained deviations.
pub fn evaluate(
    path: &Path,
    samples: u64,
    seed: u64,
    br_traversals: u64,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<crate::ProfileEvaluation> {
    let (metadata, blocks) = read_solution(path)?;
    evaluate_prepared(
        || build_solution_session(&metadata.config_toml, cache_root, on_ready),
        &metadata,
        blocks,
        samples,
        seed,
        br_traversals,
        true,
    )
}

fn combo_class(combo: usize) -> usize {
    let (first, second) = nlh::combo_cards(combo);
    let (hi, lo) = if first.rank() >= second.rank() {
        (first, second)
    } else {
        (second, first)
    };
    nlh::class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit())
}

fn explicit_range(weights: &[f32]) -> Result<String> {
    let maximum = weights.iter().copied().fold(0.0_f32, f32::max);
    if maximum <= 0.0 {
        bail!("selected node has an unvisited/empty conditional range");
    }
    Ok(weights
        .iter()
        .enumerate()
        .filter(|(_, weight)| **weight > 0.0)
        .map(|(combo, weight)| {
            let (first, second) = nlh::combo_cards(combo);
            format!("{first}{second}:{}", weight / maximum)
        })
        .collect::<Vec<_>>()
        .join(","))
}

fn node_conditioned_evaluation(
    metadata: &crate::mwsol::MultiwaySolutionMetadata,
    mut blocks: Vec<crate::mwsol::MultiwayStrategyBlock>,
    target: [u8; 16],
    samples: u64,
    seed: u64,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<crate::ProfileEvaluation> {
    let mut config = parse_solution_config(&metadata.config_toml)?;
    let game = &mut config.game;
    let mut ranges = game
        .seats
        .iter()
        .map(|seat| Ok::<_, anyhow::Error>(parse_runtime_range(&seat.range)?.weights().to_vec()))
        .collect::<Result<Vec<_>>>()?;
    let path = metadata
        .resolve_history(target)
        .ok_or_else(|| anyhow!("history {} is absent from the public tree", key_hex(target)))?;
    let mut current = [0; 16];
    for step in path {
        let state = metadata
            .public_states
            .binary_search_by_key(&current, |state| state.history)
            .ok()
            .map(|index| &metadata.public_states[index])
            .ok_or_else(|| anyhow!("public history is internally inconsistent"))?;
        if state.street != 0 {
            bail!("node-conditioned EV currently requires a preflop node");
        }
        let mut class_probability = vec![None; nlh::NUM_CLASSES];
        for block in blocks
            .iter()
            .filter(|block| block.key.history == current && block.key.actor == step.actor)
        {
            class_probability[block.key.bucket_path[0] as usize] =
                block.probabilities.get(step.action_index as usize).copied();
        }
        for (combo, weight) in ranges[step.actor as usize].iter_mut().enumerate() {
            *weight *= class_probability[combo_class(combo)].unwrap_or(0.0);
        }
        for block in blocks
            .iter_mut()
            .filter(|block| block.key.history == current && block.key.actor == step.actor)
        {
            block.probabilities.fill(0.0);
            if let Some(probability) = block.probabilities.get_mut(step.action_index as usize) {
                *probability = 1.0;
            }
        }
        current = metadata
            .histories
            .iter()
            .find(|edge| {
                edge.parent == current
                    && edge.actor == step.actor
                    && edge.action_index == step.action_index
            })
            .map(|edge| edge.key)
            .ok_or_else(|| anyhow!("public history is internally inconsistent"))?;
    }
    for (seat_index, (seat, weights)) in game.seats.iter_mut().zip(&ranges).enumerate() {
        seat.range = explicit_range(weights)
            .with_context(|| format!("conditioning seat {seat_index} at {}", key_hex(target)))?;
    }
    let conditioned_seats = game.seats.clone();
    let session = || {
        let mut p = crate::prepare::prepare(&metadata.config_toml, Path::new("embedded.toml"))?;
        p.lowered.game.seats.clone_from(&conditioned_seats);
        crate::prepare::build_typed_session(p.lowered, p.effective, None, cache_root, on_ready)
    };
    evaluate_prepared(session, metadata, blocks, samples, seed, 0, false)
}

fn inspect_ev(
    path: &Path,
    requested_history: &str,
    samples: u64,
    seed: u64,
    br_traversals: u64,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(session::AbstractionReady),
) -> Result<InspectData> {
    let (metadata, blocks) = read_solution(path)?;
    let history = parse_history(&metadata, requested_history)?;
    let cache_path = path.with_extension("mwsol.inspect-cache.json");
    let fingerprint = runfiles::config_hash_hex(&metadata.config_fingerprint);
    if let Ok(contents) = std::fs::read_to_string(&cache_path)
        && let Ok(mut cached) = serde_json::from_str::<serde_json::Value>(&contents)
        && cached["solutionFingerprint"] == fingerprint
        && cached["history"] == key_hex(history)
        && cached["samples"] == samples
        && cached["seed"] == seed
        && cached["brTraversals"] == br_traversals
    {
        cached["cacheHit"] = serde_json::Value::Bool(true);
        return Ok(InspectData::CachedEv(cached));
    }
    let (scope, evaluation) = if history == [0; 16] {
        (
            "formal-average-profile",
            evaluate_prepared(
                || build_solution_session(&metadata.config_toml, cache_root, on_ready),
                &metadata,
                blocks,
                samples,
                seed,
                br_traversals,
                true,
            )?,
        )
    } else {
        (
            "node-conditioned-profile",
            node_conditioned_evaluation(
                &metadata, blocks, history, samples, seed, cache_root, on_ready,
            )?,
        )
    };
    let rendered = serde_json::json!({
        "scope": scope,
        "history": key_hex(history),
        "solutionFingerprint": fingerprint,
        "samples": samples,
        "seed": seed,
        "brTraversals": br_traversals,
        "cacheHit": false,
        "evaluation": evaluation,
    });
    std::fs::write(
        &cache_path,
        format!("{}\n", serde_json::to_string_pretty(&rendered)?),
    )
    .with_context(|| format!("writing evaluation cache {}", cache_path.display()))?;
    Ok(InspectData::Ev(serde_json::from_value(rendered)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_solution() -> crate::mwsol::MultiwaySolution {
        let raw = include_str!("../tests/fixtures/preflop_multiway_v1_3max_smoke.toml");
        let mut session = session::build_multiway_session(raw, None).unwrap();
        session.solver.run_sweeps_with_threads(1, 1).unwrap();
        let snapshot = session.solver.snapshot_state();
        let mut row = session::metrics_row(&session.solver.metrics(), vec![0.0; 3], 0.0, None);
        row.phase = "completed".into();
        session::make_solution(
            &session.config_toml,
            session.solver.abstraction_fingerprint(),
            session.solver.configuration_fingerprint(),
            session.solver.game(),
            &snapshot,
            &row,
        )
    }

    #[test]
    fn matching_ev_cache_preserves_opaque_fields_without_rebuilding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.mwsol");
        let solution = test_solution();
        crate::mwsol::write_mwsol_with(&path, &solution, crate::mwsol::MwsolStorage::F32).unwrap();
        let mut cached = serde_json::json!({
            "solutionFingerprint": runfiles::config_hash_hex(&solution.config_fingerprint),
            "history": key_hex([0; 16]), "samples": 2, "seed": 1, "brTraversals": 0,
            "cacheHit": false, "evaluation": {"opaque": true}, "extension": [1, 2, 3],
        });
        std::fs::write(
            path.with_extension("mwsol.inspect-cache.json"),
            cached.to_string(),
        )
        .unwrap();
        let data = inspect(
            &path,
            InspectRequest {
                history: "root",
                view: InspectView::Ev,
                actor: None,
                samples: 2,
                seed: 1,
                br_traversals: 0,
            },
            None,
            &mut |_| panic!("a matching evaluation cache must not rebuild the session"),
        )
        .unwrap();
        cached["cacheHit"] = serde_json::Value::Bool(true);
        assert_eq!(serde_json::to_value(data).unwrap(), cached);
    }

    #[test]
    fn removed_artifact_families_are_refused_on_every_read_surface() {
        let mut solution = test_solution();
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("current.mwsol");
        crate::mwsol::write_mwsol_with(&current, &solution, crate::mwsol::MwsolStorage::F32)
            .unwrap();
        for family in ["solvers.multiway-preflop/v1", "solvers.postflop/v1"] {
            solution.config_toml = format!("schema = '{family}'\n");
            solution.config_fingerprint = runfiles::config_hash(solution.config_toml.as_bytes());
            let old = dir.path().join("old.mwsol");
            crate::mwsol::write_mwsol_with(&old, &solution, crate::mwsol::MwsolStorage::F32)
                .unwrap();
            let mut outcomes = vec![
                inspect(
                    &old,
                    InspectRequest {
                        history: "root",
                        view: InspectView::Summary,
                        actor: None,
                        samples: 2,
                        seed: 1,
                        br_traversals: 0,
                    },
                    None,
                    &mut |_| {},
                )
                .map(|_| ()),
                evaluate(&old, 2, 1, 0, None, &mut |_| {}).map(|_| ()),
                compare(&old, &current, false, None, &mut |_| {}).map(|_| ()),
                compare(&current, &old, false, None, &mut |_| {}).map(|_| ()),
            ];
            for view in [
                ExportView::Summary,
                ExportView::Tree,
                ExportView::Strategy,
                ExportView::Range,
                ExportView::Actions,
                ExportView::Ev,
            ] {
                outcomes.push(export(&old, view).map(|_| ()));
            }
            for result in outcomes {
                let error = result.unwrap_err();
                let message = format!("{error:#}");
                assert!(message.contains(family), "{message}");
                assert!(
                    message.contains("re-solve from a solvers.nlh/v1 config"),
                    "{message}"
                );
                assert!(message.contains("docs/nlh-input-v1.jp.md"), "{message}");
            }
        }
    }
}
