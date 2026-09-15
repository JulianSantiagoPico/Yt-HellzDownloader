-- Migración 0002: Esquema de Dominio y Persistencia Completo

-- 1. Playlists
CREATE TABLE playlists (
    id TEXT PRIMARY KEY NOT NULL,
    youtube_playlist_id TEXT UNIQUE NOT NULL,
    source_url TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('youtube', 'youtube_music')),
    title TEXT NOT NULL,
    channel TEXT NOT NULL,
    thumbnail_url TEXT,
    default_output_directory TEXT,
    last_synced_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_playlists_youtube_id ON playlists(youtube_playlist_id);

-- 2. Tracks (vídeo global)
CREATE TABLE tracks (
    id TEXT PRIMARY KEY NOT NULL,
    youtube_video_id TEXT UNIQUE NOT NULL,
    source_url TEXT NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    channel TEXT NOT NULL,
    published_at TEXT,
    duration_seconds REAL,
    thumbnail_url TEXT,
    availability TEXT NOT NULL DEFAULT 'available',
    metadata TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_tracks_youtube_video_id ON tracks(youtube_video_id);

-- 3. Playlist Tracks (aparición concreta en una playlist)
CREATE TABLE playlist_tracks (
    id TEXT PRIMARY KEY NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    source_entry_id TEXT,
    title_at_sync TEXT NOT NULL,
    discovered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    removed_at TEXT
);

CREATE UNIQUE INDEX idx_playlist_tracks_active_position 
ON playlist_tracks(playlist_id, position) 
WHERE removed_at IS NULL;

CREATE INDEX idx_playlist_tracks_playlist ON playlist_tracks(playlist_id);
CREATE INDEX idx_playlist_tracks_track ON playlist_tracks(track_id);

-- 4. Jobs
CREATE TABLE jobs (
    id TEXT PRIMARY KEY NOT NULL,
    playlist_id TEXT REFERENCES playlists(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN ('import', 'sync', 'retry', 'single_download')),
    status TEXT NOT NULL CHECK (status IN ('created', 'extracting', 'queued', 'running', 'paused', 'cancelling', 'cancelled', 'completed', 'completed_with_errors', 'failed')),
    priority INTEGER NOT NULL DEFAULT 0,
    source_url TEXT NOT NULL,
    output_directory TEXT NOT NULL,
    organization_mode TEXT NOT NULL DEFAULT 'playlist_folder' CHECK (organization_mode IN ('playlist_folder', 'flat')),
    format_profile TEXT NOT NULL DEFAULT 'mp3_192',
    existing_file_policy TEXT NOT NULL DEFAULT 'ask' CHECK (existing_file_policy IN ('ask', 'reuse', 'overwrite', 'rename')),
    cancel_requested_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT
);

CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_created_at ON jobs(created_at);

-- 5. Job Items
CREATE TABLE job_items (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    track_id TEXT REFERENCES tracks(id) ON DELETE SET NULL,
    playlist_track_id TEXT REFERENCES playlist_tracks(id) ON DELETE SET NULL,
    playlist_position INTEGER,
    status TEXT NOT NULL CHECK (status IN ('pending', 'queued', 'downloading', 'validating', 'converting', 'tagging', 'completed', 'paused', 'retry_wait', 'skipped', 'cancelled', 'failed', 'interrupted', 'waiting_for_duplicate')),
    priority_offset INTEGER NOT NULL DEFAULT 0,
    progress_percent REAL,
    downloaded_bytes INTEGER,
    estimated_total_bytes INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT,
    temporary_path TEXT,
    output_path TEXT,
    error_code TEXT,
    error_message TEXT,
    execution_lease_expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT
);

CREATE INDEX idx_job_items_job_id ON job_items(job_id);
CREATE INDEX idx_job_items_status ON job_items(status);
CREATE INDEX idx_job_items_track_id ON job_items(track_id);
CREATE INDEX idx_job_items_playlist_track_id ON job_items(playlist_track_id);

-- 6. Item Reservations / Leases (exclusión mutua entre jobs)
CREATE TABLE item_reservations (
    youtube_video_id TEXT NOT NULL,
    format_profile TEXT NOT NULL,
    owner_job_item_id TEXT NOT NULL REFERENCES job_items(id) ON DELETE CASCADE,
    lease_expires_at TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (youtube_video_id, format_profile)
);

CREATE INDEX idx_item_reservations_owner ON item_reservations(owner_job_item_id);

-- 7. Local Files (catálogo de resultados conocidos)
CREATE TABLE local_files (
    id TEXT PRIMARY KEY NOT NULL,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    playlist_track_id TEXT REFERENCES playlist_tracks(id) ON DELETE SET NULL,
    format_profile TEXT NOT NULL DEFAULT 'mp3_192',
    path TEXT UNIQUE NOT NULL,
    size_bytes INTEGER NOT NULL,
    modified_at TEXT NOT NULL,
    validation_status TEXT NOT NULL CHECK (validation_status IN ('valid', 'corrupted', 'missing', 'unverified')),
    validated_at TEXT NOT NULL,
    video_id_tag TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_local_files_track ON local_files(track_id);
CREATE INDEX idx_local_files_playlist_track ON local_files(playlist_track_id);

-- 8. Settings (configuración tipada)
CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Inserción de configuración predeterminada
INSERT OR IGNORE INTO settings (key, value) VALUES
    ('default_output_directory', ''),
    ('organization_mode', 'playlist_folder'),
    ('existing_file_policy', 'ask'),
    ('max_concurrent_downloads', '2'),
    ('max_concurrent_conversions', '1'),
    ('max_retries', '3'),
    ('check_updates', 'true');

-- 9. Activity Events (registro de auditoría y diagnóstico local)
CREATE TABLE activity_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    old_state TEXT,
    new_state TEXT,
    payload TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_activity_events_entity ON activity_events(entity_type, entity_id);
CREATE INDEX idx_activity_events_created_at ON activity_events(created_at);
