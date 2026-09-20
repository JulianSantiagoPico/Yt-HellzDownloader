use super::*;
use sqlx::SqlitePool;

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn insert_test_job(pool: &SqlitePool, id: &str, priority: i32, status: &str) {
    sqlx::query(
        "INSERT INTO jobs (id, kind, status, priority, source_url, output_directory, format_profile, existing_file_policy, created_at) VALUES (?, 'import', ?, ?, 'https://example.com', '/tmp', 'mp3_192', 'rename', '2026-01-01 00:00:00')"
    )
    .bind(id)
    .bind(status)
    .bind(priority)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_track(pool: &SqlitePool, id: &str, video_id: &str, title: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO tracks (id, youtube_video_id, source_url, title, artist, channel, availability, created_at, updated_at) VALUES (?, ?, 'https://example.com', ?, 'Artist', 'Channel', 'available', '2026-01-01 00:00:00', '2026-01-01 00:00:00')"
    )
    .bind(id)
    .bind(video_id)
    .bind(title)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_test_item(
    pool: &SqlitePool,
    id: &str,
    job_id: &str,
    track_id: &str,
    status: &str,
    position: i32,
) {
    sqlx::query(
        "INSERT INTO job_items (id, job_id, track_id, playlist_position, status, priority_offset, progress_percent, downloaded_bytes, attempts, created_at) VALUES (?, ?, ?, ?, ?, 0, 0.0, 0, 0, '2026-01-01 00:00:00')"
    )
    .bind(id)
    .bind(job_id)
    .bind(track_id)
    .bind(position)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_select_ready_items_by_priority() {
    let pool = setup_test_db().await;

    // 3 jobs con distintas prioridades
    insert_test_job(&pool, "job-low", 0, "running").await;
    insert_test_job(&pool, "job-high", 10, "running").await;
    insert_test_job(&pool, "job-mid", 5, "running").await;

    // 1 item por job, todos queued
    insert_test_track(&pool, "track-low", "vid-low", "Low").await;
    insert_test_track(&pool, "track-high", "vid-high", "High").await;
    insert_test_track(&pool, "track-mid", "vid-mid", "Mid").await;

    insert_test_item(&pool, "item-low", "job-low", "track-low", "queued", 1).await;
    insert_test_item(&pool, "item-high", "job-high", "track-high", "queued", 1).await;
    insert_test_item(&pool, "item-mid", "job-mid", "track-mid", "queued", 1).await;

    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        3,
        3,
    );

    let items = scheduler.select_ready_items().await;
    assert_eq!(items.len(), 3);
    // Primero el de mayor prioridad (job-high, priority=10)
    assert_eq!(items[0].job_id, "job-high");
    assert_eq!(items[1].job_id, "job-mid");
    assert_eq!(items[2].job_id, "job-low");
}

#[tokio::test]
async fn test_select_ready_items_respects_download_concurrency_limit() {
    let pool = setup_test_db().await;

    // 1 job con 5 items
    insert_test_job(&pool, "job1", 0, "running").await;
    for i in 0..5 {
        let track_id = format!("track-{}", i);
        let item_id = format!("item-{}", i);
        insert_test_track(&pool, &track_id, &format!("vid-{}", i), &format!("Title {}", i)).await;
        insert_test_item(&pool, &item_id, "job1", &track_id, "queued", i).await;
    }

    // Scheduler con max_downloads = 2
    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        2,
        2,
    );

    let items = scheduler.select_ready_items().await;
    assert_eq!(items.len(), 2, "Solo debe devolver 2 items (límite de concurrencia)");
}

#[tokio::test]
async fn test_select_ready_items_skips_paused_jobs() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job-paused", 10, "paused").await;
    insert_test_job(&pool, "job-running", 0, "running").await;

    insert_test_track(&pool, "track-paused", "vid-p", "Paused").await;
    insert_test_track(&pool, "track-running", "vid-r", "Running").await;

    insert_test_item(&pool, "item-paused", "job-paused", "track-paused", "queued", 1).await;
    insert_test_item(&pool, "item-running", "job-running", "track-running", "queued", 1).await;

    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        5,
        5,
    );

    let items = scheduler.select_ready_items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].job_id, "job-running");
}

#[tokio::test]
async fn test_select_ready_items_skips_non_queued_items() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;

    insert_test_track(&pool, "track-a", "vid-a", "A").await;
    insert_test_track(&pool, "track-b", "vid-b", "B").await;
    insert_test_track(&pool, "track-c", "vid-c", "C").await;

    insert_test_item(&pool, "item-a", "job1", "track-a", "queued", 1).await;
    insert_test_item(&pool, "item-b", "job1", "track-b", "downloading", 2).await;
    insert_test_item(&pool, "item-c", "job1", "track-c", "completed", 3).await;

    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        5,
        5,
    );

    let items = scheduler.select_ready_items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "item-a");
}

#[tokio::test]
async fn test_select_ready_items_same_priority_uses_playlist_position() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;

    insert_test_track(&pool, "track-1", "vid-1", "First").await;
    insert_test_track(&pool, "track-2", "vid-2", "Second").await;
    insert_test_track(&pool, "track-3", "vid-3", "Third").await;

    // Insertar items en orden invertido de posición
    insert_test_item(&pool, "item-3", "job1", "track-3", "queued", 3).await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "queued", 1).await;
    insert_test_item(&pool, "item-2", "job1", "track-2", "queued", 2).await;

    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        5,
        5,
    );

    let items = scheduler.select_ready_items().await;
    assert_eq!(items.len(), 3);
    // Deben venir ordenados por playlist_position ASC
    assert_eq!(items[0].id, "item-1");
    assert_eq!(items[1].id, "item-2");
    assert_eq!(items[2].id, "item-3");
}

