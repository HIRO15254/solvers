//! P1 arguments, display and run-directory lifecycle.
use super::SCHEMA;
use anyhow::{Context, Result};
use hu_postflop::input;
pub use hu_postflop::prepare::compatibility_hash;
use hu_postflop::prepare::{bb, prepare, required_bytes, utility_unit, warnings};
use hu_postflop::run::{self, RunSummary};
use hu_postflop::{Player, Street};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

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

fn human_summary(p: &hu_postflop::prepare::Prepared) -> String {
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
            hu_postflop::BettingAction::Fold => "f".into(),
            hu_postflop::BettingAction::Check => "x".into(),
            hu_postflop::BettingAction::Call { .. } => "c".into(),
            hu_postflop::BettingAction::BetTo { all_in: true, .. }
            | hu_postflop::BettingAction::RaiseTo { all_in: true, .. } => "a".into(),
            hu_postflop::BettingAction::BetTo { to, .. } => format!("b{}", bb(to.0)),
            hu_postflop::BettingAction::RaiseTo { to, .. } => format!("r{}", bb(to.0)),
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
    let checkpoint = hu_postflop::prepare::restore(raw, checkpoint_path)?;
    let raw = overrides(raw, threads, memory, max_time, interval)?;
    let p = prepare(&raw, Path::new("run.toml"))?;
    input::check_memory_limit(&p.estimate, p.settings.solver.storage, p.limit)?;
    let elapsed = checkpoint.elapsed.unwrap_or(previous_elapsed(
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
    p: hu_postflop::prepare::Prepared,
    directory: &Path,
    state: Option<hu_postflop::SolverState>,
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
    let mut metrics = None;
    let outcome = run::run(
        run::RunRequest {
            prepared: &p,
            checkpoint: &paths.checkpoint,
            solution: &paths.solution,
            state,
            elapsed_before: elapsed,
            cancel: &crate::CLI_CANCEL,
        },
        &mut |observation| {
            match observation {
                run::Observation::ProgressOpened => {
                    metrics = Some(runfiles::MetricsWriter::create_or_append(&paths.progress)?)
                }
                run::Observation::Progress(row) => {
                    println!(
                        "iter={:>8} expl_p0={:.3e} expl_p1={:.3e} nash_conv={:.3e}",
                        row.iteration, row.expl_p0, row.expl_p1, row.nash_conv
                    );
                    metrics.as_mut().expect("progress opened").append(&row)?;
                }
                run::Observation::Checkpoint { iterations } => {
                    let _ = recorder
                        .events_mut()
                        .info(runfiles::RunEventPayload::Checkpoint { sweeps: iterations });
                }
                run::Observation::Stop { reason } => {
                    let _ = recorder.events_mut().info(runfiles::RunEventPayload::Stop {
                        reason: reason.into(),
                    });
                }
            }
            Ok(())
        },
        &mut |diagnostic| match diagnostic {
            run::Diagnostic::Memory(estimate) => print_memory_estimate(&estimate),
            run::Diagnostic::Warning(warning) => eprintln!("warning: {warning}"),
            run::Diagnostic::Done(summary) => print_done(&summary),
            run::Diagnostic::Artifact(diagnostic) => print_artifact(diagnostic),
        },
    );
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

pub(crate) fn print_memory_estimate(estimate: &hu_postflop::MemoryEstimate) {
    println!(
        "tree: nodes={} terminals={} rank_tables={} storage={:.1} MiB (f32) / {:.1} MiB (i16)",
        estimate.nodes,
        estimate.terminals,
        estimate.rank_tables,
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_bytes as f64 / (1024.0 * 1024.0),
    );
}

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

pub(crate) fn print_done(summary: &RunSummary) {
    println!(
        "done: iterations={} wall={:.2}s ev_p0={:.6} ev_p1={:.6} nash_conv={:.3e}",
        summary.iterations,
        summary.wall.as_secs_f64(),
        summary.ev[Player::P0],
        summary.ev[Player::P1],
        summary.nash_conv
    );
}
pub(crate) fn print_artifact(diagnostic: hu_postflop::artifact::Diagnostic) {
    use hu_postflop::artifact::Diagnostic;
    match diagnostic {
        Diagnostic::RiverFull => println!(
            "note: river-start config, forcing solution_streets=full (no-rivers would store nothing for a config with no streets before the river)"
        ),
        Diagnostic::Written {
            path,
            bytes,
            blocks,
            mode,
        } => {
            let size = if bytes >= 1024 * 1024 {
                format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
            } else {
                format!("{:.1} KB", bytes as f64 / 1024.0)
            };
            println!(
                "sol: wrote {} ({}, {} block{}, mode={:?})",
                path.display(),
                size,
                blocks,
                if blocks == 1 { "" } else { "s" },
                mode
            );
        }
        Diagnostic::Rebuilding => eprintln!("rebuilding tree from embedded config..."),
        Diagnostic::Rebuilt { secs, nodes } => {
            eprintln!("tree rebuilt in {secs:.2}s ({nodes} nodes)")
        }
        Diagnostic::RiverStart {
            history,
            iterations,
        } => println!("re-solving river subgame at {history:?} (up to {iterations} iterations)..."),
        Diagnostic::RiverDone {
            iterations,
            secs,
            nash_conv,
        } => println!("done: iterations={iterations} wall={secs:.2}s nash_conv={nash_conv:.3e}"),
    }
}
