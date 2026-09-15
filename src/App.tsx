import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

interface ExtractionResult {
  total: number;
  available: number;
  unavailable: number;
  playlist_id: string;
}

interface Job {
  id: string;
  kind: string;
  status: string;
  priority: number;
  source_url: string;
  output_directory: string;
  created_at: string;
}

interface JobEvent {
  type: string;
  job_id?: string;
  previous_status?: string;
  new_status?: string;
  reason?: string;
  processed?: number;
  total?: number;
}

function formatErrorMessage(err: unknown): string {
  if (!err) return "Ha ocurrido un error inesperado.";

  let rawMessage = "";

  if (typeof err === "string") {
    rawMessage = err;
  } else if (typeof err === "object") {
    const errorObj = err as Record<string, unknown>;
    if (typeof errorObj.userMessage === "string") {
      return errorObj.userMessage;
    }
    if (typeof errorObj.user_message === "string") {
      return errorObj.user_message;
    }
    if (typeof errorObj.message === "string") {
      rawMessage = errorObj.message;
    } else {
      try {
        return JSON.stringify(err, null, 2);
      } catch {
        return String(err);
      }
    }
  } else {
    return String(err);
  }

  // Verificar si rawMessage contiene un objeto JSON clasificado (ej. error de yt-dlp)
  const jsonStart = rawMessage.indexOf("{");
  const jsonEnd = rawMessage.lastIndexOf("}");
  if (jsonStart !== -1 && jsonEnd > jsonStart) {
    try {
      const parsed = JSON.parse(rawMessage.slice(jsonStart, jsonEnd + 1));
      if (parsed && typeof parsed === "object") {
        const userMsg = parsed.userMessage || parsed.user_message || parsed.message;
        if (typeof userMsg === "string") {
          const prefix = rawMessage.slice(0, jsonStart).trim().replace(/:\s*$/, "");
          return prefix ? `${prefix}: ${userMsg}` : userMsg;
        }
      }
    } catch {
      // Mantener rawMessage
    }
  }

  return rawMessage;
}

export function App() {
  const [url, setUrl] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [defaultOutputDir, setDefaultOutputDir] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [currentJobId, setCurrentJobId] = useState<string | null>(null);
  const [extractionProgress, setExtractionProgress] = useState<string | null>(null);
  const [extractionResult, setExtractionResult] = useState<ExtractionResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Cargar directorio por defecto
    invoke<string>("get_default_output_directory")
      .then(setDefaultOutputDir)
      .catch(() => {});

    // Escuchar eventos Tauri
    const unlisten = listen<JobEvent>("job-state-changed", ({ payload }) => {
      console.log("Job state changed:", payload);
    });

    const unlisten2 = listen<JobEvent>("extraction-progress", ({ payload }) => {
      if (payload.processed !== undefined && payload.total !== undefined) {
        setExtractionProgress(`Extrayendo: ${payload.processed}/${payload.total}`);
      }
    });

    return () => {
      void unlisten.then((dispose) => dispose());
      void unlisten2.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    if (defaultOutputDir) {
      setOutputDir(defaultOutputDir);
    }
  }, [defaultOutputDir]);

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

  async function handleCreateJob(e: React.FormEvent) {
    e.preventDefault();
    if (!isValidUrl || isCreating) return;

    setError(null);
    setExtractionResult(null);
    setExtractionProgress(null);
    setIsCreating(true);

    try {
      // 1. Crear el Job
      const job = await invoke<Job>("create_job", {
        sourceUrl: url.trim(),
        outputDirectory: outputDir || null,
        formatProfile: "mp3_192",
        existingFilePolicy: "rename",
      });

      if (job) {
        setCurrentJobId(job.id);
        setExtractionProgress("Iniciando extracción...");

        // 2. Extraer items de la playlist
        const result = await invoke<ExtractionResult>("extract_and_enqueue_items", {
          jobId: job.id,
        });

        setExtractionResult(result);
        setExtractionProgress(null);
      }
    } catch (err) {
      setError(formatErrorMessage(err));
      setExtractionProgress(null);
    } finally {
      setIsCreating(false);
    }
  }

  return (
    <main>
      <header>
        <p className="eyebrow">Fase 2 · Jobs y Playlists</p>
        <h1>YT Playlist Downloader</h1>
        <p className="subtitle">
          Gestor de descarga de playlists con persistencia y control de trabajos.
        </p>
      </header>

      <section className="card">
        <form onSubmit={handleCreateJob}>
          <div className="form-group">
            <label htmlFor="yt-url">Enlace de YouTube, YouTube Music o playlist:</label>
            <input
              id="yt-url"
              type="url"
              placeholder="https://www.youtube.com/watch?v=... o https://www.youtube.com/playlist?list=..."
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={isCreating}
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
                disabled={isCreating}
              />
              <button
                type="button"
                className="secondary"
                onClick={handleSelectFolder}
                disabled={isCreating}
              >
                Cambiar
              </button>
            </div>
          </div>

          <div className="actions">
            <button
              type="submit"
              disabled={!isValidUrl || isCreating}
              className="primary"
            >
              {isCreating ? "Procesando…" : "Crear Job y Extraer Playlist"}
            </button>
          </div>
        </form>
      </section>

      {extractionProgress && (
        <section className="card progress-card" aria-live="polite">
          <div className="progress-header">
            <span className="stage-title">{extractionProgress}</span>
          </div>
        </section>
      )}

      {extractionResult && (
        <section className="card success-card" aria-live="polite">
          <div className="success-icon">✓</div>
          <div className="success-details">
            <h3>Extracción completada</h3>
            <p>Tracks encontrados: <strong>{extractionResult.total}</strong></p>
            <p>Disponibles: <strong>{extractionResult.available}</strong></p>
            {extractionResult.unavailable > 0 && (
              <p>No disponibles: <strong>{extractionResult.unavailable}</strong></p>
            )}
            <p>Playlist ID: <code>{extractionResult.playlist_id}</code></p>
            {currentJobId && <p>Job ID: <code>{currentJobId}</code></p>}
          </div>
        </section>
      )}

      {error && (
        <section className="card error-card" role="alert">
          <div className="error-title">Error</div>
          <p className="error-message">{error}</p>
        </section>
      )}
    </main>
  );
}
