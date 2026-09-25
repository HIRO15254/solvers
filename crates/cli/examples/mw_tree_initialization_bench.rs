//! Research timing of the existing public-tree initialization phases.

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, ValueEnum};
use cli::config::{GameSection, RakeSection, UtilitySection};
use multiway::abstraction::{BucketContext, BucketId, MultiwayAbstraction};
use multiway::config::{AbstractionConfig, RakeConfig, UtilityConfig};
use multiway::tree::{self, Child, DenseArena, PublicTree, TreePreflight};
use multiway::{ExternalSamplingGame, HoldemGame, Street};
use serde::Serialize;

#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Serial,
    Parallel,
}

#[derive(Parser)]
#[command(name = "mw_tree_initialization_bench")]
struct Args {
    /// Cash Multiway v1 config, including its actual public tree and bucket counts.
    #[arg(long)]
    config: PathBuf,
    /// Existing tree materializer; no production default is changed.
    #[arg(long, value_enum, default_value = "serial")]
    mode: Mode,
    /// Threads in a private Rayon pool (1 through 64).
    #[arg(long, default_value_t = 1)]
    threads: usize,
    /// Policy arena payload limit. Public-tree and temporary merge memory are additional.
    #[arg(long, default_value_t = 1_073_741_824)]
    arena_limit_bytes: u64,
    /// Decision-node limit, checked by the core non-retaining preflight first.
    #[arg(long, default_value_t = 2_000_000)]
    max_nodes: usize,
    /// Structural depth limit applied to preflight and materialization.
    #[arg(long, default_value_t = 512)]
    max_depth: u32,
    /// Immutable source revision/package identifier for the measured executable.
    #[arg(long)]
    source_revision: String,
}

fn validate_args(args: &Args) -> Result<()> {
    ensure!(
        (1..=64).contains(&args.threads),
        "--threads must be in 1..=64"
    );
    ensure!(
        args.arena_limit_bytes > 0,
        "--arena-limit-bytes must be positive"
    );
    ensure!(
        args.max_nodes > 0 && args.max_nodes <= tree::MAX_TREE_NODES,
        "--max-nodes must fit the core node representation"
    );
    ensure!(args.max_depth > 0, "--max-depth must be positive");
    ensure!(
        !args.source_revision.trim().is_empty(),
        "--source-revision must not be empty"
    );
    Ok(())
}

struct CensusAbstraction(AbstractionConfig);

impl MultiwayAbstraction for CensusAbstraction {
    fn num_buckets(&self, street: Street, active_opponents: u8) -> u32 {
        if street == Street::Preflop {
            return cards::NUM_CLASSES as u32;
        }
        let profile = self.0.buckets_for(active_opponents);
        match street {
            Street::Preflop => unreachable!(),
            Street::Flop => u32::from(profile.map_or(self.0.flop_buckets, |p| p.flop_buckets)),
            Street::Turn => u32::from(profile.map_or(self.0.turn_buckets, |p| p.turn_buckets)),
            Street::River => u32::from(profile.map_or(self.0.river_buckets, |p| p.river_buckets)),
        }
    }

    fn bucket(&self, _context: BucketContext<'_>) -> BucketId {
        unreachable!("initialization benchmark never assigns physical hands")
    }

    fn fingerprint(&self) -> [u8; 32] {
        // This census backend is never a candidate policy/card abstraction.
        // Its schedule is emitted separately; no production abstraction hash
        // or solver identity is claimed for it.
        [0; 32]
    }
}

