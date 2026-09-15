pub mod types;

use std::path::PathBuf;
use std::str::FromStr;

use chrono::Utc;
use sqlx::Row;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::commands::types::{
    CurrentItem, ExportResult, ExtractionResult, JobProgress, PlaylistDetails, PlaylistSummary,
    PlaylistTrackWithStatus, QueueEntry,
};
use crate::domain::entities::{
    AppSettings, Job, JobItem, LocalFile, Playlist, PlaylistTrack, Track,
};
use crate::domain::states::{JobItemStatus, JobKind, JobStatus, OrganizationMode, SourceKind};
use crate::error::CommandError;
use crate::events::{emit_event, AppEvent};
use crate::persistence::repositories::jobs::{self, transition_job_status};
use crate::persistence::repositories::{self as repo, settings};
use crate::persistence::repositories::{
    add_playlist_track, get_active_playlist_tracks, get_playlist as repo_get_playlist,
    get_playlist_by_youtube_id, list_playlists as repo_list_playlists, upsert_playlist,
    upsert_track,
};
use crate::youtube;
use crate::{filesystem::ExistingFilePolicy, AppState};

// ============================================================================
// COMANDOS DE JOB
// ============================================================================

/// Crea un nuevo Job para procesar una playlist.
#[tauri::command]
pub async fn create_job(
    app: AppHandle,
    state: State<'_, AppState>,
    source_url: String,
    output_directory: Option<String>,
    organization_mode: Option<String>,
    format_profile: Option<String>,
    existing_file_policy: Option<String>,
) -> Result<Job, CommandError> {
    // 1. Validar URL
    let validated = youtube::validate_youtube_url(&source_url)
        .map_err(|msg| CommandError::Validation { message: msg })?;

    let now = Utc::now().to_rfc3339();

    // 2. Obtener directorio de salida (el proporcionado o el de settings)
    let output_dir = match output_directory {
        Some(dir) if !dir.trim().is_empty() => dir,
        _ => {
            let app_settings = settings::get_settings(&state.pool).await?;
            if app_settings.default_output_directory.is_empty() {
                return Err(CommandError::Validation {
                    message: "No se ha configurado un directorio de salida".into(),
                });
            }
            app_settings.default_output_directory
        }
    };

    // 3. Verificar si ya existe la playlist y determinar el tipo de Job
    let kind = if let Some(ref playlist_id) = validated.playlist_id {
        if get_playlist_by_youtube_id(&state.pool, playlist_id)
            .await?
            .is_some()
        {
            JobKind::Sync
        } else {
            JobKind::Import
        }
    } else {
        JobKind::Import
    };

    // 4. Crear registro de playlist si es necesario
    if let Some(ref playlist_id) = validated.playlist_id {
        let existing = get_playlist_by_youtube_id(&state.pool, playlist_id).await?;
        if existing.is_none() {
            let source_kind = if source_url.contains("music.youtube.com") {
                SourceKind::YouTubeMusic
            } else {
                SourceKind::YouTube
            };

            let new_playlist = Playlist {
                id: Uuid::new_v4().to_string(),
                youtube_playlist_id: playlist_id.clone(),
                source_url: validated.canonical_url.clone(),
                source_kind,
                title: String::new(),
                channel: String::new(),
                thumbnail_url: None,
                default_output_directory: None,
                last_synced_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            upsert_playlist(&state.pool, &new_playlist).await?;
        }
    }

    // 5. Parsear organization mode y existing file policy
    let org_mode = organization_mode
        .and_then(|s| OrganizationMode::from_str(&s).ok())
        .unwrap_or(OrganizationMode::PlaylistFolder);

    let file_policy = existing_file_policy
        .map(|s| match s.as_str() {
            "reuse" => ExistingFilePolicy::Reuse,
            "overwrite" => ExistingFilePolicy::Overwrite,
            "rename" => ExistingFilePolicy::Rename,
            "fail_if_exists" => ExistingFilePolicy::FailIfExists,
            _ => ExistingFilePolicy::Rename,
        })
        .unwrap_or(ExistingFilePolicy::Rename);

    // 6. Crear Job
    let job = Job {
        id: Uuid::new_v4().to_string(),
        playlist_id: validated.playlist_id,
        kind,
        status: JobStatus::Created,
        priority: 0,
        source_url: validated.canonical_url,
        output_directory: output_dir,
        organization_mode: org_mode,
        format_profile: format_profile.unwrap_or_else(|| "mp3_192".into()),
        existing_file_policy: file_policy,
        cancel_requested_at: None,
        created_at: now.clone(),
        started_at: None,
        completed_at: None,
    };

    jobs::create_job(&state.pool, &job).await?;

    // 7. Emitir evento
    let _ = emit_event(
        &app,
        &AppEvent::QueueChanged {
            reason: "jobCreated".into(),
        },
    );

    Ok(job)
}

/// Extrae los items de una playlist y los asocia al Job.
#[tauri::command]
pub async fn extract_and_enqueue_items(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<ExtractionResult, CommandError> {
    // 1. Cargar job y verificar estado
    let job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::NotFound {
            message: format!("Job '{}' no encontrado", job_id),
        })?;

    if job.status != JobStatus::Created {
        return Err(CommandError::InvalidState {
            message: format!(
                "El job debe estar en estado 'created', actual: '{}'",
                job.status.as_str()
            ),
        });
    }

    // 2. Transicionar a Extracting
    transition_job_status(&state.pool, &job_id, JobStatus::Extracting, None).await?;

    let _ = emit_event(
        &app,
        &AppEvent::JobStateChanged {
            job_id: job_id.clone(),
            previous_status: "created".into(),
            new_status: "extracting".into(),
        },
    );

    // 3. Obtener directorio de recursos para yt-dlp
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| CommandError::Internal {
            message: format!("Error de ruta de recursos: {}", e),
        })?;

    // 4. Extraer playlist
    let token = tokio_util::sync::CancellationToken::new();
    let playlist_info = youtube::extract_playlist(&resource_dir, &job.source_url, &token)
        .await
        .map_err(|msg| CommandError::Internal {
            message: format!("Error al extraer playlist: {}", msg),
        })?;

    // 5. Crear/actualizar playlist con metadata
    let now = Utc::now().to_rfc3339();
    let playlist_id = playlist_info.playlist_id.clone();
    let source_kind = if job.source_url.contains("music.youtube.com") {
        SourceKind::YouTubeMusic
    } else {
        SourceKind::YouTube
    };

    let playlist = Playlist {
        id: Uuid::new_v4().to_string(),
        youtube_playlist_id: playlist_id.clone(),
        source_url: job.source_url.clone(),
        source_kind,
        title: playlist_info.title.clone(),
        channel: String::new(),
        thumbnail_url: None,
        default_output_directory: Some(job.output_directory.clone()),
        last_synced_at: Some(now.clone()),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    upsert_playlist(&state.pool, &playlist).await?;

    // 6. Procesar entradas en lotes
    let mut total: u32 = 0;
    let mut available: u32 = 0;
    let batch_size = 50;

    for chunk in playlist_info.entries.chunks(batch_size) {
        for entry in chunk {
            total += 1;

            // Crear/actualizar Track
            let track = Track {
                id: Uuid::new_v4().to_string(),
                youtube_video_id: entry.id.clone(),
                source_url: entry.url.clone(),
                title: entry.title.clone(),
                artist: entry.artist.clone(),
                channel: String::new(),
                published_at: None,
                duration_seconds: entry.duration_seconds,
                thumbnail_url: None,
                availability: "available".into(),
                metadata: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            upsert_track(&state.pool, &track).await?;

            // Crear PlaylistTrack
            let playlist_track = PlaylistTrack {
                id: Uuid::new_v4().to_string(),
                playlist_id: playlist.id.clone(),
                track_id: track.id.clone(),
                position: entry.index as i32,
                source_entry_id: Some(entry.id.clone()),
                title_at_sync: entry.title.clone(),
                discovered_at: now.clone(),
                removed_at: None,
            };
            add_playlist_track(&state.pool, &playlist_track).await?;

            // Crear JobItem
            let job_item = JobItem {
                id: Uuid::new_v4().to_string(),
                job_id: job_id.clone(),
                track_id: Some(track.id),
                playlist_track_id: Some(playlist_track.id),
                playlist_position: Some(entry.index as i32),
                status: JobItemStatus::Queued,
                priority_offset: 0,
                progress_percent: Some(0.0),
                downloaded_bytes: Some(0),
                estimated_total_bytes: None,
                attempts: 0,
                next_attempt_at: None,
                temporary_path: None,
                output_path: None,
                error_code: None,
                error_message: None,
                execution_lease_expires_at: None,
                created_at: now.clone(),
                started_at: None,
                completed_at: None,
            };
            repo::job_items::create_job_item(&state.pool, &job_item).await?;

            available += 1;
        }

        // Emitir progreso
        let _ = emit_event(
            &app,
            &AppEvent::ExtractionProgress {
                job_id: job_id.clone(),
                processed: available,
                total: playlist_info.entry_count as u32,
            },
        );
    }

    // 7. Asociar playlist_id al job
    sqlx::query("UPDATE jobs SET playlist_id = ? WHERE id = ?")
        .bind(&playlist.id)
        .bind(&job_id)
        .execute(&state.pool)
        .await?;

    // 8. Transicionar a Queued
    transition_job_status(&state.pool, &job_id, JobStatus::Queued, None).await?;

    let _ = emit_event(
        &app,
        &AppEvent::JobStateChanged {
            job_id: job_id.clone(),
            previous_status: "extracting".into(),
            new_status: "queued".into(),
        },
    );

    Ok(ExtractionResult {
        total,
        available,
        unavailable: 0,
        playlist_id: playlist.id,
    })
}

/// Pausa un Job.
#[tauri::command]
pub async fn pause_job(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
    immediate: bool,
) -> Result<Job, CommandError> {
    let job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::NotFound {
            message: format!("Job '{}' no encontrado", job_id),
        })?;

    let previous_status = job.status.as_str().to_string();

    if !matches!(
        job.status,
        JobStatus::Running | JobStatus::Queued | JobStatus::Extracting
    ) {
        return Err(CommandError::InvalidState {
            message: format!(
                "No se puede pausar un job en estado '{}'",
                job.status.as_str()
            ),
        });
    }

    // Si es inmediato, interrumpir items activos
    if immediate {
        let active_items: Vec<JobItem> =
            repo::job_items::list_job_items_by_job(&state.pool, &job_id)
                .await?
                .into_iter()
                .filter(|item| item.status.is_active_execution())
                .collect();

        for item in active_items {
            repo::job_items::transition_job_item_status(
                &state.pool,
                &item.id,
                JobItemStatus::Interrupted,
                None,
                Some("immediate_pause"),
            )
            .await?;
        }
    }

    let updated_status =
        transition_job_status(&state.pool, &job_id, JobStatus::Paused, Some("user_pause")).await?;

    let updated_job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::Internal {
            message: "Job no encontrado después de actualizar".into(),
        })?;

    let _ = emit_event(
        &app,
        &AppEvent::JobStateChanged {
            job_id,
            previous_status,
            new_status: updated_status.as_str().to_string(),
        },
    );

    Ok(updated_job)
}

