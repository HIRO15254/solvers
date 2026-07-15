//! Authenticated loopback HTTP bridge for the browser UI.
//!
//! This remains inside the CLI binary so the bridge can reuse the exact
//! config parser, solver entry point, metrics JSONL, and result JSON.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read, Write as _};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context, Result, anyhow};
use cards::Player;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::config::{
    AlgorithmSection, GameSection, RakeSection, SolveConfig, StorageKind, UtilitySection,
};
use crate::sol::SolStreets;

const API_VERSION: u32 = 1;
const API_VERSION_V2: u32 = 2;
const BODY_LIMIT: usize = 64 * 1024;
const MAX_ITERATIONS: u64 = 1_000_000;
const MAX_STACK_BB: f64 = 1_000.0;
const MAX_RAISES: u32 = 16;
const MAX_SIZES_PER_LEVEL: usize = 16;
const MAX_FACTOR_LEVELS: usize = 16;
const MAX_RANGE_BYTES: usize = 4 * 1024;
const MAX_GRAMMAR_PATHS: u64 = 100_000;
const MAX_STORAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MULTIWAY_BUCKETS: u16 = 4_096;
const MAX_MULTIWAY_ROLLOUT_SAMPLES: u32 = 1_000_000;
const MAX_MULTIWAY_EVALUATION_SAMPLES: u64 = 1_000_000;
const MAX_MULTIWAY_ICM_SAMPLES: u64 = 1_000_000;
const DEFAULT_STRATEGY_PAGE_SIZE: usize = 50;
const MAX_STRATEGY_PAGE_SIZE: usize = 100;
const TOKEN_BYTES: usize = 32;
const JOB_ID_BYTES: usize = 16;

pub fn run(origin: &str, port: u16, threads: Option<usize>) -> Result<()> {
    validate_origin(origin)?;

    if threads.is_some_and(|count| !(1..=256).contains(&count)) {
        return Err(anyhow!("--threads must be from 1 through 256"));
    }

    if let Some(count) = threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(count)
            .build_global()
            .map_err(|error| anyhow!("initializing the Rayon thread pool: {error}"))?;
    }

    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    let local_addr = listener
        .local_addr()
        .context("reading bridge listen address")?;
    let actual_port = local_addr.port();
    let server = Server::from_listener(listener, None)
        .map_err(|error| anyhow!("starting loopback HTTP server: {error}"))?;

    let jobs_dir = tempfile::tempdir().context("creating bridge job directory")?;
    let cache_dir = std::env::temp_dir().join("solvers-bridge-cache-v1");
    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("creating bridge cache directory {}", cache_dir.display()))?;
    let cache_dir = std::fs::canonicalize(&cache_dir)
        .with_context(|| format!("resolving bridge cache directory {}", cache_dir.display()))?;

    let state = Arc::new(BridgeState {
        jobs: Mutex::new(HashMap::new()),
        jobs_dir,
        equity_cache: cache_dir.join("preflop_equity.bin"),
        threads,
    });
    let token = random_hex(TOKEN_BYTES);
    let expected_host = format!("127.0.0.1:{actual_port}");

    println!("bridge: url=http://{expected_host} token={token} origin={origin}");
    std::io::stdout()
        .flush()
        .context("flushing bridge connection details")?;

    for request in server.incoming_requests() {
        if let Err(error) = handle_request(request, &state, origin, &expected_host, &token) {
            eprintln!("bridge request error: {error:#}");
        }
    }
    Ok(())
}

struct BridgeState {
    jobs: Mutex<HashMap<String, JobRecord>>,
    // Kept alive while background jobs may use their managed files.
    jobs_dir: tempfile::TempDir,
    equity_cache: PathBuf,
    threads: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobState {
    Running,
    Cancelling,
    Cancelled,
    ResourceLimit,
    Succeeded,
    Failed,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Cancelling => "cancelling",
            JobState::Cancelled => "cancelled",
            JobState::Succeeded => "succeeded",
            JobState::ResourceLimit => "resource_limit",
            JobState::Failed => "failed",
        }
    }

    fn is_active(self) -> bool {
        matches!(self, JobState::Running | JobState::Cancelling)
    }

    fn has_result(self) -> bool {
        matches!(
            self,
            JobState::Succeeded | JobState::Cancelled | JobState::ResourceLimit
        )
    }
}

struct JobRecord {
    state: JobState,
    metrics_path: PathBuf,
    result_path: PathBuf,
    mwsol_path: Option<PathBuf>,
    checkpoint_path: Option<PathBuf>,
    cancel: Option<Arc<AtomicBool>>,
    error: Option<ErrorBody>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    HeadsUp,
    Multiway,
}

