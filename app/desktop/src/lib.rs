mod local_backend;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use local_backend::{
    ArtifactKind, CheckpointSource, CommandError, EventSink, FileReceipt, JobResult, JobSnapshot,
    LoadedTreeSource, LocalBackend, LocalCapabilities, ProgressEventPage, ResumeJobRequest,
    ResumeOverrides, StrategySnapshot, ValidateConfigRequest, ValidationResult,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

#[tauri::command]
fn local_capabilities(backend: State<'_, LocalBackend>) -> LocalCapabilities {
    backend.capabilities()
}

#[tauri::command]
async fn local_validate_config(
    backend: State<'_, LocalBackend>,
    config_toml: String,
    source_id: Option<String>,
) -> Result<ValidationResult, CommandError> {
    let backend = backend.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        backend.validate_config(ValidateConfigRequest {
            config_toml,
            source_id,
        })
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "validation_worker_failed",
            format!("設定検証workerを完了できませんでした: {error}"),
        )
    })
}

#[tauri::command]
async fn local_start_job(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
    name: String,
    effective_config_toml: String,
    config_fingerprint: String,
) -> Result<JobSnapshot, CommandError> {
    let backend = backend.inner().clone();
    let events = event_sink(app);
    tauri::async_runtime::spawn_blocking(move || {
        backend.start_job(
            local_backend::StartJobRequest {
                name,
                effective_config_toml,
                config_fingerprint,
            },
            events,
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "start_worker_failed",
            format!("Solve開始workerを完了できませんでした: {error}"),
        )
    })?
}

#[tauri::command]
fn local_list_jobs(backend: State<'_, LocalBackend>) -> Vec<JobSnapshot> {
    backend.list_jobs()
}

#[tauri::command]
fn local_get_job(
    backend: State<'_, LocalBackend>,
    job_id: String,
) -> Result<JobSnapshot, CommandError> {
    backend.get_job(&job_id)
}

#[tauri::command]
fn local_get_progress(
    backend: State<'_, LocalBackend>,
    job_id: String,
    after_sequence: Option<String>,
) -> Result<ProgressEventPage, CommandError> {
    backend.get_progress(&job_id, after_sequence.as_deref())
}

#[tauri::command]
fn local_cancel_job(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
    job_id: String,
) -> Result<JobSnapshot, CommandError> {
    backend.cancel_job(&job_id, &event_sink(app))
}

#[tauri::command]
fn local_get_result(
    backend: State<'_, LocalBackend>,
    job_id: String,
) -> Result<JobResult, CommandError> {
    backend.get_result(&job_id)
}

#[tauri::command]
fn local_get_strategy(
    backend: State<'_, LocalBackend>,
    job_id: String,
    node_id: Option<String>,
) -> Result<StrategySnapshot, CommandError> {
    backend.get_strategy(&job_id, node_id.as_deref())
}

#[tauri::command]
async fn local_resume_job(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
    job_id: String,
    name: Option<String>,
    overrides: Option<ResumeOverrides>,
) -> Result<JobSnapshot, CommandError> {
    let backend = backend.inner().clone();
    let events = event_sink(app);
    tauri::async_runtime::spawn_blocking(move || {
        backend.resume_job(
            &job_id,
            ResumeJobRequest {
                name,
                overrides: overrides.unwrap_or_default(),
            },
            events,
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "resume_worker_failed",
            format!("Solve再開workerを完了できませんでした: {error}"),
        )
    })?
}

#[tauri::command]
async fn local_resume_checkpoint(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
    source_id: String,
    name: Option<String>,
    overrides: Option<ResumeOverrides>,
) -> Result<JobSnapshot, CommandError> {
    let backend = backend.inner().clone();
    let events = event_sink(app);
    tauri::async_runtime::spawn_blocking(move || {
        backend.resume_checkpoint(
            &source_id,
            ResumeJobRequest {
                name,
                overrides: overrides.unwrap_or_default(),
            },
            events,
        )
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "resume_worker_failed",
            format!("checkpoint再開workerを完了できませんでした: {error}"),
        )
    })?
}

#[tauri::command]
async fn local_pick_config(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
) -> Result<Option<local_backend::LoadedConfig>, CommandError> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Solve config", &["toml"])
        .blocking_pick_file();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.register_config_source(path))
        .transpose()
}

#[tauri::command]
async fn local_pick_tree_script(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
) -> Result<Option<LoadedTreeSource>, CommandError> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Multiway tree script", &["mwtree"])
        .blocking_pick_file();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.register_tree_source(path))
        .transpose()
}

#[tauri::command]
async fn local_save_config_dialog(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
    config_toml: String,
    suggested_name: Option<String>,
) -> Result<Option<FileReceipt>, CommandError> {
    let file_name = safe_suggested_name(suggested_name.as_deref(), "solve.toml");
    let selected = app
        .dialog()
        .file()
        .add_filter("Solve config", &["toml"])
        .set_file_name(file_name)
        .blocking_save_file();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.save_config_to(&path, &config_toml))
        .transpose()
}

#[tauri::command]
async fn local_pick_run(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
) -> Result<Option<JobSnapshot>, CommandError> {
    let selected = app.dialog().file().blocking_pick_folder();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.open_run(path))
        .transpose()
}

#[tauri::command]
async fn local_pick_solution(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
) -> Result<Option<JobSnapshot>, CommandError> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Multiway solution", &["mwsol"])
        .blocking_pick_file();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.open_solution(path))
        .transpose()
}

#[tauri::command]
async fn local_pick_checkpoint(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
) -> Result<Option<CheckpointSource>, CommandError> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Multiway checkpoint", &["mwckpt"])
        .blocking_pick_file();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.register_checkpoint(path))
        .transpose()
}

#[tauri::command]
async fn local_export_artifact_dialog(
    app: AppHandle,
    backend: State<'_, LocalBackend>,
    job_id: String,
    artifact: ArtifactKind,
) -> Result<Option<FileReceipt>, CommandError> {
    let selected = app
        .dialog()
        .file()
        .set_file_name(artifact.default_file_name())
        .blocking_save_file();
    selected
        .map(file_path)
        .transpose()?
        .map(|path| backend.export_artifact_to(&job_id, artifact, &path))
        .transpose()
}

fn event_sink(app: AppHandle) -> EventSink {
    Arc::new(move |event| {
        let _ = app.emit("local-job-event", event);
    })
}

fn file_path(path: FilePath) -> Result<PathBuf, CommandError> {
    path.into_path().map_err(|error| {
        CommandError::new(
            "unsupported_file_location",
            format!("選択した場所をlocal filesystem pathとして扱えません: {error}"),
        )
    })
}

fn safe_suggested_name(value: Option<&str>, fallback: &str) -> String {
    value
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_local_data_dir()?.join("local-solver");
            app.manage(LocalBackend::new(root)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            local_capabilities,
            local_validate_config,
            local_start_job,
            local_list_jobs,
            local_get_job,
            local_get_progress,
            local_cancel_job,
            local_get_result,
            local_get_strategy,
            local_resume_job,
            local_resume_checkpoint,
            local_pick_config,
            local_pick_tree_script,
            local_save_config_dialog,
            local_pick_run,
            local_pick_solution,
            local_pick_checkpoint,
            local_export_artifact_dialog,
        ])
        .run(tauri::generate_context!())
        .expect("running Solvers Strategy Studio");
}