/// Reanuda un Job pausado.
#[tauri::command]
pub async fn resume_job(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Job, CommandError> {
    let job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::NotFound {
            message: format!("Job '{}' no encontrado", job_id),
        })?;

    if job.status != JobStatus::Paused {
        return Err(CommandError::InvalidState {
            message: format!(
                "Solo se pueden reanudar jobs pausados, actual: '{}'",
                job.status.as_str()
            ),
        });
    }

    let previous_status = job.status.as_str().to_string();

    // Resetear items Interrupted/Paused a Queued
    let items = repo::job_items::list_job_items_by_job(&state.pool, &job_id).await?;
    for item in items {
        if matches!(
            item.status,
            JobItemStatus::Paused | JobItemStatus::Interrupted
        ) {
            repo::job_items::transition_job_item_status(
                &state.pool,
                &item.id,
                JobItemStatus::Queued,
                None,
                Some("resume"),
            )
            .await?;
        }
    }

    let updated_status =
        transition_job_status(&state.pool, &job_id, JobStatus::Queued, Some("user_resume")).await?;

    let updated_job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::Internal {
            message: "Job no encontrado después de actualizar".into(),
        })?;

    let _ = emit_event(
        &app,
        &AppEvent::JobStateChanged {
            job_id,
            previous_status,
            new_status: updated_status.as_str().to_string(),
        },
    );

    Ok(updated_job)
}

