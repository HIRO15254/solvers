//! P1 arguments, display and run-directory lifecycle.
use super::SCHEMA;
use anyhow::{Context, Result};
use hu_postflop::input;
pub use hu_postflop::prepare::compatibility_hash;
use hu_postflop::prepare::{bb, prepare, required_bytes, utility_unit, warnings};
use hu_postflop::run::{self, RunSummary};
use hu_postflop::{Player, Street};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

pub fn validate(
    raw: &str,
    path: &Path,
    format: crate::validate::ValidationFormat,
    show: bool,
    write: Option<&Path>,
    resources: bool,
) -> Result<()> {
    let p = prepare(raw, path)?;
    let profile = if p.document.spot.profile.is_some() {
        run::validate_profile(&p)?
    } else {
        None
    };
    let mut summary = p.document.summary();
    summary.warnings.extend(warnings(&p));
    let mut value = serde_json::to_value(&summary)?;
    crate::validate::diagnostic_bb(&mut value, p.config.scale);
    value["status"] = json!("valid");
    value["schema"] = json!(SCHEMA);
    value["gameKind"] = json!("hu-postflop");
    value["amountUnit"] = json!("BB");
    value["utilityUnit"] = json!(utility_unit(&p));
    if let Some(profile) = &profile {
        value["profile"] = serde_json::to_value(profile)?;
    }
    let normalized = p.document.normalize(&input::P1Sections)?;
    if resources {
        value["resources"] = json!({
            "storage": p.settings.solver.storage.name(),
            "nodes": p.estimate.nodes, "terminals": p.estimate.terminals,
            "f32Bytes": p.estimate.f32_bytes, "i16Bytes": p.estimate.i16_bytes,
            "i16F32avgBytes": p.estimate.i16_f32avg_bytes,
            "saveWorkspaceBytes": p.estimate.save_bytes, "compressionWorkspaceBytes": p.estimate.compression_bytes,
            "checkpointWorkspaceBytes": p.estimate.checkpoint_bytes(),
            "residentBytes": p.estimate.resident_bytes(), "gameBytes": p.estimate.game_bytes,
            "profileBytes": p.estimate.profile_bytes, "threads": p.estimate.threads,
            "threadWorkspaceBytes": p.estimate.thread_bytes,
            "memoryEstimateBytes": required_bytes(&p), "memoryLimitBytes": p.limit,
            "withinLimit": required_bytes(&p) <= p.limit,
        });
    }
    if show {
        value["effectiveConfig"] =
            serde_json::to_value(toml::from_str::<toml::Value>(&normalized)?)?;
    }
    if let Some(path) = write {
        std::fs::write(path, &normalized)?;
    }
    match format {
        crate::validate::ValidationFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&value)?)
        }
        crate::validate::ValidationFormat::Human => {
            println!(
                "valid: schema={SCHEMA} product=P1 (HU Postflop) amounts={} utility={}",
                if p.config.scale.is_decimal() {
                    "BB"
                } else {
                    "chips"
                },
                utility_unit(&p)
            );
            print!("{}", human_summary(&p));
            if let Some(profile) = &profile {
                println!(
                    "profile: matched_nodes={} matched_hands={} iso_restricted={}",
                    profile.matched_nodes, profile.matched_hands, profile.iso_restricted
                );
                for selector in &profile.selectors {
                    println!(
                        "  {} {}: nodes={} hands={}",
                        selector.kind,
                        selector.node,
                        selector.matched_nodes,
                        selector.matched_hands
                    );
                }
            }
            for warning in &summary.warnings {
                println!("warning: {warning}");
            }
            if resources {
                println!("resources:");
                println!(
                    "  storage: {}",
                    hu_postflop::prepare::storage_description(&p)
                );
                for (key, value) in value["resources"].as_object().expect("resources object") {
                    if key != "storage" {
                        println!("  {key}: {value}");
                    }
                }
            }
            if show {
                println!("\n{}", normalized);
            }
        }
    }
    Ok(())
}

