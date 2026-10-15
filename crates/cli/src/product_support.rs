//! Explicit product boundary for the P1-only distribution.
use anyhow::Result;
use std::path::Path;

const EXCLUDED: &str =
    "this build excludes P2 (Multiway Preflop); use a build with the `p2` feature";

pub(crate) fn input_error() -> anyhow::Error {
    spot::SpotError::new(spot::Code::NLH005, "spot", EXCLUDED).into()
}

pub(crate) fn derive_error() -> anyhow::Error {
    spot::SpotError::new(
        spot::Code::NLH005,
        "derive",
        format!("derive requires P2; {EXCLUDED}"),
    )
    .into()
}

#[derive(Debug)]
pub(crate) struct ArtifactError;

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(EXCLUDED)
    }
}

impl std::error::Error for ArtifactError {}

pub(crate) fn artifact_error() -> anyhow::Error {
    ArtifactError.into()
}

pub(crate) fn require_input(raw: &str, path: &Path) -> Result<()> {
    if spot::Document::parse(raw, path)?.spot.product == spot::Product::MultiwayPreflop {
        return Err(input_error());
    }
    Ok(())
}

pub(crate) fn require_artifact(path: &Path) -> Result<()> {
    if path
        .extension()
        .is_some_and(|extension| extension == "mwsol" || extension == "mwckpt")
        || path.is_dir()
            && path.join(runfiles::RUN_MANIFEST_FILE).is_file()
            && runfiles::RunManifest::read(path)?.game_kind == "mw-preflop"
    {
        return Err(artifact_error());
    }
    Ok(())
}
