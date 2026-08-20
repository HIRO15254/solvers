//! `.sol` viewer-artifact CLI support: exporting a solved postflop run to a
//! compact quantized `.sol` file (`solve --sol`), and loading one back for
//! read-only strategy queries (`inspect --sol`) through the
//! [`StrategyProvider`] seam shared with a live in-process solve.
//!
//! This module also owns [`StrategyProvider`] and its two implementations
//! ([`LiveProvider`] for an in-process [`Solver`], [`SolProvider`] for a
//! loaded [`LoadedSol`]) rather than splitting them into `inspect.rs`:
//! everything river-subgame-shaped (reach reconstruction, the lazy
//! re-solve, the trunk<->subtree node-id map) belongs next to `LoadedSol`,
//! which already owns the rebuilt tree and the rake/utility models a
//! re-solve needs. `inspect.rs`'s `Repl` only ever sees `&mut dyn
//! StrategyProvider`, never either concrete implementation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use cards::{Card, PerPlayer, Player, Street};
use engine::{
    Dcfr, F32Storage, NodeId, NodeKind, Solver, Storage, pair_subtrees, parent_array, reach_at,
};
use formats::{
    SolMeta, SolPayload, StrategyBlock, StreetsStored, dequantize_probs, quantize_probs, read_sol,
    write_sol,
};
use game::{PayoffPipeline, RakeModel, UtilityModel};
use holdem::{
    PostflopConfig, PostflopEvaluator, PostflopGame, build_postflop_game, node_streets,
    river_entry_state, river_resolve_config,
};

use crate::config::{GameSection, SolveConfig};
use crate::postflop_setup;
use crate::solve::RunSummary;

/// `--sol-streets`: which streets get stored strategy blocks in a `.sol`
/// export. Mirrors `formats::StreetsStored` one-to-one; kept as a separate
/// type (rather than teaching `formats` about `clap`) so the codec crate
/// stays free of CLI-parsing dependencies.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum SolStreets {
    NoRivers,
    Full,
}

impl From<SolStreets> for StreetsStored {
    fn from(mode: SolStreets) -> Self {
        match mode {
            SolStreets::NoRivers => StreetsStored::NoRivers,
            SolStreets::Full => StreetsStored::Full,
        }
    }
}

/// Starting street for a postflop subgame, derived from its board length --
/// mirrors `holdem::build_postflop_game`'s own board-length dispatch (3 =
/// flop, 4 = turn, 5 = river) so callers never duplicate or drift from that
/// match.
pub(crate) fn start_street_from_board_len(len: usize) -> Street {
    match len {
        3 => Street::Flop,
        4 => Street::Turn,
        5 => Street::River,
        other => panic!("board must have 3 (flop), 4 (turn), or 5 (river) cards, got {other}"),
    }
}

// --- export (`solve --sol`) ------------------------------------------------

/// Everything `solve --sol` gathers up front (while the raw config text and
/// storage-backend choice are still in hand) to export a `.sol` viewer
/// artifact once the run completes. Threaded through as `Option<SolExportSpec>`
/// rather than a `RunHooks`-style callback: unlike the per-check-cadence
/// metrics/checkpoint autosave, this export happens exactly once, after the
/// convergence loop finishes, so there is nothing to hook into a cadence.
pub(crate) struct SolExportSpec {
    pub path: PathBuf,
    pub mode: SolStreets,
    /// Raw config file text, byte-for-byte -- becomes `SolPayload::config_toml`
    /// so the viewer can rebuild an identical tree later.
    pub config_toml: String,
    /// Informational only (`SolMeta::storage`): "f32" or "i16".
    pub storage_name: String,
}

/// Human-readable file size, KB below 1 MiB and MB above -- used only for
/// the `solve --sol` summary line, not round-tripped anywhere.
fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.2} MB", b / MB)
    } else {
        format!("{:.1} KB", b / 1024.0)
    }
}

