//! The run registry: run directories under one root.
//!
//! There is no database. A run directory is the run (R2), so listing runs
//! means listing directories, and reading one means reading its manifest.
//! That is what lets the daemon restart without losing track of anything,
//! and what lets `solvers status` and the daemon agree without talking to
//! each other.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use formats::{RunManifest, RunState, is_run_directory, last_progress_row};
use protocol::RunSummary;

/// The directory holding every run this daemon knows about.
pub struct RunsRoot {
    path: PathBuf,
}

impl RunsRoot {
    pub fn new(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating the runs root {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolves a run id to its directory.
    ///
    /// Ids are single path components: anything with a separator or a parent
    /// reference is refused, so a request cannot address a directory outside
    /// the root.
    pub fn directory(&self, run_id: &str) -> Result<PathBuf> {
        let mut components = Path::new(run_id).components();
        let only = components.next();
        if components.next().is_some() || run_id.is_empty() {
            anyhow::bail!("run id {run_id:?} must be a single path component");
        }
        match only {
            Some(std::path::Component::Normal(name)) => Ok(self.path.join(name)),
            _ => anyhow::bail!("run id {run_id:?} must be a single path component"),
        }
    }

    pub fn exists(&self, run_id: &str) -> bool {
        self.directory(run_id)
            .map(|directory| is_run_directory(&directory))
            .unwrap_or(false)
    }

    /// Every run directory directly under the root, oldest id first.
    pub fn list(&self) -> Result<Vec<RunSummary>> {
        let mut directories: Vec<PathBuf> = std::fs::read_dir(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_run_directory(path))
            .collect();
        directories.sort();
        directories
            .iter()
            .map(|directory| self.summarize(directory))
            .collect()
    }

    pub fn summary(&self, run_id: &str) -> Result<RunSummary> {
        self.summarize(&self.directory(run_id)?)
    }

    fn summarize(&self, directory: &Path) -> Result<RunSummary> {
        let manifest = RunManifest::read(directory)
            .with_context(|| format!("reading the manifest in {}", directory.display()))?;
        let observed = manifest.observed_state();
        let progress_row = last_progress_row(directory)
            .with_context(|| format!("reading progress in {}", directory.display()))?;
        let events_offset = std::fs::metadata(formats::RunEventLog::path_in(directory))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let checkpoint = directory.join(formats::RUN_CHECKPOINT_FILE).is_file()
            || directory.join(formats::RUN_HU_CHECKPOINT_FILE).is_file();

        Ok(RunSummary {
            run_id: manifest.run_id,
            state: observed,
            game_kind: manifest.game_kind,
            config_schema: manifest.config_schema,
            config_hash: manifest.config_hash,
            completion: manifest.completion,
            failure: manifest.failure,
            // Multiway rows count sweeps, the heads-up engine counts
            // iterations; both answer "how far has this got".
            progress: progress_row
                .as_ref()
                .and_then(|row| row.get("sweeps").or_else(|| row.get("iteration"))?.as_u64()),
            elapsed_secs: progress_row
                .as_ref()
                .and_then(|row| row.get("elapsedSecs")?.as_f64()),
            events_offset,
            resumable: checkpoint
                && matches!(
                    observed,
                    RunState::Canceled | RunState::Interrupted | RunState::Failed
                ),
        })
    }

    /// Picks an unused directory name for a new run.
    ///
    /// `requested` is validated the same way as any other id. The generated
    /// form sorts chronologically, since the listing is ordered by name.
    pub fn allocate(&self, requested: Option<&str>, now_unix_ms: u64) -> Result<PathBuf> {
        if let Some(requested) = requested {
            let directory = self.directory(requested)?;
            if directory.exists() {
                anyhow::bail!("run {requested:?} already exists");
            }
            return Ok(directory);
        }
        for suffix in 0..1000 {
            let candidate = if suffix == 0 {
                format!("run-{now_unix_ms}")
            } else {
                format!("run-{now_unix_ms}-{suffix}")
            };
            let directory = self.path.join(&candidate);
            if !directory.exists() {
                return Ok(directory);
            }
        }
        anyhow::bail!(
            "could not find an unused run id under {}",
            self.path.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> (tempfile::TempDir, RunsRoot) {
        let directory = tempfile::tempdir().unwrap();
        let root = RunsRoot::new(directory.path()).unwrap();
        (directory, root)
    }

    fn write_run(root: &RunsRoot, id: &str, state: RunState) {
        let directory = root.directory(id).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        let mut manifest = RunManifest::new(
            id,
            "preflop-multiway",
            Some("solvers.multiway-preflop/v1".into()),
            "aa",
            vec!["solve".into()],
        );
        if state.is_terminal() {
            manifest.finish(state, Some("target-reached".into()));
        }
        manifest.write_atomic(&directory).unwrap();
        std::fs::write(
            directory.join(formats::RUN_PROGRESS_FILE),
            "{\"sweeps\":7,\"elapsedSecs\":0.5}\n",
        )
        .unwrap();
    }

    /// A run id must not be able to address anything outside the root.
    #[test]
    fn ids_that_escape_the_root_are_refused() {
        let (_guard, root) = root();
        for bad in ["../escape", "a/b", "/absolute", "", ".", ".."] {
            assert!(root.directory(bad).is_err(), "{bad:?} was accepted");
        }
        assert!(root.directory("run-1").is_ok());
    }

    #[test]
    fn listing_reports_each_run_directory_once() {
        let (_guard, root) = root();
        write_run(&root, "run-a", RunState::Completed);
        write_run(&root, "run-b", RunState::Running);
        std::fs::create_dir_all(root.path().join("not-a-run")).unwrap();

        let runs = root.list().unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, "run-a");
        assert_eq!(runs[0].state, RunState::Completed);
        assert_eq!(runs[0].progress, Some(7));
    }

    /// A run whose owner died reports `interrupted`, not `running`, and the
    /// daemon must serve that resolved state rather than the recorded one.
    #[cfg(unix)]
    #[test]
    fn an_abandoned_run_is_reported_as_interrupted() {
        let (_guard, root) = root();
        let directory = root.directory("run-c").unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        let mut manifest = RunManifest::new("run-c", "kuhn", None, "aa", vec!["solve".into()]);
        manifest.pid = 0;
        manifest.write_atomic(&directory).unwrap();
        std::fs::write(directory.join(formats::RUN_HU_CHECKPOINT_FILE), b"x").unwrap();

        let summary = root.summary("run-c").unwrap();
        assert_eq!(summary.state, RunState::Interrupted);
        assert!(summary.resumable);
    }

    #[test]
    fn allocate_refuses_an_existing_id_and_generates_a_free_one() {
        let (_guard, root) = root();
        write_run(&root, "taken", RunState::Completed);
        assert!(root.allocate(Some("taken"), 1).is_err());
        assert_eq!(
            root.allocate(Some("fresh"), 1).unwrap(),
            root.path().join("fresh")
        );

        let generated = root.allocate(None, 1_700_000_000_000).unwrap();
        assert!(
            generated
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("run-1700000000000")
        );
    }
}
