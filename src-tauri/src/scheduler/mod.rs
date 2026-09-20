use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use sqlx::{Row, SqlitePool};
use tokio::sync::{Mutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    audio,
    domain::{
        entities::LocalFile,
        states::{JobItemStatus, JobStatus, ValidationStatus},
    },
    filesystem::{self, ExistingFilePolicy},
    persistence::repositories::{
        self as repo,
        jobs::transition_job_status,
        local_files::register_local_file,
        reservations::{self, AcquireReservationResult},
    },
    youtube::{self, VideoMetadata},
};

/// Fila de job item con datos del job padre para ordenamiento.
#[derive(Clone, Debug)]
struct JobItemRow {
    id: String,
    job_id: String,
    track_id: Option<String>,
    playlist_track_id: Option<String>,
    playlist_position: Option<i32>,
    source_url: String,
    output_directory: String,
    temp_base: String,
    format_profile: String,
    existing_file_policy: ExistingFilePolicy,
    youtube_video_id: String,
    title: String,
    artist: String,
    album: Option<String>,
    upload_date: Option<String>,
    duration_seconds: Option<f64>,
    thumbnail_url: Option<String>,
    playlist_title: String,
}

/// Clasificación de error para decidir reintento.
#[derive(Debug)]
struct ClassifiedError {
    code: String,
    retryable: bool,
}

