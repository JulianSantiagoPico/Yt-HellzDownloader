import React, { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import {
  VirtualPlaylistTable,
  PlaylistItemData,
} from "./components/VirtualPlaylistTable";
import { PlaylistDiscoveryProgress } from "./components/PlaylistDiscoveryProgress";

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
  total_discovered?: number;
  available?: number;
  unavailable?: number;
  item_id?: string;
  status?: string;
  progress?: number;
}

interface PlaylistTrackDto {
  track: {
    id: string;
    youtubeVideoId: string;
    title: string;
    artist: string;
    durationSeconds?: number | null;
    availability: "available" | "private" | "deleted" | "geo_blocked" | "unknown";
  };
  position: number;
  downloadStatus?: string | null;
  progressPercent?: number | null;
}

interface PlaylistDetailsDto {
  playlist: {
    id: string;
    title: string;
    channel: string;
    sourceUrl: string;
  };
  tracks: PlaylistTrackDto[];
  total: number;
}

function formatErrorMessage(err: unknown): string {
  if (!err) return "Ha ocurrido un error inesperado.";

  if (typeof err === "string") {
    return err;
  }

  if (typeof err === "object" && err !== null) {
    const errorObj = err as Record<string, unknown>;

    if (typeof errorObj.userMessage === "string") {
      return errorObj.userMessage;
    }
    if (typeof errorObj.user_message === "string") {
      return errorObj.user_message;
    }

    if (typeof errorObj.message === "string") {
      const rawMessage = errorObj.message;
      const jsonStart = rawMessage.indexOf("{");
      const jsonEnd = rawMessage.lastIndexOf("}");
      if (jsonStart !== -1 && jsonEnd > jsonStart) {
        try {
          const parsed = JSON.parse(rawMessage.slice(jsonStart, jsonEnd + 1));
          if (parsed && typeof parsed === "object") {
            const userMsg =
              parsed.userMessage || parsed.user_message || parsed.message;
            if (typeof userMsg === "string") {
              const prefix = rawMessage
                .slice(0, jsonStart)
                .trim()
                .replace(/:\s*$/, "");
              return prefix ? `${prefix}: ${userMsg}` : userMsg;
            }
          }
        } catch {
          // Mantener rawMessage
        }
      }
      return rawMessage;
    }

    try {
      return JSON.stringify(err, null, 2);
    } catch {
      return String(err);
    }
  }

  return String(err);
}

