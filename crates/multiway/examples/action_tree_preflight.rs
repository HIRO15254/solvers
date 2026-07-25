//! Public-tree and dense-arena preflight for the tournament/cash action grids.
//!
//! This example deliberately uses a non-bucketing abstraction: it reports
//! 169 preflop buckets and defaults to 256 buckets on each postflop street,
//! but panics if physical-card bucketing is ever requested. Use
//! `--flop-buckets`, `--turn-buckets`, and `--river-buckets` to preflight
//! another postflop allocation without performing rollout or clustering.
//! Public-tree enumeration and dense-arena sizing therefore include no cache,
//! deal, utility, or rake work.
//!
//! Exactly one case and one seat count are run per process. The default
//! `benchmark` profile implements the canonical benchmark action contract:
//!
//! ```text
//! cargo run --release -p multiway --example action_tree_preflight -- \
//!   --case tournament --seats 6 --stack-bb 50 --node-limit 5000000
//! cargo run --release -p multiway --example action_tree_preflight -- \
//!   --case cash --seats 9 --stack-bb 800
//! cargo run --release -p multiway --example action_tree_preflight -- \
//!   --case cash --seats 6 --stack-bb 100 \
//!   --flop-buckets 128 --turn-buckets 128 --river-buckets 256 \
//!   --max-memory-bytes 8589934592
//! ```
//!
//! To capture peak RSS and wall time on a Linux GCP VM, build once and wrap
//! the binary with GNU time:
//!
//! ```text
//! cargo build --release -p multiway --example action_tree_preflight
//! /usr/bin/time -v target/release/examples/action_tree_preflight \
//!   --case tournament --seats 6 --stack-bb 50 --node-limit 5000000
//! ```
//!
//! The benchmark tournament tree forbids limps; opens to 2bb or all-in;
//! re-raises to 2.5x at the 3bet and 2x at 4bet+ or all-in; permits at most
//! two non-BB open cold callers; and forbids first-time callers after a
//! 3bet. The benchmark cash tree forbids limps and open jams; opens only to
//! 2.5bb; re-raises to 3x in position or 5x out of position relative to the
//! immediately preceding aggressor, plus all-in; permits open cold calls only
//! from BTN/SB/BB; and likewise forbids first-time callers after a 3bet.
//! Cash normal 3bet+ targets strictly above one third of the actor's starting
//! stack collapse to all-in. Both profiles allow six preflop aggressive
//! actions. Every postflop street offers a 50%-pot normal bet, a
//! 2.5x-previous-bet normal raise, and an additional all-in wherever legal
//! and distinct, with four total aggressive actions per street.
//!
//! `--profile legacy-rich` reproduces the previous rich-preflop harness.
//! That compatibility profile alone also accepts `--postflop checkdown`.
//!
//! `--stack-bb` may override the equal-stack depth: Tournament accepts
//! `(0, 50]` and Cash accepts `[100, 800]`. `TreeError::TooManyNodes` and
//! `TreeError::MemoryLimit` are emitted as successful, typed `RESULT`
//! records. The default resource boundary is the solver's 6GiB dense-arena
//! byte limit. The preflight walks the tree without retaining public nodes or
//! allocating the arena and stops at the first node prefix that crosses that
//! byte limit. `--node-limit` is an optional benchmark checkpoint; omitted,
//! the only node bound is the `u32` NodeId representation limit. In
//! particular, `--node-limit 50000000` is available solely to reproduce the
//! old 50M checkpoint and is not a production feasibility rule.
//!
//! The benchmark betting profiles are embedded from the tracked warm-artifact
//! compatibility TOMLs instead of being duplicated in this harness. Benchmark
//! games also use the canonical v1 button/forced-bet layout and Tournament ICM
//! or Cash rake economics. START/RESULT emit the fingerprint computed from the
//! actual preflight game so the matrix runner can verify it against the plan.

use std::env;
use std::error::Error;
use std::fmt;
use std::process;
use std::time::Instant;

use multiway::config::{
    AbstractionConfig, AnteConfig, BettingConfig, BlindConfig, ForcedBetConfig, RakeAllocation,
    RakeConfig, RakeRounding, RecallMode, RuleStreet, SizeSpec, StreetBettingConfig,
};
use multiway::tree::{MAX_TREE_NODES, preflight_arena_with_limits};
use multiway::{
    BucketContext, BucketId, ExternalSamplingGame, HoldemGame, MultiwayAbstraction, MultiwayConfig,
    SeatConfig, SeatId, Street, TreeError, UtilityConfig,
};
use serde::Deserialize;

const PREFLOP_BUCKETS: u32 = 169;
const DEFAULT_POSTFLOP_BUCKETS: u16 = 256;
const DEFAULT_MAX_MEMORY_BYTES: u64 = 6 * 1024 * 1024 * 1024;
const TREE_CONTRACT_FINGERPRINT_DOMAIN: &[u8] = b"solvers.abstraction-transfer.tree-contract/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    Benchmark,
    LegacyRich,
}

impl Profile {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "benchmark" => Ok(Self::Benchmark),
            "legacy-rich" => Ok(Self::LegacyRich),
            _ => Err(CliError(format!(
                "invalid --profile {value:?}; expected benchmark or legacy-rich"
            ))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Benchmark => "benchmark",
            Self::LegacyRich => "legacy-rich",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Tournament,
    Cash,
}

impl Case {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "tournament" => Ok(Self::Tournament),
            "cash" => Ok(Self::Cash),
            _ => Err(CliError(format!(
                "invalid --case {value:?}; expected tournament or cash"
            ))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Tournament => "tournament",
            Self::Cash => "cash",
        }
    }

    const fn default_stack_bb(self) -> f64 {
        match self {
            Self::Tournament => 50.0,
            Self::Cash => 100.0,
        }
    }

