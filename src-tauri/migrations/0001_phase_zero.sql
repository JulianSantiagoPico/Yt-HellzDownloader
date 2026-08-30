CREATE TABLE spike_runs (
    id TEXT PRIMARY KEY NOT NULL,
    tool TEXT NOT NULL CHECK (tool IN ('yt-dlp', 'ffmpeg', 'ffprobe')),
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TEXT,
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'cancelled', 'failed')),
    exit_code INTEGER
);

CREATE TABLE tool_versions (
    tool TEXT PRIMARY KEY NOT NULL,
    version TEXT NOT NULL,
    sha256 TEXT,
    source TEXT NOT NULL,
    verified_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
