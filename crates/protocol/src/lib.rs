//! Wire types for the job daemon.
//!
//! The daemon does not solve anything: it creates run directories, spawns
//! `solvers solve --out` into them, and serves what those directories say
//! (`docs/app-architecture.md` R1, R2). So the wire types here are mostly a
//! projection of the run-directory contract in `formats`, reused rather than
//! restated -- a second definition of `RunState` would be a second thing to
//! keep in sync.
//!
//! What this crate adds is the shape of the *conversation*: which requests
//! exist, what a client must send, and how a client resumes an event stream
//! it was disconnected from.

use formats::{RunEvent, RunState};
use serde::{Deserialize, Serialize};

/// Bumped when a change would break an existing client.
///
/// Adding an optional response field does not; removing one, renaming one,
/// or changing what an endpoint does with the same input, does.
pub const PROTOCOL_VERSION: u16 = 1;

/// `GET /v1` -- what this daemon is and what it can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub protocol_version: u16,
    /// The `solvers` binary the daemon spawns, so a client can tell two
    /// hosts apart when their results differ.
    pub cli_version: String,
    /// How many runs may execute at once. Beyond this, runs queue.
    pub max_concurrent_runs: usize,
}

/// `POST /v1/validate` -- parse and normalize a config without running it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateRequest {
    /// The config, as text. There is no path: the daemon must not read a
    /// file the client named (R10).
    pub config_toml: String,
    /// Also build the public tree and report the arena it would need. Costs
    /// a tree walk, so it is opt-in.
    #[serde(default)]
    pub resources: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateResponse {
    /// The normalized config with every default made explicit. This is what
    /// a client submits to `POST /v1/runs`: it is self-contained, so it
    /// means the same thing on any host.
    pub effective_config_toml: String,
    /// The `solvers validate --format json` summary, passed through.
    pub summary: serde_json::Value,
}

/// `POST /v1/runs` -- create a run and start it (or queue it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunRequest {
    /// A self-contained config. A config carrying a path -- an mwtree
    /// `source`, say -- is rejected rather than resolved against the
    /// daemon's filesystem, where it would mean something else (R10).
    pub config_toml: String,
    /// Optional name for the run directory. The daemon assigns one when
    /// absent, and rejects a name that is not a single path component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunResponse {
    pub run_id: String,
    pub state: RunState,
}

/// One run, as `GET /v1/runs/{id}` and each entry of `GET /v1/runs`.
///
/// `state` is the *observed* state: a manifest still claiming `running`
/// whose process is gone reports `interrupted`. The daemon resolves that,
/// because it is the process that knows whether the pid is one of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub state: RunState,
    pub game_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<String>,
    pub config_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// Sweeps for the multiway engine, iterations for the heads-up one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<f64>,
    /// Byte offset to resume the event stream from.
    pub events_offset: u64,
    /// The run stopped with a checkpoint, so `POST /resume` would continue it.
    pub resumable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunListResponse {
    pub runs: Vec<RunSummary>,
}

/// `GET /v1/runs/{id}/events?from=OFFSET` -- a page of the event log.
///
/// The offset is a byte position in `events.jsonl`, which is append-only, so
/// a client that keeps `next_offset` picks up exactly where it stopped even
/// after a disconnect of any length. `seq` continuity is the client's check
/// that nothing was skipped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<RunEvent>,
    pub next_offset: u64,
    /// The run has stopped: this page is the end of the stream.
    pub terminal: bool,
}

/// One downloadable file in a run directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEntry {
    pub name: String,
    pub bytes: u64,
}

/// `GET /v1/runs/{id}/artifacts` -- what the run has produced so far.
///
/// Only files the run-directory contract defines are listed. A run
/// directory is not a general file share, and a client that could name any
/// path would turn the daemon into one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactListResponse {
    pub artifacts: Vec<ArtifactEntry>,
}

/// Views a solved artifact can be rendered as, matching `solvers export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SolutionView {
    Strategy,
    Actions,
    Range,
    Ev,
    Tree,
    Summary,
}

