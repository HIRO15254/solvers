//! P2 common-input adapter. Game/session construction stays typed.
use super::SCHEMA;
use crate::config::{
    AlgorithmSection, GameSection, RakeSection, RunSection, SolveConfig, StorageKind,
    UtilitySection,
};
use anyhow::{Result, bail};
use mw_preflop::input::{self, Lowered, P2Sections, Settings};
use serde_json::json;
use std::path::Path;

pub(crate) struct Prepared {
    pub document: spot::Document,
    pub lowered: Lowered,
    pub effective: String,
}

pub(crate) fn prepare(raw: &str, path: &Path) -> Result<Prepared> {
    let document = spot::Document::parse(raw, path)?;
    let settings = Settings::parse(&document.spot, &document.solver, &document.output)?;
    let effective = document.normalize(&P2Sections)?;
    let lowered = input::lower(&document.spot, &settings)?;
    Ok(Prepared {
        document,
        lowered,
        effective,
    })
}

// The internal CLI representation supports read-only artifact summaries and
// algorithm fingerprints. It is never serialized to build a common-input game.
pub(crate) fn internal(input: Lowered) -> SolveConfig {
    let utility = match input.utility {
        mw_preflop::UtilityConfig::ChipEv => UtilitySection::ChipEv,
        mw_preflop::UtilityConfig::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => UtilitySection::TournamentIcm {
            outside_field: outside_field
                .into_iter()
                .map(|p| crate::config::OutsidePlayerSection {
                    name: p.name,
                    stack_bb: p.stack_bb,
                })
                .collect(),
            payouts,
            samples,
            seed,
        },
    };
    let rake = match input.rake {
        mw_preflop::config::RakeConfig::None => RakeSection::None,
        mw_preflop::config::RakeConfig::Generic {
            rate,
            cap_bb,
            when,
            allocation,
            rounding,
        } => RakeSection::Generic {
            rate,
            cap: cap_bb,
            when,
            allocation,
            rounding,
            rounding_unit: 0.001,
        },
        _ => unreachable!("common input lowers cash rake to Generic"),
    };
    let s = input.solver;
    let r = input.run;
    SolveConfig {
        schema: Some(SCHEMA.into()),
        game: GameSection::PreflopMultiway(input.game),
        utility,
        rake,
        algorithm: AlgorithmSection::ExternalSamplingMccfr {
            seed: s.seed,
            exploration_epsilon: s.exploration_epsilon,
            discount_every: s.discount_every,
            discount_until: s.discount_until,
            traverser_vector: s.traverser_vector,
            prune: s.prune,
            // Match the legacy adapter: threshold is derived when sessions are built.
            prune_threshold: None,
            prune_skip_probability: s.prune_skip_probability,
        },
        run: RunSection {
            iterations: r.stop.max_sweeps,
            sweeps: Some(r.stop.max_sweeps),
            seed: Some(s.seed),
            check_every: r.stop.check_every_sweeps,
            max_time_secs: None,
            storage: StorageKind::F32,
            target_nash_conv: None,
            threads: Some(r.threads),
            par_chance_depth: None,
            par_min_children: None,
            max_memory_bytes: Some(r.memory_bytes),
            checkpoint_every: None,
            evaluation_samples: Some(r.stop.evaluation_samples),
            evaluation_cadence: Some(r.stop.check_every_sweeps),
            sweep_batch: Some(s.sweep_batch),
            stop_dev_gain: Some(r.dev_gain_threshold),
            stop_confirmations: Some(r.stop.confirmations),
            stop_eval_period_secs: Some(30.0),
            stop_br_traversals: Some(r.stop.deviator_traversals),
        },
    }
}

pub(crate) fn parse_and_lower(raw: &str, path: &Path) -> Result<SolveConfig> {
    Ok(internal(prepare(raw, path)?.lowered))
}

pub(crate) fn build_typed_session(
    lowered: Lowered,
    effective: String,
    checkpoint: Option<&Path>,
) -> Result<crate::session::MultiwaySession> {
    let (table, ready) = crate::session::build_ehs2_table_abstraction(&lowered.game)?;
    let abstraction = mw_preflop::MultiwayAbstractionBackend::Ehs2Table(table);
    let session = input::build_session(lowered, abstraction, effective, checkpoint)?;
    let r = session.run;
    Ok(crate::session::MultiwaySession {
        solver: session.solver,
        abstraction_ready: Some(ready),
        sweeps_target: r.stop.max_sweeps,
        threads: r.threads,
        evaluation_cadence: r.stop.check_every_sweeps,
        evaluation_samples: r.stop.evaluation_samples,
        evaluation_seed: r.evaluation_seed,
        checkpoint_every: None,
        storage: StorageKind::F32,
        stop_rule: Some(crate::session::StopRule {
            dev_gain_threshold: r.dev_gain_threshold,
            confirmations: r.stop.confirmations,
            eval_period_secs: 30.0,
            br_traversals: r.stop.deviator_traversals,
        }),
        config_toml: session.config_toml,
        config_hash: session.config_hash,
        game_config: session.game_config,
        checkpoint_runtime: session.checkpoint_runtime,
    })
}