/// Exports a `.sol` viewer artifact for a completed postflop solve.
///
/// `start_street` is the subgame's starting street (from the built config's
/// board length, via [`start_street_from_board_len`]); when it is
/// [`Street::River`] the whole config IS the river, so `NoRivers` mode would
/// store nothing at all -- this forces `Full` instead, printing a note only
/// when that actually overrides the caller's requested mode.
///
/// `summary` supplies the iteration count and exploitability from the just-
/// finished run; `solver` is queried directly (rather than trusting anything
/// cached in `summary`) for both players' expected value, since a raked
/// (general-sum) game's `ev[1]` is never just `-ev[0]`.
pub(crate) fn export_sol<S: Storage>(
    spec: &SolExportSpec,
    solver: &Solver<PostflopEvaluator, S>,
    start_street: Street,
    summary: &RunSummary,
) -> Result<()> {
    let tree = &solver.game().tree;
    let streets = node_streets(tree, start_street);

    let mode = if start_street == Street::River {
        if spec.mode != SolStreets::Full {
            println!(
                "note: river-start config, forcing --sol-streets=full \
                 (no-rivers would store nothing for a config with no streets before the river)"
            );
        }
        SolStreets::Full
    } else {
        spec.mode
    };

    let mut blocks = Vec::new();
    for id in 0..tree.nodes.len() as NodeId {
        let node = tree.node(id);
        if node.kind != NodeKind::Action {
            continue;
        }
        if mode == SolStreets::NoRivers && streets[id as usize] == Street::River {
            continue;
        }
        let avg = solver.average_strategy_at(id);
        blocks.push(StrategyBlock {
            sref: node.aux,
            probs: quantize_probs(&avg),
        });
    }
    // Ascending by construction (node ids walked in order and `aux` assigned
    // in build order), but sorted explicitly to make that guarantee robust
    // to any future change in how `aux` is assigned.
    blocks.sort_by_key(|b| b.sref);
    let block_count = blocks.len();

    let meta = SolMeta {
        iterations: summary.iterations,
        expl: [summary.expl_p0, summary.expl_p1],
        ev: [
            solver.expected_value(Player::P0),
            solver.expected_value(Player::P1),
        ],
        nash_conv: summary.nash_conv,
        storage: spec.storage_name.clone(),
        wall_secs: summary.wall.as_secs_f64(),
    };

    let payload = SolPayload {
        config_toml: spec.config_toml.clone(),
        meta,
        mode: mode.into(),
        blocks,
    };
    write_sol(&spec.path, &payload).with_context(|| format!("writing {}", spec.path.display()))?;

    let size = std::fs::metadata(&spec.path).map(|m| m.len()).unwrap_or(0);
    println!(
        "sol: wrote {} ({}, {} block{}, mode={:?})",
        spec.path.display(),
        human_size(size),
        block_count,
        if block_count == 1 { "" } else { "s" },
        mode,
    );
    Ok(())
}

// --- load (`inspect --sol`) -------------------------------------------------

/// A loaded `.sol` artifact: the rebuilt tree (deterministically, from the
/// embedded config), the parsed config and payoff models it was built with
/// (needed again for a river re-solve), the cached metadata, and everything
/// [`SolProvider`] needs to answer strategy queries against it.
pub(crate) struct LoadedSol {
    pub pf_game: PostflopGame,
    pub config: PostflopConfig,
    pub rake: Box<dyn RakeModel>,
    pub utility: Box<dyn UtilityModel>,
    pub meta: SolMeta,
    pub mode: StreetsStored,
    /// Stored blocks keyed by `sref` (== the action node's `Node::aux`).
    pub blocks: HashMap<u32, Vec<u8>>,
    pub streets: Vec<Street>,
    pub parents: Vec<NodeId>,
    pub board: Vec<Card>,
    pub river_iterations: u64,
    pub river_target: Option<f64>,
}

/// Reads, verifies, and rebuilds a `.sol` artifact: `formats::read_sol`
/// (which itself verifies the header hash against the embedded config text)
/// -> parse that text as a `SolveConfig` (must be `kind = "postflop"`) ->
/// rebuild the exact same tree `solve --sol` built -> verify the stored
/// block set matches the mode-implied set of action-node `sref`s exactly.
/// That last check is the artifact's only defense against a hand-edited or
/// bit-rotted `.sol` file whose header hash still happens to match (e.g. a
/// `formats` version bump that changed `aux` assignment): without it, a
/// mismatched node id would silently serve the wrong node's strategy.
pub(crate) fn load_sol(
    path: &Path,
    river_iterations: u64,
    river_target: Option<f64>,
) -> Result<LoadedSol> {
    let payload = read_sol(path).with_context(|| format!("reading {}", path.display()))?;

    let config: SolveConfig = toml::from_str(&payload.config_toml)
        .context(".sol artifact's embedded config failed to parse")?;
    match &config.game {
        GameSection::Postflop { .. } => {}
        GameSection::Preflop { .. } => {
            bail!(".sol artifacts are not supported for preflop configs yet (kind = \"preflop\")");
        }
        _ => bail!(".sol artifact's embedded config is not kind = \"postflop\""),
    }
    let GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        iso_merging,
        bets,
    } = config.game
    else {
        unreachable!("checked above");
    };

    let pf_config = postflop_setup::build_postflop_config(
        &board,
        &oop_range,
        &ip_range,
        pot,
        effective_stack,
        iso_merging,
        bets,
    )?;
    let board_cards = pf_config.board.clone();
    let rake = postflop_setup::build_rake(&config.rake);
    let utility = postflop_setup::build_utility(&config.utility);

    println!("rebuilding tree from embedded config...");
    let build_start = Instant::now();
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };
    let pf_game = build_postflop_game(&pf_config, pipeline);
    println!(
        "tree rebuilt in {:.2}s ({} nodes)",
        build_start.elapsed().as_secs_f64(),
        pf_game.game.tree.nodes.len(),
    );

    let start_street = start_street_from_board_len(pf_config.board.len());
    let streets = node_streets(&pf_game.game.tree, start_street);
    let parents = parent_array(&pf_game.game.tree);

    let expected: HashSet<u32> = pf_game
        .game
        .tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(id, node)| {
            node.kind == NodeKind::Action
                && (payload.mode == StreetsStored::Full || streets[*id] != Street::River)
        })
        .map(|(_, node)| node.aux)
        .collect();

    let stored_len = payload.blocks.len();
    let got: HashSet<u32> = payload.blocks.iter().map(|b| b.sref).collect();
    if got.len() != stored_len || got != expected {
        return Err(anyhow!(
            "artifact does not match the rebuilt tree: {stored_len} stored block(s) vs {} \
             expected action node(s) for mode {:?}",
            expected.len(),
            payload.mode,
        ));
    }

    let blocks: HashMap<u32, Vec<u8>> = payload
        .blocks
        .into_iter()
        .map(|b| (b.sref, b.probs))
        .collect();

    Ok(LoadedSol {
        pf_game,
        config: pf_config,
        rake,
        utility,
        meta: payload.meta,
        mode: payload.mode,
        blocks,
        streets,
        parents,
        board: board_cards,
        river_iterations,
        river_target,
    })
}