struct SanitizedConfig {
    toml: String,
    schema_version: u16,
    kind: JobKind,
}
#[derive(Clone, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse<'a> {
    service: &'a str,
    version: &'a str,
    api_version: u32,
    busy: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthCapabilities {
    max_players: u8,
    max_icm_field: u8,
    exact_icm_field: u8,
    config_schemas: [&'static str; 2],
    result_schemas: [u16; 2],
    stages: [&'static str; 3],
    artifacts: [&'static str; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponseV2<'a> {
    service: &'a str,
    version: &'a str,
    api_version: u32,
    busy: bool,
    capabilities: HealthCapabilities,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidationResponse {
    valid: bool,
    schema_version: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateJobRequest {
    config_toml: String,
}

#[derive(Serialize)]
struct CreateJobResponse<'a> {
    id: &'a str,
    status: &'a str,
}

#[derive(Serialize)]
struct CancelJobResponse<'a> {
    id: &'a str,
    status: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressResponse {
    iteration: u64,
    elapsed_secs: f64,
    expl_p0: f64,
    expl_p1: f64,
    nash_conv: f64,
}

impl From<formats::MetricsRow> for ProgressResponse {
    fn from(row: formats::MetricsRow) -> Self {
        ProgressResponse {
            iteration: row.iteration,
            elapsed_secs: row.elapsed_secs,
            expl_p0: row.expl_p0,
            expl_p1: row.expl_p1,
            nash_conv: row.nash_conv,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JobStatusResponse {
    id: String,
    status: String,
    progress: Option<serde_json::Value>,
    result_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkpoint_url: Option<String>,
    error: Option<ErrorBody>,
}

#[derive(Serialize)]
struct StrategyPageItem<'a> {
    key: &'a formats::MultiwayStrategyKey,
    public_history: Vec<formats::MultiwayHistoryAction>,
    actions: &'a [String],
    probabilities: &'a [f32],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StrategyPageResponse<'a> {
    items: Vec<StrategyPageItem<'a>>,
    next_cursor: Option<String>,
}

#[derive(Clone)]
enum Route {
    Health,
    HealthV2,
    ValidateV2,
    Jobs,
    JobsV2,
    Job(String),
    JobV2(String),
    Result(String),
    ResultV2(String),
    CancelV2(String),
    StrategiesV2(String, String),
    CheckpointV2(String),
    Unknown,
}

fn parse_route(url: &str) -> Route {
    if url.contains('#') {
        return Route::Unknown;
    }
    let (path, query) = url
        .split_once('?')
        .map_or((url, None), |(path, query)| (path, Some(query)));
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["", "v1", "health"] if query.is_none() => Route::Health,
        ["", "v1", "jobs"] if query.is_none() => Route::Jobs,
        ["", "v1", "jobs", id] if query.is_none() && valid_job_id(id) => {
            Route::Job((*id).to_string())
        }
        ["", "v1", "jobs", id, "result"] if query.is_none() && valid_job_id(id) => {
            Route::Result((*id).to_string())
        }
        ["", "v2", "health"] if query.is_none() => Route::HealthV2,
        ["", "v2", "validate"] if query.is_none() => Route::ValidateV2,
        ["", "v2", "jobs"] if query.is_none() => Route::JobsV2,
        ["", "v2", "jobs", id] if query.is_none() && valid_job_id(id) => {
            Route::JobV2((*id).to_string())
        }
        ["", "v2", "jobs", id, "result"] if query.is_none() && valid_job_id(id) => {
            Route::ResultV2((*id).to_string())
        }
        ["", "v2", "jobs", id, "cancel"] if query.is_none() && valid_job_id(id) => {
            Route::CancelV2((*id).to_string())
        }
        ["", "v2", "jobs", id, "checkpoint"] if query.is_none() && valid_job_id(id) => {
            Route::CheckpointV2((*id).to_string())
        }
        ["", "v2", "jobs", id, "strategies"] if valid_job_id(id) => {
            Route::StrategiesV2((*id).to_string(), query.unwrap_or_default().to_string())
        }
        _ => Route::Unknown,
    }
}

fn valid_job_id(id: &str) -> bool {
    id.len() == JOB_ID_BYTES * 2 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn handle_request(
    request: Request,
    state: &Arc<BridgeState>,
    origin: &str,
    expected_host: &str,
    token: &str,
) -> Result<()> {
    if single_header(&request, "Host") != Some(expected_host) {
        return respond_error(
            request,
            403,
            "forbidden_host",
            "Host must match the bridge loopback address.",
            None,
        );
    }
    if single_header(&request, "Origin") != Some(origin) {
        return respond_error(
            request,
            403,
            "forbidden_origin",
            "Origin is not allowed by this bridge.",
            None,
        );
    }
    if request.method() == &Method::Options {
        return handle_preflight(request, origin);
    }

    let authorized = single_header(&request, "Authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|provided| constant_time_eq(provided.as_bytes(), token.as_bytes()));
    if !authorized {
        return respond_error(
            request,
            401,
            "unauthorized",
            "A valid bearer token is required.",
            Some(origin),
        );
    }

    let route = parse_route(request.url());
    let method = request.method().clone();
    match (method, route) {
        (Method::Get, Route::Health) => {
            let busy = jobs(state).values().any(|job| job.state.is_active());
            respond_json(
                request,
                200,
                &HealthResponse {
                    service: "solvers",
                    version: env!("CARGO_PKG_VERSION"),
                    api_version: API_VERSION,
                    busy,
                },
                Some(origin),
            )
        }
        (Method::Get, Route::HealthV2) => {
            let busy = jobs(state).values().any(|job| job.state.is_active());
            respond_json(
                request,
                200,
                &HealthResponseV2 {
                    service: "solvers",
                    version: env!("CARGO_PKG_VERSION"),
                    api_version: API_VERSION_V2,
                    busy,
                    capabilities: HealthCapabilities {
                        max_players: 9,
                        max_icm_field: 100,
                        exact_icm_field: 15,
                        config_schemas: ["hu-v1", "multiway-v2"],
                        result_schemas: [1, 2],
                        stages: ["preflop", "all-streets", "tournament-icm"],
                        artifacts: ["result", "strategies", "checkpoint"],
                    },
                },
                Some(origin),
            )
        }
        (Method::Post, Route::Jobs) => handle_create_job(request, state, origin, 1),
        (Method::Post, Route::JobsV2) => handle_create_job(request, state, origin, 2),
        (Method::Post, Route::ValidateV2) => handle_validate(request, state, origin),
        (Method::Get, Route::Job(id)) => handle_job_status(request, state, origin, &id, 1),
        (Method::Get, Route::JobV2(id)) => handle_job_status(request, state, origin, &id, 2),
        (Method::Get, Route::Result(id)) => handle_job_result(request, state, origin, &id),
        (Method::Get, Route::ResultV2(id)) => handle_job_result(request, state, origin, &id),
        (Method::Post, Route::CancelV2(id)) => handle_cancel_job(request, state, origin, &id),
        (Method::Get, Route::StrategiesV2(id, query)) => {
            handle_strategies(request, state, origin, &id, &query)
        }
        (Method::Get, Route::CheckpointV2(id)) => handle_checkpoint(request, state, origin, &id),
        _ => respond_error(
            request,
            404,
            "not_found",
            "The requested bridge endpoint does not exist.",
            Some(origin),
        ),
    }
}

fn handle_preflight(request: Request, origin: &str) -> Result<()> {
    let requested_method = single_header(&request, "Access-Control-Request-Method");
    let route = parse_route(request.url());
    let route_matches = matches!(
        (requested_method, route),
        (
            Some("GET"),
            Route::Health
                | Route::HealthV2
                | Route::Job(_)
                | Route::JobV2(_)
                | Route::Result(_)
                | Route::ResultV2(_)
                | Route::StrategiesV2(_, _)
                | Route::CheckpointV2(_)
        ) | (
            Some("POST"),
            Route::Jobs | Route::JobsV2 | Route::ValidateV2 | Route::CancelV2(_)
        )
    );
    if !route_matches {
        return respond_error(
            request,
            403,
            "invalid_preflight",
            "The requested preflight method or endpoint is not allowed.",
            None,
        );
    }

    let Some(requested_headers) = single_header(&request, "Access-Control-Request-Headers") else {
        return respond_error(
            request,
            403,
            "invalid_preflight",
            "Preflight must request the Authorization header.",
            None,
        );
    };
    let headers: Vec<String> = requested_headers
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    if !headers.iter().any(|value| value == "authorization")
        || headers
            .iter()
            .any(|value| value != "authorization" && value != "content-type")
    {
        return respond_error(
            request,
            403,
            "invalid_preflight",
            "Preflight requested unsupported headers.",
            None,
        );
    }

    let mut response = byte_response(204, Vec::new(), "application/json", Some(origin));
    response.add_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
    response.add_header(header(
        "Access-Control-Allow-Headers",
        "Authorization, Content-Type",
    ));
    response.add_header(header("Access-Control-Max-Age", "600"));
    response.add_header(header("Access-Control-Allow-Private-Network", "true"));
    request
        .respond(response)
        .context("writing preflight response")
}

fn handle_validate(mut request: Request, state: &Arc<BridgeState>, origin: &str) -> Result<()> {
    let content_type = single_header(&request, "Content-Type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if content_type != Some("application/json") {
        return respond_error(
            request,
            415,
            "unsupported_media_type",
            "Content-Type must be application/json.",
            Some(origin),
        );
    }
    if request
        .body_length()
        .is_some_and(|length| length > BODY_LIMIT)
    {
        return respond_error(
            request,
            413,
            "body_too_large",
            "Request body exceeds the 64 KiB limit.",
            Some(origin),
        );
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take((BODY_LIMIT + 1) as u64)
        .read_to_end(&mut body)
        .context("reading request body")?;
    if body.len() > BODY_LIMIT {
        return respond_error(
            request,
            413,
            "body_too_large",
            "Request body exceeds the 64 KiB limit.",
            Some(origin),
        );
    }
    let payload: CreateJobRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return respond_error(
                request,
                400,
                "invalid_config",
                &format!("Request JSON is invalid: {error}"),
                Some(origin),
            );
        }
    };
    let sanitized = match sanitize_config(
        &payload.config_toml,
        &state.equity_cache,
        state.threads,
        true,
    ) {
        Ok(config) => config,
        Err(message) => {
            return respond_error(request, 400, "invalid_config", &message, Some(origin));
        }
    };
    respond_json(
        request,
        200,
        &ValidationResponse {
            valid: true,
            schema_version: sanitized.schema_version,
        },
        Some(origin),
    )
}

fn handle_create_job(
    mut request: Request,
    state: &Arc<BridgeState>,
    origin: &str,
    api: u8,
) -> Result<()> {
    let content_type = single_header(&request, "Content-Type")
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if content_type != Some("application/json") {
        return respond_error(
            request,
            415,
            "unsupported_media_type",
            "Content-Type must be application/json.",
            Some(origin),
        );
    }

    if request
        .body_length()
        .is_some_and(|length| length > BODY_LIMIT)
    {
        return respond_error(
            request,
            413,
            "body_too_large",
            "Request body exceeds the 64 KiB limit.",
            Some(origin),
        );
    }

    let mut body = Vec::new();
    request
        .as_reader()
        .take((BODY_LIMIT + 1) as u64)
        .read_to_end(&mut body)
        .context("reading request body")?;
    if body.len() > BODY_LIMIT {
        return respond_error(
            request,
            413,
            "body_too_large",
            "Request body exceeds the 64 KiB limit.",
            Some(origin),
        );
    }

    let payload: CreateJobRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            return respond_error(
                request,
                400,
                "invalid_config",
                &format!("Request JSON is invalid: {error}"),
                Some(origin),
            );
        }
    };

    let sanitized = match sanitize_config(
        &payload.config_toml,
        &state.equity_cache,
        state.threads,
        api >= 2,
    ) {
        Ok(config) => config,
        Err(message) => {
            return respond_error(request, 400, "invalid_config", &message, Some(origin));
        }
    };

    if jobs(state).values().any(|job| job.state.is_active()) {
        return respond_error(
            request,
            409,
            "busy",
            "The bridge already has a running job.",
            Some(origin),
        );
    }

    let id = random_hex(JOB_ID_BYTES);
    let config_path = state.jobs_dir.path().join(format!("{id}.toml"));
    let metrics_path = state.jobs_dir.path().join(format!("{id}.jsonl"));
    let result_path = state.jobs_dir.path().join(format!("{id}.json"));
    let is_multiway = sanitized.kind == JobKind::Multiway;
    let mwsol_path = is_multiway.then(|| state.jobs_dir.path().join(format!("{id}.mwsol")));
    let cancel = is_multiway.then(|| Arc::new(AtomicBool::new(false)));
    let checkpoint_path = is_multiway.then(|| state.jobs_dir.path().join(format!("{id}.mwckpt")));
    let config_toml = sanitized.toml;
    std::fs::write(&config_path, &config_toml)
        .with_context(|| format!("writing managed config {}", config_path.display()))?;

    jobs(state).insert(
        id.clone(),
        JobRecord {
            state: JobState::Running,
            metrics_path: metrics_path.clone(),
            result_path: result_path.clone(),
            mwsol_path: mwsol_path.clone(),
            cancel: cancel.clone(),
            checkpoint_path: checkpoint_path.clone(),
            error: None,
        },
    );

    let worker_state = Arc::clone(state);
    let worker_id = id.clone();
    let spawn_result = std::thread::Builder::new()
        .name(format!("solver-job-{id}"))
        .spawn(move || {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                    if is_multiway {
                        let config: SolveConfig = toml::from_str(&config_toml)
                            .context("parsing managed multiway config")?;
                        let config_hash = formats::config_hash(config_toml.as_bytes());
                        crate::multiway_solve::run(
                            &config_toml,
                            config,
                            Some(&result_path),
                            Some(&metrics_path),
                            checkpoint_path.as_deref(),
                            config_hash,
                            mwsol_path.as_deref(),
                            cancel.as_deref(),
                        )
                    } else {
                        crate::solve::run(
                            &config_path,
                            Some(&result_path),
                            &["".to_string()],
                            Some(&metrics_path),
                            None,
                            None,
                            None,
                            SolStreets::NoRivers,
                        )
                    }
                }));
            let completed_state = is_multiway
                .then(|| multiway_result_state(&result_path))
                .flatten()
                .unwrap_or(JobState::Succeeded);
            let (new_state, error) = match outcome {
                Ok(Ok(())) => (completed_state, None),
                Ok(Err(error)) => (
                    JobState::Failed,
                    Some(ErrorBody {
                        code: "solve_failed".to_string(),
                        message: format!("{error:#}"),
                    }),
                ),
                Err(_) => (
                    JobState::Failed,
                    Some(ErrorBody {
                        code: "solve_failed".to_string(),
                        message: "The solver panicked while processing this job.".to_string(),
                    }),
                ),
            };
            if let Some(job) = jobs(&worker_state).get_mut(&worker_id) {
                job.state = new_state;
                job.error = error;
            }
        });

    if let Err(error) = spawn_result {
        if let Some(job) = jobs(state).get_mut(&id) {
            job.state = JobState::Failed;
            job.error = Some(ErrorBody {
                code: "solve_failed".to_string(),
                message: "Could not start the solver worker.".to_string(),
            });
        }
        return respond_error(
            request,
            500,
            "internal_error",
            &format!("Could not start solver worker: {error}"),
            Some(origin),
        );
    }

    respond_json(
        request,
        202,
        &CreateJobResponse {
            id: &id,
            status: "running",
        },
        Some(origin),
    )
}

fn handle_job_status(
    request: Request,
    state: &BridgeState,
    origin: &str,
    id: &str,
    api: u8,
) -> Result<()> {
    let guard = jobs(state);
    let Some(job) = guard.get(id) else {
        return respond_error(
            request,
            404,
            "not_found",
            "No job exists with that id.",
            Some(origin),
        );
    };
    let progress = latest_progress(&job.metrics_path);
    let response = JobStatusResponse {
        id: id.to_string(),
        status: job.state.as_str().to_string(),
        progress,
        result_url: job
            .state
            .has_result()
            .then(|| format!("/v{api}/jobs/{id}/result")),
        error: job.error.clone(),
        checkpoint_url: (api >= 2 && job.state.has_result() && job.checkpoint_path.is_some())
            .then(|| format!("/v2/jobs/{id}/checkpoint")),
    };
    drop(guard);
    respond_json(request, 200, &response, Some(origin))
}

fn handle_job_result(request: Request, state: &BridgeState, origin: &str, id: &str) -> Result<()> {
    let guard = jobs(state);
    let Some(job) = guard.get(id) else {
        return respond_error(
            request,
            404,
            "not_found",
            "No job exists with that id.",
            Some(origin),
        );
    };
    match job.state {
        JobState::Running | JobState::Cancelling => {
            drop(guard);
            respond_error(
                request,
                409,
                "not_ready",
                "The job has not finished yet.",
                Some(origin),
            )
        }
        JobState::Failed => {
            let message = job
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "The solve failed.".to_string());
            drop(guard);
            respond_error(request, 409, "solve_failed", &message, Some(origin))
        }
        JobState::Succeeded | JobState::Cancelled | JobState::ResourceLimit => {
            let path = job.result_path.clone();
            drop(guard);
            let body = std::fs::read(&path)
                .with_context(|| format!("reading managed result {}", path.display()))?;
            request
                .respond(byte_response(200, body, "application/json", Some(origin)))
                .context("writing result response")
        }
    }
}

fn handle_checkpoint(request: Request, state: &BridgeState, origin: &str, id: &str) -> Result<()> {
    let guard = jobs(state);
    let Some(job) = guard.get(id) else {
        return respond_error(
            request,
            404,
            "not_found",
            "No job exists with that id.",
            Some(origin),
        );
    };
    if job.state.is_active() {
        drop(guard);
        return respond_error(
            request,
            409,
            "not_ready",
            "The job has not finished yet.",
            Some(origin),
        );
    }
    if job.state == JobState::Failed {
        let message = job
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "The solve failed.".to_string());
        drop(guard);
        return respond_error(request, 409, "solve_failed", &message, Some(origin));
    }
    let Some(path) = job.checkpoint_path.clone() else {
        drop(guard);
        return respond_error(
            request,
            409,
            "checkpoint_unavailable",
            "This job does not have a managed multiway checkpoint.",
            Some(origin),
        );
    };
    drop(guard);

    let body = match std::fs::read(&path) {
        Ok(body) => body,
        Err(error) => {
            return respond_error(
                request,
                500,
                "artifact_read_failed",
                &format!("The managed checkpoint could not be read: {error}"),
                Some(origin),
            );
        }
    };
    request
        .respond(byte_response(
            200,
            body,
            "application/octet-stream",
            Some(origin),
        ))
        .context("writing checkpoint response")
}
fn handle_cancel_job(request: Request, state: &BridgeState, origin: &str, id: &str) -> Result<()> {
    let mut guard = jobs(state);
    let Some(job) = guard.get_mut(id) else {
        return respond_error(
            request,
            404,
            "not_found",
            "No job exists with that id.",
            Some(origin),
        );
    };

    match job.state {
        JobState::Running => {
            let Some(cancel) = job.cancel.as_ref() else {
                drop(guard);
                return respond_error(
                    request,
                    409,
                    "cancellation_unavailable",
                    "This job type cannot be cancelled.",
                    Some(origin),
                );
            };
            cancel.store(true, Ordering::Relaxed);
            job.state = JobState::Cancelling;
            drop(guard);
            respond_json(
                request,
                202,
                &CancelJobResponse {
                    id,
                    status: "cancelling",
                },
                Some(origin),
            )
        }
        JobState::Cancelling => {
            drop(guard);
            respond_json(
                request,
                202,
                &CancelJobResponse {
                    id,
                    status: "cancelling",
                },
                Some(origin),
            )
        }
        JobState::Cancelled => {
            drop(guard);
            respond_json(
                request,
                200,
                &CancelJobResponse {
                    id,
                    status: "cancelled",
                },
                Some(origin),
            )
        }
        JobState::Succeeded | JobState::ResourceLimit | JobState::Failed => {
            drop(guard);
            respond_error(
                request,
                409,
                "not_running",
                "The job is no longer running.",
                Some(origin),
            )
        }
    }
}

fn handle_strategies(
    request: Request,
    state: &BridgeState,
    origin: &str,
    id: &str,
    query: &str,
) -> Result<()> {
    let (cursor, limit) = match parse_strategy_query(query) {
        Ok(page) => page,
        Err(message) => {
            return respond_error(request, 400, "invalid_query", &message, Some(origin));
        }
    };

    let guard = jobs(state);
    let Some(job) = guard.get(id) else {
        return respond_error(
            request,
            404,
            "not_found",
            "No job exists with that id.",
            Some(origin),
        );
    };
    if job.state.is_active() {
        drop(guard);
        return respond_error(
            request,
            409,
            "not_ready",
            "The job has not finished yet.",
            Some(origin),
        );
    }
    if job.state == JobState::Failed {
        let message = job
            .error
            .as_ref()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "The solve failed.".to_string());
        drop(guard);
        return respond_error(request, 409, "solve_failed", &message, Some(origin));
    }
    let Some(path) = job.mwsol_path.clone() else {
        drop(guard);
        return respond_error(
            request,
            409,
            "strategies_unavailable",
            "This job does not have a paginated multiway strategy artifact.",
            Some(origin),
        );
    };
    drop(guard);

    let solution = match formats::read_mwsol(&path) {
        Ok(solution) => solution,
        Err(error) => {
            return respond_error(
                request,
                500,
                "artifact_read_failed",
                &format!("The managed strategy artifact could not be read: {error}"),
                Some(origin),
            );
        }
    };
    if cursor > solution.strategies.len() {
        return respond_error(
            request,
            400,
            "invalid_cursor",
            "cursor is past the end of the strategy index.",
            Some(origin),
        );
    }
    let end = cursor.saturating_add(limit).min(solution.strategies.len());
    let next_cursor = (end < solution.strategies.len()).then(|| end.to_string());
    let items = solution.strategies[cursor..end]
        .iter()
        .map(|block| StrategyPageItem {
            key: &block.key,
            public_history: solution
                .resolve_history(block.key.history)
                .expect("validated artifact contains every strategy history"),
            actions: &block.actions,
            probabilities: &block.probabilities,
        })
        .collect();
    respond_json(
        request,
        200,
        &StrategyPageResponse { items, next_cursor },
        Some(origin),
    )
}