fn human_summary(p: &hu_postflop::prepare::Prepared) -> String {
    use std::fmt::Write;
    let spot = &p.document.spot;
    let start = &spot.context;
    let scale = p.config.scale;
    let monetary = |amount| bb(amount, scale);
    let mut out = String::new();
    writeln!(out, "street: {:?}", start.street).unwrap();
    writeln!(out, "board: {}", spot.board_text.as_deref().unwrap_or("")).unwrap();
    writeln!(out, "starting pot: {}", monetary(start.pot.0)).unwrap();
    for (role, player) in [("OOP", &start.oop), ("IP", &start.ip)] {
        let player = player.as_ref().expect("P1 player");
        let seat = &start.seats[player.seat.index()];
        writeln!(
            out,
            "{role}: {} remaining stack {}",
            player.position,
            monetary(seat.remaining_stack.0)
        )
        .unwrap();
    }
    writeln!(
        out,
        "effective stack: {}",
        monetary(start.effective_stack.expect("P1 stack").0)
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
            hu_postflop::BettingAction::BetTo { to, .. } => format!("b{}", bb(to.0, scale)),
            hu_postflop::BettingAction::RaiseTo { to, .. } => format!("r{}", bb(to.0, scale)),
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
                rake.get("cap")
                    .map(|v| monetary(
                        v.as_str()
                            .expect("chip cap")
                            .trim_end_matches('c')
                            .parse()
                            .expect("chip cap")
                    ))
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
            + economics
                .get("outside_field")
                .and_then(toml::Value::as_array)
                .expect("ICM field")
                .len();
        writeln!(
            out,
            "economics: tournament ICM table players={table} field players={field} {}",
            if field <= 15 { "exact" } else { "quadrature" }
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
    hu_postflop::prepare::check_memory_limit(&p)?;
    crate::run_dir::create_or_adopt(out)?;
    execute(p, out, None, Vec::new(), Duration::ZERO, false)
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
    hu_postflop::prepare::check_memory_limit(&p)?;
    let progress = directory.join(runfiles::RUN_PROGRESS_FILE);
    let history = run::read_evaluation_history(&progress, checkpoint.state.iteration);
    let elapsed = match checkpoint.elapsed {
        Some(elapsed) => elapsed,
        None => previous_elapsed(&progress).unwrap_or(Duration::ZERO),
    };
    let active = out.unwrap_or(directory);
    if out.is_some() {
        crate::run_dir::create_or_adopt(active)?;
        if let Ok(progress) = std::fs::read(&progress) {
            std::fs::write(active.join(runfiles::RUN_PROGRESS_FILE), progress)?;
        }
    } else {
        crate::run_dir::lock_directory(active)?;
    }
    execute(p, active, Some(checkpoint.state), history, elapsed, true)
}

fn execute(
    p: hu_postflop::prepare::Prepared,
    directory: &Path,
    state: Option<hu_postflop::checkpoint::CheckpointReader>,
    evaluation_history: Vec<(u64, f64)>,
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
            evaluation_history,
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
                run::Observation::ProfileProgress {
                    iteration,
                    frequencies,
                } => {
                    for frequency in frequencies {
                        println!(
                            "iter={iteration:>8} frequency={}",
                            serde_json::to_string(&frequency)?
                        );
                    }
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
            run::Diagnostic::Memory(_) => {
                print_memory_estimate(&p);
                println!("storage: {}", hu_postflop::prepare::storage_description(&p));
            }
            run::Diagnostic::Warning(warning) => eprintln!("warning: {warning}"),
            run::Diagnostic::Done(summary) => print_done(&summary),
            run::Diagnostic::Artifact(diagnostic) => print_artifact(diagnostic),
        },
    );
    let outcome = outcome.and_then(|summary| -> Result<String> {
        let mut result = json!({
            "kind": "hu-postflop", "gameKind": "hu-postflop", "configSchema": SCHEMA,
            "configHash": runfiles::config_hash_hex(&hash), "utilityUnit": utility_unit(&p),
            "iterations": summary.iterations, "wallSecs": summary.wall.as_secs_f64(),
            "evP0": summary.ev[Player::P0], "evP1": summary.ev[Player::P1],
            "explP0": summary.expl_p0, "explP1": summary.expl_p1, "nashConv": summary.nash_conv,
        });
        if let Some(profile) = &summary.profile {
            result["profile"] = serde_json::to_value(profile)?;
        }
        if let Some(scope) = &summary.solve_scope {
            result["profile"]["solve_scope"] = json!(scope.scope);
            result["profile"]["subtree_roots"] = json!(scope.subtree_roots);
            result["profile"]["fallback_reason"] = json!(scope.fallback_reason);
        }
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
    });
    let completion = outcome.as_ref().ok().cloned();
    recorder.finish(outcome.map(|_| ()), completion)
}

pub(crate) fn print_memory_estimate(p: &hu_postflop::prepare::Prepared) {
    let estimate = &p.estimate;
    println!(
        "tree: nodes={} terminals={} rank_tables={} storage={:.1} MiB (f32) / {:.1} MiB (i16) / {:.1} MiB (i16-f32avg)",
        estimate.nodes,
        estimate.terminals,
        estimate.rank_tables,
        estimate.f32_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_bytes as f64 / (1024.0 * 1024.0),
        estimate.i16_f32avg_bytes as f64 / (1024.0 * 1024.0),
    );
    println!(
        "memory estimate: {:.1} MiB (f32) / {:.1} MiB (i16) / {:.1} MiB (i16-f32avg); resident {:.1} MiB (game {:.1} MiB, {} threads x {:.1} MiB), checkpoint encoder {:.1} MiB",
        hu_postflop::prepare::required_bytes_for(p, input::Storage::F32) as f64 / (1024.0 * 1024.0),
        hu_postflop::prepare::required_bytes_for(p, input::Storage::I16) as f64 / (1024.0 * 1024.0),
        hu_postflop::prepare::required_bytes_for(p, input::Storage::I16F32Avg) as f64
            / (1024.0 * 1024.0),
        estimate.resident_bytes() as f64 / (1024.0 * 1024.0),
        estimate.game_bytes as f64 / (1024.0 * 1024.0),
        estimate.threads,
        estimate.thread_bytes as f64 / (1024.0 * 1024.0),
        estimate.checkpoint_bytes() as f64 / (1024.0 * 1024.0),
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
    let mut reader = std::io::BufReader::new(file);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? != 0 {
        if line.last() != Some(&b'\n') {
            break;
        }
        if let Ok(row) = serde_json::from_slice::<runfiles::MetricsRow>(&line) {
            elapsed = Duration::try_from_secs_f64(row.elapsed_secs)
                .context("invalid recorded elapsed solve time")?;
        }
        line.clear();
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
    if let Some(scope) = &summary.solve_scope {
        println!(
            "freeze_upstream scope={} roots={} fallback={}",
            scope.scope,
            scope.subtree_roots,
            scope.fallback_reason.unwrap_or("none")
        );
    }
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
