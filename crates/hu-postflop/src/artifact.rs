//! P1 artifact export, verified loading and lazy river strategy queries.
//! `sol` owns the codec. This module owns rebuilding the tree and the
//! reach-weighted river solve; callers render the emitted diagnostics.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::sol::{
    SolMeta, SolPayload, StrategyBlock, StreetsStored, ValueBlock, dequantize_probs,
    dequantize_values, quantize_probs, quantize_values, read_sol, write_sol,
};
use crate::{
    PostflopConfig, PostflopEvaluator, PostflopGame, build_postflop_game, node_streets,
    river_entry_state, river_resolve_config,
};
use anyhow::{Context, Result, anyhow, bail};
use hu_engine::{
    F32Storage, I16Storage, NodeId, NodeKind, Solver, Storage, pair_subtrees, parent_array,
    reach_at,
};
use nlh::{Card, PerPlayer, Player, Street};

use crate::run::RunSummary;

/// Which streets get stored strategy blocks in a `.sol`
/// export. Mirrors `crate::sol::StreetsStored` one-to-one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// mirrors `crate::build_postflop_game`'s own board-length dispatch (3 =
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

/// Everything common-input P1 solve gathers up front (while the raw config text and
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

/// Per-hand opponent reach compatible with each of `p`'s hands.
///
/// A counterfactual value `v[h]` is `sum over opponent hands o of
/// reach(o) * payoff(h, o)`, so turning it into a per-hand utility value
/// means dividing by the reach that could actually be facing `h`. Hands
/// sharing a card with `h` cannot, which is the usual inclusion-exclusion:
/// total, minus the reach through each of `h`'s two cards, plus the one
/// combo that is both of them and was therefore subtracted twice.
fn compatible_reach(hands: &crate::PostflopHands, p: Player, opp_reach: &[f32]) -> Vec<f32> {
    let total: f64 = opp_reach.iter().map(|&r| r as f64).sum();
    let mut per_card = [0.0f64; 52];
    for (&combo, &reach) in hands.combos(p.opponent()).iter().zip(opp_reach) {
        if reach == 0.0 {
            continue;
        }
        let (hi, lo) = nlh::combo_cards(combo as usize);
        per_card[hi.index()] += reach as f64;
        per_card[lo.index()] += reach as f64;
    }
    hands
        .combos(p)
        .iter()
        .enumerate()
        .map(|(i, &combo)| {
            let (hi, lo) = nlh::combo_cards(combo as usize);
            let same = hands.same[p][i];
            let r = if same == crate::hands::ABSENT {
                0.0
            } else {
                opp_reach[same as usize] as f64
            };
            (total - per_card[hi.index()] - per_card[lo.index()] + r).max(0.0) as f32
        })
        .collect()
}