#[tokio::test]
async fn test_select_ready_items_skips_cancelling_jobs() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job-cancelling", 10, "cancelling").await;
    insert_test_job(&pool, "job-queued", 0, "queued").await;

    insert_test_track(&pool, "track-c", "vid-c", "Cancelling").await;
    insert_test_track(&pool, "track-q", "vid-q", "Queued").await;

    insert_test_item(&pool, "item-c", "job-cancelling", "track-c", "queued", 1).await;
    insert_test_item(&pool, "item-q", "job-queued", "track-q", "queued", 1).await;

    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        5,
        5,
    );

    let items = scheduler.select_ready_items().await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].job_id, "job-queued");
}

#[tokio::test]
async fn test_select_ready_items_empty_when_no_permits() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "queued", 1).await;

    // Scheduler con max_downloads = 0 (se ajusta a 1 en new)
    // Simulamos: consumir todos los permits
    let scheduler = DownloadScheduler::new(
        pool.clone(),
        std::path::PathBuf::from("/tmp"),
        1,
        1,
    );

    // Consumir el permit disponible
    let _permit = scheduler.download_semaphore.clone().acquire_owned().await.unwrap();

    let items = scheduler.select_ready_items().await;
    assert!(items.is_empty(), "No debe devolver items cuando no hay permits");
}

#[tokio::test]
async fn test_check_job_completion_marks_completed() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "completed", 1).await;

    check_job_completion(&pool, "job1").await;

    let job = sqlx::query_scalar::<_, String>("SELECT status FROM jobs WHERE id = ?")
        .bind("job1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(job, "completed");
}

#[tokio::test]
async fn test_check_job_completion_marks_with_errors() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_track(&pool, "track-2", "vid-2", "Title2").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "completed", 1).await;
    insert_test_item(&pool, "item-2", "job1", "track-2", "failed", 2).await;

    check_job_completion(&pool, "job1").await;

    let job = sqlx::query_scalar::<_, String>("SELECT status FROM jobs WHERE id = ?")
        .bind("job1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(job, "completed_with_errors");
}

#[tokio::test]
async fn test_check_job_completion_no_action_if_not_all_done() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_track(&pool, "track-2", "vid-2", "Title2").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "completed", 1).await;
    insert_test_item(&pool, "item-2", "job1", "track-2", "downloading", 2).await;

    check_job_completion(&pool, "job1").await;

    let job = sqlx::query_scalar::<_, String>("SELECT status FROM jobs WHERE id = ?")
        .bind("job1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(job, "running", "No debe cambiar si hay items activos");
}

#[tokio::test]
async fn test_handle_item_error_classifies_network_error() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "queued", 1).await;

    // Forzar estado a downloading para que handle_item_error pueda transicionar
    sqlx::query("UPDATE job_items SET status = 'downloading' WHERE id = ?")
        .bind("item-1")
        .execute(&pool)
        .await
        .unwrap();

    handle_item_error(&pool, "item-1", "job1", "download", "network timeout").await;

    let item = sqlx::query("SELECT status, error_code FROM job_items WHERE id = ?")
        .bind("item-1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let status: String = item.get("status");
    let error_code: Option<String> = item.get("error_code");

    // Network errors are retryable, so item gets rescheduled to queued
    assert_eq!(status, "queued");
    assert!(error_code.unwrap().contains("network_error"));
}

#[tokio::test]
async fn test_handle_item_error_retryable_reschedules() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "queued", 1).await;

    sqlx::query("UPDATE job_items SET status = 'downloading' WHERE id = ?")
        .bind("item-1")
        .execute(&pool)
        .await
        .unwrap();

    handle_item_error(&pool, "item-1", "job1", "download", "connection refused").await;

    // Debe volver a queued con next_attempt_at (porque es retryable)
    let item = sqlx::query("SELECT status, next_attempt_at FROM job_items WHERE id = ?")
        .bind("item-1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let status: String = item.get("status");
    let next_attempt: Option<String> = item.get("next_attempt_at");

    assert_eq!(status, "queued", "Reintentable debe volver a queued");
    assert!(next_attempt.is_some(), "Debe programar next_attempt_at");
}

#[tokio::test]
async fn test_handle_item_error_not_retryable_stays_failed() {
    let pool = setup_test_db().await;

    insert_test_job(&pool, "job1", 0, "running").await;
    insert_test_track(&pool, "track-1", "vid-1", "Title").await;
    insert_test_item(&pool, "item-1", "job1", "track-1", "queued", 1).await;

    sqlx::query("UPDATE job_items SET status = 'downloading' WHERE id = ?")
        .bind("item-1")
        .execute(&pool)
        .await
        .unwrap();

    handle_item_error(&pool, "item-1", "job1", "download", "This video is private").await;

    let item = sqlx::query("SELECT status, next_attempt_at FROM job_items WHERE id = ?")
        .bind("item-1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let status: String = item.get("status");
    let next_attempt: Option<String> = item.get("next_attempt_at");

    assert_eq!(status, "failed");
    assert!(next_attempt.is_none(), "No debe programar reintento para errores no reintentables");
}