impl SolutionView {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "strategy" => Self::Strategy,
            "actions" => Self::Actions,
            "range" => Self::Range,
            "ev" => Self::Ev,
            "tree" => Self::Tree,
            "summary" => Self::Summary,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strategy => "strategy",
            Self::Actions => "actions",
            Self::Range => "range",
            Self::Ev => "ev",
            Self::Tree => "tree",
            Self::Summary => "summary",
        }
    }
}

/// Every failure the daemon reports, with a code a client can branch on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// The bearer token was missing or wrong.
    Unauthorized,
    /// No run by that id.
    NotFound,
    /// The config did not parse, normalize, or validate.
    InvalidConfig,
    /// The config carries a path the daemon refuses to resolve (R10).
    ConfigNotSelfContained,
    /// The request was well-formed but not applicable -- cancelling a run
    /// that already stopped, resuming one with no checkpoint.
    Conflict,
    /// The run exists but has not produced what was asked for -- a
    /// solution view of a run that has not finished, say.
    Unavailable,
    /// The daemon failed at something that was not the client's fault.
    Internal,
}

impl ErrorResponse {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The HTTP status this error is served with.
    pub fn http_status(&self) -> u16 {
        match self.code {
            ErrorCode::Unauthorized => 401,
            ErrorCode::NotFound => 404,
            ErrorCode::InvalidConfig | ErrorCode::ConfigNotSelfContained => 400,
            ErrorCode::Conflict => 409,
            ErrorCode::Unavailable => 409,
            ErrorCode::Internal => 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client reads `state` and `runId`; renaming either breaks it. The
    /// wire names are camelCase and this test is what says so.
    #[test]
    fn a_run_summary_uses_stable_camel_case_names() {
        let summary = RunSummary {
            run_id: "run-1".into(),
            state: RunState::Running,
            game_kind: "preflop-multiway".into(),
            config_schema: Some("solvers.multiway-preflop/v1".into()),
            config_hash: "aa".into(),
            completion: None,
            failure: None,
            progress: Some(12),
            elapsed_secs: Some(1.5),
            events_offset: 256,
            resumable: false,
        };
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["runId"], "run-1");
        assert_eq!(json["state"], "running");
        assert_eq!(json["eventsOffset"], 256);
        // Absent rather than null, so a client can treat presence as meaning.
        assert!(json.get("completion").is_none());
        assert_eq!(serde_json::from_value::<RunSummary>(json).unwrap(), summary);
    }

    /// Adding an optional field must not break a client that never sends it.
    #[test]
    fn a_create_request_needs_only_a_config() {
        let request: CreateRunRequest =
            serde_json::from_str(r#"{"configToml":"schema = \"x\"\n"}"#).unwrap();
        assert!(request.run_id.is_none());
    }

    #[test]
    fn error_codes_map_to_the_statuses_clients_branch_on() {
        for (code, status) in [
            (ErrorCode::Unauthorized, 401),
            (ErrorCode::NotFound, 404),
            (ErrorCode::InvalidConfig, 400),
            (ErrorCode::ConfigNotSelfContained, 400),
            (ErrorCode::Conflict, 409),
            (ErrorCode::Unavailable, 409),
            (ErrorCode::Internal, 500),
        ] {
            assert_eq!(ErrorResponse::new(code, "x").http_status(), status);
        }
    }

    /// The event page is the resumable half of the protocol; its two fields
    /// are what makes a disconnect recoverable.
    /// The view names are the client's vocabulary and must match the CLI's.
    #[test]
    fn solution_views_round_trip_through_their_names() {
        for view in [
            SolutionView::Strategy,
            SolutionView::Actions,
            SolutionView::Range,
            SolutionView::Ev,
            SolutionView::Tree,
            SolutionView::Summary,
        ] {
            assert_eq!(SolutionView::parse(view.as_str()), Some(view));
        }
        assert_eq!(SolutionView::parse("nonsense"), None);
    }

    #[test]
    fn an_event_page_carries_the_offset_to_resume_from() {
        let page = EventPage {
            events: Vec::new(),
            next_offset: 1024,
            terminal: false,
        };
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["nextOffset"], 1024);
        assert_eq!(json["terminal"], false);
    }
}
