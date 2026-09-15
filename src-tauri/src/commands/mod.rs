use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    audio,
    filesystem::{self, ExistingFilePolicy},
    updater::{self, ReleaseEntry, UpdateResult, YtdlpVersionInfo},
    youtube::{self, PlaylistInfo, VideoMetadata},
    AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSingleRequest {
    pub client_run_id: Option<String>,
    pub url: String,
    pub output_dir: Option<String>,
    pub collision_policy: Option<ExistingFilePolicy>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    pub run_id: String,
    pub success: bool,
    pub file_path: String,
    pub title: String,
    pub artist: String,
    pub duration_seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageProgress {
    pub run_id: String,
    pub stage: String,
    pub percent: f32,
    pub message: String,
}

fn get_default_music_dir() -> PathBuf {
    if let Some(user_dir) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        return user_dir.join("Music").join("YT Downloads");
    }
    PathBuf::from("C:\\YT Downloads")
}

#[tauri::command]
pub async fn get_default_output_directory() -> Result<String, String> {
    let dir = get_default_music_dir();
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.display().to_string())
}

#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, run_id: String) -> Result<bool, String> {
    // Validar formato UUID estricto para evitar ataques de inyección o rutas arbitrarias
    let validated_id = Uuid::parse_str(run_id.trim())
        .map_err(|_| "Identificador de ejecución inválido".to_string())?;

    Ok(state.processes.cancel(&validated_id.to_string()).await)
}

#[tauri::command]
pub async fn download_single_track(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DownloadSingleRequest,
) -> Result<DownloadResult, String> {
    // 1. Validar o generar un UUID v4 seguro en Rust
    let run_id = match &request.client_run_id {
        Some(client_id) if !client_id.trim().is_empty() => Uuid::parse_str(client_id.trim())
            .map_err(|_| "clientRunId debe ser un UUID v4 válido".to_string())?,
        _ => Uuid::new_v4(),
    };

    let run_id_str = run_id.to_string();

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Error de ruta de recursos: {}", e))?;

    let destination_dir = match &request.output_dir {
        Some(custom) if !custom.trim().is_empty() => PathBuf::from(custom.trim()),
        _ => get_default_music_dir(),
    };

    std::fs::create_dir_all(&destination_dir)
        .map_err(|e| format!("No se pudo acceder o crear la carpeta de destino: {}", e))?;

    let fallback_base_temp = PathBuf::from(&state.data_directory).join("temp");
    let (item_temp_dir, _is_same_volume) =
        filesystem::prepare_item_temp_dir(&destination_dir, &fallback_base_temp, &run_id)?;

    let token = CancellationToken::new();
    state
        .processes
        .insert_token(run_id_str.clone(), token.clone())
        .await?;

    let policy = request.collision_policy.unwrap_or_default();

    let ctx = PipelineContext {
        resource_dir: &resource_dir,
        destination_dir: &destination_dir,
        item_temp_dir: &item_temp_dir,
        raw_url: &request.url,
        run_id: &run_id_str,
        policy,
    };

    let res = execute_pipeline(&app, &ctx, &token).await;

    // Limpieza estricta del directorio temporal del ítem garantizando que se remueve
    let _ = filesystem::clean_item_temp_dir(&item_temp_dir);
    state.processes.remove_token(&run_id_str).await;

    res
}

struct PipelineContext<'a> {
    resource_dir: &'a Path,
    destination_dir: &'a Path,
    item_temp_dir: &'a Path,
    raw_url: &'a str,
    run_id: &'a str,
    policy: ExistingFilePolicy,
}

