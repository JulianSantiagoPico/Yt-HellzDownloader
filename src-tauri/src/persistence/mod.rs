pub mod recovery;
pub mod repositories;

use std::path::{Path, PathBuf};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};

pub use recovery::*;
pub use repositories::*;

/// Crea una copia de seguridad local del archivo de base de datos antes de aplicar migraciones
/// si el archivo ya existe físicamente en disco y tiene contenido.
pub fn create_pre_migration_backup(db_path: &Path) -> Result<Option<PathBuf>, std::io::Error> {
    if db_path.exists() {
        let meta = std::fs::metadata(db_path)?;
        if meta.len() > 0 {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let filename = db_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("app.db");
            let backup_path = db_path.with_file_name(format!("{}.bak_{}", filename, timestamp));
            std::fs::copy(db_path, &backup_path)?;
            return Ok(Some(backup_path));
        }
    }
    Ok(None)
}

pub async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    // Si la base de datos ya existe con datos, realizar backup preventivo
    let _ = create_pre_migration_backup(path);

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        entities::{Job, JobItem},
        states::{JobItemStatus, JobKind, JobStatus, OrganizationMode},
    };
    use crate::filesystem::ExistingFilePolicy;
    use sqlx::Row;
    use uuid::Uuid;

    #[tokio::test]
    async fn migrations_apply_to_memory_database() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Verificar tablas de fase 0 y fase 2
        let spike_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM spike_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(spike_count, 0);

        let jobs_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(jobs_count, 0);

        let settings_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(settings_count >= 5);
    }

    #[test]
    fn test_pre_migration_backup_creation() {
        let temp_dir = std::env::temp_dir().join(format!("yt_test_bak_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_file = temp_dir.join("test_app.db");

        // Archivo no existe -> None
        assert_eq!(create_pre_migration_backup(&db_file).unwrap(), None);

        // Archivo vacío -> None
        std::fs::write(&db_file, b"").unwrap();
        assert_eq!(create_pre_migration_backup(&db_file).unwrap(), None);

        // Archivo con contenido -> Se crea backup
        std::fs::write(&db_file, b"SQLite format 3\0").unwrap();
        let backup = create_pre_migration_backup(&db_file).unwrap();
        assert!(backup.is_some());
        let backup_path = backup.unwrap();
        assert!(backup_path.exists());
        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            b"SQLite format 3\0".to_vec()
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_job_and_item_persistence_and_atomic_transitions() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let job_id = Uuid::new_v4().to_string();
        let job = Job {
            id: job_id.clone(),
            playlist_id: None,
            kind: JobKind::SingleDownload,
            status: JobStatus::Created,
            priority: 1,
            source_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            output_directory: "C:\\Downloads".into(),
            organization_mode: OrganizationMode::PlaylistFolder,
            format_profile: "mp3_192".into(),
            existing_file_policy: ExistingFilePolicy::Ask,
            cancel_requested_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };

        let created_job = create_job(&pool, &job).await.unwrap();
        assert_eq!(created_job.id, job_id);

        let retrieved = get_job(&pool, &job_id).await.unwrap().unwrap();
        assert_eq!(retrieved.status, JobStatus::Created);

        // Transicionar Job: Created -> Queued -> Running
        transition_job_status(&pool, &job_id, JobStatus::Queued, None)
            .await
            .unwrap();

        let new_status =
            transition_job_status(&pool, &job_id, JobStatus::Running, Some("test_start"))
                .await
                .unwrap();
        assert_eq!(new_status, JobStatus::Running);

        // Idempotencia: transicionar nuevamente a Running es inocuo
        let idempotent_status = transition_job_status(&pool, &job_id, JobStatus::Running, None)
            .await
            .unwrap();
        assert_eq!(idempotent_status, JobStatus::Running);

        // Crear JobItem
        let item_id = Uuid::new_v4().to_string();
        let item = JobItem {
            id: item_id.clone(),
            job_id: job_id.clone(),
            track_id: None,
            playlist_track_id: None,
            playlist_position: Some(1),
            status: JobItemStatus::Pending,
            priority_offset: 0,
            progress_percent: Some(0.0),
            downloaded_bytes: Some(0),
            estimated_total_bytes: Some(1024),
            attempts: 0,
            next_attempt_at: None,
            temporary_path: None,
            output_path: None,
            error_code: None,
            error_message: None,
            execution_lease_expires_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };

        create_job_item(&pool, &item).await.unwrap();

        // Transición Item: Pending -> Queued -> Downloading
        transition_job_item_status(&pool, &item_id, JobItemStatus::Queued, None, None)
            .await
            .unwrap();

        let active_status = transition_job_item_status(
            &pool,
            &item_id,
            JobItemStatus::Downloading,
            Some(45),
            Some("starting download"),
        )
        .await
        .unwrap();
        assert_eq!(active_status, JobItemStatus::Downloading);

        // Verificar que el evento de actividad quedó registrado en BD
        let events_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM activity_events WHERE entity_id = ? AND event_type = 'status_transition'",
        )
        .bind(&item_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(events_count, 2); // Queued y Downloading
    }

    #[tokio::test]
    async fn test_exclusive_reservations_and_waiting_for_duplicate() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Crear dos jobs e ítems para el mismo vídeo
        let job1 = Job {
            id: Uuid::new_v4().to_string(),
            playlist_id: None,
            kind: JobKind::SingleDownload,
            status: JobStatus::Running,
            priority: 0,
            source_url: "url1".into(),
            output_directory: "dir".into(),
            organization_mode: OrganizationMode::Flat,
            format_profile: "mp3_192".into(),
            existing_file_policy: ExistingFilePolicy::Reuse,
            cancel_requested_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };
        create_job(&pool, &job1).await.unwrap();

        let item1_id = Uuid::new_v4().to_string();
        let item1 = JobItem {
            id: item1_id.clone(),
            job_id: job1.id.clone(),
            track_id: None,
            playlist_track_id: None,
            playlist_position: Some(1),
            status: JobItemStatus::Downloading,
            priority_offset: 0,
            progress_percent: None,
            downloaded_bytes: None,
            estimated_total_bytes: None,
            attempts: 0,
            next_attempt_at: None,
            temporary_path: None,
            output_path: None,
            error_code: None,
            error_message: None,
            execution_lease_expires_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };
        create_job_item(&pool, &item1).await.unwrap();

        let item2_id = Uuid::new_v4().to_string();
        let item2 = JobItem {
            id: item2_id.clone(),
            job_id: job1.id.clone(),
            track_id: None,
            playlist_track_id: None,
            playlist_position: Some(2),
            status: JobItemStatus::Pending,
            priority_offset: 0,
            progress_percent: None,
            downloaded_bytes: None,
            estimated_total_bytes: None,
            attempts: 0,
            next_attempt_at: None,
            temporary_path: None,
            output_path: None,
            error_code: None,
            error_message: None,
            execution_lease_expires_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };
        create_job_item(&pool, &item2).await.unwrap();

        let video_id = "dQw4w9WgXcQ";
        let profile = "mp3_192";

        // Ítem 1 adquiere la reserva exclusiva por 60 segundos
        let res1 = acquire_or_wait_reservation(&pool, video_id, profile, &item1_id, 60)
            .await
            .unwrap();
        assert_eq!(res1, AcquireReservationResult::Acquired);

        // Ítem 2 intenta adquirir la misma reserva: recibe WaitingForDuplicate
        let res2 = acquire_or_wait_reservation(&pool, video_id, profile, &item2_id, 60)
            .await
            .unwrap();
        assert_eq!(
            res2,
            AcquireReservationResult::WaitingForDuplicate {
                owner_job_item_id: item1_id.clone()
            }
        );

        // Ítem 1 libera la reserva
        let released = release_reservation(&pool, video_id, profile, &item1_id)
            .await
            .unwrap();
        assert!(released);

        // Ahora Ítem 2 puede adquirirla
        let res3 = acquire_or_wait_reservation(&pool, video_id, profile, &item2_id, 60)
            .await
            .unwrap();
        assert_eq!(res3, AcquireReservationResult::Acquired);
    }

    #[tokio::test]
    async fn test_migration_0003_allows_fail_if_exists_policy() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Insertar job con fail_if_exists — debe ser aceptado tras la migración 0003
        let job_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO jobs (id, kind, status, source_url, output_directory, existing_file_policy) VALUES (?, 'import', 'created', 'https://youtube.com/test', 'C:\\temp', 'fail_if_exists')")
            .bind(&job_id)
            .execute(&pool)
            .await
            .expect("INSERT con fail_if_exists debe funcionar tras migración 0003");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE existing_file_policy = 'fail_if_exists'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);

        // Verificar que la política por defecto sigue siendo 'ask'
        let default_policy: String = sqlx::query_scalar("SELECT existing_file_policy FROM jobs WHERE id = ?")
            .bind(&job_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(default_policy, "fail_if_exists");
    }

    #[tokio::test]
    async fn test_migration_0003_creates_required_indexes() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Verificar que los índices existen
        let idx_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_jobs_playlist_id', 'idx_job_items_job_status', 'idx_local_files_format_profile')"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(idx_count, 3, "Los 3 índices nuevos deben existir tras migración 0003");
    }

    #[tokio::test]
    async fn test_migration_0003_preserves_existing_jobs() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Insertar un job antes del test
        let job_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO jobs (id, kind, status, source_url, output_directory) VALUES (?, 'import', 'created', 'https://youtube.com/old', 'C:\\old')")
            .bind(&job_id)
            .execute(&pool)
            .await
            .unwrap();

        // Verificar que el dato persiste
        let retrieved: String = sqlx::query_scalar("SELECT source_url FROM jobs WHERE id = ?")
            .bind(&job_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retrieved, "https://youtube.com/old");
    }

    #[tokio::test]
    async fn test_migration_0003_invalid_policy_rejected() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Intentar insertar un valor inválido en existing_file_policy
        let result = sqlx::query("INSERT INTO jobs (id, kind, status, source_url, output_directory, existing_file_policy) VALUES (?, 'import', 'created', 'url', 'dir', 'invalid_policy')")
            .bind(Uuid::new_v4().to_string())
            .execute(&pool)
            .await;
        assert!(result.is_err(), "Políticas inválidas deben ser rechazadas por CHECK");
    }

    #[tokio::test]
    async fn test_recovery_detects_existing_temporary_files() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Crear un archivo temporal real
        let temp_dir = std::env::temp_dir().join(format!("yt_test_recovery_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(temp_dir.join("video.tmp"), b"fake data").unwrap();

        let job_id = Uuid::new_v4().to_string();
        let job = Job {
            id: job_id.clone(),
            playlist_id: None,
            kind: JobKind::Import,
            status: JobStatus::Running,
            priority: 0,
            source_url: "playlist_url".into(),
            output_directory: "dir".into(),
            organization_mode: OrganizationMode::PlaylistFolder,
            format_profile: "mp3_192".into(),
            existing_file_policy: ExistingFilePolicy::Ask,
            cancel_requested_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };
        create_job(&pool, &job).await.unwrap();

        let item_id = Uuid::new_v4().to_string();
        let item = JobItem {
            id: item_id.clone(),
            job_id: job_id.clone(),
            track_id: None,
            playlist_track_id: None,
            playlist_position: Some(1),
            status: JobItemStatus::Downloading,
            priority_offset: 0,
            progress_percent: Some(50.0),
            downloaded_bytes: Some(500),
            estimated_total_bytes: Some(1000),
            attempts: 1,
            next_attempt_at: None,
            temporary_path: Some(temp_dir.to_string_lossy().to_string()),
            output_path: None,
            error_code: None,
            error_message: None,
            execution_lease_expires_at: Some("2026-09-14 19:00:00".into()),
            created_at: "2026-09-14 18:50:00".into(),
            started_at: Some("2026-09-14 18:51:00".into()),
            completed_at: None,
        };
        create_job_item(&pool, &item).await.unwrap();

        let report = recover_on_startup(&pool).await.unwrap();
        assert_eq!(report.interrupted_items_count, 1);

        let detail = &report.interrupted_items_details[0];
        assert_eq!(detail.item_id, item_id);
        assert!(detail.has_temporary_files, "Debe detectar archivos temporales existentes");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_recovery_detects_no_temporary_files_when_missing() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let job_id = Uuid::new_v4().to_string();
        let job = Job {
            id: job_id.clone(),
            playlist_id: None,
            kind: JobKind::Import,
            status: JobStatus::Running,
            priority: 0,
            source_url: "playlist_url".into(),
            output_directory: "dir".into(),
            organization_mode: OrganizationMode::PlaylistFolder,
            format_profile: "mp3_192".into(),
            existing_file_policy: ExistingFilePolicy::Ask,
            cancel_requested_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };
        create_job(&pool, &job).await.unwrap();

        let item_id = Uuid::new_v4().to_string();
        let item = JobItem {
            id: item_id.clone(),
            job_id: job_id.clone(),
            track_id: None,
            playlist_track_id: None,
            playlist_position: Some(1),
            status: JobItemStatus::Converting,
            priority_offset: 0,
            progress_percent: Some(50.0),
            downloaded_bytes: Some(500),
            estimated_total_bytes: Some(1000),
            attempts: 1,
            next_attempt_at: None,
            temporary_path: Some("C:\\nonexistent\\path\\item.mp3".into()),
            output_path: None,
            error_code: None,
            error_message: None,
            execution_lease_expires_at: None,
            created_at: "2026-09-14 18:50:00".into(),
            started_at: Some("2026-09-14 18:51:00".into()),
            completed_at: None,
        };
        create_job_item(&pool, &item).await.unwrap();

        let report = recover_on_startup(&pool).await.unwrap();
        assert_eq!(report.interrupted_items_count, 1);

        let detail = &report.interrupted_items_details[0];
        assert!(!detail.has_temporary_files, "No debe reportar archivos temporales si la ruta no existe");
    }

    #[tokio::test]
    async fn test_index_job_items_job_status_used_in_scheduler_query() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Insertar datos de prueba
        let job_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO jobs (id, kind, status, priority, source_url, output_directory, format_profile, existing_file_policy, created_at) VALUES (?, 'import', 'running', 0, 'url', 'dir', 'mp3_192', 'rename', '2026-01-01 00:00:00')")
            .bind(&job_id)
            .execute(&pool)
            .await
            .unwrap();

        let track_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO tracks (id, youtube_video_id, source_url, title, artist, channel, availability, created_at, updated_at) VALUES (?, 'vid1', 'url', 'Title', 'Artist', 'Channel', 'available', '2026-01-01', '2026-01-01')")
            .bind(&track_id)
            .execute(&pool)
            .await
            .unwrap();

        let item_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO job_items (id, job_id, track_id, playlist_position, status, priority_offset, progress_percent, downloaded_bytes, attempts, created_at) VALUES (?, ?, ?, 1, 'queued', 0, 0.0, 0, 0, '2026-01-01')")
            .bind(&item_id)
            .bind(&job_id)
            .bind(&track_id)
            .execute(&pool)
            .await
            .unwrap();

        // Ejecutar la query del scheduler y verificar que usa el índice
        let plan_rows = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT ji.id, ji.job_id FROM job_items ji JOIN jobs j ON ji.job_id = j.id WHERE ji.status = 'queued' AND j.status IN ('queued', 'running')"
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let plan_text: String = plan_rows
            .iter()
            .map(|r| r.get::<String, _>("detail"))
            .collect::<Vec<_>>()
            .join(" ");

        // La query plan debe usar el índice idx_job_items_job_status
        assert!(
            plan_text.contains("idx_job_items_job_status") || plan_text.contains("USING INDEX"),
            "El plan debe usar idx_job_items_job_status, plan: {}",
            plan_text
        );
    }

    #[tokio::test]
    async fn test_index_local_files_format_profile_used() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Insertar local_files con distintos format_profile
        let track_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO tracks (id, youtube_video_id, source_url, title, artist, channel, availability, created_at, updated_at) VALUES (?, 'vid1', 'url', 'Title', 'Artist', 'Channel', 'available', '2026-01-01', '2026-01-01')")
            .bind(&track_id)
            .execute(&pool)
            .await
            .unwrap();

        for (i, profile) in ["mp3_128", "mp3_192", "mp3_320", "mp3_192"].iter().enumerate() {
            sqlx::query("INSERT INTO local_files (id, track_id, format_profile, path, size_bytes, modified_at, validation_status, validated_at, video_id_tag, created_at) VALUES (?, ?, ?, ?, 1024, '2026-01-01', 'valid', '2026-01-01', 'vid1', '2026-01-01')")
                .bind(Uuid::new_v4().to_string())
                .bind(&track_id)
                .bind(profile)
                .bind(format!("/tmp/file_{}.mp3", i))
                .execute(&pool)
                .await
                .unwrap();
        }

        // Verificar que el índice existe
        let idx_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'index' AND name = 'idx_local_files_format_profile'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(idx_exists, "El índice idx_local_files_format_profile debe existir");

        // Ejecutar query con filtro por format_profile
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM local_files WHERE format_profile = 'mp3_192'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 2, "Debe encontrar 2 archivos con mp3_192");
    }

    #[tokio::test]
    async fn test_index_jobs_playlist_id_used() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Crear playlist
        let playlist_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO playlists (id, youtube_playlist_id, source_url, source_kind, title, channel, created_at, updated_at) VALUES (?, 'yt_pl', 'url', 'youtube', 'My Playlist', 'Channel', '2026-01-01', '2026-01-01')")
            .bind(&playlist_id)
            .execute(&pool)
            .await
            .unwrap();

        // Insertar jobs con distintos playlist_id
        for i in 0..5 {
            sqlx::query("INSERT INTO jobs (id, kind, status, priority, source_url, output_directory, format_profile, existing_file_policy, playlist_id, created_at) VALUES (?, 'import', 'created', 0, 'url', 'dir', 'mp3_192', 'rename', ?, '2026-01-01')")
                .bind(Uuid::new_v4().to_string())
                .bind(if i == 0 { Some(&playlist_id) } else { None })
                .execute(&pool)
                .await
                .unwrap();
        }

        // Verificar que el índice existe
        let idx_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'index' AND name = 'idx_jobs_playlist_id'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(idx_exists, "El índice idx_jobs_playlist_id debe existir");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE playlist_id = ?"
        )
        .bind(&playlist_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_insert_job_with_all_valid_policies() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let policies = ["ask", "reuse", "overwrite", "rename", "fail_if_exists"];
        for policy in policies {
            let result = sqlx::query(
                "INSERT INTO jobs (id, kind, status, source_url, output_directory, existing_file_policy) VALUES (?, 'import', 'created', 'url', 'dir', ?)"
            )
            .bind(Uuid::new_v4().to_string())
            .bind(policy)
            .execute(&pool)
            .await;
            assert!(result.is_ok(), "Policy '{}' debe ser aceptada", policy);
        }

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 5);
    }

    #[tokio::test]
    async fn test_recovery_on_startup_idempotency_and_detection() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let job_id = Uuid::new_v4().to_string();
        let job = Job {
            id: job_id.clone(),
            playlist_id: None,
            kind: JobKind::Import,
            status: JobStatus::Running,
            priority: 0,
            source_url: "playlist_url".into(),
            output_directory: "dir".into(),
            organization_mode: OrganizationMode::PlaylistFolder,
            format_profile: "mp3_192".into(),
            existing_file_policy: ExistingFilePolicy::Ask,
            cancel_requested_at: None,
            created_at: "2026-09-14 20:00:00".into(),
            started_at: None,
            completed_at: None,
        };
        create_job(&pool, &job).await.unwrap();

        let item_id = Uuid::new_v4().to_string();
        let item = JobItem {
            id: item_id.clone(),
            job_id: job_id.clone(),
            track_id: None,
            playlist_track_id: None,
            playlist_position: Some(1),
            status: JobItemStatus::Downloading,
            priority_offset: 0,
            progress_percent: Some(50.0),
            downloaded_bytes: Some(500),
            estimated_total_bytes: Some(1000),
            attempts: 1,
            next_attempt_at: None,
            temporary_path: Some("C:\\temp\\item.mp3".into()),
            output_path: None,
            error_code: None,
            error_message: None,
            execution_lease_expires_at: Some("2026-09-14 19:00:00".into()), // vencido
            created_at: "2026-09-14 18:50:00".into(),
            started_at: Some("2026-09-14 18:51:00".into()),
            completed_at: None,
        };
        create_job_item(&pool, &item).await.unwrap();

        // 1. Ejecutar recuperación al inicio
        let report = recover_on_startup(&pool).await.unwrap();
        assert_eq!(report.interrupted_jobs_count, 1);
        assert_eq!(report.interrupted_items_count, 1);
        assert_eq!(report.interrupted_item_ids, vec![item_id.clone()]);
        assert_eq!(report.interrupted_job_ids, vec![job_id.clone()]);
        assert_eq!(report.interrupted_items_details.len(), 1);
        assert_eq!(report.interrupted_items_details[0].item_id, item_id);

        // Verificar que el estado del item cambió a Interrupted y el del Job a Paused
        let updated_item = get_job_item(&pool, &item_id).await.unwrap().unwrap();
        assert_eq!(updated_item.status, JobItemStatus::Interrupted);

        let updated_job = get_job(&pool, &job_id).await.unwrap().unwrap();
        assert_eq!(updated_job.status, JobStatus::Paused);

        // 2. Ejecutar recuperación por segunda vez para verificar idempotencia
        let report2 = recover_on_startup(&pool).await.unwrap();
        assert_eq!(report2.interrupted_jobs_count, 0);
        assert_eq!(report2.interrupted_items_count, 0);
    }
}
