//! Copy this unchanged into crates/cli/examples in each retained source copy.
//! Research only: no CFR iterations, resume, production API, or cross-version read.
//! Census files compare semantic inputs and public topology. Storage diagnostics
//! and checkpoint state hashes are layout-specific and are NOT cross-arm equality.

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use cards::Player;
use clap::{Parser, ValueEnum};
use cli::config::{GameSection, SolveConfig};
use cli::{economics, postflop_setup};
use engine::{NodeKind, ReachMap, StorageState};
use game::PayoffPipeline;
use holdem::{PostflopConfig, build_postflop_game};
use serde_json::{Value, json};

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Census,
    Checkpoint,
}

#[derive(Parser)]
struct Args {
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    checkpoint: Option<PathBuf>,
    /// Must not exist. Partial output from an error must not be used.
    #[arg(long)]
    out: PathBuf,
}

fn new_file(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(
        OpenOptions::new().write(true).create_new(true).open(path)?,
    ))
}

fn json_line(output: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    Ok(())
}

fn file_identity(path: &Path) -> Result<Value> {
    let mut input = BufReader::new(File::open(path)?);
    let mut hash = blake3::Hasher::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok(json!({"bytes": bytes, "blake3": hash.finalize().to_hex().to_string()}))
}

fn postflop(config: &SolveConfig) -> Result<PostflopConfig> {
    let GameSection::Postflop {
        board,
        oop_range,
        ip_range,
        pot,
        effective_stack,
        min_bet,
        iso_merging,
        preflop_aggressor,
        tree,
    } = &config.game
    else {
        bail!("census only supports postflop");
    };
    ensure!(!iso_merging, "census requires iso_merging = false");
    postflop_setup::build_postflop_config(
        board,
        oop_range,
        ip_range,
        *pot,
        *effective_stack,
        *iso_merging,
        *min_bet,
        tree.lower()?,
        preflop_aggressor,
    )
}

