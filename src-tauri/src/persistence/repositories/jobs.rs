use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::domain::{
    entities::Job,
    states::{JobKind, JobStatus, OrganizationMode},
    transitions::{TransitionError, TransitionValidator},
};
use crate::filesystem::ExistingFilePolicy;

pub async fn create_job(pool: &SqlitePool, job: &Job) -> Result<Job, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO jobs (
            id, playlist_id, kind, status, priority, source_url,
            output_directory, organization_mode, format_profile,
            existing_file_policy, cancel_requested_at, created_at,
            started_at, completed_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&job.id)
    .bind(&job.playlist_id)
    .bind(job.kind.as_str())
    .bind(job.status.as_str())
    .bind(job.priority)
    .bind(&job.source_url)
    .bind(&job.output_directory)
    .bind(job.organization_mode.as_str())
    .bind(&job.format_profile)
    .bind(job.existing_file_policy.as_str())
    .bind(&job.cancel_requested_at)
    .bind(&job.created_at)
    .bind(&job.started_at)
    .bind(&job.completed_at)
    .execute(pool)
    .await?;

    // Registrar evento inicial de creación
    log_activity_event(
        pool,
        "job",
        &job.id,
        "job_created",
        None,
        Some(job.status.as_str()),
        None,
    )
    .await?;

    Ok(job.clone())
}

pub async fn get_job(pool: &SqlitePool, id: &str) -> Result<Option<Job>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, playlist_id, kind, status, priority, source_url,
               output_directory, organization_mode, format_profile,
               existing_file_policy, cancel_requested_at, created_at,
               started_at, completed_at
        FROM jobs WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => {
            let kind_str: String = r.get("kind");
            let status_str: String = r.get("status");
            let org_str: String = r.get("organization_mode");
            let policy_str: String = r.get("existing_file_policy");
            let priority: i32 = r.get("priority");

            Ok(Some(Job {
                id: r.get("id"),
                playlist_id: r.get("playlist_id"),
                kind: JobKind::from_str(&kind_str).unwrap_or(JobKind::Import),
                status: JobStatus::from_str(&status_str).unwrap_or(JobStatus::Created),
                priority,
                source_url: r.get("source_url"),
                output_directory: r.get("output_directory"),
                organization_mode: OrganizationMode::from_str(&org_str)
                    .unwrap_or(OrganizationMode::PlaylistFolder),
                format_profile: r.get("format_profile"),
                existing_file_policy: match policy_str.as_str() {
                    "reuse" => ExistingFilePolicy::Reuse,
                    "overwrite" => ExistingFilePolicy::Overwrite,
                    "rename" => ExistingFilePolicy::Rename,
                    _ => ExistingFilePolicy::Ask,
                },
                cancel_requested_at: r.get("cancel_requested_at"),
                created_at: r.get("created_at"),
                started_at: r.get("started_at"),
                completed_at: r.get("completed_at"),
            }))
        }
        None => Ok(None),
    }
}

pub async fn list_jobs(pool: &SqlitePool) -> Result<Vec<Job>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, playlist_id, kind, status, priority, source_url,
               output_directory, organization_mode, format_profile,
               existing_file_policy, cancel_requested_at, created_at,
               started_at, completed_at
        FROM jobs ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut jobs = Vec::with_capacity(rows.len());
    for r in rows {
        let kind_str: String = r.get("kind");
        let status_str: String = r.get("status");
        let org_str: String = r.get("organization_mode");
        let policy_str: String = r.get("existing_file_policy");
        let priority: i32 = r.get("priority");

        jobs.push(Job {
            id: r.get("id"),
            playlist_id: r.get("playlist_id"),
            kind: JobKind::from_str(&kind_str).unwrap_or(JobKind::Import),
            status: JobStatus::from_str(&status_str).unwrap_or(JobStatus::Created),
            priority,
            source_url: r.get("source_url"),
            output_directory: r.get("output_directory"),
            organization_mode: OrganizationMode::from_str(&org_str)
                .unwrap_or(OrganizationMode::PlaylistFolder),
            format_profile: r.get("format_profile"),
            existing_file_policy: match policy_str.as_str() {
                "reuse" => ExistingFilePolicy::Reuse,
                "overwrite" => ExistingFilePolicy::Overwrite,
                "rename" => ExistingFilePolicy::Rename,
                _ => ExistingFilePolicy::Ask,
            },
            cancel_requested_at: r.get("cancel_requested_at"),
            created_at: r.get("created_at"),
            started_at: r.get("started_at"),
            completed_at: r.get("completed_at"),
        });
    }

    Ok(jobs)
}

/// Ejecuta una transición atómica e idempotente del estado de un Job.
pub async fn transition_job_status(
    pool: &SqlitePool,
    job_id: &str,
    target_status: JobStatus,
    payload: Option<&str>,
) -> Result<JobStatus, Box<dyn std::error::Error + Send + Sync>> {
    let mut tx = pool.begin().await?;

    let row = sqlx::query("SELECT status FROM jobs WHERE id = ?")
        .bind(job_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| format!("Job con ID '{}' no encontrado", job_id))?;

    let status_str: String = row.get("status");
    let current_status = JobStatus::from_str(&status_str)
        .map_err(|e| format!("Estado actual inválido en BD: {}", e.0))?;

    // Validar idempotencia y transición
    let changed = TransitionValidator::validate_job_transition(current_status, target_status)
        .map_err(|e| match e {
            TransitionError::InvalidJobTransition { from, to } => {
                format!("Transición inválida de Job: {} -> {}", from, to)
            }
            _ => "Error de transición".to_string(),
        })?;

    if !changed {
        // Idempotente: el estado ya es el objetivo, no hacemos cambios
        tx.commit().await?;
        return Ok(current_status);
    }

    // Actualizar timestamps según el estado
    match target_status {
        JobStatus::Running => {
            sqlx::query(
                "UPDATE jobs SET status = ?, started_at = COALESCE(started_at, CURRENT_TIMESTAMP) WHERE id = ?",
            )
            .bind(target_status.as_str())
            .bind(job_id)
            .execute(&mut *tx)
            .await?;
        }
        s if s.is_terminal() => {
            sqlx::query(
                "UPDATE jobs SET status = ?, completed_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(target_status.as_str())
            .bind(job_id)
            .execute(&mut *tx)
            .await?;
        }
        _ => {
            sqlx::query("UPDATE jobs SET status = ? WHERE id = ?")
                .bind(target_status.as_str())
                .bind(job_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    // Registrar evento de actividad en la misma transacción
    sqlx::query(
        r#"
        INSERT INTO activity_events (entity_type, entity_id, event_type, old_state, new_state, payload)
        VALUES ('job', ?, 'status_transition', ?, ?, ?)
        "#,
    )
    .bind(job_id)
    .bind(current_status.as_str())
    .bind(target_status.as_str())
    .bind(payload)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(target_status)
}

pub async fn log_activity_event(
    pool: &SqlitePool,
    entity_type: &str,
    entity_id: &str,
    event_type: &str,
    old_state: Option<&str>,
    new_state: Option<&str>,
    payload: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO activity_events (entity_type, entity_id, event_type, old_state, new_state, payload)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(entity_type)
    .bind(entity_id)
    .bind(event_type)
    .bind(old_state)
    .bind(new_state)
    .bind(payload)
    .execute(pool)
    .await?;
    Ok(())
}
