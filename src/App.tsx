import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type CollisionPolicy = "rename" | "fail_if_exists" | "overwrite";

interface DownloadResult {
  runId: string;
  success: boolean;
  filePath: string;
  title: string;
  artist: string;
  durationSeconds: number;
}

interface StageProgress {
  runId: string;
  stage: string;
  percent: number;
  message: string;
}

interface ClassifiedError {
  userMessage: string;
  category: string;
  rawStderr: string;
}

function parseClassifiedError(raw: string): ClassifiedError | null {
  try {
    const obj = JSON.parse(raw);
    if (obj.userMessage && obj.category && obj.rawStderr !== undefined) {
      return obj as ClassifiedError;
    }
  } catch {
    // not JSON
  }
  return null;
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export interface ReleaseEntry {
  version: string;
  url: string;
  sha256: string;
  sizeBytes: number;
}

interface YtdlpVersionInfo {
  version: string;
  binaryPath: string;
}

interface UpdateResult {
  previousVersion: string;
  newVersion: string;
  success: boolean;
  message: string;
}

interface PlaylistEntry {
  id: string;
  url: string;
  title: string;
  artist: string;
  durationSeconds: number | null;
  index: number;
}

interface PlaylistInfo {
  playlistId: string;
  title: string;
  entryCount: number;
  entries: PlaylistEntry[];
}

export function App() {
  const [url, setUrl] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [collisionPolicy, setCollisionPolicy] = useState<CollisionPolicy>("rename");
  const [isDownloading, setIsDownloading] = useState(false);
  const [currentRunId, setCurrentRunId] = useState<string | null>(null);
  const [progress, setProgress] = useState<StageProgress | null>(null);
  const [result, setResult] = useState<DownloadResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ytdlpVersion, setYtdlpVersion] = useState<YtdlpVersionInfo | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [isUpdating, setIsUpdating] = useState(false);

  useEffect(() => {
    invoke<string>("get_default_output_directory")
      .then(setOutputDir)
      .catch((err) => console.error("No se pudo obtener directorio por defecto:", err));

    invoke<YtdlpVersionInfo>("get_ytdlp_version")
      .then(setYtdlpVersion)
      .catch((err) => console.error("No se pudo obtener versión de yt-dlp:", err));

    const unlisten = listen<StageProgress>("download-progress", ({ payload }) => {
      setProgress(payload);
    });

    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);

  const isValidUrl =
    url.trim().startsWith("https://") &&
    (url.includes("youtube.com") || url.includes("youtu.be")) &&
    (url.includes("/watch?v=") || url.includes("youtu.be/") || url.includes("/playlist?list="));

  async function handleSelectFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: outputDir || undefined,
      });
      if (typeof selected === "string") {
        setOutputDir(selected);
      }
    } catch (err) {
      console.error("Error al abrir diálogo de carpeta:", err);
    }
  }

  async function handleDownload(e: React.FormEvent) {
    e.preventDefault();
    if (!isValidUrl || isDownloading) return;

    setError(null);
    setResult(null);
    const runId = crypto.randomUUID();
    setCurrentRunId(runId);
    setIsDownloading(true);
    setProgress({
      runId,
      stage: "starting",
      percent: 0,
      message: "Iniciando proceso...",
    });

    try {
      const downloadRes = await invoke<DownloadResult>("download_single_track", {
        request: {
          clientRunId: runId,
          url: url.trim(),
          outputDir: outputDir || null,
          collisionPolicy,
        },
      });
      setResult(downloadRes);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsDownloading(false);
      setCurrentRunId(null);
    }
  }

  async function handleCancel() {
    if (!currentRunId) return;
    try {
      await invoke("cancel_download", { runId: currentRunId });
    } catch (err) {
      console.error("Error al cancelar:", err);
    }
  }

  async function handleCheckUpdates() {
    setUpdateMessage(null);
    setIsUpdating(true);
    try {
      const releases = await invoke<ReleaseEntry[]>("check_ytdlp_updates", {
        manifestUrl: "https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/yt-dlp-manifest.json",
      });
      if (releases.length === 0) {
        setUpdateMessage("No hay actualizaciones disponibles.");
      } else {
        const latest = releases[0];
        if (ytdlpVersion && latest.version === ytdlpVersion.version) {
          setUpdateMessage(`Ya tienes la versión más reciente: ${latest.version}`);
        } else {
          const res = await invoke<UpdateResult>("update_ytdlp", { release: latest });
          setUpdateMessage(res.message);
          if (res.success) {
            const newVersion = await invoke<YtdlpVersionInfo>("get_ytdlp_version");
            setYtdlpVersion(newVersion);
          }
        }
      }
    } catch (err) {
      setUpdateMessage(`Error al actualizar: ${String(err)}`);
    } finally {
      setIsUpdating(false);
    }
  }

  return (
    <main>
      <header>
        <p className="eyebrow">Fase 1 · Núcleo de descarga</p>
        <h1>YT Playlist Downloader</h1>
        <p className="subtitle">
          Descarga de audio en MP3 a 192 kbps estéreo con carátula y metadatos verificados.
        </p>
      </header>

      {ytdlpVersion && (
        <section className="card version-card">
          <div className="version-info">
            <span className="version-label">yt-dlp:</span>
            <span className="version-value">{ytdlpVersion.version}</span>
          </div>
          <button
            type="button"
            className="secondary"
            onClick={handleCheckUpdates}
            disabled={isUpdating || isDownloading}
          >
            {isUpdating ? "Actualizando…" : "Buscar actualización"}
          </button>
          {updateMessage && (
            <p className="update-message">{updateMessage}</p>
          )}
        </section>
      )}

      <section className="card">
        <form onSubmit={handleDownload}>
          <div className="form-group">
            <label htmlFor="yt-url">Enlace de YouTube, YouTube Music o playlist:</label>
            <input
              id="yt-url"
              type="url"
              placeholder="https://www.youtube.com/watch?v=... o https://www.youtube.com/playlist?list=..."
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={isDownloading}
              required
            />
          </div>

          <div className="form-group">
            <label>Carpeta de destino:</label>
            <div className="dir-picker">
              <input
                type="text"
                readOnly
                value={outputDir || "Cargando carpeta predeterminada…"}
                disabled={isDownloading}
              />
              <button
                type="button"
                className="secondary"
                onClick={handleSelectFolder}
                disabled={isDownloading}
              >
                Cambiar
              </button>
            </div>
          </div>

          <div className="form-group">
            <label htmlFor="collision-policy">Si el archivo ya existe:</label>
            <select
              id="collision-policy"
              value={collisionPolicy}
              onChange={(e) => setCollisionPolicy(e.target.value as CollisionPolicy)}
              disabled={isDownloading}
              className="policy-select"
            >
              <option value="rename">Renombrar automáticamente (ej. Título (1).mp3)</option>
              <option value="fail_if_exists">Detener y avisar (no sobrescribir)</option>
              <option value="overwrite">Sobrescribir archivo existente</option>
            </select>
          </div>

          <div className="actions">
            <button
              type="submit"
              disabled={!isValidUrl || isDownloading}
              className="primary"
            >
              {isDownloading ? "Procesando…" : "Descargar MP3 (192 kbps)"}
            </button>
            {isDownloading && (
              <button
                type="button"
                className="danger"
                onClick={handleCancel}
              >
                Cancelar
              </button>
            )}
          </div>
        </form>
      </section>

      {isDownloading && progress && (
        <section className="card progress-card" aria-live="polite">
          <div className="progress-header">
            <span className="stage-title">{progress.message}</span>
            <span className="percent-label">{Math.round(progress.percent)}%</span>
          </div>
          <div className="progress-track">
            <div
              className="progress-fill"
              style={{ width: `${Math.min(100, Math.max(0, progress.percent))}%` }}
            />
          </div>
          <p className="stage-step">Etapa: <strong>{progress.stage}</strong></p>
        </section>
      )}

      {result && (
        <section className="card success-card" aria-live="polite">
          <div className="success-icon">✓</div>
          <div className="success-details">
            <h3>¡Descarga y conversión exitosa!</h3>
            <p className="track-title"><strong>{result.title}</strong></p>
            <p className="track-artist">{result.artist} • {formatDuration(result.durationSeconds)}</p>
            <p className="track-path"><code>{result.filePath}</code></p>
          </div>
        </section>
      )}

      {error && (
        <section className="card error-card" role="alert">
          <ErrorDisplay rawError={error} onExport={outputDir} />
        </section>
      )}
    </main>
  );
}

