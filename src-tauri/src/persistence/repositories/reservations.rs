use sqlx::{Row, SqlitePool};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcquireReservationResult {
    Acquired,
    WaitingForDuplicate { owner_job_item_id: String },
}

/// Intenta adquirir o renovar una reserva de descarga exclusiva para (youtube_video_id, format_profile).
/// Si otro ítem tiene una reserva activa y no expirada, retorna `WaitingForDuplicate`.
pub async fn acquire_or_wait_reservation(
    pool: &SqlitePool,
    youtube_video_id: &str,
    format_profile: &str,
    owner_job_item_id: &str,
    lease_seconds: i64,
) -> Result<AcquireReservationResult, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let existing = sqlx::query(
        r#"
        SELECT owner_job_item_id, 
               lease_expires_at,
               (datetime(lease_expires_at) > datetime('now')) AS is_active
        FROM item_reservations 
        WHERE youtube_video_id = ? AND format_profile = ?
        "#,
    )
    .bind(youtube_video_id)
    .bind(format_profile)
    .fetch_optional(&mut *tx)
    .await?;

    match existing {
        Some(res) => {
            let is_active: i32 = res.get("is_active");
            let current_owner: String = res.get("owner_job_item_id");

            if is_active == 1 {
                if current_owner == owner_job_item_id {
                    // Renovar el lease del mismo dueño
                    let query_str = format!(
                        "UPDATE item_reservations SET lease_expires_at = datetime('now', '+{} seconds'), updated_at = CURRENT_TIMESTAMP WHERE youtube_video_id = ? AND format_profile = ?",
                        lease_seconds
                    );
                    sqlx::query(&query_str)
                        .bind(youtube_video_id)
                        .bind(format_profile)
                        .execute(&mut *tx)
                        .await?;

                    tx.commit().await?;
                    Ok(AcquireReservationResult::Acquired)
                } else {
                    tx.commit().await?;
                    Ok(AcquireReservationResult::WaitingForDuplicate {
                        owner_job_item_id: current_owner,
                    })
                }
            } else {
                // Reserva existía pero expiró: tomar posesión
                let query_str = format!(
                    "UPDATE item_reservations SET owner_job_item_id = ?, lease_expires_at = datetime('now', '+{} seconds'), updated_at = CURRENT_TIMESTAMP WHERE youtube_video_id = ? AND format_profile = ?",
                    lease_seconds
                );
                sqlx::query(&query_str)
                    .bind(owner_job_item_id)
                    .bind(youtube_video_id)
                    .bind(format_profile)
                    .execute(&mut *tx)
                    .await?;

                tx.commit().await?;
                Ok(AcquireReservationResult::Acquired)
            }
        }
        None => {
            // No existe reserva previa: insertar nueva
            let query_str = format!(
                "INSERT INTO item_reservations (youtube_video_id, format_profile, owner_job_item_id, lease_expires_at, updated_at) VALUES (?, ?, ?, datetime('now', '+{} seconds'), CURRENT_TIMESTAMP)",
                lease_seconds
            );
            sqlx::query(&query_str)
                .bind(youtube_video_id)
                .bind(format_profile)
                .bind(owner_job_item_id)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            Ok(AcquireReservationResult::Acquired)
        }
    }
}

/// Libera la reserva si pertenece al ítem indicado.
pub async fn release_reservation(
    pool: &SqlitePool,
    youtube_video_id: &str,
    format_profile: &str,
    owner_job_item_id: &str,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        r#"
        DELETE FROM item_reservations
        WHERE youtube_video_id = ? AND format_profile = ? AND owner_job_item_id = ?
        "#,
    )
    .bind(youtube_video_id)
    .bind(format_profile)
    .bind(owner_job_item_id)
    .execute(pool)
    .await?;

    Ok(res.rows_affected() > 0)
}

/// Limpia todas las reservas cuyo lease haya expirado. Retorna la cantidad de reservas eliminadas.
pub async fn clean_expired_reservations(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"
        DELETE FROM item_reservations
        WHERE datetime(lease_expires_at) <= datetime('now')
        "#,
    )
    .execute(pool)
    .await?;

    Ok(res.rows_affected())
}
