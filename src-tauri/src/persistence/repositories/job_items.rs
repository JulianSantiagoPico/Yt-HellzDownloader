use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::domain::{
    entities::JobItem,
    states::JobItemStatus,
    transitions::{TransitionError, TransitionValidator},
};

pub async fn create_job_item(pool: &SqlitePool, item: &JobItem) -> Result<JobItem, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO job_items (
            id, job_id, track_id, playlist_track_id, playlist_position,
            status, priority_offset, progress_percent, downloaded_bytes,
            estimated_total_bytes, attempts, next_attempt_at, temporary_path,
            output_path, error_code, error_message, execution_lease_expires_at,
            created_at, started_at, completed_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&item.id)
    .bind(&item.job_id)
    .bind(&item.track_id)
    .bind(&item.playlist_track_id)
    .bind(item.playlist_position)
    .bind(item.status.as_str())
    .bind(item.priority_offset)
    .bind(item.progress_percent)
    .bind(item.downloaded_bytes)
    .bind(item.estimated_total_bytes)
    .bind(item.attempts)
    .bind(&item.next_attempt_at)
    .bind(&item.temporary_path)
    .bind(&item.output_path)
    .bind(&item.error_code)
    .bind(&item.error_message)
    .bind(&item.execution_lease_expires_at)
    .bind(&item.created_at)
    .bind(&item.started_at)
    .bind(&item.completed_at)
    .execute(pool)
    .await?;

    Ok(item.clone())
}

pub async fn create_job_items_batch(
    pool: &SqlitePool,
    items: &[JobItem],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    for item in items {
        sqlx::query(
            r#"
            INSERT INTO job_items (
                id, job_id, track_id, playlist_track_id, playlist_position,
                status, priority_offset, progress_percent, downloaded_bytes,
                estimated_total_bytes, attempts, next_attempt_at, temporary_path,
                output_path, error_code, error_message, execution_lease_expires_at,
                created_at, started_at, completed_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&item.id)
        .bind(&item.job_id)
        .bind(&item.track_id)
        .bind(&item.playlist_track_id)
        .bind(item.playlist_position)
        .bind(item.status.as_str())
        .bind(item.priority_offset)
        .bind(item.progress_percent)
        .bind(item.downloaded_bytes)
        .bind(item.estimated_total_bytes)
        .bind(item.attempts)
        .bind(&item.next_attempt_at)
        .bind(&item.temporary_path)
        .bind(&item.output_path)
        .bind(&item.error_code)
        .bind(&item.error_message)
        .bind(&item.execution_lease_expires_at)
        .bind(&item.created_at)
        .bind(&item.started_at)
        .bind(&item.completed_at)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

fn map_job_item_row(r: &sqlx::sqlite::SqliteRow) -> JobItem {
    let status_str: String = r.get("status");
    let priority_offset: i32 = r.get("priority_offset");
    let attempts: i32 = r.get("attempts");
    let position: Option<i32> = r.get("playlist_position");
    let progress: Option<f32> = r.get("progress_percent");

    JobItem {
        id: r.get("id"),
        job_id: r.get("job_id"),
        track_id: r.get("track_id"),
        playlist_track_id: r.get("playlist_track_id"),
        playlist_position: position,
        status: JobItemStatus::from_str(&status_str).unwrap_or(JobItemStatus::Pending),
        priority_offset,
        progress_percent: progress,
        downloaded_bytes: r.get("downloaded_bytes"),
        estimated_total_bytes: r.get("estimated_total_bytes"),
        attempts,
        next_attempt_at: r.get("next_attempt_at"),
        temporary_path: r.get("temporary_path"),
        output_path: r.get("output_path"),
        error_code: r.get("error_code"),
        error_message: r.get("error_message"),
        execution_lease_expires_at: r.get("execution_lease_expires_at"),
        created_at: r.get("created_at"),
        started_at: r.get("started_at"),
        completed_at: r.get("completed_at"),
    }
}

pub async fn get_job_item(pool: &SqlitePool, id: &str) -> Result<Option<JobItem>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, job_id, track_id, playlist_track_id, playlist_position,
               status, priority_offset, progress_percent, downloaded_bytes,
               estimated_total_bytes, attempts, next_attempt_at, temporary_path,
               output_path, error_code, error_message, execution_lease_expires_at,
               created_at, started_at, completed_at
        FROM job_items WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_job_item_row))
}

pub async fn list_job_items_by_job(
    pool: &SqlitePool,
    job_id: &str,
) -> Result<Vec<JobItem>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, job_id, track_id, playlist_track_id, playlist_position,
               status, priority_offset, progress_percent, downloaded_bytes,
               estimated_total_bytes, attempts, next_attempt_at, temporary_path,
               output_path, error_code, error_message, execution_lease_expires_at,
               created_at, started_at, completed_at
        FROM job_items WHERE job_id = ?
        ORDER BY playlist_position ASC, created_at ASC
        "#,
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(map_job_item_row).collect())
}

