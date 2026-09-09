//! Bounded, non-retaining public-tree census for Multiway Preflop research.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use cli::config::GameSection;
use multiway::abstraction::{BucketContext, BucketId, MultiwayAbstraction};
use multiway::config::{RakeConfig, UtilityConfig};
use multiway::{ExternalSamplingGame, HoldemGame, Street};
use serde::Serialize;

const DEFAULT_ARENA_LIMIT_BYTES: u64 = 160 * 1024 * 1024 * 1024;

#[derive(Parser)]
#[command(name = "mw_tree_census")]
struct Args {
    /// Production v1 config whose public betting tree is measured.
    #[arg(long)]
    config: PathBuf,

    /// Flop,turn,river bucket counts used only for this resource estimate.
    #[arg(long, value_parser = parse_buckets)]
    buckets: BucketSchedule,

    /// Research-only replacement for flop/turn/river aggression caps.
    #[arg(long)]
    postflop_cap: Option<u8>,

    /// Stop before processing a decision node beyond this count.
    #[arg(long, default_value_t = 10_000_000)]
    max_nodes: u64,

    /// Stop between decision nodes after this wall-clock duration.
    #[arg(long, default_value_t = 60.0)]
    max_seconds: f64,

    /// Arena payload budget used for the final fit verdict.
    #[arg(long, default_value_t = DEFAULT_ARENA_LIMIT_BYTES)]
    arena_limit_bytes: u64,