    fn validate_stack_bb(self, stack_bb: f64) -> Result<f64, CliError> {
        let valid = stack_bb.is_finite()
            && match self {
                Self::Tournament => stack_bb > 0.0 && stack_bb <= 50.0,
                Self::Cash => (100.0..=800.0).contains(&stack_bb),
            };
        if valid {
            Ok(stack_bb)
        } else {
            let expected = match self {
                Self::Tournament => "greater than 0 and at most 50",
                Self::Cash => "from 100 through 800",
            };
            Err(CliError(format!(
                "invalid --stack-bb {stack_bb}; expected {expected} for {}",
                self.name()
            )))
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PostflopMode {
    OneSize,
    Checkdown,
}

impl PostflopMode {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "one-size" => Ok(Self::OneSize),
            "checkdown" => Ok(Self::Checkdown),
            _ => Err(CliError(format!(
                "invalid --postflop {value:?}; expected one-size or checkdown"
            ))),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::OneSize => "one-size",
            Self::Checkdown => "checkdown",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Options {
    profile: Profile,
    case: Case,
    seats: usize,
    stack_bb: f64,
    postflop: PostflopMode,
    flop_buckets: u16,
    turn_buckets: u16,
    river_buckets: u16,
    node_limit: Option<usize>,
    max_memory_bytes: u64,
}

#[derive(Debug)]
struct CliError(String);

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CliError {}

/// Count-only abstraction for public-tree preflight. A panic on `bucket`
/// makes accidental card-model work fail loudly instead of polluting the
/// resource measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreflightAbstraction {
    flop_buckets: u16,
    turn_buckets: u16,
    river_buckets: u16,
}

impl Default for PreflightAbstraction {
    fn default() -> Self {
        Self {
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
        }
    }
}

impl MultiwayAbstraction for PreflightAbstraction {
    fn num_buckets(&self, street: Street, _active_opponents: u8) -> u32 {
        match street {
            Street::Preflop => PREFLOP_BUCKETS,
            Street::Flop => u32::from(self.flop_buckets),
            Street::Turn => u32::from(self.turn_buckets),
            Street::River => u32::from(self.river_buckets),
        }
    }

    fn bucket(&self, _context: BucketContext<'_>) -> BucketId {
        panic!("action_tree_preflight must never evaluate physical cards")
    }

    fn fingerprint(&self) -> [u8; 32] {
        if *self == Self::default() {
            // Preserve the identity emitted by the original fixed-K256
            // preflight so existing checkpoint and benchmark provenance does
            // not move when the new flags are omitted.
            return [0xa7; 32];
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"solvers.multiway.action-tree-preflight.v2");
        hasher.update(&self.flop_buckets.to_le_bytes());
        hasher.update(&self.turn_buckets.to_le_bytes());
        hasher.update(&self.river_buckets.to_le_bytes());
        *hasher.finalize().as_bytes()
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR {error}");
        print_usage();
        process::exit(2);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let config = build_config(options);
    let tree_contract_fingerprint = tree_contract_fingerprint(&config.betting)?;
    let (utility, rake) = economics(options);
    let abstraction = PreflightAbstraction {
        flop_buckets: options.flop_buckets,
        turn_buckets: options.turn_buckets,
        river_buckets: options.river_buckets,
    };
    let game = HoldemGame::new(&config, &utility, &rake, abstraction)?;
    let game_fingerprint = blake3::Hash::from_bytes(game.game_fingerprint())
        .to_hex()
        .to_string();
    let node_limit = options.node_limit.unwrap_or(MAX_TREE_NODES);
    let node_limit_kind = if options.node_limit.is_some() {
        "checkpoint"
    } else {
        "representation"
    };
    let legacy_postflop_buckets = if options.flop_buckets == options.turn_buckets
        && options.turn_buckets == options.river_buckets
    {
        options.flop_buckets.to_string()
    } else {
        "mixed".to_owned()
    };

    println!(
        "START profile={} case={} seats={} stack_bb={} postflop={} \
         game_fingerprint={} tree_contract_fingerprint={} preflop_buckets={} \
         postflop_buckets={} flop_buckets={} turn_buckets={} river_buckets={} \
         node_limit_kind={} node_limit={} \
         memory_limit_kind=dense-arena-estimate-bytes max_memory_bytes={} \
         preflight_peak_rss_bytes={}",
        options.profile.name(),
        options.case.name(),
        options.seats,
        options.stack_bb,
        options.postflop.name(),
        game_fingerprint,
        tree_contract_fingerprint,
        PREFLOP_BUCKETS,
        legacy_postflop_buckets,
        options.flop_buckets,
        options.turn_buckets,
        options.river_buckets,
        node_limit_kind,
        node_limit,
        options.max_memory_bytes,
        peak_rss_bytes(),
    );

    let preflight_start = Instant::now();
    match preflight_arena_with_limits(&game, node_limit, options.max_memory_bytes) {
        Ok(preflight) => {
            println!(
                "RESULT profile={} case={} seats={} stack_bb={} postflop={} \
                 game_fingerprint={} tree_contract_fingerprint={} \
                 flop_buckets={} turn_buckets={} river_buckets={} outcome=complete \
                 node_limit_kind={} node_limit={} max_memory_bytes={} nodes={} columns={} \
                 slots={} dense_arena_bytes={} wall_seconds={:.6} \
                 preflight_peak_rss_bytes={}",
                options.profile.name(),
                options.case.name(),
                options.seats,
                options.stack_bb,
                options.postflop.name(),
                game_fingerprint,
                tree_contract_fingerprint,
                options.flop_buckets,
                options.turn_buckets,
                options.river_buckets,
                node_limit_kind,
                node_limit,
                options.max_memory_bytes,
                preflight.node_count,
                preflight.total_columns,
                preflight.total_slots,
                preflight.estimated_arena_bytes,
                preflight_start.elapsed().as_secs_f64(),
                peak_rss_bytes(),
            );
            Ok(())
        }
        Err(TreeError::TooManyNodes { limit }) => {
            println!(
                "RESULT profile={} case={} seats={} stack_bb={} postflop={} \
                 game_fingerprint={} tree_contract_fingerprint={} \
                 flop_buckets={} turn_buckets={} river_buckets={} \
                 outcome=node-checkpoint tree_error=TooManyNodes node_limit_kind={} \
                 node_limit={} max_memory_bytes={} enumerated_nodes={} attempted_node={} \
                 wall_seconds={:.6} preflight_peak_rss_bytes={}",
                options.profile.name(),
                options.case.name(),
                options.seats,
                options.stack_bb,
                options.postflop.name(),
                game_fingerprint,
                tree_contract_fingerprint,
                options.flop_buckets,
                options.turn_buckets,
                options.river_buckets,
                node_limit_kind,
                limit,
                options.max_memory_bytes,
                limit,
                limit.saturating_add(1),
                preflight_start.elapsed().as_secs_f64(),
                peak_rss_bytes(),
            );
            Ok(())
        }
        Err(TreeError::MemoryLimit {
            node_count,
            total_columns,
            limit,
            needed,
        }) => {
            println!(
                "RESULT profile={} case={} seats={} stack_bb={} postflop={} \
                 game_fingerprint={} tree_contract_fingerprint={} \
                 flop_buckets={} turn_buckets={} river_buckets={} \
                 outcome=memory-limit tree_error=MemoryLimit node_limit_kind={} \
                 node_limit={} memory_limit_kind=dense-arena-estimate-bytes max_memory_bytes={} \
                 first_exceeding_prefix_nodes={} first_exceeding_prefix_columns={} \
                 first_exceeding_dense_arena_bytes={} largest_accepted_prefix_nodes={} \
                 wall_seconds={:.6} preflight_peak_rss_bytes={}",
                options.profile.name(),
                options.case.name(),
                options.seats,
                options.stack_bb,
                options.postflop.name(),
                game_fingerprint,
                tree_contract_fingerprint,
                options.flop_buckets,
                options.turn_buckets,
                options.river_buckets,
                node_limit_kind,
                node_limit,
                limit,
                node_count,
                total_columns,
                needed,
                node_count.saturating_sub(1),
                preflight_start.elapsed().as_secs_f64(),
                peak_rss_bytes(),
            );
            Ok(())
        }
        Err(error) => Err(Box::new(error)),
    }
}

fn tree_contract_fingerprint(config: &BettingConfig) -> Result<String, serde_json::Error> {
    let payload = serde_json::to_vec(config)?;
    let mut material =
        Vec::with_capacity(TREE_CONTRACT_FINGERPRINT_DOMAIN.len() + 1 + payload.len());
    material.extend_from_slice(TREE_CONTRACT_FINGERPRINT_DOMAIN);
    material.push(0);
    material.extend_from_slice(&payload);
    Ok(blake3::hash(&material).to_hex().to_string())
}

#[cfg(unix)]
fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to writable storage for exactly one `rusage`,
    // and `getrusage` initializes it when it returns zero.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if status != 0 {
        return 0;
    }
    // SAFETY: a zero return from `getrusage` initializes the output object.
    let raw = unsafe { usage.assume_init() }.ru_maxrss;
    let raw = u64::try_from(raw).unwrap_or(0);
    if cfg!(target_os = "macos") {
        raw
    } else {
        raw.saturating_mul(1024)
    }
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> u64 {
    0
}

fn parse_options() -> Result<Options, CliError> {
    parse_options_from(env::args().skip(1))
}

fn parse_options_from(arguments: impl IntoIterator<Item = String>) -> Result<Options, CliError> {
    let mut profile = None;
    let mut case = None;
    let mut seats = None;
    let mut stack_bb = None;
    let mut postflop = None;
    let mut flop_buckets = None;
    let mut turn_buckets = None;
    let mut river_buckets = None;
    let mut node_limit = None;
    let mut max_memory_bytes = None;
    let mut args = arguments.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--profile" => {
                if profile.is_some() {
                    return Err(CliError("--profile may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--profile requires a value".into()))?;
                profile = Some(Profile::parse(&value)?);
            }
            "--case" => {
                if case.is_some() {
                    return Err(CliError("--case may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--case requires a value".into()))?;
                case = Some(Case::parse(&value)?);
            }
            "--seats" => {
                if seats.is_some() {
                    return Err(CliError("--seats may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--seats requires a value".into()))?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| CliError(format!("invalid --seats {value:?}")))?;
                if !(6..=9).contains(&parsed) {
                    return Err(CliError("--seats must be from 6 through 9".into()));
                }
                seats = Some(parsed);
            }
            "--postflop" => {
                if postflop.is_some() {
                    return Err(CliError("--postflop may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--postflop requires a value".into()))?;
                postflop = Some(PostflopMode::parse(&value)?);
            }
            "--stack-bb" => {
                if stack_bb.is_some() {
                    return Err(CliError("--stack-bb may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--stack-bb requires a value".into()))?;
                stack_bb = Some(
                    value
                        .parse::<f64>()
                        .map_err(|_| CliError(format!("invalid --stack-bb {value:?}")))?,
                );
            }
            "--flop-buckets" => {
                if flop_buckets.is_some() {
                    return Err(CliError("--flop-buckets may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--flop-buckets requires a value".into()))?;
                flop_buckets = Some(parse_bucket_count("--flop-buckets", &value)?);
            }
            "--turn-buckets" => {
                if turn_buckets.is_some() {
                    return Err(CliError("--turn-buckets may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--turn-buckets requires a value".into()))?;
                turn_buckets = Some(parse_bucket_count("--turn-buckets", &value)?);
            }
            "--river-buckets" => {
                if river_buckets.is_some() {
                    return Err(CliError("--river-buckets may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--river-buckets requires a value".into()))?;
                river_buckets = Some(parse_bucket_count("--river-buckets", &value)?);
            }
            "--node-limit" => {
                if node_limit.is_some() {
                    return Err(CliError("--node-limit may be supplied only once".into()));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--node-limit requires a value".into()))?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| CliError(format!("invalid --node-limit {value:?}")))?;
                if parsed == 0 || parsed > MAX_TREE_NODES {
                    return Err(CliError(format!(
                        "--node-limit must be from 1 through {MAX_TREE_NODES}"
                    )));
                }
                node_limit = Some(parsed);
            }
            "--max-memory-bytes" => {
                if max_memory_bytes.is_some() {
                    return Err(CliError(
                        "--max-memory-bytes may be supplied only once".into(),
                    ));
                }
                let value = args
                    .next()
                    .ok_or_else(|| CliError("--max-memory-bytes requires a value".into()))?;
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| CliError(format!("invalid --max-memory-bytes {value:?}")))?;
                if parsed == 0 {
                    return Err(CliError(
                        "--max-memory-bytes must be greater than zero".into(),
                    ));
                }
                max_memory_bytes = Some(parsed);
            }
            "--help" | "-h" => {
                print_usage();
                process::exit(0);
            }
            _ => {
                return Err(CliError(format!("unknown argument {argument:?}")));
            }
        }
    }
    let case = case.ok_or_else(|| CliError("missing required --case".into()))?;
    let profile = profile.unwrap_or(Profile::Benchmark);
    let postflop = postflop.unwrap_or(PostflopMode::OneSize);
    if profile == Profile::Benchmark && matches!(postflop, PostflopMode::Checkdown) {
        return Err(CliError(
            "--postflop checkdown is available only with --profile legacy-rich".into(),
        ));
    }
    let stack_bb = case.validate_stack_bb(stack_bb.unwrap_or(case.default_stack_bb()))?;
    Ok(Options {
        profile,
        case,
        seats: seats.ok_or_else(|| CliError("missing required --seats".into()))?,
        stack_bb,
        postflop,
        flop_buckets: flop_buckets.unwrap_or(DEFAULT_POSTFLOP_BUCKETS),
        turn_buckets: turn_buckets.unwrap_or(DEFAULT_POSTFLOP_BUCKETS),
        river_buckets: river_buckets.unwrap_or(DEFAULT_POSTFLOP_BUCKETS),
        node_limit,
        max_memory_bytes: max_memory_bytes.unwrap_or(DEFAULT_MAX_MEMORY_BYTES),
    })
}

fn parse_bucket_count(flag: &str, value: &str) -> Result<u16, CliError> {
    let parsed = value
        .parse::<u16>()
        .map_err(|_| CliError(format!("invalid {flag} {value:?}; expected 1..=65535")))?;
    if parsed == 0 {
        return Err(CliError(format!(
            "invalid {flag} {value:?}; expected 1..=65535"
        )));
    }
    Ok(parsed)
}

fn print_usage() {
    eprintln!(
        "usage: action_tree_preflight [--profile <benchmark|legacy-rich>] \
         --case <tournament|cash> --seats <6|7|8|9> \
         [--stack-bb <depth>] [--postflop <one-size|checkdown>] \
         [--flop-buckets <1..=65535>] [--turn-buckets <1..=65535>] \
         [--river-buckets <1..=65535>] \
         [--node-limit <decision-node-checkpoint>] [--max-memory-bytes <bytes>]"
    );
}

fn build_config(options: Options) -> MultiwayConfig {
    let mut betting = match options.profile {
        Profile::Benchmark => benchmark_betting(options.case),
        Profile::LegacyRich => {
            let (allow_limp, preflop) = legacy_rich_preflop(options.case);
            let postflop = StreetBettingConfig {
                bet_sizes: vec![SizeSpec::PotAfterCall { fraction: 0.5 }],
                isolate_sizes: None,
                raise_sizes: vec![SizeSpec::PreviousBetMultiple { factor: 2.5 }],
                max_aggressive_actions: 4,
                include_allin: true,
                allin_threshold: None,
                reraise_jam_above_actor_starting_stack: None,
                max_betting_players: None,
            };
            BettingConfig {
                allow_limp,
                preflop,
                flop: postflop.clone(),
                turn: postflop.clone(),
                river: postflop,
                rules: Vec::new(),
            }
        }
    };
    if matches!(options.postflop, PostflopMode::Checkdown) {
        let checkdown = StreetBettingConfig {
            bet_sizes: Vec::new(),
            isolate_sizes: None,
            raise_sizes: Vec::new(),
            max_aggressive_actions: 0,
            include_allin: false,
            allin_threshold: None,
            reraise_jam_above_actor_starting_stack: None,
            // Any live postflop hand has at least two non-folded players, so
            // a threshold of one skips every remaining street.
            max_betting_players: Some(1),
        };
        betting.flop = checkdown.clone();
        betting.turn = checkdown.clone();
        betting.river = checkdown;
        betting
            .rules
            .retain(|rule| rule.street == RuleStreet::Preflop);
    }

    let (button, ante, forced_bets) = match options.profile {
        Profile::Benchmark => {
            let mut blinds_bb = vec![0.0; options.seats];
            blinds_bb[options.seats - 2] = 0.5;
            blinds_bb[options.seats - 1] = 1.0;
            let ante_bb = match options.case {
                Case::Tournament => 0.125,
                Case::Cash => 0.0,
            };
            (
                SeatId((options.seats - 3) as u8),
                AnteConfig::None,
                Some(ForcedBetConfig {
                    blinds_bb,
                    antes_bb: vec![ante_bb; options.seats],
                    common_ante_bb: 0.0,
                    nominal_big_blind_bb: 1.0,
                    first_to_act: SeatId(0),
                }),
            )
        }
        Profile::LegacyRich => (
            SeatId(0),
            match options.case {
                Case::Tournament => AnteConfig::Each { amount_bb: 0.125 },
                Case::Cash => AnteConfig::None,
            },
            None,
        ),
    };

    MultiwayConfig {
        seats: (0..options.seats)
            .map(|_| SeatConfig {
                name: None,
                stack_bb: options.stack_bb,
                range: String::new(),
                betting: None,
            })
            .collect(),
        button,
        blinds: BlindConfig::default(),
        ante,
        betting,
        forced_bets,
        abstraction: AbstractionConfig {
            flop_buckets: options.flop_buckets,
            turn_buckets: options.turn_buckets,
            river_buckets: options.river_buckets,
            recall: RecallMode::Street,
            ..AbstractionConfig::default()
        },
    }
}

fn economics(options: Options) -> (UtilityConfig, RakeConfig) {
    if matches!(options.profile, Profile::LegacyRich) {
        return (UtilityConfig::ChipEv, RakeConfig::None);
    }
    match options.case {
        Case::Tournament => {
            let mut payouts = vec![0.0; options.seats];
            payouts[..3].copy_from_slice(&[50.0, 30.0, 20.0]);
            (
                UtilityConfig::TournamentIcm {
                    outside_field: Vec::new(),
                    payouts,
                    samples: 100_000,
                    seed: 0,
                },
                RakeConfig::None,
            )
        }
        Case::Cash => (
            UtilityConfig::ChipEv,
            RakeConfig::Generic {
                rate: 0.05,
                cap_bb: Some(4.0),
                when: "flop_dealt".to_owned(),
                allocation: RakeAllocation::MainFirst,
                rounding: RakeRounding::Down,
            },
        ),
    }
}

#[derive(Deserialize)]
struct EmbeddedBenchmarkConfig {
    game: EmbeddedBenchmarkGame,
}

#[derive(Deserialize)]
struct EmbeddedBenchmarkGame {
    betting: BettingConfig,
}

fn benchmark_betting(case: Case) -> BettingConfig {
    let raw = match case {
        Case::Tournament => include_str!(
            "../../../experiments/abstraction-2026-07-23/tournament-6max-50bb-one-size-postflop.toml"
        ),
        Case::Cash => include_str!(
            "../../../experiments/abstraction-2026-07-23/cash-6max-100bb-one-size-postflop.toml"
        ),
    };
    toml::from_str::<EmbeddedBenchmarkConfig>(raw)
        .expect("tracked benchmark compatibility config must parse")
        .game
        .betting
}

fn legacy_rich_preflop(case: Case) -> (bool, StreetBettingConfig) {
    let preflop = match case {
        Case::Tournament => StreetBettingConfig {
            bet_sizes: vec![SizeSpec::ToBb { value: 2.2 }, SizeSpec::ToBb { value: 2.5 }],
            isolate_sizes: Some(vec![
                SizeSpec::ToBb { value: 3.0 },
                SizeSpec::ToBb { value: 3.5 },
            ]),
            raise_sizes: vec![
                SizeSpec::PreviousBetMultiple { factor: 2.5 },
                SizeSpec::PreviousBetMultiple { factor: 3.0 },
            ],
            max_aggressive_actions: 4,
            include_allin: true,
            allin_threshold: None,
            reraise_jam_above_actor_starting_stack: None,
            max_betting_players: None,
        },
        Case::Cash => StreetBettingConfig {
            bet_sizes: vec![SizeSpec::ToBb { value: 2.5 }],
            isolate_sizes: Some(vec![SizeSpec::ToBb { value: 3.5 }]),
            raise_sizes: vec![SizeSpec::PreviousBetMultiple { factor: 3.0 }],
            max_aggressive_actions: 4,
            include_allin: true,
            allin_threshold: None,
            reraise_jam_above_actor_starting_stack: None,
            max_betting_players: None,
        },
    };
    (true, preflop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiway::config::StackRatio;
    use multiway::{Action, BettingState, MwChips};

    fn benchmark_state(case: Case, stack_bb: f64) -> (BettingState, BettingConfig) {
        let config = build_config(Options {
            profile: Profile::Benchmark,
            case,
            seats: 6,
            stack_bb,
            postflop: PostflopMode::OneSize,
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
            node_limit: None,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        });
        let validated = config.validated().unwrap();
        let betting = validated.betting.clone();
        (BettingState::new(&validated).unwrap(), betting)
    }

    fn benchmark_game_fingerprint(case: Case, seats: usize, stack_bb: f64) -> String {
        let options = Options {
            profile: Profile::Benchmark,
            case,
            seats,
            stack_bb,
            postflop: PostflopMode::OneSize,
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
            node_limit: None,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        };
        let config = build_config(options);
        let (utility, rake) = economics(options);
        let game =
            HoldemGame::new(&config, &utility, &rake, PreflightAbstraction::default()).unwrap();
        blake3::Hash::from_bytes(game.game_fingerprint())
            .to_hex()
            .to_string()
    }

    fn actions(state: &BettingState, betting: &BettingConfig) -> Vec<Action> {
        state.legal_actions(betting).unwrap()
    }

    fn has_call(state: &BettingState, betting: &BettingConfig) -> bool {
        actions(state, betting)
            .iter()
            .any(|action| matches!(action, Action::Call { .. }))
    }

    fn has_raise_to(state: &BettingState, betting: &BettingConfig, to: u64, all_in: bool) -> bool {
        actions(state, betting).iter().any(|action| {
            matches!(
                action,
                Action::RaiseTo {
                    to: MwChips(target),
                    all_in: action_all_in,
                    ..
                } if *target == to && *action_all_in == all_in
            )
        })
    }

    fn has_allin_raise(state: &BettingState, betting: &BettingConfig) -> bool {
        actions(state, betting)
            .iter()
            .any(|action| matches!(action, Action::RaiseTo { all_in: true, .. }))
    }

    fn normal_aggressive_targets(state: &BettingState, betting: &BettingConfig) -> Vec<u64> {
        actions(state, betting)
            .into_iter()
            .filter_map(|action| match action {
                Action::BetTo {
                    to, all_in: false, ..
                }
                | Action::RaiseTo {
                    to, all_in: false, ..
                } => Some(to.raw()),
                _ => None,
            })
            .collect()
    }

    fn allin_aggressive_count(state: &BettingState, betting: &BettingConfig) -> usize {
        actions(state, betting)
            .iter()
            .filter(|action| {
                matches!(
                    action,
                    Action::BetTo { all_in: true, .. } | Action::RaiseTo { all_in: true, .. }
                )
            })
            .count()
    }

    fn apply_matching(
        state: &mut BettingState,
        betting: &BettingConfig,
        predicate: impl Fn(&Action) -> bool,
    ) {
        let action = actions(state, betting)
            .into_iter()
            .find(predicate)
            .expect("expected legal action");
        state.apply(action, betting).unwrap();
    }

    fn apply_fold(state: &mut BettingState, betting: &BettingConfig) {
        apply_matching(state, betting, |action| matches!(action, Action::Fold));
    }

    fn apply_call(state: &mut BettingState, betting: &BettingConfig) {
        apply_matching(state, betting, |action| {
            matches!(action, Action::Call { .. })
        });
    }

    fn apply_raise_to(state: &mut BettingState, betting: &BettingConfig, to: u64) {
        apply_matching(state, betting, |action| {
            matches!(
                action,
                Action::RaiseTo {
                    to: MwChips(target),
                    all_in: false,
                    ..
                } if *target == to
            )
        });
    }

    #[test]
    fn target_stack_ranges_are_enforced() {
        assert!(Case::Tournament.validate_stack_bb(0.0).is_err());
        assert_eq!(Case::Tournament.validate_stack_bb(50.0).unwrap(), 50.0);
        assert!(Case::Tournament.validate_stack_bb(50.001).is_err());

        assert!(Case::Cash.validate_stack_bb(99.999).is_err());
        assert_eq!(Case::Cash.validate_stack_bb(100.0).unwrap(), 100.0);
        assert_eq!(Case::Cash.validate_stack_bb(800.0).unwrap(), 800.0);
        assert!(Case::Cash.validate_stack_bb(800.001).is_err());
    }

    #[test]
    fn bucket_flags_default_to_256_and_accept_the_full_u16_range() {
        let defaults = parse_options_from(
            ["--case", "cash", "--seats", "6"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(defaults.flop_buckets, DEFAULT_POSTFLOP_BUCKETS);
        assert_eq!(defaults.turn_buckets, DEFAULT_POSTFLOP_BUCKETS);
        assert_eq!(defaults.river_buckets, DEFAULT_POSTFLOP_BUCKETS);

        let custom = parse_options_from(
            [
                "--case",
                "tournament",
                "--seats",
                "9",
                "--flop-buckets",
                "1",
                "--turn-buckets",
                "128",
                "--river-buckets",
                "65535",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(custom.flop_buckets, 1);
        assert_eq!(custom.turn_buckets, 128);
        assert_eq!(custom.river_buckets, u16::MAX);

        for invalid in ["0", "65536", "not-a-number"] {
            assert!(
                parse_options_from(
                    ["--case", "cash", "--seats", "6", "--flop-buckets", invalid,]
                        .into_iter()
                        .map(str::to_owned),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn configured_bucket_counts_reach_config_and_preflight_abstraction() {
        let options = parse_options_from(
            [
                "--case",
                "cash",
                "--seats",
                "6",
                "--flop-buckets",
                "64",
                "--turn-buckets",
                "128",
                "--river-buckets",
                "512",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let config = build_config(options);
        assert_eq!(config.abstraction.flop_buckets, 64);
        assert_eq!(config.abstraction.turn_buckets, 128);
        assert_eq!(config.abstraction.river_buckets, 512);

        let abstraction = PreflightAbstraction {
            flop_buckets: options.flop_buckets,
            turn_buckets: options.turn_buckets,
            river_buckets: options.river_buckets,
        };
        assert_eq!(abstraction.num_buckets(Street::Preflop, 5), 169);
        assert_eq!(abstraction.num_buckets(Street::Flop, 5), 64);
        assert_eq!(abstraction.num_buckets(Street::Turn, 3), 128);
        assert_eq!(abstraction.num_buckets(Street::River, 1), 512);
    }

    #[test]
    fn default_bucket_fingerprint_and_tree_betting_contract_remain_stable() {
        assert_eq!(PreflightAbstraction::default().fingerprint(), [0xa7; 32]);
        assert_ne!(
            PreflightAbstraction {
                flop_buckets: 64,
                ..PreflightAbstraction::default()
            }
            .fingerprint(),
            [0xa7; 32]
        );

        let defaults = parse_options_from(
            ["--case", "cash", "--seats", "6"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let config = build_config(defaults);
        assert_eq!(config.abstraction.flop_buckets, 256);
        assert_eq!(config.abstraction.turn_buckets, 256);
        assert_eq!(config.abstraction.river_buckets, 256);
        assert_eq!(
            serde_json::to_value(&config.betting).unwrap(),
            serde_json::to_value(benchmark_betting(Case::Cash)).unwrap()
        );
        assert_eq!(
            tree_contract_fingerprint(&benchmark_betting(Case::Tournament)).unwrap(),
            "97e3c3ebf3da73e7f6e218bffd2b43399f87ab3a207b099f90de63058b8db604"
        );
        assert_eq!(
            tree_contract_fingerprint(&benchmark_betting(Case::Cash)).unwrap(),
            "bb50096add4d3a554dec23b3eaaba35a15c26e121eaa77c4c5b0d402b1a6b821"
        );
    }

    #[test]
    fn benchmark_games_reproduce_the_fixed_envelope_fingerprints() {
        for (case, seats, stack_bb, expected) in [
            (
                Case::Tournament,
                6,
                5.0,
                "02f0179b06ec036f79c6b0141e67a65bfb9a71f73557203db210516bb2bba63c",
            ),
            (
                Case::Tournament,
                6,
                50.0,
                "a31e08f41b36e46fcf340e432d27209dea872bcfa21e3104ee6037255e0af512",
            ),
            (
                Case::Tournament,
                8,
                20.0,
                "c357fb191000573a575708cfee241722ab201c8f1e584b8783ac299da4258e9f",
            ),
            (
                Case::Tournament,
                9,
                5.0,
                "0908aa79db3d1149e055d3a25bb1e6ab25fd5e7fb956cbe7cb06e4e8ab5c8b5f",
            ),
            (
                Case::Tournament,
                9,
                50.0,
                "39e87301e3169b897cfb59b9be452be7360ba13c574ad8a5d0a03bb8013c2282",
            ),
            (
                Case::Cash,
                6,
                100.0,
                "c35365b8e6ab8df1bbacdd9a6a862c527205bc048c58fa7db8b05f81bf3bc55b",
            ),
            (
                Case::Cash,
                6,
                800.0,
                "9d88e6644bccff8e5bde62ec9c8a480b0b344b60ac2ffeaba7140e201dfd3ce5",
            ),
            (
                Case::Cash,
                8,
                400.0,
                "b40006617d4fc8c8129699ffeba92cbe71306975127fbcfa76264a4a9ba1e843",
            ),
            (
                Case::Cash,
                9,
                100.0,
                "488aa4a6817cfb4d918ae033a5de1432f5484c1117afbad044bccd4cb002cbfa",
            ),
            (
                Case::Cash,
                9,
                800.0,
                "e28ba6bd1205332cbb98291772bf3d8b47925264bb89d384ad81e98ddb92cb20",
            ),
        ] {
            assert_eq!(
                benchmark_game_fingerprint(case, seats, stack_bb),
                expected,
                "{} {seats}-max/{stack_bb}bb",
                case.name()
            );
        }
    }

    #[test]
    fn one_size_config_keeps_postflop_betting_live() {
        let config = build_config(Options {
            profile: Profile::Benchmark,
            case: Case::Tournament,
            seats: 6,
            stack_bb: 10.0,
            postflop: PostflopMode::OneSize,
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
            node_limit: None,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        });

        assert!(config.seats.iter().all(|seat| seat.stack_bb == 10.0));
        assert!(!config.betting.allow_limp);
        assert_eq!(config.betting.preflop.max_aggressive_actions, 6);
        for street in [
            &config.betting.flop,
            &config.betting.turn,
            &config.betting.river,
        ] {
            assert_eq!(
                street.bet_sizes,
                vec![SizeSpec::PotAfterCall { fraction: 0.5 }]
            );
            assert_eq!(street.max_aggressive_actions, 4);
            assert!(street.include_allin);
            assert_eq!(street.max_betting_players, None);
        }
        assert!(config.betting.rules.iter().any(|rule| {
            rule.street == RuleStreet::Postflop
                && rule.condition == "aggressions == 0"
                && rule.sizes == vec![SizeSpec::PotAfterCall { fraction: 0.5 }, SizeSpec::AllIn]
        }));
        assert!(config.betting.rules.iter().any(|rule| {
            rule.street == RuleStreet::Postflop
                && rule.condition == "aggressions >= 1"
                && rule.sizes
                    == vec![
                        SizeSpec::PreviousBetMultiple { factor: 2.5 },
                        SizeSpec::AllIn,
                    ]
        }));
    }

    #[test]
    fn live_flop_turn_and_river_menus_keep_one_size_raise_allin_and_cap_four() {
        let (mut state, betting) = benchmark_state(Case::Cash, 100.0);

        // Fold to BTN, open 2.5bb, SB folds, BB calls. The resulting heads-up
        // pot is 5.5bb and BB acts first on every postflop street.
        for _ in 0..3 {
            apply_fold(&mut state, &betting);
        }
        apply_raise_to(&mut state, &betting, 2_500);
        apply_fold(&mut state, &betting);
        apply_call(&mut state, &betting);
        assert_eq!(state.street, Street::Flop);

        for street in [Street::Flop, Street::Turn, Street::River] {
            assert_eq!(state.street, street);
            assert_eq!(normal_aggressive_targets(&state, &betting), vec![2_750]);
            assert_eq!(allin_aggressive_count(&state, &betting), 1);

            let mut raised = state.clone();
            apply_matching(&mut raised, &betting, |action| {
                matches!(
                    action,
                    Action::BetTo {
                        to: MwChips(2_750),
                        all_in: false,
                        ..
                    }
                )
            });
            assert_eq!(normal_aggressive_targets(&raised, &betting), vec![6_875]);
            assert_eq!(allin_aggressive_count(&raised, &betting), 1);

            if street == Street::Flop {
                let mut capped = state.clone();
                for _ in 0..4 {
                    let target = normal_aggressive_targets(&capped, &betting)
                        .into_iter()
                        .min()
                        .expect("100bb flop ladder must retain four normal aggressions");
                    apply_matching(&mut capped, &betting, |action| {
                        matches!(
                            action,
                            Action::BetTo {
                                to,
                                all_in: false,
                                ..
                            } | Action::RaiseTo {
                                to,
                                all_in: false,
                                ..
                            } if to.raw() == target
                        )
                    });
                }
                assert_eq!(capped.aggressive_actions, 4);
                assert!(
                    actions(&capped, &betting)
                        .iter()
                        .all(|action| !action.is_aggressive())
                );
            }

            apply_matching(&mut state, &betting, |action| {
                matches!(action, Action::Check)
            });
            apply_matching(&mut state, &betting, |action| {
                matches!(action, Action::Check)
            });
        }
    }

    #[test]
    fn benchmark_profiles_encode_the_requested_preflop_contract() {
        let cash = build_config(Options {
            profile: Profile::Benchmark,
            case: Case::Cash,
            seats: 6,
            stack_bb: 100.0,
            postflop: PostflopMode::OneSize,
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
            node_limit: None,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        });
        assert!(!cash.betting.allow_limp);
        assert_eq!(cash.betting.preflop.max_aggressive_actions, 6);
        assert_eq!(
            cash.betting.preflop.reraise_jam_above_actor_starting_stack,
            Some(StackRatio {
                numerator: 1,
                denominator: 3,
            })
        );
        assert!(cash.betting.rules.iter().any(|rule| {
            rule.street == RuleStreet::Preflop
                && rule.condition == "unopened"
                && rule.sizes == vec![SizeSpec::ToBb { value: 2.5 }]
        }));
        assert!(cash.betting.rules.iter().any(|rule| {
            rule.condition == "aggressions >= 1 && in_position_to_last_aggressor"
                && rule.sizes
                    == vec![
                        SizeSpec::PreviousBetMultiple { factor: 3.0 },
                        SizeSpec::AllIn,
                    ]
        }));
        assert!(cash.betting.rules.iter().any(|rule| {
            rule.condition == "aggressions >= 1 && !in_position_to_last_aggressor"
                && rule.sizes
                    == vec![
                        SizeSpec::PreviousBetMultiple { factor: 5.0 },
                        SizeSpec::AllIn,
                    ]
        }));

        let tournament = build_config(Options {
            profile: Profile::Benchmark,
            case: Case::Tournament,
            seats: 9,
            stack_bb: 50.0,
            postflop: PostflopMode::OneSize,
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
            node_limit: None,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
        });
        assert!(!tournament.betting.allow_limp);
        assert_eq!(tournament.betting.preflop.max_aggressive_actions, 6);
        assert!(tournament.betting.rules.iter().any(|rule| {
            rule.street == RuleStreet::Preflop
                && rule.condition == "unopened"
                && rule.sizes == vec![SizeSpec::ToBb { value: 2.0 }, SizeSpec::AllIn]
        }));
        assert!(tournament.betting.rules.iter().any(|rule| {
            rule.condition == "aggressions == 1"
                && rule.sizes
                    == vec![
                        SizeSpec::PreviousBetMultiple { factor: 2.5 },
                        SizeSpec::AllIn,
                    ]
        }));
        assert!(tournament.betting.rules.iter().any(|rule| {
            rule.condition == "aggressions >= 2"
                && rule.sizes
                    == vec![
                        SizeSpec::PreviousBetMultiple { factor: 2.0 },
                        SizeSpec::AllIn,
                    ]
        }));
    }

    #[test]
    fn cash_root_and_ip_oop_reraise_menus_match_benchmark_contract() {
        let (mut versus_utg, betting) = benchmark_state(Case::Cash, 100.0);
        let root = actions(&versus_utg, &betting);
        assert!(
            !root
                .iter()
                .any(|action| matches!(action, Action::Call { .. }))
        );
        assert!(has_raise_to(&versus_utg, &betting, 2_500, false));
        assert!(
            !root
                .iter()
                .any(|action| { matches!(action, Action::RaiseTo { all_in: true, .. }) })
        );
        assert_eq!(
            root.iter()
                .filter(|action| matches!(action, Action::RaiseTo { .. }))
                .count(),
            1
        );

        // UTG opens, HJ folds, and CO is postflop-IP to the last aggressor.
        apply_raise_to(&mut versus_utg, &betting, 2_500);
        apply_fold(&mut versus_utg, &betting);
        assert_eq!(versus_utg.to_act, Some(SeatId(2)));
        assert!(has_raise_to(&versus_utg, &betting, 7_500, false));
        assert!(!has_raise_to(&versus_utg, &betting, 12_500, false));

        // Fold to BTN, open, then both blinds are OOP to BTN and use 5x.
        let (mut versus_btn, betting) = benchmark_state(Case::Cash, 100.0);
        for _ in 0..3 {
            apply_fold(&mut versus_btn, &betting);
        }
        assert_eq!(versus_btn.to_act, Some(SeatId(3)));
        apply_raise_to(&mut versus_btn, &betting, 2_500);
        assert_eq!(versus_btn.to_act, Some(SeatId(4)));
        assert!(has_raise_to(&versus_btn, &betting, 12_500, false));
        let mut bb = versus_btn.clone();
        apply_fold(&mut bb, &betting);
        assert_eq!(bb.to_act, Some(SeatId(5)));
        assert!(has_raise_to(&bb, &betting, 12_500, false));
    }

    #[test]
    fn cash_open_call_and_participant_rules_are_applied_to_live_states() {
        // UTG opens. CO cannot cold-call, while BTN/SB/BB can.
        let (mut state, betting) = benchmark_state(Case::Cash, 100.0);
        apply_raise_to(&mut state, &betting, 2_500);
        apply_fold(&mut state, &betting);
        assert_eq!(state.to_act, Some(SeatId(2)));
        assert!(!has_call(&state, &betting));
        apply_fold(&mut state, &betting);
        assert_eq!(state.to_act, Some(SeatId(3)));
        assert!(has_call(&state, &betting));
        let mut sb = state.clone();
        apply_fold(&mut sb, &betting);
        assert_eq!(sb.to_act, Some(SeatId(4)));
        assert!(has_call(&sb, &betting));
        let mut bb = sb.clone();
        apply_fold(&mut bb, &betting);
        assert_eq!(bb.to_act, Some(SeatId(5)));
        assert!(has_call(&bb, &betting));

        // A 3bet prevents a new CO entrant from calling; after the remaining
        // new entrants fold, the participating opener may still call.
        let (mut reopened, betting) = benchmark_state(Case::Cash, 100.0);
        apply_raise_to(&mut reopened, &betting, 2_500);
        apply_raise_to(&mut reopened, &betting, 7_500);
        assert_eq!(reopened.to_act, Some(SeatId(2)));
        assert!(!has_call(&reopened, &betting));
        for _ in 0..4 {
            apply_fold(&mut reopened, &betting);
        }
        assert_eq!(reopened.to_act, Some(SeatId(0)));
        assert!(has_call(&reopened, &betting));

        // A prior open caller is also a participant and retains its call
        // after BB squeezes.
        let (mut prior_caller, betting) = benchmark_state(Case::Cash, 100.0);
        apply_raise_to(&mut prior_caller, &betting, 2_500);
        apply_fold(&mut prior_caller, &betting);
        apply_fold(&mut prior_caller, &betting);
        apply_call(&mut prior_caller, &betting);
        apply_fold(&mut prior_caller, &betting);
        apply_raise_to(&mut prior_caller, &betting, 12_500);
        assert_eq!(prior_caller.to_act, Some(SeatId(0)));
        apply_call(&mut prior_caller, &betting);
        assert_eq!(prior_caller.to_act, Some(SeatId(3)));
        assert!(has_call(&prior_caller, &betting));
    }

    #[test]
    fn tournament_raise_ladder_and_cold_call_cap_match_live_states() {
        let (mut ladder, betting) = benchmark_state(Case::Tournament, 50.0);
        assert!(has_raise_to(&ladder, &betting, 2_000, false));
        assert!(has_allin_raise(&ladder, &betting));
        apply_raise_to(&mut ladder, &betting, 2_000);
        assert!(has_raise_to(&ladder, &betting, 5_000, false));
        assert!(has_allin_raise(&ladder, &betting));
        apply_raise_to(&mut ladder, &betting, 5_000);
        assert!(has_raise_to(&ladder, &betting, 10_000, false));
        assert!(has_allin_raise(&ladder, &betting));

        let (mut calls, betting) = benchmark_state(Case::Tournament, 50.0);
        apply_raise_to(&mut calls, &betting, 2_000);
        apply_call(&mut calls, &betting); // HJ, first non-BB cold caller.
        apply_call(&mut calls, &betting); // CO, second non-BB cold caller.
        assert_eq!(calls.to_act, Some(SeatId(3)));
        assert!(!has_call(&calls, &betting)); // BTN would be third.
        apply_fold(&mut calls, &betting);
        assert_eq!(calls.to_act, Some(SeatId(4)));
        assert!(!has_call(&calls, &betting)); // SB would also be third.
        apply_fold(&mut calls, &betting);
        assert_eq!(calls.to_act, Some(SeatId(5)));
        assert!(has_call(&calls, &betting)); // BB defense is outside cap.
    }

    #[test]
    fn tournament_threebet_blocks_new_callers_but_keeps_participant_calls() {
        let (mut state, betting) = benchmark_state(Case::Tournament, 50.0);
        apply_raise_to(&mut state, &betting, 2_000); // UTG opens.
        apply_call(&mut state, &betting); // HJ becomes an open-call participant.
        apply_raise_to(&mut state, &betting, 5_000); // CO 3bets.

        for expected_actor in [SeatId(3), SeatId(4), SeatId(5)] {
            assert_eq!(state.to_act, Some(expected_actor));
            assert!(!has_call(&state, &betting));
            apply_fold(&mut state, &betting);
        }

        assert_eq!(state.to_act, Some(SeatId(0)));
        assert!(has_call(&state, &betting)); // The opener may continue.
        apply_call(&mut state, &betting);
        assert_eq!(state.to_act, Some(SeatId(1)));
        assert!(has_call(&state, &betting)); // The prior open caller may continue.
    }

    #[test]
    fn six_action_cap_does_not_cut_off_stack_reaching_raise_ladders() {
        for (case, depths) in [
            (
                Case::Tournament,
                &[5.0, 10.0, 15.0, 20.0, 30.0, 40.0, 50.0][..],
            ),
            (Case::Cash, &[100.0, 200.0, 400.0, 800.0][..]),
        ] {
            for &stack_bb in depths {
                let (mut state, betting) = benchmark_state(case, stack_bb);
                let mut reached_allin = false;
                for _ in 0..6 {
                    let menu = actions(&state, &betting);
                    let smallest_normal = menu
                        .iter()
                        .filter(|action| action.is_aggressive())
                        .filter(|action| {
                            matches!(
                                action,
                                Action::BetTo { all_in: false, .. }
                                    | Action::RaiseTo { all_in: false, .. }
                            )
                        })
                        .min_by_key(|action| action.amount())
                        .cloned();
                    if let Some(action) = smallest_normal {
                        state.apply(action, &betting).unwrap();
                        continue;
                    }
                    let allin = menu
                        .into_iter()
                        .find(|action| {
                            matches!(
                                action,
                                Action::BetTo { all_in: true, .. }
                                    | Action::RaiseTo { all_in: true, .. }
                            )
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "{} {stack_bb}bb lost every aggressive action before reaching \
                                 the stack at aggression {}",
                                case.name(),
                                state.aggressive_actions + 1,
                            )
                        });
                    state.apply(allin, &betting).unwrap();
                    reached_allin = true;
                    break;
                }
                assert!(
                    reached_allin,
                    "{} {stack_bb}bb still had only normal raises after the sixth aggression",
                    case.name(),
                );
                assert!(state.aggressive_actions <= 6);
            }
        }
    }

    #[test]
    fn legacy_rich_profile_preserves_old_preflop_and_checkdown_shapes() {
        let config = build_config(Options {
            profile: Profile::LegacyRich,
            case: Case::Tournament,
            seats: 6,
            stack_bb: 50.0,
            postflop: PostflopMode::Checkdown,
            flop_buckets: DEFAULT_POSTFLOP_BUCKETS,
            turn_buckets: DEFAULT_POSTFLOP_BUCKETS,
            river_buckets: DEFAULT_POSTFLOP_BUCKETS,
            node_limit: Some(50_000_000),
            max_memory_bytes: u64::MAX,
        });
        assert!(config.betting.allow_limp);
        assert_eq!(config.betting.preflop.max_aggressive_actions, 4);
        assert_eq!(config.betting.flop.max_aggressive_actions, 0);
        assert_eq!(config.betting.flop.max_betting_players, Some(1));
    }
}