fn census(config_path: &Path, out: &Path) -> Result<Value> {
    let raw = fs::read_to_string(config_path)?;
    let config = cli::config::parse_solve_config_at(&raw, config_path)?;
    ensure!(config.run.threads == Some(1), "census requires threads = 1");
    let pf = postflop(&config)?;
    let rake = economics::build_rake(&config.rake)?;
    let utility = economics::build_utility(&config.utility)?;
    let built = build_postflop_game(
        &pf,
        PayoffPipeline {
            rake: &*rake,
            utility: &*utility,
        },
    );
    let game = &built.game;
    let tree = &game.tree;
    ensure!(
        game.normalizer.is_finite() && game.normalizer > 0.0,
        "invalid normalizer"
    );

    // Both releases accept global 1,326-combo input ranges. The compact layout
    // contract retains positive, unblocked combos in ascending global order.
    // Validate root values and dimensions before using that documented mapping.
    let mut support = [Vec::<usize>::new(), Vec::<usize>::new()];
    let mut slots = [Vec::<usize>::new(), Vec::<usize>::new()];
    let mut input_seats = Vec::new();
    for (seat, player) in [Player::P0, Player::P1].into_iter().enumerate() {
        let weights = pf.ranges[player].weights();
        ensure!(weights.len() == 1326, "unexpected global input dimension");
        let mut rows = Vec::with_capacity(1326);
        for (combo, &weight) in weights.iter().enumerate() {
            ensure!(weight.is_finite() && weight >= 0.0, "invalid input weight");
            let (a, b) = cards::combo_cards(combo);
            let blocked = pf.board.contains(&a) || pf.board.contains(&b);
            let root = if blocked { 0.0 } else { weight };
            rows.push(json!([
                combo,
                a.index(),
                b.index(),
                weight.to_bits(),
                root.to_bits()
            ]));
            if root > 0.0 {
                support[seat].push(combo);
            }
        }
        let dims = tree.root_dims[player] as usize;
        ensure!(
            game.root_ranges[player].len() == dims,
            "root reach dimension mismatch"
        );
        if dims == 1326 {
            slots[seat] = support[seat].clone();
            for (combo, &actual) in game.root_ranges[player].iter().enumerate() {
                let (a, b) = cards::combo_cards(combo);
                let expected = if pf.board.contains(&a) || pf.board.contains(&b) {
                    0.0
                } else {
                    weights[combo]
                };
                ensure!(
                    actual.to_bits() == expected.to_bits(),
                    "dense root reach mismatch"
                );
            }
        } else {
            ensure!(
                dims == support[seat].len(),
                "unrecognized compact dimension"
            );
            slots[seat] = (0..dims).collect();
        }
        for (&combo, &slot) in support[seat].iter().zip(&slots[seat]) {
            ensure!(
                game.root_ranges[player][slot].to_bits() == weights[combo].to_bits(),
                "root support mapping mismatch"
            );
        }
        input_seats.push(json!({"seat": seat, "columns": ["global_combo", "card0", "card1", "input_f32_bits", "board_filtered_f32_bits"], "rows": rows}));
    }
    let input_path = out.join("input.json");
    let mut input = new_file(&input_path)?;
    json_line(
        &mut input,
        &json!({
            "schema": "solvers.hu-pipeline-census-input/v1",
            "normalized_config": config,
            "board": pf.board.iter().map(|card| card.index()).collect::<Vec<_>>(),
            "normalizer_f64_bits": game.normalizer.to_bits(),
            "zero_sum": game.zero_sum,
            "seats": input_seats,
        }),
    )?;
    input.flush()?;

    let tree_path = out.join("tree.jsonl");
    let mut output = new_file(&tree_path)?;
    json_line(
        &mut output,
        &json!({
            "schema": "solvers.hu-pipeline-census-tree/v1",
            "nodes": tree.nodes.len(), "deals": tree.deals.len(),
            "support_global_combo": support,
            "chance_scope": "compiled order, exact weight and masks projected onto positive root support; no inferred public card IDs",
        }),
    )?;
    for (id, node) in tree.nodes.iter().enumerate() {
        let kind = match node.kind {
            NodeKind::Action => "action",
            NodeKind::Chance => "chance",
            NodeKind::Terminal => "terminal",
        };
        let mut row = json!({"node": id, "kind": kind, "first_child": node.first_child, "children": node.num_children});
        if node.kind == NodeKind::Action {
            let info = &built.node_info[tree.tags[id] as usize];
            row["actor"] = json!(if node.player == Player::P0 { 0 } else { 1 });
            row["history"] = json!(info.history);
            row["actions"] = json!(info.actions);
            row["street"] = json!(format!("{:?}", info.street));
            row["contrib"] = json!([info.contrib[Player::P0].0, info.contrib[Player::P1].0]);
        }
        if node.kind == NodeKind::Chance {
            let mut branches = Vec::new();
            for offset in 0..usize::from(node.num_children) {
                let deal = &tree.deals[node.aux as usize + offset];
                ensure!(
                    deal.weight.is_finite() && deal.weight > 0.0,
                    "invalid chance weight"
                );
                let mut masks = Vec::new();
                for (seat, player) in [Player::P0, Player::P1].into_iter().enumerate() {
                    let ReachMap::Mask(mask_id) = deal.maps[player] else {
                        bail!("iso=false postflop branch must use mask");
                    };
                    let mask = &tree.masks[mask_id as usize];
                    ensure!(
                        mask.len() == tree.root_dims[player] as usize,
                        "mask dimension mismatch"
                    );
                    let values = slots[seat]
                        .iter()
                        .map(|&slot| mask[slot])
                        .collect::<Vec<_>>();
                    ensure!(
                        values.iter().all(|&v| v == 0.0 || v == 1.0),
                        "non-binary card mask"
                    );
                    masks.push(values.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
                }
                branches.push(json!({"child_offset": offset, "weight_f32_bits": deal.weight.to_bits(), "support_mask_f32_bits": masks}));
            }
            row["branches"] = json!(branches);
        }
        json_line(&mut output, &row)?;
    }
    output.flush()?;
    Ok(json!({
        "schema": "solvers.hu-pipeline-probe/v1", "mode": "census",
        "semantic_files": {"input.json": file_identity(&input_path)?, "tree.jsonl": file_identity(&tree_path)?},
        "normalizer_f64_bits": game.normalizer.to_bits(),
        "layout_diagnostics": {"root_dims": [tree.root_dims[Player::P0], tree.root_dims[Player::P1]], "storage_len": tree.storage_len, "storage_refs": tree.storage_refs.len()},
        "limitations": ["terminal evaluator internals are not enumerated", "public card IDs are not inferred from sparse masks", "not a solver or quality measurement"]
    }))
}

fn checkpoint(path: &Path, out: &Path) -> Result<Value> {
    let before = file_identity(path)?;
    let decoded = formats::read_checkpoint(path)?;
    ensure!(
        decoded.iteration == decoded.state.iteration,
        "checkpoint header/state iteration mismatch"
    );
    let mut header = [0u8; 50];
    File::open(path)?.read_exact(&mut header)?;
    let state_path = out.join("state.bin");
    let mut output = new_file(&state_path)?;
    output.write_all(b"HUPST001")?;
    output.write_all(&decoded.iteration.to_le_bytes())?;
    let arrays = match &decoded.state.storage {
        StorageState::F32 {
            regrets,
            strategy_sum,
        } => {
            output.write_all(&[0])?;
            for values in [regrets, strategy_sum] {
                output.write_all(&(values.len() as u64).to_le_bytes())?;
                for &value in values {
                    ensure!(value.is_finite(), "nonfinite checkpoint state");
                    output.write_all(&value.to_bits().to_le_bytes())?;
                }
            }
            json!({"kind": "f32", "regrets": regrets.len(), "strategy_sum": strategy_sum.len()})
        }
        StorageState::I16 {
            regrets,
            strategy_sum,
            regret_scales,
            strategy_scales,
        } => {
            output.write_all(&[1])?;
            for values in [regrets, strategy_sum] {
                output.write_all(&(values.len() as u64).to_le_bytes())?;
                for value in values {
                    output.write_all(&value.to_le_bytes())?;
                }
            }
            for values in [regret_scales, strategy_scales] {
                output.write_all(&(values.len() as u64).to_le_bytes())?;
                for &value in values {
                    ensure!(value.is_finite(), "nonfinite checkpoint scale");
                    output.write_all(&value.to_bits().to_le_bytes())?;
                }
            }
            json!({"kind": "i16", "regrets": regrets.len(), "strategy_sum": strategy_sum.len(), "regret_scales": regret_scales.len(), "strategy_scales": strategy_scales.len()})
        }
    };
    output.flush()?;
    ensure!(
        before == file_identity(path)?,
        "checkpoint changed during probe"
    );
    Ok(json!({
        "schema": "solvers.hu-pipeline-probe/v1", "mode": "checkpoint",
        "input": before, "format_version": u16::from_le_bytes([header[8], header[9]]),
        "iteration": decoded.iteration,
        "config_hash_hex": decoded.config_hash.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "arrays": arrays, "state.bin": file_identity(&state_path)?,
        "state_encoding": "HUPST001; u64 iteration; u8 backend (f32=0,i16=1); each vector u64 length then little-endian element bits; F32 regrets/strategy_sum, I16 regrets/strategy_sum/regret_scales/strategy_scales",
        "scope": "read/decode only; layout-specific raw state, not cross-arm semantic equality or resume"
    }))
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.mode {
        Mode::Census => ensure!(
            args.config.is_some() && args.checkpoint.is_none(),
            "census requires only --config"
        ),
        Mode::Checkpoint => ensure!(
            args.checkpoint.is_some() && args.config.is_none(),
            "checkpoint requires only --checkpoint"
        ),
    }
    fs::create_dir(&args.out).context("--out must be a new directory under an existing parent")?;
    let report = match args.mode {
        Mode::Census => postflop_setup::with_threads(Some(1), || {
            census(args.config.as_deref().unwrap(), &args.out)
        })?,
        Mode::Checkpoint => checkpoint(args.checkpoint.as_deref().unwrap(), &args.out)?,
    };
    let mut saved = new_file(&args.out.join("report.json"))?;
    json_line(&mut saved, &report)?;
    saved.flush()?;
    json_line(&mut std::io::stdout().lock(), &report)
}
