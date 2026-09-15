use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::path::Path;

use crate::domain::states::{JobItemStatus, JobStatus};
use crate::persistence::repositories::{
    job_items::transition_job_item_status, jobs::transition_job_status,
    reservations::clean_expired_reservations,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterruptedItemSummary {
    pub item_id: String,
    pub job_id: String,
    pub temporary_path: Option<String>,
    pub has_temporary_files: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub interrupted_jobs_count: usize,
    pub interrupted_items_count: usize,
    pub cleaned_reservations_count: u64,
    pub interrupted_job_ids: Vec<String>,
    pub interrupted_item_ids: Vec<String>,
    pub interrupted_items_details: Vec<InterruptedItemSummary>,
}

/// Ejecuta la detección y recuperación de trabajos e ítems interrumpidos tras un cierre inesperado.
/// Esta función es completamente idempotente: invocarla repetidamente no genera efectos no deseados.
pub async fn recover_on_startup(
    pool: &SqlitePool,
) -> Result<RecoveryReport, Box<dyn std::error::Error + Send + Sync>> {
    let mut report = RecoveryReport::default();

    // 1. Identificar ítems activos que quedaron colgados por caída o lease vencido
    let active_item_rows = sqlx::query(
        r#"
        SELECT id, status, job_id, temporary_path
        FROM job_items
        WHERE status IN ('downloading', 'validating', 'converting', 'tagging')
           OR (execution_lease_expires_at IS NOT NULL 
               AND datetime(execution_lease_expires_at) <= datetime('now')
               AND status NOT IN ('completed', 'failed', 'cancelled', 'skipped', 'interrupted', 'paused'))
        "#,
    )
    .fetch_all(pool)
    .await?;

    for row in active_item_rows {
        let item_id: String = row.get("id");
        let job_id: String = row.get("job_id");
        let temp_path: Option<String> = row.get("temporary_path");

        // Inspeccionar si existen archivos temporales o .part en la ruta del ítem
        let has_temporary_files = temp_path.as_ref().is_some_and(|p| {
            let path = Path::new(p);
            if path.is_file() {
                true
            } else if path.is_dir() {
                std::fs::read_dir(path)
                    .map(|mut entries| entries.next().is_some())
                    .unwrap_or(false)
            } else {
                false
            }
        });

        let outcome = transition_job_item_status(
            pool,
            &item_id,
            JobItemStatus::Interrupted,
            None,
            Some("recovery_startup_interrupted"),
        )
        .await;

        if let Ok(new_status) = outcome {
            if new_status == JobItemStatus::Interrupted {
                report.interrupted_item_ids.push(item_id.clone());
                report
                    .interrupted_items_details
                    .push(InterruptedItemSummary {
                        item_id,
                        job_id,
                        temporary_path: temp_path,
                        has_temporary_files,
                    });
            }
        }
    }
    report.interrupted_items_count = report.interrupted_item_ids.len();

    // 2. Identificar Jobs que estaban en ejecución o extracción cuando el proceso finalizó
    let active_jobs = sqlx::query(
        r#"
        SELECT id, status
        FROM jobs
        WHERE status IN ('running', 'extracting')
        "#,
    )
    .fetch_all(pool)
    .await?;

    for job in active_jobs {
        let job_id: String = job.get("id");
        let outcome = transition_job_status(
            pool,
            &job_id,
            JobStatus::Paused,
            Some("recovery_startup_interrupted_job"),
        )
        .await;

        if let Ok(new_status) = outcome {
            if new_status == JobStatus::Paused {
                report.interrupted_job_ids.push(job_id);
            }
        }
    }
    report.interrupted_jobs_count = report.interrupted_job_ids.len();

    // 3. Limpiar reservas expiradas
    let cleaned = clean_expired_reservations(pool).await?;
    report.cleaned_reservations_count = cleaned;

    Ok(report)
}