fn parse_strategy_query(query: &str) -> std::result::Result<(usize, usize), String> {
    let mut cursor = None;
    let mut limit = None;
    if !query.is_empty() {
        for parameter in query.split('&') {
            let (name, value) = parameter
                .split_once('=')
                .ok_or_else(|| "Strategy query parameters must use name=value.".to_string())?;
            match name {
                "cursor" if cursor.is_none() => {
                    cursor =
                        Some(value.parse::<usize>().map_err(|_| {
                            "cursor must be a non-negative decimal index.".to_string()
                        })?);
                }
                "limit" if limit.is_none() => {
                    let parsed = value.parse::<usize>().map_err(|_| {
                        format!("limit must be from 1 through {MAX_STRATEGY_PAGE_SIZE}.")
                    })?;
                    if !(1..=MAX_STRATEGY_PAGE_SIZE).contains(&parsed) {
                        return Err(format!(
                            "limit must be from 1 through {MAX_STRATEGY_PAGE_SIZE}."
                        ));
                    }
                    limit = Some(parsed);
                }
                "cursor" | "limit" => {
                    return Err(format!(
                        "Strategy query parameter {name} may appear only once."
                    ));
                }
                _ => return Err(format!("Unsupported strategy query parameter {name}.")),
            }
        }
    }
    Ok((
        cursor.unwrap_or(0),
        limit.unwrap_or(DEFAULT_STRATEGY_PAGE_SIZE),
    ))
}