/// Cancela un Job.
#[tauri::command]
pub async fn cancel_job(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Job, CommandError> {
    let job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::NotFound {
            message: format!("Job '{}' no encontrado", job_id),
        })?;

    if job.status.is_terminal() {
        return Err(CommandError::InvalidState {
            message: format!(
                "El job ya está en estado terminal '{}'",
                job.status.as_str()
            ),
        });
    }

    let previous_status = job.status.as_str().to_string();

    // Transicionar a Cancelling
    transition_job_status(
        &state.pool,
        &job_id,
        JobStatus::Cancelling,
        Some("user_cancel"),
    )
    .await?;

    // Cancelar items activos
    let items = repo::job_items::list_job_items_by_job(&state.pool, &job_id).await?;
    for item in items {
        if item.status.is_active_execution() {
            repo::job_items::transition_job_item_status(
                &state.pool,
                &item.id,
                JobItemStatus::Cancelled,
                None,
                Some("job_cancelled"),
            )
            .await?;
        }
    }

    // Liberar reservas del job
    sqlx::query("DELETE FROM item_reservations WHERE owner_job_item_id IN (SELECT id FROM job_items WHERE job_id = ?)")
        .bind(&job_id)
        .execute(&state.pool)
        .await?;

    // Transicionar a Cancelled
    let updated_status = transition_job_status(
        &state.pool,
        &job_id,
        JobStatus::Cancelled,
        Some("user_cancel_complete"),
    )
    .await?;

    let updated_job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::Internal {
            message: "Job no encontrado después de actualizar".into(),
        })?;

    let _ = emit_event(
        &app,
        &AppEvent::JobStateChanged {
            job_id,
            previous_status,
            new_status: updated_status.as_str().to_string(),
        },
    );

    Ok(updated_job)
}