pub(crate) fn build_session(
    raw: &str,
    checkpoint: Option<&Path>,
) -> Result<crate::session::MultiwaySession> {
    let p = prepare(raw, Path::new("embedded.toml"))?;
    build_typed_session(p.lowered, p.effective, checkpoint)
}

fn resources(p: &Prepared) -> Result<crate::session::MultiwayResourcePreflight> {
    crate::session::preflight_multiway_typed(internal(p.lowered.clone()))
}

fn check_resources(p: &Prepared) -> Result<()> {
    let r = resources(p)?;
    if !r.complete {
        return Err(spot::SpotError::new(
            spot::Code::NLH003,
            "run.memory",
            "public tree/policy arena exceeds the memory budget",
        )
        .into());
    }
    Ok(())
}

fn utility_unit(p: &Prepared) -> &'static str {
    if matches!(p.lowered.utility, mw_preflop::UtilityConfig::ChipEv) {
        "BB"
    } else {
        "prizes"
    }
}

// Find unreachable rules on the board-independent public tree. Cache exact
// states in a bounded table: collisions cause a revisit, never a false match.
fn warnings(p: &Prepared) -> Result<Vec<String>> {
    use mw_preflop::betting::BettingMenu;
    use std::hash::{DefaultHasher, Hash, Hasher};
    let rules = &p.document.spot.tree.compiled.rules;
    let mut hits = vec![false; rules.len()];
    let game = p.lowered.game.validated()?;
    let root = mw_preflop::BettingState::from_config(&game)?;
    let mut seen: Vec<Option<Vec<u8>>> = vec![None; 1 << 16];
    fn visit(
        state: mw_preflop::BettingState,
        betting: &mw_preflop::BettingConfig,
        rules: &[nlh::script::Rule<spot::TreeVar>],
        hits: &mut [bool],
        seen: &mut [Option<Vec<u8>>],
        depth: usize,
    ) -> Result<()> {
        if hits.iter().all(|hit| *hit) || state.phase.is_terminal() {
            return Ok(());
        }
        if depth > 512 {
            bail!("NLH003: tree: exceeds the P2 traversal depth limit");
        }
        let key = serde_json::to_vec(&state)?;
        let mut hash = DefaultHasher::new();
        key.hash(&mut hash);
        let slot = hash.finish() as usize & (seen.len() - 1);
        if seen[slot].as_ref() == Some(&key) {
            return Ok(());
        }
        seen[slot] = Some(key);
        let actor = state.to_act.expect("decision actor");
        for (rule, hit) in rules.iter().zip(hits.iter_mut()) {
            if rule.street == state.street
                && rule.condition.eval(&mw_preflop::tree_rules::NlhContext {
                    state: &state,
                    actor,
                })
            {
                *hit = true;
            }
        }
        let actions = state
            .legal_actions(betting)
            .map_err(|e| spot::SpotError::new(spot::Code::NLH003, "tree", e.to_string()))?;
        for action in &actions {
            let mut next = state.clone();
            next.apply_from_actions(action.clone(), &actions, betting)?;
            visit(next, betting, rules, hits, seen, depth + 1)?;
        }
        Ok(())
    }
    visit(root, &game.betting, rules, &mut hits, &mut seen, 0)?;
    Ok(rules
        .iter()
        .zip(hits)
        .enumerate()
        .filter(|(_, (_, hit))| !hit)
        .map(|(i, (rule, _))| format!("unmatched {:?} tree rule {}", rule.street, i + 1))
        .collect())
}

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
    summary.warnings = warnings(&p)?;
    let mut value = serde_json::to_value(&summary)?;
    super::p1::diagnostic_bb(&mut value);
    value["status"] = json!("valid");
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

    if with_resources {
        let r = resources(&p)?;
        value["resources"] = json!({
            "complete": r.complete, "recall": "current-street", "decisionNodes": r.decision_nodes,
            "terminalEdges": r.terminal_edges, "policyColumns": r.policy_columns,
            "policySlots": r.policy_slots, "solverStateBytes": r.solver_state_bytes,
            "memoryLimitBytes": p.lowered.run.memory_bytes, "withinLimit": r.complete,
            "icm": r.icm.map(|i| json!({"fieldPlayers": i.field_players, "paidPlaces": i.paid_places,
                "mode": if matches!(i.mode, crate::session::IcmPreflightMode::Exact) { "exact" } else { "sampled" },
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
    let session = build_typed_session(p.lowered.clone(), p.effective.clone(), Some(checkpoint))?;
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
    let config = internal(p.lowered);
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