fn multiway_result_state(path: &Path) -> Option<JobState> {
    std::fs::read(path)
        .ok()
        .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok())
        .and_then(
            |result| match result.get("status").and_then(|status| status.as_str()) {
                Some("completed") => Some(JobState::Succeeded),
                Some("cancelled") => Some(JobState::Cancelled),
                Some("resource_limit") => Some(JobState::ResourceLimit),
                _ => None,
            },
        )
}
fn latest_progress(path: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().rev().find_map(|line| {
        if let Ok(row) = serde_json::from_str::<formats::MultiwayMetricsRow>(line) {
            return serde_json::to_value(row).ok();
        }
        serde_json::from_str::<formats::MetricsRow>(line)
            .ok()
            .and_then(|row| serde_json::to_value(ProgressResponse::from(row)).ok())
    })
}

fn jobs(state: &BridgeState) -> MutexGuard<'_, HashMap<String, JobRecord>> {
    state
        .jobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn sanitize_config(
    raw: &str,
    managed_cache: &Path,
    threads: Option<usize>,
    allow_multiway: bool,
) -> std::result::Result<SanitizedConfig, String> {
    let config: SolveConfig =
        toml::from_str(raw).map_err(|error| format!("TOML config is invalid: {error}"))?;

    if let GameSection::PreflopMultiway(game) = &config.game {
        if !allow_multiway {
            return Err("The bridge MVP supports kind = \"preflop\" only.".to_string());
        }
        return sanitize_multiway_config(raw, &config, game, managed_cache, threads);
    }

    let (
        effective_stack_bb,
        sb_bb,
        open_sizes_bb,
        raise_factors,
        max_raises,
        include_allin,
        allow_limp,
        sb_range,
        bb_range,
        equity_realization,
    ) = match &config.game {
        GameSection::Preflop {
            effective_stack_bb,
            sb_bb,
            open_sizes_bb,
            raise_factors,
            max_raises,
            include_allin,
            allow_limp,
            sb_range,
            bb_range,
            equity_realization,
            postflop: None,
            ..
        } => (
            *effective_stack_bb,
            *sb_bb,
            open_sizes_bb,
            raise_factors,
            *max_raises,
            *include_allin,
            *allow_limp,
            sb_range,
            bb_range,
            *equity_realization,
        ),
        GameSection::Preflop {
            postflop: Some(_), ..
        } => {
            return Err(
                "The bridge MVP supports EquityShowdown only; remove [game.postflop].".to_string(),
            );
        }
        _ => return Err("The bridge MVP supports kind = \"preflop\" only.".to_string()),
    };

    validate_finite("game.effective_stack_bb", effective_stack_bb)?;
    if !(effective_stack_bb > 1.0 && effective_stack_bb <= MAX_STACK_BB)
        || (effective_stack_bb * preflop::CHIPS_PER_BB as f64).round()
            <= preflop::CHIPS_PER_BB as f64
    {
        return Err(format!(
            "game.effective_stack_bb must be greater than 1 and at most {MAX_STACK_BB}."
        ));
    }

    validate_finite("game.sb_bb", sb_bb)?;
    let rounded_sb = (sb_bb * preflop::CHIPS_PER_BB as f64).round();
    if !(sb_bb > 0.0 && sb_bb < 1.0 && (1.0..preflop::CHIPS_PER_BB as f64).contains(&rounded_sb)) {
        return Err("game.sb_bb must round to 0.1bb through 0.9bb.".to_string());
    }

    if open_sizes_bb.len() > MAX_SIZES_PER_LEVEL {
        return Err(format!(
            "game.open_sizes_bb may contain at most {MAX_SIZES_PER_LEVEL} sizes."
        ));
    }
    for value in open_sizes_bb {
        validate_finite("game.open_sizes_bb", *value)?;
        if *value < 2.0 || *value > effective_stack_bb {
            return Err(
                "Every game.open_sizes_bb value must be between 2bb and the effective stack."
                    .to_string(),
            );
        }
    }

    if raise_factors.len() > MAX_FACTOR_LEVELS {
        return Err(format!(
            "game.raise_factors may contain at most {MAX_FACTOR_LEVELS} levels."
        ));
    }
    for level in raise_factors {
        if level.len() > MAX_SIZES_PER_LEVEL {
            return Err(format!(
                "Each game.raise_factors level may contain at most {MAX_SIZES_PER_LEVEL} values."
            ));
        }
        for value in level {
            validate_finite("game.raise_factors", *value)?;
            if *value <= 1.0 {
                return Err("Every game.raise_factors value must be greater than 1.".to_string());
            }
        }
    }

    if max_raises > MAX_RAISES {
        return Err(format!("game.max_raises may not exceed {MAX_RAISES}."));
    }
    let root_has_raise = max_raises > 0 && (include_allin || !open_sizes_bb.is_empty());
    if !allow_limp && !root_has_raise {
        return Err("The SB needs a limp or raise in addition to folding at the root.".to_string());
    }
    if grammar_paths(
        open_sizes_bb,
        raise_factors,
        max_raises,
        include_allin,
        allow_limp,
    ) > MAX_GRAMMAR_PATHS
    {
        return Err(format!(
            "The betting grammar exceeds the {MAX_GRAMMAR_PATHS} action-path limit."
        ));
    }

    for (label, range) in [("game.sb_range", sb_range), ("game.bb_range", bb_range)] {
        if range
            .as_ref()
            .is_some_and(|value| value.len() > MAX_RANGE_BYTES)
        {
            return Err(format!("{label} exceeds the {MAX_RANGE_BYTES}-byte limit."));
        }
    }
    for (index, value) in equity_realization.iter().enumerate() {
        validate_finite(&format!("game.equity_realization[{index}]"), *value)?;
        if *value <= 0.0 {
            return Err("game.equity_realization values must be greater than zero.".to_string());
        }
    }

    if matches!(&config.utility, UtilitySection::TournamentIcm { .. }) {
        return Err(
            "Heads-up bridge jobs require utility kind = \"chip-ev\" or \"icm\".".to_string(),
        );
    }
    if matches!(
        &config.algorithm,
        AlgorithmSection::ExternalSamplingMccfr { .. }
    ) {
        return Err(
            "Heads-up bridge jobs do not support schedule = \"external-sampling-mccfr\"."
                .to_string(),
        );
    }
    if config.run.sweeps.is_some()
        || config.run.seed.is_some()
        || config.run.max_memory_bytes.is_some()
        || config.run.checkpoint_every.is_some()
        || config.run.evaluation_samples.is_some()
        || config.run.evaluation_cadence.is_some()
    {
        return Err("Multiway-only [run] fields are not valid for heads-up jobs.".to_string());
    }

    validate_common_sections(&config)?;

    if config.run.iterations == 0 || config.run.iterations > MAX_ITERATIONS {
        return Err(format!(
            "run.iterations must be from 1 through {MAX_ITERATIONS}."
        ));
    }
    if config.run.check_every == 0 || config.run.check_every > config.run.iterations {
        return Err("run.check_every must be from 1 through run.iterations.".to_string());
    }
    if let Some(target) = config.run.target_nash_conv {
        validate_finite("run.target_nash_conv", target)?;
        if target < 0.0 {
            return Err("run.target_nash_conv may not be negative.".to_string());
        }
    }

    let trunk = crate::preflop_setup::build_preflop_config(
        effective_stack_bb,
        sb_bb,
        open_sizes_bb.clone(),
        raise_factors.clone(),
        max_raises,
        include_allin,
        allow_limp,
        sb_range.as_deref(),
        bb_range.as_deref(),
    )
    .map_err(|error| format!("Preflop config is invalid: {error:#}"))?;
    for (label, range) in [
        ("game.sb_range", &trunk.ranges[Player::P0]),
        ("game.bb_range", &trunk.ranges[Player::P1]),
    ] {
        if range.total_weight() <= 0.0 {
            return Err(format!("{label} must contain positive hand weight."));
        }
    }

    let estimate = preflop::memory_usage(&trunk);
    let storage_bytes = match config.run.storage {
        StorageKind::F32 => estimate.f32_bytes,
        StorageKind::I16 => estimate.i16_bytes,
    };
    if storage_bytes > MAX_STORAGE_BYTES {
        return Err(format!(
            "Solver storage estimate {storage_bytes} bytes exceeds the {MAX_STORAGE_BYTES}-byte bridge limit."
        ));
    }

    let mut value: toml::Value =
        toml::from_str(raw).map_err(|error| format!("TOML config is invalid: {error}"))?;
    let table = value
        .as_table_mut()
        .ok_or_else(|| "TOML root must be a table.".to_string())?;
    let game = table
        .get_mut("game")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| "TOML [game] table is missing.".to_string())?;
    let cache = managed_cache
        .to_str()
        .ok_or_else(|| "Managed cache path is not valid UTF-8.".to_string())?;
    game.insert(
        "equity_cache".to_string(),
        toml::Value::String(cache.to_string()),
    );

    let run = table
        .get_mut("run")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| "TOML [run] table is missing.".to_string())?;
    run.remove("threads");
    run.remove("par_chance_depth");
    run.remove("par_min_children");
    if let Some(count) = threads {
        run.insert("threads".to_string(), toml::Value::Integer(count as i64));
    }

    let toml = toml::to_string(&value)
        .map_err(|error| format!("Serializing safe config failed: {error}"))?;
    Ok(SanitizedConfig {
        toml,
        schema_version: 1,
        kind: JobKind::HeadsUp,
    })
}

