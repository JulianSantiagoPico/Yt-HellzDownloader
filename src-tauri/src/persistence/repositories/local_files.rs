use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::domain::{entities::LocalFile, states::ValidationStatus};

pub async fn register_local_file(
    pool: &SqlitePool,
    file: &LocalFile,
) -> Result<LocalFile, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO local_files (
            id, track_id, playlist_track_id, format_profile, path,
            size_bytes, modified_at, validation_status, validated_at,
            video_id_tag, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(path) DO UPDATE SET
            track_id = excluded.track_id,
            playlist_track_id = COALESCE(excluded.playlist_track_id, local_files.playlist_track_id),
            format_profile = excluded.format_profile,
            size_bytes = excluded.size_bytes,
            modified_at = excluded.modified_at,
            validation_status = excluded.validation_status,
            validated_at = excluded.validated_at,
            video_id_tag = excluded.video_id_tag
        "#,
    )
    .bind(&file.id)
    .bind(&file.track_id)
    .bind(&file.playlist_track_id)
    .bind(&file.format_profile)
    .bind(&file.path)
    .bind(file.size_bytes)
    .bind(&file.modified_at)
    .bind(file.validation_status.as_str())
    .bind(&file.validated_at)
    .bind(&file.video_id_tag)
    .bind(&file.created_at)
    .execute(pool)
    .await?;

    Ok(file.clone())
}

fn map_local_file_row(r: &sqlx::sqlite::SqliteRow) -> LocalFile {
    let status_str: String = r.get("validation_status");
    let size_bytes: i64 = r.get("size_bytes");

    LocalFile {
        id: r.get("id"),
        track_id: r.get("track_id"),
        playlist_track_id: r.get("playlist_track_id"),
        format_profile: r.get("format_profile"),
        path: r.get("path"),
        size_bytes,
        modified_at: r.get("modified_at"),
        validation_status: ValidationStatus::from_str(&status_str)
            .unwrap_or(ValidationStatus::Unverified),
        validated_at: r.get("validated_at"),
        video_id_tag: r.get("video_id_tag"),
        created_at: r.get("created_at"),
    }
}

pub async fn get_local_file_by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<LocalFile>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, track_id, playlist_track_id, format_profile, path,
               size_bytes, modified_at, validation_status, validated_at,
               video_id_tag, created_at
        FROM local_files WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_local_file_row))
}

pub async fn get_local_file_by_path(
    pool: &SqlitePool,
    path: &str,
) -> Result<Option<LocalFile>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, track_id, playlist_track_id, format_profile, path,
               size_bytes, modified_at, validation_status, validated_at,
               video_id_tag, created_at
        FROM local_files WHERE path = ?
        "#,
    )
    .bind(path)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_local_file_row))
}

pub async fn list_local_files_by_track(
    pool: &SqlitePool,
    track_id: &str,
) -> Result<Vec<LocalFile>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, track_id, playlist_track_id, format_profile, path,
               size_bytes, modified_at, validation_status, validated_at,
               video_id_tag, created_at
        FROM local_files WHERE track_id = ?
        ORDER BY created_at DESC
        "#,
    )
    .bind(track_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(map_local_file_row).collect())
}

pub async fn update_file_validation_status(
    pool: &SqlitePool,
    id: &str,
    status: ValidationStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE local_files
        SET validation_status = ?, validated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}