// --- strategy-source seam ---------------------------------------------------

/// Source of average strategies (and an EV summary line) for `inspect`'s
/// REPL, abstracting over "a live in-process solve" ([`LiveProvider`]) and
/// "a loaded `.sol` artifact, re-solving river subgames lazily"
/// ([`SolProvider`]) so `inspect.rs`'s `Repl` has exactly one code path for
/// both.
pub(crate) trait StrategyProvider {
    /// Normalized A*H action-major average strategy at an Action node.
    fn average_strategy(&mut self, node: NodeId) -> Result<Vec<f32>>;
    /// One-line summary for the `ev` command.
    fn ev_line(&mut self) -> String;
}

/// Wraps a live in-process `Solver`: every query reads straight through to
/// it, no caching needed since the underlying storage is already in memory.
pub(crate) struct LiveProvider<'a, S: Storage>(pub &'a Solver<PostflopEvaluator, S>);

impl<S: Storage> StrategyProvider for LiveProvider<'_, S> {
    fn average_strategy(&mut self, node: NodeId) -> Result<Vec<f32>> {
        Ok(self.0.average_strategy_at(node))
    }

    fn ev_line(&mut self) -> String {
        let solver = self.0;
        let ev_oop = solver.expected_value(Player::P0);
        let ev_ip = solver.expected_value(Player::P1);
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        format!(
            "ev_oop={ev_oop:.6} ev_ip={ev_ip:.6} expl_oop={:.3e} expl_ip={:.3e} nash_conv={nash_conv:.3e} iterations={}",
            expl[Player::P0],
            expl[Player::P1],
            solver.iteration(),
        )
    }
}

/// A cached river re-solve: the fresh subgame's own solver, plus the map
/// from trunk node ids (rooted at the river-entry node) to this subgame's
/// node ids (from `engine::pair_subtrees`), needed to translate a query
/// against the trunk into a query against the re-solved subgame.
struct RiverSolve {
    solver: Solver<PostflopEvaluator, F32Storage>,
    map: HashMap<NodeId, NodeId>,
}

/// Serves strategies from a loaded `.sol` artifact: a stored block is
/// dequantized directly (this covers every non-river node always, and every
/// node at all in `Full` mode); a river node with no stored block (`NoRivers`
/// mode) triggers a lazy re-solve of its whole river-entry subtree the first
/// time any node in it is queried, cached by entry node thereafter.
///
/// Dequantization happens fresh on every call rather than being cached: it's
/// a single pass over `num_actions * num_hands` `u16`s (at most a few
/// thousand elements even for a full 1,326-combo river node), cheap enough
/// next to a REPL's human-paced query rate that a `HashMap<NodeId, Vec<f32>>`
/// cache would only add bookkeeping for no measurable benefit.
pub(crate) struct SolProvider<'a> {
    loaded: &'a LoadedSol,
    river_solves: HashMap<NodeId, RiverSolve>,
}

impl<'a> SolProvider<'a> {
    pub(crate) fn new(loaded: &'a LoadedSol) -> Self {
        SolProvider {
            loaded,
            river_solves: HashMap::new(),
        }
    }

    /// The river-entry ancestor of a river-street node: walks parents
    /// upward while they stay on the river street, stopping at the highest
    /// such node (whose parent is the chance node that dealt the river, or
    /// -- for a river-start artifact, forced to `Full` mode so this path is
    /// never actually taken -- the tree root). If `node` itself has no
    /// river-street parent, it IS the entry.
    fn river_entry_for(&self, node: NodeId) -> NodeId {
        let streets = &self.loaded.streets;
        let parents = &self.loaded.parents;
        let mut cur = node;
        loop {
            let parent = parents[cur as usize];
            if parent == NodeId::MAX || streets[parent as usize] != Street::River {
                return cur;
            }
            cur = parent;
        }
    }