fn sanitize_multiway_config(
    raw: &str,
    config: &SolveConfig,
    game: &multiway::MultiwayConfig,
    managed_cache: &Path,
    threads: Option<usize>,
) -> std::result::Result<SanitizedConfig, String> {
    let utility = match &config.utility {
        UtilitySection::ChipEv => multiway::config::UtilityConfig::ChipEv,
        UtilitySection::TournamentIcm {
            outside_field,
            payouts,
            samples,
            seed,
        } => {
            if *samples > MAX_MULTIWAY_ICM_SAMPLES {
                return Err(format!(
                    "utility.samples may not exceed {MAX_MULTIWAY_ICM_SAMPLES}."
                ));
            }
            for (index, player) in outside_field.iter().enumerate() {
                validate_finite(
                    &format!("utility.outside_field[{index}].stack_bb"),
                    player.stack_bb,
                )?;
                if player.stack_bb > MAX_STACK_BB {
                    return Err(format!(
                        "utility.outside_field[{index}].stack_bb may not exceed {MAX_STACK_BB}."
                    ));
                }
            }
            multiway::config::UtilityConfig::TournamentIcm {
                outside_field: outside_field
                    .iter()
                    .map(|player| multiway::config::FieldPlayerConfig {
                        name: player.name.clone(),
                        stack_bb: player.stack_bb,
                    })
                    .collect(),
                payouts: payouts.clone(),
                samples: *samples,
                seed: *seed,
            }
        }
        UtilitySection::Icm { .. } => {
            return Err(
                "preflop-multiway requires utility kind = \"chip-ev\" or \"tournament-icm\"."
                    .to_string(),
            );
        }
    };
    let rake = multiway_rake_config(&config.rake);
    game.validate_economics(&utility, &rake)
        .map_err(|error| format!("Multiway config is invalid: {error}"))?;

    for (index, seat) in game.seats.iter().enumerate() {
        if seat.stack_bb > MAX_STACK_BB {
            return Err(format!(
                "game.seats[{index}].stack_bb may not exceed {MAX_STACK_BB}."
            ));
        }
        if seat.range.len() > MAX_RANGE_BYTES {
            return Err(format!(
                "game.seats[{index}].range exceeds the {MAX_RANGE_BYTES}-byte limit."
            ));
        }
    }
    for (label, value) in [
        ("game.blinds.small_bb", game.blinds.small_bb),
        ("game.blinds.big_bb", game.blinds.big_bb),
    ] {
        if value > MAX_STACK_BB {
            return Err(format!("{label} may not exceed {MAX_STACK_BB}."));
        }
    }
    let ante = match game.ante {
        multiway::config::AnteConfig::None => 0.0,
        multiway::config::AnteConfig::Each { amount_bb }
        | multiway::config::AnteConfig::BigBlind { amount_bb } => amount_bb,
    };
    if ante > MAX_STACK_BB {
        return Err(format!(
            "game.ante.amount_bb may not exceed {MAX_STACK_BB}."
        ));
    }

    for (street, betting) in [
        ("preflop", &game.betting.preflop),
        ("flop", &game.betting.flop),
        ("turn", &game.betting.turn),
        ("river", &game.betting.river),
    ] {
        if betting.bet_sizes.len() > MAX_SIZES_PER_LEVEL
            || betting.raise_sizes.len() > MAX_SIZES_PER_LEVEL
        {
            return Err(format!(
                "game.betting.{street} may contain at most {MAX_SIZES_PER_LEVEL} bet and raise sizes."
            ));
        }
        if u32::from(betting.max_aggressive_actions) > MAX_RAISES {
            return Err(format!(
                "game.betting.{street}.max_aggressive_actions may not exceed {MAX_RAISES}."
            ));
        }
    }
    if game.abstraction.flop_buckets > MAX_MULTIWAY_BUCKETS
        || game.abstraction.turn_buckets > MAX_MULTIWAY_BUCKETS
        || game.abstraction.river_buckets > MAX_MULTIWAY_BUCKETS
    {
        return Err(format!(
            "Multiway abstraction bucket counts may not exceed {MAX_MULTIWAY_BUCKETS}."
        ));
    }
    if game.abstraction.rollout_samples > MAX_MULTIWAY_ROLLOUT_SAMPLES {
        return Err(format!(
            "game.abstraction.rollout_samples may not exceed {MAX_MULTIWAY_ROLLOUT_SAMPLES}."
        ));
    }

    validate_common_sections(config)?;
    if !matches!(
        config.algorithm,
        AlgorithmSection::ExternalSamplingMccfr { .. }
    ) {
        return Err(
            "preflop-multiway requires schedule = \"external-sampling-mccfr\".".to_string(),
        );
    }
    if config.run.storage != StorageKind::F32 {
        return Err("preflop-multiway requires run.storage = \"f32\".".to_string());
    }
    if config.run.target_nash_conv.is_some() {
        return Err(
            "preflop-multiway does not expose NashConv; remove run.target_nash_conv.".to_string(),
        );
    }
    let sweeps = config.run.sweeps.unwrap_or(config.run.iterations);
    if sweeps == 0 || sweeps > MAX_ITERATIONS {
        return Err(format!(
            "run.sweeps (or run.iterations) must be from 1 through {MAX_ITERATIONS}."
        ));
    }
    let evaluation_cadence = config
        .run
        .evaluation_cadence
        .unwrap_or(config.run.check_every);
    if evaluation_cadence == 0 || evaluation_cadence > MAX_ITERATIONS {
        return Err(format!(
            "run.evaluation_cadence (or run.check_every) must be from 1 through {MAX_ITERATIONS}."
        ));
    }
    if config
        .run
        .checkpoint_every
        .is_some_and(|cadence| cadence == 0 || cadence > MAX_ITERATIONS)
    {
        return Err(format!(
            "run.checkpoint_every must be from 1 through {MAX_ITERATIONS} when supplied."
        ));
    }
    if config
        .run
        .evaluation_samples
        .is_some_and(|samples| samples == 0 || samples > MAX_MULTIWAY_EVALUATION_SAMPLES)
    {
        return Err(format!(
            "run.evaluation_samples must be from 1 through {MAX_MULTIWAY_EVALUATION_SAMPLES} when supplied."
        ));
    }
    let memory_limit = config.run.max_memory_bytes.unwrap_or(MAX_STORAGE_BYTES);
    if memory_limit == 0 || memory_limit > MAX_STORAGE_BYTES {
        return Err(format!(
            "run.max_memory_bytes must be from 1 through {MAX_STORAGE_BYTES}."
        ));
    }

    let mut value: toml::Value =
        toml::from_str(raw).map_err(|error| format!("TOML config is invalid: {error}"))?;
    let table = value
        .as_table_mut()
        .ok_or_else(|| "TOML root must be a table.".to_string())?;
    let cache_fingerprint = formats::config_hash(raw.as_bytes());
    let cache_key = formats::config_hash_hex(&cache_fingerprint);
    let cache_directory = managed_cache.parent().unwrap_or_else(|| Path::new("."));
    let cache_path = cache_directory.join(format!("multiway-rollout-{cache_key}.mwab"));
    let game_table = table
        .get_mut("game")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| "TOML [game] table is missing.".to_string())?;
    let abstraction = game_table
        .entry("abstraction")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| "TOML [game.abstraction] must be a table.".to_string())?;
    // Browser jobs may not choose arbitrary local write paths. Reuse a
    // deterministic cache in the bridge-owned directory instead.
    abstraction.insert(
        "artifact_cache".to_string(),
        toml::Value::String(cache_path.to_string_lossy().into_owned()),
    );
    let run = table
        .get_mut("run")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| "TOML [run] table is missing.".to_string())?;
    run.remove("threads");
    run.remove("par_chance_depth");
    run.remove("par_min_children");
    run.insert(
        "max_memory_bytes".to_string(),
        toml::Value::Integer(memory_limit as i64),
    );
    if let Some(count) = threads {
        run.insert("threads".to_string(), toml::Value::Integer(count as i64));
    }

    let toml = toml::to_string(&value)
        .map_err(|error| format!("Serializing safe config failed: {error}"))?;
    Ok(SanitizedConfig {
        toml,
        schema_version: formats::MULTIWAY_SCHEMA_VERSION,
        kind: JobKind::Multiway,
    })
}

