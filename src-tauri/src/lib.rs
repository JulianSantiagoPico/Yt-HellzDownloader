pub mod audio;
pub mod commands;
pub mod filesystem;
pub mod persistence;
pub mod processes;
pub mod updater;
pub mod youtube;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{Manager, State};

use processes::ProcessRegistry;

pub struct AppState {
    pub pool: SqlitePool,
    pub data_directory: String,
    pub processes: ProcessRegistry,
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
            app.manage(AppState {
                pool,
                data_directory: data_dir.display().to_string(),
                processes: ProcessRegistry::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            commands::download_single_track,
            commands::cancel_download,
            commands::get_default_output_directory,
            commands::get_ytdlp_version,
            commands::check_ytdlp_updates,
            commands::update_ytdlp,
            commands::extract_playlist,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
