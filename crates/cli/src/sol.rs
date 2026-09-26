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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use cards::{Card, PerPlayer, Player, Street};
use engine::{
    Dcfr, F32Storage, NodeId, NodeKind, Solver, Storage, pair_subtrees, parent_array, reach_at,
};
use formats::{
    SolMeta, SolPayload, StrategyBlock, StreetsStored, ValueBlock, dequantize_probs,
    dequantize_values, quantize_probs, quantize_values, read_sol, write_sol,
};
use game::{PayoffPipeline, RakeModel, UtilityModel};
use holdem::{
    PostflopConfig, PostflopEvaluator, PostflopGame, PostflopHands, build_postflop_game,
    node_streets, river_entry_state, river_resolve_config,
};

use crate::config::GameSection;
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

/// Per-hand opponent reach compatible with each of `p`'s hands.
///
/// A counterfactual value `v[h]` is `sum over opponent hands o of
/// reach(o) * payoff(h, o)`, so turning it into a per-hand utility value
/// means dividing by the reach that could actually be facing `h`. Hands
/// sharing a card with `h` cannot, which is the usual inclusion-exclusion:
/// total, minus the reach through each of `h`'s two cards, plus the one
/// combo that is both of them and was therefore subtracted twice.
fn compatible_reach(hands: &PostflopHands, player: Player, opp_reach: &[f32]) -> Vec<f32> {
    let opponent = player.opponent();
    assert_eq!(opp_reach.len(), hands.len(opponent));
    let total: f64 = opp_reach.iter().map(|&r| r as f64).sum();
    let mut per_card = [0.0f64; 52];
    for (local, &reach) in opp_reach.iter().enumerate() {
        if reach == 0.0 {
            continue;
        }
        let (hi, lo) = cards::combo_cards(hands.combo(opponent, local));
        per_card[hi.index()] += reach as f64;
        per_card[lo.index()] += reach as f64;
    }
    hands
        .combos(player)
        .iter()
        .map(|&combo| {
            let combo = combo as usize;
            let (hi, lo) = cards::combo_cards(combo);
            let overlap = hands
                .index(opponent, combo)
                .map_or(0.0, |index| opp_reach[index]);
            let compatible = total - per_card[hi.index()] - per_card[lo.index()] + overlap as f64;
            compatible.max(0.0) as f32
        })
        .collect()
}

/// Whether at least one legal private deal can reach this public line.
/// Check support directly: a positive but tiny weight must not be rounded
/// away by an epsilon, multiplication, or subtractive card totals.
fn has_compatible_reach(hands: &PostflopHands, reach: &PerPlayer<Vec<f32>>) -> bool {
    for (own, &weight) in reach[Player::P0].iter().enumerate() {
        if weight <= 0.0 {
            continue;
        }
        let (a, b) = cards::combo_cards(hands.combo(Player::P0, own));
        for (opp, &other) in reach[Player::P1].iter().enumerate() {
            if other <= 0.0 {
                continue;
            }
            let (c, d) = cards::combo_cards(hands.combo(Player::P1, opp));
            if a != c && a != d && b != c && b != d {
                return true;
            }
        }
    }
    false
}

