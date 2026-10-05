//! Derive I/O and rendering; poker computation remains in product crates.
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

#[derive(Debug)]
pub(crate) struct ArtifactError(String);
impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ArtifactError {}

#[derive(Debug)]
pub(crate) struct InputError(anyhow::Error);
impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#}", self.0)
    }
}
// Display already renders the whole chain, so no source is exposed.
impl std::error::Error for InputError {}

pub fn run(
    run: &Path,
    line: &str,
    board: &str,
    base: Option<&Path>,
    out: Option<&Path>,
) -> Result<()> {
    let solution = run.join(runfiles::RUN_SOLUTION_FILE);
    let bytes = std::fs::read(&solution)
        .map_err(|e| ArtifactError(format!("reading {}: {e}", solution.display())))?;
    let manifest = runfiles::RunManifest::read(run)
        .map_err(|e| ArtifactError(format!("reading P2 run manifest: {e}")))?;
    let derived = mw_preflop::derive::derive(&solution, line, board)?;
    let base_text = base
        .map(std::fs::read_to_string)
        .transpose()
        .context("reading derive base")?;
    let ranges: BTreeMap<_, _> = derived
        .seats
        .iter()
        .map(|seat| (seat.position.clone(), seat.range.clone()))
        .collect();
    let prepared_input = (|| -> Result<_> {
        let (document, warnings) = spot::derive::assemble(
            &derived.config_toml,
            base_text.as_deref().zip(base),
            line,
            board,
            &ranges,
            spot::DerivedFrom {
                run_id: Some(manifest.run_id),
                solution_hash: Some(blake3::hash(&bytes).to_hex().to_string()),
                line: Some(line.into()),
                board: Some(board.into()),
            },
        )?;
        let effective = document.normalize(&hu_postflop::input::P1Sections)?;
        let prepared = hu_postflop::prepare::prepare(&effective, Path::new("derived.toml"))
            .context("validating derived P1 input")?;
        Ok((prepared, warnings))
    })();
    let (prepared, mut warnings) = prepared_input.map_err(InputError)?;
    warnings.extend(hu_postflop::prepare::warnings(&prepared));
    if manifest.state != runfiles::RunState::Completed {
        warnings.push(format!(
            "P2 run state is {}, not completed; using saved solution",
            manifest.state.as_str()
        ));
    }
    for seat in &derived.seats {
        if !seat.unvisited_classes.is_empty() {
            warnings.push(format!(
                "{}: unvisited classes dropped ({}): {}",
                seat.position,
                seat.unvisited_classes.len(),
                seat.unvisited_classes.join(", ")
            ));
        }
    }
    // All preparation must succeed before opening the requested output.
    if let Some(path) = out {
        std::fs::write(path, &prepared.effective)
            .with_context(|| format!("writing {}", path.display()))?;
    } else {
        use std::io::Write;
        std::io::stdout()
            .lock()
            .write_all(prepared.effective.as_bytes())?;
    }
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    for seat in &derived.seats {
        eprintln!(
            "derive {}: {} combos with positive weight, {} unvisited classes",
            seat.position,
            seat.weights.iter().filter(|&&w| w > 0.0).count(),
            seat.unvisited_classes.len()
        );
    }
    Ok(())
}
