use serde::{Deserialize, Serialize};

use crate::{
    domain::states::{
        Availability, JobItemStatus, JobKind, JobStatus, OrganizationMode, SourceKind,
        ValidationStatus,
    },
    filesystem::ExistingFilePolicy,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub youtube_playlist_id: String,
    pub source_url: String,
    pub source_kind: SourceKind,
    pub title: String,
    pub channel: String,
    pub thumbnail_url: Option<String>,
    pub default_output_directory: Option<String>,
    pub last_synced_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub youtube_video_id: String,
    pub source_url: String,
    pub title: String,
    pub artist: String,
    pub channel: String,
    pub published_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub availability: Availability,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTrack {
    pub id: String,
    pub playlist_id: String,
    pub track_id: String,
    pub position: i32,
    pub source_entry_id: Option<String>,
    pub title_at_sync: String,
    pub discovered_at: String,
    pub removed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub playlist_id: Option<String>,
    pub kind: JobKind,
    pub status: JobStatus,
    pub priority: i32,
    pub source_url: String,
    pub output_directory: String,
    pub organization_mode: OrganizationMode,
    pub format_profile: String,
    pub existing_file_policy: ExistingFilePolicy,
    pub cancel_requested_at: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobItem {
    pub id: String,
    pub job_id: String,
    pub track_id: Option<String>,
    pub playlist_track_id: Option<String>,
    pub playlist_position: Option<i32>,
    pub status: JobItemStatus,
    pub priority_offset: i32,
    pub progress_percent: Option<f32>,
    pub downloaded_bytes: Option<i64>,
    pub estimated_total_bytes: Option<i64>,
    pub attempts: i32,
    pub next_attempt_at: Option<String>,
    pub temporary_path: Option<String>,
    pub output_path: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub execution_lease_expires_at: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemReservation {
    pub youtube_video_id: String,
    pub format_profile: String,
    pub owner_job_item_id: String,
    pub lease_expires_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalFile {
    pub id: String,
    pub track_id: String,
    pub playlist_track_id: Option<String>,
    pub format_profile: String,
    pub path: String,
    pub size_bytes: i64,
    pub modified_at: String,
    pub validation_status: ValidationStatus,
    pub validated_at: String,
    pub video_id_tag: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub default_output_directory: String,
    pub organization_mode: OrganizationMode,
    pub existing_file_policy: ExistingFilePolicy,
    pub max_concurrent_downloads: u32,
    pub max_concurrent_conversions: u32,
    pub max_retries: u32,
    pub check_updates: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_output_directory: String::new(),
            organization_mode: OrganizationMode::PlaylistFolder,
            existing_file_policy: ExistingFilePolicy::Ask,
            max_concurrent_downloads: 2,
            max_concurrent_conversions: 1,
            max_retries: 3,
            check_updates: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub event_type: String,
    pub old_state: Option<String>,
    pub new_state: Option<String>,
    pub payload: Option<String>,
    pub created_at: String,
}
