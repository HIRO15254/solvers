use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use multiway::solver::{
    HistoryEntry, HistoryKey, InfoKey, PolicyColumn, PolicyEntry, ProfileVariant,
    SOLVER_STATE_VERSION, SolverState,
};
use multiway::{ExternalSamplingGame, HoldemGame, MultiwayAbstractionBackend};
use rand::{SeedableRng, rngs::StdRng};
use serde::Serialize;

use crate::session;

fn parse_solution_config(config_toml: &str) -> Result<crate::config::SolveConfig> {
    let (compatible, _) = crate::config::solution_artifact_compatible_config(config_toml)?;
    crate::config::parse_solve_config(&compatible)
}

fn build_solution_session(config_toml: &str) -> Result<session::MultiwaySession> {
    let (compatible, _) = crate::config::solution_artifact_compatible_config(config_toml)?;
    let config = crate::config::parse_solve_config(&compatible)?;
    let crate::config::GameSection::PreflopMultiway(game) = &config.game else {
        bail!("solution does not contain a Multiway Preflop game");
    };
    let retired = !matches!(
        game.abstraction.kind,
        multiway::config::AbstractionKind::Ehs2Table
    ) || !matches!(
        game.abstraction.recall,
        multiway::config::RecallMode::Street
    );
    #[cfg(not(feature = "research"))]
    if retired {
        bail!(
            "MWP004: this historical artifact uses retired rollout/full-recall semantics; \
             summary, tree, strategy, ranges, and recorded EV remain readable, but live \
             re-evaluation and real-card comparison require an opt-in research build"
        );
    }
    #[cfg(feature = "research")]
    {
        let _ = retired;
        session::build_multiway_session(&compatible, None)
    }
    #[cfg(not(feature = "research"))]
    {
        session::build_production_multiway_session(&compatible, None)
    }
}
fn key_hex(key: [u8; 16]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_solution(
    path: &Path,
) -> Result<(
    formats::MultiwaySolutionMetadata,
    Vec<formats::MultiwayStrategyBlock>,
)> {
    let mut reader =
        formats::MwSolReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let metadata = reader.metadata().clone();
    let mut strategies = Vec::with_capacity(reader.strategy_count());
    let mut cursor = 0;
    while cursor < reader.strategy_count() {
        let page = reader.read_strategy_page(cursor, formats::MWSOL_MAX_PAGE_LIMIT)?;
        cursor += page.strategies.len();
        strategies.extend(page.strategies);
    }
    Ok((metadata, strategies))
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum InspectView {
    Node,
    Summary,
    Strategy,
    Range,
    Ev,
}

fn parse_history(
    metadata: &formats::MultiwaySolutionMetadata,
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

fn parse_runtime_range(raw: &str) -> Result<cards::Range> {
    if raw.trim().is_empty() {
        Ok(cards::Range::full())
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
    strategies: &[formats::MultiwayStrategyBlock],
    weights: &[formats::MultiwayStrategyWeight],
    history: [u8; 16],
    actor: u8,
) -> Vec<Vec<serde_json::Value>> {
    let weight_by_key: BTreeMap<_, _> = weights
        .iter()
        .map(|entry| (entry.key, entry.weight))
        .collect();
    let cells = (0..cards::NUM_CLASSES)
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
    metadata: &formats::MultiwaySolutionMetadata,
    strategies: &[formats::MultiwayStrategyBlock],
    target: [u8; 16],
) -> Result<Vec<serde_json::Value>> {
    let config = parse_solution_config(&metadata.config_toml)?;
    let crate::config::GameSection::PreflopMultiway(game) = config.game else {
        bail!("range inspection requires a Multiway Preflop solution");
    };
    let mut ranges = game
        .seats
        .iter()
        .map(|seat| {
            let parsed = parse_runtime_range(&seat.range)?;
            let mut classes = vec![Some(0.0_f64); cards::NUM_CLASSES];
            for combo in 0..cards::NUM_COMBOS {
                let (first, second) = cards::combo_cards(combo);
                let (hi, lo) = if first.rank() >= second.rank() {
                    (first, second)
                } else {
                    (second, first)
                };
                let class = cards::class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit());
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

pub fn inspect(
    path: &Path,
    requested_history: &str,
    view: InspectView,
    actor_override: Option<u8>,
    samples: u64,
    seed: u64,
    br_traversals: u64,
) -> Result<()> {
    if matches!(view, InspectView::Ev) {
        return inspect_ev(path, requested_history, samples, seed, br_traversals);
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
        "formatVersion": formats::MWSOL_FORMAT_VERSION,
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
    println!("{}", serde_json::to_string_pretty(&rendered)?);
    Ok(())
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportView {
    Strategy,
    Actions,
    Range,
    Ev,
    Tree,
    Summary,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}

fn action_fields(
    action: &formats::MultiwayPublicAction,
) -> (&'static str, Option<u64>, bool, bool) {
    match action {
        formats::MultiwayPublicAction::Fold => ("fold", None, false, false),
        formats::MultiwayPublicAction::Check => ("check", None, false, false),
        formats::MultiwayPublicAction::Call {
            amount_millibb,
            all_in,
        } => ("call", Some(*amount_millibb), *all_in, false),
        formats::MultiwayPublicAction::BetTo {
            amount_millibb,
            all_in,
            full_raise,
        } => ("bet-to", Some(*amount_millibb), *all_in, *full_raise),
        formats::MultiwayPublicAction::RaiseTo {
            amount_millibb,
            all_in,
            full_raise,
        } => ("raise-to", Some(*amount_millibb), *all_in, *full_raise),
    }
}

pub fn export(
    path: &Path,
    view: ExportView,
    format: ExportFormat,
    output: Option<&Path>,
) -> Result<()> {
    let (metadata, strategies) = read_solution(path)?;
    let rendered = match (view, format) {
        (ExportView::Summary, ExportFormat::Json) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": metadata.schema_version,
                "sweeps": metadata.sweeps,
                "approximateProfile": metadata.approximate_profile,
                "visitedInfosets": strategies.len(),
                "seats": metadata.seats,
            }))?
        }
        (ExportView::Tree, ExportFormat::Json) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "states": metadata.public_states,
                "edges": metadata.histories,
            }))?
        }
        (ExportView::Strategy, ExportFormat::Json) => serde_json::to_string_pretty(&strategies)?,
        (ExportView::Ev, ExportFormat::Json) => serde_json::to_string_pretty(&metadata.seats)?,
        (ExportView::Range, ExportFormat::Json) => export_ranges_json(&metadata.config_toml)?,
        (ExportView::Actions, ExportFormat::Json) => {
            let rows: Vec<_> = metadata
                .public_states
                .iter()
                .filter(|state| !state.legal_actions.is_empty())
                .map(|state| {
                    serde_json::json!({
                        "history": key_hex(state.history),
                        "actor": state.actor,
                        "street": state.street,
                        "actions": state.legal_actions,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&rows)?
        }
        (ExportView::Strategy, ExportFormat::Csv) => {
            let mut csv = String::from(
                "history,actor,street,active_opponents,bucket_path,action,probability\n",
            );
            for block in &strategies {
                for (action, probability) in block.actions.iter().zip(&block.probabilities) {
                    csv.push_str(&format!(
                        "{},{},{},{},\"{:?}\",\"{}\",{}\n",
                        key_hex(block.key.history),
                        block.key.actor,
                        block.key.street,
                        block.key.active_opponents,
                        block.key.bucket_path,
                        action.replace('"', "\"\""),
                        probability
                    ));
                }
            }
            csv
        }
        (ExportView::Actions, ExportFormat::Csv) => {
            let mut csv = String::from(
                "history,actor,street,action_index,kind,amount_millibb,all_in,full_raise,label\n",
            );
            for state in &metadata.public_states {
                for (index, action) in state.legal_actions.iter().enumerate() {
                    let (kind, amount, all_in, full_raise) = action_fields(action);
                    csv.push_str(&format!(
                        "{},{},{},{},{},{},{},{},{}\n",
                        key_hex(state.history),
                        state
                            .actor
                            .map_or_else(String::new, |actor| actor.to_string()),
                        state.street,
                        index,
                        kind,
                        amount.map_or_else(String::new, |amount| amount.to_string()),
                        all_in,
                        full_raise,
                        action.label(),
                    ));
                }
            }
            csv
        }
        (ExportView::Tree, ExportFormat::Csv) => {
            let mut csv = String::from("history,parent,actor,action_index,action\n");
            for node in &metadata.histories {
                csv.push_str(&format!(
                    "{},{},{},{},\"{}\"\n",
                    key_hex(node.key),
                    key_hex(node.parent),
                    node.actor,
                    node.action_index,
                    node.action.replace('"', "\"\"")
                ));
            }
            csv
        }
        (ExportView::Summary, ExportFormat::Csv) => {
            format!(
                "schema_version,sweeps,approximate_profile,visited_infosets\n{},{},{},{}\n",
                metadata.schema_version,
                metadata.sweeps,
                metadata.approximate_profile,
                strategies.len()
            )
        }
        (ExportView::Range, ExportFormat::Csv) => export_ranges_csv(&metadata.config_toml)?,
        (ExportView::Ev, ExportFormat::Csv) => export_ev_csv(&metadata.seats),
    };
    if let Some(output) = output {
        std::fs::write(output, rendered)
            .with_context(|| format!("writing {}", output.display()))?;
    } else {
        println!("{rendered}");
    }
    Ok(())
}

fn export_ranges_json(config_toml: &str) -> Result<String> {
    let config = parse_solution_config(config_toml)?;
    let crate::config::GameSection::PreflopMultiway(game) = config.game else {
        bail!("range export requires a Multiway Preflop solution");
    };
    let rows: Vec<_> = game
        .seats
        .into_iter()
        .enumerate()
        .map(|(seat, value)| serde_json::json!({ "seat": seat, "range": value.range }))
        .collect();
    Ok(serde_json::to_string_pretty(&rows)?)
}

fn export_ranges_csv(config_toml: &str) -> Result<String> {
    let config = parse_solution_config(config_toml)?;
    let crate::config::GameSection::PreflopMultiway(game) = config.game else {
        bail!("range export requires a Multiway Preflop solution");
    };
    let mut csv = String::from("seat,range\n");
    for (seat, value) in game.seats.into_iter().enumerate() {
        csv.push_str(&format!(
            "{},\"{}\"\n",
            seat,
            value.range.replace('"', "\"\"")
        ));
    }
    Ok(csv)
}

fn export_ev_csv(seats: &[formats::MultiwaySeatResult]) -> String {
    let mut csv = String::from(
        "seat,profile_ev,stderr,ci95_low,ci95_high,measured_deviation_mean,measured_deviation_ci95_high\n",
    );
    for seat in seats {
        let ev = seat.profile_ev.as_ref();
        let deviation = seat.deviation_gain_lower_bound.as_ref();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            seat.seat,
            ev.map(|value| value.mean).unwrap_or(f64::NAN),
            ev.map(|value| value.stderr).unwrap_or(f64::NAN),
            ev.map(|value| value.ci95[0]).unwrap_or(f64::NAN),
            ev.map(|value| value.ci95[1]).unwrap_or(f64::NAN),
            deviation.map(|value| value.mean).unwrap_or(f64::NAN),
            deviation.map(|value| value.ci95[1]).unwrap_or(f64::NAN),
        ));
    }
    csv
}

fn comparison_mapping(config_toml: &str) -> Result<(usize, &'static str)> {
    let config = parse_solution_config(config_toml)?;
    let crate::config::GameSection::PreflopMultiway(game) = &config.game else {
        bail!("compare requires Multiway Preflop solutions");
    };
    let unit = match &config.utility {
        crate::config::UtilitySection::ChipEv => "bb",
        crate::config::UtilitySection::TournamentIcm { .. }
        | crate::config::UtilitySection::Icm { .. } => "prize",
    };
    Ok((game.seats.len(), unit))
}

fn comparison_recall(config_toml: &str) -> Result<multiway::RecallMode> {
    let config = parse_solution_config(config_toml)?;
    let crate::config::GameSection::PreflopMultiway(game) = config.game else {
        bail!("compare requires Multiway Preflop solutions");
    };
    Ok(game.abstraction.recall)
}

const CROSS_ABSTRACTION_SAMPLES: u64 = 1_024;
const CROSS_ABSTRACTION_SEED: u64 = 0x6d77_636f_6d70_6172;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SeatComparison {
    seat: u8,
    profile_ev_mean_delta: Option<f64>,
    profile_ev_stderr_delta: Option<f64>,
    average_positive_regret_delta: f64,
    strategy_drift_l1_delta: f64,
    deviation_gain_lower_bound_mean_delta: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Comparison {
    basis: &'static str,
    shared_infosets: usize,
    only_left: usize,
    only_right: usize,
    real_card_samples: Option<u64>,
    mean_strategy_l1: f64,
    max_strategy_l1: f64,
    sweep_delta: i128,
    left_stop_status: String,
    right_stop_status: String,
    seats: Vec<SeatComparison>,
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
        left_recall: multiway::RecallMode,
        right_recall: multiway::RecallMode,
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
    key: formats::MultiwayStrategyKey,
    left: &formats::MultiwayStrategyBlock,
    right: &formats::MultiwayStrategyBlock,
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
    metadata: &formats::MultiwaySolutionMetadata,
    history: [u8; 16],
) -> Result<multiway::BettingState> {
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
    left: &BTreeMap<formats::MultiwayStrategyKey, formats::MultiwayStrategyBlock>,
    right: &BTreeMap<formats::MultiwayStrategyKey, formats::MultiwayStrategyBlock>,
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
    left_meta: &formats::MultiwaySolutionMetadata,
    right_meta: &formats::MultiwaySolutionMetadata,
    left: &BTreeMap<formats::MultiwayStrategyKey, formats::MultiwayStrategyBlock>,
    right: &BTreeMap<formats::MultiwayStrategyKey, formats::MultiwayStrategyBlock>,
) -> Result<(usize, usize, usize, f64, f64)> {
    let left_session = build_solution_session(&left_meta.config_toml)
        .context("rebuilding the left abstraction")?;
    let right_session = build_solution_session(&right_meta.config_toml)
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
            let left_key = formats::MultiwayStrategyKey {
                history: *history,
                actor: *actor,
                street: left_private.street,
                active_opponents: left_private.active_opponents,
                bucket_path: left_private.bucket_path,
            };
            let right_key = formats::MultiwayStrategyKey {
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

pub fn compare(left_path: &Path, right_path: &Path, cross_game: bool) -> Result<()> {
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
            compare_shared_real_cards(&left_meta, &right_meta, &left, &right)?
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
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn evaluate_prepared(
    config_toml: &str,
    metadata: &formats::MultiwaySolutionMetadata,
    blocks: Vec<formats::MultiwayStrategyBlock>,
    samples: u64,
    seed: u64,
    br_traversals: u64,
    train_deviators: bool,
) -> Result<serde_json::Value> {
    if samples == 0 || (train_deviators && br_traversals == 0) {
        bail!("evaluation requires positive samples and deviation traversals");
    }
    let mut mw_session =
        build_solution_session(config_toml).context("rebuilding the solution game")?;
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
    #[cfg(feature = "research")]
    let restored = multiway::MultiwaySolver::from_state_with_config(game, sampler, state, config);
    #[cfg(not(feature = "research"))]
    let restored =
        multiway::MultiwaySolver::from_state_with_config_preallocated(game, sampler, state, config);
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
    Ok(serde_json::to_value(evaluation)?)
}

fn evaluate_value(
    path: &Path,
    samples: u64,
    seed: u64,
    br_traversals: u64,
) -> Result<serde_json::Value> {
    let (metadata, blocks) = read_solution(path)?;
    evaluate_prepared(
        &metadata.config_toml,
        &metadata,
        blocks,
        samples,
        seed,
        br_traversals,
        true,
    )
}

fn combo_class(combo: usize) -> usize {
    let (first, second) = cards::combo_cards(combo);
    let (hi, lo) = if first.rank() >= second.rank() {
        (first, second)
    } else {
        (second, first)
    };
    cards::class_index(hi.rank(), lo.rank(), hi.suit() == lo.suit())
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
            let (first, second) = cards::combo_cards(combo);
            format!("{first}{second}:{}", weight / maximum)
        })
        .collect::<Vec<_>>()
        .join(","))
}

fn node_conditioned_evaluation(
    metadata: &formats::MultiwaySolutionMetadata,
    mut blocks: Vec<formats::MultiwayStrategyBlock>,
    target: [u8; 16],
    samples: u64,
    seed: u64,
) -> Result<serde_json::Value> {
    let mut config = parse_solution_config(&metadata.config_toml)?;
    let crate::config::GameSection::PreflopMultiway(game) = &mut config.game else {
        bail!("node EV requires a Multiway Preflop solution");
    };
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
        let mut class_probability = vec![None; cards::NUM_CLASSES];
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
    let conditioned_config = toml::to_string(&config)?;
    evaluate_prepared(
        &conditioned_config,
        metadata,
        blocks,
        samples,
        seed,
        0,
        false,
    )
}

pub fn evaluate(path: &Path, samples: u64, seed: u64, br_traversals: u64) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&evaluate_value(path, samples, seed, br_traversals)?)?
    );
    Ok(())
}

