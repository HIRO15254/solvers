//! Common-input P1 CLI adapter and shared P1 query helpers.
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use hu_engine::{
    DiscountSchedule, F32Storage, I16Storage, ParConfig, Solver, SolverState, Storage,
    TerminalEvaluator,
};
use hu_postflop::input::{self, Algorithm, NlhPayoff, P1Sections, Settings, SolutionStreets};
use hu_postflop::{MemoryEstimate, RuleHits};
use nlh::{PerPlayer, Player, Street};
use serde_json::{Value, json};

use super::SCHEMA;

pub(crate) struct Prepared {
    pub document: spot::Document,
    pub settings: Settings,
    pub config: hu_postflop::PostflopConfig,
    pub payoff: NlhPayoff,
    effective: String,
    pub estimate: hu_postflop::MemoryEstimate,
    limit: u64,
    target: Option<f64>,
}

fn tree_error(error: hu_postflop::TreeBuildError) -> spot::SpotError {
    spot::SpotError::new(spot::Code::NLH003, "tree", error.to_string())
}

pub(crate) fn prepare(raw: &str, path: &Path) -> Result<Prepared> {
    let document = spot::Document::parse(raw, path)?;
    if document.spot.product != spot::Product::HuPostflop {
        bail!("NLH005: this command requires a P1 (HU Postflop) spot");
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
    warnings_for_hits(p, &p.estimate.rule_hits)
}

pub(crate) fn warnings_for_hits(p: &Prepared, hits: &hu_postflop::RuleHits) -> Vec<String> {
    let mut warnings = Vec::new();
    if p.target.is_none() {
        warnings.push("no stop target: runs until max_iterations or max_time".into());
    }
    for street in [Street::Flop, Street::Turn, Street::River] {
        for (index, &hit) in hits[street].iter().enumerate() {
            if !hit {
                warnings.push(format!("unmatched {street:?} tree rule {}", index + 1));
            }
        }
    }
    warnings
}

// The Spot diagnostic IR is milli-BB. Convert only its monetary fields at the CLI boundary.
pub(super) fn diagnostic_bb(value: &mut Value) {
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
            print!("{}", human_summary(&p));
            for warning in warnings(&p) {
                println!("warning: {warning}");
            }
            if resources {
                println!("resources:");
                for (key, value) in value["resources"].as_object().expect("resources object") {
                    println!("  {key}: {value}");
                }
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

pub(crate) fn threads(p: &Prepared) -> Result<Option<usize>> {
    Ok(Some(
        p.document
            .spot
            .run
            .threads
            .map(usize::try_from)
            .transpose()?
            .unwrap_or(std::thread::available_parallelism()?.get()),
    ))
}

/// Read-only solve shared by live inspection and board reports.
pub(crate) fn query<S: Storage>(
    p: &Prepared,
    iterations: Option<u64>,
    target_nash_conv: Option<f64>,
) -> Result<(
    Solver<hu_postflop::PostflopEvaluator, S>,
    Vec<hu_postflop::PostflopNodeInfo>,
)> {
    input::check_memory_limit(&p.estimate, p.settings.solver.storage, p.limit)?;
    let iterations = iterations.unwrap_or(p.settings.solver.stop.max_iterations);
    if iterations == 0 || target_nash_conv.is_some_and(|t| !t.is_finite() || t < 0.0) {
        bail!("NLH003: iterations must be positive and target_nash_conv finite and non-negative");
    }
    let mut game =
        hu_postflop::try_build_postflop_game(&p.config, p.payoff.pipeline()).map_err(tree_error)?;
    display_game(&mut game);
    let mut solver = Solver::<_, S>::new(
        game.game,
        schedule(&p.settings.solver.algorithm),
        Some(iterations),
    );
    solver.set_par(ParConfig {
        chance_depth: p.settings.solver.parallel.chance_depth,
        min_children: p.settings.solver.parallel.min_children,
    });
    let start = Instant::now();
    let target = target_nash_conv.or(p.target.map(|t| 2.0 * t));
    while solver.iteration() < iterations {
        if crate::CLI_CANCEL.load(std::sync::atomic::Ordering::SeqCst)
            || p.document
                .spot
                .run
                .max_time_seconds
                .is_some_and(|t| start.elapsed().as_secs_f64() >= t)
        {
            break;
        }
        solver.run(
            p.settings
                .solver
                .stop
                .check_every
                .min(iterations - solver.iteration()),
        );
        if let Some(target) = target {
            let expl = solver.exploitability();
            if expl[Player::P0] + expl[Player::P1] <= target {
                break;
            }
        }
    }
    Ok((solver, game.node_info))
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

/// Exact shortest decimal on the milli-BB grid; also used for query labels.
pub(crate) fn bb(amount: u64) -> String {
    let fraction = amount % 1000;
    if fraction == 0 {
        (amount / 1000).to_string()
    } else {
        format!("{}.{fraction:03}", amount / 1000)
            .trim_end_matches('0')
            .into()
    }
}

fn human_summary(p: &Prepared) -> String {
    use std::fmt::Write;
    let spot = &p.document.spot;
    let start = &spot.context;
    let mut out = String::new();
    writeln!(out, "street: {:?}", start.street).unwrap();
    writeln!(out, "board: {}", spot.board_text.as_deref().unwrap_or("")).unwrap();
    writeln!(out, "starting pot: {} BB", bb(start.pot.0)).unwrap();
    for (role, player) in [("OOP", &start.oop), ("IP", &start.ip)] {
        let player = player.as_ref().expect("P1 player");
        let seat = &start.seats[player.seat.index()];
        writeln!(
            out,
            "{role}: {} remaining stack {} BB",
            player.position,
            bb(seat.remaining_stack.0)
        )
        .unwrap();
    }
    writeln!(
        out,
        "effective stack: {} BB",
        bb(start.effective_stack.expect("P1 stack").0)
    )
    .unwrap();
    let folded: Vec<_> = start
        .seats
        .iter()
        .filter(|s| s.folded)
        .map(|s| s.position.as_str())
        .collect();
    writeln!(
        out,
        "folded: {}",
        if folded.is_empty() {
            "none".into()
        } else {
            folded.join(", ")
        }
    )
    .unwrap();
    let mut line = String::new();
    let mut street = Street::Preflop;
    for a in &start.actions {
        if !line.is_empty() {
            line.push_str(if a.street != street { " / " } else { ", " });
        }
        street = a.street;
        let action = match a.action {
            nlh::betting::Action::Fold => "f".into(),
            nlh::betting::Action::Check => "x".into(),
            nlh::betting::Action::Call { .. } => "c".into(),
            nlh::betting::Action::BetTo { all_in: true, .. }
            | nlh::betting::Action::RaiseTo { all_in: true, .. } => "a".into(),
            nlh::betting::Action::BetTo { to, .. } => format!("b{}", bb(to.0)),
            nlh::betting::Action::RaiseTo { to, .. } => format!("r{}", bb(to.0)),
        };
        write!(
            line,
            "{} {action}{}",
            a.position,
            if a.implicit { "*" } else { "" }
        )
        .unwrap();
    }
    writeln!(out, "line: {line}").unwrap();
    if start.actions.iter().any(|a| a.implicit) {
        writeln!(out, "* marks an implied fold").unwrap();
    }
    let effective: toml::Value = p.effective.parse().expect("effective config");
    let economics = &effective["economics"];
    if utility_unit(p) == "BB" {
        if let Some(rake) = economics.get("rake") {
            let number = |v: &toml::Value| {
                v.as_float()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| v.to_string())
            };
            writeln!(
                out,
                "economics: cash with rake rate={} cap={} when={}",
                number(&rake["rate"]),
                rake.get("cap_bb")
                    .map(|v| format!("{} BB", number(v)))
                    .unwrap_or_else(|| "none".into()),
                rake["when"].as_str().unwrap()
            )
            .unwrap();
        } else {
            writeln!(out, "economics: cash without rake").unwrap();
        }
    } else {
        let table = spot.table.positions.len();
        let field = table
            + economics["outside_field_bb"]
                .as_array()
                .expect("ICM field")
                .len();
        writeln!(
            out,
            "economics: tournament ICM table players={table} field players={field} {}",
            if field <= 15 { "exact" } else { "sampled" }
        )
        .unwrap();
    }
    let diagnostics = p.document.summary().tree;
    for param in diagnostics.params {
        writeln!(
            out,
            "tree param: {} = {} ({})",
            param.name, param.value, param.kind
        )
        .unwrap();
    }
    for rule in diagnostics.rules {
        writeln!(out, "tree rule: {rule}").unwrap();
    }
    out
}

/// Convert display metadata only. The compiled tree and all monetary IR stay in milli-BB.
pub(crate) fn display_game(game: &mut hu_postflop::PostflopGame) {
    for info in &mut game.node_info {
        info.history = convert_history(&info.history, false);
        for label in &mut info.actions {
            for prefix in ["bet ", "raise to "] {
                if let Some(amount) = label.strip_prefix(prefix) {
                    *label = format!("{prefix}{}", bb(amount.parse().expect("builder amount")));
                    break;
                }
            }
        }
    }
}

pub(crate) fn convert_history(history: &str, to_internal: bool) -> String {
    let mut chars = history.chars().peekable();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        result.push(c);
        if c == '[' {
            for c in chars.by_ref() {
                result.push(c);
                if c == ']' {
                    break;
                }
            }
        } else if c == 'r' {
            let mut amount = String::new();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                amount.push(chars.next().unwrap());
            }
            if to_internal {
                let amount =
                    nlh::MwChips::try_from_bb(amount.parse().expect("display history amount"))
                        .expect("BB history grid");
                result.push_str(&amount.0.to_string());
            } else {
                result.push_str(&bb(amount.parse().expect("builder history amount")));
            }
        }
    }
    result
}

/// Compatibility stamp deliberately omits the operating settings in [run]
/// and the descriptive [meta], neither of which affects the computation.
pub fn compatibility_hash(effective: &str) -> Result<[u8; 32]> {
    let mut document: toml_edit::DocumentMut = effective.parse()?;
    document.remove("run");
    document.remove("meta");
    Ok(runfiles::config_hash(document.to_string().as_bytes()))
}

pub(super) fn overrides(
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
    crate::nlh_v1::require_artifact_config(raw)?;
    let historical = prepare(raw, Path::new("run.toml"))?;
    let checkpoint = hu_postflop::checkpoint::read_checkpoint(checkpoint_path)?;
    if let Some(embedded) = &checkpoint.config_toml {
        crate::nlh_v1::require_artifact_config(embedded)?;
    }
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
        .unwrap_or(previous_elapsed(
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
    let outcome = with_threads(Some(threads), || match p.settings.solver.storage {
        input::Storage::F32 => run::<F32Storage>(&p, &paths, state, elapsed, recorder.events_mut()),
        input::Storage::I16 => run::<I16Storage>(&p, &paths, state, elapsed, recorder.events_mut()),
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

pub(crate) fn schedule(algorithm: &Algorithm) -> Box<dyn DiscountSchedule> {
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
) -> Result<RunSummary> {
    print_memory_estimate(&p.estimate);
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
    let mut summary = print_done(&solver, elapsed, subgame_ev(solver_ev(&solver), offset));
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

/// Run work in a local pool when the configuration specifies a thread count.
pub(crate) fn with_threads<T: Send>(
    threads: Option<usize>,
    work: impl FnOnce() -> Result<T> + Send,
) -> Result<T> {
    match threads {
        Some(threads) => rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?
            .install(work),
        None => work(),
    }
}

/// Applies the payoff's subgame-start offset to a solver's root values.
///
/// Under chip EV without rake the two results sum to the starting pot; with
/// rake, to the pot less the expected rake. They never sum to zero.
pub(crate) fn subgame_ev(solver_ev: PerPlayer<f64>, offset: PerPlayer<f64>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver_ev[Player::P0] + offset[Player::P0],
        solver_ev[Player::P1] + offset[Player::P1],
    )
}

pub(crate) fn print_memory_estimate(estimate: &MemoryEstimate) {
    println!(
        "tree: nodes={} terminals={} rank_tables={} storage={:.1} MiB (f32) / {:.1} MiB (i16)",
        estimate.nodes,
        estimate.terminals,
        estimate.rank_tables,
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_bytes as f64 / (1024.0 * 1024.0),
    );
}

/// OR-merges `hits` into `acc`, street by street and rule by rule -- the
/// aggregation `report`'s multi-board sweep needs: a rule counts as
/// "matched" the moment ANY board's build satisfies it, so this must be a
/// logical OR across boards, not a per-board report (a rule that only fires
/// on one board out of twenty is working as intended, not a bug). Panics if `acc` and `hits` don't
/// have the same per-street rule counts, which would mean they came from
/// two different tree scripts; callers only ever merge `RuleHits` produced
/// from the same config's `streets`, so that never happens in practice.
pub(crate) fn merge_rule_hits(acc: &mut RuleHits, hits: &RuleHits) {
    for street in [Street::Flop, Street::Turn, Street::River] {
        let acc_street = &mut acc[street];
        let hit_street = &hits[street];
        assert_eq!(
            acc_street.len(),
            hit_street.len(),
            "merged RuleHits must come from the same tree script"
        );
        for (a, &b) in acc_street.iter_mut().zip(hit_street) {
            *a |= b;
        }
    }
}

/// Reach-weighted overall frequency of each action at an action node: for
/// action `a`, `sum_combo(weight[combo] * avg_strategy[a][combo]) /
/// sum_combo(weight[combo])`.
///
/// `weight` is the acting player's reach *at that node* (see
/// `hu_engine::reach_at`), not their root range. The two agree at the root and
/// diverge below it: a hand that folded upstream, or that card removal has
/// made impossible, still carries root weight but no reach, and counting it
/// would report a frequency over hands that could not be there.
///
/// Returns one frequency per action, and `0.0` for every action when the
/// node is unreachable (total weight zero).
pub(crate) fn action_frequencies(
    avg_strategy: &[f32],
    weight: &[f32],
    num_actions: usize,
    num_hands: usize,
) -> Vec<f64> {
    let total: f64 = weight.iter().map(|&w| w as f64).sum();
    if total <= 0.0 {
        return vec![0.0; num_actions];
    }
    (0..num_actions)
        .map(|a| {
            let row = &avg_strategy[a * num_hands..(a + 1) * num_hands];
            let sum: f64 = weight
                .iter()
                .zip(row)
                .map(|(&w, &s)| w as f64 * s as f64)
                .sum();
            sum / total
        })
        .collect()
}

/// Last durable cumulative solve time; truncated tail rows are ignored.
pub(crate) fn previous_elapsed(path: &Path) -> Result<Duration> {
    use std::io::BufRead;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Duration::ZERO),
        Err(error) => return Err(error.into()),
    };
    let mut elapsed = Duration::ZERO;
    for line in std::io::BufReader::new(file).lines() {
        if let Ok(row) = serde_json::from_str::<runfiles::MetricsRow>(&line?) {
            elapsed = Duration::try_from_secs_f64(row.elapsed_secs)
                .context("invalid recorded elapsed solve time")?;
        }
    }
    Ok(elapsed)
}

/// Final convergence numbers shared by P1 run publication and solution export.
pub(crate) struct RunSummary {
    pub iterations: u64,
    /// Root EV per player, on the subgame-start reporting basis.
    pub ev: PerPlayer<f64>,
    pub wall: Duration,
    pub expl_p0: f64,
    pub expl_p1: f64,
    pub nash_conv: f64,
    /// The run stopped on request at a checkpoint boundary, not because it
    /// reached its budget or its convergence target.
    pub canceled: bool,
}

/// Final convergence summary, shared by every heads-up caller.
///
/// `ev` is already on the subgame-start reporting basis — for postflop that is
/// the subgame-start basis (see [`crate::nlh_v1::subgame_ev`]). Both
/// players' numbers are printed because `ev_p1 == -ev_p0` is not guaranteed:
/// rake makes any game general-sum, and postflop's basis makes the pair sum
/// to the starting pot rather than to zero.
pub(crate) fn print_done<E: TerminalEvaluator, S: Storage>(
    solver: &Solver<E, S>,
    elapsed: Duration,
    ev: PerPlayer<f64>,
) -> RunSummary {
    let expl = solver.exploitability();
    let nash_conv = expl[Player::P0] + expl[Player::P1];
    println!(
        "done: iterations={} wall={:.2}s ev_p0={:.6} ev_p1={:.6} nash_conv={:.3e}",
        solver.iteration(),
        elapsed.as_secs_f64(),
        ev[Player::P0],
        ev[Player::P1],
        nash_conv,
    );
    RunSummary {
        canceled: false,
        iterations: solver.iteration(),
        wall: elapsed,
        ev,
        expl_p0: expl[Player::P0],
        expl_p1: expl[Player::P1],
        nash_conv,
    }
}

/// The solver's own root values, before the subgame-start reporting offset (see
/// [`crate::nlh_v1::subgame_ev`]) is applied.
pub(crate) fn solver_ev<E: TerminalEvaluator, S: Storage>(solver: &Solver<E, S>) -> PerPlayer<f64> {
    PerPlayer::new(
        solver.expected_value(Player::P0),
        solver.expected_value(Player::P1),
    )
}
