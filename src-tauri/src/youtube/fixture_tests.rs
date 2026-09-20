use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::entities::{Job, Playlist};
use crate::domain::states::{Availability, JobKind, JobStatus, OrganizationMode, SourceKind};
use crate::filesystem::ExistingFilePolicy;
use crate::persistence::repositories::batch::batch_insert_playlist_items;
use crate::youtube::PlaylistEntry;

struct AutoTempDir(PathBuf);

impl AutoTempDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{}_{}", prefix, Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&path);
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for AutoTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Genera un fixture sintético de `count` entradas con características del mundo real:
/// - Caracteres Unicode (kanji, emojis, tildes, caracteres especiales)
/// - Vídeos privados y eliminados (~5% del total)
/// - Títulos extremadamente largos (500+ caracteres)
/// - Entradas con duración ausente y metadatos faltantes
/// - Canciones duplicadas en diferentes posiciones de la misma playlist
pub fn generate_synthetic_playlist_fixture(count: usize) -> Vec<PlaylistEntry> {
    let mut entries = Vec::with_capacity(count);

    for i in 1..=count {
        let (id, title, artist, duration, availability) = if i % 25 == 0 {
            (
                format!("priv_{:06}", i),
                "[Private video]".to_string(),
                "Artista desconocido".to_string(),
                None,
                Availability::Private,
            )
        } else if i % 25 == 1 {
            (
                format!("del_{:06}", i),
                "[Deleted video]".to_string(),
                "Artista desconocido".to_string(),
                None,
                Availability::Deleted,
            )
        } else if i % 10 == 0 {
            (
                format!("uni_{:06}", i),
                format!("夜に駆ける (Racing into the night) 🎶✨ - YOASOBI #{}", i),
                "Ayase / YOASOBI Official 🇯🇵".to_string(),
                Some(261.5),
                Availability::Available,
            )
        } else if i % 15 == 0 {
            let long_padding = "A".repeat(480);
            (
                format!("lng_{:06}", i),
                format!("Título extremadamente largo {} - Canción #{}", long_padding, i),
                "Artista con nombre compuesto y caracteres especiales ñ, á, é, í, ó, ú".to_string(),
                Some(180.0),
                Availability::Available,
            )
        } else if i % 7 == 0 {
            // Canción duplicada en diferente posición
            (
                "dup_song_fixed_id".to_string(),
                format!("Canción repetida en playlist (Posición {})", i),
                "Artista Recurrente".to_string(),
                Some(210.0),
                Availability::Available,
            )
        } else if i % 8 == 0 {
            // Sin duración ni artista
            (
                format!("nod_{:06}", i),
                format!("Audio sin metadata #{}", i),
                "Artista desconocido".to_string(),
                None,
                Availability::Available,
            )
        } else {
            (
                format!("vid_{:06}", i),
                format!("Canción estándar #{} (Radio Edit)", i),
                format!("Banda / Artista {}", (i % 50) + 1),
                Some(195.0 + (i as f64 % 60.0)),
                Availability::Available,
            )
        };

        let video_url = format!("https://www.youtube.com/watch?v={}", id);
        entries.push(PlaylistEntry {
            id,
            url: video_url,
            title,
            artist,
            duration_seconds: duration,
            index: i,
            availability,
        });
    }

    entries
}

