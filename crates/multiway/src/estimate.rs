//! Pure-config dense-arena size estimator, factored out of the
//! [`crate::solver::MultiwaySolver::dense_arena_stats`] preflight so a
//! caller (CLI/GUI "auto mode" derivation, see [`crate::config::MultiwayConfig`])
//! can size a [`crate::config::RecallMode::Street`] arena *before* committing
//! to a card abstraction, without training a rollout/EHS² abstraction or
//! building a deal sampler.
//!
//! [`estimate_dense_arena`] builds the public betting tree with a
//! [`CountingAbstraction`] -- a zero-cost stand-in that only ever answers
//! "how many buckets does this street/opponent-count have", mirroring
//! exactly what [`crate::abstraction::RolloutKMeansAbstraction`] (per-street,
//! per-active-opponent-count budgets, honoring
//! [`crate::config::AbstractionConfig::active_opponent_buckets`]) and
//! [`crate::abstraction::TableAbstractionAdapter`] (flat per-street counts,
//! opponent-count-agnostic) would report, without any of their training or
//! Monte Carlo cost -- then reuses [`crate::tree::enumerate_tree`] and
//! [`crate::tree::build_arena`], the same functions the real preflight uses,
//! to size the arena.

use crate::abstraction::{BucketContext, BucketId, MultiwayAbstraction};
use crate::config::{AbstractionKind, MultiwayConfig, RakeConfig, UtilityConfig};
use crate::holdem::{HoldemGame, HoldemGameError};
use crate::tree::{self, TreeError};
use crate::types::Street;

/// Dense-arena preflight numbers computed from a [`MultiwayConfig`] alone,
/// without training any card abstraction or building a deal sampler. See
/// [`estimate_dense_arena`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseArenaEstimate {
    pub node_count: u64,
    pub total_columns: u64,
    pub estimated_bytes: u64,
}

