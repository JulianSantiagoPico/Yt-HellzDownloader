use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("Estado inválido: {0}")]
pub struct ParseStatusError(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Created,
    Extracting,
    Queued,
    Running,
    Paused,
    Cancelling,
    Cancelled,
    Completed,
    CompletedWithErrors,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Extracting => "extracting",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
            Self::CompletedWithErrors => "completed_with_errors",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Completed | Self::CompletedWithErrors | Self::Failed
        )
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Extracting | Self::Running)
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for JobStatus {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "created" => Ok(Self::Created),
            "extracting" => Ok(Self::Extracting),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "cancelling" => Ok(Self::Cancelling),
            "cancelled" => Ok(Self::Cancelled),
            "completed" => Ok(Self::Completed),
            "completed_with_errors" => Ok(Self::CompletedWithErrors),
            "failed" => Ok(Self::Failed),
            other => Err(ParseStatusError(format!(
                "JobStatus desconocido: {}",
                other
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobItemStatus {
    Pending,
    Queued,
    Downloading,
    Validating,
    Converting,
    Tagging,
    Completed,
    Paused,
    RetryWait,
    Skipped,
    Cancelled,
    Failed,
    Interrupted,
    WaitingForDuplicate,
}

impl JobItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Validating => "validating",
            Self::Converting => "converting",
            Self::Tagging => "tagging",
            Self::Completed => "completed",
            Self::Paused => "paused",
            Self::RetryWait => "retry_wait",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::WaitingForDuplicate => "waiting_for_duplicate",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Skipped | Self::Cancelled | Self::Failed
        )
    }

    pub fn is_active_execution(&self) -> bool {
        matches!(
            self,
            Self::Downloading | Self::Validating | Self::Converting | Self::Tagging
        )
    }
}

impl fmt::Display for JobItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for JobItemStatus {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "downloading" => Ok(Self::Downloading),
            "validating" => Ok(Self::Validating),
            "converting" => Ok(Self::Converting),
            "tagging" => Ok(Self::Tagging),
            "completed" => Ok(Self::Completed),
            "paused" => Ok(Self::Paused),
            "retry_wait" => Ok(Self::RetryWait),
            "skipped" => Ok(Self::Skipped),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            "waiting_for_duplicate" => Ok(Self::WaitingForDuplicate),
            other => Err(ParseStatusError(format!(
                "JobItemStatus desconocido: {}",
                other
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Corrupted,
    Missing,
    Unverified,
}

impl ValidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Corrupted => "corrupted",
            Self::Missing => "missing",
            Self::Unverified => "unverified",
        }
    }
}

impl fmt::Display for ValidationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ValidationStatus {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "valid" => Ok(Self::Valid),
            "corrupted" => Ok(Self::Corrupted),
            "missing" => Ok(Self::Missing),
            "unverified" => Ok(Self::Unverified),
            other => Err(ParseStatusError(format!(
                "ValidationStatus desconocido: {}",
                other
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    YouTube,
    YouTubeMusic,
}

impl SourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::YouTube => "youtube",
            Self::YouTubeMusic => "youtube_music",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for SourceKind {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "youtube" => Ok(Self::YouTube),
            "youtube_music" => Ok(Self::YouTubeMusic),
            other => Err(ParseStatusError(format!(
                "SourceKind desconocido: {}",
                other
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Import,
    Sync,
    Retry,
    SingleDownload,
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Sync => "sync",
            Self::Retry => "retry",
            Self::SingleDownload => "single_download",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for JobKind {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "import" => Ok(Self::Import),
            "sync" => Ok(Self::Sync),
            "retry" => Ok(Self::Retry),
            "single_download" => Ok(Self::SingleDownload),
            other => Err(ParseStatusError(format!("JobKind desconocido: {}", other))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationMode {
    PlaylistFolder,
    Flat,
}

impl OrganizationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlaylistFolder => "playlist_folder",
            Self::Flat => "flat",
        }
    }
}

impl fmt::Display for OrganizationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for OrganizationMode {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "playlist_folder" => Ok(Self::PlaylistFolder),
            "flat" => Ok(Self::Flat),
            other => Err(ParseStatusError(format!(
                "OrganizationMode desconocido: {}",
                other
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Private,
    Deleted,
    GeoBlocked,
    Unknown,
}

impl Availability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Private => "private",
            Self::Deleted => "deleted",
            Self::GeoBlocked => "geo_blocked",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_ytdlp_title(title: &str) -> Self {
        match title.trim() {
            "[Private video]" => Self::Private,
            "[Deleted video]" => Self::Deleted,
            _ => Self::Available,
        }
    }
}

impl Default for Availability {
    fn default() -> Self {
        Self::Available
    }
}

impl fmt::Display for Availability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for Availability {
    type Err = ParseStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "available" => Ok(Self::Available),
            "private" => Ok(Self::Private),
            "deleted" => Ok(Self::Deleted),
            "geo_blocked" => Ok(Self::GeoBlocked),
            "unknown" => Ok(Self::Unknown),
            other => Err(ParseStatusError(format!(
                "Availability desconocida: {}",
                other
            ))),
        }
    }
}