fn multiway_rake_config(rake: &RakeSection) -> multiway::config::RakeConfig {
    let chips_per_bb = multiway::types::CHIPS_PER_BB as f64;
    match rake {
        RakeSection::None => multiway::config::RakeConfig::None,
        RakeSection::PercentCap {
            rate,
            cap,
            no_flop_no_drop,
        } => multiway::config::RakeConfig::PercentCap {
            rate: *rate,
            cap_bb: *cap / chips_per_bb,
            no_flop_no_drop: *no_flop_no_drop,
        },
        RakeSection::GgPreflop {
            rate,
            cap,
            exempt_pot,
        } => multiway::config::RakeConfig::GgPreflop {
            rate: *rate,
            cap_bb: *cap / chips_per_bb,
            exempt_pot_bb: f64::from(*exempt_pot) / chips_per_bb,
        },
    }
}
fn validate_common_sections(config: &SolveConfig) -> std::result::Result<(), String> {
    match &config.rake {
        RakeSection::None => {}
        RakeSection::PercentCap { rate, cap, .. } | RakeSection::GgPreflop { rate, cap, .. } => {
            validate_finite("rake.rate", *rate)?;
            validate_finite("rake.cap", *cap)?;
            if !(0.0..=1.0).contains(rate) {
                return Err("rake.rate must be between 0 and 1.".to_string());
            }
            if *cap < 0.0 {
                return Err("rake.cap may not be negative.".to_string());
            }
        }
    }

    if let UtilitySection::Icm { payouts } = &config.utility {
        for value in payouts {
            validate_finite("utility.payouts", *value)?;
            if *value < 0.0 {
                return Err("utility.payouts may not be negative.".to_string());
            }
        }
        if payouts[0] < payouts[1] {
            return Err("utility.payouts must be ordered highest to lowest.".to_string());
        }
    }

    match &config.algorithm {
        AlgorithmSection::Vanilla | AlgorithmSection::CfrPlus | AlgorithmSection::LinearCfr => {}
        AlgorithmSection::Dcfr {
            alpha, beta, gamma, ..
        } => {
            validate_finite("algorithm.alpha", *alpha)?;
            validate_finite("algorithm.beta", *beta)?;
            validate_finite("algorithm.gamma", *gamma)?;
        }
        AlgorithmSection::HsDcfr { gamma0 } => {
            validate_finite("algorithm.gamma0", *gamma0)?;
            if *gamma0 <= 0.0 {
                return Err("algorithm.gamma0 must be greater than zero.".to_string());
            }
        }
        AlgorithmSection::ExternalSamplingMccfr {
            exploration_epsilon,
            discount_every,
            discount_until,
            ..
        } => {
            validate_finite("algorithm.exploration_epsilon", *exploration_epsilon)?;
            if !(0.0..=1.0).contains(exploration_epsilon) {
                return Err("algorithm.exploration_epsilon must be between 0 and 1.".to_string());
            }
            if *discount_every == 0 {
                return Err("algorithm.discount_every must be positive.".to_string());
            }
            if *discount_until < *discount_every {
                return Err("algorithm.discount_until must not precede discount_every.".to_string());
            }
        }
    }
    Ok(())
}