async fn execute_pipeline(
    app: &AppHandle,
    ctx: &PipelineContext<'_>,
    token: &CancellationToken,
) -> Result<DownloadResult, String> {
    let run_id = ctx.run_id;

    // 1. Validar URL y extraer metadata
    emit_stage(
        app,
        run_id,
        "extracting",
        5.0,
        "Validando URL y extrayendo metadatos…",
    );
    let metadata: VideoMetadata =
        youtube::extract_metadata(ctx.resource_dir, ctx.raw_url, token).await?;

    // 2. Comprobar colisión de archivos en destino ANTES de descargar
    let final_dest = filesystem::resolve_destination_path(
        ctx.destination_dir,
        &metadata.title,
        None,
        ctx.policy,
    )?;

    // 3. Descargar flujo de audio original y portada
    emit_stage(
        app,
        run_id,
        "downloading",
        15.0,
        "Descargando flujo de audio original…",
    );
    let app_clone = app.clone();
    let run_id_clone = run_id.to_string();
    let raw_audio = youtube::download_audio_stream(
        ctx.resource_dir,
        ctx.raw_url,
        ctx.item_temp_dir,
        token,
        move |percent, _line| {
            let mapped = 15.0 + (percent * 0.45); // 15% a 60%
            let _ = app_clone.emit(
                "download-progress",
                StageProgress {
                    run_id: run_id_clone.clone(),
                    stage: "downloading".into(),
                    percent: mapped,
                    message: format!("Descargando audio… {:.1}%", percent),
                },
            );
        },
    )
    .await?;

    // 4. Recodificar a MP3 192 kbps estéreo (-ac 2)
    emit_stage(
        app,
        run_id,
        "converting",
        65.0,
        "Recodificando a MP3 192 kbps estéreo…",
    );
    let converted_mp3 = ctx.item_temp_dir.join("converted.mp3");
    audio::convert_to_mp3(ctx.resource_dir, &raw_audio, &converted_mp3, token).await?;

    // 5. Etiquetar con ID3v2.3 y verificar legibilidad
    emit_stage(
        app,
        run_id,
        "tagging",
        80.0,
        "Escribiendo etiquetas ID3 y carátula…",
    );
    let cover_path = ctx.item_temp_dir.join("cover.jpg");
    let cover_ref = if cover_path.exists() {
        Some(cover_path.as_path())
    } else {
        None
    };
    audio::tag_mp3(&converted_mp3, &metadata, cover_ref, None)?;

    // 6. Validar con ffprobe (códec mp3, 2 canales estéreo, duración válida)
    emit_stage(
        app,
        run_id,
        "validating",
        90.0,
        "Validando integridad y perfil con ffprobe…",
    );
    let validation = audio::validate_mp3(ctx.resource_dir, &converted_mp3, token).await?;

    // 7. Mover al destino final con política de colisión
    let allow_overwrite = ctx.policy == ExistingFilePolicy::Overwrite;
    filesystem::move_file_safely(&converted_mp3, &final_dest, allow_overwrite)?;

    emit_stage(
        app,
        run_id,
        "completed",
        100.0,
        "Descarga completada y verificada.",
    );

    Ok(DownloadResult {
        run_id: run_id.to_string(),
        success: true,
        file_path: final_dest.display().to_string(),
        title: metadata.title,
        artist: metadata.artist,
        duration_seconds: validation.duration_seconds,
    })
}

fn emit_stage(app: &AppHandle, run_id: &str, stage: &str, percent: f32, message: &str) {
    let _ = app.emit(
        "download-progress",
        StageProgress {
            run_id: run_id.to_string(),
            stage: stage.to_string(),
            percent,
            message: message.to_string(),
        },
    );
}

#[tauri::command]
pub async fn get_ytdlp_version(app: AppHandle) -> Result<YtdlpVersionInfo, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Error de ruta de recursos: {}", e))?;
    updater::get_current_version(&resource_dir).await
}

#[tauri::command]
pub async fn check_ytdlp_updates(
    app: AppHandle,
    manifest_url: String,
) -> Result<Vec<ReleaseEntry>, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Error de ruta de recursos: {}", e))?;
    let token = CancellationToken::new();
    updater::check_for_updates(&resource_dir, &manifest_url, &token).await
}

#[tauri::command]
pub async fn update_ytdlp(app: AppHandle, release: ReleaseEntry) -> Result<UpdateResult, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Error de ruta de recursos: {}", e))?;
    let token = CancellationToken::new();
    updater::update_binary(&resource_dir, &release, &token).await
}

#[tauri::command]
pub async fn extract_playlist(app: AppHandle, url: String) -> Result<PlaylistInfo, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Error de ruta de recursos: {}", e))?;
    let token = CancellationToken::new();
    youtube::extract_playlist(&resource_dir, &url, &token).await
}