    /// Re-solves the river subgame rooted at `entry` and caches it: replays
    /// `entry`'s history against the trunk config to get the completed
    /// board/pot/stack, reconstructs both players' reach at `entry` from the
    /// trunk's stored (non-river, hence always present) blocks, builds and
    /// solves a fresh river-only subgame, and pairs its tree against the
    /// trunk's subtree so later queries can translate node ids.
    fn solve_river(&mut self, entry: NodeId) -> Result<()> {
        let loaded = self.loaded;
        let trunk_tree = &loaded.pf_game.game.tree;
        let tag = trunk_tree.tags[entry as usize];
        let info = &loaded.pf_game.node_info[tag as usize];
        if info.history.is_empty() || info.history == "<untagged>" {
            bail!("river-entry node {entry} has no recorded history (untagged node)");
        }
        let history = info.history.clone();

        let root_ranges = &loaded.pf_game.game.root_ranges;
        let root_slices = PerPlayer::new(
            root_ranges[Player::P0].as_slice(),
            root_ranges[Player::P1].as_slice(),
        );
        let reach = reach_at(trunk_tree, root_slices, entry, |_id, sref, out| {
            // Every strict ancestor of a river-entry node is non-river by
            // construction, so its block is always present regardless of
            // `loaded.mode` -- a missing entry here would mean `load_sol`'s
            // own stored-set verification failed to catch a corrupt file.
            let bytes = loaded.blocks.get(&sref.index).unwrap_or_else(|| {
                panic!(
                    "missing stored block for ancestor sref {} while computing river reach",
                    sref.index
                )
            });
            let probs = dequantize_probs(bytes, sref.num_actions as usize, sref.num_hands as usize)
                .unwrap_or_else(|e| panic!("corrupt stored block for sref {}: {e}", sref.index));
            out.copy_from_slice(&probs);
        });

        // A line the solved trunk (essentially) never takes gives one player
        // an all-zero reach vector; the fresh subgame builder would then
        // panic on "ranges share no compatible combos". Refuse up front with
        // an explanation instead — the REPL surfaces provider errors as a
        // plain "error: ..." line, so navigation itself survives. (A reach
        // that is positive only on board-conflicting combos can still slip
        // past this check into the builder assert; that requires a corrupt
        // artifact rather than a merely-unreached line, so the loud panic is
        // the right response there.)
        for p in Player::BOTH {
            if !reach[p].iter().any(|&r| r > 1e-9) {
                bail!(
                    "cannot re-solve river at {history:?}: the solved strategy never \
                     reaches this line for {} (reach is zero for every hand)",
                    if p == Player::P0 { "oop" } else { "ip" },
                );
            }
        }

        let state = river_entry_state(&loaded.config, &history)
            .with_context(|| format!("replaying history {history:?} for river entry {entry}"))?;
        let sub_cfg = river_resolve_config(&loaded.config, &state, &reach);

        println!(
            "re-solving river subgame at {history:?} (up to {} iterations)...",
            loaded.river_iterations
        );
        let start = Instant::now();
        let pipeline = PayoffPipeline {
            rake: loaded.rake.as_ref(),
            utility: loaded.utility.as_ref(),
        };
        let sub_game = build_postflop_game(&sub_cfg, pipeline);

        let mut solver = Solver::<PostflopEvaluator, F32Storage>::new(
            sub_game.game,
            Box::<Dcfr>::default(),
            Some(loaded.river_iterations),
        );
        let mut done = 0u64;
        while done < loaded.river_iterations {
            let chunk = 100.min(loaded.river_iterations - done);
            solver.run(chunk);
            done += chunk;
            if let Some(target) = loaded.river_target {
                let expl = solver.exploitability();
                if expl[Player::P0] + expl[Player::P1] < target {
                    break;
                }
            }
        }
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        println!(
            "done: iterations={} wall={:.2}s nash_conv={:.3e}",
            solver.iteration(),
            start.elapsed().as_secs_f64(),
            nash_conv,
        );

        let map: HashMap<NodeId, NodeId> = pair_subtrees(trunk_tree, entry, &solver.game().tree, 0)
            .map_err(|e| anyhow!("river re-solve produced a structurally different subtree: {e}"))?
            .into_iter()
            .collect();

        self.river_solves.insert(entry, RiverSolve { solver, map });
        Ok(())
    }
}

impl StrategyProvider for SolProvider<'_> {
    fn average_strategy(&mut self, node: NodeId) -> Result<Vec<f32>> {
        let loaded = self.loaded;
        let tree = &loaded.pf_game.game.tree;
        let n = tree.node(node);
        if n.kind != NodeKind::Action {
            bail!("node {node} is not an action node");
        }
        let sref = tree.storage_ref(n);
        if let Some(bytes) = loaded.blocks.get(&n.aux) {
            return dequantize_probs(bytes, sref.num_actions as usize, sref.num_hands as usize)
                .map_err(Into::into);
        }

        if loaded.streets[node as usize] != Street::River {
            bail!(
                "artifact does not match the rebuilt tree: no stored block for non-river node {node}"
            );
        }

        let entry = self.river_entry_for(node);
        if !self.river_solves.contains_key(&entry) {
            self.solve_river(entry)?;
        }
        let rs = &self.river_solves[&entry];
        let sub_node = *rs.map.get(&node).ok_or_else(|| {
            anyhow!("node {node} not found in the re-solved river subtree for entry {entry}")
        })?;
        Ok(rs.solver.average_strategy_at(sub_node))
    }

    fn ev_line(&mut self) -> String {
        let m = &self.loaded.meta;
        format!(
            "at export: ev_oop={:.6} ev_ip={:.6} expl_oop={:.3e} expl_ip={:.3e} nash_conv={:.3e} iterations={}",
            m.ev[0], m.ev[1], m.expl[0], m.expl[1], m.nash_conv, m.iterations,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::I16Storage;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "cli-sol-test-{}-{}-{}",
            std::process::id(),
            id,
            name
        ))
    }

    /// Small turn-start config (board "2s 7s Ks 2h", ranges "44,55" vs
    /// "33,66", pot 2, stack 20, turn [0.75], river [1.0], max_raises 1/1),
    /// copied from `crates/holdem/tests/viewer.rs`'s `small_turn_config`: a
    /// single chance node whose children are exactly the river-entry nodes,
    /// small enough to build/solve/re-solve fast even in a debug build.
    const TINY_TURN_TOML: &str = r#"
