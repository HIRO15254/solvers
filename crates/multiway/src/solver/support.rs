use super::*;

pub(super) fn resume_configs_match(mut stored: SolverConfig, mut current: SolverConfig) -> bool {
    // This is a process resource guard, not part of the sampled algorithm.
    // Raising it after a resource-limit checkpoint must not alter results.
    stored.max_memory_bytes = 0;
    current.max_memory_bytes = 0;
    // `sweep_batch` is compared like every other algorithm knob (seed,
    // epsilon, discount cadence): batch size changes which linear weights
    // and how much staleness a within-batch update carries, so resuming
    // with a different batch size is rejected rather than silently mixed
    // into one checkpoint's history.
    stored == current
}

pub(super) fn validate_setup<G: ExternalSamplingGame>(
    game: &G,
    sampler: &DealSampler,
    config: SolverConfig,
) -> Result<(), SolverError> {
    let num_players = game.num_players();
    if !(MIN_SEATS..=MAX_SEATS).contains(&num_players) {
        return Err(SolverError::PlayerCount { found: num_players });
    }
    if sampler.num_players() != num_players {
        return Err(SolverError::SamplerPlayerCount {
            game: num_players,
            sampler: sampler.num_players(),
        });
    }
    if config.max_memory_bytes == 0 {
        return Err(SolverError::ZeroMemoryLimit);
    }
    if config.max_traversal_depth == 0 {
        return Err(SolverError::ZeroDepthLimit);
    }
    if !config.exploration_epsilon.is_finite() || !(0.0..=1.0).contains(&config.exploration_epsilon)
    {
        return Err(SolverError::InvalidExploration {
            epsilon: config.exploration_epsilon,
        });
    }
    if config.discount_every == 0 {
        return Err(SolverError::ZeroDiscountCadence);
    }
    if config.sweep_batch == 0 {
        return Err(SolverError::ZeroSweepBatch);
    }
    if config.prune {
        if !config.traverser_vector {
            return Err(SolverError::PruneRequiresVector);
        }
        if matches!(game.recall_mode(), RecallMode::Full) {
            return Err(SolverError::PruneRequiresStreetRecall);
        }
        if !config.prune_threshold.is_finite() || config.prune_threshold >= 0.0 {
            return Err(SolverError::PruneThresholdNotNegative(
                config.prune_threshold,
            ));
        }
        if !config.prune_skip_probability.is_finite()
            || !(0.0..=1.0).contains(&config.prune_skip_probability)
        {
            return Err(SolverError::PruneSkipProbabilityOutOfRange(
                config.prune_skip_probability,
            ));
        }
    }
    Ok(())
}