fn public_game(
    raw: &str,
    path: &Path,
) -> Result<(HoldemGame<CensusAbstraction>, AbstractionConfig)> {
    cli::multiway_v1::validate_production_contract_at(raw, path)?;
    let lowered = cli::multiway_v1::parse_and_lower_at(raw, path)?;
    let GameSection::PreflopMultiway(config) = lowered.game else {
        bail!("expected a Multiway Preflop v1 config");
    };
    ensure!(
        matches!(lowered.utility, UtilitySection::ChipEv),
        "initialization benchmark supports cash ChipEV only; ICM preparation is a separate phase"
    );
    // Production v1 cash lowering emits None or Generic rake. Preserve it so
    // game_fingerprint is the real game's identity, including economics.
    let rake = match lowered.rake {
        RakeSection::None => RakeConfig::None,
        RakeSection::Generic {
            rate,
            cap,
            when,
            allocation,
            rounding,
            ..
        } => RakeConfig::Generic {
            rate,
            cap_bb: cap,
            when,
            allocation,
            rounding,
        },
        _ => bail!("unexpected legacy rake after production v1 lowering"),
    };
    let abstraction = config.abstraction.clone();
    let game = HoldemGame::new(
        &config,
        &UtilityConfig::ChipEv,
        &rake,
        CensusAbstraction(abstraction.clone()),
    )?;
    Ok((game, abstraction))
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Timings {
    pool_build_secs: Option<f64>,
    preflight_secs: Option<f64>,
    materialize_secs: Option<f64>,
    build_arena_secs: Option<f64>,
    commit_pages_secs: Option<f64>,
    digest_secs: Option<f64>,
    release_secs: Option<f64>,
}

fn timed<T>(seconds: &mut Option<f64>, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let started = Instant::now();
    let result = work();
    *seconds = Some(started.elapsed().as_secs_f64());
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    nodes: usize,
    terminal_edges: u64,
    columns: u64,
    slots: u64,
    arena_bytes: u64,
}

impl From<TreePreflight> for Counts {
    fn from(value: TreePreflight) -> Self {
        Self {
            nodes: value.node_count,
            terminal_edges: value.terminal_edges,
            columns: value.total_columns,
            slots: value.total_slots,
            arena_bytes: value.estimated_arena_bytes,
        }
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    complete: bool,
    failed_phase: Option<&'static str>,
    error: Option<String>,
    timings: Timings,
    counts: Option<Counts>,
    tree_blake3: Option<String>,
    arena_layout_blake3: Option<String>,
    pages_committed: bool,
}

fn measure_game<G: ExternalSamplingGame>(game: &G, args: &Args) -> Measurement
where
    G::State: Send,
{
    let mut output = Measurement::default();
    let mut phase = "pool-build";
    let result = (|| -> Result<()> {
        let pool = timed(&mut output.timings.pool_build_secs, || {
            Ok(rayon::ThreadPoolBuilder::new()
                .num_threads(args.threads)
                .build()?)
        })?;
        phase = "preflight";
        let preflight = timed(&mut output.timings.preflight_secs, || {
            Ok(tree::preflight_arena_with_limits_and_depth(
                game,
                args.max_nodes,
                args.arena_limit_bytes,
                args.max_depth,
            )?)
        })?;
        output.counts = Some(preflight.into());
        phase = "materialize";
        let tree = timed(&mut output.timings.materialize_secs, || {
            Ok(pool.install(|| match args.mode {
                Mode::Serial => {
                    tree::enumerate_tree_with_limits(game, args.max_nodes, args.max_depth)
                }
                Mode::Parallel => {
                    tree::enumerate_tree_with_limits_parallel(game, args.max_nodes, args.max_depth)
                }
            })?)
        })?;
        phase = "build-arena";
        let mut arena = timed(&mut output.timings.build_arena_secs, || {
            Ok(tree::build_arena(game, &tree, args.arena_limit_bytes)?)
        })?;
        ensure!(
            tree.nodes.len() == preflight.node_count
                && arena.node_count() == preflight.node_count
                && arena.total_columns() == preflight.total_columns
                && arena.total_slots() == preflight.total_slots
                && arena.estimated_bytes() == preflight.estimated_arena_bytes,
            "preflight and retained layout differ"
        );
        phase = "commit-pages";
        timed(&mut output.timings.commit_pages_secs, || {
            arena.commit_pages();
            Ok(())
        })?;
        output.pages_committed = arena.pages_committed();
        phase = "digest";
        let (tree_digest, arena_digest) = timed(&mut output.timings.digest_secs, || {
            Ok((tree_digest(&tree)?, arena_layout_digest(&arena)?))
        })?;
        output.tree_blake3 = Some(tree_digest);
        output.arena_layout_blake3 = Some(arena_digest);
        phase = "release";
        timed(&mut output.timings.release_secs, || {
            drop(arena);
            drop(tree);
            drop(pool);
            Ok(())
        })?;
        Ok(())
    })();
    match result {
        Ok(()) => output.complete = true,
        Err(error) => {
            output.failed_phase = Some(phase);
            output.error = Some(format!("{error:#}"));
        }
    }
    output
}

fn hash_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn tree_digest(tree: &PublicTree) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"solvers.multiway.initialization-tree.v1");
    hash_u64(&mut hash, tree.nodes.len() as u64);
    hash_u64(&mut hash, tree.by_history.len() as u64);
    ensure!(
        tree.nodes.len() == tree.by_history.len(),
        "history map cardinality differs from tree"
    );
    for (index, node) in tree.nodes.iter().enumerate() {
        hash.update(&node.history.0);
        match node.parent {
            Some(parent) => {
                hash.update(&[1]);
                hash_u64(&mut hash, u64::from(parent));
            }
            None => {
                hash.update(&[0]);
            }
        }
        hash_u64(&mut hash, u64::from(node.parent_action_index));
        hash.update(&[
            node.actor,
            node.street.index() as u8,
            node.active_opponents,
            node.bucket_active_opponents,
        ]);
        hash_u64(&mut hash, node.action_labels.len() as u64);
        for label in &node.action_labels {
            hash_u64(&mut hash, label.len() as u64);
            hash.update(label.as_bytes());
        }
        hash_u64(&mut hash, node.children.len() as u64);
        for child in &node.children {
            match child {
                Child::Decision(id) => {
                    hash.update(&[1]);
                    hash_u64(&mut hash, u64::from(*id));
                }
                Child::Terminal => {
                    hash.update(&[0]);
                }
            }
        }
        let id = tree
            .by_history
            .get(&node.history)
            .context("missing tree history index")?;
        ensure!(
            *id as usize == index,
            "history index does not match preorder"
        );
        hash_u64(&mut hash, u64::from(*id));
    }
    Ok(hash.finalize().to_hex().to_string())
}