/// Walks the tree once carrying both players' reach, calling `visit` at
/// every action node.
///
/// `hu_engine::reach_at` answers one node by re-walking the path to it, which
/// is the right shape for a viewer but quadratic when every node needs an
/// answer. Only the current path's reaches are alive at a time, so this
/// stays linear in memory as well as in work.
fn walk_reaches<F>(
    tree: &hu_engine::PublicTree,
    node_id: NodeId,
    reach: &PerPlayer<Vec<f32>>,
    strategy: &dyn Fn(NodeId) -> Vec<f32>,
    visit: &mut F,
) where
    F: FnMut(NodeId, &PerPlayer<Vec<f32>>),
{
    let node = *tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {}
        NodeKind::Action => {
            visit(node_id, reach);
            let sref = tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            let sigma = strategy(node_id);
            for (position, child) in tree.children(node_id).enumerate() {
                let column = &sigma[position * num_hands..(position + 1) * num_hands];
                let mut next = reach.clone();
                for (r, &c) in next[node.player].iter_mut().zip(column) {
                    *r *= c;
                }
                walk_reaches(tree, child, &next, strategy, visit);
            }
        }
        NodeKind::Chance => {
            for (position, child) in tree.children(node_id).enumerate() {
                let deal = *tree.deal(&node, position);
                let mut next = PerPlayer::new(Vec::new(), Vec::new());
                for player in Player::BOTH {
                    let map = deal.maps[player];
                    let len = tree.mapped_dim(map, reach[player].len() as u32) as usize;
                    let mut mapped = vec![0.0f32; len];
                    tree.map_reach_into(map, &reach[player], &mut mapped);
                    next[player] = mapped;
                }
                walk_reaches(tree, child, &next, strategy, visit);
            }
        }
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
    ev_offset: PerPlayer<f64>,
    start_street: Street,
    summary: &RunSummary,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<()> {
    let tree = &solver.game().tree;
    let streets = node_streets(tree, start_street);

    let mode = if start_street == Street::River {
        if spec.mode != SolStreets::Full {
            diagnostics(Diagnostic::RiverFull);
        }
        SolStreets::Full
    } else {
        spec.mode
    };

    // One value pass per player records every action node's per-hand
    // values, so storing them costs two walks of the tree rather than one
    // walk per node.
    let recorded = PerPlayer::new(
        solver.expected_values_everywhere(Player::P0),
        solver.expected_values_everywhere(Player::P1),
    );

    // Reaches for every node in one walk, so a node's counterfactual values
    // can be turned into per-hand chips where they are produced.
    let mut reaches: Vec<Option<PerPlayer<Vec<f32>>>> = vec![None; tree.nodes.len()];
    {
        let strategy = |id: NodeId| solver.average_strategy_at(id);
        let roots = &solver.game().root_ranges;
        let root_reach = PerPlayer::new(roots[Player::P0].clone(), roots[Player::P1].clone());
        walk_reaches(tree, 0, &root_reach, &strategy, &mut |id, reach| {
            reaches[id as usize] = Some(reach.clone());
        });
    }

    let mut blocks = Vec::new();
    let mut values = Vec::new();
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
        let reach = reaches[id as usize]
            .as_ref()
            .expect("every action node is visited by the reach walk");
        let mut per_node = Vec::new();
        for player in Player::BOTH {
            // A counterfactual value is opponent-reach-weighted, so it has
            // to be divided by the reach that could be facing this hand
            // before a utility-valued offset can be added to it. Skipping that would
            // be a unit error, not a scaling one.
            let compatible = compatible_reach(
                &solver.game().evaluator.hands,
                player,
                &reach[player.opponent()],
            );
            // Every node uses the original subgame's utility baseline.
            // Adding this node's contributions would erase sunk wagers;
            // adding chips at all would mix units for ICM.
            let offset = ev_offset[player] as f32;
            let per_hand = recorded[player][node.aux as usize]
                .as_ref()
                .expect("every action node is recorded by the value pass");
            let own = &reach[player];
            per_node.extend(per_hand.iter().zip(&compatible).zip(own).map(
                |((value, &facing), &here)| {
                    // A hand that cannot be held here has no EV to
                    // report, and neither does one no opponent hand can
                    // face. Zeroing both is not just tidy: a
                    // counterfactual value is defined for every hand
                    // whether or not it can arrive, so storing them all
                    // would make these blocks dense where the strategy
                    // blocks are sparse, which is most of what the
                    // artifact's size is.
                    if here > 0.0 && facing > 0.0 {
                        value / facing + offset
                    } else {
                        0.0
                    }
                },
            ));
        }
        let (scale, bytes) = quantize_values(&per_node);
        values.push(ValueBlock {
            sref: node.aux,
            scale,
            values: bytes,
        });
    }
    // Ascending by construction (node ids walked in order and `aux` assigned
    // in build order), but sorted explicitly to make that guarantee robust
    // to any future change in how `aux` is assigned.
    blocks.sort_by_key(|b| b.sref);
    values.sort_by_key(|b| b.sref);
    let block_count = blocks.len();

    let meta = SolMeta {
        iterations: summary.iterations,
        expl: [summary.expl_p0, summary.expl_p1],
        // Already on the subgame-start basis: the run summary carries the
        // same numbers `done:` printed, so an artifact and the console can
        // never disagree about what EV means.
        ev: [summary.ev[Player::P0], summary.ev[Player::P1]],
        nash_conv: summary.nash_conv,
        storage: spec.storage_name.clone(),
        wall_secs: summary.wall.as_secs_f64(),
    };

    let payload = SolPayload {
        config_toml: spec.config_toml.clone(),
        meta,
        mode: mode.into(),
        blocks,
        values,
    };
    write_sol(&spec.path, &payload).with_context(|| format!("writing {}", spec.path.display()))?;

    let size = std::fs::metadata(&spec.path).map(|m| m.len()).unwrap_or(0);
    diagnostics(Diagnostic::Written {
        path: spec.path.clone(),
        bytes: size,
        blocks: block_count,
        mode,
    });
    Ok(())
}

// --- load (`inspect --sol`) -------------------------------------------------

