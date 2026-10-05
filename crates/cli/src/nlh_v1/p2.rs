//! P2 common-input adapter. Game/session construction stays typed.
use super::SCHEMA;
use anyhow::{Result, bail};
use serde_json::json;
use std::path::Path;

use mw_preflop::prepare::{
    Prepared, check_resources, prepare, resources, utility_unit, warnings_for_hits,
};
pub(crate) fn validate(
    raw: &str,
    path: &Path,
    format: crate::validate::ValidationFormat,
    show: bool,
    write: Option<&Path>,
    with_resources: bool,
) -> Result<()> {
    let p = prepare(raw, path)?;
    let mut summary = p.document.summary();
    let measured = if with_resources {
        Some(resources(&p)?)
    } else {
        None
    };
    let rule_hit_status = match &measured {
        None => {
            summary.warnings.push(
                "unused tree rules not checked; use validate --resources to check them".into(),
            );
            "not-checked"
        }
        Some((r, hits)) if r.complete => {
            summary
                .warnings
                .extend(warnings_for_hits(&p.lowered.game, hits));
            "complete"
        }
        Some(_) => "incomplete",
    };
    let mut value = serde_json::to_value(&summary)?;
    super::p1::diagnostic_bb(&mut value);
    value["status"] = json!("valid");
    value["ruleHitStatus"] = json!(rule_hit_status);
    value["schema"] = json!(SCHEMA);
    value["gameKind"] = json!("mw-preflop");
    value["amountUnit"] = json!("BB");
    value["utilityUnit"] = json!(utility_unit(&p));
    let effective_value: toml::Value = p.effective.parse()?;
    value["table"] = serde_json::to_value(&effective_value["table"])?;
    value["table"]["firstActor"] =
        json!(p.document.spot.table.positions[p.document.spot.table.setup.preflop_first_to_act]);
    value["economics"] = serde_json::to_value(&effective_value["economics"])?;
    value["ranges"] = json!(
        p.document
            .spot
            .table
            .positions
            .seats()
            .map(|seat| {
                let range = &p.document.spot.ranges[seat];
                json!({"position": p.document.spot.table.positions[seat], "range": range.text,
            "combos": range.range.weights().iter().filter(|w| **w > 0.0).count(),
            "totalWeight": range.range.weights().iter().map(|w| f64::from(*w)).sum::<f64>()})
            })
            .collect::<Vec<_>>()
    );

    if let Some((r, _)) = measured {
        value["resources"] = json!({
            "complete": r.complete, "recall": "current-street", "decisionNodes": r.decision_nodes,
            "terminalEdges": r.terminal_edges, "policyColumns": r.policy_columns,
            "policySlots": r.policy_slots, "solverStateBytes": r.solver_state_bytes,
            "memoryLimitBytes": p.lowered.run.memory_bytes, "withinLimit": r.complete,
            "icm": r.icm.map(|i| json!({"fieldPlayers": i.field_players, "paidPlaces": i.paid_places,
                "mode": if matches!(i.mode, mw_preflop::session::IcmPreflightMode::Exact) { "exact" } else { "sampled" },
                "samples": i.samples, "seed": i.seed, "preparedBytes": i.prepared_bytes, "preparedLimitBytes": i.prepared_limit_bytes})),
        });
    }
    if show {
        value["effectiveConfig"] = serde_json::to_value(&effective_value)?;
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
                "valid: schema={SCHEMA} product=P2 (Multiway Preflop) amounts=BB utility={}",
                utility_unit(&p)
            );
            println!(
                "table: {} players; first actor {}",
                p.lowered.game.seats.len(),
                p.document.spot.table.positions[p.document.spot.table.setup.preflop_first_to_act]
            );
            for seat in p.document.spot.table.positions.seats() {
                println!(
                    "{}: stack {} BB range {}",
                    p.document.spot.table.positions[seat],
                    p.document.spot.table.stacks[seat].as_bb(),
                    p.document.spot.ranges[seat].text
                );
            }
            println!("economics: {}", value["economics"]);
            println!("rule hits: {rule_hit_status}");
            for rule in &summary.tree.rules {
                println!("tree rule: {rule}");
            }
            for param in &summary.tree.params {
                println!(
                    "tree param: {} = {} ({})",
                    param.name, param.value, param.kind
                );
            }
            for warning in &summary.warnings {
                println!("warning: {warning}");
            }
            if with_resources {
                println!("resources: {}", value["resources"]);
            }
            if show {
                println!("\n{}", p.effective);
            }
        }
    }
    Ok(())
}