/// `Full` recall requires every already-reached street (`0..=info.street`)
/// to carry a real bucket and every later street to stay
/// [`UNREACHED_BUCKET`]. `Street` recall (imperfect recall) is stricter in
/// the other direction: *only* the current street's slot may be non-
/// sentinel -- earlier streets are deliberately never populated (see
/// [`PrivateInfo::from_current_bucket`]), not just later ones.
pub(super) fn validate_private_info(
    info: PrivateInfo,
    num_players: usize,
    recall: RecallMode,
) -> Result<(), SolverError> {
    if info.street > 3 {
        return Err(SolverError::InvalidPrivateInfo("street is outside 0..=3"));
    }
    if info.active_opponents == 0 || info.active_opponents as usize >= num_players {
        return Err(SolverError::InvalidPrivateInfo(
            "active opponents is outside 1..players",
        ));
    }
    match recall {
        RecallMode::Full => {
            let reached = info.street as usize + 1;
            if info.bucket_path[..reached].contains(&UNREACHED_BUCKET) {
                return Err(SolverError::InvalidPrivateInfo(
                    "reached street has sentinel bucket",
                ));
            }
            if info.bucket_path[reached..]
                .iter()
                .any(|&bucket| bucket != UNREACHED_BUCKET)
            {
                return Err(SolverError::InvalidPrivateInfo(
                    "future street bucket was exposed",
                ));
            }
        }
        RecallMode::Street => {
            let current = info.street as usize;
            if info.bucket_path[current] == UNREACHED_BUCKET {
                return Err(SolverError::InvalidPrivateInfo(
                    "current street has sentinel bucket",
                ));
            }
            if info
                .bucket_path
                .iter()
                .enumerate()
                .any(|(index, &bucket)| index != current && bucket != UNREACHED_BUCKET)
            {
                return Err(SolverError::InvalidPrivateInfo(
                    "non-current street bucket was exposed under street recall",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_column(
    key: InfoKey,
    column: &PolicyColumn,
    num_players: usize,
    recall: RecallMode,
) -> Result<(), SolverError> {
    if key.player as usize >= num_players {
        return Err(SolverError::InvalidState("policy player is out of range"));
    }
    validate_private_info(
        PrivateInfo {
            street: key.street,
            active_opponents: key.active_opponents,
            bucket_path: key.bucket_path,
        },
        num_players,
        recall,
    )?;
    validate_action_labels(&column.action_labels)?;
    if column.regrets.is_empty()
        || column.regrets.len() != column.strategy_sum.len()
        || column.regrets.len() != column.action_labels.len()
    {
        return Err(SolverError::InvalidState(
            "policy vectors are empty or have different lengths",
        ));
    }
    if column
        .regrets
        .iter()
        .chain(&column.strategy_sum)
        .any(|value| !value.is_finite())
    {
        return Err(SolverError::InvalidState(
            "policy contains a non-finite value",
        ));
    }
    if column.strategy_sum.iter().any(|&value| value < 0.0) {
        return Err(SolverError::InvalidState(
            "strategy sums must be non-negative",
        ));
    }
    Ok(())
}

pub(super) fn validate_history_entry(
    entry: &HistoryEntry,
    num_players: usize,
) -> Result<(), SolverError> {
    if entry.key == HistoryKey::ROOT
        || entry.actor as usize >= num_players
        || entry.action_label.is_empty()
        || entry.key
            != entry
                .parent
                .child(entry.actor as usize, entry.action_index as usize)
    {
        return Err(SolverError::InvalidState("invalid public history entry"));
    }
    Ok(())
}

#[cfg(any(feature = "research-abstractions", test))]
pub(super) fn validate_history_graph(
    histories: &FxHashMap<HistoryKey, HistoryEntry>,
) -> Result<(), SolverError> {
    for entry in histories.values() {
        let mut key = entry.parent;
        for _ in 0..=histories.len() {
            if key == HistoryKey::ROOT {
                break;
            }
            key = histories
                .get(&key)
                .ok_or(SolverError::InvalidState(
                    "public history has an unknown parent",
                ))?
                .parent;
        }
        if key != HistoryKey::ROOT {
            return Err(SolverError::InvalidState("public history contains a cycle"));
        }
    }
    Ok(())
}

pub(super) fn validate_action_labels(labels: &[String]) -> Result<(), SolverError> {
    if labels.iter().any(|label| label.is_empty()) {
        return Err(SolverError::InvalidActionLabels("empty action label"));
    }
    for (index, label) in labels.iter().enumerate() {
        if labels[..index].contains(label) {
            return Err(SolverError::InvalidActionLabels("duplicate action label"));
        }
    }
    Ok(())
}

pub(super) fn entry_memory_bytes(action_labels: &[String]) -> Result<u64, SolverError> {
    let labels = action_labels.iter().try_fold(0u64, |total, label| {
        total
            .checked_add(size_of::<String>() as u64)
            .and_then(|value| value.checked_add(label.len() as u64))
            .ok_or(SolverError::MemoryAccountingOverflow)
    })?;
    let vector_bytes = (action_labels.len() as u64)
        .checked_mul(2 * size_of::<f32>() as u64)
        .ok_or(SolverError::MemoryAccountingOverflow)?;
    ((size_of::<InfoKey>() + size_of::<PolicyColumn>()) as u64)
        .checked_add(ENTRY_OVERHEAD_BYTES)
        .and_then(|value| value.checked_add(vector_bytes))
        .and_then(|value| value.checked_add(labels))
        .ok_or(SolverError::MemoryAccountingOverflow)
}

pub(super) fn history_memory_bytes(action_label: &str) -> Result<u64, SolverError> {
    (size_of::<HistoryKey>() as u64)
        .checked_add(size_of::<HistoryEntry>() as u64)
        .and_then(|value| value.checked_add(HISTORY_OVERHEAD_BYTES))
        .and_then(|value| value.checked_add(action_label.len() as u64))
        .ok_or(SolverError::MemoryAccountingOverflow)
}

pub(super) fn regret_matching(regrets: &[f32]) -> Vec<f64> {
    let sum = regrets
        .iter()
        .map(|&regret| f64::from(regret.max(0.0)))
        .sum::<f64>();
    if sum > 0.0 {
        regrets
            .iter()
            .map(|&regret| f64::from(regret.max(0.0)) / sum)
            .collect()
    } else {
        vec![1.0 / regrets.len() as f64; regrets.len()]
    }
}

pub(super) fn regret_matching_f32(regrets: &[f32]) -> Vec<f32> {
    let sum = regrets.iter().map(|&regret| regret.max(0.0)).sum::<f32>();
    if sum > 0.0 {
        regrets
            .iter()
            .map(|&regret| regret.max(0.0) / sum)
            .collect()
    } else {
        vec![1.0 / regrets.len() as f32; regrets.len()]
    }
}

pub(super) fn normalize_nonnegative_f32(values: &[f32]) -> Option<Vec<f32>> {
    let sum = values.iter().copied().sum::<f32>();
    (sum > 0.0).then(|| values.iter().map(|&value| value / sum).collect())
}

pub(super) fn checked_add_f32(target: &mut f32, delta: f64) -> Result<(), SolverError> {
    let value = f64::from(*target) + delta;
    if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
        return Err(SolverError::NumericOverflow);
    }
    *target = value as f32;
    Ok(())
}

pub(super) fn sample_exploratory_action(
    strategy: &[f64],
    epsilon: f64,
    rng: &mut ChaCha20Rng,
) -> (usize, f64) {
    let needle = rng.gen_range(0.0..1.0);
    let mut cumulative = 0.0;
    let uniform = epsilon / strategy.len() as f64;
    for (action, &probability) in strategy.iter().enumerate() {
        let sampling_probability = (1.0 - epsilon) * probability + uniform;
        cumulative += sampling_probability;
        if needle < cumulative {
            return (action, sampling_probability);
        }
    }
    let action = strategy.len() - 1;
    let sampling_probability = (1.0 - epsilon) * strategy[action] + uniform;
    (action, sampling_probability)
}

pub(super) fn sample_profile_action(strategy: &[f32], rng: &mut ChaCha20Rng) -> usize {
    let needle = rng.gen_range(0.0..1.0);
    let mut cumulative = 0.0;
    for (action, &probability) in strategy.iter().enumerate() {
        cumulative += f64::from(probability);
        if needle < cumulative {
            return action;
        }
    }
    strategy.len() - 1
}

pub(super) fn traversal_deal_rng(seed: u64, sample_id: u64, traverser: usize) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.mccfr-deal.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    hasher.update(&(traverser as u64).to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

pub(super) fn traversal_action_rng(seed: u64, sample_id: u64, traverser: usize) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.mccfr-actions.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    hasher.update(&(traverser as u64).to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

pub(super) fn evaluation_deal_rng(seed: u64, sample_id: u64) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.profile-evaluation-deal.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

pub(super) fn evaluation_action_rng(
    seed: u64,
    sample_id: u64,
    deviator: Option<usize>,
) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    match deviator {
        Some(player) => {
            hasher.update(b"solvers.multiway.profile-evaluation-deviation.v1");
            hasher.update(&(player as u64).to_le_bytes());
        }
        None => {
            hasher.update(b"solvers.multiway.profile-evaluation-baseline.v1");
        }
    }
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

/// Action stream for the TRAINED-deviator replay inside
/// [`MultiwaySolver::evaluate_profile`]: distinct from
/// `evaluation_action_rng(_, _, Some(seat))`, which stays reserved for the
/// regret-greedy candidate replay so that candidate's estimate is exactly
/// the one a plain [`MultiwaySolver::evaluate_average_profile`] call would
/// produce.
pub(super) fn deviator_evaluation_action_rng(
    seed: u64,
    sample_id: u64,
    seat: usize,
) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.profile-evaluation-trained-deviation.v1");
    hasher.update(&(seat as u64).to_le_bytes());
    hasher.update(&seed.to_le_bytes());
    hasher.update(&sample_id.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

pub(super) fn deviator_training_deal_rng(seed: u64, seat: usize, traversal: u64) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.deviator-training-deal.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&(seat as u64).to_le_bytes());
    hasher.update(&traversal.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

pub(super) fn deviator_training_action_rng(seed: u64, seat: usize, traversal: u64) -> ChaCha20Rng {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"solvers.multiway.deviator-training-action.v1");
    hasher.update(&seed.to_le_bytes());
    hasher.update(&(seat as u64).to_le_bytes());
    hasher.update(&traversal.to_le_bytes());
    ChaCha20Rng::from_seed(*hasher.finalize().as_bytes())
}

pub(super) fn traversals_for_player(traversals: u64, num_players: usize, player: usize) -> u64 {
    traversals / num_players as u64 + u64::from((player as u64) < traversals % num_players as u64)
}