/// Reintenta items fallidos de un Job.
#[tauri::command]
pub async fn retry_failed_items(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<u32, CommandError> {
    let items = repo::job_items::list_job_items_by_job(&state.pool, &job_id).await?;

    // Códigos de error reintentables
    let retryable_codes = [
        "network_error",
        "timeout",
        "rate_limited",
        "temporary_failure",
        "connection_refused",
        "interrupted",
    ];

    let mut retried_count = 0;

    for item in items {
        if item.status != JobItemStatus::Failed {
            continue;
        }

        let is_retryable = item
            .error_code
            .as_ref()
            .map(|code| retryable_codes.contains(&code.as_str()))
            .unwrap_or(false);

        if !is_retryable {
            continue;
        }

        // Resetear item
        sqlx::query(
            "UPDATE job_items SET status = 'queued', attempts = 0, error_code = NULL, error_message = NULL WHERE id = ?"
        )
        .bind(&item.id)
        .execute(&state.pool)
        .await?;

        retried_count += 1;
    }

    Ok(retried_count)
}

/// Establece la prioridad de un Job.
#[tauri::command]
pub async fn set_job_priority(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: String,
    priority: i32,
) -> Result<Job, CommandError> {
    sqlx::query("UPDATE jobs SET priority = ? WHERE id = ?")
        .bind(priority)
        .bind(&job_id)
        .execute(&state.pool)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al actualizar prioridad: {}", e),
        })?;

    let _ = emit_event(
        &app,
        &AppEvent::QueueChanged {
            reason: "priorityChanged".into(),
        },
    );

    let job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::NotFound {
            message: format!("Job '{}' no encontrado", job_id),
        })?;

    Ok(job)
}