export function App() {
  const [url, setUrl] = useState("");
  const [outputDir, setOutputDir] = useState("");
  const [defaultOutputDir, setDefaultOutputDir] = useState("");

  const [isProcessing, setIsProcessing] = useState(false);
  const [isExtracting, setIsExtracting] = useState(false);
  const [currentJob, setCurrentJob] = useState<Job | null>(null);

  // Progreso de descubrimiento
  const [discoveryStats, setDiscoveryStats] = useState<{
    discovered: number;
    available: number;
    unavailable: number;
    total?: number;
  }>({ discovered: 0, available: 0, unavailable: 0 });

  // Tabla virtualizada
  const [tracks, setTracks] = useState<PlaylistItemData[]>([]);
  const [totalTracks, setTotalTracks] = useState<number>(0);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [playlistTitle, setPlaylistTitle] = useState<string>("");

  // Filtro de texto con debounce
  const [searchInput, setSearchInput] = useState("");
  const [debouncedFilter, setDebouncedFilter] = useState("");
  const debounceTimerRef = useRef<NodeJS.Timeout | null>(null);

  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Cargar directorio por defecto
    invoke<string>("get_default_output_directory")
      .then(setDefaultOutputDir)
      .catch(() => {});

    // Escuchar eventos de cambio de estado de Job
    const unlistenJob = listen<JobEvent>("job-state-changed", ({ payload }) => {
      console.log("Job state changed:", payload);
      if (payload.new_status) {
        setCurrentJob((prev) =>
          prev ? { ...prev, status: payload.new_status || prev.status } : null
        );
        if (
          payload.new_status === "cancelled" ||
          payload.new_status === "failed" ||
          payload.new_status === "completed"
        ) {
          setIsExtracting(false);
          setIsProcessing(false);
        }
      }
    });

    // Escuchar eventos de extracción progresiva
    const unlistenExtraction = listen<JobEvent>(
      "extraction-progress",
      ({ payload }) => {
        setDiscoveryStats({
          discovered: payload.total_discovered ?? payload.processed ?? 0,
          available: payload.available ?? payload.processed ?? 0,
          unavailable: payload.unavailable ?? 0,
          total: payload.total && payload.total > 0 ? payload.total : undefined,
        });
      }
    );

    // Escuchar progreso individual de items
    const unlistenItem = listen<JobEvent>(
      "item-progress-changed",
      ({ payload }) => {
        if (!payload.item_id) return;
        setTracks((prev) =>
          prev.map((t) =>
            t.id === payload.item_id || t.title === payload.reason
              ? {
                  ...t,
                  downloadStatus: payload.status ?? t.downloadStatus,
                  progressPercent:
                    payload.progress !== undefined
                      ? payload.progress
                      : t.progressPercent,
                }
              : t
          )
        );
      }
    );

    return () => {
      void unlistenJob.then((d) => d());
      void unlistenExtraction.then((d) => d());
      void unlistenItem.then((d) => d());
    };
  }, []);

  useEffect(() => {
    if (defaultOutputDir && !outputDir) {
      setOutputDir(defaultOutputDir);
    }
  }, [defaultOutputDir, outputDir]);

  // Manejo de debounce para búsqueda rápida (300ms)
  useEffect(() => {
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
    }
    debounceTimerRef.current = setTimeout(() => {
      setDebouncedFilter(searchInput);
    }, 300);

    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, [searchInput]);

  const isValidUrl =
    url.trim().startsWith("https://") &&
    (url.includes("youtube.com") || url.includes("youtu.be")) &&
    (url.includes("/watch?v=") ||
      url.includes("youtu.be/") ||
      url.includes("/playlist?list="));

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

  async function handleCancelCurrentJob() {
    if (!currentJob) return;
    try {
      await invoke("cancel_job", { jobId: currentJob.id });
      setIsExtracting(false);
      setIsProcessing(false);
    } catch (err) {
      console.error("Error al cancelar job:", err);
    }
  }

  async function handleCreateAndExtract(e: React.FormEvent) {
    e.preventDefault();
    if (!isValidUrl || isProcessing || isExtracting) return;

    setError(null);
    setTracks([]);
    setTotalTracks(0);
    setSelectedIds(new Set());
    setPlaylistTitle("");
    setDiscoveryStats({ discovered: 0, available: 0, unavailable: 0 });
    setIsProcessing(true);
    setIsExtracting(true);

    try {
      // 1. Crear el Job
      const job = await invoke<Job>("create_job", {
        sourceUrl: url.trim(),
        outputDirectory: outputDir || null,
        formatProfile: "mp3_192",
        existingFilePolicy: "rename",
      });

      if (!job) {
        throw new Error("No se pudo instanciar el Job");
      }

      setCurrentJob(job);

      // 2. Iniciar extracción en streaming
      const result = await invoke<ExtractionResult>("extract_and_enqueue_items", {
        jobId: job.id,
      });

      setIsExtracting(false);

      // 3. Cargar detalles y lista de canciones mediante JOIN optimizado
      const details = await invoke<PlaylistDetailsDto>("get_playlist_details", {
        jobId: job.id,
        playlistId: result.playlistId,
        offset: 0,
        limit: 5000,
      });

      if (details) {
        setPlaylistTitle(details.playlist.title);
        setTotalTracks(details.total);

        const loadedTracks: PlaylistItemData[] = details.tracks.map((pt) => ({
          id: pt.track.id,
          position: pt.position,
          title: pt.track.title,
          artist: pt.track.artist,
          durationSeconds: pt.track.durationSeconds,
          availability: pt.track.availability,
          downloadStatus: pt.downloadStatus,
          progressPercent: pt.progressPercent,
        }));

        setTracks(loadedTracks);

        // Seleccionar por defecto todas las disponibles
        const autoSelected = new Set(
          loadedTracks
            .filter((t) => t.availability === "available")
            .map((t) => t.id)
        );
        setSelectedIds(autoSelected);
      }
    } catch (err) {
      setError(formatErrorMessage(err));
      setIsExtracting(false);
    } finally {
      setIsProcessing(false);
    }
  }

  return (
    <main>
      <header>
        <p className="eyebrow">Fase 3 · Playlists y UI Escalable</p>
        <h1>YT Playlist Downloader</h1>
        <p className="subtitle">
          Extracción streaming y renderizado de alto rendimiento para playlists de hasta 5.000 canciones.
        </p>
      </header>

      <section className="card">
        <form onSubmit={handleCreateAndExtract}>
          <div className="form-group">
            <label htmlFor="yt-url">Enlace de YouTube, YouTube Music o playlist:</label>
            <input
              id="yt-url"
              type="url"
              placeholder="https://www.youtube.com/playlist?list=... o https://www.youtube.com/watch?v=...&list=..."
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={isProcessing || isExtracting}
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
                disabled={isProcessing || isExtracting}
              />
              <button
                type="button"
                className="secondary"
                onClick={handleSelectFolder}
                disabled={isProcessing || isExtracting}
              >
                Cambiar
              </button>
            </div>
          </div>

          <div className="actions">
            <button
              type="submit"
              disabled={!isValidUrl || isProcessing || isExtracting}
              className="primary"
            >
              {isExtracting
                ? "Extrayendo playlist…"
                : isProcessing
                ? "Procesando…"
                : "Crear Job y Extraer Playlist"}
            </button>
            {isExtracting && (
              <button
                type="button"
                className="danger"
                onClick={handleCancelCurrentJob}
              >
                Cancelar
              </button>
            )}
          </div>
        </form>
      </section>

      {/* Progreso de descubrimiento en streaming */}
      {isExtracting && (
        <PlaylistDiscoveryProgress
          discoveredCount={discoveryStats.discovered}
          availableCount={discoveryStats.available}
          unavailableCount={discoveryStats.unavailable}
          totalEstimated={discoveryStats.total}
          isExtracting={isExtracting}
          statusText="Descubriendo canciones de la playlist en tiempo real..."
          onCancel={handleCancelCurrentJob}
        />
      )}

      {/* Mensaje de error si ocurre */}
      {error && (
        <section className="card error-card" role="alert">
          <div className="error-title">Error</div>
          <p className="error-message">{error}</p>
        </section>
      )}

      {/* Tabla virtualizada de canciones */}
      {tracks.length > 0 && (
        <section className="playlist-results-section">
          <div className="results-header">
            <h2>{playlistTitle || "Playlist extraída"}</h2>
            <div className="filter-input-container">
              <input
                type="text"
                placeholder="Filtrar por título o artista… (Ctrl+A selecciona filtradas)"
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                aria-label="Filtrar canciones de la playlist"
              />
            </div>
          </div>

          <VirtualPlaylistTable
            items={tracks}
            totalCount={totalTracks}
            selectedIds={selectedIds}
            onSelectionChange={setSelectedIds}
            filterText={debouncedFilter}
            isLoading={isProcessing}
          />
        </section>
      )}
    </main>
  );
}