/// Orquestador de descarga: selecciona items y ejecuta el pipeline completo.
pub struct DownloadScheduler {
    pool: SqlitePool,
    resource_dir: PathBuf,
    download_semaphore: Arc<Semaphore>,
    conversion_semaphore: Arc<Semaphore>,
    shutdown_token: CancellationToken,
    notify: Notify,
    job_tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl DownloadScheduler {
    pub fn new(
        pool: SqlitePool,
        resource_dir: PathBuf,
        max_downloads: usize,
        max_conversions: usize,
    ) -> Self {
        Self {
            pool,
            resource_dir,
            download_semaphore: Arc::new(Semaphore::new(max_downloads.max(1))),
            conversion_semaphore: Arc::new(Semaphore::new(max_conversions.max(1))),
            shutdown_token: CancellationToken::new(),
            notify: Notify::new(),
            job_tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Notifica al scheduler que hay nuevos items disponibles.
    pub fn notify_new_items(&self) {
        self.notify.notify_one();
    }

    /// Señala al scheduler que pare de forma ordenada.
    pub fn shutdown(&self) {
        self.shutdown_token.cancel();
    }

    /// Crea un token de cancelación para un job específico.
    pub async fn register_job(&self, job_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.job_tokens.lock().await.insert(job_id.to_string(), token.clone());
        token
    }

    /// Obtiene o crea el token de un job.
    pub async fn get_job_token(&self, job_id: &str) -> CancellationToken {
        let mut tokens = self.job_tokens.lock().await;
        tokens
            .entry(job_id.to_string())
            .or_insert_with(CancellationToken::new)
            .clone()
    }

    /// Pausa un job: cancela su token.
    pub async fn pause_job(&self, job_id: &str) {
        if let Some(token) = self.job_tokens.lock().await.remove(job_id) {
            token.cancel();
        }
    }

    /// Cancela un job: cancela su token.
    pub async fn cancel_job(&self, job_id: &str) {
        if let Some(token) = self.job_tokens.lock().await.remove(job_id) {
            token.cancel();
        }
    }

    /// Reanuda un job: crea un nuevo token.
    pub async fn resume_job(&self, job_id: &str) {
        let token = CancellationToken::new();
        self.job_tokens.lock().await.insert(job_id.to_string(), token);
        self.notify.notify_one();
    }

    /// Bucle principal del scheduler.
    pub async fn run(&self) {
        loop {
            tokio::select! {
                _ = self.shutdown_token.cancelled() => break,
                _ = self.notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_secs(2)) => {},
            }

            let ready_items = self.select_ready_items().await;
            for item in ready_items {
                let pool = self.pool.clone();
                let resource_dir = self.resource_dir.clone();
                let dl_sem = self.download_semaphore.clone();
                let cv_sem = self.conversion_semaphore.clone();
                let job_token = self.get_job_token(&item.job_id).await;

                tokio::spawn(async move {
                    process_item(pool, resource_dir, item, dl_sem, cv_sem, job_token).await;
                });
            }
        }
    }

    /// Selecciona items encolados respetando prioridad y concurrencia.
    async fn select_ready_items(&self) -> Vec<JobItemRow> {
        let available_dl = self.download_semaphore.available_permits();
        if available_dl == 0 {
            return Vec::new();
        }

        let rows = sqlx::query(
            r#"
            SELECT
                ji.id AS item_id,
                ji.job_id,
                ji.track_id,
                ji.playlist_track_id,
                ji.playlist_position,
                j.source_url,
                j.output_directory,
                j.format_profile,
                j.existing_file_policy,
                j.priority AS job_priority,
                t.youtube_video_id,
                t.title,
                t.artist,
                t.duration_seconds,
                t.thumbnail_url,
                p.title AS playlist_title
            FROM job_items ji
            JOIN jobs j ON ji.job_id = j.id
            LEFT JOIN tracks t ON ji.track_id = t.id
            LEFT JOIN playlists p ON j.playlist_id = p.id
            WHERE ji.status = 'queued'
              AND j.status IN ('queued', 'running')
            ORDER BY j.priority DESC, ji.playlist_position ASC, ji.created_at ASC
            LIMIT ?
            "#,
        )
        .bind(available_dl as i64)
        .fetch_all(&self.pool)
        .await;

        let rows = match rows {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let mut items = Vec::new();
        for r in rows {
            let policy_str: String = r.get("existing_file_policy");
            let policy = match policy_str.as_str() {
                "reuse" => ExistingFilePolicy::Reuse,
                "overwrite" => ExistingFilePolicy::Overwrite,
                "rename" => ExistingFilePolicy::Rename,
                "fail_if_exists" => ExistingFilePolicy::FailIfExists,
                _ => ExistingFilePolicy::Ask,
            };

            items.push(JobItemRow {
                id: r.get("item_id"),
                job_id: r.get("job_id"),
                track_id: r.get("track_id"),
                playlist_track_id: r.get("playlist_track_id"),
                playlist_position: r.get("playlist_position"),
                source_url: r.get("source_url"),
                output_directory: r.get("output_directory"),
                temp_base: std::env::temp_dir()
                    .join("yt-downloader-temp")
                    .to_string_lossy()
                    .to_string(),
                format_profile: r.get("format_profile"),
                existing_file_policy: policy,
                youtube_video_id: r.get("youtube_video_id"),
                title: r.get("title"),
                artist: r.get("artist"),
                album: None,
                upload_date: None,
                duration_seconds: r.get("duration_seconds"),
                thumbnail_url: r.get("thumbnail_url"),
                playlist_title: r.get("playlist_title"),
            });
        }

        items
    }
}

/// Ejecuta el pipeline completo para un item.
async fn process_item(
    pool: SqlitePool,
    resource_dir: PathBuf,
    item: JobItemRow,
    dl_sem: Arc<Semaphore>,
    cv_sem: Arc<Semaphore>,
    job_token: CancellationToken,
) {
    let item_id = item.id.clone();
    let job_id = item.job_id.clone();

    // 0. Preparar directorio temporal
    let dest_dir = PathBuf::from(&item.output_directory);
    let item_uuid = match Uuid::parse_str(&item_id) {
        Ok(u) => u,
        Err(_) => {
            let _ = repo::job_items::update_job_item_error(
                &pool,
                &item_id,
                "invalid_uuid",
                "ID de item inválido",
            )
            .await;
            return;
        }
    };

    let (temp_dir, _same_volume) =
        match filesystem::prepare_item_temp_dir(&dest_dir, &PathBuf::from(&item.temp_base), &item_uuid)
        {
            Ok(d) => d,
            Err(e) => {
                handle_item_error(&pool, &item_id, &job_id, "prepare", &e).await;
                return;
            }
        };

    // 1. Adquirir reserva exclusiva
    let reservation = reservations::acquire_or_wait_reservation(
        &pool,
        &item.youtube_video_id,
        &item.format_profile,
        &item_id,
        300,
    )
    .await;

    match reservation {
        Ok(AcquireReservationResult::WaitingForDuplicate { .. }) => {
            // Reintentar más tarde
            return;
        }
        Ok(AcquireReservationResult::Acquired) => {}
        Err(e) => {
            handle_item_error(&pool, &item_id, &job_id, "reservation", &e.to_string()).await;
            return;
        }
    }

    // 2. Transicionar a Downloading
    let _ = repo::job_items::transition_job_item_status(
        &pool,
        &item_id,
        JobItemStatus::Downloading,
        Some(600),
        Some("scheduler_start"),
    )
    .await;

    // 3. Descargar audio
    let _dl_permit = match dl_sem.acquire().await {
        Ok(p) => p,
        Err(_) => return,
    };

    let download_result = tokio::select! {
        res = youtube::download_audio_stream(
            &resource_dir,
            &item.source_url,
            &temp_dir,
            &job_token,
            |_pct, _line| {},
        ) => res,
        _ = job_token.cancelled() => {
            let _ = repo::job_items::transition_job_item_status(
                &pool,
                &item_id,
                JobItemStatus::Interrupted,
                None,
                Some("job_cancelled"),
            )
            .await;
            let _ = reservations::release_reservation(
                &pool,
                &item.youtube_video_id,
                &item.format_profile,
                &item_id,
            )
            .await;
            return;
        }
    };

    let source_path = match download_result {
        Ok(path) => path,
        Err(e) => {
            handle_item_error(&pool, &item_id, &job_id, "download", &e).await;
            return;
        }
    };

    // 4. Transicionar a Converting
    let _cv_permit = match cv_sem.acquire().await {
        Ok(p) => p,
        Err(_) => return,
    };

    let _ = repo::job_items::transition_job_item_status(
        &pool,
        &item_id,
        JobItemStatus::Converting,
        Some(300),
        Some("convert_start"),
    )
    .await;

    let mp3_path = temp_dir.join("output.mp3");
    let convert_result = tokio::select! {
        res = audio::convert_to_mp3(&resource_dir, &source_path, &mp3_path, &job_token) => res,
        _ = job_token.cancelled() => {
            let _ = repo::job_items::transition_job_item_status(
                &pool,
                &item_id,
                JobItemStatus::Interrupted,
                None,
                Some("job_cancelled"),
            )
            .await;
            return;
        }
    };

    if let Err(e) = convert_result {
        handle_item_error(&pool, &item_id, &job_id, "convert", &e).await;
        return;
    }

    // 5. Transicionar a Tagging
    let _ = repo::job_items::transition_job_item_status(
        &pool,
        &item_id,
        JobItemStatus::Tagging,
        None,
        Some("tag_start"),
    )
    .await;

    let metadata = VideoMetadata {
        id: item.youtube_video_id.clone(),
        canonical_url: item.source_url.clone(),
        title: item.title.clone(),
        artist: item.artist.clone(),
        album: item.album.clone(),
        upload_date: item.upload_date.clone(),
        duration_seconds: item.duration_seconds,
        thumbnail_url: item.thumbnail_url.clone(),
    };

    let cover_path = temp_dir.join("cover.jpg");
    let cover_opt = if cover_path.exists() {
        Some(cover_path.as_path())
    } else {
        None
    };

    if let Err(e) = audio::tag_mp3(
        &mp3_path,
        &metadata,
        cover_opt,
        Some(&item.playlist_title),
    ) {
        handle_item_error(&pool, &item_id, &job_id, "tag", &e).await;
        return;
    }

    // 6. Transicionar a Validating
    let _ = repo::job_items::transition_job_item_status(
        &pool,
        &item_id,
        JobItemStatus::Validating,
        Some(60),
        Some("validate_start"),
    )
    .await;

    let validation = match audio::validate_mp3(&resource_dir, &mp3_path, &job_token).await {
        Ok(v) => v,
        Err(e) => {
            handle_item_error(&pool, &item_id, &job_id, "validate", &e).await;
            return;
        }
    };

    // 7. Mover a destino final
    let final_path = filesystem::resolve_destination_path(
        &dest_dir,
        &item.title,
        item.playlist_position.map(|p| p as u32),
        item.existing_file_policy,
    );

    let final_path = match final_path {
        Ok(p) => p,
        Err(e) => {
            handle_item_error(&pool, &item_id, &job_id, "resolve_path", &e).await;
            return;
        }
    };

    if let Err(e) = filesystem::move_file_safely(&mp3_path, &final_path, true) {
        handle_item_error(&pool, &item_id, &job_id, "move", &e).await;
        return;
    }

    // 8. Registrar en local_files
    let now = Utc::now().to_rfc3339();
    let size_bytes = std::fs::metadata(&final_path).map(|m| m.len() as i64).unwrap_or(0);
    let local_file = LocalFile {
        id: Uuid::new_v4().to_string(),
        track_id: item.track_id.clone().unwrap_or_default(),
        playlist_track_id: item.playlist_track_id.clone(),
        format_profile: item.format_profile.clone(),
        path: final_path.to_string_lossy().to_string(),
        size_bytes,
        modified_at: now.clone(),
        validation_status: if validation.valid {
            ValidationStatus::Valid
        } else {
            ValidationStatus::Corrupted
        },
        validated_at: now.clone(),
        video_id_tag: item.youtube_video_id.clone(),
        created_at: now,
    };

    let _ = register_local_file(&pool, &local_file).await;

    // 9. Limpiar temporales
    let _ = filesystem::clean_item_temp_dir(&temp_dir);

    // 10. Liberar reserva
    let _ = reservations::release_reservation(
        &pool,
        &item.youtube_video_id,
        &item.format_profile,
        &item_id,
    )
    .await;

    // 11. Transicionar a Completed
    let _ = repo::job_items::transition_job_item_status(
        &pool,
        &item_id,
        JobItemStatus::Completed,
        None,
        Some("completed"),
    )
    .await;

    // 12. Verificar si el job completo terminó
    check_job_completion(&pool, &job_id).await;
}

/// Maneja el error de un item: transiciona a Failed y programa reintento si aplica.
async fn handle_item_error(
    pool: &SqlitePool,
    item_id: &str,
    _job_id: &str,
    stage: &str,
    error: &str,
) {
    let classified = classify_error(error);
    let error_code = format!("{}_{}", stage, classified.code);

    let _ = repo::job_items::transition_job_item_status(
        pool,
        item_id,
        JobItemStatus::Failed,
        None,
        Some(&error_code),
    )
    .await;

    // Programar reintento si es reintentable
    if classified.retryable {
        let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM job_items WHERE id = ?")
            .bind(item_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        let delay_secs = compute_backoff(attempts as u32);
        let next_attempt = (Utc::now() + delay_secs).to_rfc3339();

        let _ = sqlx::query(
            "UPDATE job_items SET status = 'queued', attempts = attempts + 1, next_attempt_at = ?, error_code = ? WHERE id = ?",
        )
        .bind(&next_attempt)
        .bind(&error_code)
        .bind(item_id)
        .execute(pool)
        .await;
    }
}

/// Clasifica un mensaje de error para decidir si es reintentable.
fn classify_error(error: &str) -> ClassifiedError {
    let lower = error.to_ascii_lowercase();
    if lower.contains("network")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("temporary")
    {
        ClassifiedError {
            code: "network_error".into(),
            retryable: true,
        }
    } else if lower.contains("private")
        || lower.contains("deleted")
        || lower.contains("unavailable")
        || lower.contains("not available")
    {
        ClassifiedError {
            code: "video_unavailable".into(),
            retryable: false,
        }
    } else if lower.contains("rate") || lower.contains("429") {
        ClassifiedError {
            code: "rate_limited".into(),
            retryable: true,
        }
    } else if lower.contains("cancel") {
        ClassifiedError {
            code: "cancelled".into(),
            retryable: false,
        }
    } else {
        ClassifiedError {
            code: "unknown".into(),
            retryable: false,
        }
    }
}

/// Calcula delay de backoff exponencial con jitter.
fn compute_backoff(attempt: u32) -> chrono::Duration {
    let base = 2u64;
    let max_delay = 300u64;
    let delay = base.saturating_pow(attempt).min(max_delay);
    let jitter = rand::random::<u64>() % 1000;
    chrono::Duration::milliseconds(((delay * 1000) + jitter) as i64)
}

/// Verifica si todos los items de un job terminaron y actualiza el estado del job.
async fn check_job_completion(pool: &SqlitePool, job_id: &str) {
    let items = match repo::job_items::list_job_items_by_job(pool, job_id).await {
        Ok(i) => i,
        Err(_) => return,
    };

    let all_done = items.iter().all(|i| i.status.is_terminal());
    if !all_done {
        return;
    }

    let any_failed = items.iter().any(|i| i.status == JobItemStatus::Failed);
    let any_cancelled = items
        .iter()
        .any(|i| i.status == JobItemStatus::Cancelled);

    let new_status = if any_cancelled && !any_failed {
        JobStatus::Cancelled
    } else if any_failed {
        JobStatus::CompletedWithErrors
    } else {
        JobStatus::Completed
    };

    let _ = transition_job_status(pool, job_id, new_status, Some("all_items_done")).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_error_network_retryable() {
        let result = classify_error("network timeout occurred");
        assert!(result.retryable);
        assert_eq!(result.code, "network_error");
    }

    #[test]
    fn test_classify_error_private_not_retryable() {
        let result = classify_error("This video is private");
        assert!(!result.retryable);
        assert_eq!(result.code, "video_unavailable");
    }

    #[test]
    fn test_classify_error_rate_limited() {
        let result = classify_error("HTTP 429 rate limited");
        assert!(result.retryable);
        assert_eq!(result.code, "rate_limited");
    }

    #[test]
    fn test_classify_error_unknown() {
        let result = classify_error("something weird happened");
        assert!(!result.retryable);
        assert_eq!(result.code, "unknown");
    }

    #[test]
    fn test_compute_backoff_respects_max() {
        let delay = compute_backoff(100);
        assert!(delay.num_milliseconds() <= 301000); // max 300s + jitter
    }

    #[test]
    fn test_compute_backoff_exponential() {
        let d0 = compute_backoff(0);
        let d1 = compute_backoff(1);
        let d2 = compute_backoff(2);
        // 2^0=1s, 2^1=2s, 2^2=4s (+ jitter 0-999ms)
        let ms0 = d0.num_milliseconds();
        let ms1 = d1.num_milliseconds();
        let ms2 = d2.num_milliseconds();
        assert!(ms0 >= 1000 && ms0 <= 2000, "ms0={}", ms0);
        assert!(ms1 >= 2000 && ms1 <= 3000, "ms1={}", ms1);
        assert!(ms2 >= 4000 && ms2 <= 5000, "ms2={}", ms2);
    }
}

#[cfg(test)]
mod integration_tests;
