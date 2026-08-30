import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface HealthStatus {
  database: string;
  dataDirectory: string;
}

interface ToolProgress {
  runId: string;
  stream: "stdout" | "stderr";
  line: string;
}

export function App() {
  const [health, setHealth] = useState<HealthStatus>();
  const [output, setOutput] = useState<string[]>([]);
  const [runId, setRunId] = useState<string>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    const unlisten = listen<ToolProgress>("tool-progress", ({ payload }) => {
      setOutput((current) => [...current.slice(-99), `[${payload.stream}] ${payload.line}`]);
    });
    invoke<HealthStatus>("health_check").then(setHealth).catch((reason) => setError(String(reason)));
    return () => { void unlisten.then((dispose) => dispose()); };
  }, []);

  async function runProbe() {
    setError(undefined);
    setOutput([]);
    try {
      const id = crypto.randomUUID();
      setRunId(id);
      await invoke("run_tool", { request: { runId: id, tool: "ffprobe", args: ["-version"] } });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setRunId(undefined);
    }
  }

  return (
    <main>
      <p className="eyebrow">Fase 0 · Spike técnico</p>
      <h1>YT Playlist Downloader</h1>
      <section>
        <h2>Estado local</h2>
        <dl>
          <div><dt>SQLite</dt><dd>{health?.database ?? "Inicializando…"}</dd></div>
          <div><dt>Datos</dt><dd>{health?.dataDirectory ?? "…"}</dd></div>
        </dl>
      </section>
      <section>
        <h2>Sidecar controlado</h2>
        <div className="actions">
          <button onClick={runProbe} disabled={Boolean(runId)}>Ejecutar ffprobe</button>
          <button className="secondary" disabled={!runId} onClick={() => invoke("cancel_tool", { runId })}>Cancelar</button>
        </div>
        <pre>{output.join("\n") || "Sin salida todavía."}</pre>
      </section>
      {error && <p role="alert">{error}</p>}
    </main>
  );
}