fn validate_finite(label: &str, value: f64) -> std::result::Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{label} must be finite."))
    }
}

fn grammar_paths(
    open_sizes: &[f64],
    raise_factors: &[Vec<f64>],
    max_raises: u32,
    include_allin: bool,
    allow_limp: bool,
) -> u64 {
    if max_raises == 0 {
        return u64::from(allow_limp);
    }
    let allin = usize::from(include_allin);
    let mut frontier = (open_sizes.len() + allin) as u64;
    let mut total = frontier;
    for level in 1..max_raises as usize {
        let sized = if raise_factors.is_empty() {
            0
        } else {
            raise_factors[(level - 1).min(raise_factors.len() - 1)].len()
        };
        frontier = frontier.saturating_mul((sized + allin) as u64);
        total = total.saturating_add(frontier);
        if total > MAX_GRAMMAR_PATHS {
            return total;
        }
    }
    if allow_limp {
        total = total.saturating_mul(2);
    }
    total
}

fn validate_origin(origin: &str) -> Result<()> {
    let host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .ok_or_else(|| anyhow!("--origin must start with http:// or https://"))?;
    if host.is_empty()
        || host
            .chars()
            .any(|character| character.is_whitespace() || "/?#@".contains(character))
    {
        return Err(anyhow!(
            "--origin must be an exact origin without path, query, fragment, or user info"
        ));
    }
    Ok(())
}

fn single_header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    let mut matching = request
        .headers()
        .iter()
        .filter(|header| header.field.equiv(name));
    let first = matching.next()?.value.as_str();
    matching.next().is_none().then_some(first)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (&a, &b)| difference | (a ^ b))
        == 0
}