pub(crate) fn solve(
    raw: &str,
    path: &Path,
    out: &Path,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
) -> Result<()> {
    let raw = super::p1::overrides(raw, threads, memory, max_time, None)?;
    let p = prepare(&raw, path)?;
    check_resources(&p)?;
    crate::run_dir::create_or_adopt(out)?;
    execute(p, out, None, false)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resume(
    raw: &str,
    checkpoint: &Path,
    out: Option<&Path>,
    threads: Option<usize>,
    memory: Option<&str>,
    max_time: Option<&str>,
    max_sweeps: Option<u64>,
    target: Option<f64>,
    samples: Option<u64>,
    cadence: Option<u64>,
    interval: Option<&str>,
) -> Result<()> {
    let raw = super::p1::overrides(raw, threads, memory, max_time, interval)?;
    let mut doc: toml_edit::DocumentMut = raw.parse()?;
    if !doc.contains_key("solver") {
        doc["solver"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if doc["solver"].as_table_like().is_none() {
        bail!("NLH002: solver: expected a table");
    }
    if doc["solver"].get("stop").is_none() {
        doc["solver"]["stop"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if doc["solver"]["stop"].as_table_like().is_none() {
        bail!("NLH002: solver.stop: expected a table");
    }
    for (key, value) in [
        ("max_sweeps", max_sweeps),
        ("evaluation_samples", samples),
        ("check_every_sweeps", cadence),
    ] {
        if let Some(value) = value {
            doc["solver"]["stop"][key] = toml_edit::value(i64::try_from(value)?);
        }
    }
    if let Some(value) = target {
        doc["solver"]["stop"]["target"] = toml_edit::value(value);
    }
    let p = prepare(&doc.to_string(), Path::new("run.toml"))?;
    check_resources(&p)?;
    let directory = out.unwrap_or_else(|| checkpoint.parent().unwrap_or(Path::new(".")));
    // Check compatibility before creating/writing a fork directory.
    let session = mw_preflop::prepare::build_typed_session(
        p.lowered.clone(),
        p.effective.clone(),
        Some(checkpoint),
        crate::cache::root().as_deref(),
        &mut crate::multiway_solve::print_abstraction,
    )?;
    drop(session);
    if out.is_some() {
        crate::run_dir::create_or_adopt(directory)?;
    }
    let active_checkpoint = if out.is_some() {
        directory.join(runfiles::RUN_CHECKPOINT_FILE)
    } else {
        checkpoint.to_path_buf()
    };
    if out.is_some() {
        std::fs::copy(checkpoint, &active_checkpoint)?;
    }
    execute(p, directory, Some(&active_checkpoint), target.is_some())
}

fn execute(p: Prepared, directory: &Path, checkpoint: Option<&Path>, reset: bool) -> Result<()> {
    let paths = crate::run_dir::RunPaths::multiway(directory);
    let hash = runfiles::config_hash(p.effective.as_bytes());
    let command = vec![
        if checkpoint.is_some() {
            "resume".into()
        } else {
            "solve".into()
        },
        directory.display().to_string(),
    ];
    let mut recorder = if checkpoint.is_some() {
        std::fs::write(directory.join(runfiles::RUN_CONFIG_FILE), &p.effective)?;
        crate::run_dir::RunRecorder::reopen(
            directory,
            "mw-preflop",
            Some(SCHEMA.into()),
            hash,
            &p.effective,
            command,
        )?
    } else {
        crate::run_dir::RunRecorder::start(
            directory,
            "mw-preflop",
            Some(SCHEMA.into()),
            hash,
            &p.effective,
            command,
        )?
    };
    let config = p.lowered;
    let outcome = if let Some(checkpoint) = checkpoint {
        crate::multiway_solve::resume_observed(
            &p.effective,
            config,
            Some(&paths.result),
            Some(&paths.progress),
            checkpoint,
            hash,
            Some(&paths.solution),
            Some(&crate::CLI_CANCEL),
            reset,
            true,
            &mut |o| recorder.observe(&o),
        )
    } else {
        crate::multiway_solve::run_observed(
            &p.effective,
            config,
            Some(&paths.result),
            Some(&paths.progress),
            Some(&paths.checkpoint),
            hash,
            Some(&paths.solution),
            Some(&crate::CLI_CANCEL),
            true,
            &mut |o| recorder.observe(&o),
        )
    };
    let completion = crate::run_dir::completion_status(directory);
    recorder.finish(outcome, completion)
}

#[cfg(test)]
mod tests {
    #[test]
    fn product_diagnostics_keep_cli_exit_mapping() {
        use mw_preflop::session::{MultiwayResourcePreflight, ResourceLimit};
        for limit in [ResourceLimit::Node, ResourceLimit::Memory] {
            let measurement = MultiwayResourcePreflight {
                complete: false,
                limit: Some(limit),
                recall: mw_preflop::RecallMode::Street,
                decision_nodes: 1,
                terminal_edges: None,
                icm: None,
                policy_columns: None,
                policy_slots: None,
                solver_state_bytes: None,
            };
            let error = mw_preflop::prepare::ensure_resources(&measurement).unwrap_err();
            assert_eq!(crate::error_exit_code(&error), 75);
        }
        for family in ["solvers.multiway-preflop/v1", "solvers.postflop/v1"] {
            let error = crate::nlh_v1::require_artifact_config(&format!("schema = '{family}'\n"))
                .unwrap_err();
            assert_eq!(crate::error_exit_code(&error), 3);
        }
    }
}
