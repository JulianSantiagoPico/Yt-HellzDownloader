-- Migración 0004: Prepara el esquema para la Fase 3
-- 1. Asegurar 'skipped' en el CHECK constraint de job_items.status
CREATE TABLE job_items_new (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    track_id TEXT REFERENCES tracks(id) ON DELETE SET NULL,
    playlist_track_id TEXT REFERENCES playlist_tracks(id) ON DELETE SET NULL,
    playlist_position INTEGER,
    status TEXT NOT NULL CHECK (status IN (
        'pending', 'queued', 'downloading', 'validating', 'converting',
        'tagging', 'completed', 'paused', 'retry_wait', 'skipped',
        'cancelled', 'failed', 'interrupted', 'waiting_for_duplicate'
    )),
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

INSERT INTO job_items_new SELECT * FROM job_items;
DROP TABLE job_items;
ALTER TABLE job_items_new RENAME TO job_items;

-- Recrear índices de job_items
CREATE INDEX idx_job_items_job_id ON job_items(job_id);
CREATE INDEX idx_job_items_status ON job_items(status);
CREATE INDEX idx_job_items_track_id ON job_items(track_id);
CREATE INDEX idx_job_items_playlist_track_id ON job_items(playlist_track_id);
CREATE INDEX idx_job_items_job_status ON job_items(job_id, status);

-- 2. Índice compuesto para el JOIN y paginación de get_playlist_details
CREATE INDEX IF NOT EXISTS idx_playlist_tracks_playlist_position 
    ON playlist_tracks(playlist_id, position);

-- 3. Índice para búsquedas rápidas de job_items por track y job
CREATE INDEX IF NOT EXISTS idx_job_items_track_job 
    ON job_items(track_id, job_id);