/// Obtiene el progreso de un Job.
#[tauri::command]
pub async fn get_job_progress(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobProgress, CommandError> {
    let job = jobs::get_job(&state.pool, &job_id)
        .await?
        .ok_or(CommandError::NotFound {
            message: format!("Job '{}' no encontrado", job_id),
        })?;

    let items = repo::job_items::list_job_items_by_job(&state.pool, &job_id).await?;

    let mut completed = 0u32;
    let mut failed = 0u32;
    let mut pending = 0u32;
    let mut current_item: Option<CurrentItem> = None;

    for item in &items {
        match item.status {
            JobItemStatus::Completed => completed += 1,
            JobItemStatus::Failed => failed += 1,
            JobItemStatus::Queued | JobItemStatus::Pending => pending += 1,
            JobItemStatus::Downloading
            | JobItemStatus::Converting
            | JobItemStatus::Tagging
            | JobItemStatus::Validating => {
                pending += 1;
                if current_item.is_none() {
                    let track_title = if let Some(ref track_id) = item.track_id {
                        repo::playlists::get_track(&state.pool, track_id)
                            .await?
                            .map(|t| t.title)
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    current_item = Some(CurrentItem {
                        item_id: item.id.clone(),
                        track_title,
                        stage: item.status.as_str().into(),
                        progress: item.progress_percent.unwrap_or(0.0),
                    });
                }
            }
            _ => pending += 1,
        }
    }

    let total_items = items.len() as u32;
    let percent_complete = if total_items > 0 {
        (completed as f32 / total_items as f32) * 100.0
    } else {
        0.0
    };

    Ok(JobProgress {
        job_id,
        status: job.status.as_str().into(),
        total_items,
        completed_items: completed,
        failed_items: failed,
        pending_items: pending,
        current_item,
        percent_complete,
    })
}

/// Obtiene el estado de la cola de jobs.
#[tauri::command]
pub async fn get_queue_status(state: State<'_, AppState>) -> Result<Vec<QueueEntry>, CommandError> {
    let all_jobs = jobs::list_jobs(&state.pool).await?;
    let mut entries = Vec::new();

    for job in all_jobs {
        if !matches!(
            job.status,
            JobStatus::Queued | JobStatus::Running | JobStatus::Extracting | JobStatus::Paused
        ) {
            continue;
        }

        let items = repo::job_items::list_job_items_by_job(&state.pool, &job.id).await?;
        let total_items = items.len() as u32;
        let completed_items = items
            .iter()
            .filter(|i| i.status == JobItemStatus::Completed)
            .count() as u32;

        let created_at = chrono::DateTime::parse_from_rfc3339(&job.created_at)
            .unwrap_or_else(|_| Utc::now().into())
            .with_timezone(&Utc);

        entries.push(QueueEntry {
            job_id: job.id,
            kind: job.kind.as_str().into(),
            status: job.status.as_str().into(),
            priority: job.priority,
            total_items,
            completed_items,
            created_at,
        });
    }

    // Ordenar por prioridad descendente
    entries.sort_by_key(|a| std::cmp::Reverse(a.priority));

    Ok(entries)
}

// ============================================================================
// COMANDOS DE PLAYLIST
// ============================================================================

/// Lista todas las playlists.
#[tauri::command]
pub async fn list_playlists(
    state: State<'_, AppState>,
) -> Result<Vec<PlaylistSummary>, CommandError> {
    let playlists = repo_list_playlists(&state.pool).await?;
    let mut summaries = Vec::new();

    for playlist in playlists {
        let track_count = get_active_playlist_tracks(&state.pool, &playlist.id)
            .await?
            .len() as u32;

        let active_jobs: Vec<Job> = jobs::list_jobs(&state.pool)
            .await?
            .into_iter()
            .filter(|j| j.playlist_id == Some(playlist.id.clone()) && !j.status.is_terminal())
            .collect();

        let last_synced_at = playlist
            .last_synced_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        summaries.push(PlaylistSummary {
            id: playlist.id,
            title: playlist.title,
            channel: playlist.channel,
            track_count,
            last_synced_at,
            active_job_count: active_jobs.len() as u32,
        });
    }

    Ok(summaries)
}

/// Obtiene los detalles de una playlist con paginación.
#[tauri::command]
pub async fn get_playlist_details(
    state: State<'_, AppState>,
    playlist_id: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<PlaylistDetails, CommandError> {
    let playlist =
        repo_get_playlist(&state.pool, &playlist_id)
            .await?
            .ok_or(CommandError::NotFound {
                message: format!("Playlist '{}' no encontrada", playlist_id),
            })?;

    let all_tracks = get_active_playlist_tracks(&state.pool, &playlist_id).await?;
    let total = all_tracks.len() as u32;

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(100);

    let paginated: Vec<PlaylistTrack> = all_tracks
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();

    let mut tracks_with_status = Vec::new();
    for pt in paginated {
        let track = repo::playlists::get_track(&state.pool, &pt.track_id)
            .await?
            .ok_or(CommandError::NotFound {
                message: format!("Track '{}' no encontrado", pt.track_id),
            })?;

        // Verificar si hay un job_item activo para este track
        let download_status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM job_items WHERE track_id = ? AND job_id IN (SELECT id FROM jobs WHERE status NOT IN ('completed', 'cancelled', 'failed')) LIMIT 1"
        )
        .bind(&pt.track_id)
        .fetch_optional(&state.pool)
        .await?;

        tracks_with_status.push(PlaylistTrackWithStatus {
            track,
            position: pt.position as u32,
            download_status,
        });
    }

    Ok(PlaylistDetails {
        playlist,
        tracks: tracks_with_status,
        total,
    })
}

// ============================================================================
// COMANDOS DE ARCHIVOS LOCALES
// ============================================================================

/// Obtiene archivos locales registrados.
#[tauri::command]
pub async fn get_local_files(
    state: State<'_, AppState>,
    track_id: Option<String>,
    playlist_id: Option<String>,
) -> Result<Vec<LocalFile>, CommandError> {
    let mut query = String::from(
        "SELECT id, track_id, playlist_track_id, format_profile, path, size_bytes, modified_at, validation_status, validated_at, video_id_tag, created_at FROM local_files WHERE 1=1"
    );

    if track_id.is_some() {
        query.push_str(" AND track_id = ?");
    }
    if playlist_id.is_some() {
        query.push_str(
            " AND playlist_track_id IN (SELECT id FROM playlist_tracks WHERE playlist_id = ?)",
        );
    }

    let mut q = sqlx::query(&query);

    if let Some(ref id) = track_id {
        q = q.bind(id);
    }
    if let Some(ref id) = playlist_id {
        q = q.bind(id);
    }

    let rows = q
        .fetch_all(&state.pool)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al consultar archivos locales: {}", e),
        })?;

    let files: Vec<LocalFile> = rows
        .iter()
        .map(|r| LocalFile {
            id: r.get("id"),
            track_id: r.get("track_id"),
            playlist_track_id: r.get("playlist_track_id"),
            format_profile: r.get("format_profile"),
            path: r.get("path"),
            size_bytes: r.get("size_bytes"),
            modified_at: r.get("modified_at"),
            validation_status: r.get("validation_status"),
            validated_at: r.get("validated_at"),
            video_id_tag: r.get("video_id_tag"),
            created_at: r.get("created_at"),
        })
        .collect();

    Ok(files)
}

