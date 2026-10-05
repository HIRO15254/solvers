//! Common-input P1 CLI adapter. Legacy families retain their runners and units.
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use hu_engine::{
    DiscountSchedule, F32Storage, I16Storage, ParConfig, Solver, SolverState, Storage,
};
use hu_postflop::input::{self, Algorithm, NlhPayoff, P1Sections, Settings, SolutionStreets};
use nlh::{Player, Street};
use serde_json::{Value, json};

pub const SCHEMA: &str = "solvers.nlh/v1";

pub fn has_schema(raw: &str) -> Result<bool> {
    let value: toml::Value = toml::from_str(raw).context("parsing config")?;
    Ok(value.get("schema").and_then(toml::Value::as_str) == Some(SCHEMA))
}

struct Prepared {
    document: spot::Document,
    settings: Settings,
    config: hu_postflop::PostflopConfig,
    payoff: NlhPayoff,
    effective: String,
    estimate: hu_postflop::MemoryEstimate,
    limit: u64,
    target: Option<f64>,
}

fn tree_error(error: hu_postflop::TreeBuildError) -> spot::SpotError {
    spot::SpotError::new(spot::Code::NLH003, "tree", error.to_string())
}

fn prepare(raw: &str, path: &Path) -> Result<Prepared> {
    let document = spot::Document::parse(raw, path)?;
    if document.spot.product != spot::Product::HuPostflop {
        bail!("NLH005: P2 (Multiway Preflop) is not wired to solvers.nlh/v1 yet (M6)");
    }
    let settings = Settings::parse(&document.spot, &document.solver, &document.output)?;
    let effective = document.normalize(&P1Sections)?;
    let config = input::lower(&document.spot, &settings)?;
    let payoff = NlhPayoff::new(&document.spot)?;
    let target = settings
        .solver
        .stop
        .target
        .as_deref()
        .map(|target| input::resolve_target(&document.spot, target))
        .transpose()?;
    let estimate = hu_postflop::try_memory_usage(&config).map_err(tree_error)?;
    let physical = if document.spot.run.memory_bytes.is_none() {
        input::physical_memory_bytes().context("querying physical RAM")?
    } else {
        0
    };
    let limit = input::resolve_memory_limit(document.spot.run.memory_bytes, physical);
    Ok(Prepared {
        document,
        settings,
        config,
        payoff,
        effective,
        estimate,
        limit,
        target,
    })
}

fn warnings(p: &Prepared) -> Vec<String> {
    let mut warnings = Vec::new();
    if p.target.is_none() {
        warnings.push("no stop target: runs until max_iterations or max_time".into());
    }
    for street in [Street::Flop, Street::Turn, Street::River] {
        for (index, &hit) in p.estimate.rule_hits[street].iter().enumerate() {
            if !hit {
                warnings.push(format!("unmatched {street:?} tree rule {}", index + 1));
            }
        }
    }
    warnings
}

// The Spot diagnostic IR is milli-BB. Convert only its monetary fields at the CLI boundary.
fn diagnostic_bb(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "pot"
                        | "effective_stack"
                        | "starting_stack"
                        | "remaining_stack"
                        | "total_contribution"
                        | "refund"
                        | "amount"
                        | "to"
                ) {
                    if let Some(amount) = value.as_u64() {
                        *value = if amount % 1000 == 0 {
                            json!(amount / 1000)
                        } else {
                            json!(amount as f64 / 1000.0)
                        };
                    }
                } else {
                    diagnostic_bb(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(diagnostic_bb),
        _ => {}
    }
}