/// Errors from [`estimate_dense_arena`]: either the config itself does not
/// describe a valid game (bad seats/blinds/betting; see
/// [`crate::config::ConfigError`], wrapped inside [`HoldemGameError::Config`]),
/// or the enumerated public tree overflows the safety caps in
/// [`crate::tree`].
#[derive(Debug, thiserror::Error)]
pub enum EstimateError {
    #[error(transparent)]
    Game(#[from] HoldemGameError),
    #[error(transparent)]
    Tree(#[from] TreeError),
}

/// Sizes the [`RecallMode::Street`](crate::config::RecallMode::Street) dense
/// arena `config` would preallocate, using only `config`'s own betting tree
/// and bucket-count fields -- no card abstraction is trained and no deal
/// sampler is built, so this runs in at most a few seconds even for a
/// several-million-node public tree (and far faster once a
/// `max_betting_players` check-down threshold shrinks the tree).
///
/// The game's `[utility]`/`[rake]` sections never affect the public betting
/// tree's shape, so this always evaluates the tree under `ChipEv`/no rake
/// regardless of what the caller's actual run configures there.
pub fn estimate_dense_arena(config: &MultiwayConfig) -> Result<DenseArenaEstimate, EstimateError> {
    let abstraction = CountingAbstraction::new(config);
    let game = HoldemGame::new(
        config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        abstraction,
    )?;
    let tree = tree::enumerate_tree(&game)?;
    let arena = tree::build_arena(&game, &tree, u64::MAX)?;
    Ok(DenseArenaEstimate {
        node_count: tree.nodes.len() as u64,
        total_columns: arena.total_columns(),
        estimated_bytes: arena.estimated_bytes(),
    })
}

/// A [`MultiwayAbstraction`] whose only real behavior is
/// [`MultiwayAbstraction::num_buckets`], answered directly from
/// [`crate::config::AbstractionConfig`] (mirroring
/// [`crate::abstraction::RolloutKMeansAbstraction::num_buckets`]'s
/// active-opponent-bucket lookup for `AbstractionKind::RolloutKmeans`, and
/// [`crate::abstraction::TableAbstractionAdapter::num_buckets`]'s flat,
/// opponent-count-agnostic counts for `AbstractionKind::Ehs2Table`).
/// [`MultiwayAbstraction::bucket`] is never called during tree enumeration or
/// arena sizing, so it is left unimplemented.
struct CountingAbstraction {
    kind: AbstractionKind,
    flop_buckets: u32,
    turn_buckets: u32,
    river_buckets: u32,
    active_opponent_buckets: Vec<crate::config::ActiveOpponentBucketConfig>,
}

impl CountingAbstraction {
    fn new(config: &MultiwayConfig) -> Self {
        let abstraction = &config.abstraction;
        Self {
            kind: abstraction.kind,
            flop_buckets: u32::from(abstraction.flop_buckets),
            turn_buckets: u32::from(abstraction.turn_buckets),
            river_buckets: u32::from(abstraction.river_buckets),
            active_opponent_buckets: abstraction.active_opponent_buckets.clone(),
        }
    }

    fn global_count(&self, street: Street) -> u32 {
        match street {
            Street::Preflop => cards::NUM_CLASSES as u32,
            Street::Flop => self.flop_buckets,
            Street::Turn => self.turn_buckets,
            Street::River => self.river_buckets,
        }
    }
}

impl MultiwayAbstraction for CountingAbstraction {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        if street == Street::Preflop || self.kind == AbstractionKind::Ehs2Table {
            return self.global_count(street);
        }
        let Some(profile) = self
            .active_opponent_buckets
            .iter()
            .find(|profile| profile.active_opponents == active_opponents)
        else {
            return self.global_count(street);
        };
        match street {
            Street::Flop => u32::from(profile.flop_buckets),
            Street::Turn => u32::from(profile.turn_buckets),
            Street::River => u32::from(profile.river_buckets),
            Street::Preflop => unreachable!("handled above"),
        }
    }

    fn bucket(&self, _context: BucketContext<'_>) -> BucketId {
        unimplemented!(
            "CountingAbstraction only estimates bucket counts for dense-arena preflight; it \
             never buckets an actual hand"
        )
    }

    fn fingerprint(&self) -> [u8; 32] {
        [0; 32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnteConfig, BettingConfig, BlindConfig, SeatConfig};
    use crate::solver::DenseArenaStats;
    use crate::types::SeatId;

    fn smoke_config() -> MultiwayConfig {
        MultiwayConfig {
            seats: (0..3)
                .map(|_| SeatConfig {
                    name: None,
                    stack_bb: 10.0,
                    range: String::new(),
                    betting: None,
                })
                .collect(),
            button: SeatId(0),
            blinds: BlindConfig::default(),
            ante: AnteConfig::None,
            betting: BettingConfig::default(),
            abstraction: crate::config::AbstractionConfig::default(),
        }
    }

    /// Agreement check with the real street-recall preflight
    /// (`MultiwaySolver::dense_arena_stats`), which builds an actual trained
    /// `FeatureHashAbstraction` game: since both walk the identical betting
    /// tree, node/column counts must match exactly once the estimator's
    /// `CountingAbstraction` is fed the same bucket counts the trained
    /// abstraction reports.
    #[test]
    fn agrees_with_the_trained_street_recall_preflight() {
        use crate::abstraction::{FeatureHashAbstraction, FeatureHashParams};
        use crate::solver::MultiwaySolver;

        let mut config = smoke_config();
        config.abstraction.recall = crate::config::RecallMode::Street;
        // FeatureHashAbstraction ignores active_opponents (flat per-street
        // counts), matching the estimator's ehs2-table-style codepath;
        // exercise that agreement by pinning `kind` there too, since
        // `FeatureHashAbstraction` and `CountingAbstraction` must both
        // resolve to the *same* flat counts for this to be an apples-to-apples
        // comparison.
        config.abstraction.kind = crate::config::AbstractionKind::Ehs2Table;
        config.abstraction.flop_buckets = 32;
        config.abstraction.turn_buckets = 32;
        config.abstraction.river_buckets = 32;

        let trained = FeatureHashAbstraction::new(FeatureHashParams {
            flop_buckets: 32,
            turn_buckets: 32,
            river_buckets: 32,
        })
        .unwrap();
        let game =
            HoldemGame::new(&config, &UtilityConfig::ChipEv, &RakeConfig::None, trained).unwrap();
        let sampler = game.deal_sampler().unwrap();
        let solver = MultiwaySolver::with_defaults(game, sampler).unwrap();
        let DenseArenaStats {
            node_count: expected_nodes,
            total_columns: expected_columns,
            estimated_bytes: expected_bytes,
            ..
        } = solver
            .dense_arena_stats()
            .expect("dense mode has arena stats");

        let estimate = estimate_dense_arena(&config).expect("pure-config estimate must succeed");
        assert_eq!(estimate.node_count, expected_nodes);
        assert_eq!(estimate.total_columns, expected_columns);
        assert_eq!(estimate.estimated_bytes, expected_bytes);
    }

    #[test]
    fn checkdown_config_shrinks_the_estimate() {
        let baseline = smoke_config();
        let baseline_estimate = estimate_dense_arena(&baseline).unwrap();

        let mut capped = baseline.clone();
        capped.betting.flop.max_betting_players = Some(1);
        capped.betting.turn.max_betting_players = Some(1);
        capped.betting.river.max_betting_players = Some(1);
        let capped_estimate = estimate_dense_arena(&capped).unwrap();

        assert!(capped_estimate.node_count < baseline_estimate.node_count);
        assert!(capped_estimate.total_columns < baseline_estimate.total_columns);
        assert!(capped_estimate.estimated_bytes < baseline_estimate.estimated_bytes);
    }

    #[test]
    fn active_opponent_bucket_overrides_change_the_estimate() {
        let mut config = smoke_config();
        let baseline = estimate_dense_arena(&config).unwrap();

        config.abstraction.active_opponent_buckets =
            vec![crate::config::ActiveOpponentBucketConfig {
                active_opponents: 2,
                flop_buckets: 512,
                turn_buckets: 512,
                river_buckets: 512,
            }];
        let overridden = estimate_dense_arena(&config).unwrap();
        assert!(overridden.estimated_bytes > baseline.estimated_bytes);
    }

    #[test]
    fn invalid_config_surfaces_as_an_estimate_error() {
        let mut config = smoke_config();
        config.seats.clear();
        assert!(matches!(
            estimate_dense_arena(&config),
            Err(EstimateError::Game(_))
        ));
    }
}