// ============================================================================
// COMANDOS DE CONFLICTOS
// ============================================================================

/// Resuelve conflictos de archivos para un Job.
#[tauri::command]
pub async fn resolve_file_conflicts(
    state: State<'_, AppState>,
    job_id: String,
    resolution: String,
    apply_to_all: bool,
) -> Result<u32, CommandError> {
    let _policy = match resolution.as_str() {
        "reuse" => ExistingFilePolicy::Reuse,
        "overwrite" => ExistingFilePolicy::Overwrite,
        "rename" => ExistingFilePolicy::Rename,
        _ => {
            return Err(CommandError::Validation {
                message: format!("Resolución no válida: {}", resolution),
            })
        }
    };

    if apply_to_all {
        // Aplicar política a todos los items del job
        sqlx::query(
            "UPDATE job_items SET status = 'queued' WHERE job_id = ? AND status = 'waiting_for_duplicate'"
        )
        .bind(&job_id)
        .execute(&state.pool)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al resolver conflictos: {}", e),
        })?;
    }

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM job_items WHERE job_id = ? AND status = 'waiting_for_duplicate'",
    )
    .bind(&job_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    Ok(count as u32)
}

// ============================================================================
// COMANDOS DE DIAGNÓSTICO
// ============================================================================

/// Exporta logs de diagnóstico a un archivo ZIP.
#[tauri::command]
pub async fn export_diagnostic_logs(
    state: State<'_, AppState>,
    output_path: String,
) -> Result<ExportResult, CommandError> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let path = std::path::Path::new(&output_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CommandError::Internal {
            message: format!("Error al crear directorio de destino: {}", e),
        })?;
    }

    let file = std::fs::File::create(path).map_err(|e| CommandError::Internal {
        message: format!("Error al crear archivo ZIP: {}", e),
    })?;

    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    // 1. Info de versión
    let version_info = format!(
        "App Version: 0.1.0\nTimestamp: {}\n",
        Utc::now().to_rfc3339()
    );
    zip.start_file("version.txt", options)
        .map_err(|e| CommandError::Internal {
            message: format!("Error al escribir en ZIP: {}", e),
        })?;
    zip.write_all(version_info.as_bytes())
        .map_err(|e| CommandError::Internal {
            message: format!("Error al escribir versión: {}", e),
        })?;

    // 2. Últimos 500 eventos de actividad (sanitizados)
    let rows = sqlx::query(
        "SELECT id, entity_type, entity_id, event_type, old_state, new_state, payload, created_at FROM activity_events ORDER BY id DESC LIMIT 500"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| CommandError::Internal {
        message: format!("Error al consultar eventos: {}", e),
    })?;

    zip.start_file("activity_events.csv", options)
        .map_err(|e| CommandError::Internal {
            message: format!("Error al escribir en ZIP: {}", e),
        })?;
    zip.write_all(b"id,entity_type,entity_id,event_type,old_state,new_state,created_at\n")
        .map_err(|e| CommandError::Internal {
            message: format!("Error al escribir header: {}", e),
        })?;

    for row in rows {
        let id: i64 = row.get("id");
        let entity_type: String = row.get("entity_type");
        let entity_id: String = row.get("entity_id");
        let event_type: String = row.get("event_type");
        let old_state: Option<String> = row.get("old_state");
        let new_state: Option<String> = row.get("new_state");
        let created_at: String = row.get("created_at");

        let line = format!(
            "{},{},{},{},{},{},{}\n",
            id,
            entity_type,
            entity_id,
            event_type,
            old_state.as_deref().unwrap_or(""),
            new_state.as_deref().unwrap_or(""),
            created_at
        );
        zip.write_all(line.as_bytes())
            .map_err(|e| CommandError::Internal {
                message: format!("Error al escribir evento: {}", e),
            })?;
    }

    // 3. Resumen de jobs
    let all_jobs = jobs::list_jobs(&state.pool).await?;
    zip.start_file("jobs_summary.csv", options)
        .map_err(|e| CommandError::Internal {
            message: format!("Error al escribir en ZIP: {}", e),
        })?;
    zip.write_all(b"id,kind,status,priority,created_at,started_at,completed_at\n")
        .map_err(|e| CommandError::Internal {
            message: format!("Error al escribir header: {}", e),
        })?;

    for job in all_jobs {
        let line = format!(
            "{},{},{},{},{},{},{}\n",
            job.id,
            job.kind.as_str(),
            job.status.as_str(),
            job.priority,
            job.created_at,
            job.started_at.as_deref().unwrap_or(""),
            job.completed_at.as_deref().unwrap_or("")
        );
        zip.write_all(line.as_bytes())
            .map_err(|e| CommandError::Internal {
                message: format!("Error al escribir job: {}", e),
            })?;
    }

    zip.finish().map_err(|e| CommandError::Internal {
        message: format!("Error al finalizar ZIP: {}", e),
    })?;

    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    Ok(ExportResult {
        path: output_path,
        size_bytes,
    })
}

