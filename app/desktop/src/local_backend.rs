use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use cli::config::{GameSection, SolveConfig, UtilitySection};
use formats::{
    MultiwayMetricsRow, MultiwayPublicAction, MultiwayStrategyBlock, MultiwayStrategyKey,
};
use multiway::HistoryKey;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_RESULT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PROGRESS_EVENTS: usize = 500;
const ROOT_NODE_ID: &str = "00000000000000000000000000000000";
// The Solve screen normally renews this every two seconds. Keep enough slack
// for a briefly busy or background-throttled webview without leaving live
// strategy scans enabled indefinitely after the screen is closed.
const LIVE_STRATEGY_LEASE_MILLIS: u64 = 30_000;

pub type EventSink = Arc<dyn Fn(LocalJobEvent) + Send + Sync + 'static>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl CommandError {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }

    fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: true,
        }
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(error: anyhow::Error) -> Self {
        Self::new("local_backend_error", format!("{error:#}"))
    }
}

pub(crate) type CommandResult<T> = std::result::Result<T, CommandError>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCapabilities {
    pub service: &'static str,
    pub server_id: &'static str,
    pub solver_version: &'static str,
    pub api_version: u8,
    pub busy: bool,
    pub config_schemas: [&'static str; 1],
    pub artifact_schemas: ArtifactSchemaCapabilities,
    pub features: LocalFeatureCapabilities,
    pub limits: LocalLimits,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSchemaCapabilities {
    pub run_json: [u16; 1],
    pub progress_jsonl: [u16; 1],
    pub mwsol: [u16; 1],
    pub mwckpt: [u16; 1],
    pub strategy_snapshot: [u16; 1],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFeatureCapabilities {
    pub live_strategy_snapshots: bool,
    pub resume: bool,
    pub durable_jobs: bool,
    pub native_file_dialogs: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLimits {
    pub max_players: u8,
    pub max_concurrent_jobs: u8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedConfig {
    pub source_id: String,
    pub file_name: String,
    pub config_toml: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedTreeSource {
    pub source_id: String,
    pub file_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateConfigRequest {
    pub config_toml: String,
    #[serde(default)]
    pub source_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationMessage {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationPreflight {
    pub threads: String,
    pub abstraction: String,
    pub economics: ValidationEconomicsPreflight,
    pub tree: ValidationTreePreflight,
    pub memory: ValidationMemoryPreflight,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ValidationEconomicsPreflight {
    Cash,
    TournamentIcm {
        field_players: String,
        paid_places: String,
        mode: &'static str,
        samples: Option<String>,
        seed: Option<String>,
        prepared_bytes: Option<String>,
        prepared_limit_bytes: Option<String>,
        fits_prepared_limit: Option<bool>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationTreePreflight {
    pub recall_mode: &'static str,
    pub decision_nodes: String,
    pub terminal_edges: Option<String>,
    pub policy_columns: Option<String>,
    pub policy_slots: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationMemoryPreflight {
    pub estimate_kind: &'static str,
    pub solver_state_bytes: Option<String>,
    pub budget_mode: &'static str,
    pub available_bytes: Option<String>,
    pub budget_bytes: String,
    pub headroom_bytes: Option<String>,
    pub fits_budget: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitsDto {
    pub utility: String,
    pub chip_unit_bb: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationMessage>,
    pub warnings: Vec<ValidationMessage>,
    pub effective_config_toml: Option<String>,
    pub config_fingerprint: Option<String>,
    pub preflight: Option<ValidationPreflight>,
    pub guarantee_boundary: String,
    pub units: UnitsDto,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartJobRequest {
    pub name: String,
    pub effective_config_toml: String,
    pub config_fingerprint: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeOverrides {
    #[serde(default)]
    pub max_sweeps: Option<String>,
    #[serde(default)]
    pub max_time: Option<String>,
    #[serde(default)]
    pub stop_target: Option<String>,
    #[serde(default)]
    pub evaluation_samples: Option<String>,
    #[serde(default)]
    pub check_every_sweeps: Option<String>,
    #[serde(default)]
    pub checkpoint_interval: Option<String>,
    #[serde(default)]
    pub threads: Option<String>,
    #[serde(default)]
    pub memory: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeJobRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overrides: ResumeOverrides,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Queued,
    #[allow(dead_code)]
    Validating,
    Running,
    Cancelling,
    TargetReached,
    SweepLimit,
    TimeLimit,
    Cancelled,
    ResourceLimit,
    Failed,
}

impl JobState {
    fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Validating | Self::Running | Self::Cancelling
        )
    }

    fn is_terminal(self) -> bool {
        !self.is_active()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobErrorDto {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDescriptor {
    pub available: bool,
    pub byte_length: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobArtifacts {
    pub run: ArtifactDescriptor,
    pub progress: ArtifactDescriptor,
    pub solution: ArtifactDescriptor,
    pub checkpoint: ArtifactDescriptor,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateDto {
    pub mean: String,
    pub stderr: String,
    pub ci95: [String; 2],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatMetricsDto {
    pub seat: u8,
    pub profile_ev: Option<EstimateDto>,
    pub average_positive_regret: Option<String>,
    pub strategy_drift_l1: Option<String>,
    pub deviation_gain: Option<EstimateDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointProgressDto {
    pub available: bool,
    pub generated_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub sweeps: String,
    pub max_sweeps: String,
    pub elapsed_secs: Option<String>,
    pub stop_target: String,
    pub stop_target_unit: String,
    pub memory_bytes: Option<String>,
    pub traversals_per_second: Option<String>,
    pub hand_updates_per_second: Option<String>,
    pub infosets: Option<String>,
    pub checkpoint: CheckpointProgressDto,
    pub seats: Vec<SeatMetricsDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: String,
    pub state: JobState,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub config_fingerprint: String,
    pub resumed_from_job_id: Option<String>,
    pub progress: Option<ProgressDto>,
    pub resume_available: bool,
    pub artifacts: JobArtifacts,
    pub error: Option<JobErrorDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalJobEvent {
    pub sequence: String,
    pub job_id: String,
    pub generated_at: String,
    pub kind: String,
    pub state: JobState,
    pub progress: Option<ProgressDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEventPage {
    pub events: Vec<LocalJobEvent>,
    pub last_sequence: Option<String>,
    pub terminal: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub job: JobSnapshot,
    pub terminal_status: JobState,
    pub effective_config_toml: String,
    pub guarantee_boundary: String,
    pub units: UnitsDto,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Run,
    Progress,
    Solution,
    Checkpoint,
}

impl ArtifactKind {
    pub fn default_file_name(self) -> &'static str {
        match self {
            Self::Run => "run.json",
            Self::Progress => "progress.jsonl",
            Self::Solution => "solution.mwsol",
            Self::Checkpoint => "checkpoint.mwckpt",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReceipt {
    pub file_name: String,
    pub byte_length: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointSource {
    pub source_id: String,
    pub file_name: String,
    pub size_bytes: String,
    pub modified_at: Option<String>,
    pub completed_sweeps: String,
    pub config_fingerprint: String,
    pub suggested_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedActionDto {
    pub id: String,
    pub semantic: String,
    pub amount_milli_bb: Option<String>,
    pub all_in: bool,
    pub full_raise: Option<bool>,
    pub label: String,
    pub destination: &'static str,
    pub child_node_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreadcrumbDto {
    pub node_id: String,
    pub actor_seat: u8,
    pub action: TypedActionDto,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyNodeDto {
    pub node_id: String,
    pub actor_seat: u8,
    pub street: String,
    pub pot_milli_bb: Option<String>,
    pub active_opponents: u8,
    pub breadcrumb: Vec<BreadcrumbDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyEntryDto {
    pub id: String,
    pub label: String,
    pub status: &'static str,
    pub weight: String,
    pub combo_count: Option<u8>,
    pub bucket_path: Option<Vec<u32>>,
    pub probability_u16: Option<Vec<u16>>,
    pub ev: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyViewDto {
    pub kind: String,
    pub entries: Vec<StrategyEntryDto>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategySnapshot {
    pub schema_version: u16,
    pub job_id: String,
    pub revision: String,
    pub status: String,
    pub strategy_kind: &'static str,
    pub generated_at: String,
    pub as_of_sweeps: String,
    pub current_sweeps: String,
    pub node: StrategyNodeDto,
    pub actions: Vec<TypedActionDto>,
    pub view: StrategyViewDto,
    pub approximate: bool,
    pub coverage: String,
}

#[derive(Clone)]
pub struct LocalBackend {
    inner: Arc<BackendInner>,
}

struct BackendInner {
    root: PathBuf,
    jobs: Mutex<HashMap<String, Arc<JobRecord>>>,
    active_job: Mutex<Option<String>>,
    config_sources: Mutex<HashMap<String, PathBuf>>,
    checkpoint_sources: Mutex<HashMap<String, PathBuf>>,
}

struct JobRecord {
    id: String,
    created_ms: u64,
    config_toml: String,
    config_fingerprint: String,
    max_sweeps: u64,
    stop_target: String,
    stop_target_unit: String,
    resumed_from_job_id: Option<String>,
    paths: JobPaths,
    cancel: Option<Arc<AtomicBool>>,
    next_event_sequence: AtomicU64,
    inner: Mutex<JobInner>,
}

#[derive(Clone)]
struct JobPaths {
    run: Option<PathBuf>,
    progress: Option<PathBuf>,
    solution: Option<PathBuf>,
    checkpoint: Option<PathBuf>,
}

struct JobInner {
    state: JobState,
    started_ms: Option<u64>,
    finished_ms: Option<u64>,
    error: Option<JobErrorDto>,
    latest_progress: Option<ProgressDto>,
    live_strategy: Option<StrategySnapshot>,
    requested_live_node: Option<LiveNodeRequest>,
}

#[derive(Clone, Copy)]
struct LiveNodeRequest {
    history: HistoryKey,
    expires_ms: u64,
}

impl LiveNodeRequest {
    fn active_history_at(self, now_ms: u64) -> Option<HistoryKey> {
        (self.expires_ms >= now_ms).then_some(self.history)
    }
}

impl JobRecord {
    fn started_ms(&self) -> u64 {
        lock(&self.inner).started_ms.unwrap_or(self.created_ms)
    }
}

struct ConfigContract {
    max_sweeps: u64,
    stop_target: String,
    stop_target_unit: String,
}

struct PreparedJobConfig {
    effective: String,
    fingerprint: String,
    config: SolveConfig,
}

impl LocalBackend {
    pub fn new(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("runs"))
            .with_context(|| format!("creating local run storage {}", root.display()))?;
        Ok(Self {
            inner: Arc::new(BackendInner {
                root,
                jobs: Mutex::new(HashMap::new()),
                active_job: Mutex::new(None),
                config_sources: Mutex::new(HashMap::new()),
                checkpoint_sources: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub fn capabilities(&self) -> LocalCapabilities {
        LocalCapabilities {
            service: "solvers",
            server_id: "local-in-process",
            solver_version: env!("CARGO_PKG_VERSION"),
            api_version: 3,
            busy: lock(&self.inner.active_job).is_some(),
            config_schemas: [cli::multiway_v1::SCHEMA],
            artifact_schemas: ArtifactSchemaCapabilities {
                run_json: [formats::MULTIWAY_SCHEMA_VERSION],
                progress_jsonl: [formats::MULTIWAY_SCHEMA_VERSION],
                mwsol: [formats::MWSOL_FORMAT_VERSION],
                mwckpt: [multiway::checkpoint::CHECKPOINT_VERSION],
                strategy_snapshot: [1],
            },
            features: LocalFeatureCapabilities {
                live_strategy_snapshots: true,
                resume: true,
                durable_jobs: false,
                native_file_dialogs: true,
            },
            limits: LocalLimits {
                max_players: 9,
                max_concurrent_jobs: 1,
            },
        }
    }

    pub fn register_config_source(&self, path: PathBuf) -> CommandResult<LoadedConfig> {
        let path = canonical_file(&path)?;
        let metadata = std::fs::metadata(&path).map_err(|error| {
            CommandError::new(
                "config_read_failed",
                format!("設定ファイルを読めません: {error}"),
            )
        })?;
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(CommandError::new(
                "config_too_large",
                "設定ファイルは1 MiB以下である必要があります。",
            ));
        }
        let config_toml = std::fs::read_to_string(&path).map_err(|error| {
            CommandError::new(
                "config_read_failed",
                format!("設定ファイルはUTF-8で読み込める必要があります: {error}"),
            )
        })?;
        let source_id = random_id();
        lock(&self.inner.config_sources).insert(source_id.clone(), path.clone());
        Ok(LoadedConfig {
            source_id,
            file_name: display_file_name(&path),
            config_toml,
        })
    }

    pub fn register_tree_source(&self, path: PathBuf) -> CommandResult<LoadedTreeSource> {
        let path = canonical_file(&path)?;
        let metadata = std::fs::metadata(&path).map_err(|error| {
            CommandError::new(
                "tree_script_read_failed",
                format!("Tree scriptを読めません: {error}"),
            )
        })?;
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(CommandError::new(
                "tree_script_too_large",
                "Tree scriptは1 MiB以下である必要があります。",
            ));
        }
        std::fs::read_to_string(&path).map_err(|error| {
            CommandError::new(
                "tree_script_read_failed",
                format!("Tree scriptはUTF-8で読み込める必要があります: {error}"),
            )
        })?;
        let source_id = random_id();
        lock(&self.inner.config_sources).insert(source_id.clone(), path.clone());
        Ok(LoadedTreeSource {
            source_id,
            file_name: display_file_name(&path),
        })
    }

    pub fn validate_config(&self, request: ValidateConfigRequest) -> ValidationResult {
        match self.validate_config_inner(&request) {
            Ok(result) => result,
            Err(error) => ValidationResult {
                valid: false,
                errors: vec![ValidationMessage {
                    code: "invalid_config".into(),
                    path: String::new(),
                    message: format!("{error:#}"),
                }],
                warnings: Vec::new(),
                effective_config_toml: None,
                config_fingerprint: None,
                preflight: None,
                guarantee_boundary: "設定が有効になるまでprofileの保証境界は確定しません。".into(),
                units: UnitsDto {
                    utility: "unknown".into(),
                    chip_unit_bb: "0.001",
                },
            },
        }
    }

    fn validate_config_inner(&self, request: &ValidateConfigRequest) -> Result<ValidationResult> {
        if !cli::multiway_v1::has_v1_schema(&request.config_toml)? {
            bail!(
                "GUI local solve requires schema = {:?}",
                cli::multiway_v1::SCHEMA
            );
        }
        let normalized = if let Some(source_id) = request.source_id.as_deref() {
            let sources = lock(&self.inner.config_sources);
            let path = sources
                .get(source_id)
                .ok_or_else(|| anyhow!("config source has expired; choose the TOML file again"))?;
            cli::multiway_v1::normalized_toml_at(&request.config_toml, path)?
        } else {
            cli::multiway_v1::normalized_toml(&request.config_toml)?
        };
        cli::multiway_v1::validate_production_contract(&normalized)?;
        let automatic_memory = toml::from_str::<toml::Value>(&request.config_toml)
            .ok()
            .and_then(|value| {
                value
                    .get("run")?
                    .get("resources")?
                    .get("memory")?
                    .as_str()
                    .map(|memory| memory == "auto")
            })
            .unwrap_or(false);
        let effective = normalized;
        let effective = cli::multiway_v1::normalized_toml(&effective)
            .context("canonicalizing the effective v1 config")?;
        let config = cli::config::parse_solve_config(&effective)?;
        let (seat_count, abstraction) = match &config.game {
            GameSection::PreflopMultiway(game) => (
                game.seats.len(),
                format!(
                    "{} / {}/{}/{} buckets",
                    match game.abstraction.kind {
                        multiway::AbstractionKind::RolloutKmeans => "rollout-kmeans",
                        multiway::AbstractionKind::Ehs2Table => "ehs2-table",
                    },
                    game.abstraction.flop_buckets,
                    game.abstraction.turn_buckets,
                    game.abstraction.river_buckets,
                ),
            ),
            _ => bail!("GUI local solve supports Multiway Preflop v1 only"),
        };
        let resource = cli::session::preflight_multiway_config(&effective)
            .context("preflighting ranges, betting tree, and solver memory")?;
        let utility = utility_name(&config.utility).to_string();
        let guarantee_boundary = if seat_count >= 3 {
            "3人以上はregret-minimized approximationです。Nash/GTO保証はありません。"
        } else {
            "External-sampling MCCFRのlinear average profileです。"
        };
        let threads = config
            .run
            .threads
            .map_or_else(|| "auto".into(), |count| count.to_string());
        let memory_budget_bytes = config
            .run
            .max_memory_bytes
            .filter(|bytes| *bytes != u64::MAX)
            .unwrap_or(cli::multiway_v1::PRODUCTION_POLICY_ARENA_AUTO_BYTES);
        let fits_budget = resource
            .solver_state_bytes
            .map(|estimate| estimate <= memory_budget_bytes);
        let headroom_bytes = resource
            .solver_state_bytes
            .filter(|estimate| *estimate <= memory_budget_bytes)
            .map(|estimate| memory_budget_bytes - estimate);
        let economics = resource
            .icm
            .map_or(ValidationEconomicsPreflight::Cash, |icm| {
                let fits_prepared_limit = icm
                    .prepared_bytes
                    .zip(icm.prepared_limit_bytes)
                    .map(|(required, limit)| required <= limit);
                ValidationEconomicsPreflight::TournamentIcm {
                    field_players: icm.field_players.to_string(),
                    paid_places: icm.paid_places.to_string(),
                    mode: match icm.mode {
                        cli::session::IcmPreflightMode::Exact => "exact",
                        cli::session::IcmPreflightMode::Sampled => "sampled",
                    },
                    samples: icm.samples.map(|value| value.to_string()),
                    seed: icm.seed.map(|value| value.to_string()),
                    prepared_bytes: icm.prepared_bytes.map(|value| value.to_string()),
                    prepared_limit_bytes: icm.prepared_limit_bytes.map(|value| value.to_string()),
                    fits_prepared_limit,
                }
            });
        let mut errors = Vec::new();
        if fits_budget == Some(false) {
            errors.push(ValidationMessage {
                code: "memory_budget_exceeded".into(),
                path: "run.resources.memory".into(),
                message: format!(
                    "Policy arenaに少なくとも{} bytes必要ですが、設定上限は{} bytesです。ツリーまたはbucket数を調整してください。",
                    resource.solver_state_bytes.expect("checked above"),
                    memory_budget_bytes,
                ),
            });
        }
        if let Some(icm) = resource.icm
            && let (Some(required), Some(limit)) = (icm.prepared_bytes, icm.prepared_limit_bytes)
            && required > limit
        {
            errors.push(ValidationMessage {
                code: "icm_prepared_memory_exceeded".into(),
                path: "economics.samples".into(),
                message: format!(
                    "Sampled ICMの準備領域に{required} bytes必要ですが、内部上限は{limit} bytesです。samplesまたは有賞順位数を減らしてください。"
                ),
            });
        }
        let mut warnings = Vec::new();
        if !resource.complete {
            warnings.push(ValidationMessage {
                code: "memory_prefix_exceeded".into(),
                path: "run.resources.memory".into(),
                message: "設定上限を超えた最初のtree prefixで検証を停止しました。表示node数と必要bytesは下限値です。".into(),
            });
        }
        let recall_mode = "current-street";
        let estimate_kind = if resource.complete {
            "exact-dense"
        } else {
            "prefix-lower-bound"
        };
        Ok(ValidationResult {
            valid: errors.is_empty(),
            errors,
            warnings,
            effective_config_toml: Some(effective.clone()),
            config_fingerprint: Some(fingerprint(effective.as_bytes())),
            preflight: Some(ValidationPreflight {
                threads,
                abstraction,
                economics,
                tree: ValidationTreePreflight {
                    recall_mode,
                    decision_nodes: resource.decision_nodes.to_string(),
                    terminal_edges: resource.terminal_edges.map(|value| value.to_string()),
                    policy_columns: resource.policy_columns.map(|value| value.to_string()),
                    policy_slots: resource.policy_slots.map(|value| value.to_string()),
                },
                memory: ValidationMemoryPreflight {
                    estimate_kind,
                    solver_state_bytes: resource.solver_state_bytes.map(|value| value.to_string()),
                    budget_mode: if automatic_memory { "auto" } else { "explicit" },
                    available_bytes: None,
                    budget_bytes: memory_budget_bytes.to_string(),
                    headroom_bytes: headroom_bytes.map(|value| value.to_string()),
                    fits_budget,
                },
            }),
            guarantee_boundary: guarantee_boundary.into(),
            units: UnitsDto {
                utility,
                chip_unit_bb: "0.001",
            },
        })
    }

    pub fn start_job(
        &self,
        request: StartJobRequest,
        events: EventSink,
    ) -> CommandResult<JobSnapshot> {
        let prepared = self.prepare_job_config(&request.effective_config_toml)?;
        if request.config_fingerprint != prepared.fingerprint {
            return Err(CommandError::new(
                "config_fingerprint_mismatch",
                "検証後にSolve設定が変更されています。もう一度検証してください。",
            ));
        }
        self.start_prepared_job(request.name, prepared, None, None, false, events)
    }

    fn prepare_job_config(&self, raw: &str) -> CommandResult<PreparedJobConfig> {
        let validation = self
            .validate_config_inner(&ValidateConfigRequest {
                config_toml: raw.into(),
                source_id: None,
            })
            .map_err(|error| {
                CommandError::new("invalid_config", format!("Solve設定が無効です: {error:#}"))
            })?;
        if let Some(error) = validation.errors.first() {
            return Err(CommandError::new(
                error.code.clone(),
                format!("Solve前検証に失敗しました: {}", error.message),
            ));
        }
        if !validation.valid {
            return Err(CommandError::new(
                "preflight_failed",
                "Solve前検証に失敗しました。",
            ));
        }
        let effective = validation.effective_config_toml.ok_or_else(|| {
            CommandError::new(
                "preflight_failed",
                "Solve前検証から実効設定が返りませんでした。",
            )
        })?;
        let fingerprint = validation.config_fingerprint.ok_or_else(|| {
            CommandError::new(
                "preflight_failed",
                "Solve前検証からfingerprintが返りませんでした。",
            )
        })?;
        let config = cli::config::parse_solve_config(&effective).map_err(|error| {
            CommandError::new("invalid_config", format!("Solve設定が無効です: {error:#}"))
        })?;
        if !matches!(config.game, GameSection::PreflopMultiway(_)) {
            return Err(CommandError::new(
                "unsupported_config",
                "GUI local solve supports Multiway Preflop v1 only.",
            ));
        }
        Ok(PreparedJobConfig {
            effective,
            fingerprint,
            config,
        })
    }

    fn start_prepared_job(
        &self,
        name: String,
        prepared: PreparedJobConfig,
        resume_source: Option<&Path>,
        resumed_from_job_id: Option<String>,
        reset_confirmations: bool,
        events: EventSink,
    ) -> CommandResult<JobSnapshot> {
        if name.chars().count() > 120 {
            return Err(CommandError::new(
                "invalid_job_name",
                "Solve名は120文字以内で指定してください。",
            ));
        }
        let PreparedJobConfig {
            effective,
            fingerprint: config_fingerprint,
            config,
        } = prepared;

        let mut active = lock(&self.inner.active_job);
        if let Some(active_id) = active.as_deref() {
            return Err(CommandError::retryable(
                "local_solver_busy",
                format!("このマシンでは別のSolve ({active_id}) が実行中です。"),
            ));
        }
        let (id, run_dir) = create_run_directory(&self.inner.root)?;
        let paths = managed_paths(run_dir);
        if let Some(source) = resume_source {
            let destination = paths
                .checkpoint
                .as_ref()
                .expect("managed jobs always have a checkpoint path");
            std::fs::copy(source, destination).map_err(|error| {
                CommandError::new(
                    "checkpoint_copy_failed",
                    format!("再開用checkpointをrun directoryへコピーできません: {error}"),
                )
            })?;
        }
        let max_sweeps = config.run.sweeps.unwrap_or(config.run.iterations);
        let stop_target = finite_decimal(config.run.stop_dev_gain.unwrap_or_default());
        let stop_target_unit = utility_name(&config.utility).to_string();
        let is_resume = resume_source.is_some();
        let cancel = Arc::new(AtomicBool::new(false));
        let record = Arc::new(JobRecord {
            id: id.clone(),
            created_ms: unix_ms(),
            config_toml: effective.clone(),
            config_fingerprint,
            max_sweeps,
            stop_target,
            stop_target_unit,
            resumed_from_job_id,
            paths,
            cancel: Some(Arc::clone(&cancel)),
            next_event_sequence: AtomicU64::new(0),
            inner: Mutex::new(JobInner {
                state: JobState::Queued,
                started_ms: None,
                finished_ms: None,
                error: None,
                latest_progress: None,
                live_strategy: None,
                requested_live_node: None,
            }),
        });
        lock(&self.inner.jobs).insert(id.clone(), Arc::clone(&record));
        *active = Some(id.clone());
        drop(active);

        let backend = Arc::clone(&self.inner);
        let worker_record = Arc::clone(&record);
        let worker_events = Arc::clone(&events);
        let worker_name = format!("local-solver-{id}");
        let spawn = std::thread::Builder::new()
            .name(worker_name)
            .spawn(move || {
                run_worker(
                    &backend,
                    &worker_record,
                    config,
                    cancel,
                    is_resume,
                    reset_confirmations,
                    worker_events,
                );
            });
        if let Err(error) = spawn {
            mark_failed(
                &self.inner,
                &record,
                "worker_start_failed",
                format!("Solver workerを開始できません: {error}"),
                &events,
            );
            return Err(CommandError::new(
                "worker_start_failed",
                format!("Solver workerを開始できません: {error}"),
            ));
        }
        Ok(snapshot(&record))
    }

    pub fn list_jobs(&self) -> Vec<JobSnapshot> {
        let mut jobs: Vec<_> = lock(&self.inner.jobs)
            .values()
            .map(|record| snapshot(record))
            .collect();
        jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        jobs
    }

    pub fn get_job(&self, id: &str) -> CommandResult<JobSnapshot> {
        let record = self.job(id)?;
        Ok(snapshot(&record))
    }

    pub fn cancel_job(&self, id: &str, events: &EventSink) -> CommandResult<JobSnapshot> {
        let record = self.job(id)?;
        let mut inner = lock(&record.inner);
        match inner.state {
            JobState::Queued | JobState::Validating | JobState::Running => {
                let cancel = record.cancel.as_ref().ok_or_else(|| {
                    CommandError::new(
                        "cancellation_unavailable",
                        "importした成果物は停止できません。",
                    )
                })?;
                cancel.store(true, Ordering::Relaxed);
                inner.state = JobState::Cancelling;
            }
            JobState::Cancelling | JobState::Cancelled => {}
            _ => {}
        }
        let state = inner.state;
        drop(inner);
        emit_event(events, &record, "state", state, latest_progress(&record));
        Ok(snapshot(&record))
    }

    pub fn get_progress(
        &self,
        id: &str,
        after_sequence: Option<&str>,
    ) -> CommandResult<ProgressEventPage> {
        let record = self.job(id)?;
        let after = after_sequence
            .map(|value| {
                value.parse::<u64>().map_err(|_| {
                    CommandError::new(
                        "invalid_sequence",
                        "afterSequence must be an unsigned base-10 integer.",
                    )
                })
            })
            .transpose()?;
        let events = read_progress_events(&record, after)?;
        let last_sequence = events.last().map(|event| event.sequence.clone());
        let terminal = lock(&record.inner).state.is_terminal();
        Ok(ProgressEventPage {
            events,
            last_sequence,
            terminal,
        })
    }

    pub fn get_result(&self, id: &str) -> CommandResult<JobResult> {
        let record = self.job(id)?;
        let job = snapshot(&record);
        if !job.state.is_terminal() {
            return Err(CommandError::retryable(
                "result_not_ready",
                "Solveはまだ実行中です。",
            ));
        }
        let run = match record.paths.run.as_deref() {
            Some(path) if path.is_file() => read_json_limited(path, MAX_RESULT_BYTES)?,
            _ if record
                .paths
                .solution
                .as_ref()
                .is_some_and(|path| path.is_file()) =>
            {
                let path = record.paths.solution.as_ref().expect("checked above");
                let reader = formats::MwSolReader::open(path).map_err(|error| {
                    CommandError::new(
                        "artifact_read_failed",
                        format!("solution.mwsolを開けません: {error}"),
                    )
                })?;
                let metadata = reader.metadata();
                serde_json::json!({
                    "schemaVersion": metadata.schema_version,
                    "status": metadata.stop_status,
                    "approximateProfile": metadata.approximate_profile,
                    "sweeps": metadata.sweeps,
                    "seats": metadata.seats,
                    "configHash": formats::config_hash_hex(&metadata.config_fingerprint),
                    "importedSolution": true,
                })
            }
            _ => {
                return Err(CommandError::new(
                    "result_unavailable",
                    "このjobには閲覧できるrun resultがありません。",
                ));
            }
        };
        let effective = effective_config_for(&record)?;
        let utility = cli::config::parse_solve_config(&effective)
            .map(|config| utility_name(&config.utility).to_string())
            .map_err(|error| {
                CommandError::new(
                    "invalid_effective_config",
                    format!("成果物内のeffective configが無効です: {error:#}"),
                )
            })?;
        let guarantee_boundary = run
            .get("guaranteeBoundary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(
                "Multiway profile is a regret-minimized approximation; no Nash/GTO guarantee.",
            )
            .to_string();
        Ok(JobResult {
            terminal_status: job.state,
            job,
            effective_config_toml: effective,
            guarantee_boundary,
            units: UnitsDto {
                utility,
                chip_unit_bb: "0.001",
            },
        })
    }

    pub fn open_run(&self, path: PathBuf) -> CommandResult<JobSnapshot> {
        let run_dir = canonical_directory(&path)?;
        let run_path = run_dir.join("run.json");
        let run = read_json_limited(&run_path, MAX_RESULT_BYTES)?;
        ensure_v1_run(&run)?;
        let status = run
            .get("status")
            .and_then(serde_json::Value::as_str)
            .and_then(parse_terminal_state)
            .ok_or_else(|| {
                CommandError::new(
                    "invalid_run",
                    "run.jsonに対応済みのterminal statusがありません。",
                )
            })?;
        let id = format!("import-{}", random_id());
        let config_fingerprint = run
            .get("configHash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let run_sweeps = run
            .get("sweeps")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| CommandError::new("invalid_run", "run.jsonにsweepsがありません。"))?;
        let paths = JobPaths {
            run: Some(run_path),
            progress: run_dir
                .join("progress.jsonl")
                .is_file()
                .then(|| run_dir.join("progress.jsonl")),
            solution: run_dir
                .join("solution.mwsol")
                .is_file()
                .then(|| run_dir.join("solution.mwsol")),
            checkpoint: run_dir
                .join("checkpoint.mwckpt")
                .is_file()
                .then(|| run_dir.join("checkpoint.mwckpt")),
        };
        let mut config_toml = String::new();
        if let Some(solution) = paths.solution.as_deref() {
            let reader = formats::MwSolReader::open(solution).map_err(|error| {
                CommandError::new(
                    "artifact_read_failed",
                    format!("run directoryのsolution.mwsolが無効です: {error}"),
                )
            })?;
            ensure_supported_solution_version(reader.format_version())?;
            ensure_v1_toml(&reader.metadata().config_toml, "solution.mwsol")?;
            let metadata = reader.metadata();
            let solution_config_fingerprint =
                formats::config_hash_hex(&metadata.config_fingerprint);
            let run_status = run
                .get("status")
                .and_then(serde_json::Value::as_str)
                .expect("validated above");
            if config_fingerprint != solution_config_fingerprint
                || run_status != metadata.stop_status
                || run_sweeps != metadata.sweeps
            {
                return Err(CommandError::new(
                    "artifact_identity_mismatch",
                    "run.jsonと同居するsolution.mwsolのconfig fingerprint、status、sweepsが一致しません。",
                ));
            }
            config_toml = reader.metadata().config_toml.clone();
        }
        if let Some(checkpoint_path) = paths.checkpoint.as_deref() {
            let checkpoint = multiway::checkpoint::MultiwayCheckpoint::load_unchecked(
                checkpoint_path,
            )
            .map_err(|error| {
                CommandError::new(
                    "checkpoint_read_failed",
                    format!("run directoryのcheckpoint.mwckptが無効です: {error}"),
                )
            })?;
            let raw = checkpoint.config_toml.as_deref().ok_or_else(|| {
                CommandError::new(
                    "checkpoint_not_self_contained",
                    "run directoryのcheckpointはv7 self-contained形式ではありません。",
                )
            })?;
            ensure_v1_toml(raw, "checkpoint.mwckpt")?;
            let normalized = cli::multiway_v1::normalized_toml(raw).map_err(|error| {
                CommandError::new(
                    "invalid_effective_config",
                    format!("checkpoint.mwckptのconfigが無効です: {error:#}"),
                )
            })?;
            if fingerprint(normalized.as_bytes()) != config_fingerprint
                || checkpoint.state.completed_sweeps != run_sweeps
            {
                return Err(CommandError::new(
                    "artifact_identity_mismatch",
                    "run.jsonと同居するcheckpoint.mwckptのconfig fingerprintまたはsweepsが一致しません。",
                ));
            }
            if config_toml.is_empty() {
                config_toml = normalized;
            }
        }
        let contract = if config_toml.is_empty() {
            None
        } else {
            Some(config_contract(&config_toml).map_err(|error| {
                CommandError::new(
                    "invalid_effective_config",
                    format!("run directoryのconfigが無効です: {error:#}"),
                )
            })?)
        };
        let finished_ms = run
            .get("finishedUnixMs")
            .and_then(serde_json::Value::as_u64)
            .or_else(|| file_modified_ms(&run_dir));
        let record = Arc::new(JobRecord {
            id: id.clone(),
            created_ms: run
                .get("startedUnixMs")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_else(unix_ms),
            config_toml,
            config_fingerprint,
            max_sweeps: contract
                .as_ref()
                .map(|contract| contract.max_sweeps)
                .unwrap_or(run_sweeps),
            stop_target: contract
                .as_ref()
                .map(|contract| contract.stop_target.clone())
                .unwrap_or_else(|| "0".into()),
            stop_target_unit: contract
                .as_ref()
                .map(|contract| contract.stop_target_unit.clone())
                .unwrap_or_else(|| {
                    run.get("utilityUnit")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                        .to_string()
                }),
            resumed_from_job_id: None,
            paths,
            cancel: None,
            next_event_sequence: AtomicU64::new(0),
            inner: Mutex::new(JobInner {
                state: status,
                started_ms: run.get("startedUnixMs").and_then(serde_json::Value::as_u64),
                finished_ms,
                error: None,
                latest_progress: None,
                live_strategy: None,
                requested_live_node: None,
            }),
        });
        lock(&self.inner.jobs).insert(id, Arc::clone(&record));
        Ok(snapshot(&record))
    }

    pub fn open_solution(&self, path: PathBuf) -> CommandResult<JobSnapshot> {
        let path = canonical_file(&path)?;
        let reader = formats::MwSolReader::open(&path).map_err(|error| {
            CommandError::new(
                "artifact_read_failed",
                format!("solution.mwsolが無効です: {error}"),
            )
        })?;
        ensure_supported_solution_version(reader.format_version())?;
        let metadata = reader.metadata().clone();
        ensure_v1_toml(&metadata.config_toml, "solution.mwsol")?;
        let state = parse_terminal_state(&metadata.stop_status).ok_or_else(|| {
            CommandError::new(
                "unsupported_artifact",
                format!(
                    "solution.mwsolのstop status {:?} はGUI v1で未対応です。",
                    metadata.stop_status
                ),
            )
        })?;
        let contract = config_contract(&metadata.config_toml).map_err(|error| {
            CommandError::new(
                "invalid_effective_config",
                format!("solution.mwsolのconfigが無効です: {error:#}"),
            )
        })?;
        let imported_progress = progress_from_solution(&metadata, &contract);
        let id = format!("import-{}", random_id());
        let record = Arc::new(JobRecord {
            id: id.clone(),
            created_ms: file_modified_ms(&path).unwrap_or_else(unix_ms),
            config_toml: metadata.config_toml,
            config_fingerprint: formats::config_hash_hex(&metadata.config_fingerprint),
            max_sweeps: contract.max_sweeps,
            stop_target: contract.stop_target,
            stop_target_unit: contract.stop_target_unit,
            resumed_from_job_id: None,
            paths: JobPaths {
                run: None,
                progress: None,
                solution: Some(path.clone()),
                checkpoint: None,
            },
            cancel: None,
            next_event_sequence: AtomicU64::new(0),
            inner: Mutex::new(JobInner {
                state,
                started_ms: None,
                finished_ms: file_modified_ms(&path),
                error: None,
                latest_progress: Some(imported_progress),
                live_strategy: None,
                requested_live_node: None,
            }),
        });
        lock(&self.inner.jobs).insert(id, Arc::clone(&record));
        Ok(snapshot(&record))
    }

    pub fn register_checkpoint(&self, path: PathBuf) -> CommandResult<CheckpointSource> {
        let path = canonical_file(&path)?;
        let checkpoint =
            multiway::checkpoint::MultiwayCheckpoint::load_unchecked(&path).map_err(|error| {
                CommandError::new(
                    "checkpoint_read_failed",
                    format!("checkpoint.mwckptが無効です: {error}"),
                )
            })?;
        let config_toml = checkpoint.config_toml.as_deref().ok_or_else(|| {
            CommandError::new(
                "checkpoint_not_self_contained",
                "GUI resumeにはv7 self-contained checkpointが必要です。",
            )
        })?;
        if !cli::multiway_v1::has_v1_schema(config_toml).map_err(CommandError::from)? {
            return Err(CommandError::new(
                "unsupported_checkpoint",
                "GUI resumeにはMultiway Preflop v1 checkpointが必要です。",
            ));
        }
        let source_id = random_id();
        lock(&self.inner.checkpoint_sources).insert(source_id.clone(), path.clone());
        let metadata = std::fs::metadata(&path).map_err(|error| {
            CommandError::new(
                "checkpoint_read_failed",
                format!("checkpoint metadataを読めません: {error}"),
            )
        })?;
        Ok(CheckpointSource {
            source_id,
            file_name: display_file_name(&path),
            size_bytes: metadata.len().to_string(),
            modified_at: file_modified_ms(&path).map(rfc3339),
            completed_sweeps: checkpoint.state.completed_sweeps.to_string(),
            config_fingerprint: fingerprint(config_toml.as_bytes()),
            suggested_name: format!("Resumed {}", display_file_name(&path)),
        })
    }

    pub fn resume_checkpoint(
        &self,
        source_id: &str,
        request: ResumeJobRequest,
        events: EventSink,
    ) -> CommandResult<JobSnapshot> {
        let source = lock(&self.inner.checkpoint_sources)
            .get(source_id)
            .cloned()
            .ok_or_else(|| {
                CommandError::new(
                    "checkpoint_source_expired",
                    "checkpointをもう一度選択してください。",
                )
            })?;
        self.resume_from_path(&source, None, request, events)
    }

    pub fn resume_job(
        &self,
        id: &str,
        request: ResumeJobRequest,
        events: EventSink,
    ) -> CommandResult<JobSnapshot> {
        let record = self.job(id)?;
        if !lock(&record.inner).state.is_terminal() {
            return Err(CommandError::new(
                "job_not_terminal",
                "実行中のjobはresumeできません。",
            ));
        }
        let checkpoint = record
            .paths
            .checkpoint
            .as_ref()
            .filter(|path| path.is_file())
            .cloned()
            .ok_or_else(|| {
                CommandError::new(
                    "checkpoint_unavailable",
                    "このjobには再開可能なcheckpointがありません。",
                )
            })?;
        self.resume_from_path(&checkpoint, Some(record.id.clone()), request, events)
    }

    fn resume_from_path(
        &self,
        checkpoint_path: &Path,
        resumed_from_job_id: Option<String>,
        request: ResumeJobRequest,
        events: EventSink,
    ) -> CommandResult<JobSnapshot> {
        let checkpoint = multiway::checkpoint::MultiwayCheckpoint::load_unchecked(checkpoint_path)
            .map_err(|error| {
                CommandError::new(
                    "checkpoint_read_failed",
                    format!("checkpointを読み込めません: {error}"),
                )
            })?;
        let raw = checkpoint.config_toml.ok_or_else(|| {
            CommandError::new(
                "checkpoint_not_self_contained",
                "GUI resumeにはv7 self-contained checkpointが必要です。",
            )
        })?;
        let overrides = request.overrides;
        let max_sweeps = parse_optional_u64("maxSweeps", overrides.max_sweeps.as_deref())?;
        let evaluation_samples =
            parse_optional_u64("evaluationSamples", overrides.evaluation_samples.as_deref())?;
        let evaluation_cadence =
            parse_optional_u64("checkEverySweeps", overrides.check_every_sweeps.as_deref())?;
        let threads = parse_optional_usize("threads", overrides.threads.as_deref())?;
        let stop_target =
            parse_optional_positive_f64("stopTarget", overrides.stop_target.as_deref())?;
        let effective = cli::multiway_v1::apply_resume_overrides(
            &raw,
            threads,
            overrides.memory.as_deref(),
            overrides.max_time.as_deref(),
            max_sweeps,
            stop_target,
            evaluation_samples,
            evaluation_cadence,
            overrides.checkpoint_interval.as_deref(),
        )
        .map_err(|error| {
            CommandError::new(
                "invalid_resume_overrides",
                format!("再開設定が無効です: {error:#}"),
            )
        })?;
        let prepared = self.prepare_job_config(&effective)?;
        self.start_prepared_job(
            request.name.unwrap_or_else(|| "Resumed solve".into()),
            prepared,
            Some(checkpoint_path),
            resumed_from_job_id,
            overrides.stop_target.is_some(),
            events,
        )
    }

    pub fn get_strategy(&self, id: &str, node_id: Option<&str>) -> CommandResult<StrategySnapshot> {
        let record = self.job(id)?;
        let requested_id = node_id.unwrap_or(ROOT_NODE_ID).to_ascii_lowercase();
        if !record
            .paths
            .solution
            .as_ref()
            .is_some_and(|path| path.is_file())
        {
            let requested = HistoryKey(parse_node_id(&requested_id)?);
            let mut inner = lock(&record.inner);
            inner.requested_live_node = Some(LiveNodeRequest {
                history: requested,
                expires_ms: unix_ms().saturating_add(LIVE_STRATEGY_LEASE_MILLIS),
            });
            if let Some(strategy) = inner
                .live_strategy
                .as_ref()
                .filter(|strategy| strategy.node.node_id == requested_id)
                .cloned()
            {
                return Ok(strategy);
            }
            return Err(CommandError::retryable(
                "strategy_snapshot_pending",
                "選択したPreflop Nodeのstrategyを次のlive更新で取得します。",
            ));
        }
        let solution = record
            .paths
            .solution
            .as_ref()
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                CommandError::retryable(
                    "strategy_not_ready",
                    "正式strategyはsolution.mwsol生成後に閲覧できます。",
                )
            })?;
        strategy_snapshot(&record, solution, &requested_id)
    }

    pub fn artifact_path(&self, id: &str, kind: ArtifactKind) -> CommandResult<PathBuf> {
        let record = self.job(id)?;
        let path = match kind {
            ArtifactKind::Run => record.paths.run.as_ref(),
            ArtifactKind::Progress => record.paths.progress.as_ref(),
            ArtifactKind::Solution => record.paths.solution.as_ref(),
            ArtifactKind::Checkpoint => record.paths.checkpoint.as_ref(),
        }
        .filter(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            CommandError::new(
                "artifact_unavailable",
                format!("{} はまだ生成されていません。", kind.default_file_name()),
            )
        })?;
        Ok(path)
    }

    pub fn save_config_to(
        &self,
        destination: &Path,
        config_toml: &str,
    ) -> CommandResult<FileReceipt> {
        write_atomic(destination, config_toml.as_bytes()).map_err(CommandError::from)?;
        file_receipt(destination)
    }

    pub fn export_artifact_to(
        &self,
        id: &str,
        kind: ArtifactKind,
        destination: &Path,
    ) -> CommandResult<FileReceipt> {
        let source = self.artifact_path(id, kind)?;
        copy_atomic(&source, destination).map_err(CommandError::from)?;
        file_receipt(destination)
    }

    fn job(&self, id: &str) -> CommandResult<Arc<JobRecord>> {
        lock(&self.inner.jobs).get(id).cloned().ok_or_else(|| {
            CommandError::new("job_not_found", format!("job {id:?} は存在しません。"))
        })
    }
}

fn run_worker(
    backend: &Arc<BackendInner>,
    record: &Arc<JobRecord>,
    config: SolveConfig,
    cancel: Arc<AtomicBool>,
    is_resume: bool,
    reset_confirmations: bool,
    events: EventSink,
) {
    {
        let mut inner = lock(&record.inner);
        inner.state = JobState::Running;
        inner.started_ms = Some(unix_ms());
    }
    emit_event(
        &events,
        record,
        "state",
        JobState::Running,
        latest_progress(record),
    );

    let run_path = record
        .paths
        .run
        .as_deref()
        .expect("managed job has run.json");
    let progress_path = record
        .paths
        .progress
        .as_deref()
        .expect("managed job has progress.jsonl");
    let checkpoint_path = record
        .paths
        .checkpoint
        .as_deref()
        .expect("managed job has checkpoint.mwckpt");
    let solution_path = record
        .paths
        .solution
        .as_deref()
        .expect("managed job has solution.mwsol");
    let config_hash = formats::config_hash(record.config_toml.as_bytes());

    let request_record = Arc::clone(record);
    let live_node_request = move || {
        lock(&request_record.inner)
            .requested_live_node
            .and_then(|request| request.active_history_at(unix_ms()))
    };
    let observation_record = Arc::clone(record);
    let observation_events = Arc::clone(&events);
    let mut observer = move |observation: cli::multiway_solve::MultiwayRunObservation| {
        let (progress, strategy) = match observation {
            cli::multiway_solve::MultiwayRunObservation::Quality(observation) => {
                let progress = progress_from_metrics(&observation.metrics, &observation_record);
                let strategy = live_node_snapshot(
                    &observation_record,
                    observation.metrics.sweeps,
                    observation.strategy_node.as_ref(),
                );
                (progress, strategy)
            }
            cli::multiway_solve::MultiwayRunObservation::Live(observation) => {
                let previous = lock(&observation_record.inner).latest_progress.clone();
                let progress = progress_from_live(&observation, &observation_record, previous);
                let strategy = live_node_snapshot(
                    &observation_record,
                    observation.sweeps,
                    observation.strategy_node.as_ref(),
                );
                (progress, strategy)
            }
        };
        let state = {
            let mut inner = lock(&observation_record.inner);
            inner.latest_progress = Some(progress.clone());
            inner.live_strategy = strategy;
            inner.state
        };
        emit_event(
            &observation_events,
            &observation_record,
            "progress",
            state,
            Some(progress),
        );
    };

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if is_resume {
            cli::multiway_solve::resume_observed_with_node(
                &record.config_toml,
                config,
                Some(run_path),
                Some(progress_path),
                checkpoint_path,
                config_hash,
                Some(solution_path),
                Some(cancel.as_ref()),
                reset_confirmations,
                false,
                &live_node_request,
                &mut observer,
            )
        } else {
            cli::multiway_solve::run_observed_with_node(
                &record.config_toml,
                config,
                Some(run_path),
                Some(progress_path),
                Some(checkpoint_path),
                config_hash,
                Some(solution_path),
                Some(cancel.as_ref()),
                false,
                &live_node_request,
                &mut observer,
            )
        }
    }));

    let (state, error) = match outcome {
        Ok(Ok(())) => match read_terminal_state(run_path) {
            Ok(state) => (state, None),
            Err(error) => (
                JobState::Failed,
                Some(JobErrorDto {
                    code: "invalid_run_result".into(),
                    message: format!("{error:#}"),
                    retryable: false,
                }),
            ),
        },
        Ok(Err(error)) => (
            JobState::Failed,
            Some(JobErrorDto {
                code: "solve_failed".into(),
                message: format!("{error:#}"),
                retryable: false,
            }),
        ),
        Err(_) => (
            JobState::Failed,
            Some(JobErrorDto {
                code: "solver_panicked".into(),
                message: "Solver workerでpanicが発生しました。".into(),
                retryable: false,
            }),
        ),
    };
    {
        let mut inner = lock(&record.inner);
        inner.state = state;
        inner.finished_ms = Some(unix_ms());
        inner.error = error;
        if let Some(progress) = latest_progress_file(record) {
            inner.latest_progress = Some(progress);
        }
        if let Some(strategy) = inner.live_strategy.as_mut() {
            strategy.status = if record
                .paths
                .solution
                .as_ref()
                .is_some_and(|path| path.is_file())
            {
                "final".into()
            } else {
                "stale".into()
            };
            strategy.current_sweeps = strategy.as_of_sweeps.clone();
        }
    }
    {
        let mut active = lock(&backend.active_job);
        if active.as_deref() == Some(record.id.as_str()) {
            *active = None;
        }
    }
    emit_event(&events, record, "state", state, latest_progress(record));
}

fn progress_from_metrics(metrics: &MultiwayMetricsRow, record: &JobRecord) -> ProgressDto {
    ProgressDto {
        sweeps: metrics.sweeps.to_string(),
        max_sweeps: record.max_sweeps.to_string(),
        elapsed_secs: Some(finite_decimal(metrics.elapsed_secs)),
        stop_target: record.stop_target.clone(),
        stop_target_unit: record.stop_target_unit.clone(),
        memory_bytes: Some(metrics.memory_bytes.to_string()),
        traversals_per_second: Some(finite_decimal(metrics.traversals_per_second)),
        hand_updates_per_second: Some(finite_decimal(metrics.hand_updates_per_second)),
        infosets: Some(metrics.infosets.to_string()),
        checkpoint: checkpoint_progress(record),
        seats: metrics.seats.iter().map(seat_metrics_dto).collect(),
    }
}

fn progress_from_live(
    live: &cli::multiway_solve::MultiwayLiveObservation,
    record: &JobRecord,
    previous: Option<ProgressDto>,
) -> ProgressDto {
    let mut progress = previous.unwrap_or_else(|| ProgressDto {
        sweeps: "0".into(),
        max_sweeps: record.max_sweeps.to_string(),
        elapsed_secs: None,
        stop_target: record.stop_target.clone(),
        stop_target_unit: record.stop_target_unit.clone(),
        memory_bytes: None,
        traversals_per_second: None,
        hand_updates_per_second: None,
        infosets: None,
        checkpoint: checkpoint_progress(record),
        seats: Vec::new(),
    });
    progress.sweeps = live.sweeps.to_string();
    progress.elapsed_secs = Some(finite_decimal(live.elapsed_secs));
    progress.traversals_per_second = (live.elapsed_secs > 0.0)
        .then(|| finite_decimal(live.traversals as f64 / live.elapsed_secs));
    progress.hand_updates_per_second = (live.elapsed_secs > 0.0)
        .then(|| finite_decimal(live.hand_updates as f64 / live.elapsed_secs));
    progress.checkpoint = checkpoint_progress(record);
    progress
}

fn progress_from_solution(
    metadata: &formats::MultiwaySolutionMetadata,
    contract: &ConfigContract,
) -> ProgressDto {
    ProgressDto {
        sweeps: metadata.sweeps.to_string(),
        max_sweeps: contract.max_sweeps.to_string(),
        elapsed_secs: None,
        stop_target: contract.stop_target.clone(),
        stop_target_unit: contract.stop_target_unit.clone(),
        memory_bytes: None,
        traversals_per_second: None,
        hand_updates_per_second: None,
        infosets: None,
        checkpoint: CheckpointProgressDto {
            available: false,
            generated_at: None,
        },
        seats: metadata.seats.iter().map(seat_result_dto).collect(),
    }
}

fn seat_metrics_dto(metrics: &formats::MultiwaySeatMetrics) -> SeatMetricsDto {
    SeatMetricsDto {
        seat: metrics.seat,
        profile_ev: metrics.profile_ev.as_ref().map(estimate_dto),
        average_positive_regret: Some(finite_decimal(metrics.average_positive_regret)),
        strategy_drift_l1: Some(finite_decimal(metrics.strategy_drift_l1)),
        deviation_gain: metrics
            .deviation_gain_lower_bound
            .as_ref()
            .map(estimate_dto),
    }
}

fn seat_result_dto(result: &formats::MultiwaySeatResult) -> SeatMetricsDto {
    SeatMetricsDto {
        seat: result.seat,
        profile_ev: result.profile_ev.as_ref().map(estimate_dto),
        average_positive_regret: Some(finite_decimal(result.average_positive_regret)),
        strategy_drift_l1: Some(finite_decimal(result.strategy_drift_l1)),
        deviation_gain: result.deviation_gain_lower_bound.as_ref().map(estimate_dto),
    }
}

fn estimate_dto(estimate: &formats::Estimate) -> EstimateDto {
    EstimateDto {
        mean: finite_decimal(estimate.mean),
        stderr: finite_decimal(estimate.stderr),
        ci95: [
            finite_decimal(estimate.ci95[0]),
            finite_decimal(estimate.ci95[1]),
        ],
    }
}

fn checkpoint_progress(record: &JobRecord) -> CheckpointProgressDto {
    let modified = record
        .paths
        .checkpoint
        .as_deref()
        .filter(|path| path.is_file())
        .and_then(file_modified_ms);
    CheckpointProgressDto {
        available: modified.is_some(),
        generated_at: modified.map(rfc3339),
    }
}

fn live_node_snapshot(
    record: &JobRecord,
    sweeps: u64,
    node: Option<&cli::multiway_solve::LiveStrategyNode>,
) -> Option<StrategySnapshot> {
    let node = node?;
    if node.street != 0 {
        return None;
    }
    let actor = node.actor;
    let actions = node
        .actions
        .iter()
        .map(|action| {
            let mut typed = typed_action_from_label(&action.action);
            typed.destination = action.destination;
            typed.child_node_id = action.child.map(|child| hex(&child.0));
            typed
        })
        .collect::<Vec<_>>();
    let by_bucket: BTreeMap<u32, &cli::multiway_solve::LiveStrategyEntry> = node
        .strategy
        .iter()
        .filter(|entry| entry.key.player == actor && entry.key.street == 0)
        .map(|entry| (entry.key.bucket_path[0], entry))
        .collect();
    let entries = (0..cards_class_count())
        .map(|bucket| {
            let label = hand_label(bucket);
            match by_bucket.get(&(bucket as u32)) {
                Some(entry) => StrategyEntryDto {
                    id: format!("preflop:{bucket}"),
                    label,
                    status: "visited",
                    weight: finite_decimal(entry.weight),
                    combo_count: Some(combo_count(bucket)),
                    bucket_path: None,
                    probability_u16: Some(quantize_u16(&entry.probabilities)),
                    ev: None,
                },
                None => StrategyEntryDto {
                    id: format!("preflop:{bucket}"),
                    label,
                    status: "unvisited",
                    weight: "0".into(),
                    combo_count: Some(combo_count(bucket)),
                    bucket_path: None,
                    probability_u16: None,
                    ev: None,
                },
            }
        })
        .collect::<Vec<_>>();
    let coverage = entries
        .iter()
        .filter(|entry| entry.status == "visited")
        .count() as f64
        / cards_class_count() as f64;
    Some(StrategySnapshot {
        schema_version: 1,
        job_id: record.id.clone(),
        revision: sweeps.to_string(),
        status: "live-average".into(),
        strategy_kind: "linear-average",
        generated_at: rfc3339(unix_ms()),
        as_of_sweeps: sweeps.to_string(),
        current_sweeps: sweeps.to_string(),
        node: StrategyNodeDto {
            node_id: hex(&node.history.0),
            actor_seat: actor,
            street: "preflop".into(),
            pot_milli_bb: None,
            active_opponents: node.active_opponents,
            breadcrumb: node
                .breadcrumb
                .iter()
                .map(|item| BreadcrumbDto {
                    node_id: hex(&item.node.0),
                    actor_seat: item.actor,
                    action: typed_action_from_label(&item.action),
                })
                .collect(),
        },
        actions,
        view: StrategyViewDto {
            kind: "preflop-hand-classes".into(),
            entries,
        },
        approximate: true,
        coverage: finite_decimal(coverage),
    })
}

fn snapshot(record: &JobRecord) -> JobSnapshot {
    let (state, started_ms, finished_ms, error, cached_progress) = {
        let inner = lock(&record.inner);
        (
            inner.state,
            inner.started_ms,
            inner.finished_ms,
            inner.error.clone(),
            inner.latest_progress.clone(),
        )
    };
    // Live observations are newer than the last durable progress.jsonl row.
    // Prefer the cache while a process is attached; terminal/import paths
    // already seed or refresh that cache from their durable artifact.
    let mut progress = cached_progress.or_else(|| latest_progress_file(record));
    if let Some(progress) = progress.as_mut() {
        progress.checkpoint = checkpoint_progress(record);
    }
    let artifacts = JobArtifacts {
        run: descriptor(record.paths.run.as_deref()),
        progress: descriptor(record.paths.progress.as_deref()),
        solution: descriptor(record.paths.solution.as_deref()),
        checkpoint: descriptor(record.paths.checkpoint.as_deref()),
    };
    JobSnapshot {
        id: record.id.clone(),
        state,
        created_at: rfc3339(record.created_ms),
        started_at: started_ms.map(rfc3339),
        finished_at: finished_ms.map(rfc3339),
        config_fingerprint: record.config_fingerprint.clone(),
        resumed_from_job_id: record.resumed_from_job_id.clone(),
        progress,
        resume_available: artifacts.checkpoint.available && state.is_terminal(),
        artifacts,
        error,
    }
}

fn latest_progress(record: &JobRecord) -> Option<ProgressDto> {
    let mut progress = lock(&record.inner)
        .latest_progress
        .clone()
        .or_else(|| latest_progress_file(record));
    if let Some(progress) = progress.as_mut() {
        progress.checkpoint = checkpoint_progress(record);
    }
    progress
}

fn latest_progress_file(record: &JobRecord) -> Option<ProgressDto> {
    let path = record.paths.progress.as_deref()?;
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    reader
        .lines()
        .map_while(std::result::Result::ok)
        .filter_map(|line| parse_progress_line(&line, record))
        .map(|event| event.progress)
        .last()
}

struct ParsedProgressEvent {
    sequence: u64,
    event: String,
    progress: ProgressDto,
}

fn read_progress_events(
    record: &JobRecord,
    after: Option<u64>,
) -> CommandResult<Vec<LocalJobEvent>> {
    let Some(path) = record.paths.progress.as_deref() else {
        return Ok(Vec::new());
    };
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file = File::open(path).map_err(|error| {
        CommandError::new(
            "progress_read_failed",
            format!("progress.jsonlを開けません: {error}"),
        )
    })?;
    let mut events = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| {
            CommandError::new(
                "progress_read_failed",
                format!("progress.jsonlを読めません: {error}"),
            )
        })?;
        let Some(parsed) = parse_progress_line(&line, record) else {
            continue;
        };
        if after.is_none_or(|after| parsed.sequence > after) {
            let state = parse_terminal_state(&parsed.event).unwrap_or(JobState::Running);
            let kind = if parsed.event == "checkpoint" {
                "checkpoint"
            } else {
                "progress"
            };
            let generated_ms = record.started_ms().saturating_add(
                parsed
                    .progress
                    .elapsed_secs
                    .as_deref()
                    .map(decimal_seconds_to_millis)
                    .unwrap_or_default(),
            );
            events.push(LocalJobEvent {
                sequence: parsed.sequence.to_string(),
                job_id: record.id.clone(),
                generated_at: rfc3339(generated_ms),
                kind: kind.into(),
                state,
                progress: Some(parsed.progress),
            });
        }
        if events.len() >= MAX_PROGRESS_EVENTS {
            break;
        }
    }
    Ok(events)
}

fn parse_progress_line(line: &str, record: &JobRecord) -> Option<ParsedProgressEvent> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let sequence = value.get("sequence")?.as_u64()?;
    let event = value
        .get("event")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("progress")
        .to_string();
    let row: MultiwayMetricsRow = serde_json::from_value(value).ok()?;
    Some(ParsedProgressEvent {
        sequence,
        event,
        progress: progress_from_metrics(&row, record),
    })
}

fn strategy_snapshot(
    record: &JobRecord,
    solution: &Path,
    node_id: &str,
) -> CommandResult<StrategySnapshot> {
    let history = parse_node_id(node_id)?;
    let mut reader = formats::MwSolReader::open(solution).map_err(|error| {
        CommandError::new(
            "artifact_read_failed",
            format!("solution.mwsolを開けません: {error}"),
        )
    })?;
    let metadata = reader.metadata().clone();
    let state = metadata
        .public_states
        .binary_search_by_key(&history, |state| state.history)
        .ok()
        .and_then(|index| metadata.public_states.get(index))
        .ok_or_else(|| {
            CommandError::new(
                "node_not_found",
                format!("node {node_id} はsolutionに存在しません。"),
            )
        })?;
    let actor = state.actor.ok_or_else(|| {
        CommandError::new("terminal_node", "terminal nodeにはstrategyがありません。")
    })?;
    let mut blocks = Vec::new();
    let mut cursor = 0;
    let mut passed_history = false;
    while cursor < reader.strategy_count() && !passed_history {
        let page = reader.read_strategy_page(cursor, 512).map_err(|error| {
            CommandError::new(
                "artifact_read_failed",
                format!("strategy pageを読めません: {error}"),
            )
        })?;
        for block in page.strategies {
            if block.key.history == history && block.key.actor == actor {
                blocks.push(block);
            } else if block.key.history > history {
                passed_history = true;
                break;
            }
        }
        cursor = page.next_cursor.unwrap_or(reader.strategy_count());
    }
    let weights: BTreeMap<MultiwayStrategyKey, f64> = metadata
        .strategy_weights
        .iter()
        .map(|entry| (entry.key, entry.weight))
        .collect();
    let actions = state
        .legal_actions
        .iter()
        .map(typed_action)
        .collect::<Vec<_>>();
    let (kind, entries, coverage) = if state.street == 0 {
        let by_bucket: BTreeMap<u32, &MultiwayStrategyBlock> = blocks
            .iter()
            .filter(|block| block.key.street == 0)
            .map(|block| (block.key.bucket_path[0], block))
            .collect();
        let entries = (0..cards_class_count())
            .map(|bucket| match by_bucket.get(&(bucket as u32)) {
                Some(block) => StrategyEntryDto {
                    id: format!("preflop:{bucket}"),
                    label: hand_label(bucket),
                    status: "visited",
                    weight: finite_decimal(weights.get(&block.key).copied().unwrap_or(0.0)),
                    combo_count: Some(combo_count(bucket)),
                    bucket_path: None,
                    probability_u16: Some(quantize_u16(&block.probabilities)),
                    ev: None,
                },
                None => StrategyEntryDto {
                    id: format!("preflop:{bucket}"),
                    label: hand_label(bucket),
                    status: "unvisited",
                    weight: "0".into(),
                    combo_count: Some(combo_count(bucket)),
                    bucket_path: None,
                    probability_u16: None,
                    ev: None,
                },
            })
            .collect::<Vec<_>>();
        let coverage = entries
            .iter()
            .filter(|entry| entry.status == "visited")
            .count() as f64
            / cards_class_count() as f64;
        ("preflop-hand-classes".to_string(), entries, coverage)
    } else {
        let total = blocks.len();
        let entries = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| StrategyEntryDto {
                id: format!("bucket:{index}"),
                label: format!("bucket {}", block.key.bucket_path[state.street as usize]),
                status: "visited",
                weight: finite_decimal(weights.get(&block.key).copied().unwrap_or(0.0)),
                combo_count: None,
                bucket_path: Some(block.key.bucket_path.to_vec()),
                probability_u16: Some(quantize_u16(&block.probabilities)),
                ev: None,
            })
            .collect();
        (
            "postflop-buckets".to_string(),
            entries,
            usize::from(total > 0) as f64,
        )
    };
    let mut breadcrumb_cursor = HistoryKey::ROOT;
    let breadcrumb = metadata
        .resolve_history(history)
        .unwrap_or_default()
        .into_iter()
        .map(|action| {
            breadcrumb_cursor =
                breadcrumb_cursor.child(action.actor as usize, action.action_index as usize);
            BreadcrumbDto {
                node_id: hex(&breadcrumb_cursor.0),
                actor_seat: action.actor,
                action: typed_action_from_label(&action.action),
            }
        })
        .collect();
    let active_opponents = blocks
        .first()
        .map(|block| block.key.active_opponents)
        .unwrap_or_default();
    let generated_at = lock(&record.inner)
        .finished_ms
        .map(rfc3339)
        .unwrap_or_else(|| rfc3339(unix_ms()));
    Ok(StrategySnapshot {
        schema_version: 1,
        job_id: record.id.clone(),
        revision: metadata.sweeps.to_string(),
        status: "final".into(),
        strategy_kind: "linear-average",
        generated_at,
        as_of_sweeps: metadata.sweeps.to_string(),
        current_sweeps: metadata.sweeps.to_string(),
        node: StrategyNodeDto {
            node_id: node_id.to_ascii_lowercase(),
            actor_seat: actor,
            street: street_name(state.street).into(),
            pot_milli_bb: Some(state.pot_millibb.to_string()),
            active_opponents,
            breadcrumb,
        },
        actions,
        view: StrategyViewDto { kind, entries },
        approximate: metadata.approximate_profile,
        coverage: finite_decimal(coverage),
    })
}

fn typed_action(action: &MultiwayPublicAction) -> TypedActionDto {
    let (semantic, amount_milli_bb, all_in, full_raise) = match action {
        MultiwayPublicAction::Fold => ("fold", None, false, None),
        MultiwayPublicAction::Check => ("check", None, false, None),
        MultiwayPublicAction::Call {
            amount_millibb,
            all_in,
        } => ("call", Some(amount_millibb.to_string()), *all_in, None),
        MultiwayPublicAction::BetTo {
            amount_millibb,
            all_in,
            full_raise,
        } => (
            "bet-to",
            Some(amount_millibb.to_string()),
            *all_in,
            Some(*full_raise),
        ),
        MultiwayPublicAction::RaiseTo {
            amount_millibb,
            all_in,
            full_raise,
        } => (
            "raise-to",
            Some(amount_millibb.to_string()),
            *all_in,
            Some(*full_raise),
        ),
    };
    let label = action.label();
    TypedActionDto {
        id: label.clone(),
        semantic: semantic.into(),
        amount_milli_bb,
        all_in,
        full_raise,
        label,
        destination: "unknown",
        child_node_id: None,
    }
}

fn typed_action_from_label(label: &str) -> TypedActionDto {
    let mut parts = label.split(':');
    let semantic = parts.next().unwrap_or(label);
    let amount_milli_bb = parts
        .next()
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
        .map(ToOwned::to_owned);
    let all_in = label.split(':').any(|part| part == "all-in");
    TypedActionDto {
        id: label.into(),
        semantic: semantic.into(),
        amount_milli_bb,
        all_in,
        full_raise: None,
        label: label.into(),
        destination: "unknown",
        child_node_id: None,
    }
}

fn descriptor(path: Option<&Path>) -> ArtifactDescriptor {
    let metadata = path.and_then(|path| std::fs::metadata(path).ok());
    ArtifactDescriptor {
        available: metadata.is_some(),
        byte_length: metadata.map(|metadata| metadata.len().to_string()),
        sha256: None,
    }
}

fn read_terminal_state(path: &Path) -> Result<JobState> {
    let run =
        read_json_limited(path, MAX_RESULT_BYTES).map_err(|error| anyhow!("{}", error.message))?;
    run.get("status")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_terminal_state)
        .ok_or_else(|| anyhow!("run.json does not contain a supported terminal status"))
}

fn parse_terminal_state(value: &str) -> Option<JobState> {
    Some(match value {
        "target-reached" => JobState::TargetReached,
        "sweep-limit" => JobState::SweepLimit,
        "time-limit" => JobState::TimeLimit,
        "cancelled" => JobState::Cancelled,
        "resource-limit" => JobState::ResourceLimit,
        "failed" => JobState::Failed,
        _ => return None,
    })
}

fn mark_failed(
    backend: &BackendInner,
    record: &JobRecord,
    code: &str,
    message: String,
    events: &EventSink,
) {
    {
        let mut inner = lock(&record.inner);
        inner.state = JobState::Failed;
        inner.finished_ms = Some(unix_ms());
        inner.error = Some(JobErrorDto {
            code: code.into(),
            message,
            retryable: false,
        });
    }
    {
        let mut active = lock(&backend.active_job);
        if active.as_deref() == Some(record.id.as_str()) {
            *active = None;
        }
    }
    emit_event(events, record, "state", JobState::Failed, None);
}

fn emit_event(
    sink: &EventSink,
    record: &JobRecord,
    kind: &str,
    state: JobState,
    progress: Option<ProgressDto>,
) {
    let sequence = record
        .next_event_sequence
        .fetch_add(1, Ordering::Relaxed)
        .to_string();
    let event = LocalJobEvent {
        sequence,
        job_id: record.id.clone(),
        generated_at: rfc3339(unix_ms()),
        kind: kind.into(),
        state,
        progress,
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event)));
}

fn managed_paths(run_dir: PathBuf) -> JobPaths {
    JobPaths {
        run: Some(run_dir.join("run.json")),
        progress: Some(run_dir.join("progress.jsonl")),
        solution: Some(run_dir.join("solution.mwsol")),
        checkpoint: Some(run_dir.join("checkpoint.mwckpt")),
    }
}

fn create_run_directory(root: &Path) -> CommandResult<(String, PathBuf)> {
    let runs = root.join("runs");
    for _ in 0..16 {
        let id = random_id();
        let path = runs.join(&id);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok((id, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CommandError::new(
                    "run_directory_create_failed",
                    format!("run directoryを作成できません: {error}"),
                ));
            }
        }
    }
    Err(CommandError::new(
        "run_id_collision",
        "一意なrun IDを確保できませんでした。",
    ))
}

fn random_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

fn fingerprint(bytes: &[u8]) -> String {
    formats::config_hash_hex(&formats::config_hash(bytes))
}

fn hex(bytes: &[u8]) -> String {
    let mut rendered = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut rendered, "{byte:02x}");
    }
    rendered
}

fn parse_node_id(value: &str) -> CommandResult<[u8; 16]> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CommandError::new(
            "invalid_node_id",
            "nodeId must be exactly 32 hexadecimal characters.",
        ));
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| {
            CommandError::new(
                "invalid_node_id",
                "nodeId must be exactly 32 hexadecimal characters.",
            )
        })?;
    }
    Ok(bytes)
}

fn quantize_u16(probabilities: &[f32]) -> Vec<u16> {
    if probabilities.is_empty() {
        return Vec::new();
    }
    let sanitized = probabilities
        .iter()
        .map(|&probability| {
            if probability.is_finite() {
                f64::from(probability.max(0.0))
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    let sum = sanitized.iter().sum::<f64>();
    if sum <= 0.0 || !sum.is_finite() {
        return vec![0; probabilities.len()];
    }
    let mut scaled = sanitized
        .iter()
        .enumerate()
        .map(|(index, probability)| {
            let exact = *probability / sum * f64::from(u16::MAX);
            (index, exact.floor() as u64, exact.fract())
        })
        .collect::<Vec<_>>();
    let assigned = scaled.iter().map(|(_, value, _)| *value).sum::<u64>();
    let mut remaining = u64::from(u16::MAX).saturating_sub(assigned);
    scaled.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
    });
    for (_, value, _) in &mut scaled {
        if remaining == 0 {
            break;
        }
        *value += 1;
        remaining -= 1;
    }
    scaled.sort_by_key(|(index, _, _)| *index);
    scaled
        .into_iter()
        .map(|(_, value, _)| value.min(u64::from(u16::MAX)) as u16)
        .collect()
}

fn cards_class_count() -> usize {
    13 * 13
}

fn hand_label(index: usize) -> String {
    const RANKS: [char; 13] = [
        'A', 'K', 'Q', 'J', 'T', '9', '8', '7', '6', '5', '4', '3', '2',
    ];
    let row = index / 13;
    let column = index % 13;
    if row == column {
        format!("{}{}", RANKS[row], RANKS[column])
    } else if row < column {
        format!("{}{}s", RANKS[row], RANKS[column])
    } else {
        format!("{}{}o", RANKS[column], RANKS[row])
    }
}

fn combo_count(index: usize) -> u8 {
    let row = index / 13;
    let column = index % 13;
    if row == column {
        6
    } else if row < column {
        4
    } else {
        12
    }
}

fn street_name(street: u8) -> &'static str {
    match street {
        0 => "preflop",
        1 => "flop",
        2 => "turn",
        3 => "river",
        _ => "unknown",
    }
}

fn utility_name(utility: &UtilitySection) -> &'static str {
    match utility {
        UtilitySection::ChipEv => "bb",
        UtilitySection::Icm { .. } | UtilitySection::TournamentIcm { .. } => "prize",
    }
}

fn effective_config_for(record: &JobRecord) -> CommandResult<String> {
    if !record.config_toml.is_empty() {
        ensure_v1_toml(&record.config_toml, "effective config")?;
        return Ok(record.config_toml.clone());
    }
    if let Some(solution) = record
        .paths
        .solution
        .as_deref()
        .filter(|path| path.is_file())
    {
        let reader = formats::MwSolReader::open(solution).map_err(|error| {
            CommandError::new(
                "artifact_read_failed",
                format!("solution.mwsolを開けません: {error}"),
            )
        })?;
        let raw = reader.metadata().config_toml.clone();
        ensure_v1_toml(&raw, "solution.mwsol")?;
        return Ok(raw);
    }
    if let Some(checkpoint_path) = record
        .paths
        .checkpoint
        .as_deref()
        .filter(|path| path.is_file())
    {
        let checkpoint = multiway::checkpoint::MultiwayCheckpoint::load_unchecked(checkpoint_path)
            .map_err(|error| {
                CommandError::new(
                    "checkpoint_read_failed",
                    format!("checkpoint.mwckptを開けません: {error}"),
                )
            })?;
        let raw = checkpoint.config_toml.ok_or_else(|| {
            CommandError::new(
                "effective_config_unavailable",
                "checkpointにeffective configが含まれていません。",
            )
        })?;
        ensure_v1_toml(&raw, "checkpoint.mwckpt")?;
        return Ok(raw);
    }
    Err(CommandError::new(
        "effective_config_unavailable",
        "この成果物には閲覧可能なMultiway Preflop v1 effective configがありません。",
    ))
}

fn config_contract(raw: &str) -> Result<ConfigContract> {
    let config = cli::config::parse_solve_config(raw)?;
    if !matches!(config.game, GameSection::PreflopMultiway(_)) {
        bail!("effective config is not Multiway Preflop");
    }
    Ok(ConfigContract {
        max_sweeps: config.run.sweeps.unwrap_or(config.run.iterations),
        stop_target: finite_decimal(config.run.stop_dev_gain.unwrap_or_default()),
        stop_target_unit: utility_name(&config.utility).into(),
    })
}

fn ensure_v1_toml(raw: &str, artifact: &str) -> CommandResult<()> {
    match cli::multiway_v1::has_v1_schema(raw) {
        Ok(true) => Ok(()),
        Ok(false) => Err(CommandError::new(
            "unsupported_artifact",
            format!("{artifact}はMultiway Preflop v1成果物ではありません。"),
        )),
        Err(error) => Err(CommandError::new(
            "unsupported_artifact",
            format!("{artifact}のembedded configを検証できません: {error:#}"),
        )),
    }
}

fn ensure_supported_solution_version(version: u16) -> CommandResult<()> {
    if version == formats::MWSOL_FORMAT_VERSION {
        Ok(())
    } else {
        Err(CommandError::new(
            "unsupported_artifact",
            format!(
                "solution.mwsol format v{version}は未対応です（v{}が必要です）。",
                formats::MWSOL_FORMAT_VERSION
            ),
        ))
    }
}

fn ensure_v1_run(run: &serde_json::Value) -> CommandResult<()> {
    if run.get("schemaVersion").and_then(serde_json::Value::as_u64)
        != Some(u64::from(formats::MULTIWAY_SCHEMA_VERSION))
    {
        return Err(CommandError::new(
            "unsupported_artifact",
            "run.jsonのschemaVersionはGUI対応のv3ではありません。",
        ));
    }
    let schema = run
        .get("effectiveConfig")
        .and_then(|config| config.get("schema"))
        .and_then(serde_json::Value::as_str);
    if schema != Some(cli::multiway_v1::SCHEMA) {
        return Err(CommandError::new(
            "unsupported_artifact",
            "run.jsonのeffective configはMultiway Preflop v1ではありません。",
        ));
    }
    Ok(())
}

fn finite_decimal(value: f64) -> String {
    if !value.is_finite() {
        return "0".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let rendered = format!("{value:.17}");
    let trimmed = if rendered.contains('.') {
        rendered.trim_end_matches('0').trim_end_matches('.')
    } else {
        rendered.as_str()
    };
    if trimmed == "-0" {
        "0".into()
    } else {
        trimmed.into()
    }
}

fn decimal_seconds_to_millis(value: &str) -> u64 {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| (value * 1000.0).min(u64::MAX as f64) as u64)
        .unwrap_or_default()
}

fn parse_optional_u64(label: &str, value: Option<&str>) -> CommandResult<Option<u64>> {
    value
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                CommandError::new(
                    "invalid_resume_overrides",
                    format!("{label} must be an unsigned base-10 integer."),
                )
            })
        })
        .transpose()
}

fn parse_optional_usize(label: &str, value: Option<&str>) -> CommandResult<Option<usize>> {
    value
        .map(|value| {
            value
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    CommandError::new(
                        "invalid_resume_overrides",
                        format!("{label} must be a positive base-10 integer."),
                    )
                })
        })
        .transpose()
}

fn parse_optional_positive_f64(label: &str, value: Option<&str>) -> CommandResult<Option<f64>> {
    value
        .map(|value| {
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or_else(|| {
                    CommandError::new(
                        "invalid_resume_overrides",
                        format!("{label} must be a finite positive number."),
                    )
                })
        })
        .transpose()
}

fn canonical_file(path: &Path) -> CommandResult<PathBuf> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        CommandError::new(
            "file_not_found",
            format!("選択したファイルを開けません: {error}"),
        )
    })?;
    if !path.is_file() {
        return Err(CommandError::new(
            "not_a_file",
            "選択対象は通常ファイルである必要があります。",
        ));
    }
    Ok(path)
}

fn canonical_directory(path: &Path) -> CommandResult<PathBuf> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        CommandError::new(
            "directory_not_found",
            format!("選択したdirectoryを開けません: {error}"),
        )
    })?;
    if !path.is_dir() {
        return Err(CommandError::new(
            "not_a_directory",
            "runの選択対象はdirectoryである必要があります。",
        ));
    }
    Ok(path)
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("selected file")
        .to_string()
}

fn read_json_limited(path: &Path, limit: u64) -> CommandResult<serde_json::Value> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        CommandError::new(
            "artifact_read_failed",
            format!("成果物を開けません: {error}"),
        )
    })?;
    if metadata.len() > limit {
        return Err(CommandError::new(
            "artifact_too_large",
            format!(
                "JSON成果物が{} MiBの上限を超えています。",
                limit / 1024 / 1024
            ),
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        CommandError::new(
            "artifact_read_failed",
            format!("成果物を読めません: {error}"),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        CommandError::new("invalid_artifact", format!("成果物JSONが無効です: {error}"))
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .with_context(|| format!("creating temporary file in {}", directory.display()))?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| anyhow!(error.error))?;
    Ok(())
}

fn copy_atomic(source: &Path, destination: &Path) -> Result<()> {
    let directory = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut input = File::open(source)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    std::io::copy(&mut input, &mut temporary)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(destination)
        .map_err(|error| anyhow!(error.error))?;
    Ok(())
}

fn file_receipt(path: &Path) -> CommandResult<FileReceipt> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        CommandError::new(
            "artifact_stat_failed",
            format!("保存したファイルを確認できません: {error}"),
        )
    })?;
    let mut file = File::open(path).map_err(|error| {
        CommandError::new(
            "artifact_read_failed",
            format!("保存したファイルを検証できません: {error}"),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            CommandError::new(
                "artifact_read_failed",
                format!("保存したファイルを検証できません: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(FileReceipt {
        file_name: display_file_name(path),
        byte_length: metadata.len().to_string(),
        sha256: hex(&hasher.finalize()),
    })
}

fn file_modified_ms(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn rfc3339(unix_ms: u64) -> String {
    let seconds = unix_ms / 1000;
    let millis = unix_ms % 1000;
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_terminal(backend: &LocalBackend, id: &str) -> JobSnapshot {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            let snapshot = backend.get_job(id).unwrap();
            if snapshot.state.is_terminal() {
                return snapshot;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "real smoke solve did not terminate"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn real_solver_acceptance_lock() -> MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn live_node_request_expires_when_the_ui_stops_polling() {
        let history = HistoryKey::ROOT.child(2, 3);
        let request = LiveNodeRequest {
            history,
            expires_ms: 10_000,
        };
        assert_eq!(request.active_history_at(10_000), Some(history));
        assert_eq!(request.active_history_at(10_001), None);
    }

    #[test]
    fn fixed_point_probabilities_sum_exactly() {
        let encoded = quantize_u16(&[0.333_333_34, 0.333_333_34, 0.333_333_34]);
        assert_eq!(
            encoded.iter().map(|value| u64::from(*value)).sum::<u64>(),
            65_535
        );
        assert_eq!(encoded, vec![21_845, 21_845, 21_845]);
    }

    #[test]
    fn fixed_point_probabilities_normalize_and_handle_degenerate_input() {
        for probabilities in [
            vec![0.600_000_1, 0.600_000_1],
            vec![-1.0, 0.2, 0.8],
            vec![1.0e-9, 1.0, 1.0e9],
            (1..=64)
                .map(|index| (index * index) as f32 / 17.0)
                .collect(),
        ] {
            let encoded = quantize_u16(&probabilities);
            assert_eq!(encoded.len(), probabilities.len());
            assert_eq!(
                encoded.iter().map(|value| u64::from(*value)).sum::<u64>(),
                u64::from(u16::MAX)
            );
        }
        assert_eq!(quantize_u16(&[0.0, -1.0, f32::NAN]), vec![0, 0, 0]);
    }

    #[test]
    fn decimal_wire_values_never_use_exponents() {
        for value in [0.0, -0.0, 1.0e-9, -123.456, 1.0e20] {
            let rendered = finite_decimal(value);
            assert!(!rendered.contains(['e', 'E']));
            assert!(rendered.parse::<f64>().unwrap().is_finite());
        }
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_735_689_600_123), "2025-01-01T00:00:00.123Z");
    }

    #[test]
    fn validation_uses_real_v1_normalizer() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
                .into(),
            source_id: None,
        });
        assert!(result.valid, "{:?}", result.errors);
        assert_eq!(result.config_fingerprint.as_deref().map(str::len), Some(64));
        assert!(
            result
                .effective_config_toml
                .as_deref()
                .is_some_and(|toml| toml.contains("kind = \"ehs2-percentile\""))
        );
        let preflight = result.preflight.expect("valid config has preflight");
        assert_eq!(preflight.tree.recall_mode, "current-street");
        assert_eq!(preflight.memory.estimate_kind, "exact-dense");
        assert!(preflight.memory.fits_budget.is_some_and(|fits| fits));
        assert!(
            preflight
                .memory
                .solver_state_bytes
                .is_some_and(|bytes| bytes.parse::<u64>().unwrap() > 0)
        );
    }

    #[test]
    fn validation_expands_a_registered_mwtree_source_and_runs_preflight() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let script_path = directory.path().join("short-stack.mwtree");
        std::fs::write(
            &script_path,
            r#"
param open = 2.5x

preflop when unopened {
  replace raise [open, allin]
}

postflop when players >= 2 {
  checkdown
}
"#,
        )
        .unwrap();
        let loaded = backend.register_tree_source(script_path).unwrap();
        let tree = r#"[game.tree]
kind = "script"
source = "short-stack.mwtree"

[game.tree.params]
open = "2.2x"
"#;
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml").replacen(
            "[game.abstraction]",
            &format!("{tree}\n[game.abstraction]"),
            1,
        );

        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: Some(loaded.source_id),
        });

        assert!(result.valid, "{:?}", result.errors);
        let effective = result
            .effective_config_toml
            .expect("script config has an effective config");
        assert!(effective.contains("kind = \"standard\""));
        assert!(effective.contains("2.2x"));
        assert!(!effective.contains("short-stack.mwtree"));
        let preflight = result.preflight.expect("script tree has preflight");
        assert!(preflight.tree.decision_nodes.parse::<u64>().unwrap() > 0);
        assert!(preflight.memory.fits_budget.is_some_and(|fits| fits));
    }

    #[test]
    fn validation_builds_the_gui_default_tree_and_sizes_its_solver_state() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_default.toml")
            .replace("memory = \"auto\"", "memory = \"4GiB\"");
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(result.valid, "{:?}", result.errors);
        let preflight = result.preflight.expect("GUI default has preflight");
        assert!(preflight.tree.decision_nodes.parse::<u64>().unwrap() > 0);
        assert!(
            preflight
                .tree
                .terminal_edges
                .as_deref()
                .is_some_and(|value| value.parse::<u64>().unwrap() > 0)
        );
        assert!(
            preflight
                .tree
                .policy_columns
                .as_deref()
                .is_some_and(|value| value.parse::<u64>().unwrap() > 0)
        );
        assert!(
            preflight
                .tree
                .policy_slots
                .as_deref()
                .is_some_and(|value| value.parse::<u64>().unwrap() > 0)
        );
        assert!(
            preflight
                .memory
                .solver_state_bytes
                .as_deref()
                .is_some_and(|value| value.parse::<u64>().unwrap() > 0)
        );
        assert_eq!(preflight.memory.fits_budget, Some(true));
    }

    #[test]
    fn validation_accepts_the_gui_full_surface_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: include_str!(
                "../../../examples/preflop_multiway_v1_gui_full_surface.toml"
            )
            .into(),
            source_id: None,
        });

        assert!(result.valid, "{:?}", result.errors);
        let effective = result
            .effective_config_toml
            .expect("full GUI surface has an effective config");
        assert!(effective.contains("preflop_first_to_act = 4"));
        assert!(effective.contains("opponent_exploration = 0.125"));
        assert!(effective.contains("max_time = \"12h\""));
        assert!(effective.contains("probability_encoding = \"f32\""));
        assert!(result.preflight.is_some());
    }

    #[test]
    fn validation_accepts_the_main_tentative_recommendations() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        for raw in [
            include_str!(
                "../../../experiments/abstraction-optimization-2026-07-25/tentative-defaults/cash-6max-100bb-tentative-ehs2-k256-current-street-v1.toml"
            ),
            include_str!(
                "../../../experiments/abstraction-optimization-2026-07-25/tentative-defaults/tournament-6max-50bb-tentative-ehs2-k128-current-street-v1.toml"
            ),
        ] {
            let result = backend.validate_config(ValidateConfigRequest {
                config_toml: raw.into(),
                source_id: None,
            });
            assert!(result.valid, "{:?}", result.errors);
            let preflight = result.preflight.expect("recommendation has preflight");
            assert!(preflight.memory.fits_budget.is_some_and(|fits| fits));
            assert!(preflight.tree.decision_nodes.parse::<u64>().unwrap() > 0);
        }
    }

    #[test]
    fn validation_accepts_the_gui_postflop_tree_builder_rules() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let tree = r#"[game.tree]