pub fn validate(
    raw: &str,
    path: &Path,
    format: crate::validate::ValidationFormat,
    show: bool,
    write: Option<&Path>,
    resources: bool,
) -> Result<()> {
    let p = prepare(raw, path)?;
    let mut summary = p.document.summary();
    summary.warnings = warnings(&p);
    let mut value = serde_json::to_value(summary)?;
    diagnostic_bb(&mut value);
    value["status"] = json!("valid");
    value["schema"] = json!(SCHEMA);
    value["gameKind"] = json!("hu-postflop");
    value["amountUnit"] = json!("BB");
    value["utilityUnit"] = json!(utility_unit(&p));
    if resources {
        value["resources"] = json!({
            "nodes": p.estimate.nodes, "terminals": p.estimate.terminals,
            "f32Bytes": p.estimate.f32_bytes, "i16Bytes": p.estimate.i16_bytes,
            "memoryEstimateBytes": required_bytes(&p), "memoryLimitBytes": p.limit,
            "withinLimit": required_bytes(&p) <= p.limit,
        });
    }
    if show {
        value["effectiveConfig"] =
            serde_json::to_value(toml::from_str::<toml::Value>(&p.effective)?)?;
    }
    if let Some(path) = write {
        std::fs::write(path, &p.effective)?;
    }
    match format {
        crate::validate::ValidationFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&value)?)
        }
        crate::validate::ValidationFormat::Human => {
            println!(
                "valid: schema={SCHEMA} product=P1 (HU Postflop) amounts=BB utility={}",
                utility_unit(&p)
            );
            println!("start: {}", serde_json::to_string(&value["start"])?);
            println!("actions: {}", serde_json::to_string(&value["actions"])?);
            println!("tree: {}", serde_json::to_string(&value["tree"])?);
            for warning in warnings(&p) {
                println!("warning: {warning}");
            }
            if resources {
                println!("resources: {}", value["resources"]);
            }
            if show {
                println!("\n{}", p.effective);
            }
        }
    }
    Ok(())
}

fn required_bytes(p: &Prepared) -> u64 {
    match p.settings.solver.storage {
        input::Storage::F32 => p.estimate.f32_bytes,
        input::Storage::I16 => p.estimate.i16_bytes,
    }
}

fn utility_unit(p: &Prepared) -> &'static str {
    // The normalized economics kind is authoritative and avoids duplicating the economics enum.
    if p.effective.parse::<toml::Value>().expect("normalized TOML")["economics"]["kind"].as_str()
        == Some("cash")
    {
        "BB"
    } else {
        "prizes"
    }
}

/// Compatibility stamp deliberately omits the operating settings in [run].
pub fn compatibility_hash(effective: &str) -> Result<[u8; 32]> {
    let mut document: toml_edit::DocumentMut = effective.parse()?;
    document.remove("run");
    Ok(runfiles::config_hash(document.to_string().as_bytes()))
}