fn random_hex(bytes: usize) -> String {
    let mut random = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut random);
    let mut output = String::with_capacity(bytes * 2);
    for byte in random {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn respond_json<T: Serialize>(
    request: Request,
    status: u16,
    value: &T,
    cors_origin: Option<&str>,
) -> Result<()> {
    let body = serde_json::to_vec(value).context("serializing JSON response")?;
    request
        .respond(byte_response(status, body, "application/json", cors_origin))
        .context("writing JSON response")
}

fn respond_error(
    request: Request,
    status: u16,
    code: &str,
    message: &str,
    cors_origin: Option<&str>,
) -> Result<()> {
    respond_json(
        request,
        status,
        &ErrorEnvelope {
            error: ErrorBody {
                code: code.to_string(),
                message: message.to_string(),
            },
        },
        cors_origin,
    )
}

fn byte_response(
    status: u16,
    body: Vec<u8>,
    content_type: &str,
    cors_origin: Option<&str>,
) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(body).with_status_code(StatusCode(status));
    response.add_header(header("Content-Type", content_type));
    response.add_header(header("Cache-Control", "no-store"));
    if let Some(origin) = cors_origin {
        response.add_header(header("Access-Control-Allow-Origin", origin));
        response.add_header(header("Vary", "Origin"));
    }
    response
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("constant HTTP header names and validated values are legal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_limit_saturates_before_expensive_tree_enumeration() {
        let factors = vec![vec![2.0; 16]; 16];
        assert!(grammar_paths(&[2.0; 16], &factors, 16, true, true) > MAX_GRAMMAR_PATHS);
    }

    #[test]
    fn origins_are_exact_and_pathless() {
        assert!(validate_origin("http://localhost:3000").is_ok());
        assert!(validate_origin("https://solver.example").is_ok());
        assert!(validate_origin("http://localhost:3000/").is_err());
        assert!(validate_origin("file://local").is_err());
    }

    #[test]
    fn v2_routes_accept_only_the_strategy_query_string() {
        let id = "ab".repeat(JOB_ID_BYTES);
        assert!(matches!(parse_route("/v1/health?probe=1"), Route::Unknown));
        assert!(matches!(
            parse_route(&format!("/v2/jobs/{id}/cancel")),
            Route::CancelV2(found) if found == id
        ));
        assert!(matches!(
            parse_route(&format!("/v2/jobs/{id}/checkpoint")),
            Route::CheckpointV2(found) if found == id
        ));
        let Route::StrategiesV2(found, query) =
            parse_route(&format!("/v2/jobs/{id}/strategies?cursor=50&limit=10"))
        else {
            panic!("expected the v2 strategies route");
        };
        assert_eq!(found, id);
        assert_eq!(query, "cursor=50&limit=10");
    }

    #[test]
    fn strategy_pagination_is_bounded_and_rejects_ambiguous_queries() {
        assert_eq!(
            parse_strategy_query("").unwrap(),
            (0, DEFAULT_STRATEGY_PAGE_SIZE)
        );
        assert_eq!(
            parse_strategy_query("limit=100&cursor=7").unwrap(),
            (7, 100)
        );
        assert!(parse_strategy_query("limit=0").is_err());
        assert!(parse_strategy_query("limit=101").is_err());
        assert!(parse_strategy_query("cursor=1&cursor=2").is_err());
        assert!(parse_strategy_query("actor=3").is_err());
    }

    #[test]
    fn v1_preflop_sanitization_keeps_the_managed_cache_contract() {
        let raw = r#"
[game]
kind = "preflop"
effective_stack_bb = 10.0

[run]
iterations = 1
check_every = 1
"#;
        let sanitized =
            sanitize_config(raw, Path::new("managed-equity.bin"), Some(2), false).unwrap();
        assert!(matches!(sanitized.kind, JobKind::HeadsUp));
        assert_eq!(sanitized.schema_version, 1);
        let parsed: SolveConfig = toml::from_str(&sanitized.toml).unwrap();
        assert_eq!(parsed.run.threads, Some(2));
        let GameSection::Preflop { equity_cache, .. } = parsed.game else {
            panic!("expected heads-up preflop config");
        };
        assert_eq!(equity_cache, Some(PathBuf::from("managed-equity.bin")));

        let with_sweeps = raw.replace("check_every = 1", "check_every = 1\nsweeps = 1");
        assert!(sanitize_config(&with_sweeps, Path::new("unused"), None, true).is_err());
        let with_mccfr = format!("{raw}\n[algorithm]\nschedule = \"external-sampling-mccfr\"\n");
        assert!(sanitize_config(&with_mccfr, Path::new("unused"), None, true).is_err());
    }

    #[test]
    fn multiway_sanitization_is_v2_only_and_caps_managed_memory() {
        let mut raw = include_str!("../../../examples/preflop_multiway_9max.toml").to_string();
        raw.push_str("threads = 999\npar_chance_depth = 99\npar_min_children = 99\n");
        assert!(sanitize_config(&raw, Path::new("unused"), None, false).is_err());

        let sanitized = sanitize_config(&raw, Path::new("unused"), Some(3), true).unwrap();
        assert!(matches!(sanitized.kind, JobKind::Multiway));
        assert_eq!(sanitized.schema_version, formats::MULTIWAY_SCHEMA_VERSION);
        let parsed: SolveConfig = toml::from_str(&sanitized.toml).unwrap();
        assert_eq!(parsed.run.threads, Some(3));
        assert_eq!(parsed.run.par_chance_depth, None);
        assert_eq!(parsed.run.par_min_children, None);
        assert_eq!(parsed.run.max_memory_bytes, Some(MAX_STORAGE_BYTES));
        let GameSection::PreflopMultiway(game) = parsed.game else {
            panic!("expected multiway config");
        };
        let artifact_cache = game.abstraction.artifact_cache.unwrap();
        assert!(
            artifact_cache
                .to_string_lossy()
                .contains("multiway-rollout-")
        );
        assert_eq!(
            artifact_cache.extension().and_then(|value| value.to_str()),
            Some("mwab")
        );

        let oversized = raw.replace(
            "max_memory_bytes = 2147483648",
            "max_memory_bytes = 2147483649",
        );
        assert!(sanitize_config(&oversized, Path::new("unused"), None, true).is_err());
    }

    #[test]
    fn multiway_rake_validation_converts_legacy_chips_to_bb() {
        let percent = multiway_rake_config(&RakeSection::PercentCap {
            rate: 0.05,
            cap: 3_000.0,
            no_flop_no_drop: true,
        });
        let multiway::config::RakeConfig::PercentCap {
            cap_bb,
            no_flop_no_drop,
            ..
        } = percent
        else {
            panic!("expected percent-cap rake");
        };
        assert_eq!(cap_bb, 3.0);
        assert!(no_flop_no_drop);

        let gg = multiway_rake_config(&RakeSection::GgPreflop {
            rate: 0.05,
            cap: 1_500.0,
            exempt_pot: 2_500,
        });
        let multiway::config::RakeConfig::GgPreflop {
            cap_bb,
            exempt_pot_bb,
            ..
        } = gg
        else {
            panic!("expected GG preflop rake");
        };
        assert_eq!(cap_bb, 1.5);
        assert_eq!(exempt_pot_bb, 2.5);
    }
    #[test]
    fn terminal_state_is_read_from_the_managed_multiway_result() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("result.json");
        std::fs::write(&path, br#"{"status":"completed"}"#).unwrap();
        assert_eq!(multiway_result_state(&path), Some(JobState::Succeeded));
        std::fs::write(&path, br#"{"status":"cancelled"}"#).unwrap();
        assert_eq!(multiway_result_state(&path), Some(JobState::Cancelled));
        std::fs::write(&path, br#"{"status":"resource_limit"}"#).unwrap();
        assert_eq!(multiway_result_state(&path), Some(JobState::ResourceLimit));
    }
}