/// Transiciona atómica e idempotentemente el estado de un JobItem.
pub async fn transition_job_item_status(
    pool: &SqlitePool,
    item_id: &str,
    target_status: JobItemStatus,
    lease_seconds: Option<i64>,
    payload: Option<&str>,
) -> Result<JobItemStatus, Box<dyn std::error::Error + Send + Sync>> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query("SELECT status FROM job_items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| format!("JobItem con ID '{}' no encontrado", item_id))?;

    let status_str: String = row.get("status");
    let current_status = JobItemStatus::from_str(&status_str)
        .map_err(|e| format!("Estado actual inválido en BD: {}", e.0))?;

    // Validar transición
    let changed = TransitionValidator::validate_job_item_transition(current_status, target_status)
        .map_err(|e| match e {
            TransitionError::InvalidJobItemTransition { from, to } => {
                format!("Transición inválida de JobItem: {} -> {}", from, to)
            }
            _ => "Error de transición".to_string(),
        })?;

    if !changed {
        // Idempotente: estado ya coincide
        tx.commit().await?;
        return Ok(current_status);
    }

    // Calcular nuevo lease si corresponde
    let lease_clause = match (target_status.is_active_execution(), lease_seconds) {
        (true, Some(secs)) => {
            format!("datetime('now', '+{} seconds')", secs)
        }
        (true, None) => "datetime('now', '+60 seconds')".to_string(),
        (false, _) => "NULL".to_string(),
    };

    // Actualizar estado y marcas temporales
    let query_str = match target_status {
        JobItemStatus::Downloading => {
            format!(
                "UPDATE job_items SET status = ?, started_at = COALESCE(started_at, CURRENT_TIMESTAMP), execution_lease_expires_at = {} WHERE id = ?",
                lease_clause
            )
        }
        s if s.is_terminal() => {
            "UPDATE job_items SET status = ?, completed_at = CURRENT_TIMESTAMP, execution_lease_expires_at = NULL WHERE id = ?".to_string()
        }
        _ => {
            format!(
                "UPDATE job_items SET status = ?, execution_lease_expires_at = {} WHERE id = ?",
                lease_clause
            )
        }
    };

    sqlx::query(&query_str)
        .bind(target_status.as_str())
        .bind(item_id)
        .execute(&mut *tx)
        .await?;

    // Si pasa a estado terminal, liberar cualquier reserva asociada a este ítem
    if target_status.is_terminal() {
        sqlx::query("DELETE FROM item_reservations WHERE owner_job_item_id = ?")
            .bind(item_id)
            .execute(&mut *tx)
            .await?;
    }

    // Registrar evento de actividad
    sqlx::query(
        r#"
        INSERT INTO activity_events (entity_type, entity_id, event_type, old_state, new_state, payload)
        VALUES ('job_item', ?, 'status_transition', ?, ?, ?)
        "#,
    )
    .bind(item_id)
    .bind(current_status.as_str())
    .bind(target_status.as_str())
    .bind(payload)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(target_status)
}

pub async fn update_job_item_progress(
    pool: &SqlitePool,
    item_id: &str,
    progress_percent: f32,
    downloaded_bytes: Option<i64>,
    estimated_total_bytes: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE job_items
        SET progress_percent = ?, downloaded_bytes = ?, estimated_total_bytes = ?
        WHERE id = ?
        "#,
    )
    .bind(progress_percent)
    .bind(downloaded_bytes)
    .bind(estimated_total_bytes)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_job_item_paths(
    pool: &SqlitePool,
    item_id: &str,
    temporary_path: Option<&str>,
    output_path: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE job_items
        SET temporary_path = COALESCE(?, temporary_path),
            output_path = COALESCE(?, output_path)
        WHERE id = ?
        "#,
    )
    .bind(temporary_path)
    .bind(output_path)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_job_item_error(
    pool: &SqlitePool,
    item_id: &str,
    error_code: &str,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE job_items
        SET error_code = ?, error_message = ?
        WHERE id = ?
        "#,
    )
    .bind(error_code)
    .bind(error_message)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn renew_job_item_lease(
    pool: &SqlitePool,
    item_id: &str,
    lease_seconds: i64,
) -> Result<(), sqlx::Error> {
    let query_str = format!(
        "UPDATE job_items SET execution_lease_expires_at = datetime('now', '+{} seconds') WHERE id = ?",
        lease_seconds
    );
    sqlx::query(&query_str).bind(item_id).execute(pool).await?;
    Ok(())
}
