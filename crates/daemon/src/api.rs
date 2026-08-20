//! Request handling, independent of the HTTP layer.
//!
//! Every handler takes a parsed request and returns a value or an
//! [`ErrorResponse`], so the routing in `http.rs` stays a translation of
//! paths and status codes, and these can be tested without a socket.

use std::path::Path;
use std::process::Command;

use formats::{RunEventLog, RunManifest, RunState, read_events};
use protocol::{
    ArtifactEntry, ArtifactListResponse, CreateRunRequest, CreateRunResponse, ErrorCode,
    ErrorResponse, EventPage, PROTOCOL_VERSION, RunListResponse, RunSummary, ServerInfo,
    SolutionView, ValidateRequest, ValidateResponse,
};

use crate::jobs::{CancelOutcome, JobCommand, JobRunner};
use crate::runs::RunsRoot;

pub type ApiResult<T> = std::result::Result<T, ErrorResponse>;

pub struct Api {
    pub runs: RunsRoot,
    pub jobs: JobRunner,
    /// The `solvers` binary. The daemon shells out for config work too, so
    /// there is exactly one implementation of the config contract (R4) and
    /// nothing here parses TOML.
    pub solver: std::path::PathBuf,
}

fn internal(error: impl std::fmt::Display) -> ErrorResponse {
    ErrorResponse::new(ErrorCode::Internal, error.to_string())
}