fn inspect_ev(
    path: &Path,
    requested_history: &str,
    samples: u64,
    seed: u64,
    br_traversals: u64,
) -> Result<()> {
    let (metadata, blocks) = read_solution(path)?;
    let history = parse_history(&metadata, requested_history)?;
    let cache_path = path.with_extension("mwsol.inspect-cache.json");
    let fingerprint = formats::config_hash_hex(&metadata.config_fingerprint);
    if let Ok(contents) = std::fs::read_to_string(&cache_path)
        && let Ok(mut cached) = serde_json::from_str::<serde_json::Value>(&contents)
        && cached["solutionFingerprint"] == fingerprint
        && cached["history"] == key_hex(history)
        && cached["samples"] == samples
        && cached["seed"] == seed
        && cached["brTraversals"] == br_traversals
    {
        cached["cacheHit"] = serde_json::Value::Bool(true);
        println!("{}", serde_json::to_string_pretty(&cached)?);
        return Ok(());
    }
    let (scope, evaluation) = if history == [0; 16] {
        (
            "formal-average-profile",
            evaluate_prepared(
                &metadata.config_toml,
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
            node_conditioned_evaluation(&metadata, blocks, history, samples, seed)?,
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
    println!("{}", serde_json::to_string_pretty(&rendered)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ComparisonBasis, build_solution_session, comparison_recall, parse_solution_config,
    };
    use crate::session;
    use multiway::ExternalSamplingGame;

    #[test]
    fn solution_reads_normalize_only_historical_full_recall_pruning() {
        let v1 = format!(
            "{}\n[game.information]\nrecall = \"bucket-history\"\n\
             [solver.pruning]\nkind = \"regret-based\"\n",
            include_str!("../../../examples/preflop_multiway_v1_smoke.toml")
        );
        assert!(
            crate::config::parse_solve_config(&v1).is_err(),
            "new solve configs must reject the unsupported combination"
        );
        let (compatible, migrated) =
            crate::config::solution_artifact_compatible_config(&v1).unwrap();
        assert!(migrated);
        let parsed = parse_solution_config(&compatible).unwrap();
        let crate::config::AlgorithmSection::ExternalSamplingMccfr { prune, .. } = parsed.algorithm
        else {
            panic!()
        };
        assert!(!prune);
        #[cfg(not(feature = "research"))]
        {
            let Err(retired) = build_solution_session(&v1) else {
                panic!("retired v1 artifact must not rebuild a live production backend");
            };
            let retired = retired.to_string();
            assert!(retired.contains("MWP004"), "{retired}");
        }
        #[cfg(feature = "research")]
        assert!(
            build_solution_session(&v1).is_ok(),
            "research builds must reconstruct retired v1 abstractions"
        );

        let single_hand = v1.replace("kind = \"range-vector\"", "kind = \"single-hand\"");
        let (_, migrated) =
            crate::config::solution_artifact_compatible_config(&single_hand).unwrap();
        assert!(
            !migrated,
            "an independently invalid single-hand combination must not be relaxed"
        );

        let current_street = v1.replace("bucket-history", "current-street");
        let (_, migrated) =
            crate::config::solution_artifact_compatible_config(&current_street).unwrap();
        assert!(!migrated);

        let legacy = include_str!("../../../examples/preflop_multiway_3max_smoke.toml").replacen(
            "discount_until = 10000000",
            "discount_until = 10000000\ntraverser_vector = true\nprune = true",
            1,
        );
        assert!(
            session::build_multiway_session(&legacy, None).is_err(),
            "the engine must reject new legacy configs with the no-op bit"
        );
        let (compatible, migrated) =
            crate::config::solution_artifact_compatible_config(&legacy).unwrap();
        assert!(migrated);
        #[cfg(not(feature = "research"))]
        {
            let Err(retired) = build_solution_session(&compatible) else {
                panic!("retired legacy artifact must not rebuild a live production backend");
            };
            let retired = retired.to_string();
            assert!(retired.contains("MWP004"), "{retired}");
        }
        #[cfg(feature = "research")]
        assert!(
            build_solution_session(&compatible).is_ok(),
            "research builds must reconstruct retired legacy abstractions"
        );
    }

    #[test]
    fn compare_routes_different_recall_fingerprints_through_real_cards() {
        let bucket_history =
            include_str!("../../../examples/preflop_multiway_3max_smoke.toml").to_string();
        let anchor = "seed = 17\n";
        let current_street =
            bucket_history.replacen(anchor, &format!("{anchor}recall = \"street\"\n"), 1);
        assert_ne!(current_street, bucket_history);
        assert_eq!(
            comparison_recall(&bucket_history).unwrap(),
            multiway::RecallMode::Full
        );
        assert_eq!(
            comparison_recall(&current_street).unwrap(),
            multiway::RecallMode::Street
        );
        assert_eq!(
            comparison_recall(include_str!(
                "../../../examples/preflop_multiway_v1_smoke.toml"
            ))
            .unwrap(),
            multiway::RecallMode::Street,
            "the v1 default must lower to current-street recall"
        );

        let bucket_history =
            session::build_multiway_session(&bucket_history, None).expect("bucket-history session");
        let current_street =
            session::build_multiway_session(&current_street, None).expect("current-street session");
        let bucket_history_fingerprint = bucket_history.solver.abstraction_fingerprint();
        let current_street_fingerprint = current_street.solver.abstraction_fingerprint();

        assert_eq!(
            bucket_history.solver.game().game_fingerprint(),
            current_street.solver.game().game_fingerprint(),
            "recall must not turn a same-game comparison into cross-game mode"
        );
        assert_ne!(
            bucket_history_fingerprint, current_street_fingerprint,
            "recall semantics must be part of abstraction compatibility"
        );
        assert_eq!(
            ComparisonBasis::for_artifacts(
                bucket_history_fingerprint,
                bucket_history_fingerprint,
                multiway::RecallMode::Full,
                multiway::RecallMode::Full,
            ),
            ComparisonBasis::BucketInfoset
        );
        assert_eq!(
            ComparisonBasis::for_artifacts(
                bucket_history_fingerprint,
                current_street_fingerprint,
                multiway::RecallMode::Full,
                multiway::RecallMode::Street,
            ),
            ComparisonBasis::SharedRealCardSample
        );
        assert_eq!(
            ComparisonBasis::for_artifacts(
                bucket_history_fingerprint,
                bucket_history_fingerprint,
                multiway::RecallMode::Full,
                multiway::RecallMode::Street,
            ),
            ComparisonBasis::SharedRealCardSample,
            "legacy artifacts may share the backend fingerprint across recall modes"
        );
    }
}