// ============================================================================
// COMANDOS EXISTENTES (CONSERVADOS)
// ============================================================================

fn get_default_music_dir() -> PathBuf {
    if let Some(user_dir) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        return user_dir.join("Music").join("YT Downloads");
    }
    PathBuf::from("C:\\YT Downloads")
}

/// Obtiene el directorio de salida por defecto.
#[tauri::command]
pub async fn get_default_output_directory() -> Result<String, CommandError> {
    let dir = get_default_music_dir();
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.display().to_string())
}

/// Obtiene la versión actual de yt-dlp.
#[tauri::command]
pub async fn get_ytdlp_version(
    app: AppHandle,
) -> Result<crate::updater::YtdlpVersionInfo, CommandError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| CommandError::Internal {
            message: format!("Error de ruta de recursos: {}", e),
        })?;
    crate::updater::get_current_version(&resource_dir)
        .await
        .map_err(|msg| CommandError::Internal { message: msg })
}

/// Verifica actualizaciones de yt-dlp.
#[tauri::command]
pub async fn check_ytdlp_updates(
    app: AppHandle,
    manifest_url: String,
) -> Result<Vec<crate::updater::ReleaseEntry>, CommandError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| CommandError::Internal {
            message: format!("Error de ruta de recursos: {}", e),
        })?;
    let token = tokio_util::sync::CancellationToken::new();
    crate::updater::check_for_updates(&resource_dir, &manifest_url, &token)
        .await
        .map_err(|msg| CommandError::Internal { message: msg })
}