schema = "solvers.postflop/v1"

[game]
kind = "postflop"
board = "2s 7s Ks 2h"
oop_range = "44,55"
ip_range = "33,66"
pot = 2
effective_stack = 20

[game.bets.turn]
oop = [0.75]
ip = [0.75]
max_raises = 1

[game.bets.river]
oop = [1.0]
ip = [1.0]
max_raises = 1

[run]
iterations = 32
check_every = 32
"#;

    /// River-start variant of the same board/ranges (5-card board), used by
    /// the `Full`-mode test: a river-start config has no streets before the
    /// river at all, so `export_sol` must force `Full` regardless of the
    /// caller's requested mode.
    const TINY_RIVER_TOML: &str = r#"
schema = "solvers.postflop/v1"

[game]
kind = "postflop"
board = "2s 7s Ks 2h 9d"
oop_range = "44,55"
ip_range = "33,66"
pot = 2
effective_stack = 20

[game.bets.river]
oop = [1.0]
ip = [1.0]
max_raises = 1

[run]
iterations = 16
check_every = 16
"#;

    /// Builds+solves `raw` (a `SolveConfig` TOML, `kind = "postflop"`)
    /// in-process, bypassing the CLI's stdout-printing `solve::run` path
    /// (not needed for these unit tests), and returns everything
    /// `export_sol` needs.
    ///
    /// Uses the default (parallel) `ParConfig` rather than a sequential one:
    /// every assertion in this module compares a value against the *same*
    /// solver's own output (or checks an internal invariant like "columns
    /// sum to 1"), never against a second, independently-run solve, so nondet
    /// float-reduction order across parallel chance-branch fan-out cannot
    /// make any of these tests flaky. Skipping the sequential-only
    /// restriction cuts this fixture's already-noted-elsewhere-as-slow
    /// solve time (see `crates/holdem/tests/viewer.rs`'s comment on
    /// `small_turn_config`) roughly 3x on this machine's 4 cores.
    fn build_and_solve<S: Storage>(
        raw: &str,
        iterations: u64,
    ) -> (Solver<PostflopEvaluator, S>, Street, RunSummary) {
        let config: SolveConfig = toml::from_str(raw).expect("parse fixture toml");
        let GameSection::Postflop {
            board,
            oop_range,
            ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
        } = config.game
        else {
            panic!("fixture must be kind = \"postflop\"");
        };
        let pf_config = postflop_setup::build_postflop_config(
            &board,
            &oop_range,
            &ip_range,
            pot,
            effective_stack,
            iso_merging,
            bets,
        )
        .expect("build postflop config");
        let start_street = start_street_from_board_len(pf_config.board.len());
        let rake = postflop_setup::build_rake(&config.rake);
        let utility = postflop_setup::build_utility(&config.utility);
        let pipeline = PayoffPipeline {
            rake: rake.as_ref(),
            utility: utility.as_ref(),
        };
        let pf_game = build_postflop_game(&pf_config, pipeline);

        let mut solver =
            Solver::<_, S>::new(pf_game.game, Box::<Dcfr>::default(), Some(iterations));
        let start = Instant::now();
        solver.run(iterations);
        let elapsed = start.elapsed();
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        let summary = RunSummary {
            canceled: false,
            iterations: solver.iteration(),
            wall: elapsed,
            expl_p0: expl[Player::P0],
            expl_p1: expl[Player::P1],
            nash_conv,
        };
        (solver, start_street, summary)
    }

    /// Covers both "round trip" and "`SolProvider` seam" test concerns in one
    /// function, sharing a single (expensive: this fixture's actual CFR
    /// solve is the dominant cost in this module's test suite, per
    /// `build_and_solve`'s doc comment) build+solve+export+load rather than
    /// repeating it once per concern:
    /// 1. every stored block dequantizes to within `1e-3` of the live
    ///    solver's own `average_strategy_at` at the same node, and river
    ///    action nodes have no stored block in `NoRivers` mode;
    /// 2. `SolProvider` serves a non-river node matching the live solver,
    ///    and a river-node query triggers a re-solve that returns a valid
    ///    (per-hand-normalized) strategy of the right shape, then caches it
    ///    (a second query to the same entry does not re-solve).
    #[test]
    fn round_trip_and_sol_provider_no_rivers_f32() {
        let (solver, start_street, summary) = build_and_solve::<F32Storage>(TINY_TURN_TOML, 32);
        let path = temp_path("roundtrip.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, start_street, &summary).expect("export");

        let loaded = load_sol(&path, 64, None).expect("load");
        assert_eq!(loaded.mode, StreetsStored::NoRivers);

        // --- 1. round trip: every stored block matches the live solver ---
        let tree = &solver.game().tree;
        let streets = node_streets(tree, start_street);
        let mut river_node = None;
        let mut checked_any = false;
        for id in 0..tree.nodes.len() as NodeId {
            let node = tree.node(id);
            if node.kind != NodeKind::Action {
                continue;
            }
            if streets[id as usize] == Street::River {
                assert!(
                    !loaded.blocks.contains_key(&node.aux),
                    "river node {id} must have no stored block in NoRivers mode"
                );
                river_node.get_or_insert(id);
                continue;
            }
            let sref = tree.storage_ref(node);
            let bytes = loaded
                .blocks
                .get(&node.aux)
                .unwrap_or_else(|| panic!("missing block for non-river node {id}"));
            let dequant =
                dequantize_probs(bytes, sref.num_actions as usize, sref.num_hands as usize)
                    .unwrap();
            let live = solver.average_strategy_at(id);
            for (a, b) in dequant.iter().zip(live.iter()) {
                assert!(
                    (a - b).abs() < 1e-3,
                    "node {id}: dequantized {a} vs live {b}"
                );
            }
            checked_any = true;
        }
        assert!(checked_any, "expected at least one non-river action node");
        let river_node = river_node.expect("fixture must have a river action node");

        // --- 2. SolProvider: non-river direct, river lazy-resolve+cache ---
        let mut provider = SolProvider::new(&loaded);

        let sol_avg = provider.average_strategy(0).expect("root strategy");
        let live_avg = solver.average_strategy_at(0);
        for (a, b) in sol_avg.iter().zip(live_avg.iter()) {
            assert!((a - b).abs() < 1e-3, "root: sol {a} vs live {b}");
        }

        let strategy = provider
            .average_strategy(river_node)
            .expect("river re-solve strategy");
        let sref = tree.storage_ref(tree.node(river_node));
        assert_eq!(strategy.len(), sref.len());
        let num_hands = sref.num_hands as usize;
        let num_actions = sref.num_actions as usize;
        for h in 0..num_hands {
            let sum: f32 = (0..num_actions).map(|a| strategy[a * num_hands + h]).sum();
            assert!(
                (sum - 1.0).abs() < 1e-3,
                "hand {h} action probabilities should sum to 1, got {sum}"
            );
        }
        assert_eq!(provider.river_solves.len(), 1);

        // A second query into the same river-entry subtree must reuse the
        // cached re-solve, not trigger another one.
        let _ = provider
            .average_strategy(river_node)
            .expect("cached river strategy");
        assert_eq!(
            provider.river_solves.len(),
            1,
            "second query to the same entry must not re-solve"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn i16_export_dequantizes_to_valid_distributions() {
        let (solver, start_street, summary) = build_and_solve::<I16Storage>(TINY_TURN_TOML, 32);
        let path = temp_path("i16.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "i16".to_string(),
        };
        export_sol(&spec, &solver, start_street, &summary).expect("export");

        let loaded = load_sol(&path, 64, None).expect("load");
        let tree = &solver.game().tree;
        let mut checked_any = false;
        for id in 0..tree.nodes.len() as NodeId {
            let node = tree.node(id);
            if node.kind != NodeKind::Action {
                continue;
            }
            let Some(bytes) = loaded.blocks.get(&node.aux) else {
                continue;
            };
            let sref = tree.storage_ref(node);
            let num_hands = sref.num_hands as usize;
            let num_actions = sref.num_actions as usize;
            let probs = dequantize_probs(bytes, num_actions, num_hands).unwrap();
            for h in 0..num_hands {
                let sum: f32 = (0..num_actions).map(|a| probs[a * num_hands + h]).sum();
                assert!((sum - 1.0).abs() < 1e-3, "hand {h} column sum {sum}");
            }
            checked_any = true;
        }
        assert!(checked_any);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn full_mode_forced_on_river_start_config() {
        let (solver, start_street, summary) = build_and_solve::<F32Storage>(TINY_RIVER_TOML, 16);
        assert_eq!(start_street, Street::River);
        let path = temp_path("river-full.sol");
        // Deliberately request NoRivers: export_sol must force Full anyway.
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_RIVER_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, start_street, &summary).expect("export");

        let loaded = load_sol(&path, 10, None).expect("load");
        assert_eq!(loaded.mode, StreetsStored::Full);

        let tree = &solver.game().tree;
        let mut checked_any = false;
        for id in 0..tree.nodes.len() as NodeId {
            let node = tree.node(id);
            if node.kind == NodeKind::Action {
                assert!(
                    loaded.blocks.contains_key(&node.aux),
                    "action node {id} must have a stored block in Full mode"
                );
                checked_any = true;
            }
        }
        assert!(checked_any);
        let _ = std::fs::remove_file(&path);
    }

    /// Validates the lazy river re-solve's *accuracy*, not just its shape:
    /// solves `TINY_TURN_TOML` tightly (3000 iterations, well past the point
    /// this tiny fixture's own `nash_conv` bottoms out) so the trunk's own
    /// average strategy at each river-entry node is a meaningful ground
    /// truth, exports `NoRivers`, then re-solves several river-entry
    /// subgames (via `SolProvider`, `river_iterations = 2000`,
    /// `river_target = Some(0.002)` -- a tight budget relative to this
    /// fixture's entry pots) and compares the result against the trunk's own
    /// `average_strategy_at` at the *same* node id (no trunk<->subtree
    /// mapping needed on the trunk side -- `SolProvider::average_strategy`
    /// already translates its answer back to trunk coordinates).
    ///
    /// As `holdem::viewer`'s module doc spells out, a reach-weighted fresh
    /// subgame solve reproduces the trunk's river strategy only
    /// approximately: the trunk solved the whole game jointly, so its river
    /// strategy is correlated with every other river-entry node through the
    /// shared regret-matching/averaging dynamics, whereas the re-solve seeds
    /// each subgame independently from a snapshotted reach and solves it in
    /// isolation. The two agree in the limit of exact convergence (both are
    /// best responses to the same fixed opponent range) but not bit-for-bit
    /// at any finite iteration count -- and near-indifferent hands (multiple
    /// actions roughly tied in EV) can land on different points of the same
    /// equilibrium set between the two solves. So per-hand tolerance here is
    /// deliberately loose (0.15 absolute, and only for the acting player's
    /// hands with non-negligible reach at the node) while the tight,
    /// load-bearing assertion is the reach-weighted aggregate per action
    /// (0.03): a real bug (wrong reach reconstruction, a wrong trunk<->
    /// subtree node mapping) shows up as a large *aggregate* disagreement,
    /// not just a few wandering indifferent hands.
    ///
    /// Candidates are restricted to river-entry nodes whose acting-player
    /// reach vector has genuine per-hand **spread** (see the `candidates`
    /// loop below for the precise definition) -- a filter earned the hard
    /// way while tuning this test against the real fixture. This tiny
    /// config's root decision (turn check vs. bet) converges to an
    /// essentially pure "always check" for *every* hand after 3000
    /// iterations, so every river-entry node under the check-check line
    /// inherits identical reach for every hand regardless of which specific
    /// river card falls -- and that turns out to correlate with a genuine
    /// equilibrium tie at the river action itself (confirmed empirically:
    /// an isolated re-solve there reaches `nash_conv` on the order of
    /// `1e-8`, an essentially exact equilibrium of the *isolated* subgame,
    /// while still landing on a non-corner mixed strategy identical across
    /// every hand in the range -- only possible when the two actions are
    /// nearly exactly tied in EV). That is not "some hands wander," it's
    /// the whole node's decision being indeterminate: an artifact of this
    /// fixture's tiny two-rank-per-side ranges having no natural way to
    /// differentiate once the upstream decision doesn't depend on hand
    /// strength either, not a re-solve defect. Symmetrically, a node the
    /// trunk's average strategy essentially never reaches (e.g. anything
    /// under the root's `bet` line) has an all-but-empty reach vector (no
    /// two survivors to compute a spread from, so it's filtered out too)
    /// and is both meaningless to compare and reproducibly crashes
    /// `SolProvider::solve_river` (`river_resolve_config` builds a `Range`
    /// with every weight clamped to ~0, and `build_postflop_game` panics
    /// with "ranges share no compatible combos") -- a pre-existing
    /// limitation of the re-solve path when asked to navigate to an
    /// effectively-unreachable node, out of scope for this test to fix, but
    /// worth steering around here rather than tripping over by accident.
    /// The one spread-passing line this fixture has (facing a turn bet,
    /// where OOP's call/fold split genuinely depends on hand strength)
    /// checks out beautifully (aggregate diffs several orders of magnitude
    /// under tolerance) -- exactly the well-identified case this test is
    /// meant to validate.
    #[test]
    #[ignore = "slow unoptimized; CI runs it in release with --include-ignored"]
    fn river_resolve_accuracy() {
        let (solver, start_street, summary) = build_and_solve::<F32Storage>(TINY_TURN_TOML, 3000);
        let path = temp_path("river-accuracy.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, start_street, &summary).expect("export");

        let loaded = load_sol(&path, 2000, Some(0.002)).expect("load");
        let mut provider = SolProvider::new(&loaded);

        let tree = &solver.game().tree;
        let streets = node_streets(tree, start_street);
        let parents = parent_array(tree);
        let root_ranges = &solver.game().root_ranges;
        let root_slices = PerPlayer::new(
            root_ranges[Player::P0].as_slice(),
            root_ranges[Player::P1].as_slice(),
        );
        // River-entry nodes: Action nodes on the river street whose parent
        // is a Chance node -- the same definition `SolProvider::river_entry_for`
        // walks toward, applied directly here since we already have the
        // trunk's own `streets`/`parents` in hand. For each, compute the
        // acting player's reach up front (cheap: `reach_at` is one pass over
        // the short path to the node), then keep only nodes whose reach
        // vector has genuine per-hand *spread* among the hands that are
        // both originally in range and still card-compatible with this
        // node's board (i.e. `min`/`max` computed only over survivors of
        // card removal, so a river card that happens to block a few combos
        // doesn't get mistaken for real strategic differentiation): this is
        // this test's own doc comment's filter, discovered empirically to
        // cleanly separate this fixture's two lines -- every "check-check"
        // river entry has *zero* spread (every surviving hand reaches with
        // identical probability, root's own check decision being pure) vs.
        // the "bet-call" line's ~0.013 spread (a genuinely hand-dependent
        // call/fold split upstream).
        let mut candidates: Vec<(NodeId, Vec<f32>, f64)> = Vec::new();
        for id in 0..tree.nodes.len() as NodeId {
            let node = *tree.node(id);
            if node.kind != NodeKind::Action || streets[id as usize] != Street::River {
                continue;
            }
            let parent = parents[id as usize];
            if parent == NodeId::MAX || tree.node(parent).kind != NodeKind::Chance {
                continue;
            }
            let reach = reach_at(tree, root_slices, id, |aid, _sref, out| {
                out.copy_from_slice(&solver.average_strategy_at(aid));
            });
            let acting_reach = reach[node.player].clone();
            let reach_sum: f64 = acting_reach.iter().map(|&r| f64::from(r)).sum();
            let root_full = root_slices[node.player];
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for h in 0..acting_reach.len() {
                if root_full[h] <= 0.5 || acting_reach[h] <= 1e-6 {
                    continue;
                }
                let v = f64::from(acting_reach[h]);
                min = min.min(v);
                max = max.max(v);
            }
            if max - min <= 0.005 {
                continue;
            }
            candidates.push((id, acting_reach, reach_sum));
        }
        assert!(
            candidates.len() >= 3,
            "expected at least 3 river-entry nodes with genuine per-hand reach spread, found {}",
            candidates.len()
        );

        // Sample a spread across the filtered candidates (first, last, and
        // evenly spaced in between) rather than only whichever nodes happen
        // to sort first by id.
        let sample_count = 4.min(candidates.len());
        let mut sample_idx: Vec<usize> = Vec::new();
        for i in 0..sample_count {
            sample_idx.push(i * (candidates.len() - 1) / (sample_count - 1).max(1));
        }
        sample_idx.dedup();

        let mut max_per_action_diff = 0.0f32;
        let mut max_aggregate_diff = 0.0f32;

        for &idx in &sample_idx {
            let (entry, ref acting_reach, reach_sum) = candidates[idx];
            let node = *tree.node(entry);
            let sref = tree.storage_ref(&node);
            let num_actions = sref.num_actions as usize;
            let num_hands = sref.num_hands as usize;
            assert_eq!(acting_reach.len(), num_hands);

            let sol_avg = provider
                .average_strategy(entry)
                .expect("provider river re-solve query");
            let trunk_avg = solver.average_strategy_at(entry);
            assert_eq!(sol_avg.len(), trunk_avg.len());

            // The re-solve's own strategy must be a valid per-hand
            // distribution -- a cheap stand-in for "the cached RiverSolve
            // actually converged" without exposing any new public API to
            // re-derive its `nash_conv` from outside this module.
            for h in 0..num_hands {
                let sum: f32 = (0..num_actions).map(|a| sol_avg[a * num_hands + h]).sum();
                assert!(
                    (sum - 1.0).abs() < 1e-3,
                    "entry {entry} hand {h}: re-solved column sums to {sum}, expected 1.0"
                );
            }

            let mut agg_diff = vec![0.0f64; num_actions];
            for h in 0..num_hands {
                let r = f64::from(acting_reach[h]);
                for a in 0..num_actions {
                    let sol_p = sol_avg[a * num_hands + h];
                    let trunk_p = trunk_avg[a * num_hands + h];
                    agg_diff[a] += r * f64::from(sol_p - trunk_p);
                    if r >= 1e-3 {
                        let diff = (sol_p - trunk_p).abs();
                        max_per_action_diff = max_per_action_diff.max(diff);
                        assert!(
                            diff <= 0.15,
                            "entry {entry} hand {h} action {a}: reach={r:.4} sol={sol_p:.4} \
                             trunk={trunk_p:.4} diff={diff:.4} exceeds 0.15"
                        );
                    }
                }
            }
            for (a, diff_sum) in agg_diff.iter().enumerate() {
                let diff = (diff_sum / reach_sum).abs() as f32;
                max_aggregate_diff = max_aggregate_diff.max(diff);
                assert!(
                    diff <= 0.03,
                    "entry {entry} action {a}: reach-weighted diff {diff:.4} exceeds 0.03"
                );
            }
        }

        eprintln!(
            "river_resolve_accuracy: sampled {} of {} candidate entries, \
             max_per_action_diff={max_per_action_diff:.8}, max_aggregate_diff={max_aggregate_diff:.8}",
            sample_idx.len(),
            candidates.len(),
        );

        let _ = std::fs::remove_file(&path);
    }
}
