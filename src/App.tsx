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

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
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

  useEffect(() => {
    invoke<string>("get_default_output_directory")
      .then(setOutputDir)
      .catch((err) => console.error("No se pudo obtener directorio por defecto:", err));

    const unlisten = listen<StageProgress>("download-progress", ({ payload }) => {
      setProgress(payload);
    });

    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);

  const isValidUrl =
    url.trim().startsWith("https://") &&
    (url.includes("youtube.com") || url.includes("youtu.be"));

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

  return (
    <main>
      <header>
        <p className="eyebrow">Fase 1 · Núcleo de descarga</p>
        <h1>YT Playlist Downloader</h1>
        <p className="subtitle">
          Descarga de audio en MP3 a 192 kbps estéreo con carátula y metadatos verificados.
        </p>
      </header>

      <section className="card">
        <form onSubmit={handleDownload}>
          <div className="form-group">
            <label htmlFor="yt-url">Enlace de YouTube o YouTube Music:</label>
            <input
              id="yt-url"
              type="url"
              placeholder="https://www.youtube.com/watch?v=..."
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
          <div className="error-title">Error en el proceso</div>
          <p className="error-message">{error}</p>
        </section>
      )}
    </main>
  );
}