kind = "standard"

[[game.tree.rules]]
priority = 300
street = "flop"
when = "players >= 2"
effect = "replace"
action = "bet"
sizes = ["50%pot", "allin"]

[[game.tree.rules]]
priority = 301
street = "flop"
when = "players >= 2"
effect = "replace"
action = "raise"
sizes = ["75%pot", "allin"]

[[game.tree.rules]]
priority = 302
street = "flop"
when = "aggressions >= 3"
effect = "remove"
action = "raise"

[[game.tree.rules]]
priority = 310
street = "turn"
when = "players >= 2"
effect = "replace"
action = "bet"
sizes = ["50%pot", "allin"]

[[game.tree.rules]]
priority = 311
street = "turn"
when = "players >= 2"
effect = "replace"
action = "raise"
sizes = ["75%pot", "allin"]

[[game.tree.rules]]
priority = 312
street = "turn"
when = "aggressions >= 3"
effect = "remove"
action = "raise"

[[game.tree.rules]]
priority = 320
street = "river"
when = "players >= 2"
effect = "replace"
action = "bet"
sizes = ["50%pot", "allin"]

[[game.tree.rules]]
priority = 321
street = "river"
when = "players >= 2"
effect = "replace"
action = "raise"
sizes = ["75%pot", "allin"]

