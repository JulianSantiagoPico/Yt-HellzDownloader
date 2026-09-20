pub mod audio;
pub mod commands;
pub mod domain;
pub mod error;
pub mod events;
pub mod filesystem;
pub mod persistence;
pub mod processes;
pub mod scheduler;
pub mod updater;
pub mod youtube;

use std::sync::Arc;

use scheduler::DownloadScheduler;
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Mutex;
use tauri::{Manager, State};

use processes::ProcessRegistry;

pub struct AppState {
    pub pool: SqlitePool,
    pub data_directory: String,
    pub processes: ProcessRegistry,
    pub initial_recovery_report: Mutex<persistence::RecoveryReport>,
    pub scheduler: Arc<DownloadScheduler>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthStatus {
    database: &'static str,
    data_directory: String,
}

#[tauri::command]
async fn health_check(state: State<'_, AppState>) -> Result<HealthStatus, String> {
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|error| error.to_string())?;
    Ok(HealthStatus {
        database: "ready",
        data_directory: state.data_directory.clone(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let pool =
                tauri::async_runtime::block_on(persistence::connect(&data_dir.join("app.db")))?;

            // Ejecución obligatoria de recuperación de cierres inesperados al inicio
            let recovery_report =
                tauri::async_runtime::block_on(persistence::recover_on_startup(&pool))
                    .unwrap_or_default();

            // Crear scheduler
            let resource_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_default();
            let settings = tauri::async_runtime::block_on(persistence::repositories::settings::get_settings(&pool))
                .unwrap_or_default();
            let max_downloads = settings.max_concurrent_downloads.max(1) as usize;
            let max_conversions = settings.max_concurrent_conversions.max(1) as usize;

            let scheduler = Arc::new(DownloadScheduler::new(
                pool.clone(),
                resource_dir,
                max_downloads,
                max_conversions,
            ));

            // Arrancar el loop del scheduler en background
            let scheduler_clone = scheduler.clone();
            tauri::async_runtime::spawn(async move {
                scheduler_clone.run().await;
            });

            app.manage(AppState {
                pool,
                data_directory: data_dir.display().to_string(),
                processes: ProcessRegistry::default(),
                initial_recovery_report: Mutex::new(recovery_report),
                scheduler,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            // Job lifecycle commands
            commands::create_job,
            commands::extract_and_enqueue_items,
            commands::pause_job,
            commands::resume_job,
            commands::cancel_job,
            commands::retry_failed_items,
            commands::set_job_priority,
            commands::get_job_progress,
            commands::get_queue_status,
            // Playlist commands
            commands::list_playlists,
            commands::get_playlist_details,
            commands::get_local_files,
            commands::resolve_file_conflicts,
            // Diagnostic commands
            commands::export_diagnostic_logs,
            // Existing commands
            commands::get_default_output_directory,
            commands::get_ytdlp_version,
            commands::check_ytdlp_updates,
            commands::update_ytdlp,
            commands::extract_playlist,
            commands::get_recovery_report,
            commands::get_app_settings,
            commands::update_app_settings,
            commands::list_jobs,
            commands::get_job_details,
            commands::list_job_items,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