fn overrides(
    raw: &str,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    interval: Option<&str>,
) -> Result<String> {
    let mut document: toml_edit::DocumentMut = raw.parse()?;
    if document
        .get("run")
        .is_some_and(|run| run.as_table_like().is_none())
    {
        return Err(spot::SpotError::new(spot::Code::NLH002, "run", "expected a table").into());
    }
    if !document.contains_key("run") {
        document["run"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if let Some(threads) = threads {
        let threads = i64::try_from(threads).context("NLH003: threads exceeds integer limit")?;
        document["run"]["threads"] = toml_edit::value(threads);
    }
    for (key, value) in [
        ("memory", memory),
        ("max_time", max_time),
        ("checkpoint_interval", interval),
    ] {
        if let Some(value) = value {
            document["run"][key] = if key == "memory"
                && !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_digit())
            {
                toml_edit::value(
                    value
                        .parse::<i64>()
                        .context("NLH003: memory exceeds integer limit")?,
                )
            } else {
                toml_edit::value(value)
            };
        }
    }
    Ok(document.to_string())
}

pub fn solve(
    raw: &str,
    path: &Path,
    out: &Path,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
) -> Result<()> {
    let raw = overrides(raw, threads, memory, max_time, None)?;
    let p = prepare(&raw, path)?;
    input::check_memory_limit(&p.estimate, p.settings.solver.storage, p.limit)?;
    crate::run_dir::create_or_adopt(out)?;
    execute(p, out, None, Duration::ZERO, false)
}

#[allow(clippy::too_many_arguments)]
pub fn resume(
    raw: &str,
    directory: &Path,
    checkpoint_path: &Path,
    out: Option<&Path>,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    interval: Option<&str>,
) -> Result<()> {
    let historical = prepare(raw, Path::new("run.toml"))?;
    let checkpoint = hu_postflop::checkpoint::read_checkpoint(checkpoint_path)?;
    let expected = compatibility_hash(&historical.effective)?;
    if checkpoint.config_hash != expected {
        bail!("checkpoint config hash does not match run.toml; refusing to resume");
    }
    let embedded = checkpoint
        .config_toml
        .as_deref()
        .ok_or_else(|| anyhow!("checkpoint has no effective config"))?;
    let embedded = prepare(embedded, Path::new("run.toml"))?;
    if compatibility_hash(&embedded.effective)? != expected {
        bail!("checkpoint embedded config hash does not match run.toml");
    }
    let raw = overrides(raw, threads, memory, max_time, interval)?;
    let p = prepare(&raw, Path::new("run.toml"))?;
    input::check_memory_limit(&p.estimate, p.settings.solver.storage, p.limit)?;
    let elapsed = checkpoint
        .elapsed_secs
        .map(Duration::try_from_secs_f64)
        .transpose()
        .context("invalid checkpoint elapsed time")?
        .unwrap_or(crate::solve::previous_elapsed(
            &directory.join(runfiles::RUN_PROGRESS_FILE),
        )?);
    let active = out.unwrap_or(directory);
    if out.is_some() {
        crate::run_dir::create_or_adopt(active)?;
        let progress = directory.join(runfiles::RUN_PROGRESS_FILE);
        if progress.exists() {
            std::fs::copy(progress, active.join(runfiles::RUN_PROGRESS_FILE))?;
        }
    }
    execute(p, active, Some(checkpoint.state), elapsed, true)
}

fn execute(
    p: Prepared,
    directory: &Path,
    state: Option<SolverState>,
    elapsed: Duration,
    resumed: bool,
) -> Result<()> {
    let paths = crate::run_dir::RunPaths::heads_up(directory);
    let hash = runfiles::config_hash(p.effective.as_bytes());
    let command = vec![
        if resumed { "resume" } else { "solve" }.into(),
        directory.display().to_string(),
    ];
    let mut recorder = if resumed {
        std::fs::write(directory.join(runfiles::RUN_CONFIG_FILE), &p.effective)?;
        crate::run_dir::RunRecorder::reopen(
            directory,
            "hu-postflop",
            Some(SCHEMA.into()),
            hash,
            &p.effective,
            command,
        )?
    } else {
        crate::run_dir::RunRecorder::start(
            directory,
            "hu-postflop",
            Some(SCHEMA.into()),
            hash,
            &p.effective,
            command,
        )?
    };
    let threads = p
        .document
        .spot
        .run
        .threads
        .map(usize::try_from)
        .transpose()?
        .unwrap_or(std::thread::available_parallelism()?.get());
    let outcome =
        crate::postflop_setup::with_threads(Some(threads), || match p.settings.solver.storage {
            input::Storage::F32 => {
                run::<F32Storage>(&p, &paths, state, elapsed, recorder.events_mut())
            }
            input::Storage::I16 => {
                run::<I16Storage>(&p, &paths, state, elapsed, recorder.events_mut())
            }
        });
    let completion = outcome
        .as_ref()
        .ok()
        .map(|summary| -> Result<String> {
            let result = json!({
                "kind": "hu-postflop", "gameKind": "hu-postflop", "configSchema": SCHEMA,
                "configHash": runfiles::config_hash_hex(&hash), "utilityUnit": utility_unit(&p),
                "iterations": summary.iterations, "wallSecs": summary.wall.as_secs_f64(),
                "evP0": summary.ev[Player::P0], "evP1": summary.ev[Player::P1],
                "explP0": summary.expl_p0, "explP1": summary.expl_p1, "nashConv": summary.nash_conv,
            });
            std::fs::write(
                &paths.result,
                format!("{}\n", serde_json::to_string_pretty(&result)?),
            )?;
            if summary.canceled {
                crate::CLI_EXIT_CODE.store(130, std::sync::atomic::Ordering::SeqCst);
            }
            Ok(if summary.canceled {
                "cancelled"
            } else {
                "completed"
            }
            .into())
        })
        .transpose()?;
    recorder.finish(outcome.map(|_| ()), completion)
}

fn schedule(algorithm: &Algorithm) -> Box<dyn DiscountSchedule> {
    match algorithm {
        Algorithm::Vanilla => Box::new(hu_engine::Vanilla),
        Algorithm::CfrPlus => Box::new(hu_engine::CfrPlus),
        Algorithm::Dcfr {
            alpha,
            beta,
            gamma,
            pow4_reset,
        } => Box::new(hu_engine::Dcfr {
            alpha: *alpha,
            beta: *beta,
            gamma: *gamma,
            pow4_reset: *pow4_reset,
        }),
        Algorithm::LinearCfr => Box::new(hu_engine::linear_cfr()),
        Algorithm::HsDcfr { gamma0 } => Box::new(hu_engine::HsDcfr { gamma0: *gamma0 }),
    }
}

fn run<S: Storage>(
    p: &Prepared,
    paths: &crate::run_dir::RunPaths,
    state: Option<SolverState>,
    elapsed_before: Duration,
    events: &mut runfiles::RunEventLog,
) -> Result<crate::solve::RunSummary> {
    crate::postflop_setup::print_memory_estimate(&p.estimate);
    for warning in warnings(p) {
        eprintln!("warning: {warning}");
    }
    let game =
        hu_postflop::try_build_postflop_game(&p.config, p.payoff.pipeline()).map_err(tree_error)?;
    let stop = &p.settings.solver.stop;
    let mut solver = Solver::<_, S>::new(
        game.game,
        schedule(&p.settings.solver.algorithm),
        Some(stop.max_iterations),
    );
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    if let Some(state) = state {
        solver
            .restore_state(state)
            .context("restoring checkpoint state")?;
    }
    let mut metrics = runfiles::MetricsWriter::create_or_append(&paths.progress)?;
    let start = Instant::now();
    let mut last_checkpoint = Instant::now();
    let interval = Duration::try_from_secs_f64(p.document.spot.run.checkpoint_interval_seconds)?;
    let max_time = p.document.spot.run.max_time_seconds;
    let mut canceled = false;
    let mut reason = "max-iterations";
    while solver.iteration() < stop.max_iterations {
        if max_time.is_some_and(|limit| (elapsed_before + start.elapsed()).as_secs_f64() >= limit) {
            reason = "time-limit";
            break;
        }
        if crate::CLI_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            canceled = true;
            reason = "cancelled";
            break;
        }
        solver.run(
            stop.check_every
                .min(stop.max_iterations - solver.iteration()),
        );
        let expl = solver.exploitability();
        let nash_conv = expl[Player::P0] + expl[Player::P1];
        println!(
            "iter={:>8} expl_p0={:.3e} expl_p1={:.3e} nash_conv={:.3e}",
            solver.iteration(),
            expl[Player::P0],
            expl[Player::P1],
            nash_conv
        );
        let elapsed_secs = (elapsed_before + start.elapsed()).as_secs_f64();
        metrics.append(&runfiles::MetricsRow {
            iteration: solver.iteration(),
            elapsed_secs,
            expl_p0: expl[Player::P0],
            expl_p1: expl[Player::P1],
            nash_conv,
        })?;
        if last_checkpoint.elapsed() >= interval {
            save(&solver, p, paths, elapsed_secs, events)?;
            last_checkpoint = Instant::now();
        }
        if crate::CLI_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            canceled = true;
            reason = "cancelled";
            break;
        }
        if p.target.is_some_and(|target| nash_conv / 2.0 <= target) {
            reason = "target-reached";
            break;
        }
    }
    let elapsed = elapsed_before + start.elapsed();
    let _ = events.info(runfiles::RunEventPayload::Stop {
        reason: reason.into(),
    });
    let offset = p.payoff.ev_offset();
    let mut summary = crate::solve::print_done(
        &solver,
        elapsed,
        crate::postflop_setup::subgame_ev(crate::solve::solver_ev(&solver), offset),
    );
    summary.canceled = canceled;
    save(&solver, p, paths, elapsed.as_secs_f64(), events)?;
    let spec = crate::sol::SolExportSpec {
        path: paths.solution.clone(),
        mode: match p.settings.output.solution_streets {
            SolutionStreets::Full => crate::sol::SolStreets::Full,
            SolutionStreets::NoRivers => crate::sol::SolStreets::NoRivers,
        },
        config_toml: p.effective.clone(),
        storage_name: match p.settings.solver.storage {
            input::Storage::F32 => "f32",
            input::Storage::I16 => "i16",
        }
        .into(),
    };
    crate::sol::export_sol(
        &spec,
        &solver,
        offset,
        p.document.spot.context.street,
        &summary,
    )?;
    Ok(summary)
}

fn save<S: Storage>(
    solver: &Solver<hu_postflop::PostflopEvaluator, S>,
    p: &Prepared,
    paths: &crate::run_dir::RunPaths,
    elapsed: f64,
    events: &mut runfiles::RunEventLog,
) -> Result<()> {
    hu_postflop::checkpoint::write_checkpoint_with_config(
        &paths.checkpoint,
        compatibility_hash(&p.effective)?,
        &solver.state(),
        &p.effective,
        elapsed,
    )?;
    let _ = events.info(runfiles::RunEventPayload::Checkpoint {
        sweeps: solver.iteration(),
    });
    Ok(())
}
