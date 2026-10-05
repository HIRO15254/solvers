//! Experimental L0 evaluation of class profiles and legacy P2 artifacts.
use anyhow::{Context, Result, bail, ensure};
use mw_preflop::{
    mwsol::{MWSOL_MAX_PAGE_LIMIT, MwSolReader},
    trunk::{
        l0::{self, ClassProfileDocument, EvaluationOptions, Model, Profile, Tree},
        tables::{HuShowdownTable, ThreeWayTable},
    },
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut config = None;
    let mut solution = None;
    let mut profile_kind = None;
    let mut tables_dir = PathBuf::from(".cache/p2-trunk");
    let mut t3_samples = 4096;
    let mut t3_seed = 0;
    let mut options = EvaluationOptions::default();
    let mut threads = None;
    let mut output = None;
    let mut export_tree = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "l0_eval (--config TOML | --mwsol FILE) [--profile uniform|mwsol|json:FILE] [--tables-dir PATH] [--t3-samples 4096] [--t3-seed 0] [--k4-samples 2048] [--seed 0] [--threads N] [--top-infosets 20] [--output JSON] [--export-tree JSON]"
            );
            return Ok(());
        }
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(value)),
            "--mwsol" => solution = Some(PathBuf::from(value)),
            "--profile" => profile_kind = Some(value),
            "--tables-dir" => tables_dir = value.into(),
            "--t3-samples" => t3_samples = value.parse()?,
            "--t3-seed" => t3_seed = value.parse()?,
            "--k4-samples" => options.k4_samples = value.parse()?,
            "--seed" => options.seed = value.parse()?,
            "--threads" => threads = Some(value.parse::<usize>()?),
            "--top-infosets" => options.top_infosets = value.parse()?,
            "--output" => output = Some(PathBuf::from(value)),
            "--export-tree" => export_tree = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {arg}"),
        }
    }
    ensure!(
        config.is_some() != solution.is_some(),
        "supply exactly one of --config or --mwsol"
    );
    ensure!(threads != Some(0), "threads must be positive");
    let mut reader = solution
        .as_ref()
        .map(|p| MwSolReader::open(p))
        .transpose()?;
    let source = config.as_ref().or(solution.as_ref()).unwrap();
    let game = if let Some(reader) = &reader {
        l0::game_from_solution(reader.metadata())?
    } else {
        l0::game_from_config(&std::fs::read_to_string(source)?, source)?
    };
    let start = Instant::now();
    let tree = Tree::build(&game)?;
    let tree_seconds = start.elapsed().as_secs_f64();
    if let Some(path) = export_tree {
        write_json(&path, &tree.export())?;
    }
    println!(
        "Tree: {} decisions, {} terminals ({tree_seconds:.3}s)",
        tree.nodes.iter().filter(|n| n.actor.is_some()).count(),
        tree.terminal_counts().iter().sum::<usize>()
    );
    let start = Instant::now();
    let kind = profile_kind.unwrap_or_else(|| {
        if reader.is_some() {
            "mwsol".into()
        } else {
            "uniform".into()
        }
    });
    let (profile, profile_path) = if kind == "uniform" {
        (Profile::uniform(&tree), None)
    } else if kind == "mwsol" {
        let reader = reader
            .as_mut()
            .context("--profile mwsol requires --mwsol")?;
        let mut profile = Profile::missing(&tree);
        let mut cursor = 0;
        loop {
            let page = reader.read_strategy_page(cursor, MWSOL_MAX_PAGE_LIMIT)?;
            profile.apply_blocks(&tree, &page.strategies)?;
            match page.next_cursor {
                Some(next) => cursor = next,
                None => break,
            }
        }
        (profile, solution.clone())
    } else if let Some(path) = kind.strip_prefix("json:") {
        let document: ClassProfileDocument = serde_json::from_slice(&std::fs::read(path)?)?;
        (
            Profile::from_json(&tree, &document)?,
            Some(PathBuf::from(path)),
        )
    } else {
        bail!("unknown profile kind {kind}")
    };
    let profile_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let t2 = HuShowdownTable::load_or_build(&tables_dir)?;
    let t3 = ThreeWayTable::load_or_build(&tables_dir, t3_samples, t3_seed)?;
    let tables_seconds = start.elapsed().as_secs_f64();
    println!("Profile: {profile_seconds:.3}s; tables: {tables_seconds:.3}s");
    let tables = (&t2, &t3);
    let model = Model::new(&game, &tables, options)?;
    let mut builder = rayon::ThreadPoolBuilder::new();
    if let Some(threads) = threads {
        builder = builder.num_threads(threads);
    }
    let pool = builder.build()?;
    let start = Instant::now();
    let evaluation = pool.install(|| l0::evaluate(&tree, &profile, &model))?;
    let evaluation_seconds = start.elapsed().as_secs_f64();
    for s in &evaluation.seats {
        println!(
            "{}: value {:.9}, BR {:.9}, gain {:.9} bb; defaulted mass {:.6}; reach k {:?}",
            s.name,
            s.value,
            s.best_response,
            s.gain,
            s.defaulted_mass,
            &s.reach_by_active_count[1..]
        );
    }
    println!(
        "NashConv {:.9} bb; evaluation {evaluation_seconds:.3}s; phases {:?}",
        evaluation.nash_conv, evaluation.timings
    );
    for warning in &evaluation.warnings {
        println!("Warning: {warning}");
    }
    if let Some(path) = output {
        let fingerprint: String = tree
            .game_fingerprint
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let result = json!({
            "format": "p2-l0-evaluation", "version": 1,
            "source": source, "profile": {"kind": if kind.starts_with("json:") { "json" } else { &kind }, "path": profile_path},
            "game_fingerprint": fingerprint,
            "tables": {"t2": {"file": "t2-v1.bin", "payload_blake3": t2.payload_hash().to_hex().as_str()}, "t3": {"file": format!("t3-v1-n{t3_samples}-seed{t3_seed}.bin"), "payload_blake3": t3.payload_hash().to_hex().as_str(), "samples": t3_samples, "seed": t3_seed}},
            "k4": {"samples": options.k4_samples, "seed": options.seed},
            "terminals": {"counts_by_k": tree.terminal_counts()},
            "seats": evaluation.seats, "nash_conv": evaluation.nash_conv, "units": "bb", "warnings": evaluation.warnings,
            "timings": {"tables": tables_seconds, "tree": tree_seconds, "profile": profile_seconds, "evaluation": evaluation_seconds, "phases": evaluation.timings}
        });
        write_json(&path, &result)?;
    }
    Ok(())
}
