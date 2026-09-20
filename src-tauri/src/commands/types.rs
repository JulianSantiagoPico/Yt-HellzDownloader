use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::domain::entities::{JobItem, Playlist, Track};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionResult {
    pub total: u32,
    pub available: u32,
    pub unavailable: u32,
    pub playlist_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub track_count: u32,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub active_job_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetails {
    pub playlist: Playlist,
    pub tracks: Vec<PlaylistTrackWithStatus>,
    pub total: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTrackWithStatus {
    pub track: Track,
    pub position: u32,
    pub download_status: Option<String>,
    pub progress_percent: Option<f32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItemWithTrack {
    pub item: JobItem,
    pub track: Option<Track>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedJobItems {
    pub items: Vec<JobItemWithTrack>,
    pub total: u32,
    pub has_more: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub job_id: String,
    pub status: String,
    pub total_items: u32,
    pub completed_items: u32,
    pub failed_items: u32,
    pub pending_items: u32,
    pub current_item: Option<CurrentItem>,
    pub percent_complete: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentItem {
    pub item_id: String,
    pub track_title: String,
    pub stage: String,
    pub progress: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub job_id: String,
    pub kind: String,
    pub status: String,
    pub priority: i32,
    pub total_items: u32,
    pub completed_items: u32,
    pub created_at: DateTime<Utc>,
}