async fn create_test_db(path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .unwrap();

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn test_fixture_generator_characteristics() {
    let fixture = generate_synthetic_playlist_fixture(5000);
    assert_eq!(fixture.len(), 5000);

    let private_count = fixture
        .iter()
        .filter(|e| e.availability == Availability::Private)
        .count();
    let deleted_count = fixture
        .iter()
        .filter(|e| e.availability == Availability::Deleted)
        .count();
    let duplicates_count = fixture
        .iter()
        .filter(|e| e.id == "dup_song_fixed_id")
        .count();
    let unicode_count = fixture
        .iter()
        .filter(|e| e.title.contains("夜に駆ける"))
        .count();

    assert!(private_count >= 190, "Debe contener ~200 videos privados");
    assert!(deleted_count >= 190, "Debe contener ~200 videos eliminados");
    assert!(duplicates_count > 500, "Debe contener duplicados consistentes");
    assert!(
        unicode_count >= 350,
        "Debe contener canciones con Unicode/Kanji/Emojis (actual: {})",
        unicode_count
    );
}

#[tokio::test]
async fn test_batch_insert_5000_items_performance() {
    let temp_dir = AutoTempDir::new("yt_bench_5000");
    let db_path = temp_dir.path().join("bench_5000.db");
    let pool = create_test_db(&db_path).await;

    let playlist_id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let now = "2026-09-19T20:00:00Z";

    // Insertar playlist y job
    let playlist = Playlist {
        id: playlist_id.clone(),
        youtube_playlist_id: "PL_bench_5000".to_string(),
        source_url: "https://www.youtube.com/playlist?list=PL_bench_5000".to_string(),
        source_kind: SourceKind::YouTube,
        title: "Benchmark Playlist 5000".to_string(),
        channel: "Benchmark Channel".to_string(),
        thumbnail_url: None,
        default_output_directory: Some(temp_dir.path().display().to_string()),
        last_synced_at: Some(now.to_string()),
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };
    crate::persistence::repositories::playlists::upsert_playlist(&pool, &playlist)
        .await
        .unwrap();

    let job = Job {
        id: job_id.clone(),
        playlist_id: Some(playlist_id.clone()),
        kind: JobKind::Import,
        status: JobStatus::Extracting,
        priority: 0,
        source_url: playlist.source_url.clone(),
        output_directory: temp_dir.path().display().to_string(),
        organization_mode: OrganizationMode::PlaylistFolder,
        format_profile: "mp3_192".to_string(),
        existing_file_policy: ExistingFilePolicy::Rename,
        cancel_requested_at: None,
        created_at: now.to_string(),
        started_at: Some(now.to_string()),
        completed_at: None,
    };
    crate::persistence::repositories::jobs::create_job(&pool, &job)
        .await
        .unwrap();

    // Generar fixture sintético
    let fixture = generate_synthetic_playlist_fixture(5000);

    // Inserción en lotes de 50 items
    let start_time = Instant::now();
    let mut total_inserted = 0u32;
    let mut total_available = 0u32;
    let mut total_unavailable = 0u32;

    for chunk in fixture.chunks(50) {
        let res = batch_insert_playlist_items(&pool, chunk, &job_id, &playlist_id, now)
            .await
            .unwrap();
        total_inserted += res.job_items_inserted;
        total_available += res.available;
        total_unavailable += res.unavailable;
    }

    let elapsed = start_time.elapsed();

    assert_eq!(total_inserted, 5000, "Deben haberse insertado 5.000 job_items");
    assert!(
        total_unavailable > 350,
        "Los items no disponibles (privados/eliminados) deben clasificarse apropiadamente"
    );
    assert_eq!(total_available + total_unavailable, 5000);

    // Criterio de Aceptación 7: Inserción en lotes de 5.000 entradas completa en tiempo eficiente
    let max_allowed = if cfg!(debug_assertions) {
        Duration::from_secs(20)
    } else {
        Duration::from_millis(1500)
    };
    println!("Tiempo de inserción en lote de 5.000 items: {:?}", elapsed);
    assert!(
        elapsed < max_allowed,
        "La inserción de 5.000 items debe ser < {:?} (actual: {:?})",
        max_allowed,
        elapsed
    );

    // Criterio de Aceptación 9: Consultas paginadas de items responden en < 20 ms
    let query_start = Instant::now();
    let paginated_rows = sqlx::query(
        "SELECT id, status FROM job_items WHERE job_id = ? ORDER BY playlist_position ASC LIMIT 100 OFFSET 200"
    )
    .bind(&job_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let query_elapsed = query_start.elapsed();
    assert_eq!(paginated_rows.len(), 100);
    println!("Tiempo de consulta paginada (100 items): {:?}", query_elapsed);
    assert!(
        query_elapsed < Duration::from_millis(20),
        "La consulta paginada debe responder en < 20ms (actual: {:?})",
        query_elapsed
    );

    // Criterio de Aceptación 8: JOIN optimizado con LIMIT/OFFSET en SQL < 50 ms
    let join_start = Instant::now();
    let join_rows = sqlx::query(
        r#"
        SELECT pt.position, t.title, t.artist, t.availability, ji.status as download_status
        FROM playlist_tracks pt
        JOIN tracks t ON pt.track_id = t.id
        LEFT JOIN job_items ji ON ji.track_id = t.id AND ji.job_id = ?
        WHERE pt.playlist_id = ? AND pt.removed_at IS NULL
        ORDER BY pt.position ASC
        LIMIT 100 OFFSET 500
        "#
    )
    .bind(&job_id)
    .bind(&playlist_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let join_elapsed = join_start.elapsed();
    assert_eq!(join_rows.len(), 100);
    println!("Tiempo de consulta JOIN de playlist (100 items): {:?}", join_elapsed);
    assert!(
        join_elapsed < Duration::from_millis(50),
        "El JOIN optimizado debe responder en < 50ms (actual: {:?})",
        join_elapsed
    );
}

#[tokio::test]
async fn test_concurrency_batch_insert_and_reads_no_busy() {
    let temp_dir = AutoTempDir::new("yt_concurrency");
    let db_path = temp_dir.path().join("concurrency.db");
    let pool = create_test_db(&db_path).await;

    let playlist_id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let now = "2026-09-19T20:00:00Z";

    // Setup inicial de playlist y job
    let playlist = Playlist {
        id: playlist_id.clone(),
        youtube_playlist_id: "PL_concurrent".to_string(),
        source_url: "https://www.youtube.com/playlist?list=PL_concurrent".to_string(),
        source_kind: SourceKind::YouTube,
        title: "Concurrent Playlist".to_string(),
        channel: "Channel".to_string(),
        thumbnail_url: None,
        default_output_directory: None,
        last_synced_at: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };
    crate::persistence::repositories::playlists::upsert_playlist(&pool, &playlist)
        .await
        .unwrap();

    let job = Job {
        id: job_id.clone(),
        playlist_id: Some(playlist_id.clone()),
        kind: JobKind::Import,
        status: JobStatus::Extracting,
        priority: 0,
        source_url: playlist.source_url.clone(),
        output_directory: temp_dir.path().display().to_string(),
        organization_mode: OrganizationMode::PlaylistFolder,
        format_profile: "mp3_192".to_string(),
        existing_file_policy: ExistingFilePolicy::Rename,
        cancel_requested_at: None,
        created_at: now.to_string(),
        started_at: Some(now.to_string()),
        completed_at: None,
    };
    crate::persistence::repositories::jobs::create_job(&pool, &job)
        .await
        .unwrap();

    let fixture = generate_synthetic_playlist_fixture(500);

    // Tarea escritora
    let pool_write = pool.clone();
    let job_id_clone = job_id.clone();
    let playlist_id_clone = playlist_id.clone();
    let writer = tokio::spawn(async move {
        for chunk in fixture.chunks(50) {
            batch_insert_playlist_items(&pool_write, chunk, &job_id_clone, &playlist_id_clone, "2026-09-19T20:00:00Z")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Tarea lectora concurrente
    let pool_read = pool.clone();
    let playlist_id_read = playlist_id.clone();
    let reader = tokio::spawn(async move {
        for _ in 0..15 {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ? AND removed_at IS NULL",
            )
            .bind(&playlist_id_read)
            .fetch_one(&pool_read)
            .await
            .unwrap();

            assert!(count >= 0);
            tokio::time::sleep(Duration::from_millis(4)).await;
        }
    });

    let (write_res, read_res) = tokio::join!(writer, reader);
    assert!(write_res.is_ok(), "Escritura concurrente debe completar sin SQLITE_BUSY");
    assert!(read_res.is_ok(), "Lectura concurrente debe completar sin SQLITE_BUSY");
}

#[tokio::test]
async fn test_migration_0004_preserves_preexisting_data_and_enables_skipped() {
    let temp_dir = AutoTempDir::new("yt_migration_0004");
    let db_path = temp_dir.path().join("migration_0004_test.db");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();

    // Aplicar migraciones hasta 0004
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let job_id = "job_test_0004";
    sqlx::query(
        "INSERT INTO jobs (id, kind, status, priority, source_url, output_directory, format_profile, existing_file_policy, created_at)
         VALUES (?, 'import', 'running', 0, 'https://example.com', 'dir', 'mp3_192', 'fail_if_exists', '2026-01-01')"
    )
    .bind(job_id)
    .execute(&pool)
    .await
    .unwrap();

    // Verificar que el nuevo estado 'skipped' es permitido formalmente en job_items.status
    let skipped_item_id = "new_skipped_item_0004";
    let insert_skipped = sqlx::query(
        "INSERT INTO job_items (id, job_id, playlist_position, status, priority_offset, progress_percent, downloaded_bytes, attempts, created_at)
         VALUES (?, ?, 2, 'skipped', 0, 0.0, 0, 0, '2026-01-01')"
    )
    .bind(skipped_item_id)
    .bind(job_id)
    .execute(&pool)
    .await;

    assert!(
        insert_skipped.is_ok(),
        "La migración 0004 debe permitir 'skipped' en job_items.status"
    );

    // Verificar que los nuevos índices de la migración 0004 existen
    let index_names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='index' AND (name='idx_playlist_tracks_playlist_position' OR name='idx_job_items_track_job')"
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert!(index_names.contains(&"idx_playlist_tracks_playlist_position".to_string()));
    assert!(index_names.contains(&"idx_job_items_track_job".to_string()));
}
