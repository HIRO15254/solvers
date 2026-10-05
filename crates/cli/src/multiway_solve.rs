//! CLI persistence and rendering around the product-owned P2 run loop.
use anyhow::{Context, Result, anyhow};
use mw_preflop::input::Lowered;
use mw_preflop::metrics::MultiwayMetricsWriter;
pub use mw_preflop::run::MultiwayRunObservation;
use mw_preflop::run::{self, CompletionStatus, Diagnostic, ProgressPoint, RunRequest};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn print_abstraction(ready: mw_preflop::session::AbstractionReady) {
    eprintln!(
        "ehs2 tables: {} in {:.2}s",
        if ready.cached { "loaded" } else { "built" },
        ready.secs
    );
}

pub(crate) fn print_diagnostic(diagnostic: Diagnostic) {
    match diagnostic {
        Diagnostic::Abstraction(ready) => print_abstraction(ready),
        Diagnostic::Warning(warning) => eprintln!("warning: {warning}"),
        Diagnostic::Arena(a) => eprintln!(
            "policy arena committed before sweep 0: nodes={} columns={} slots={} bytes={}",
            a.nodes, a.columns, a.slots, a.bytes
        ),
        Diagnostic::Underutilized {
            seats,
            sweep_batch,
            threads,
        } => eprintln!(
            "hint: {seats} seats x solver.batch_sweeps {sweep_batch} = {} parallel traversals \
                 < {threads} threads; solver.batch_sweeps = {} would use every core",
            seats * sweep_batch,
            threads.div_ceil(seats)
        ),
        Diagnostic::SamplesDoubled {
            max_width,
            threshold,
            before,
            after,
        } => eprintln!(
            "stop-rule: max CI width {:.6} exceeds threshold {:.6}; \
                     doubling evaluation samples {before} -> {after}",
            max_width, threshold
        ),
        Diagnostic::Evaluation(now) => eprintln!(
            "sweeps={:>8} traversals={:>10} infosets={:>9} regret_proxy={:.3e} memory={}MiB",
            now.sweeps,
            now.traversals,
            now.infosets,
            if now.average_positive_regret.is_empty() {
                0.0
            } else {
                now.average_positive_regret.iter().sum::<f64>()
                    / now.average_positive_regret.len() as f64
            },
            now.memory_bytes / (1024 * 1024)
        ),
        Diagnostic::ResourceCheckpoint(path) => {
            eprintln!("resource_limit checkpoint: {}", path.display())
        }
    }
}

/// Starts a production solve and publishes owned progress observations.
#[allow(clippy::too_many_arguments)]
pub fn run_observed(
    raw_config: &str,
    config: Lowered,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: Option<&Path>,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    emit_progress: bool,
    observer: &mut dyn FnMut(MultiwayRunObservation),
) -> Result<()> {
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        checkpoint_path,
        config_hash,
        mwsol_path,
        cancel,
        None,
        false,
        emit_progress,
        Some(observer),
    )
}

/// Resume with an observer, so a resumed segment records the same run
/// events as a fresh solve.
#[allow(clippy::too_many_arguments)]
pub fn resume_observed(
    raw_config: &str,
    config: Lowered,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: &Path,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    reset_confirmations: bool,
    emit_progress: bool,
    observer: &mut dyn FnMut(MultiwayRunObservation),
) -> Result<()> {
    run_inner(
        raw_config,
        config,
        output,
        metrics_path,
        Some(checkpoint_path),
        config_hash,
        mwsol_path,
        cancel,
        Some(checkpoint_path),
        reset_confirmations,
        emit_progress,
        Some(observer),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    raw_config: &str,
    config: Lowered,
    output: Option<&Path>,
    metrics_path: Option<&Path>,
    checkpoint_path: Option<&Path>,
    config_hash: [u8; 32],
    mwsol_path: Option<&Path>,
    cancel: Option<&AtomicBool>,
    resume_checkpoint: Option<&Path>,
    reset_confirmations: bool,
    emit_progress: bool,
    mut observer: Option<&mut dyn FnMut(MultiwayRunObservation)>,
) -> Result<()> {
    let cache_root = crate::cache::root();
    let request = RunRequest {
        raw_config,
        config,
        output,
        metrics_path,
        checkpoint_path,
        config_hash,
        mwsol_path,
        cancel,
        cache_root: cache_root.as_deref(),
        emit_progress,
    };
    let mut writer = None;
    let mut observe = |observation: MultiwayRunObservation| -> Result<()> {
        match &observation {
            MultiwayRunObservation::ProgressOpened => {
                writer = metrics_path
                    .map(MultiwayMetricsWriter::create_or_append)
                    .transpose()
                    .context("opening multiway metrics")?;
            }
            MultiwayRunObservation::Progress {
                metrics: row,
                point,
            } => {
                if let Some(writer) = writer.as_mut() {
                    writer.append(row).context(match point {
                        ProgressPoint::Resume => "writing resume progress event",
                        ProgressPoint::Quality => "writing multiway metrics",
                        ProgressPoint::Checkpoint => "writing checkpoint progress event",
                        ProgressPoint::Final => "writing final multiway metrics",
                    })?;
                }
            }
            _ => {
                if let Some(observer) = observer.as_deref_mut() {
                    observer(observation);
                }
            }
        }
        Ok(())
    };
    let result = if let Some(checkpoint) = resume_checkpoint {
        run::resume(
            request,
            checkpoint,
            reset_confirmations,
            &mut observe,
            &mut print_diagnostic,
        )?
    } else {
        run::run(request, &mut observe, &mut print_diagnostic)?
    };
    let json = serde_json::to_string_pretty(&result)?;
    if let Some(path) = output {
        write_atomic(path, json.as_bytes())
            .with_context(|| format!("writing {}", path.display()))?;
    } else {
        println!("{json}");
    }
    if emit_progress {
        let exit_code = match result.status {
            CompletionStatus::ResourceLimit => 75,
            CompletionStatus::Cancelled => 130,
            _ => 0,
        };
        crate::CLI_EXIT_CODE.store(exit_code, Ordering::SeqCst);
    }
    Ok(())
}
fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(contents)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| anyhow!(error.error))?;
    Ok(())
}
