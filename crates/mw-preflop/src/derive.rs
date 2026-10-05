//! Saved preflop average strategy → per-combo conditional ranges.
use crate::mwsol::{MultiwayPublicAction, MultiwaySolutionMetadata, MultiwayStrategyBlock};
use nlh::{NUM_CLASSES, NUM_COMBOS, Range, betting::Action};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DeriveError {
    #[error("{actor}: requested {requested}; available actions: {available}")]
    MissingAction {
        actor: String,
        requested: String,
        available: String,
    },
    #[error(transparent)]
    Input(#[from] spot::SpotError),
    #[error("{seat}: all derived combo weights are zero")]
    EmptyRange { seat: String },
    #[error("invalid P2 solution")]
    Artifact(#[source] anyhow::Error),
}

/// The source config and the two surviving seats, in postflop action order.
pub struct DerivedRanges {
    pub config_toml: String,
    pub seats: Vec<DerivedSeat>,
}
pub struct DerivedSeat {
    pub seat: usize,
    pub position: String,
    pub weights: Vec<f32>,
    pub range: String,
    pub unvisited_classes: Vec<String>,
}

fn combo_class(combo: usize) -> usize {
    let (a, b) = nlh::combo_cards(combo);
    nlh::class_index(
        a.rank().max(b.rank()),
        a.rank().min(b.rank()),
        a.suit() == b.suit(),
    )
}

/// Deterministic 169-class order; non-uniform classes use individual combos.
pub fn format_range(weights: &[f32]) -> String {
    assert_eq!(weights.len(), NUM_COMBOS);
    let mut entries = Vec::new();
    let entry = |label: String, weight: f32| {
        if weight == 1.0 {
            label
        } else {
            format!("{label}:{weight}")
        }
    };
    for class in 0..NUM_CLASSES {
        let combos: Vec<_> = (0..NUM_COMBOS)
            .filter(|&c| combo_class(c) == class)
            .collect();
        let weight = weights[combos[0]];
        if combos.iter().all(|&c| weights[c] == weight) {
            if weight > 0.0 {
                entries.push(entry(crate::views::hand_label(class), weight));
            }
        } else {
            for combo in combos {
                if weights[combo] > 0.0 {
                    let (a, b) = nlh::combo_cards(combo);
                    entries.push(entry(format!("{a}{b}"), weights[combo]));
                }
            }
        }
    }
    entries.join(",")
}

fn public_action(action: &Action) -> MultiwayPublicAction {
    match *action {
        Action::Fold => MultiwayPublicAction::Fold,
        Action::Check => MultiwayPublicAction::Check,
        Action::Call { amount, all_in } => MultiwayPublicAction::Call {
            amount_millibb: amount.0,
            all_in,
        },
        Action::BetTo {
            to,
            all_in,
            full_raise,
        } => MultiwayPublicAction::BetTo {
            amount_millibb: to.0,
            all_in,
            full_raise,
        },
        Action::RaiseTo {
            to,
            all_in,
            full_raise,
        } => MultiwayPublicAction::RaiseTo {
            amount_millibb: to.0,
            all_in,
            full_raise,
        },
    }
}

/// Read/validate a .mwsol and condition the two surviving ranges on the line.
pub fn derive(path: &Path, line: &str, board: &str) -> Result<DerivedRanges, DeriveError> {
    let (metadata, blocks) = crate::views::read_solution(path).map_err(DeriveError::Artifact)?;
    from_solution(&metadata, &blocks, line, board)
}

fn from_solution(
    metadata: &MultiwaySolutionMetadata,
    blocks: &[MultiwayStrategyBlock],
    line: &str,
    board: &str,
) -> Result<DerivedRanges, DeriveError> {
    let source = spot::Document::parse(&metadata.config_toml, Path::new("embedded.toml"))
        .map_err(|e| DeriveError::Artifact(e.into()))?;
    if source.spot.product != spot::Product::MultiwayPreflop {
        return Err(DeriveError::Artifact(anyhow::anyhow!(
            "embedded config must describe P2"
        )));
    }
    crate::input::Settings::parse(&source.spot, &source.solver, &source.output)
        .map_err(|e| DeriveError::Artifact(e.into()))?;
    let replay = spot::derive::replay(&source, line, board)?;
    let mut path = Vec::new();
    let mut current = [0; 16];
    for step in &replay.actions {
        let state = metadata
            .public_states
            .iter()
            .find(|s| s.history == current)
            .ok_or_else(|| {
                DeriveError::Artifact(anyhow::anyhow!("public history is internally inconsistent"))
            })?;
        let requested = public_action(&step.action);
        if state.actor != Some(step.seat.index() as u8) || state.street != 0 {
            // P2 applies a matched `checkdown` x/f without a decision node;
            // the replay's x/f at that turn is the same forced action.
            if matches!(step.action, Action::Fold | Action::Check) {
                continue;
            }
            return Err(DeriveError::MissingAction {
                actor: step.position.clone(),
                requested: requested.label(),
                available: "check or fold only, forced by the P2 tree rules".into(),
            });
        }
        let index = state
            .legal_actions
            .iter()
            .position(|a| a == &requested)
            .ok_or_else(|| DeriveError::MissingAction {
                actor: step.position.clone(),
                requested: requested.label(),
                available: state
                    .legal_actions
                    .iter()
                    .map(MultiwayPublicAction::label)
                    .collect::<Vec<_>>()
                    .join(", "),
            })?;
        path.push((current, step.seat.index() as u8, index as u32));
        current = metadata
            .histories
            .iter()
            .find(|e| {
                e.parent == current
                    && e.actor == step.seat.index() as u8
                    && e.action_index == index as u32
            })
            .map(|e| e.key)
            .ok_or_else(|| {
                DeriveError::Artifact(anyhow::anyhow!("public history edge is absent"))
            })?;
    }
    let mut seats = Vec::new();
    for player in [replay.oop.as_ref(), replay.ip.as_ref()]
        .into_iter()
        .flatten()
    {
        let start = &source.spot.ranges[player.seat].range;
        let (weights, unvisited_classes) =
            condition(start, blocks, &path, player.seat.index() as u8);
        if !weights.iter().any(|&w| w > 0.0) {
            return Err(DeriveError::EmptyRange {
                seat: player.position.clone(),
            });
        }
        seats.push(DerivedSeat {
            seat: player.seat.index(),
            position: player.position.clone(),
            range: format_range(&weights),
            weights,
            unvisited_classes,
        });
    }
    Ok(DerivedRanges {
        config_toml: metadata.config_toml.clone(),
        seats,
    })
}

fn condition(
    start: &Range,
    blocks: &[MultiwayStrategyBlock],
    path: &[([u8; 16], u8, u32)],
    seat: u8,
) -> (Vec<f32>, Vec<String>) {
    let mut probabilities = vec![Some(1.0_f64); NUM_CLASSES];
    for &(history, actor, action) in path.iter().filter(|(_, actor, _)| *actor == seat) {
        crate::views::apply_class_action(&mut probabilities, blocks, history, actor, action);
    }
    let weights = (0..NUM_COMBOS)
        .map(|combo| {
            (f64::from(start.weight(combo)) * probabilities[combo_class(combo)].unwrap_or(0.0))
                as f32
        })
        .collect();
    // Only classes the starting range holds can be dropped.
    let mut held = [false; NUM_CLASSES];
    for combo in (0..NUM_COMBOS).filter(|&c| start.weight(c) > 0.0) {
        held[combo_class(combo)] = true;
    }
    let unvisited = probabilities
        .iter()
        .enumerate()
        .filter(|&(c, p)| held[c] && p.is_none())
        .map(|(c, _)| crate::views::hand_label(c))
        .collect();
    (weights, unvisited)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_solution_without_blocks_refuses_empty_surviving_range() {
        let solution = crate::views::tests::test_solution();
        let metadata = MultiwaySolutionMetadata::from_solution(&solution);
        let result = from_solution(&metadata, &[], "BTN c, BB x", "Ks 7h 2d");
        assert!(matches!(result, Err(DeriveError::EmptyRange { .. })));
    }
    #[test]
    fn forced_checkdown_turns_need_no_decision_node() {
        let last = "river when !unopened { replace raise [75, a] }";
        let raw = crate::views::tests::SMOKE.replace(
            last,
            &format!("{last}\npreflop when position == SB {{ checkdown }}"),
        );
        assert_ne!(raw, crate::views::tests::SMOKE);
        let solution = crate::views::tests::solution_from(&raw);
        assert!(
            !solution
                .public_states
                .iter()
                .any(|s| s.street == 0 && s.actor == Some(1))
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("forced.mwsol");
        crate::mwsol::write_mwsol_with(&path, &solution, crate::mwsol::MwsolStorage::F32).unwrap();
        // SB's implicit fold is the forced x/f, so the line still matches.
        let derived = derive(&path, "BTN c, BB x", "Ks 7h 2d").unwrap();
        let positions: Vec<_> = derived.seats.iter().map(|s| s.position.as_str()).collect();
        assert_eq!(positions, ["BB", "BTN"]);
        let Err(DeriveError::MissingAction {
            actor, available, ..
        }) = derive(&path, "SB c, BB x", "Ks 7h 2d")
        else {
            panic!("SB cannot call past its forced fold");
        };
        assert_eq!(actor, "SB");
        assert!(available.contains("forced"), "{available}");
    }

    #[test]
    fn own_action_product_preserves_combo_weights_and_missing_probability() {
        let start: Range = "AA,AsKd:0.25".parse().unwrap();
        let key = crate::mwsol::MultiwayStrategyKey {
            history: [0; 16],
            actor: 0,
            street: 0,
            active_opponents: 2,
            bucket_path: [0, u32::MAX, u32::MAX, u32::MAX],
        };
        let mut blocks = Vec::new();
        for class in [0, nlh::class_index(12, 11, false)] {
            for (history, probability) in [([0; 16], 0.5), ([1; 16], 0.25)] {
                blocks.push(MultiwayStrategyBlock {
                    key: crate::mwsol::MultiwayStrategyKey {
                        history,
                        bucket_path: [class as u32, u32::MAX, u32::MAX, u32::MAX],
                        ..key
                    },
                    actions: vec!["call".into()],
                    probabilities: vec![probability],
                });
            }
        }
        let path = [([0; 16], 0, 0), ([9; 16], 1, 0), ([1; 16], 0, 0)];
        let (weights, missing) = condition(&start, &blocks, &path, 0);
        assert!(missing.is_empty(), "{missing:?}");
        for (combo, weight) in weights.iter().enumerate() {
            assert_eq!(*weight, start.weight(combo) * 0.125);
        }
        let text = format_range(&weights);
        assert!(text.starts_with("AA:0.125,"), "{text}");
        assert!(text.contains(":0.03125"));
        blocks
            .iter_mut()
            .find(|b| b.key.history == [1; 16] && b.key.bucket_path[0] == 0)
            .unwrap()
            .probabilities
            .clear();
        let (weights, missing) = condition(&start, &blocks, &path, 0);
        assert_eq!(missing, ["AA"]);
        for (combo, &weight) in weights.iter().enumerate() {
            if combo_class(combo) == 0 {
                assert_eq!(weight, 0.0);
            }
        }
    }
    #[test]
    fn unvisited_has_zero_weight_and_reports_classes() {
        let (weights, missing) = condition(&Range::full(), &[], &[([0; 16], 0, 0)], 0);
        assert_eq!(missing.len(), NUM_CLASSES);
        assert!(weights.iter().all(|&w| w == 0.0));
        let (weights, missing) = condition(&Range::full(), &[], &[([0; 16], 1, 0)], 0);
        assert!(missing.is_empty());
        assert!(weights.iter().all(|&w| w == 1.0));
    }
    #[test]
    fn formatting_preserves_nonuniform_starting_combos_and_roundtrips() {
        let start: Range = "AA,AKs:0.12345679,AsKd:0.25".parse().unwrap();
        let (weights, _) = condition(&start, &[], &[], 0);
        let formatted = format_range(&weights);
        assert!(formatted.starts_with("AA,AKs:0.12345679,"), "{formatted}");
        assert!(!formatted.contains("AKo"));
        assert!(formatted.contains(":0.25"));
        let parsed: Range = formatted.parse().unwrap();
        assert_eq!(parsed.weights(), start.weights());
        assert_eq!(format_range(&vec![0.0; NUM_COMBOS]), "");
    }
}
