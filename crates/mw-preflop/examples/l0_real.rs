//! Compare L0 profiles and their best responses against physical deals.
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
    let mut real_options = l0::RealOptions {
        deals: 4194304,
        seed: 0,
    };
    let mut fit_thresholds = vec![1.0, 2.0, 3.0];
    let mut fit_options = l0::RealOptions { deals: 0, seed: 1 };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!(
                "l0_real (--config TOML | --mwsol FILE) [--profile uniform|mwsol|json:FILE] [--tables-dir PATH] [--t3-samples 4096] [--t3-seed 0] [--k4-samples 2048] [--seed 0] [--threads N] [--top-infosets 20] [--output JSON] [--deals 4194304] [--deal-seed 0] [--fit-deals 0] [--fit-seed 1] [--fit-thresholds 1,2,3|none]"
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
            "--deals" => real_options.deals = value.parse()?,
            "--deal-seed" => real_options.seed = value.parse()?,
            "--fit-deals" => fit_options.deals = value.parse()?,
            "--fit-seed" => fit_options.seed = value.parse()?,
            "--fit-thresholds" => {
                fit_thresholds = if value == "none" {
                    Vec::new()
                } else {
                    value
                        .split(',')
                        .map(str::parse)
                        .collect::<std::result::Result<Vec<f64>, _>>()?
                };
                ensure!(
                    fit_thresholds.len() <= 4
                        && fit_thresholds.iter().all(|z| z.is_finite() && *z >= 0.0),
                    "fit thresholds must contain at most four finite nonnegative values"
                );
            }
            _ => bail!("unknown argument {arg}"),
        }
    }
    ensure!(
        config.is_some() != solution.is_some(),
        "supply exactly one of --config or --mwsol"
    );
    ensure!(threads != Some(0), "threads must be positive");
    ensure!(
        real_options.deals >= 2,
        "real evaluation needs at least two deals"
    );
    ensure!(
        fit_options.deals == 0 || fit_options.seed != real_options.seed,
        "fit seed must differ from deal seed"
    );
    ensure!(
        fit_options.deals == 0 || fit_options.deals >= 2,
        "fit needs at least two deals"
    );
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
    let start = Instant::now();
    let fit = if fit_options.deals > 0 {
        Some(pool.install(|| {
            l0::fit_real_responses(&tree, &profile, &game, fit_options, &fit_thresholds)
        })?)
    } else {
        None
    };
    let fit_seconds = start.elapsed().as_secs_f64();
    let mut responses = vec![evaluation.best_response_actions.clone()];
    let mut response_sets = vec!["l0".to_owned()];
    if let Some(fit) = &fit {
        responses.push(fit.actions.clone());
        response_sets.push("fitted".to_owned());
        for gated in &fit.gated {
            responses.push(gated.actions.clone());
            response_sets.push(format!("gated-z{}", gated.threshold));
        }
        println!(
            "Fit: {fit_seconds:.3}s; {} deals, {:.0} deals/s",
            fit.deals,
            fit.deals as f64 / fit_seconds
        );
    }
    let start = Instant::now();
    let real =
        pool.install(|| l0::evaluate_real(&tree, &profile, &responses, &game, real_options))?;
    let real_seconds = start.elapsed().as_secs_f64();
    for (s, r) in evaluation.seats.iter().zip(&real.seats) {
        println!(
            "{}: L0 value {:.9}, real {:.9} +/- {:.9}, difference {:.9}; L0 gain {:.9}, real BR gain {:.9} +/- {:.9} bb",
            s.name,
            s.value,
            r.value.mean,
            r.value.stderr,
            r.value.mean - s.value,
            s.gain,
            r.responses[0].gain.mean,
            r.responses[0].gain.stderr,
        );
        if let Some(fit) = &fit {
            println!(
                "  fitted: in-sample gain {:.9}, evaluation gain {:.9} +/- {:.9} bb",
                fit.seats[s.seat].gain, r.responses[1].gain.mean, r.responses[1].gain.stderr
            );
            for (g, gated) in fit.gated.iter().enumerate() {
                let seat = &gated.seats[s.seat];
                let held_out = &r.responses[2 + g].gain;
                println!(
                    "  gated-z{}: in-sample gain {:.9}, evaluation gain {:.9} +/- {:.9} bb; {} deviations",
                    gated.threshold, seat.gain, held_out.mean, held_out.stderr, seat.deviations
                );
            }
        }
        for k in 1..=tree.seats {
            println!(
                "  k={k}: L0 {:.9}, real {:.9} +/- {:.9}, difference {:.9}",
                s.value_by_active_count[k],
                r.value_by_active_count[k].mean,
                r.value_by_active_count[k].stderr,
                r.value_by_active_count[k].mean - s.value_by_active_count[k]
            );
        }
    }
    println!(
        "Seat-value sums: L0 {:.12}, real {:.12} +/- {:.12} bb; L0 NashConv {:.9}",
        evaluation.seats.iter().map(|s| s.value).sum::<f64>(),
        real.value_sum.mean,
        real.value_sum.stderr,
        evaluation.nash_conv
    );
    println!(
        "Real gains of the L0 best responses, summed over seats: {:.9} +/- {:.9} bb",
        real.response_gain_sums[0].mean, real.response_gain_sums[0].stderr
    );
    if let Some(fit) = &fit {
        println!(
            "Fitted gains summed: in-sample {:.9}, evaluation {:.9} +/- {:.9} bb",
            fit.seats.iter().map(|s| s.gain).sum::<f64>(),
            real.response_gain_sums[1].mean,
            real.response_gain_sums[1].stderr
        );
        for (g, gated) in fit.gated.iter().enumerate() {
            let held_out = &real.response_gain_sums[2 + g];
            println!(
                "Gated-z{} gains summed: in-sample {:.9}, evaluation {:.9} +/- {:.9} bb; {} deviations",
                gated.threshold,
                gated.seats.iter().map(|s| s.gain).sum::<f64>(),
                held_out.mean,
                held_out.stderr,
                gated.seats.iter().map(|s| s.deviations).sum::<u64>()
            );
        }
    }
    println!(
        "L0 {evaluation_seconds:.3}s; real {real_seconds:.3}s; {} deals, {:.0} deals/s; mean attempts {:.6}",
        real.deals,
        real.deals as f64 / real_seconds,
        real.mean_deal_attempts
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
            "format": "p2-l0-real-check", "version": 3,
            "source": source, "profile": {"kind": if kind.starts_with("json:") { "json" } else { &kind }, "path": profile_path},
            "game_fingerprint": fingerprint,
            "tables": {"t2": {"file": "t2-v1.bin", "payload_blake3": t2.payload_hash().to_hex().as_str()}, "t3": {"file": format!("t3-v1-n{t3_samples}-seed{t3_seed}.bin"), "payload_blake3": t3.payload_hash().to_hex().as_str(), "samples": t3_samples, "seed": t3_seed}},
            "k4": {"samples": options.k4_samples, "seed": options.seed},
            "terminals": {"counts_by_k": tree.terminal_counts()},
            "response_sets": response_sets, "fit": fit,
            "l0": {"seats": evaluation.seats, "nash_conv": evaluation.nash_conv}, "real": real, "units": "bb", "warnings": evaluation.warnings,
            "timings": {"tables": tables_seconds, "tree": tree_seconds, "profile": profile_seconds, "l0": evaluation_seconds, "real": real_seconds, "fit": if fit.is_some() { fit_seconds } else { 0.0 }}
        });
        write_json(&path, &result)?;
    }
    Ok(())
}