fn arena_layout_digest(arena: &DenseArena) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"solvers.multiway.initialization-arena-layout.v1");
    for value in [
        arena.node_count() as u64,
        arena.total_columns(),
        arena.total_slots(),
        arena.estimated_bytes(),
        arena.regrets.len() as u64,
        arena.strategy_sum.len() as u64,
        arena.touched_count(),
    ] {
        hash_u64(&mut hash, value);
    }
    hash.update(&[u8::from(arena.pages_committed())]);
    // The actual first-column range exposes each private column_base,
    // slot_base and num_actions element. Together with bucket_count and the
    // final totals these describe every bucket/column/slot mapping exactly.
    // No policy payload reads occur before the page-commit timing barrier.
    for index in 0..arena.node_count() {
        let node = u32::try_from(index)?;
        let buckets = arena.bucket_count_of(node);
        ensure!(buckets > 0, "zero buckets in arena layout");
        let first = arena.slot_range(node, 0)?;
        let last = arena.slot_range(node, buckets - 1)?;
        let first_column = arena.column_id(node, 0)?;
        let last_column = arena.column_id(node, buckets - 1)?;
        ensure!(
            arena.slot_range_for_column(first_column, first.len())? == first
                && arena.slot_range_for_column(last_column, first.len())? == last,
            "arena wire-column lookup differs from node layout"
        );
        for value in [
            u64::from(buckets),
            u64::from(first_column),
            first.start as u64,
            first.end as u64,
            u64::from(last_column),
            last.start as u64,
            last.end as u64,
        ] {
            hash_u64(&mut hash, value);
        }
    }
    Ok(hash.finalize().to_hex().to_string())
}