/// A loaded `.sol` artifact: the rebuilt tree (deterministically, from the
/// embedded config), the parsed config and payoff models it was built with
/// (needed again for a river re-solve), the cached metadata, and everything
/// [`SolProvider`] needs to answer strategy queries against it.
pub struct LoadedSol {
    pub pf_game: PostflopGame,
    pub config: PostflopConfig,
    pub meta: SolMeta,
    pub mode: StreetsStored,
    /// Stored blocks keyed by `sref` (== the action node's `Node::aux`).
    pub blocks: HashMap<u32, Vec<u8>>,
    /// Stored per-hand values, same keys as `blocks`: OOP's hands then
    /// IP's, on the subgame-start basis.
    pub values: HashMap<u32, Vec<f32>>,
    pub streets: Vec<Street>,
    pub parents: Vec<NodeId>,
    pub board: Vec<Card>,
    pub river_iterations: u64,
    pub river_target: Option<f64>,
    pub nlh: (spot::Spot, crate::input::Settings),
}

/// Reads, verifies, and rebuilds a `.sol` artifact: `crate::sol::read_sol`
/// (which itself verifies the header hash against the embedded config text)
/// -> parse the embedded common-input P1 spot ->
/// rebuild the exact same tree the P1 run built -> verify the stored
/// block set matches the mode-implied set of action-node `sref`s exactly.
/// That last check is the artifact's only defense against a hand-edited or
/// bit-rotted `.sol` file whose header hash still happens to match (e.g. a
/// `crate::sol` version bump that changed `aux` assignment): without it, a
/// mismatched node id would silently serve the wrong node's strategy.
pub fn load_sol(
    path: &Path,
    river_iterations: u64,
    river_target: Option<f64>,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<LoadedSol> {
    let payload = read_sol(path).with_context(|| format!("reading {}", path.display()))?;

    crate::prepare::require_artifact_config(&payload.config_toml)?;
    let p = crate::prepare::prepare(&payload.config_toml, Path::new("embedded.toml"))?;
    let pf_config = p.config;
    let payoff = p.payoff;
    let nlh = (p.document.spot, p.settings);
    let board_cards = pf_config.board.clone();

    // stderr, not stdout: `export` writes machine-readable data there, and
    // a progress line in the middle of a CSV would corrupt it.
    diagnostics(Diagnostic::Rebuilding);
    let build_start = Instant::now();
    let pipeline = payoff.pipeline();
    let mut pf_game = build_postflop_game(&pf_config, pipeline);
    crate::prepare::display_game(&mut pf_game);
    diagnostics(Diagnostic::Rebuilt {
        secs: build_start.elapsed().as_secs_f64(),
        nodes: pf_game.game.tree.nodes.len(),
    });

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

    for block in &payload.blocks {
        let r = pf_game.game.tree.storage_refs[block.sref as usize];
        dequantize_probs(&block.probs, r.num_actions as usize, r.num_hands as usize)?;
    }
    let value_set: HashSet<u32> = payload.values.iter().map(|b| b.sref).collect();
    if value_set != expected || value_set.len() != payload.values.len() {
        bail!("artifact value blocks do not match the rebuilt tree");
    }
    let value_len = Player::BOTH
        .iter()
        .map(|&p| pf_game.game.evaluator.hands.len(p))
        .sum();
    for block in &payload.values {
        dequantize_values(&block.values, block.scale, value_len)?;
    }
    let blocks: HashMap<u32, Vec<u8>> = payload
        .blocks
        .into_iter()
        .map(|b| (b.sref, b.probs))
        .collect();
    // Dequantized once at load: every reader wants chips, and the scale is
    // per block, so leaving it packed would push that arithmetic into each
    // of them.
    let values: HashMap<u32, Vec<f32>> = payload
        .values
        .into_iter()
        .map(|b| {
            let len = b.values.len() / 2;
            let restored = dequantize_values(&b.values, b.scale, len)
                .with_context(|| format!("decoding stored values for sref {}", b.sref))?;
            Ok((b.sref, restored))
        })
        .collect::<Result<_>>()?;
    if values.len() != blocks.len() {
        return Err(anyhow!(
            "artifact stores {} strategy block(s) but {} value block(s); \
             the two must cover the same nodes",
            blocks.len(),
            values.len(),
        ));
    }

    Ok(LoadedSol {
        pf_game,
        config: pf_config,
        meta: payload.meta,
        mode: payload.mode,
        blocks,
        values,
        streets,
        parents,
        board: board_cards,
        river_iterations,
        river_target,
        nlh,
    })
}

/// Product artifact diagnostics; callers choose their display destination.
pub enum Diagnostic {
    RiverFull,
    Written {
        path: PathBuf,
        bytes: u64,
        blocks: usize,
        mode: SolStreets,
    },
    Rebuilding,
    Rebuilt {
        secs: f64,
        nodes: usize,
    },
    RiverStart {
        history: String,
        iterations: u64,
    },
    RiverDone {
        iterations: u64,
        secs: f64,
        nash_conv: f64,
    },
}
/// Root values from a live solve or from artifact export metadata.
pub struct EvSummary {
    pub ev: [f64; 2],
    pub expl: [f64; 2],
    pub nash_conv: f64,
    pub iterations: u64,
    pub at_export: bool,
}

// --- strategy-source seam ---------------------------------------------------

/// Source of average strategies and typed root values for `inspect`'s
/// REPL, abstracting over "a live in-process solve" ([`crate::queries::LiveProvider`]) and
/// "a loaded `.sol` artifact, re-solving river subgames lazily"
/// ([`SolProvider`]) so `inspect.rs`'s `Repl` has exactly one code path for
/// both.
pub trait StrategyProvider {
    /// Normalized A*H action-major average strategy at an Action node.
    fn average_strategy(&mut self, node: NodeId) -> Result<Vec<f32>>;
    /// Root values and whether they describe the original artifact export.
    fn ev_summary(&self) -> EvSummary;
}

/// A cached river re-solve: the fresh subgame's own solver, plus the map
/// from trunk node ids (rooted at the river-entry node) to this subgame's
/// node ids (from `hu_engine::pair_subtrees`), needed to translate a query
/// against the trunk into a query against the re-solved subgame.
struct RiverSolve {
    solver: RiverSolver,
    map: HashMap<NodeId, NodeId>,
}

enum RiverSolver {
    F32(Solver<PostflopEvaluator, F32Storage>),
    I16(Solver<PostflopEvaluator, I16Storage>),
}

impl RiverSolver {
    fn game(&self) -> &hu_engine::CompiledGame<PostflopEvaluator> {
        match self {
            Self::F32(s) => s.game(),
            Self::I16(s) => s.game(),
        }
    }
    fn average_strategy_at(&self, node: NodeId) -> Vec<f32> {
        match self {
            Self::F32(solver) => solver.average_strategy_at(node),
            Self::I16(solver) => solver.average_strategy_at(node),
        }
    }
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
pub struct SolProvider<'a> {
    loaded: &'a LoadedSol,
    river_solves: HashMap<NodeId, RiverSolve>,
    diagnostics: Box<dyn FnMut(Diagnostic) + 'a>,
}

impl<'a> SolProvider<'a> {
    /// Create a query source with caller-owned diagnostic rendering.
    pub fn new(loaded: &'a LoadedSol, diagnostics: Box<dyn FnMut(Diagnostic) + 'a>) -> Self {
        SolProvider {
            loaded,
            river_solves: HashMap::new(),
            diagnostics,
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

        let internal_history = crate::prepare::convert_history(&history, true);
        let state = river_entry_state(&loaded.config, &internal_history)
            .with_context(|| format!("replaying history {history:?} for river entry {entry}"))?;
        let sub_cfg = river_resolve_config(&loaded.config, &state, &reach);

        (self.diagnostics)(Diagnostic::RiverStart {
            history: history.clone(),
            iterations: loaded.river_iterations,
        });
        let start = Instant::now();
        // The synthetic river tree starts after both players' sunk wagers.
        // Rebind the v1 payoff to their actual stacks at this entry, keeping
        // folded table players and the outside field unchanged.
        let mut spot = loaded.nlh.0.clone();
        let spent = loaded.config.effective_stack.0 - state.effective_stack.0;
        for player in [&spot.context.oop, &spot.context.ip] {
            let seat = &mut spot.context.seats[player.as_ref().expect("P1 player").seat.index()];
            seat.remaining_stack.0 -= spent as u64;
            seat.total_contribution.0 += spent as u64;
        }
        spot.context.pot.0 = state.pot.0 as u64;
        spot.context.effective_stack = Some(nlh::MwChips(state.effective_stack.0 as u64));
        let payoff = crate::input::NlhPayoff::new(&spot)?;
        let sub_game = build_postflop_game(&sub_cfg, payoff.pipeline());
        let rs = if loaded.nlh.1.solver.storage == crate::input::Storage::I16 {
            resolve_river::<I16Storage>(
                loaded,
                sub_game.game,
                entry,
                start,
                RiverSolver::I16,
                &mut self.diagnostics,
            )?
        } else {
            resolve_river::<F32Storage>(
                loaded,
                sub_game.game,
                entry,
                start,
                RiverSolver::F32,
                &mut self.diagnostics,
            )?
        };
        self.river_solves.insert(entry, rs);
        Ok(())
    }
}

fn resolve_river<S: Storage>(
    loaded: &LoadedSol,
    game: hu_engine::CompiledGame<PostflopEvaluator>,
    entry: NodeId,
    start: Instant,
    wrap: impl FnOnce(Solver<PostflopEvaluator, S>) -> RiverSolver,
    diagnostics: &mut dyn FnMut(Diagnostic),
) -> Result<RiverSolve> {
    let settings = &loaded.nlh.1;
    let schedule = crate::run::schedule(&settings.solver.algorithm);
    let mut solver =
        Solver::<PostflopEvaluator, S>::new(game, schedule, Some(loaded.river_iterations));
    solver.set_par(hu_engine::ParConfig {
        chance_depth: settings.solver.parallel.chance_depth,
        min_children: settings.solver.parallel.min_children,
    });
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
    diagnostics(Diagnostic::RiverDone {
        iterations: solver.iteration(),
        secs: start.elapsed().as_secs_f64(),
        nash_conv,
    });

    let map: HashMap<NodeId, NodeId> =
        pair_subtrees(&loaded.pf_game.game.tree, entry, &solver.game().tree, 0)
            .map_err(|e| anyhow!("river re-solve produced a structurally different subtree: {e}"))?
            .into_iter()
            .collect();

    Ok(RiverSolve {
        solver: wrap(solver),
        map,
    })
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
        let sub_avg = rs.solver.average_strategy_at(sub_node);
        let sub_hands = &rs.solver.game().evaluator.hands;
        let trunk_hands = &loaded.pf_game.game.evaluator.hands;
        let mut avg = vec![
            1.0 / sref.num_actions as f32;
            sref.num_actions as usize * sref.num_hands as usize
        ];
        for (i, &global) in sub_hands.combos(n.player).iter().enumerate() {
            let j = trunk_hands
                .local(n.player, global as usize)
                .ok_or_else(|| anyhow!("river support is outside parent range"))?;
            for a in 0..sref.num_actions as usize {
                avg[a * sref.num_hands as usize + j] = sub_avg[a * sub_hands.len(n.player) + i];
            }
        }
        Ok(avg)
    }

    fn ev_summary(&self) -> EvSummary {
        let m = &self.loaded.meta;
        EvSummary {
            ev: m.ev,
            expl: m.expl,
            nash_conv: m.nash_conv,
            iterations: m.iterations,
            at_export: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PostflopNodeInfo;
    use crate::game::{PayoffPipeline, UtilityModel};
    use hu_engine::I16Storage;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    fn load_sol(path: &Path, iterations: u64, target: Option<f64>) -> Result<LoadedSol> {
        super::load_sol(path, iterations, target, &mut |_| {})
    }
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn v1_river_resolve_uses_embedded_vanilla_and_i16() {
        let raw = r#"schema='solvers.nlh/v1'
[table]
players=2
stack_bb=21
[spot]
line='BTN c, BB x / BB x, BTN x'
board='2s 7s Ks 2h'
[ranges]
BB='44,55'
BTN='33,66'
[tree]
script='turn { replace bet [75] } river { replace bet [100] }'
[tree.max_aggressive_actions]
turn=1
river=1
[solver]
storage='i16'
iso_merging=false
[solver.algorithm]
schedule='vanilla'
[solver.parallel]
chance_depth=0
[solver.stop]
max_iterations=8
check_every=8
[output]
solution_streets='no-rivers'
[run]
threads=1
memory='1GiB'
"#;
        let p = crate::prepare::prepare(raw, Path::new("test.toml")).unwrap();
        let (solver, _) =
            crate::run::query::<I16Storage>(&p, None, None, &AtomicBool::new(false)).unwrap();
        let offset = p.payoff.ev_offset();
        let expl = solver.exploitability();
        let summary = RunSummary {
            canceled: false,
            iterations: 8,
            wall: std::time::Duration::ZERO,
            ev: crate::run::subgame_ev(crate::run::solver_ev(&solver), offset),
            expl_p0: expl[Player::P0],
            expl_p1: expl[Player::P1],
            nash_conv: expl[Player::P0] + expl[Player::P1],
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.sol");
        super::export_sol(
            &SolExportSpec {
                path: path.clone(),
                mode: SolStreets::NoRivers,
                config_toml: p.document.normalize(&crate::input::P1Sections).unwrap(),
                storage_name: "i16".into(),
            },
            &solver,
            offset,
            Street::Turn,
            &summary,
            &mut |_| {},
        )
        .unwrap();
        let loaded = load_sol(&path, 8, None).unwrap();
        let entry = loaded.pf_game.node_by_history("xr1.5c[9d]").unwrap();
        let mut provider = SolProvider::new(&loaded, Box::new(|_| {}));
        provider.average_strategy(entry).unwrap();
        let resolved = &provider.river_solves[&entry];
        assert!(matches!(resolved.solver, RiverSolver::I16(_)));
        // Independent symmetric cash reference: old chip utility expressed in
        // BB, with the embedded non-default vanilla schedule and i16 storage.
        struct BbUtility;
        impl UtilityModel for BbUtility {
            fn utility(&self, s: &PerPlayer<f64>) -> PerPlayer<f64> {
                s.map(|v| v / 1000.0)
            }
            fn is_zero_sum_affine(&self) -> bool {
                true
            }
        }
        let root = &loaded.pf_game.game.root_ranges;
        let reach = reach_at(
            &loaded.pf_game.game.tree,
            PerPlayer::new(root[Player::P0].as_slice(), root[Player::P1].as_slice()),
            entry,
            |id, _, out| out.copy_from_slice(&provider.average_strategy(id).unwrap()),
        );
        let state = river_entry_state(&loaded.config, "xr1500c[9d]").unwrap();
        let cfg = river_resolve_config(&loaded.config, &state, &reach);
        let game = build_postflop_game(
            &cfg,
            PayoffPipeline {
                rake: &crate::game::NoRake,
                utility: &BbUtility,
            },
        );
        let mut reference =
            Solver::<_, I16Storage>::new(game.game, Box::new(hu_engine::Vanilla), Some(8));
        reference.set_par(hu_engine::ParConfig {
            chance_depth: 0,
            min_children: 12,
        });
        reference.run(8);
        for (trunk, sub) in provider.river_solves[&entry].map.clone() {
            if reference.game().tree.node(sub).kind == NodeKind::Action {
                assert_eq!(
                    provider.average_strategy(trunk).unwrap(),
                    reference.average_strategy_at(sub)
                );
            }
        }
    }

    fn export_sol<S: Storage>(
        spec: &SolExportSpec,
        solver: &Solver<PostflopEvaluator, S>,
        _node_info: &[PostflopNodeInfo],
        start_street: Street,
        summary: &RunSummary,
    ) -> Result<()> {
        let internal = crate::run::solver_ev(solver);
        let offset = PerPlayer::new(
            summary.ev[Player::P0] - internal[Player::P0],
            summary.ev[Player::P1] - internal[Player::P1],
        );
        super::export_sol(spec, solver, offset, start_street, summary, &mut |_| {})
    }

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
    /// "33,66", pot 2, stack 20, turn [2bb], river [100], max_aggressive_actions 1/1),
    /// copied from `crates/hu-postflop/tests/viewer.rs`'s `small_turn_config`: a
    /// single chance node whose children are exactly the river-entry nodes,
    /// small enough to build/solve/re-solve fast even in a debug build.
    const TINY_TURN_TOML: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 2h"
[ranges]
BB = "44,55"
BTN = "33,66"
[tree]
script = "turn { replace bet [2bb] } river { replace bet [100] }"
[tree.max_aggressive_actions]
turn = 1
river = 1
[solver.stop]
max_iterations = 32
check_every = 32
[run]
memory = "1GiB"
"#;

    /// River-start variant used to verify forced full-street export.
    const TINY_RIVER_TOML: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack_bb = 21
[spot]
line = "BTN c, BB x / BB x, BTN x / BB x, BTN x"
board = "2s 7s Ks 2h 9d"
[ranges]
BB = "44,55"
BTN = "33,66"
[tree]
script = "river { replace bet [100] }"
[tree.max_aggressive_actions]
river = 1
[solver.stop]
max_iterations = 16
check_every = 16
[run]
memory = "1GiB"
"#;

    /// Builds and solves a common-input P1 fixture
    /// in-process and returns everything
    /// `export_sol` needs.
    ///
    /// Uses the default (parallel) `ParConfig` rather than a sequential one:
    /// every assertion in this module compares a value against the *same*
    /// solver's own output (or checks an internal invariant like "columns
    /// sum to 1"), never against a second, independently-run solve, so nondet
    /// float-reduction order across parallel chance-branch fan-out cannot
    /// make any of these tests flaky. Skipping the sequential-only
    /// restriction cuts this fixture's already-noted-elsewhere-as-slow
    /// solve time (see `crates/hu-postflop/tests/viewer.rs`'s comment on
    /// `small_turn_config`) roughly 3x on this machine's 4 cores.
    fn build_and_solve<S: Storage>(
        raw: &str,
        iterations: u64,
    ) -> (
        Solver<PostflopEvaluator, S>,
        Vec<PostflopNodeInfo>,
        Street,
        RunSummary,
    ) {
        let p = crate::prepare::prepare(raw, Path::new("test.toml"))
            .expect("parse common-input fixture");
        let start_street = p.document.spot.context.street;
        let ev_offset = p.payoff.ev_offset();
        let mut pf_game = build_postflop_game(&p.config, p.payoff.pipeline());
        crate::prepare::display_game(&mut pf_game);
        let node_info = pf_game.node_info.clone();
        let mut solver = Solver::<_, S>::new(
            pf_game.game,
            crate::run::schedule(&p.settings.solver.algorithm),
            Some(iterations),
        );
        solver.set_par(hu_engine::ParConfig {
            chance_depth: p.settings.solver.parallel.chance_depth,
            min_children: p.settings.solver.parallel.min_children,
        });
        let start = Instant::now();
        solver.run(iterations);
        let elapsed = start.elapsed();
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        let summary = RunSummary {
            canceled: false,
            iterations: solver.iteration(),
            wall: elapsed,
            ev: crate::run::subgame_ev(crate::run::solver_ev(&solver), ev_offset),
            expl_p0: expl[Player::P0],
            expl_p1: expl[Player::P1],
            nash_conv,
        };
        (solver, node_info, start_street, summary)
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
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_TURN_TOML, 32);
        let path = temp_path("roundtrip.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");

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
        let mut provider = SolProvider::new(&loaded, Box::new(|_| {}));

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
        let (solver, node_info, start_street, summary) =
            build_and_solve::<I16Storage>(TINY_TURN_TOML, 32);
        let path = temp_path("i16.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "i16".to_string(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");

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
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_RIVER_TOML, 16);
        assert_eq!(start_street, Street::River);
        let path = temp_path("river-full.sol");
        // Deliberately request NoRivers: export_sol must force Full anyway.
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_RIVER_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");

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
    /// As `crate::viewer`'s module doc spells out, a reach-weighted fresh
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
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_TURN_TOML, 3000);
        let path = temp_path("river-accuracy.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::NoRivers,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");

        let loaded = load_sol(&path, 2000, Some(0.002)).expect("load");
        let mut provider = SolProvider::new(&loaded, Box::new(|_| {}));

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

    /// Value slots exist only for root support. Runout-blocked or otherwise
    /// unreachable hands inside that support carry zero at the node.
    #[test]
    fn unreachable_hands_are_stored_as_zero() {
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_TURN_TOML, 64);
        let path = temp_path("value-sparsity.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::Full,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");
        let payload = read_sol(&path).expect("read");
        let _ = std::fs::remove_file(&path);

        let tree = &solver.game().tree;
        let hands = &solver.game().evaluator.hands;
        let value_len = hands.len(Player::P0) + hands.len(Player::P1);
        assert!(value_len < 2 * nlh::NUM_COMBOS);
        let blocks: HashMap<_, _> = payload.values.iter().map(|b| (b.sref, b)).collect();
        let mut zero_slots = 0;
        walk_reaches(
            tree,
            0,
            &solver.game().root_ranges,
            &|id| solver.average_strategy_at(id),
            &mut |id, reach| {
                let block = blocks[&tree.node(id).aux];
                let stored = dequantize_values(&block.values, block.scale, value_len)
                    .expect("compact values");
                for seat in Player::BOTH {
                    let base = if seat == Player::P0 {
                        0
                    } else {
                        hands.len(Player::P0)
                    };
                    for (i, &here) in reach[seat].iter().enumerate() {
                        if here <= 0.0 {
                            zero_slots += 1;
                            assert_eq!(stored[base + i], 0.0, "{id} {seat:?} {i}");
                        }
                    }
                }
            },
        );
        assert!(
            zero_slots > 0,
            "the fixture must exercise unreachable support slots"
        );
    }

    /// The per-hand values a `.sol` stores must aggregate back to the root
    /// EV the same artifact reports in its metadata, or the `ev` view and
    /// the `summary` view would describe different solves.
    ///
    /// The weights are reach times compatible-opponent-reach: a
    /// counterfactual value was divided by the latter to become a per-hand
    /// chip amount, so putting it back is what re-forms the aggregate.
    #[test]
    fn stored_values_aggregate_to_the_reported_root_ev() {
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_RIVER_TOML, 256);
        let path = temp_path("value-aggregate.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::Full,
            config_toml: TINY_RIVER_TOML.to_string(),
            storage_name: "f32".to_string(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");
        let payload = read_sol(&path).expect("read");
        let _ = std::fs::remove_file(&path);

        let tree = &solver.game().tree;
        let root_aux = tree.node(0).aux;
        let block = payload
            .values
            .iter()
            .find(|b| b.sref == root_aux)
            .expect("root value block");
        let hands = &solver.game().evaluator.hands;
        let value_len = hands.len(Player::P0) + hands.len(Player::P1);
        let stored =
            dequantize_values(&block.values, block.scale, value_len).expect("decode values");

        for seat in Player::BOTH {
            let own = &solver.game().root_ranges[seat];
            let facing = compatible_reach(
                &solver.game().evaluator.hands,
                seat,
                &solver.game().root_ranges[seat.opponent()],
            );
            let num_hands = hands.len(seat);
            let base = if seat == Player::P0 {
                0
            } else {
                hands.len(Player::P0)
            };
            let mut weighted = 0.0f64;
            let mut total = 0.0f64;
            for hand in 0..num_hands {
                let weight = own[hand] as f64 * facing[hand] as f64;
                if weight <= 0.0 {
                    continue;
                }
                weighted += weight * stored[base + hand] as f64;
                total += weight;
            }
            let aggregated = weighted / total;
            let reported = summary.ev[seat];
            assert!(
                (aggregated - reported).abs() < 0.05,
                "{seat:?}: stored values aggregate to {aggregated}, artifact reports {reported}"
            );
        }
    }

    /// Strategies and values are written by the same loop, so they cover
    /// exactly the same nodes: a reader that found a strategy can always
    /// find its values. `NoRivers` drops both together — a river node that
    /// has no stored strategy must not have stored values either, or a
    /// reader would think it could answer a node it cannot replay.
    #[test]
    fn stored_values_cover_exactly_the_stored_strategy_nodes() {
        for mode in [SolStreets::NoRivers, SolStreets::Full] {
            let (solver, node_info, start_street, summary) =
                build_and_solve::<F32Storage>(TINY_TURN_TOML, 16);
            let path = temp_path("value-coverage.sol");
            let spec = SolExportSpec {
                path: path.clone(),
                mode,
                config_toml: TINY_TURN_TOML.to_string(),
                storage_name: "f32".to_string(),
            };
            export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");
            let payload = read_sol(&path).expect("read");
            let _ = std::fs::remove_file(&path);

            let strategy_srefs: Vec<u32> = payload.blocks.iter().map(|b| b.sref).collect();
            let value_srefs: Vec<u32> = payload.values.iter().map(|b| b.sref).collect();
            assert_eq!(strategy_srefs, value_srefs, "{mode:?}");
            assert!(!strategy_srefs.is_empty(), "{mode:?}: nothing stored");

            let tree = &solver.game().tree;
            let streets = node_streets(tree, start_street);
            let mut saw_river = false;
            for id in 0..tree.nodes.len() as NodeId {
                let node = tree.node(id);
                if node.kind != NodeKind::Action || streets[id as usize] != Street::River {
                    continue;
                }
                saw_river = true;
                let stored = value_srefs.contains(&node.aux);
                match mode {
                    SolStreets::NoRivers => assert!(
                        !stored,
                        "no-rivers stored values for river node {id}; the viewer re-solves \
                         those subtrees, so their values would be unreplayable"
                    ),
                    SolStreets::Full => assert!(stored, "full omitted river node {id}"),
                }
            }
            assert!(
                saw_river,
                "{mode:?}: fixture has no river action node to check"
            );
        }
    }
}
