use std::collections::HashSet;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use uuid::Uuid;

use crate::{
    domain::states::Availability,
    youtube::PlaylistEntry,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchInsertResult {
    pub tracks_inserted: u32,
    pub playlist_tracks_inserted: u32,
    pub job_items_inserted: u32,
    pub available: u32,
    pub unavailable: u32,
}

/// Inserta un lote de entradas de playlist de forma atómica en una única transacción multi-tabla
/// utilizando multi-row INSERTs optimizados con QueryBuilder (3 queries por chunk en vez de 150).
pub async fn batch_insert_playlist_items(
    pool: &SqlitePool,
    entries: &[PlaylistEntry],
    job_id: &str,
    actual_playlist_id: &str,
    now: &str,
) -> Result<BatchInsertResult, sqlx::Error> {
    if entries.is_empty() {
        return Ok(BatchInsertResult::default());
    }

    let mut tx = pool.begin().await?;

    // 1. Deduplicar por youtube_video_id e insertar tracks
    //    Usamos ID determinista trk_{youtube_video_id}. ON CONFLICT actualiza metadata.
    let mut seen_ids = HashSet::new();
    let unique_tracks: Vec<&PlaylistEntry> = entries
        .iter()
        .filter(|e| seen_ids.insert(&e.id))
        .collect();

    let mut track_builder = QueryBuilder::<Sqlite>::new(
        "INSERT INTO tracks (id, youtube_video_id, source_url, title, artist, channel, published_at, duration_seconds, thumbnail_url, availability, metadata, created_at, updated_at) "
    );

    track_builder.push_values(unique_tracks.iter(), |mut b, entry| {
        let track_id = format!("trk_{}", entry.id);
        b.push_bind(track_id)
            .push_bind(&entry.id)
            .push_bind(&entry.url)
            .push_bind(&entry.title)
            .push_bind(&entry.artist)
            .push_bind("")
            .push_bind(Option::<String>::None)
            .push_bind(entry.duration_seconds)
            .push_bind(Option::<String>::None)
            .push_bind(entry.availability.as_str())
            .push_bind(Option::<String>::None)
            .push_bind(now)
            .push_bind(now);
    });

    track_builder.push(
        " ON CONFLICT(youtube_video_id) DO UPDATE SET \
         title = excluded.title, \
         artist = excluded.artist, \
         duration_seconds = COALESCE(excluded.duration_seconds, tracks.duration_seconds), \
         availability = excluded.availability, \
         updated_at = excluded.updated_at",
    );

    track_builder.build().execute(&mut *tx).await?;

    // 2. Multi-row INSERT para playlist_tracks
    let pt_ids: Vec<String> = (0..entries.len())
        .map(|_| Uuid::new_v4().to_string())
        .collect();

    let mut pt_builder = QueryBuilder::<Sqlite>::new(
        "INSERT INTO playlist_tracks (id, playlist_id, track_id, position, source_entry_id, title_at_sync, discovered_at, removed_at) "
    );

    pt_builder.push_values(entries.iter().zip(pt_ids.iter()), |mut b, (entry, pt_id)| {
        let track_id = format!("trk_{}", entry.id);
        b.push_bind(pt_id)
            .push_bind(actual_playlist_id)
            .push_bind(track_id)
            .push_bind(entry.index as i32)
            .push_bind(&entry.id)
            .push_bind(&entry.title)
            .push_bind(now)
            .push_bind(Option::<String>::None);
    });

    pt_builder.push(
        " ON CONFLICT(playlist_id, position) WHERE removed_at IS NULL DO UPDATE SET \
         track_id = excluded.track_id, \
         title_at_sync = excluded.title_at_sync",
    );

    pt_builder.build().execute(&mut *tx).await?;

    // 2b. Recuperar los IDs reales de playlist_tracks (nuevos o existentes por conflicto)
    let positions: Vec<i32> = entries.iter().map(|e| e.index as i32).collect();
    let mut real_pt_ids_query = QueryBuilder::<Sqlite>::new(
        "SELECT position, id FROM playlist_tracks WHERE playlist_id = "
    );
    real_pt_ids_query.push_bind(actual_playlist_id);
    real_pt_ids_query.push(" AND position IN (");
    let mut separated = real_pt_ids_query.separated(", ");
    for pos in &positions {
        separated.push_bind(*pos);
    }
    separated.push_unseparated(") ORDER BY position");

    let real_pt_rows: Vec<(i32, String)> = real_pt_ids_query
        .build_query_as()
        .fetch_all(&mut *tx)
        .await?;

    let real_pt_map: std::collections::HashMap<i32, String> =
        real_pt_rows.into_iter().collect();

    let resolved_pt_ids: Vec<String> = entries
        .iter()
        .map(|e| {
            real_pt_map
                .get(&(e.index as i32))
                .cloned()
                .unwrap_or_else(|| panic!(
                    "playlist_track no encontrado para position {} en playlist {}",
                    e.index, actual_playlist_id
                ))
        })
        .collect();

    // 3. Multi-row INSERT para job_items
    let mut available_count = 0u32;
    let mut unavailable_count = 0u32;

    let mut ji_builder = QueryBuilder::<Sqlite>::new(
        "INSERT INTO job_items (id, job_id, track_id, playlist_track_id, playlist_position, status, priority_offset, progress_percent, downloaded_bytes, estimated_total_bytes, attempts, next_attempt_at, temporary_path, output_path, error_code, error_message, execution_lease_expires_at, created_at, started_at, completed_at) "
    );

    ji_builder.push_values(entries.iter().zip(resolved_pt_ids.iter()), |mut b, (entry, pt_id)| {
        let track_id = format!("trk_{}", entry.id);
        let job_item_id = Uuid::new_v4().to_string();

        let is_unavailable = matches!(
            entry.availability,
            Availability::Private | Availability::Deleted | Availability::GeoBlocked
        );

        let (status_str, err_code, err_msg) = if is_unavailable {
            unavailable_count += 1;
            let (code, msg) = match entry.availability {
                Availability::Private => ("video_private", "Vídeo privado no disponible"),
                Availability::Deleted => ("video_deleted", "Vídeo eliminado"),
                Availability::GeoBlocked => ("video_geoblocked", "Vídeo bloqueado por región"),
                _ => ("video_unavailable", "Vídeo no disponible"),
            };
            ("skipped", Some(code), Some(msg))
        } else {
            available_count += 1;
            ("queued", None, None)
        };

        b.push_bind(job_item_id)
            .push_bind(job_id)
            .push_bind(track_id)
            .push_bind(pt_id)
            .push_bind(entry.index as i32)
            .push_bind(status_str)
            .push_bind(0i32)
            .push_bind(0.0f32)
            .push_bind(0i64)
            .push_bind(Option::<i64>::None)
            .push_bind(0i32)
            .push_bind(Option::<String>::None)
            .push_bind(Option::<String>::None)
            .push_bind(Option::<String>::None)
            .push_bind(err_code)
            .push_bind(err_msg)
            .push_bind(Option::<String>::None)
            .push_bind(now)
            .push_bind(Option::<String>::None)
            .push_bind(Option::<String>::None);
    });

    ji_builder.build().execute(&mut *tx).await?;

    tx.commit().await?;

    Ok(BatchInsertResult {
        tracks_inserted: entries.len() as u32,
        playlist_tracks_inserted: entries.len() as u32,
        job_items_inserted: entries.len() as u32,
        available: available_count,
        unavailable: unavailable_count,
    })
}