fn file_blake3(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().to_hex().to_string())
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;
    let raw = fs::read_to_string(&args.config)
        .with_context(|| format!("reading {}", args.config.display()))?;
    let started = Instant::now();
    let (game, abstraction) = public_game(&raw, &args.config)?;
    let config_and_game_secs = started.elapsed().as_secs_f64();
    let measurement = measure_game(&game, &args);
    let complete = measurement.complete;
    let report = serde_json::json!({
        "schemaVersion": "solvers.multiway-tree-initialization-bench/v1",
        "sourceRevision": args.source_revision,
        "executableBlake3": file_blake3(&std::env::current_exe()?)?,
        "config": args.config,
        "configBlake3": blake3::hash(raw.as_bytes()).to_hex().as_str(),
        "gameFingerprint": game.game_fingerprint().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "gameFingerprintIncludesAbstraction": false,
        "abstractionBackend": "census-counts-only",
        "structuralAbstractionConfig": abstraction,
        "mode": args.mode,
        "threads": args.threads,
        "maxNodes": args.max_nodes,
        "maxDepth": args.max_depth,
        "arenaLimitBytes": args.arena_limit_bytes,
        "configAndGameSecs": config_and_game_secs,
        "measurement": measurement,
        "interpretation": "research-only cash public-tree phases; no physical worlds, EHS tables, solve, checkpoint or solution; exact tree/arena-layout digests are separate from solver identities; policy-arena byte limit excludes public tree, allocator and temporary parallel merge storage; parallel prototype may retain up to twice the node slots plus bounded prefix and worker capacity; errors may rerun serial enumeration; phase times exclude digest/release unless named; process wall time and peak memory must be measured externally",
    });
    serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
    println!();
    ensure!(
        complete,
        "initialization phase failed; see JSON measurement"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke() -> (HoldemGame<CensusAbstraction>, Args) {
        let config = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/preflop_multiway_v1_3max_smoke.toml");
        let raw = fs::read_to_string(&config).unwrap();
        let (game, _) = public_game(&raw, &config).unwrap();
        let args = Args {
            config,
            mode: Mode::Serial,
            threads: 1,
            arena_limit_bytes: 64 * 1024 * 1024,
            max_nodes: 100_000,
            max_depth: 512,
            source_revision: "test".into(),
        };
        (game, args)
    }

    #[test]
    fn initialization_modes_match_full_tree_and_arena_layout() {
        let (game, mut args) = smoke();
        let serial = measure_game(&game, &args);
        args.mode = Mode::Parallel;
        args.threads = 2;
        let parallel = measure_game(&game, &args);
        assert!(serial.complete, "{:?}", serial.error);
        assert!(parallel.complete, "{:?}", parallel.error);
        assert_eq!(serial.tree_blake3, parallel.tree_blake3);
        assert_eq!(serial.arena_layout_blake3, parallel.arena_layout_blake3);
        assert!(serial.pages_committed && parallel.pages_committed);
    }

    #[test]
    fn tree_digest_detects_labels_structure_and_bad_history_index() {
        let (game, args) = smoke();
        let tree = tree::enumerate_tree_with_limits(&game, args.max_nodes, args.max_depth).unwrap();
        let expected = tree_digest(&tree).unwrap();
        let mut changed = tree.clone();
        changed.nodes[0].action_labels[0].push('x');
        assert_ne!(expected, tree_digest(&changed).unwrap());
        let mut changed = tree.clone();
        changed.nodes[0].children[0] = match changed.nodes[0].children[0] {
            Child::Terminal => Child::Decision(0),
            Child::Decision(_) => Child::Terminal,
        };
        assert_ne!(expected, tree_digest(&changed).unwrap());
        let mut changed = tree;
        changed.by_history.insert(multiway::HistoryKey::ROOT, 1);
        assert!(tree_digest(&changed).is_err());
    }

    #[test]
    fn arena_digest_binds_layout_and_commit_flag() {
        let (game, args) = smoke();
        let tree = tree::enumerate_tree_with_limits(&game, args.max_nodes, args.max_depth).unwrap();
        let mut arena = tree::build_arena(&game, &tree, args.arena_limit_bytes).unwrap();
        let uncommitted = arena_layout_digest(&arena).unwrap();
        arena.commit_pages();
        assert_ne!(uncommitted, arena_layout_digest(&arena).unwrap());
        let mut changed = tree.clone();
        changed.nodes[0].action_labels.push("extra-action".into());
        let mut changed_arena = tree::build_arena(&game, &changed, args.arena_limit_bytes).unwrap();
        changed_arena.commit_pages();
        assert_ne!(
            arena_layout_digest(&arena).unwrap(),
            arena_layout_digest(&changed_arena).unwrap()
        );
    }

    #[test]
    fn preflight_failure_never_reports_completed_digests() {
        let (game, mut args) = smoke();
        args.arena_limit_bytes = 1;
        let result = measure_game(&game, &args);
        assert!(!result.complete);
        assert_eq!(result.failed_phase, Some("preflight"));
        assert!(result.timings.preflight_secs.is_some());
        assert!(result.timings.materialize_secs.is_none());
        assert!(result.tree_blake3.is_none() && result.arena_layout_blake3.is_none());
        assert!(!result.pages_committed);
        args.threads = 0;
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn argument_defaults_are_bounded_and_cash_rake_identity_is_preserved() {
        let args =
            Args::try_parse_from(["bench", "--config", "unused", "--source-revision", "test"])
                .unwrap();
        validate_args(&args).unwrap();
        assert_eq!(args.threads, 1);
        assert_eq!(args.max_nodes, 2_000_000);
        assert_eq!(args.arena_limit_bytes, 1_073_741_824);
        assert!(
            Args::try_parse_from([
                "bench",
                "--config",
                "unused",
                "--source-revision",
                "test",
                "--mode",
                "unknown",
            ])
            .is_err()
        );
        let (unraked, args) = smoke();
        let mut raw = fs::read_to_string(&args.config).unwrap();
        raw.push_str("\n[economics]\nkind = \"cash\"\n[economics.rake]\nrate = 0.05\ncap_bb = 1.0\nwhen = \"flop_dealt\"\n");
        let (raked, _) = public_game(&raw, &args.config).unwrap();
        assert_ne!(unraked.game_fingerprint(), raked.game_fingerprint());
    }
}