    /// Structural action-depth guard.
    #[arg(long, default_value_t = 512)]
    max_depth: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct BucketSchedule {
    flop: u32,
    turn: u32,
    river: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct AggressionCaps {
    preflop: u8,
    flop: u8,
    turn: u8,
    river: u8,
}

fn parse_buckets(value: &str) -> Result<BucketSchedule, String> {
    let values = value
        .split(',')
        .map(|part| {
            part.parse::<u32>()
                .map_err(|_| format!("invalid bucket count {part:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [flop, turn, river] = values.as_slice() else {
        return Err("--buckets requires exactly FLOP,TURN,RIVER".to_string());
    };
    if [*flop, *turn, *river].contains(&0) {
        return Err("bucket counts must be positive".to_string());
    }
    Ok(BucketSchedule {
        flop: *flop,
        turn: *turn,
        river: *river,
    })
}

#[derive(Clone, Copy)]
struct CensusAbstraction(BucketSchedule);

impl MultiwayAbstraction for CensusAbstraction {
    fn num_buckets(&self, street: Street, _active_opponents: u8) -> u32 {
        match street {
            Street::Preflop => cards::NUM_CLASSES as u32,
            Street::Flop => self.0.flop,
            Street::Turn => self.0.turn,
            Street::River => self.0.river,
        }
    }

    fn bucket(&self, _context: BucketContext<'_>) -> BucketId {
        unreachable!("the structural census never assigns physical hands")
    }

    fn fingerprint(&self) -> [u8; 32] {
        [0; 32]
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    decision_nodes: u64,
    public_action_edges: u64,
    policy_columns: u64,
    policy_slots: u64,
}

impl Counts {
    fn add_node(&mut self, actions: u64, buckets: u64) -> Result<()> {
        self.decision_nodes = checked_add(self.decision_nodes, 1, "decision nodes")?;
        self.public_action_edges = checked_add(self.public_action_edges, actions, "action edges")?;
        self.policy_columns = checked_add(self.policy_columns, buckets, "policy columns")?;
        self.policy_slots = checked_add(
            self.policy_slots,
            buckets
                .checked_mul(actions)
                .ok_or_else(|| anyhow!("policy slot count overflow"))?,
            "policy slots",
        )?;
        Ok(())
    }
}

fn checked_add(left: u64, right: u64, label: &str) -> Result<u64> {
    left.checked_add(right)
        .ok_or_else(|| anyhow!("{label} overflow"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupReport {
    street: &'static str,
    bucket_active_opponents: u8,
    buckets_per_node: u32,
    #[serde(flatten)]
    counts: Counts,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum StopReason {
    NodeLimit,
    TimeLimit,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CensusReport {
    schema_version: &'static str,
    config: String,
    config_blake3: String,
    bucket_schedule: BucketSchedule,
    effective_aggression_caps: AggressionCaps,
    complete: bool,
    counts_are_lower_bounds: bool,
    stop_reason: Option<StopReason>,
    elapsed_secs: f64,
    max_nodes: u64,
    max_seconds: f64,
    max_depth: u32,
    terminal_edges: u64,
    totals: Counts,
    estimated_arena_payload_bytes: u64,
    arena_limit_bytes: u64,
    fits_arena_limit: Option<bool>,
    fits_u32_columns: Option<bool>,
    groups: Vec<GroupReport>,
    interpretation: &'static str,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.max_nodes == 0 {
        bail!("--max-nodes must be positive");
    }
    if !args.max_seconds.is_finite() || args.max_seconds <= 0.0 {
        bail!("--max-seconds must be finite and positive");
    }
    if args.arena_limit_bytes == 0 {
        bail!("--arena-limit-bytes must be positive");
    }
    if args.postflop_cap == Some(0) {
        bail!("--postflop-cap must be positive");
    }
    let report = run_census(&args)?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
    println!();
    Ok(())
}

fn run_census(args: &Args) -> Result<CensusReport> {
    let raw = fs::read_to_string(&args.config)
        .with_context(|| format!("reading {}", args.config.display()))?;
    cli::multiway_v1::validate_production_contract_at(&raw, &args.config)
        .context("validating production Multiway Preflop contract")?;
    let lowered = cli::multiway_v1::parse_and_lower_at(&raw, &args.config)
        .context("lowering production Multiway Preflop config")?;
    let GameSection::PreflopMultiway(mut game_config) = lowered.game else {
        bail!("expected a Multiway Preflop v1 config");
    };
    if let Some(cap) = args.postflop_cap {
        game_config.betting.flop.max_aggressive_actions = cap;
        game_config.betting.turn.max_aggressive_actions = cap;
        game_config.betting.river.max_aggressive_actions = cap;
    }
    let effective_aggression_caps = AggressionCaps {
        preflop: game_config.betting.preflop.max_aggressive_actions,
        flop: game_config.betting.flop.max_aggressive_actions,
        turn: game_config.betting.turn.max_aggressive_actions,
        river: game_config.betting.river.max_aggressive_actions,
    };
    let game = HoldemGame::new(
        &game_config,
        &UtilityConfig::ChipEv,
        &RakeConfig::None,
        CensusAbstraction(args.buckets),
    )
    .context("building the public Holdem state machine")?;
    census_game(&game, args, &args.config, &raw, effective_aggression_caps)
}

fn census_game<G: ExternalSamplingGame>(
    game: &G,
    args: &Args,
    config_path: &Path,
    raw_config: &str,
    effective_aggression_caps: AggressionCaps,
) -> Result<CensusReport> {
    let root = game.root_state();
    if game.actor(&root).is_none() {
        bail!("public tree root is terminal");
    }
    let started = Instant::now();
    let time_limit = Duration::from_secs_f64(args.max_seconds);
    let mut stack = vec![(root, 0_u32)];
    let mut totals = Counts::default();
    let mut terminal_edges = 0_u64;
    let mut groups = BTreeMap::<(u8, u8), (u32, Counts)>::new();
    let mut stop_reason = None;

    while !stack.is_empty() {
        if totals.decision_nodes >= args.max_nodes {
            stop_reason = Some(StopReason::NodeLimit);
            break;
        }
        if started.elapsed() >= time_limit {
            stop_reason = Some(StopReason::TimeLimit);
            break;
        }
        let (state, depth) = stack.pop().expect("checked nonempty");
        let actor = game.actor(&state).expect("only decision states are pushed");
        let actions = game.node_actions(&state);
        let num_actions = game.num_actions_of(&actions);
        if num_actions == 0 {
            bail!("actor {actor} has no legal actions");
        }
        let actions_u64 = u64::try_from(num_actions).context("counting legal actions")?;
        let context = game.dense_node_context(&state);
        let bucket_count = game.bucket_count(context.street, context.bucket_active_opponents);
        if bucket_count == 0 {
            bail!("abstraction returned zero buckets");
        }
        totals.add_node(actions_u64, u64::from(bucket_count))?;
        let entry = groups
            .entry((
                context.street.index() as u8,
                context.bucket_active_opponents,
            ))
            .or_insert((bucket_count, Counts::default()));
        if entry.0 != bucket_count {
            bail!("bucket count changed within one street/opponent context");
        }
        entry.1.add_node(actions_u64, u64::from(bucket_count))?;

        let child_depth = depth
            .checked_add(1)
            .ok_or_else(|| anyhow!("public tree depth overflow"))?;
        if child_depth > args.max_depth {
            bail!("public tree exceeds --max-depth {}", args.max_depth);
        }
        for action in (0..num_actions).rev() {
            let child = game.next_state_with(&state, &actions, action);
            if game.actor(&child).is_some() {
                stack.push((child, child_depth));
            } else {
                terminal_edges = checked_add(terminal_edges, 1, "terminal edges")?;
            }
        }
    }

    let complete = stop_reason.is_none();
    let estimated_arena_payload_bytes = estimate_arena_bytes(totals)?;
    let groups = groups
        .into_iter()
        .map(
            |((street, bucket_active_opponents), (buckets_per_node, counts))| GroupReport {
                street: street_name(street),
                bucket_active_opponents,
                buckets_per_node,
                counts,
            },
        )
        .collect();
    Ok(CensusReport {
        schema_version: "solvers.multiway-tree-census/v1",
        config: config_path.display().to_string(),
        config_blake3: blake3::hash(raw_config.as_bytes()).to_hex().to_string(),
        bucket_schedule: args.buckets,
        effective_aggression_caps,
        complete,
        counts_are_lower_bounds: !complete,
        stop_reason,
        elapsed_secs: started.elapsed().as_secs_f64(),
        max_nodes: args.max_nodes,
        max_seconds: args.max_seconds,
        max_depth: args.max_depth,
        terminal_edges,
        totals,
        estimated_arena_payload_bytes,
        arena_limit_bytes: args.arena_limit_bytes,
        fits_arena_limit: if complete || estimated_arena_payload_bytes > args.arena_limit_bytes {
            Some(estimated_arena_payload_bytes <= args.arena_limit_bytes)
        } else {
            None
        },
        fits_u32_columns: if complete || totals.policy_columns > u64::from(u32::MAX) + 1 {
            Some(totals.policy_columns <= u64::from(u32::MAX) + 1)
        } else {
            None
        },
        groups,
        interpretation: "complete=false reports only the observed deterministic DFS prefix; counts and bytes are lower bounds; a false fit verdict can be proven by that lower bound, while an unproven fit remains null",
    })
}

fn estimate_arena_bytes(counts: Counts) -> Result<u64> {
    let slots = counts
        .policy_slots
        .checked_mul(2 * std::mem::size_of::<f32>() as u64)
        .ok_or_else(|| anyhow!("arena slot byte count overflow"))?;
    let touched = counts
        .policy_columns
        .div_ceil(64)
        .checked_mul(8)
        .ok_or_else(|| anyhow!("arena touched-bit byte count overflow"))?;
    let nodes = counts
        .decision_nodes
        .checked_mul(24)
        .ok_or_else(|| anyhow!("arena node-table byte count overflow"))?;
    slots
        .checked_add(touched)
        .and_then(|value| value.checked_add(nodes))
        .and_then(|value| value.checked_add(16))
        .ok_or_else(|| anyhow!("arena payload byte count overflow"))
}

fn street_name(index: u8) -> &'static str {
    match index {
        0 => "preflop",
        1 => "flop",
        2 => "turn",
        3 => "river",
        _ => unreachable!("Street has four variants"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_schedule_is_strict() {
        assert_eq!(
            parse_buckets("128,64,32").unwrap(),
            BucketSchedule {
                flop: 128,
                turn: 64,
                river: 32
            }
        );
        assert!(parse_buckets("128,64").is_err());
        assert!(parse_buckets("128,0,32").is_err());
        assert!(parse_buckets("x,64,32").is_err());
    }

    #[test]
    fn arena_formula_matches_hand_calculation() {
        let counts = Counts {
            decision_nodes: 2,
            public_action_edges: 0,
            policy_columns: 65,
            policy_slots: 10,
        };
        assert_eq!(estimate_arena_bytes(counts).unwrap(), 160);
    }

    #[test]
    fn complete_census_matches_core_nonretaining_preflight() {
        let config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/preflop_multiway_v1_3max_smoke.toml");
        let raw = fs::read_to_string(&config_path).unwrap();
        let lowered = cli::multiway_v1::parse_and_lower_at(&raw, &config_path).unwrap();
        let GameSection::PreflopMultiway(game_config) = lowered.game else {
            panic!("expected multiway game")
        };
        let buckets = BucketSchedule {
            flop: 64,
            turn: 32,
            river: 16,
        };
        let game = HoldemGame::new(
            &game_config,
            &UtilityConfig::ChipEv,
            &RakeConfig::None,
            CensusAbstraction(buckets),
        )
        .unwrap();
        let args = Args {
            config: config_path.clone(),
            buckets,
            postflop_cap: None,
            max_nodes: 1_000_000,
            max_seconds: 10.0,
            arena_limit_bytes: u64::MAX,
            max_depth: 512,
        };
        let caps = AggressionCaps {
            preflop: game_config.betting.preflop.max_aggressive_actions,
            flop: game_config.betting.flop.max_aggressive_actions,
            turn: game_config.betting.turn.max_aggressive_actions,
            river: game_config.betting.river.max_aggressive_actions,
        };
        let report = census_game(&game, &args, &config_path, &raw, caps).unwrap();
        let core = multiway::tree::preflight_arena(&game, u64::MAX).unwrap();
        assert!(report.complete);
        assert_eq!(report.totals.decision_nodes, core.node_count as u64);
        assert_eq!(report.terminal_edges, core.terminal_edges);
        assert_eq!(report.totals.policy_columns, core.total_columns);
        assert_eq!(report.totals.policy_slots, core.total_slots);
        assert_eq!(
            report.estimated_arena_payload_bytes,
            core.estimated_arena_bytes
        );
    }
}