/// Walks included subtrees carrying both players' reach, calling `visit` at
/// each action node with the same strategy used to propagate the reach.
///
/// `engine::reach_at` answers one node by re-walking the path to it, which
/// is the right shape for a viewer but quadratic when every node needs an
/// answer. Only the current path's reaches are alive at a time, so this
/// stays bounded by path depth in memory. `include` must retain all ancestors
/// of a retained node: a rejected child prunes its entire subtree before any
/// reach vector is allocated or mapped for it.
fn walk_reaches<F>(
    tree: &engine::PublicTree,
    node_id: NodeId,
    reach: &PerPlayer<Vec<f32>>,
    strategy: &dyn Fn(NodeId) -> Vec<f32>,
    include: &dyn Fn(NodeId) -> bool,
    visit: &mut F,
) where
    F: FnMut(NodeId, &PerPlayer<Vec<f32>>, &[f32]),
{
    let node = *tree.node(node_id);
    match node.kind {
        NodeKind::Terminal => {}
        NodeKind::Action => {
            let sref = tree.storage_ref(&node);
            let num_hands = sref.num_hands as usize;
            let sigma = strategy(node_id);
            visit(node_id, reach, &sigma);
            for (position, child) in tree.children(node_id).enumerate() {
                if !include(child) || tree.node(child).kind == NodeKind::Terminal {
                    continue;
                }
                let column = &sigma[position * num_hands..(position + 1) * num_hands];
                let mut next = reach.clone();
                for (r, &c) in next[node.player].iter_mut().zip(column) {
                    *r *= c;
                }
                walk_reaches(tree, child, &next, strategy, include, visit);
            }
        }
        NodeKind::Chance => {
            for (position, child) in tree.children(node_id).enumerate() {
                if !include(child) || tree.node(child).kind == NodeKind::Terminal {
                    continue;
                }
                let deal = *tree.deal(&node, position);
                let mut next = PerPlayer::new(Vec::new(), Vec::new());
                for player in Player::BOTH {
                    let map = deal.maps[player];
                    let len = tree.mapped_dim(map, reach[player].len() as u32) as usize;
                    let mut mapped = vec![0.0f32; len];
                    tree.map_reach_into(map, &reach[player], &mut mapped);
                    next[player] = mapped;
                }
                walk_reaches(tree, child, &next, strategy, include, visit);
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
/// `summary` supplies the final iteration count, root EV and exploitability.
/// Both players' per-node values are computed independently, since a raked
/// (general-sum) game's values need not sum to zero.
pub(crate) fn export_sol<S: Storage>(
    spec: &SolExportSpec,
    solver: &Solver<PostflopEvaluator, S>,
    ev_offset: PerPlayer<f64>,
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

    // Downstream river values still contribute to every retained trunk
    // node, but no-rivers need not keep copies of the river vectors.
    let include = |id: NodeId| mode == SolStreets::Full || streets[id as usize] != Street::River;
    let mut recorded = PerPlayer::new(
        solver.expected_values_where(Player::P0, include),
        solver.expected_values_where(Player::P1, include),
    );

    // Quantize while the current path's reach is alive. Consume each saved
    // CFV after use instead of retaining reaches/CFVs for the whole export.
    let strategy = |id: NodeId| solver.average_strategy_at(id);
    let roots = &solver.game().root_ranges;
    let mut blocks = Vec::new();
    let mut values = Vec::new();
    let mut visit = |id: NodeId, reach: &PerPlayer<Vec<f32>>, avg: &[f32]| {
        let node = tree.node(id);
        blocks.push(StrategyBlock {
            sref: node.aux,
            probs: quantize_probs(avg),
        });
        let mut per_node = Vec::with_capacity(reach[Player::P0].len() + reach[Player::P1].len());
        for player in Player::BOTH {
            // A counterfactual value is opponent-reach-weighted, so it has
            // to be divided by the reach that could be facing this hand
            // before a utility-valued offset can be added to it. Skipping that would
            // be a unit error, not a scaling one.
            let compatible = compatible_reach(
                solver.game().evaluator.hands(),
                player,
                &reach[player.opponent()],
            );
            // Every node uses the original subgame's utility baseline.
            // Adding this node's contributions would erase sunk wagers;
            // adding chips at all would mix units for ICM.
            let offset = ev_offset[player] as f32;
            let per_hand = recorded[player][node.aux as usize]
                .take()
                .expect("every retained action node is recorded by the value pass");
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
    };
    walk_reaches(tree, 0, roots, &strategy, &include, &mut visit);
    // Keep the on-disk sref ordering independent of tree traversal order.
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
        node_count: tree.nodes.len() as u64,
        blocks,
        values,
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
    /// Stored per-hand values, same keys as `blocks`: OOP's hands then
    /// IP's, on the subgame-start basis.
    pub values: HashMap<u32, Vec<f32>>,
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

    // An artifact embeds whatever config text produced it, so this reads
    // both the current families and the shapes older artifacts carry.
    let config = crate::config::parse_internal_config(&payload.config_toml)
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
        min_bet,
        preflop_aggressor,
        tree,
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
        min_bet,
        tree.lower()?,
        &preflop_aggressor,
    )?;
    let board_cards = pf_config.board.clone();
    let rake = crate::economics::build_rake(&config.rake)?;
    let utility = crate::economics::build_utility(&config.utility)?;

    // stderr, not stdout: `export` writes machine-readable data there, and
    // a progress line in the middle of a CSV would corrupt it.
    eprintln!("rebuilding tree from embedded config...");
    let build_start = Instant::now();
    let pipeline = PayoffPipeline {
        rake: rake.as_ref(),
        utility: utility.as_ref(),
    };
    let pf_game = build_postflop_game(&pf_config, pipeline);
    if payload.node_count != pf_game.game.tree.nodes.len() as u64 {
        bail!(
            "artifact node count {} does not match the rebuilt tree's {} nodes",
            payload.node_count,
            pf_game.game.tree.nodes.len(),
        );
    }
    eprintln!(
        "tree rebuilt in {:.2}s ({} nodes)",
        build_start.elapsed().as_secs_f64(),
        pf_game.game.tree.nodes.len(),
    );

    let start_street = start_street_from_board_len(pf_config.board.len());
    let streets = node_streets(&pf_game.game.tree, start_street);
    let parents = parent_array(&pf_game.game.tree);

    // Derive shapes from the rebuilt game, never from the untrusted block
    // lengths. Carry both private dimensions through chance maps: a value
    // block holds H0 + H1 entries, not twice the acting player's dimension.
    let tree = &pf_game.game.tree;
    let roots = &pf_game.game.root_ranges;
    let root_dims = PerPlayer::new(
        u32::try_from(roots[Player::P0].len()).context("OOP range dimension exceeds u32")?,
        u32::try_from(roots[Player::P1].len()).context("IP range dimension exceeds u32")?,
    );
    if root_dims != tree.root_dims {
        bail!("rebuilt tree's root dimensions do not match its ranges");
    }
    let mut expected = HashMap::new();
    let mut pending = vec![(0, root_dims)];
    while let Some((id, dims)) = pending.pop() {
        if payload.mode == StreetsStored::NoRivers && streets[id as usize] == Street::River {
            continue;
        }
        let node = tree.node(id);
        if node.kind == NodeKind::Action {
            let sref = tree.storage_ref(node);
            if sref.num_hands != dims[node.player] {
                bail!(
                    "rebuilt tree's private dimension disagrees at sref {}",
                    node.aux
                );
            }
            let strategy_bytes = usize::from(sref.num_actions)
                .checked_mul(usize::try_from(sref.num_hands)?)
                .and_then(|len| len.checked_mul(2))
                .context("strategy block length overflow")?;
            let value_len = usize::try_from(dims[Player::P0])?
                .checked_add(usize::try_from(dims[Player::P1])?)
                .context("value block length overflow")?;
            let value_bytes = value_len
                .checked_mul(2)
                .context("value block byte length overflow")?;
            expected.insert(node.aux, (strategy_bytes, value_len, value_bytes));
        }
        for (position, child) in tree.children(id).enumerate() {
            let child_dims = if node.kind == NodeKind::Chance {
                let deal = tree.deal(node, position);
                PerPlayer::new(
                    tree.mapped_dim(deal.maps[Player::P0], dims[Player::P0]),
                    tree.mapped_dim(deal.maps[Player::P1], dims[Player::P1]),
                )
            } else {
                dims
            };
            pending.push((child, child_dims));
        }
    }

    let stored_len = payload.blocks.len();
    let got: HashSet<u32> = payload.blocks.iter().map(|b| b.sref).collect();
    if got.len() != stored_len
        || got.len() != expected.len()
        || !got.iter().all(|sref| expected.contains_key(sref))
    {
        return Err(anyhow!(
            "artifact does not match the rebuilt tree: {stored_len} stored block(s) vs {} \
             expected action node(s) for mode {:?}",
            expected.len(),
            payload.mode,
        ));
    }

    let value_keys: HashSet<u32> = payload.values.iter().map(|b| b.sref).collect();
    if value_keys.len() != payload.values.len() || value_keys != got {
        bail!("artifact strategy and value blocks must cover exactly the same nodes");
    }
    for block in &payload.blocks {
        let (strategy_bytes, _, _) = expected[&block.sref];
        if block.probs.len() != strategy_bytes {
            bail!(
                "strategy block length mismatch for sref {}: expected {strategy_bytes} bytes, got {}",
                block.sref,
                block.probs.len(),
            );
        }
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
            let (_, len, value_bytes) = expected[&b.sref];
            if b.values.len() != value_bytes {
                bail!(
                    "value block length mismatch for sref {}: expected {value_bytes} bytes, got {}",
                    b.sref,
                    b.values.len(),
                );
            }
            if !b.scale.is_finite() || b.scale < 0.0 {
                bail!(
                    "value block scale must be finite and nonnegative for sref {}",
                    b.sref
                );
            }
            let restored = dequantize_values(&b.values, b.scale, len)
                .with_context(|| format!("decoding stored values for sref {}", b.sref))?;
            if restored.iter().any(|value| !value.is_finite()) {
                bail!("nonfinite decoded value for sref {}", b.sref);
            }
            Ok((b.sref, restored))
        })
        .collect::<Result<_>>()?;

    Ok(LoadedSol {
        pf_game,
        config: pf_config,
        rake,
        utility,
        meta: payload.meta,
        mode: payload.mode,
        blocks,
        values,
        streets,
        parents,
        board: board_cards,
        river_iterations,
        river_target,
    })
}

// --- research-only saved-profile evaluation --------------------------------

/// Identity of the immutable Full artifact used by a research audit.
#[derive(Debug, serde::Serialize)]
pub struct SavedProfileArtifact {
    pub path: PathBuf,
    pub blake3: String,
    pub bytes: u64,
    pub config_blake3: String,
    pub format_version: u16,
    pub mode: &'static str,
    pub iterations: u64,
    pub source_storage: String,
    pub stored_nodes: u64,
    pub node_count: u64,
}

/// Recomputed values for the stored, dequantized average policy, in OOP/IP order.
/// EV and BR use the same subgame-start utility basis. Gains are BR minus EV;
/// they are not clamped, and NashConv is their sum, also in general-sum games.
#[derive(Debug, serde::Serialize)]
pub struct SavedProfileValues {
    pub profile: &'static str,
    pub ev: [f64; 2],
    pub br: [f64; 2],
    pub gains: [f64; 2],
    pub nash_conv: f64,
}

/// Research report consumed by `examples/hu_saved_profile_audit.rs`.
/// This is separate from the main CLI/artifact contract. Stored value blocks
/// and pre-save metrics never supply the recomputed evaluation below.
#[derive(Debug, serde::Serialize)]
pub struct SavedProfileAudit {
    pub schema: &'static str,
    pub artifact: SavedProfileArtifact,
    pub threads: usize,
    pub par_chance_depth: u32,
    pub par_min_children: usize,
    pub pot_chips: u32,
    pub effective_stack_chips: u32,
    pub rake: crate::config::RakeSection,
    pub utility: crate::config::UtilitySection,
    pub zero_sum_terminal_utility: bool,
    pub value_basis: &'static str,
    pub ev_offset: [f64; 2],
    pub pre_save_metadata: SolMeta,
    pub recomputed: SavedProfileValues,
    /// Sum of the initial and final streaming file-hash passes.
    pub input_hash_secs: f64,
    /// Metadata preflight, full validated load, tree build and policy restore.
    pub load_secs: f64,
    /// Four value traversals: each seat's EV and BR, plus the common offset.
    pub eval_secs: f64,
}

fn hash_artifact(path: &Path) -> Result<(blake3::Hash, u64)> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("hashing saved profile {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hasher.update(&buffer[..len]);
        bytes += len as u64;
    }
    Ok((hasher.finalize(), bytes))
}

#[derive(Debug)]
struct ProfileEvaluation {
    ev: PerPlayer<f64>,
    br: PerPlayer<f64>,
    nash_conv: f64,
}

fn evaluate_profile<S: Storage>(solver: &Solver<PostflopEvaluator, S>) -> ProfileEvaluation {
    // Evaluate both seats independently, including in the zero-sum case.
    let ev = PerPlayer::new(
        solver.expected_value(Player::P0),
        solver.expected_value(Player::P1),
    );
    let br = PerPlayer::new(
        solver.best_response_value(Player::P0),
        solver.best_response_value(Player::P1),
    );
    ProfileEvaluation {
        ev,
        br,
        nash_conv: (br[Player::P0] - ev[Player::P0]) + (br[Player::P1] - ev[Player::P1]),
    }
}

/// Only the stored average policy is restored. Zero regrets are never stepped,
/// and absent river policies cannot turn into a uniform fallback. Keep this
/// mutable solver private: the research API exposes evaluation results only.
fn restore_full_artifact_profile(
    loaded: LoadedSol,
) -> Result<Solver<PostflopEvaluator, F32Storage>> {
    if loaded.mode != StreetsStored::Full {
        bail!("saved-profile evaluation requires a Full artifact");
    }
    let tree = &loaded.pf_game.game.tree;
    if loaded.blocks.len() != tree.storage_refs.len() {
        bail!("saved-profile evaluation requires every storage ref");
    }
    let mut strategy_sum = vec![0.0; tree.storage_len];
    for sref in &tree.storage_refs {
        let bytes = loaded
            .blocks
            .get(&sref.index)
            .context("missing saved policy")?;
        let policy = dequantize_probs(bytes, sref.num_actions as usize, sref.num_hands as usize)?;
        strategy_sum[sref.offset..sref.offset + sref.len()].copy_from_slice(&policy);
    }
    let state = engine::SolverState {
        iteration: loaded.meta.iterations,
        storage: engine::StorageState::F32 {
            regrets: vec![0.0; tree.storage_len],
            strategy_sum,
        },
    };
    let mut solver = Solver::new(
        loaded.pf_game.game,
        Box::<Dcfr>::default(),
        Some(state.iteration),
    );
    solver.restore_state(state)?;
    Ok(solver)
}

/// Re-evaluate a Full NLH HU postflop artifact's stored quantized profile.
/// Runs validation, rebuilding and evaluation inside a dedicated Rayon pool.
/// No training, river resolving, checkpoint state or stored EV reuse occurs.
/// Positive `threads` is mandatory; hashes before/after reject a changed input.
/// This computes unilateral gains against the fixed profile; it makes no
/// zero-sum exploitability or equilibrium-convergence claim for raked/ICM games.
pub fn audit_saved_full_profile(path: &Path, threads: usize) -> Result<SavedProfileAudit> {
    if threads == 0 {
        bail!("saved-profile audit threads must be positive");
    }
    postflop_setup::with_threads(Some(threads), || {
        let hash_start = Instant::now();
        let identity = hash_artifact(path)?;
        let mut input_hash_secs = hash_start.elapsed().as_secs_f64();
        let load_start = Instant::now();
        let metadata = formats::SolReader::open(path)?.metadata().clone();
        if metadata.mode != StreetsStored::Full {
            bail!("saved-profile evaluation requires a Full artifact");
        }
        let config = crate::config::parse_internal_config(&metadata.config_toml)?;
        let loaded = load_sol(path, 0, None)?;
        let offset = postflop_setup::subgame_ev_offset(&loaded.config, loaded.utility.as_ref());
        let pot_chips = loaded.config.pot.0;
        let effective_stack_chips = loaded.config.effective_stack.0;
        let zero_sum_terminal_utility = loaded.pf_game.game.zero_sum;
        let mut solver = restore_full_artifact_profile(loaded)?;
        postflop_setup::configure_solver(&mut solver, &config.run);
        let load_secs = load_start.elapsed().as_secs_f64();

        let eval_start = Instant::now();
        let evaluated = evaluate_profile(&solver);
        let ev = [
            evaluated.ev[Player::P0] + offset[Player::P0],
            evaluated.ev[Player::P1] + offset[Player::P1],
        ];
        let br = [
            evaluated.br[Player::P0] + offset[Player::P0],
            evaluated.br[Player::P1] + offset[Player::P1],
        ];
        let gains = [br[0] - ev[0], br[1] - ev[1]];
        let nash_conv = gains.iter().sum::<f64>();
        if ev
            .into_iter()
            .chain(br)
            .chain(gains)
            .chain([nash_conv, evaluated.nash_conv])
            .any(|value| !value.is_finite())
        {
            bail!("saved-profile evaluation produced nonfinite values");
        }
        let eval_secs = eval_start.elapsed().as_secs_f64();
        let hash_start = Instant::now();
        if hash_artifact(path)? != identity {
            bail!("saved-profile artifact changed during the audit");
        }
        input_hash_secs += hash_start.elapsed().as_secs_f64();
        Ok(SavedProfileAudit {
            schema: "solvers.research.hu-saved-profile-audit/v1",
            artifact: SavedProfileArtifact {
                path: path.to_owned(),
                blake3: identity.0.to_hex().to_string(),
                bytes: identity.1,
                config_blake3: formats::config_hash_hex(&formats::config_hash(
                    metadata.config_toml.as_bytes(),
                )),
                format_version: formats::SOL_FORMAT_VERSION,
                mode: "full",
                iterations: solver.iteration(),
                source_storage: metadata.meta.storage.clone(),
                stored_nodes: metadata.stored_nodes,
                node_count: metadata.node_count,
            },
            threads,
            par_chance_depth: config.run.par_chance_depth.unwrap_or(2),
            par_min_children: config.run.par_min_children.unwrap_or(12),
            pot_chips,
            effective_stack_chips,
            rake: config.rake,
            utility: config.utility,
            zero_sum_terminal_utility,
            value_basis: "subgame_start_utility",
            ev_offset: [offset[Player::P0], offset[Player::P1]],
            pre_save_metadata: metadata.meta,
            recomputed: SavedProfileValues {
                profile: "stored_quantized",
                ev,
                br,
                gains,
                nash_conv,
            },
            input_hash_secs,
            load_secs,
            eval_secs,
        })
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
pub(crate) struct LiveProvider<'a, S: Storage> {
    pub solver: &'a Solver<PostflopEvaluator, S>,
    /// Re-bases the root EV on the start of the subgame (see
    /// [`crate::postflop_setup::subgame_ev_offset`]).
    pub ev_offset: PerPlayer<f64>,
}

impl<S: Storage> StrategyProvider for LiveProvider<'_, S> {
    fn average_strategy(&mut self, node: NodeId) -> Result<Vec<f32>> {
        Ok(self.solver.average_strategy_at(node))
    }

    fn ev_line(&mut self) -> String {
        let solver = self.solver;
        let ev = crate::postflop_setup::subgame_ev(crate::solve::solver_ev(solver), self.ev_offset);
        let (ev_oop, ev_ip) = (ev[Player::P0], ev[Player::P1]);
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

        // Positive ranges can still have no compatible private deal. Surface
        // an unreachable line as a query error before the builder's assert.
        if !has_compatible_reach(loaded.pf_game.game.evaluator.hands(), &reach) {
            bail!(
                "cannot re-solve river at {history:?}: no compatible private deal reaches this line"
            );
        }

        let state = river_entry_state(&loaded.config, &history)
            .with_context(|| format!("replaying history {history:?} for river entry {entry}"))?;
        let sub_cfg = river_resolve_config(
            &loaded.config,
            loaded.pf_game.game.evaluator.hands(),
            &state,
            &reach,
        );

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
        let sub_hands = rs.solver.game().evaluator.hands();
        let trunk_hands = loaded.pf_game.game.evaluator.hands();
        let sub_strategy = rs.solver.average_strategy_at(sub_node);
        let actions = sref.num_actions as usize;
        let trunk_len = trunk_hands.len(n.player);
        let sub_len = sub_hands.len(n.player);
        // A fresh river range omits blocked or zero-reach trunk hands. Give
        // those unreachable columns a valid distribution, then restore every
        // retained column to its original trunk combo index.
        let mut result = vec![1.0 / actions as f32; actions * trunk_len];
        for (sub_index, &combo) in sub_hands.combos(n.player).iter().enumerate() {
            let trunk_index = trunk_hands.index(n.player, combo as usize).ok_or_else(|| {
                anyhow!("river re-solve introduced combo {combo} outside the trunk range")
            })?;
            for action in 0..actions {
                result[action * trunk_len + trunk_index] =
                    sub_strategy[action * sub_len + sub_index];
            }
        }
        Ok(result)
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
    use holdem::PostflopNodeInfo;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn export_sol<S: Storage>(
        spec: &SolExportSpec,
        solver: &Solver<PostflopEvaluator, S>,
        _node_info: &[PostflopNodeInfo],
        start_street: Street,
        summary: &RunSummary,
    ) -> Result<()> {
        let internal = crate::solve::solver_ev(solver);
        let offset = PerPlayer::new(
            summary.ev[Player::P0] - internal[Player::P0],
            summary.ev[Player::P1] - internal[Player::P1],
        );
        super::export_sol(spec, solver, offset, start_street, summary)
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
    /// "33,66", pot 2, stack 20, turn [0.75], river [1.0], max_aggressive_actions 1/1),
    /// copied from `crates/holdem/tests/viewer.rs`'s `small_turn_config`: a
    /// single chance node whose children are exactly the river-entry nodes,
    /// small enough to build/solve/re-solve fast even in a debug build.
    const TINY_TURN_TOML: &str = r#"
schema = "solvers.postflop/v1"

[game]
board = "2s 7s Ks 2h"
oop_range = "44,55"
ip_range = "33,66"
pot = 2
effective_stack = 20

[game.tree]
kind = "script"
script = '''
turn { replace bet [75] }
river { replace bet [100] }
'''

[game.tree.max_aggressive_actions]
turn = 1
river = 1

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
board = "2s 7s Ks 2h 9d"
oop_range = "44,55"
ip_range = "33,66"
pot = 2
effective_stack = 20

[game.tree]
kind = "script"
script = '''
river { replace bet [100] }
'''

[game.tree.max_aggressive_actions]
river = 1

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
    ) -> (
        Solver<PostflopEvaluator, S>,
        Vec<PostflopNodeInfo>,
        Street,
        RunSummary,
    ) {
        // The fixtures are source configs, so they go through the contract
        // parser rather than the internal shape.
        let config = crate::solver_config_v1::parse_and_lower(raw).expect("parse fixture toml");
        let GameSection::Postflop {
            board,
            oop_range,
            ip_range,
            pot,
            effective_stack,
            iso_merging,
            min_bet,
            preflop_aggressor,
            tree,
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
            min_bet,
            tree.lower().expect("lower tree script"),
            &preflop_aggressor,
        )
        .expect("build postflop config");
        let start_street = start_street_from_board_len(pf_config.board.len());
        let rake = crate::economics::build_rake(&config.rake).expect("fixture rake");
        let utility = crate::economics::build_utility(&config.utility).expect("fixture utility");
        let pipeline = PayoffPipeline {
            rake: rake.as_ref(),
            utility: utility.as_ref(),
        };
        let ev_offset = crate::postflop_setup::subgame_ev_offset(&pf_config, pipeline.utility);
        let pf_game = build_postflop_game(&pf_config, pipeline);
        let node_info = pf_game.node_info.clone();

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
            ev: crate::postflop_setup::subgame_ev(crate::solve::solver_ev(&solver), ev_offset),
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
    fn compatible_river_support_has_no_epsilon_and_rejects_card_collisions() {
        let ranges = PerPlayer::new("AsAh,AcAd".parse().unwrap(), "AsKh,KcKd".parse().unwrap());
        let hands = PostflopHands::from_ranges(&[], &ranges);
        let at = |player, a: &str, b: &str| {
            hands
                .index(
                    player,
                    cards::combo_index(a.parse().unwrap(), b.parse().unwrap()),
                )
                .unwrap()
        };
        let mut reach = PerPlayer::new(
            vec![0.0; hands.len(Player::P0)],
            vec![0.0; hands.len(Player::P1)],
        );
        reach[Player::P0][at(Player::P0, "As", "Ah")] = f32::MIN_POSITIVE;
        reach[Player::P1][at(Player::P1, "Kc", "Kd")] = f32::MIN_POSITIVE;
        assert!(has_compatible_reach(&hands, &reach));
        reach[Player::P1].fill(0.0);
        reach[Player::P1][at(Player::P1, "As", "Kh")] = 1.0;
        assert!(!has_compatible_reach(&hands, &reach));
    }

    #[test]
    fn lazy_river_restores_trunk_columns_after_board_removal() {
        let (solver, info, street, summary) = build_and_solve::<F32Storage>(TINY_TURN_TOML, 8);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lazy-columns.sol");
        export_sol(
            &SolExportSpec {
                path: path.clone(),
                mode: SolStreets::NoRivers,
                config_toml: TINY_TURN_TOML.into(),
                storage_name: "f32".into(),
            },
            &solver,
            &info,
            street,
            &summary,
        )
        .unwrap();
        let loaded = load_sol(&path, 8, None).unwrap();
        let tree = &loaded.pf_game.game.tree;
        let ranges = &loaded.pf_game.game.root_ranges;
        let roots = PerPlayer::new(ranges[Player::P0].as_slice(), ranges[Player::P1].as_slice());
        let entry = (0..tree.nodes.len() as NodeId)
            .find(|&id| {
                let node = tree.node(id);
                if node.kind != NodeKind::Action
                    || loaded.streets[id as usize] != Street::River
                    || loaded.parents[id as usize] == NodeId::MAX
                    || tree.node(loaded.parents[id as usize]).kind != NodeKind::Chance
                {
                    return false;
                }
                let reach = reach_at(tree, roots, id, |_, sref, out| {
                    out.copy_from_slice(
                        &dequantize_probs(
                            &loaded.blocks[&sref.index],
                            sref.num_actions as usize,
                            sref.num_hands as usize,
                        )
                        .unwrap(),
                    );
                });
                reach[node.player].contains(&0.0)
                    && has_compatible_reach(loaded.pf_game.game.evaluator.hands(), &reach)
            })
            .expect("fixture must include a reachable river that removes an acting hand");
        let mut provider = SolProvider::new(&loaded);
        let restored = provider.average_strategy(entry).unwrap();
        let fresh = &provider.river_solves[&entry];
        let node = tree.node(entry);
        let sref = tree.storage_ref(node);
        let trunk_hands = loaded.pf_game.game.evaluator.hands();
        let sub_hands = fresh.solver.game().evaluator.hands();
        let sub = fresh.solver.average_strategy_at(fresh.map[&entry]);
        assert!(sub_hands.len(node.player) < trunk_hands.len(node.player));
        assert_eq!(restored.len(), sref.len());
        for (local, &combo) in trunk_hands.combos(node.player).iter().enumerate() {
            for action in 0..sref.num_actions as usize {
                let expected = sub_hands
                    .index(node.player, combo as usize)
                    .map_or(1.0 / sref.num_actions as f32, |i| {
                        sub[action * sub_hands.len(node.player) + i]
                    });
                assert_eq!(
                    restored[action * sref.num_hands as usize + local].to_bits(),
                    expected.to_bits()
                );
            }
        }
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

    /// Root-zero hands are omitted, while later board-blocked hands retain
    /// their root index and carry a zero saved value.
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
        let ranges = &solver.game().root_ranges;
        assert_eq!(ranges[Player::P0].len(), 12);
        assert_eq!(ranges[Player::P1].len(), 12);
        assert!(ranges[Player::P0].iter().all(|&weight| weight > 0.0));
        let roots = PerPlayer::new(ranges[Player::P0].as_slice(), ranges[Player::P1].as_slice());
        let mut zeros_checked = 0;
        for id in 0..tree.nodes.len() as NodeId {
            let node = tree.node(id);
            if node.kind != NodeKind::Action {
                continue;
            }
            let block = payload
                .values
                .iter()
                .find(|block| block.sref == node.aux)
                .unwrap();
            let reach = reach_at(tree, roots, id, |aid, _, out| {
                out.copy_from_slice(&solver.average_strategy_at(aid))
            });
            let stored = dequantize_values(
                &block.values,
                block.scale,
                reach[Player::P0].len() + reach[Player::P1].len(),
            )
            .unwrap();
            for seat in Player::BOTH {
                let base = if seat == Player::P0 {
                    0
                } else {
                    reach[Player::P0].len()
                };
                for (hand, &weight) in reach[seat].iter().enumerate() {
                    if weight == 0.0 {
                        assert_eq!(stored[base + hand], 0.0, "{seat:?} node {id} hand {hand}");
                        zeros_checked += 1;
                    }
                }
            }
        }
        assert!(zeros_checked > 0);
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
        let raw = TINY_RIVER_TOML.replace("ip_range = \"33,66\"", "ip_range = \"33,66,88\"");
        let (solver, node_info, start_street, summary) = build_and_solve::<F32Storage>(&raw, 256);
        let path = temp_path("value-aggregate.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::Full,
            config_toml: raw,
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
        let hands = solver.game().evaluator.hands();
        assert_ne!(hands.len(Player::P0), hands.len(Player::P1));
        let stored = dequantize_values(
            &block.values,
            block.scale,
            hands.len(Player::P0) + hands.len(Player::P1),
        )
        .expect("decode values");

        for seat in Player::BOTH {
            let own = &solver.game().root_ranges[seat];
            let facing = compatible_reach(
                solver.game().evaluator.hands(),
                seat,
                &solver.game().root_ranges[seat.opponent()],
            );
            let base = if seat == Player::P0 {
                0
            } else {
                hands.len(Player::P0)
            };
            let mut weighted = 0.0f64;
            let mut total = 0.0f64;
            for hand in 0..hands.len(seat) {
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

    /// Point queries rebuild each selected node's reach independently and
    /// evaluate its subtree. Their quantized bytes must match the batched
    /// export, including unreachable hands and the common trunk of both modes.
    fn assert_export_matches_node_queries<S: Storage>() {
        let (solver, _node_info, start_street, summary) = build_and_solve::<S>(TINY_TURN_TOML, 1);
        let internal = crate::solve::solver_ev(&solver);
        let offset = PerPlayer::new(
            summary.ev[Player::P0] - internal[Player::P0],
            summary.ev[Player::P1] - internal[Player::P1],
        );
        let tree = &solver.game().tree;
        let streets = node_streets(tree, start_street);
        let full_path = temp_path("point-query-full.sol");
        let mut spec = SolExportSpec {
            path: full_path.clone(),
            mode: SolStreets::Full,
            config_toml: TINY_TURN_TOML.to_string(),
            storage_name: "f32".into(),
        };
        super::export_sol(&spec, &solver, offset, start_street, &summary).expect("full export");
        let full = read_sol(&full_path).expect("full payload");
        let _ = std::fs::remove_file(&full_path);
        let roots = &solver.game().root_ranges;
        let root_slices =
            PerPlayer::new(roots[Player::P0].as_slice(), roots[Player::P1].as_slice());
        let mut nodes: Vec<_> = (0..tree.nodes.len() as NodeId)
            .filter(|&id| tree.node(id).kind == NodeKind::Action)
            .collect();
        nodes.sort_by_key(|&id| tree.node(id).aux);
        assert_eq!(full.blocks.len(), nodes.len());
        assert_eq!(full.values.len(), nodes.len());
        let mut retained = vec![false; tree.storage_refs.len()];
        for ((id, block), value_block) in nodes.iter().zip(&full.blocks).zip(&full.values) {
            let id = *id;
            let sref = tree.node(id).aux;
            assert_eq!(block.sref, sref);
            assert_eq!(value_block.sref, sref);
            assert_eq!(block.probs, quantize_probs(&solver.average_strategy_at(id)));
            let reach = reach_at(tree, root_slices, id, |source, _, out| {
                out.copy_from_slice(&solver.average_strategy_at(source));
            });
            let reach_slices =
                PerPlayer::new(reach[Player::P0].as_slice(), reach[Player::P1].as_slice());
            let mut expected = Vec::new();
            for player in Player::BOTH {
                let cfvs = solver.expected_values_at(id, player, reach_slices);
                let facing = compatible_reach(
                    solver.game().evaluator.hands(),
                    player,
                    &reach[player.opponent()],
                );
                expected.extend(cfvs.iter().zip(&facing).zip(&reach[player]).map(
                    |((&cfv, &opponent), &own)| {
                        if own > 0.0 && opponent > 0.0 {
                            cfv / opponent + offset[player] as f32
                        } else {
                            0.0
                        }
                    },
                ));
            }
            let (scale, bytes) = quantize_values(&expected);
            assert_eq!(value_block.scale.to_bits(), scale.to_bits(), "node {id}");
            assert_eq!(value_block.values, bytes, "node {id}");
            retained[sref as usize] = streets[id as usize] != Street::River;
        }

        let trunk_path = temp_path("point-query-trunk.sol");
        spec.path = trunk_path.clone();
        spec.mode = SolStreets::NoRivers;
        super::export_sol(&spec, &solver, offset, start_street, &summary).expect("trunk export");
        let trunk = read_sol(&trunk_path).expect("trunk payload");
        let _ = std::fs::remove_file(&trunk_path);
        assert_eq!(
            trunk.blocks,
            full.blocks
                .into_iter()
                .filter(|block| retained[block.sref as usize])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            trunk.values,
            full.values
                .into_iter()
                .filter(|block| retained[block.sref as usize])
                .collect::<Vec<_>>()
        );

        // No-rivers must never request a river strategy for reach propagation.
        let include = |id: NodeId| streets[id as usize] != Street::River;
        let strategy = |id: NodeId| {
            assert!(include(id));
            solver.average_strategy_at(id)
        };
        let mut visited = 0;
        walk_reaches(tree, 0, roots, &strategy, &include, &mut |_, _, _| {
            visited += 1;
        });
        assert_eq!(visited, trunk.blocks.len());
        assert!(visited < nodes.len());
    }

    #[test]
    fn exported_values_match_point_queries_f32() {
        assert_export_matches_node_queries::<F32Storage>();
    }

    #[test]
    fn exported_values_match_point_queries_i16() {
        assert_export_matches_node_queries::<engine::I16Storage>();
    }

    /// A priori fixture tolerance, independent of the observed EV difference.
    /// Rounding A probabilities to multiples of 1/D has L1 error <= A/(2D);
    /// renormalization gives TV <= A/(2D-A). A coupling over at most L action
    /// decisions bounds EV drift by payoff_span * L * TV. The same bound
    /// applies to each fixed response, hence also their maximum (BR).
    fn saved_profile_value_tolerance(tree: &engine::PublicTree, payoff_span: f64) -> f64 {
        let max_actions = tree
            .storage_refs
            .iter()
            .map(|sref| sref.num_actions)
            .max()
            .unwrap() as f64;
        let mut longest = 0;
        let mut pending = vec![(0, 0u32)];
        while let Some((id, depth)) = pending.pop() {
            let depth = depth + u32::from(tree.node(id).kind == NodeKind::Action);
            longest = longest.max(depth);
            pending.extend(tree.children(id).map(|child| (child, depth)));
        }
        let epsilon = f32::EPSILON as f64;
        // Cover f32 probability conversion/normalization in both profiles.
        let node_tv = max_actions / (2.0 * 65_535.0 - max_actions) + 4.0 * max_actions * epsilon;
        // Fixed numerical allowance for these small chip-EV fixtures' f32
        // terminal/chance reductions; not a bound for arbitrary game sizes.
        payoff_span * (f64::from(longest) * node_tv + 256.0 * epsilon)
    }

    fn assert_saved_full_profile_is_evaluated<S: Storage>(raw: &str, storage: &str) {
        let (source, node_info, start_street, summary) = build_and_solve::<S>(raw, 3);
        let before_quantization = evaluate_profile(&source);
        let directory = tempfile::tempdir().unwrap();
        let mut spec = SolExportSpec {
            path: directory.path().join("saved-profile.sol"),
            mode: SolStreets::Full,
            config_toml: raw.to_owned(),
            storage_name: storage.to_owned(),
        };
        export_sol(&spec, &source, &node_info, start_street, &summary).unwrap();
        let loaded = load_sol(&spec.path, 0, None).unwrap();
        let offset = postflop_setup::subgame_ev_offset(&loaded.config, loaded.utility.as_ref());
        let payoff_span =
            f64::from(loaded.config.pot.0) + 2.0 * f64::from(loaded.config.effective_stack.0);
        let value_tolerance = saved_profile_value_tolerance(&loaded.pf_game.game.tree, payoff_span);

        // Metadata remains an exact record of the live, pre-quantization
        // solve. It is not used as the saved profile's evaluation result.
        assert_eq!(
            loaded.meta.ev,
            [summary.ev[Player::P0], summary.ev[Player::P1]]
        );
        assert_eq!(loaded.meta.expl, [summary.expl_p0, summary.expl_p1]);
        assert_eq!(loaded.meta.nash_conv, summary.nash_conv);
        for player in Player::BOTH {
            assert_eq!(
                loaded.meta.ev[player.index()],
                before_quantization.ev[player] + offset[player]
            );
            assert!(
                (loaded.meta.expl[player.index()]
                    - (before_quantization.br[player] - before_quantization.ev[player]))
                    .abs()
                    <= value_tolerance
            );
        }
        drop(loaded);
        let audit = audit_saved_full_profile(&spec.path, 1).unwrap();
        assert_eq!(audit.artifact.iterations, source.iteration());
        assert_eq!(audit.artifact.source_storage, storage);
        assert_eq!(audit.artifact.format_version, formats::SOL_FORMAT_VERSION);
        assert_eq!(audit.artifact.mode, "full");
        assert_eq!(audit.threads, 1);
        assert_eq!(
            audit.pre_save_metadata.ev,
            [summary.ev[Player::P0], summary.ev[Player::P1]]
        );
        assert_eq!(audit.ev_offset, [offset[Player::P0], offset[Player::P1]]);
        assert_eq!(audit.recomputed.profile, "stored_quantized");
        let after_quantization = &audit.recomputed;
        for player in Player::BOTH {
            assert!(
                (after_quantization.ev[player.index()]
                    - before_quantization.ev[player]
                    - offset[player])
                    .abs()
                    <= value_tolerance,
                "{storage} {start_street:?} {player:?}: EV drift exceeds {value_tolerance}"
            );
            assert!(
                (after_quantization.br[player.index()]
                    - before_quantization.br[player]
                    - offset[player])
                    .abs()
                    <= value_tolerance,
                "{storage} {start_street:?} {player:?}: BR drift exceeds {value_tolerance}"
            );
            assert_eq!(
                after_quantization.gains[player.index()],
                after_quantization.br[player.index()] - after_quantization.ev[player.index()]
            );
        }
        assert_eq!(
            after_quantization.nash_conv,
            after_quantization.gains.iter().sum::<f64>()
        );
        assert!(
            (after_quantization.nash_conv - before_quantization.nash_conv).abs()
                <= 4.0 * value_tolerance
        );
        println!(
            "saved-profile {storage} {start_street:?}: before={before_quantization:?}, after={after_quantization:?}, value_tolerance={value_tolerance}"
        );

        if start_street == Street::Turn {
            spec.mode = SolStreets::NoRivers;
            export_sol(&spec, &source, &node_info, start_street, &summary).unwrap();
            let trunk = load_sol(&spec.path, 0, None).unwrap();
            assert!(trunk.blocks.len() < trunk.pf_game.game.tree.storage_refs.len());
            drop(trunk);
            let error =
                audit_saved_full_profile(&spec.path, 1).expect_err("incomplete profile must fail");
            assert!(error.to_string().contains("requires a Full artifact"));
        }
    }

    #[test]
    fn full_artifact_profiles_are_reevaluated_after_u16_quantization() {
        let raked_river =
            format!("{TINY_RIVER_TOML}\n[rake]\nkind = \"percent-cap\"\nrate = 0.125\ncap = 1.5\n");
        for raw in [raked_river.as_str(), TINY_TURN_TOML] {
            assert_saved_full_profile_is_evaluated::<F32Storage>(raw, "f32");
            assert_saved_full_profile_is_evaluated::<I16Storage>(raw, "i16");
        }
    }

    #[test]
    fn saved_profile_audit_ignores_recorded_values_and_rejects_other_games() {
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_RIVER_TOML, 3);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audit.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::Full,
            config_toml: TINY_RIVER_TOML.to_owned(),
            storage_name: "f32".to_owned(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).unwrap();
        let original = audit_saved_full_profile(&path, 1).unwrap();
        let mut payload = read_sol(&path).unwrap();
        payload.meta.ev = [1234.0, 5678.0];
        payload.meta.expl = [10.0, 20.0];
        payload.meta.nash_conv = 30.0;
        for value in &mut payload.values {
            value.scale = 0.0;
            value.values.fill(0);
        }
        write_sol(&path, &payload).unwrap();
        let audit = audit_saved_full_profile(&path, 2).unwrap();
        assert_ne!(audit.artifact.blake3, original.artifact.blake3);
        assert_eq!(
            audit.artifact.config_blake3,
            original.artifact.config_blake3
        );
        assert_eq!(audit.artifact.iterations, 3);
        assert_eq!(audit.threads, 2);
        assert_eq!(audit.pre_save_metadata, payload.meta);
        assert_eq!(audit.recomputed.ev, original.recomputed.ev);
        assert_eq!(audit.recomputed.br, original.recomputed.br);
        assert_eq!(audit.recomputed.gains, original.recomputed.gains);
        assert_eq!(audit.recomputed.nash_conv, original.recomputed.nash_conv);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(audit.artifact.bytes, bytes.len() as u64);
        assert_eq!(
            audit.artifact.blake3,
            blake3::hash(&bytes).to_hex().to_string()
        );
        for seconds in [audit.input_hash_secs, audit.load_secs, audit.eval_secs] {
            assert!(seconds.is_finite() && seconds >= 0.0);
        }
        let json = serde_json::to_value(&audit).unwrap();
        assert_eq!(json["recomputed"]["profile"], "stored_quantized");
        assert_eq!(json["pre_save_metadata"]["ev"][0], 1234.0);
        let error = audit_saved_full_profile(&path, 0).unwrap_err();
        assert!(error.to_string().contains("threads must be positive"));

        payload.config_toml =
            "schema = \"solvers.toy/v1\"\n[game]\nkind = \"kuhn\"\n[run]\niterations = 3\n"
                .to_owned();
        write_sol(&path, &payload).unwrap();
        let error = audit_saved_full_profile(&path, 1).unwrap_err();
        assert!(
            error.to_string().contains("not kind = \"postflop\""),
            "{error:#}"
        );
    }

    #[test]
    fn load_rejects_metadata_node_count_disagreement() {
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_RIVER_TOML, 0);
        let path = temp_path("node-count.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::Full,
            config_toml: TINY_RIVER_TOML.to_string(),
            storage_name: "f32".into(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");
        let mut payload = read_sol(&path).expect("payload");
        let _ = std::fs::remove_file(&path);
        payload.node_count += 1;
        let corrupt_path = temp_path("node-count-mismatch.sol");
        write_sol(&corrupt_path, &payload).expect("mismatched metadata");
        let error = load_sol(&corrupt_path, 0, None)
            .err()
            .expect("node count mismatch must fail");
        let _ = std::fs::remove_file(&corrupt_path);
        assert!(error.to_string().contains("node count"), "{error}");
    }

    #[test]
    fn load_rejects_reencoded_block_shape_and_scale_errors() {
        let (solver, node_info, start_street, summary) =
            build_and_solve::<F32Storage>(TINY_RIVER_TOML, 0);
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("block-shape.sol");
        let spec = SolExportSpec {
            path: path.clone(),
            mode: SolStreets::Full,
            config_toml: TINY_RIVER_TOML.to_string(),
            storage_name: "f32".into(),
        };
        export_sol(&spec, &solver, &node_info, start_street, &summary).expect("export");
        let valid = read_sol(&path).expect("valid payload");
        load_sol(&path, 0, None).expect("valid shapes and zero scales are accepted");

        for (damage, expected_error) in [
            ("short-strategy", "strategy block length mismatch"),
            ("short-values", "value block length mismatch"),
            ("nan-scale", "scale must be finite and nonnegative"),
            ("infinite-scale", "scale must be finite and nonnegative"),
            ("negative-scale", "scale must be finite and nonnegative"),
            ("overflow-values", "nonfinite decoded value"),
            ("missing-node", "artifact does not match the rebuilt tree"),
        ] {
            let mut payload = valid.clone();
            match damage {
                "short-strategy" => {
                    let probs = &mut payload.blocks[0].probs;
                    probs.truncate(probs.len() - 2);
                }
                "short-values" => {
                    let values = &mut payload.values[0].values;
                    values.truncate(values.len() - 2);
                }
                "nan-scale" => payload.values[0].scale = f32::NAN,
                "infinite-scale" => payload.values[0].scale = f32::INFINITY,
                "negative-scale" => payload.values[0].scale = -1.0,
                "overflow-values" => {
                    payload.values[0].scale = f32::MAX;
                    payload.values[0].values[..2].copy_from_slice(&2i16.to_le_bytes());
                }
                "missing-node" => {
                    payload.blocks.pop();
                    payload.values.pop();
                }
                _ => unreachable!(),
            }
            // Re-encode through the real indexed writer: lengths/checksums and
            // frame identities are valid, but the semantic block is not.
            write_sol(&path, &payload).expect("reencode with valid frame checksums");
            read_sol(&path).expect("format framing itself remains valid");
            let error = load_sol(&path, 0, None)
                .err()
                .unwrap_or_else(|| panic!("{damage} must fail during full load"));
            assert!(
                error.to_string().contains(expected_error),
                "{damage}: {error}"
            );
            let audit_error = audit_saved_full_profile(&path, 1).unwrap_err();
            assert!(
                audit_error.to_string().contains(expected_error),
                "saved-profile audit accepted {damage}: {audit_error}"
            );
        }
    }
}
