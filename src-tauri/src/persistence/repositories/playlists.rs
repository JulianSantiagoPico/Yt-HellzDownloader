use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::domain::{
    entities::{Playlist, PlaylistTrack, Track},
    states::SourceKind,
};

pub async fn upsert_playlist(
    pool: &SqlitePool,
    playlist: &Playlist,
) -> Result<String, sqlx::Error> {
    let returned_id: String = sqlx::query_scalar(
        r#"
        INSERT INTO playlists (
            id, youtube_playlist_id, source_url, source_kind, title,
            channel, thumbnail_url, default_output_directory, last_synced_at,
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(youtube_playlist_id) DO UPDATE SET
            source_url = excluded.source_url,
            title = excluded.title,
            channel = excluded.channel,
            thumbnail_url = COALESCE(excluded.thumbnail_url, playlists.thumbnail_url),
            default_output_directory = COALESCE(excluded.default_output_directory, playlists.default_output_directory),
            last_synced_at = excluded.last_synced_at,
            updated_at = CURRENT_TIMESTAMP
        RETURNING id
        "#,
    )
    .bind(&playlist.id)
    .bind(&playlist.youtube_playlist_id)
    .bind(&playlist.source_url)
    .bind(playlist.source_kind.as_str())
    .bind(&playlist.title)
    .bind(&playlist.channel)
    .bind(&playlist.thumbnail_url)
    .bind(&playlist.default_output_directory)
    .bind(&playlist.last_synced_at)
    .bind(&playlist.created_at)
    .bind(&playlist.updated_at)
    .fetch_one(pool)
    .await?;

    Ok(returned_id)
}

fn map_playlist_row(r: &sqlx::sqlite::SqliteRow) -> Playlist {
    let source_kind_str: String = r.get("source_kind");
    Playlist {
        id: r.get("id"),
        youtube_playlist_id: r.get("youtube_playlist_id"),
        source_url: r.get("source_url"),
        source_kind: SourceKind::from_str(&source_kind_str).unwrap_or(SourceKind::YouTube),
        title: r.get("title"),
        channel: r.get("channel"),
        thumbnail_url: r.get("thumbnail_url"),
        default_output_directory: r.get("default_output_directory"),
        last_synced_at: r.get("last_synced_at"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

pub async fn get_playlist(pool: &SqlitePool, id: &str) -> Result<Option<Playlist>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, youtube_playlist_id, source_url, source_kind, title,
               channel, thumbnail_url, default_output_directory, last_synced_at,
               created_at, updated_at
        FROM playlists WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_playlist_row))
}

pub async fn get_playlist_by_youtube_id(
    pool: &SqlitePool,
    yt_id: &str,
) -> Result<Option<Playlist>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, youtube_playlist_id, source_url, source_kind, title,
               channel, thumbnail_url, default_output_directory, last_synced_at,
               created_at, updated_at
        FROM playlists WHERE youtube_playlist_id = ?
        "#,
    )
    .bind(yt_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_playlist_row))
}

pub async fn list_playlists(pool: &SqlitePool) -> Result<Vec<Playlist>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, youtube_playlist_id, source_url, source_kind, title,
               channel, thumbnail_url, default_output_directory, last_synced_at,
               created_at, updated_at
        FROM playlists ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(map_playlist_row).collect())
}

pub async fn upsert_track(pool: &SqlitePool, track: &Track) -> Result<String, sqlx::Error> {
    let returned_id: String = sqlx::query_scalar(
        r#"
        INSERT INTO tracks (
            id, youtube_video_id, source_url, title, artist, channel,
            published_at, duration_seconds, thumbnail_url, availability,
            metadata, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(youtube_video_id) DO UPDATE SET
            source_url = excluded.source_url,
            title = excluded.title,
            artist = excluded.artist,
            channel = excluded.channel,
            published_at = COALESCE(excluded.published_at, tracks.published_at),
            duration_seconds = COALESCE(excluded.duration_seconds, tracks.duration_seconds),
            thumbnail_url = COALESCE(excluded.thumbnail_url, tracks.thumbnail_url),
            availability = excluded.availability,
            metadata = COALESCE(excluded.metadata, tracks.metadata),
            updated_at = CURRENT_TIMESTAMP
        RETURNING id
        "#,
    )
    .bind(&track.id)
    .bind(&track.youtube_video_id)
    .bind(&track.source_url)
    .bind(&track.title)
    .bind(&track.artist)
    .bind(&track.channel)
    .bind(&track.published_at)
    .bind(track.duration_seconds)
    .bind(&track.thumbnail_url)
    .bind(&track.availability)
    .bind(&track.metadata)
    .bind(&track.created_at)
    .bind(&track.updated_at)
    .fetch_one(pool)
    .await?;

    Ok(returned_id)
}

fn map_track_row(r: &sqlx::sqlite::SqliteRow) -> Track {
    Track {
        id: r.get("id"),
        youtube_video_id: r.get("youtube_video_id"),
        source_url: r.get("source_url"),
        title: r.get("title"),
        artist: r.get("artist"),
        channel: r.get("channel"),
        published_at: r.get("published_at"),
        duration_seconds: r.get("duration_seconds"),
        thumbnail_url: r.get("thumbnail_url"),
        availability: r.get("availability"),
        metadata: r.get("metadata"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

pub async fn get_track(pool: &SqlitePool, id: &str) -> Result<Option<Track>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, youtube_video_id, source_url, title, artist, channel,
               published_at, duration_seconds, thumbnail_url, availability,
               metadata, created_at, updated_at
        FROM tracks WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_track_row))
}

pub async fn get_track_by_youtube_id(
    pool: &SqlitePool,
    yt_id: &str,
) -> Result<Option<Track>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, youtube_video_id, source_url, title, artist, channel,
               published_at, duration_seconds, thumbnail_url, availability,
               metadata, created_at, updated_at
        FROM tracks WHERE youtube_video_id = ?
        "#,
    )
    .bind(yt_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.as_ref().map(map_track_row))
}

pub async fn add_playlist_track(
    pool: &SqlitePool,
    entry: &PlaylistTrack,
) -> Result<PlaylistTrack, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO playlist_tracks (
            id, playlist_id, track_id, position, source_entry_id,
            title_at_sync, discovered_at, removed_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&entry.id)
    .bind(&entry.playlist_id)
    .bind(&entry.track_id)
    .bind(entry.position)
    .bind(&entry.source_entry_id)
    .bind(&entry.title_at_sync)
    .bind(&entry.discovered_at)
    .bind(&entry.removed_at)
    .execute(pool)
    .await?;

    Ok(entry.clone())
}

pub async fn get_active_playlist_tracks(
    pool: &SqlitePool,
    playlist_id: &str,
) -> Result<Vec<PlaylistTrack>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, playlist_id, track_id, position, source_entry_id,
               title_at_sync, discovered_at, removed_at
        FROM playlist_tracks
        WHERE playlist_id = ? AND removed_at IS NULL
        ORDER BY position ASC
        "#,
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await?;

    let entries = rows
        .iter()
        .map(|r| {
            let pos: i32 = r.get("position");
            PlaylistTrack {
                id: r.get("id"),
                playlist_id: r.get("playlist_id"),
                track_id: r.get("track_id"),
                position: pos,
                source_entry_id: r.get("source_entry_id"),
                title_at_sync: r.get("title_at_sync"),
                discovered_at: r.get("discovered_at"),
                removed_at: r.get("removed_at"),
            }
        })
        .collect();

    Ok(entries)
}
