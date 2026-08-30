mod persistence;
mod processes;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{Manager, State};

use processes::{ProcessRegistry, RunToolRequest};

struct AppState {
    pool: SqlitePool,
    data_directory: String,
    processes: ProcessRegistry,
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

#[tauri::command]
async fn run_tool(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: RunToolRequest,
) -> Result<i32, String> {
    processes::run(app, state.processes.clone(), request).await
}

#[tauri::command]
async fn cancel_tool(state: State<'_, AppState>, run_id: String) -> Result<bool, String> {
    Ok(processes::cancel(&state.processes, &run_id).await)
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
            run_tool,
            cancel_tool
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
