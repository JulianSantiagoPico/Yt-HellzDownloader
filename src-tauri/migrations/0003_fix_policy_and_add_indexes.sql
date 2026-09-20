-- Migración 0003: Corrige CHECK constraint de existing_file_policy y añade índices faltantes

-- 1. Corregir CHECK constraint de existing_file_policy en jobs
-- SQLite no soporta ALTER TABLE ... ALTER COLUMN ... ADD CHECK.
-- Se recrea la tabla jobs con el valor 'fail_if_exists' incluido.

CREATE TABLE jobs_new (
    id TEXT PRIMARY KEY NOT NULL,
    playlist_id TEXT REFERENCES playlists(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN ('import', 'sync', 'retry', 'single_download')),
    status TEXT NOT NULL CHECK (status IN ('created', 'extracting', 'queued', 'running', 'paused', 'cancelling', 'cancelled', 'completed', 'completed_with_errors', 'failed')),
    priority INTEGER NOT NULL DEFAULT 0,
    source_url TEXT NOT NULL,
    output_directory TEXT NOT NULL,
    organization_mode TEXT NOT NULL DEFAULT 'playlist_folder' CHECK (organization_mode IN ('playlist_folder', 'flat')),
    format_profile TEXT NOT NULL DEFAULT 'mp3_192',
    existing_file_policy TEXT NOT NULL DEFAULT 'ask' CHECK (existing_file_policy IN ('ask', 'reuse', 'overwrite', 'rename', 'fail_if_exists')),
    cancel_requested_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT
);

INSERT INTO jobs_new SELECT * FROM jobs;
DROP TABLE jobs;
ALTER TABLE jobs_new RENAME TO jobs;

-- Recrear índices de jobs
CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_created_at ON jobs(created_at);
CREATE INDEX idx_jobs_playlist_id ON jobs(playlist_id);

-- 2. Índices faltantes para rendimiento

-- Para dispatch de items: el scheduler necesita buscar items queued por job
CREATE INDEX idx_job_items_job_status ON job_items(job_id, status);

-- Para queries de local_files por playlist y perfil
CREATE INDEX idx_local_files_format_profile ON local_files(format_profile);
