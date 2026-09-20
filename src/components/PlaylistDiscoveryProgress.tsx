import React from "react";

export interface PlaylistDiscoveryProgressProps {
  jobId?: string | null;
  discoveredCount: number;
  availableCount: number;
  unavailableCount: number;
  totalEstimated?: number | null;
  isExtracting: boolean;
  statusText?: string;
  onCancel?: () => void;
}

export function PlaylistDiscoveryProgress({
  discoveredCount,
  availableCount,
  unavailableCount,
  totalEstimated,
  isExtracting,
  statusText = "Extrayendo playlist...",
  onCancel,
}: PlaylistDiscoveryProgressProps) {
  const hasTotal =
    totalEstimated !== undefined &&
    totalEstimated !== null &&
    totalEstimated > 0;
  const percent = hasTotal
    ? Math.min(100, Math.round((discoveredCount / totalEstimated) * 100))
    : null;

  return (
    <section className="card progress-card" aria-live="polite">
      <div className="progress-header">
        <div className="progress-title-area">
          <span className="stage-title">{statusText}</span>
          <span className="discovery-stats">
            Descubiertos: <strong>{discoveredCount}</strong>
            {hasTotal ? ` / ${totalEstimated}` : ""}
            {" · "}
            <span className="avail-badge">{availableCount} disponibles</span>
            {unavailableCount > 0 && (
              <span className="unavail-badge">
                {" · "}
                {unavailableCount} no disponibles
              </span>
            )}
          </span>
        </div>
        {percent !== null && (
          <span className="percent-label">{percent}%</span>
        )}
      </div>

      <div className="progress-track" role="progressbar" aria-valuenow={percent ?? undefined} aria-valuemin={0} aria-valuemax={100}>
        <div
          className={`progress-fill ${!hasTotal && isExtracting ? "indeterminate" : ""}`}
          style={{ width: percent !== null ? `${percent}%` : "100%" }}
        />
      </div>

      {isExtracting && onCancel && (
        <div className="discovery-actions">
          <button
            type="button"
            className="secondary small danger-hover"
            onClick={onCancel}
          >
            Cancelar Extracción
          </button>
        </div>
      )}
    </section>
  );
}