[[game.tree.rules]]
priority = 322
street = "river"
when = "aggressions >= 3"
effect = "remove"
action = "raise"
"#;
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml").replacen(
            "[game.abstraction]",
            &format!("{tree}\n[game.abstraction]"),
            1,
        );
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(result.valid, "{:?}", result.errors);
        let preflight = result.preflight.expect("valid tree has preflight");
        assert!(preflight.tree.decision_nodes.parse::<u64>().unwrap() > 0);
        assert!(
            preflight
                .tree
                .terminal_edges
                .expect("complete tree has terminal edges")
                .parse::<u64>()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn validation_reports_exact_and_sampled_icm_preflight() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml");
        let exact = raw.replacen(
            "[solver]",
            "[economics]\nkind = \"tournament-icm\"\npayouts = [100, 60, 40]\n\n[solver]",
            1,
        );
        let exact_result = backend.validate_config(ValidateConfigRequest {
            config_toml: exact,
            source_id: None,
        });
        assert!(exact_result.valid, "{:?}", exact_result.errors);
        assert!(matches!(
            exact_result
                .preflight
                .expect("exact ICM preflight")
                .economics,
            ValidationEconomicsPreflight::TournamentIcm {
                ref field_players,
                ref paid_places,
                mode: "exact",
                samples: None,
                prepared_bytes: None,
                ..
            } if field_players == "3" && paid_places == "3"
        ));

        let outside = std::iter::repeat_n("20", 13).collect::<Vec<_>>().join(", ");
        let sampled = raw.replacen(
            "[solver]",
            &format!(
                "[economics]\nkind = \"tournament-icm\"\npayouts = [100, 60, 40]\noutside_field_bb = [{outside}]\nsamples = 100000\nseed = 17\n\n[solver]"
            ),
            1,
        );
        let sampled_result = backend.validate_config(ValidateConfigRequest {
            config_toml: sampled,
            source_id: None,
        });
        assert!(sampled_result.valid, "{:?}", sampled_result.errors);
        assert!(matches!(
            sampled_result
                .preflight
                .expect("sampled ICM preflight")
                .economics,
            ValidationEconomicsPreflight::TournamentIcm {
                ref field_players,
                ref paid_places,
                mode: "sampled",
                samples: Some(ref samples),
                seed: Some(ref seed),
                prepared_bytes: Some(ref prepared),
                fits_prepared_limit: Some(true),
                ..
            } if field_players == "16"
                && paid_places == "3"
                && samples == "100000"
                && seed == "17"
                && prepared == "6000000"
        ));
    }

    #[test]
    fn validation_rejects_sampled_icm_above_the_prepared_memory_limit() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml");
        let outside = std::iter::repeat_n("20", 13).collect::<Vec<_>>().join(", ");
        let oversized = raw.replacen(
            "[solver]",
            &format!(
                "[economics]\nkind = \"tournament-icm\"\npayouts = [100, 60, 40]\noutside_field_bb = [{outside}]\nsamples = 20000000\nseed = 0\n\n[solver]"
            ),
            1,
        );
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: oversized,
            source_id: None,
        });
        assert!(!result.valid);
        assert!(result.errors.iter().any(|error| {
            error.code == "icm_prepared_memory_exceeded" && error.path == "economics.samples"
        }));
        assert!(matches!(
            result
                .preflight
                .expect("resource error keeps preflight")
                .economics,
            ValidationEconomicsPreflight::TournamentIcm {
                fits_prepared_limit: Some(false),
                ..
            }
        ));
    }

    #[test]
    fn validation_rejects_a_dense_tree_above_the_memory_budget() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
            .replace("memory = \"64MiB\"", "memory = 1");
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.code == "memory_budget_exceeded")
        );
        let memory = result
            .preflight
            .expect("resource error keeps preflight")
            .memory;
        assert_eq!(memory.budget_mode, "explicit");
        assert_eq!(memory.budget_bytes, "1");
        assert_eq!(memory.fits_budget, Some(false));
        assert!(memory.headroom_bytes.is_none());
    }

    #[test]
    fn validation_uses_the_production_auto_memory_budget() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
            .replace("memory = \"64MiB\"", "memory = \"auto\"");
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(result.valid, "{:?}", result.errors);
        let memory = result.preflight.expect("valid config has preflight").memory;
        assert_eq!(memory.budget_mode, "auto");
        let budget = memory.budget_bytes.parse::<u64>().unwrap();
        assert_eq!(budget, 6 * 1024 * 1024 * 1024);
        let effective = result.effective_config_toml.unwrap();
        let config = cli::config::parse_solve_config(&effective).unwrap();
        assert_eq!(config.run.max_memory_bytes, Some(u64::MAX));
    }

    #[test]
    fn validation_resolves_auto_threads_to_the_effective_parallelism() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
            .replace("threads = 1", "threads = \"auto\"");
        let result = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(result.valid, "{:?}", result.errors);

        let expected = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(3);
        let preflight = result.preflight.expect("valid config has preflight");
        assert_eq!(preflight.threads, expected.to_string());
        let effective = result.effective_config_toml.unwrap();
        let config = cli::config::parse_solve_config(&effective).unwrap();
        assert_eq!(config.run.threads, Some(expected));
    }

    #[test]
    fn job_creation_cannot_bypass_memory_preflight() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
            .replace("memory = \"64MiB\"", "memory = 1");
        let effective = cli::multiway_v1::normalized_toml(&raw).unwrap();
        let error = backend
            .start_job(
                StartJobRequest {
                    name: "must-not-start".into(),
                    config_fingerprint: fingerprint(effective.as_bytes()),
                    effective_config_toml: effective,
                },
                Arc::new(|_| {}),
            )
            .unwrap_err();
        assert_eq!(error.code, "memory_budget_exceeded");
        assert!(backend.list_jobs().is_empty());
    }

    #[test]
    fn job_creation_cannot_bypass_icm_prepared_memory_preflight() {
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml");
        let outside = std::iter::repeat_n("20", 13).collect::<Vec<_>>().join(", ");
        let oversized = raw.replacen(
            "[solver]",
            &format!(
                "[economics]\nkind = \"tournament-icm\"\npayouts = [100, 60, 40]\noutside_field_bb = [{outside}]\nsamples = 20000000\nseed = 0\n\n[solver]"
            ),
            1,
        );
        let effective = cli::multiway_v1::normalized_toml(&oversized).unwrap();
        let error = backend
            .start_job(
                StartJobRequest {
                    name: "must-not-start-icm".into(),
                    config_fingerprint: fingerprint(effective.as_bytes()),
                    effective_config_toml: effective,
                },
                Arc::new(|_| {}),
            )
            .unwrap_err();
        assert_eq!(error.code, "icm_prepared_memory_exceeded");
        assert!(backend.list_jobs().is_empty());
    }

    #[test]
    fn start_job_wire_request_requires_a_fingerprint() {
        let error = serde_json::from_value::<StartJobRequest>(serde_json::json!({
            "name": "missing-fingerprint",
            "effectiveConfigToml": "schema = \"solvers.multiway-preflop/v1\"",
        }))
        .unwrap_err();
        assert!(error.to_string().contains("configFingerprint"));
    }

    #[test]
    fn run_directories_are_new_and_owned() {
        let directory = tempfile::tempdir().unwrap();
        let runs = directory.path().join("runs");
        std::fs::create_dir(&runs).unwrap();
        let (first_id, first) = create_run_directory(directory.path()).unwrap();
        let (second_id, second) = create_run_directory(directory.path()).unwrap();
        assert_ne!(first_id, second_id);
        assert!(first.is_dir());
        assert!(second.is_dir());
        assert_eq!(first.parent(), Some(runs.as_path()));
    }

    #[test]
    fn native_dtos_match_the_v3_camel_case_wire_contract() {
        let progress = ProgressDto {
            sweeps: "2".into(),
            max_sweeps: "10".into(),
            elapsed_secs: Some("0.25".into()),
            stop_target: "0.05".into(),
            stop_target_unit: "bb".into(),
            memory_bytes: Some("1024".into()),
            traversals_per_second: Some("12.5".into()),
            hand_updates_per_second: Some("6.25".into()),
            infosets: Some("9".into()),
            checkpoint: CheckpointProgressDto {
                available: true,
                generated_at: Some("2025-01-01T00:00:00.000Z".into()),
            },
            seats: vec![SeatMetricsDto {
                seat: 0,
                profile_ev: Some(EstimateDto {
                    mean: "0.1".into(),
                    stderr: "0.01".into(),
                    ci95: ["0.08".into(), "0.12".into()],
                }),
                average_positive_regret: Some("0.2".into()),
                strategy_drift_l1: Some("0.3".into()),
                deviation_gain: None,
            }],
        };
        assert_eq!(
            serde_json::to_value(&progress).unwrap(),
            serde_json::json!({
                "sweeps": "2",
                "maxSweeps": "10",
                "elapsedSecs": "0.25",
                "stopTarget": "0.05",
                "stopTargetUnit": "bb",
                "memoryBytes": "1024",
                "traversalsPerSecond": "12.5",
                "handUpdatesPerSecond": "6.25",
                "infosets": "9",
                "checkpoint": {
                    "available": true,
                    "generatedAt": "2025-01-01T00:00:00.000Z"
                },
                "seats": [{
                    "seat": 0,
                    "profileEv": {
                        "mean": "0.1",
                        "stderr": "0.01",
                        "ci95": ["0.08", "0.12"]
                    },
                    "averagePositiveRegret": "0.2",
                    "strategyDriftL1": "0.3",
                    "deviationGain": null
                }]
            })
        );

        let strategy = StrategySnapshot {
            schema_version: 1,
            job_id: "job".into(),
            revision: "2".into(),
            status: "live-average".into(),
            strategy_kind: "linear-average",
            generated_at: "2025-01-01T00:00:00.000Z".into(),
            as_of_sweeps: "2".into(),
            current_sweeps: "3".into(),
            node: StrategyNodeDto {
                node_id: ROOT_NODE_ID.into(),
                actor_seat: 0,
                street: "preflop".into(),
                pot_milli_bb: None,
                active_opponents: 2,
                breadcrumb: Vec::new(),
            },
            actions: vec![TypedActionDto {
                id: "fold".into(),
                semantic: "fold".into(),
                amount_milli_bb: None,
                all_in: false,
                full_raise: None,
                label: "fold".into(),
                destination: "terminal",
                child_node_id: None,
            }],
            view: StrategyViewDto {
                kind: "preflop-hand-classes".into(),
                entries: vec![StrategyEntryDto {
                    id: "preflop:0".into(),
                    label: "AA".into(),
                    status: "visited",
                    weight: "4.5".into(),
                    combo_count: Some(6),
                    bucket_path: None,
                    probability_u16: Some(vec![u16::MAX]),
                    ev: None,
                }],
            },
            approximate: true,
            coverage: "1".into(),
        };
        let serialized = serde_json::to_value(strategy).unwrap();
        assert_eq!(serialized["revision"], "2");
        assert_eq!(serialized["node"]["potMilliBb"], serde_json::Value::Null);
        assert_eq!(serialized["view"]["entries"][0]["weight"], "4.5");
        assert_eq!(serialized["coverage"], "1");
        assert!(serialized["actions"][0]["amountMilliBb"].is_null());

        let economics = ValidationEconomicsPreflight::TournamentIcm {
            field_players: "16".into(),
            paid_places: "3".into(),
            mode: "sampled",
            samples: Some("100000".into()),
            seed: Some("17".into()),
            prepared_bytes: Some("6000000".into()),
            prepared_limit_bytes: Some("1073741824".into()),
            fits_prepared_limit: Some(true),
        };
        assert_eq!(
            serde_json::to_value(economics).unwrap(),
            serde_json::json!({
                "kind": "tournament-icm",
                "fieldPlayers": "16",
                "paidPlaces": "3",
                "mode": "sampled",
                "samples": "100000",
                "seed": "17",
                "preparedBytes": "6000000",
                "preparedLimitBytes": "1073741824",
                "fitsPreparedLimit": true
            })
        );
    }

    #[test]
    #[ignore = "production EHS2 solve acceptance; run explicitly"]
    fn real_solver_writes_and_reopens_all_v1_artifacts() {
        let _serial = real_solver_acceptance_lock();
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
            .replace("memory = \"64MiB\"", "memory = \"auto\"");
        let validation = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(validation.valid, "{:?}", validation.errors);
        let effective = validation.effective_config_toml.unwrap();
        let job = backend
            .start_job(
                StartJobRequest {
                    name: "acceptance".into(),
                    effective_config_toml: effective,
                    config_fingerprint: validation
                        .config_fingerprint
                        .expect("valid config has a fingerprint"),
                },
                Arc::new(|_| {}),
            )
            .unwrap();

        let terminal = wait_for_terminal(&backend, &job.id);
        assert!(
            matches!(
                terminal.state,
                JobState::TargetReached | JobState::SweepLimit | JobState::TimeLimit
            ),
            "{:?}",
            terminal.error
        );

        let record = backend.job(&job.id).unwrap();
        for path in [
            record.paths.run.as_ref(),
            record.paths.progress.as_ref(),
            record.paths.checkpoint.as_ref(),
            record.paths.solution.as_ref(),
        ] {
            assert!(path.is_some_and(|path| path.is_file()));
        }
        backend.get_result(&job.id).unwrap();
        let strategy = backend.get_strategy(&job.id, Some(ROOT_NODE_ID)).unwrap();
        assert_eq!(strategy.status, "final");
        assert_eq!(strategy.node.node_id, ROOT_NODE_ID);

        let solution = record.paths.solution.clone().unwrap();
        let run_directory = record
            .paths
            .run
            .as_deref()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf();
        let imported_run = backend.open_run(run_directory).unwrap();
        assert_eq!(imported_run.state, terminal.state);
        let imported_run_result = backend.get_result(&imported_run.id).unwrap();
        assert_eq!(imported_run_result.terminal_status, terminal.state);
        let imported_run_strategy = backend
            .get_strategy(&imported_run.id, Some(ROOT_NODE_ID))
            .unwrap();
        assert_eq!(imported_run_strategy.revision, strategy.revision);

        let export_directory = directory.path().join("exports");
        std::fs::create_dir(&export_directory).unwrap();
        for artifact in [
            ArtifactKind::Run,
            ArtifactKind::Progress,
            ArtifactKind::Solution,
            ArtifactKind::Checkpoint,
        ] {
            let destination = export_directory.join(artifact.default_file_name());
            let receipt = backend
                .export_artifact_to(&job.id, artifact, &destination)
                .unwrap();
            assert_eq!(receipt.file_name, artifact.default_file_name());
            assert_eq!(receipt.sha256.len(), 64);
            assert!(destination.is_file());
            assert_eq!(
                receipt.byte_length,
                std::fs::metadata(destination).unwrap().len().to_string()
            );
        }

        let completed_sweeps = terminal
            .progress
            .as_ref()
            .unwrap()
            .sweeps
            .parse::<u64>()
            .unwrap();
        let resumed = backend
            .resume_job(
                &job.id,
                ResumeJobRequest {
                    name: Some("acceptance-resumed".into()),
                    overrides: ResumeOverrides {
                        max_sweeps: Some(completed_sweeps.saturating_add(1).to_string()),
                        stop_target: None,
                        threads: Some("1".into()),
                        memory: Some("1GiB".into()),
                        ..Default::default()
                    },
                },
                Arc::new(|_| {}),
            )
            .unwrap();
        assert_eq!(
            resumed.resumed_from_job_id.as_deref(),
            Some(job.id.as_str())
        );
        let resumed_terminal = wait_for_terminal(&backend, &resumed.id);
        assert!(resumed_terminal.state.is_terminal());
        assert!(resumed_terminal.artifacts.run.available);
        assert!(resumed_terminal.artifacts.progress.available);
        assert!(resumed_terminal.artifacts.checkpoint.available);
        assert!(resumed_terminal.artifacts.solution.available);
        backend.get_result(&resumed.id).unwrap();
        backend
            .get_strategy(&resumed.id, Some(ROOT_NODE_ID))
            .unwrap();

        let imported = backend.open_solution(solution).unwrap();
        let imported_progress = imported.progress.as_ref().unwrap();
        assert_eq!(imported_progress.sweeps, strategy.revision);
        assert_eq!(
            imported_progress.seats.len(),
            terminal.progress.as_ref().unwrap().seats.len()
        );
        assert!(imported_progress.elapsed_secs.is_none());
        assert!(imported_progress.memory_bytes.is_none());
        let imported_result = backend.get_result(&imported.id).unwrap();
        assert_eq!(
            imported_result.job.progress.as_ref().unwrap().sweeps,
            imported_progress.sweeps
        );
        let reopened = backend
            .get_strategy(&imported.id, Some(ROOT_NODE_ID))
            .unwrap();
        assert_eq!(reopened.status, "final");
        assert_eq!(reopened.revision, strategy.revision);
    }

    #[test]
    #[ignore = "production EHS2 solve acceptance; run explicitly"]
    fn real_solver_completes_a_sampled_icm_job() {
        let _serial = real_solver_acceptance_lock();
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = format!(
            "{}\n[economics]\nkind = \"tournament-icm\"\n\
             payouts = [100, 60, 40]\n\
             outside_field_bb = [{}]\n\
             samples = 2\nseed = 17\n",
            include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml"),
            vec!["2"; 13].join(", ")
        );
        let validation = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(validation.valid, "{:?}", validation.errors);
        assert!(matches!(
            validation.preflight.as_ref().map(|value| &value.economics),
            Some(ValidationEconomicsPreflight::TournamentIcm {
                mode: "sampled",
                ..
            })
        ));
        let job = backend
            .start_job(
                StartJobRequest {
                    name: "sampled-icm".into(),
                    effective_config_toml: validation.effective_config_toml.unwrap(),
                    config_fingerprint: validation.config_fingerprint.unwrap(),
                },
                Arc::new(|_| {}),
            )
            .unwrap();
        let terminal = wait_for_terminal(&backend, &job.id);

        assert!(
            matches!(
                terminal.state,
                JobState::TargetReached | JobState::SweepLimit | JobState::TimeLimit
            ),
            "{:?}",
            terminal.error
        );
        assert!(terminal.artifacts.run.available);
        assert!(terminal.artifacts.progress.available);
        assert!(terminal.artifacts.checkpoint.available);
        assert!(terminal.artifacts.solution.available);
        backend.get_result(&job.id).unwrap();
    }

    #[test]
    #[ignore = "production EHS2 solve acceptance; run explicitly"]
    fn local_cancel_stops_the_real_worker_and_keeps_a_resumable_checkpoint() {
        let _serial = real_solver_acceptance_lock();
        let directory = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(directory.path().to_path_buf()).unwrap();
        let raw = include_str!("../../../examples/preflop_multiway_v1_gui_smoke.toml")
            .replace("max_sweeps = 2", "max_sweeps = 1000000")
            .replace("target = 1000000.0", "target = 0.000000001");
        let validation = backend.validate_config(ValidateConfigRequest {
            config_toml: raw,
            source_id: None,
        });
        assert!(validation.valid, "{:?}", validation.errors);
        let job = backend
            .start_job(
                StartJobRequest {
                    name: "cancel-acceptance".into(),
                    effective_config_toml: validation.effective_config_toml.unwrap(),
                    config_fingerprint: validation
                        .config_fingerprint
                        .expect("valid config has a fingerprint"),
                },
                Arc::new(|_| {}),
            )
            .unwrap();
        let events: EventSink = Arc::new(|_| {});
        backend.cancel_job(&job.id, &events).unwrap();
        let terminal = wait_for_terminal(&backend, &job.id);

        assert_eq!(terminal.state, JobState::Cancelled);
        assert!(terminal.artifacts.run.available);
        assert!(terminal.artifacts.progress.available);
        assert!(terminal.artifacts.checkpoint.available);
        assert!(!terminal.artifacts.solution.available);
        assert!(terminal.resume_available);

        let resume_error = backend
            .resume_job(
                &job.id,
                ResumeJobRequest {
                    name: Some("too-small".into()),
                    overrides: ResumeOverrides {
                        memory: Some("1KiB".into()),
                        ..ResumeOverrides::default()
                    },
                },
                Arc::clone(&events),
            )
            .unwrap_err();
        assert_eq!(resume_error.code, "memory_budget_exceeded");
        assert_eq!(backend.list_jobs().len(), 1);
    }
}