/// Actualiza el binario de yt-dlp.
#[tauri::command]
pub async fn update_ytdlp(
    app: AppHandle,
    release: crate::updater::ReleaseEntry,
) -> Result<crate::updater::UpdateResult, CommandError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| CommandError::Internal {
            message: format!("Error de ruta de recursos: {}", e),
        })?;
    let token = tokio_util::sync::CancellationToken::new();
    crate::updater::update_binary(&resource_dir, &release, &token)
        .await
        .map_err(|msg| CommandError::Internal { message: msg })
}

/// Extrae información de una playlist.
#[tauri::command]
pub async fn extract_playlist(
    app: AppHandle,
    url: String,
) -> Result<youtube::PlaylistInfo, CommandError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| CommandError::Internal {
            message: format!("Error de ruta de recursos: {}", e),
        })?;
    let token = tokio_util::sync::CancellationToken::new();
    youtube::extract_playlist(&resource_dir, &url, &token)
        .await
        .map_err(|msg| CommandError::Internal { message: msg })
}

/// Obtiene el reporte de recuperación al inicio.
#[tauri::command]
pub async fn get_recovery_report(
    state: State<'_, AppState>,
) -> Result<crate::persistence::RecoveryReport, CommandError> {
    let report = state
        .initial_recovery_report
        .lock()
        .map_err(|_| CommandError::Internal {
            message: "Error de concurrencia al leer reporte".into(),
        })?;
    Ok(report.clone())
}

/// Obtiene la configuración de la app.
#[tauri::command]
pub async fn get_app_settings(state: State<'_, AppState>) -> Result<AppSettings, CommandError> {
    settings::get_settings(&state.pool)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al consultar configuración: {}", e),
        })
}

/// Actualiza la configuración de la app.
#[tauri::command]
pub async fn update_app_settings(
    state: State<'_, AppState>,
    settings_payload: AppSettings,
) -> Result<AppSettings, CommandError> {
    settings::update_settings(&state.pool, &settings_payload)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al actualizar configuración: {}", e),
        })
}

/// Lista todos los jobs.
#[tauri::command]
pub async fn list_jobs(state: State<'_, AppState>) -> Result<Vec<Job>, CommandError> {
    jobs::list_jobs(&state.pool)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al listar trabajos: {}", e),
        })
}

/// Obtiene detalles de un job.
#[tauri::command]
pub async fn get_job_details(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Option<Job>, CommandError> {
    jobs::get_job(&state.pool, &job_id)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al consultar trabajo: {}", e),
        })
}

/// Lista los items de un job.
#[tauri::command]
pub async fn list_job_items(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Vec<JobItem>, CommandError> {
    repo::job_items::list_job_items_by_job(&state.pool, &job_id)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("Error al listar ítems: {}", e),
        })
}