function ErrorDisplay({ rawError, onExport }: { rawError: string; onExport: string }) {
  const [expanded, setExpanded] = useState(false);
  const classified = parseClassifiedError(rawError);

  const handleExport = () => {
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const filename = `diagnostico-error-${timestamp}.txt`;
    const path = `${onExport}/${filename}`;

    const content = [
      `Diagnóstico de error - ${new Date().toLocaleString()}`,
      `Categoría: ${classified?.category ?? 'Error en el proceso'}`,
      '',
      '--- Mensaje ---',
      classified?.userMessage ?? rawError,
      '',
      '--- stderr completo ---',
      classified?.rawStderr ?? rawError,
    ].join('\n');

    const blob = new Blob([content], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  };

  if (classified) {
    return (
      <>
        <div className="error-title" data-category={classified.category}>
          {classified.category}
        </div>
        <p className="error-message" style={{ whiteSpace: "pre-line" }}>
          {classified.userMessage}
        </p>
        <div className="error-actions">
          <button
            type="button"
            className="error-details-toggle"
            onClick={() => setExpanded(!expanded)}
          >
            {expanded ? "Ocultar detalles" : "Ver detalles técnicos"}
          </button>
          <button
            type="button"
            className="error-details-toggle export-diagnostic"
            onClick={handleExport}
          >
            Exportar diagnóstico
          </button>
        </div>
        {expanded && (
          <pre className="error-raw-stderr">{classified.rawStderr}</pre>
        )}
      </>
    );
  }

  return (
    <>
      <div className="error-title">Error en el proceso</div>
      <p className="error-message">{rawError}</p>
      <div className="error-actions">
        <button
          type="button"
          className="error-details-toggle export-diagnostic"
          onClick={handleExport}
        >
          Exportar diagnóstico
        </button>
      </div>
    </>
  );
}