impl Api {
    pub fn info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: PROTOCOL_VERSION,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            max_concurrent_runs: self.jobs.max_concurrent(),
        }
    }

    /// Normalizes a config by running `solvers validate` over a temporary
    /// copy of the submitted text.
    ///
    /// The temporary file is the daemon's own, never a path the client
    /// named, so this cannot be turned into a way to read the daemon's
    /// filesystem.
    pub fn validate(&self, request: &ValidateRequest) -> ApiResult<ValidateResponse> {
        let scratch = tempfile::tempdir().map_err(internal)?;
        let config = scratch.path().join("submitted.toml");
        std::fs::write(&config, &request.config_toml).map_err(internal)?;
        let effective = scratch.path().join("effective.toml");

        let mut command = Command::new(&self.solver);
        command
            .arg("validate")
            .arg(&config)
            .arg("--format")
            .arg("json")
            .arg("--write-effective")
            .arg(&effective);
        if request.resources {
            command.arg("--resources");
        }
        let output = command.output().map_err(internal)?;
        if !output.status.success() {
            return Err(ErrorResponse::new(
                ErrorCode::InvalidConfig,
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let summary = serde_json::from_slice(&output.stdout).map_err(internal)?;
        let effective_config_toml = std::fs::read_to_string(&effective).map_err(internal)?;
        Ok(ValidateResponse {
            effective_config_toml,
            summary,
        })
    }

    pub fn list_runs(&self) -> ApiResult<RunListResponse> {
        let runs = self.runs.list().map_err(internal)?;
        Ok(RunListResponse { runs })
    }

    pub fn run(&self, run_id: &str) -> ApiResult<RunSummary> {
        if !self.runs.exists(run_id) {
            return Err(ErrorResponse::new(
                ErrorCode::NotFound,
                format!("no run {run_id:?}"),
            ));
        }
        self.runs.summary(run_id).map_err(internal)
    }

    /// Creates a run directory, writes the config into it, and submits the
    /// job.
    ///
    /// The config is normalized first, which is also what enforces R10: a
    /// config carrying an mwtree `source` cannot normalize without the file
    /// it names, so it is refused here rather than resolved against the
    /// daemon's filesystem.
    pub fn create_run(&self, request: &CreateRunRequest) -> ApiResult<CreateRunResponse> {
        let effective = self
            .validate(&ValidateRequest {
                config_toml: request.config_toml.clone(),
                resources: false,
            })
            .map_err(|error| match error.code {
                ErrorCode::InvalidConfig if mentions_a_path(&error.message) => ErrorResponse::new(
                    ErrorCode::ConfigNotSelfContained,
                    format!(
                        "the config refers to a file this daemon cannot resolve; \
                         submit the effective config instead ({})",
                        error.message
                    ),
                ),
                _ => error,
            })?;

        let directory = self
            .runs
            .allocate(request.run_id.as_deref(), formats::unix_millis())
            .map_err(|error| ErrorResponse::new(ErrorCode::Conflict, error.to_string()))?;
        std::fs::create_dir_all(&directory).map_err(internal)?;
        std::fs::write(
            directory.join(formats::RUN_CONFIG_FILE),
            &effective.effective_config_toml,
        )
        .map_err(internal)?;

        let run_id = directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        // A queued run has no process yet, so nothing has written its
        // manifest. Write it here so the run is visible -- and correctly
        // reported as queued -- from the moment it is accepted.
        let mut manifest = RunManifest::new(
            run_id.clone(),
            "pending",
            None,
            "pending",
            vec!["solve".to_string(), run_id.clone()],
        );
        manifest.state = RunState::Queued;
        manifest.started_unix_ms = None;
        manifest.write_atomic(&directory).map_err(internal)?;

        let started = self
            .jobs
            .submit(&run_id, &directory, JobCommand::Solve)
            .map_err(internal)?;
        Ok(CreateRunResponse {
            run_id,
            state: if started {
                RunState::Running
            } else {
                RunState::Queued
            },
        })
    }

    /// A page of a run's event log, starting at `from` bytes.
    pub fn events(&self, run_id: &str, from: u64) -> ApiResult<EventPage> {
        let directory = self.run_directory(run_id)?;
        let path = RunEventLog::path_in(&directory);
        let (events, next_offset) = match read_events(&path, from) {
            Ok(page) => page,
            // A run accepted a moment ago may not have written its first
            // event yet; that is an empty page, not an error.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Vec::new(), from),
            Err(error) => return Err(internal(error)),
        };
        let manifest = RunManifest::read(&directory).map_err(internal)?;
        Ok(EventPage {
            events,
            next_offset,
            terminal: manifest.observed_state().is_terminal(),
        })
    }

    pub fn cancel(&self, run_id: &str) -> ApiResult<RunSummary> {
        let summary = self.run(run_id)?;
        if summary.state.is_terminal() {
            return Err(ErrorResponse::new(
                ErrorCode::Conflict,
                format!(
                    "run {run_id:?} already stopped ({})",
                    summary.state.as_str()
                ),
            ));
        }
        match self.jobs.cancel(run_id) {
            CancelOutcome::Signalled | CancelOutcome::Dequeued => self.run(run_id),
            CancelOutcome::NotRunning => Err(ErrorResponse::new(
                ErrorCode::Conflict,
                format!("run {run_id:?} is not being run by this daemon"),
            )),
        }
    }

    pub fn resume(&self, run_id: &str) -> ApiResult<CreateRunResponse> {
        let summary = self.run(run_id)?;
        if !summary.resumable {
            return Err(ErrorResponse::new(
                ErrorCode::Conflict,
                format!(
                    "run {run_id:?} is {} with no checkpoint to resume from",
                    summary.state.as_str()
                ),
            ));
        }
        let directory = self.runs.directory(run_id).map_err(internal)?;
        let started = self
            .jobs
            .submit(run_id, &directory, JobCommand::Resume)
            .map_err(internal)?;
        Ok(CreateRunResponse {
            run_id: run_id.to_string(),
            state: if started {
                RunState::Running
            } else {
                RunState::Queued
            },
        })
    }

    /// The files a run has produced, of those the contract defines.
    ///
    /// The list is an allow-list rather than a directory listing: a run
    /// directory holds whatever the solver wrote there, and a client that
    /// could name any path would turn the daemon into a file server.
    pub fn artifacts(&self, run_id: &str) -> ApiResult<ArtifactListResponse> {
        let directory = self.run_directory(run_id)?;
        let mut artifacts = Vec::new();
        for name in ARTIFACTS {
            if let Ok(metadata) = std::fs::metadata(directory.join(name)) {
                artifacts.push(ArtifactEntry {
                    name: (*name).to_string(),
                    bytes: metadata.len(),
                });
            }
        }
        Ok(ArtifactListResponse { artifacts })
    }

    /// One artifact's bytes, with the content type to serve it as.
    pub fn artifact(&self, run_id: &str, name: &str) -> ApiResult<(Vec<u8>, &'static str)> {
        let directory = self.run_directory(run_id)?;
        if !ARTIFACTS.contains(&name) {
            return Err(ErrorResponse::new(
                ErrorCode::NotFound,
                format!("{name:?} is not a run artifact"),
            ));
        }
        let path = directory.join(name);
        let bytes = std::fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ErrorResponse::new(
                    ErrorCode::Unavailable,
                    format!("run {run_id:?} has not produced {name:?}"),
                )
            } else {
                internal(error)
            }
        })?;
        Ok((bytes, content_type(name)))
    }

    /// Renders a view of a finished run's solution artifact.
    ///
    /// Shelling out to `solvers export` keeps one implementation of what a
    /// view means, so the daemon and the CLI cannot drift into showing
    /// different numbers for the same solve (R4, R5).
    pub fn solution_view(
        &self,
        run_id: &str,
        view: SolutionView,
        csv: bool,
    ) -> ApiResult<(Vec<u8>, &'static str)> {
        let directory = self.run_directory(run_id)?;
        let solution = directory.join(formats::RUN_SOLUTION_FILE);
        if !solution.is_file() {
            return Err(ErrorResponse::new(
                ErrorCode::Unavailable,
                format!(
                    "run {run_id:?} has no {}; only a finished multiway run has one",
                    formats::RUN_SOLUTION_FILE
                ),
            ));
        }
        let output = Command::new(&self.solver)
            .arg("export")
            .arg(&solution)
            .arg(view.as_str())
            .arg("--format")
            .arg(if csv { "csv" } else { "json" })
            .output()
            .map_err(internal)?;
        if !output.status.success() {
            return Err(ErrorResponse::new(
                ErrorCode::Internal,
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok((
            output.stdout,
            if csv { "text/csv" } else { "application/json" },
        ))
    }

    fn run_directory(&self, run_id: &str) -> ApiResult<std::path::PathBuf> {
        let directory = self
            .runs
            .directory(run_id)
            .map_err(|error| ErrorResponse::new(ErrorCode::NotFound, error.to_string()))?;
        if !formats::is_run_directory(&directory) {
            return Err(ErrorResponse::new(
                ErrorCode::NotFound,
                format!("no run {run_id:?}"),
            ));
        }
        Ok(directory)
    }

    /// Restarts anything the finished jobs made room for.
    pub fn pump(&self) {
        let _ = self.jobs.pump();
    }
}

/// The files a client may download, in the order a listing reports them.
///
/// Every name comes from the run-directory contract; nothing else in the
/// directory is reachable.
const ARTIFACTS: &[&str] = &[
    formats::RUN_MANIFEST_FILE,
    formats::RUN_CONFIG_FILE,
    formats::RUN_RESULT_FILE,
    formats::RUN_PROGRESS_FILE,
    formats::RUN_EVENTS_FILE,
    formats::RUN_STRATEGY_FILE,
    formats::RUN_SOLUTION_FILE,
    formats::RUN_HU_SOLUTION_FILE,
    formats::RUN_CHECKPOINT_FILE,
    formats::RUN_HU_CHECKPOINT_FILE,
    "stdout.log",
];

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("json") => "application/json",
        // JSON Lines is not JSON: served as plain text so a client streams
        // it line by line rather than trying to parse the whole file.
        Some("jsonl") | Some("log") => "text/plain",
        Some("toml") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Whether a validation failure looks like an unresolvable file reference.
///
/// The CLI reports these as ordinary validation errors, so the daemon
/// recognizes them to give the client the more useful code. A miss only
/// costs the client a less specific error, never a wrong result.
fn mentions_a_path(message: &str) -> bool {
    message.contains("mwtree")
        || message.contains("No such file")
        || message.contains("requires a config file path")
}

/// Where the daemon looks for the CLI when it was not told.
pub fn default_solver_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("solvers")))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| Path::new("solvers").to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing outside the contract is downloadable, however it is spelled.
    #[test]
    fn only_contract_file_names_are_artifacts() {
        assert!(ARTIFACTS.contains(&"manifest.json"));
        assert!(ARTIFACTS.contains(&"solution.mwsol"));
        for outside in ["../../etc/passwd", ".cache", "run.toml.bak", ""] {
            assert!(!ARTIFACTS.contains(&outside), "{outside:?} is reachable");
        }
    }

    #[test]
    fn content_types_follow_the_file_kind() {
        assert_eq!(content_type("manifest.json"), "application/json");
        assert_eq!(content_type("events.jsonl"), "text/plain");
        assert_eq!(content_type("solution.mwsol"), "application/octet-stream");
    }

    #[test]
    fn a_validation_error_naming_a_missing_file_is_classified_as_not_self_contained() {
        assert!(mentions_a_path("reading mwtree source /x/y.mwtree"));
        assert!(mentions_a_path("No such file or directory"));
        assert!(!mentions_a_path("MWP001: rollout-kmeans was removed"));
    }
}
