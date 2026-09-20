import React, { useRef, useState, useEffect, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";

export type AvailabilityType =
  | "available"
  | "private"
  | "deleted"
  | "geo_blocked"
  | "unknown";

export interface PlaylistItemData {
  id: string;
  position: number;
  title: string;
  artist: string;
  durationSeconds?: number | null;
  availability: AvailabilityType;
  downloadStatus?: string | null;
  progressPercent?: number | null;
}

export interface VirtualPlaylistTableProps {
  items: PlaylistItemData[];
  totalCount: number;
  selectedIds: Set<string>;
  onSelectionChange: (newSelected: Set<string>) => void;
  filterText?: string;
  isLoading?: boolean;
}

function formatDuration(seconds?: number | null): string {
  if (seconds == null || isNaN(seconds) || seconds <= 0) return "--:--";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export function VirtualPlaylistTable({
  items,
  totalCount,
  selectedIds,
  onSelectionChange,
  filterText = "",
  isLoading = false,
}: VirtualPlaylistTableProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [focusedIndex, setFocusedIndex] = useState<number>(0);

  // Filtrado reactivo en memoria sobre la lista de items
  const filteredItems = useMemo(() => {
    const query = filterText.trim().toLowerCase();
    if (!query) return items;
    return items.filter(
      (item) =>
        item.title.toLowerCase().includes(query) ||
        item.artist.toLowerCase().includes(query) ||
        item.id.toLowerCase().includes(query)
    );
  }, [items, filterText]);

  const filteredIds = useMemo(
    () => new Set(filteredItems.map((item) => item.id)),
    [filteredItems]
  );

  const rowVirtualizer = useVirtualizer({
    count: filteredItems.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 40,
    overscan: 15,
  });

  const handleToggleSelect = useCallback(
    (id: string) => {
      const next = new Set(selectedIds);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      onSelectionChange(next);
    },
    [selectedIds, onSelectionChange]
  );

  const handleSelectAllFiltered = useCallback(() => {
    const next = new Set(selectedIds);
    for (const id of filteredIds) {
      next.add(id);
    }
    onSelectionChange(next);
  }, [selectedIds, filteredIds, onSelectionChange]);

  const handleDeselectAll = useCallback(() => {
    onSelectionChange(new Set());
  }, [onSelectionChange]);

  const handleOmitUnavailable = useCallback(() => {
    const next = new Set(selectedIds);
    for (const item of items) {
      if (item.availability !== "available") {
        next.delete(item.id);
      }
    }
    onSelectionChange(next);
  }, [items, selectedIds, onSelectionChange]);

  // Manejo accesible de navegación por teclado
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (filteredItems.length === 0) return;

      const pageSize = 10;
      let nextIndex = focusedIndex;

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          nextIndex = Math.min(filteredItems.length - 1, focusedIndex + 1);
          break;
        case "ArrowUp":
          e.preventDefault();
          nextIndex = Math.max(0, focusedIndex - 1);
          break;
        case "PageDown":
          e.preventDefault();
          nextIndex = Math.min(filteredItems.length - 1, focusedIndex + pageSize);
          break;
        case "PageUp":
          e.preventDefault();
          nextIndex = Math.max(0, focusedIndex - pageSize);
          break;
        case "Home":
          e.preventDefault();
          nextIndex = 0;
          break;
        case "End":
          e.preventDefault();
          nextIndex = filteredItems.length - 1;
          break;
        case " ":
          e.preventDefault();
          if (filteredItems[focusedIndex]) {
            handleToggleSelect(filteredItems[focusedIndex].id);
          }
          return;
        case "a":
        case "A":
          if (e.ctrlKey || e.metaKey) {
            e.preventDefault();
            handleSelectAllFiltered();
          }
          return;
        default:
          return;
      }

      setFocusedIndex(nextIndex);
      rowVirtualizer.scrollToIndex(nextIndex, { align: "auto" });
    },
    [focusedIndex, filteredItems, handleToggleSelect, handleSelectAllFiltered, rowVirtualizer]
  );

  // Asegurar que el foco permanezca dentro de límites al cambiar filtro
  useEffect(() => {
    if (focusedIndex >= filteredItems.length) {
      setFocusedIndex(Math.max(0, filteredItems.length - 1));
    }
  }, [filteredItems.length, focusedIndex]);

  const isAllFilteredSelected =
    filteredItems.length > 0 &&
    filteredItems.every((item) => selectedIds.has(item.id));

  return (
    <div className="virtual-table-wrapper">
      <div className="table-controls">
        <div className="table-stats">
          <span className="badge selection-badge">
            {selectedIds.size} de {totalCount} seleccionados
          </span>
          {filterText.trim() && (
            <span className="badge filter-badge">
              {filteredItems.length} coinciden con el filtro
            </span>
          )}
        </div>
        <div className="table-actions">
          <button
            type="button"
            className="secondary small"
            onClick={handleSelectAllFiltered}
            title="Seleccionar todas las canciones visibles según el filtro"
          >
            Seleccionar todas {filterText.trim() ? "filtradas" : ""}
          </button>
          <button
            type="button"
            className="secondary small"
            onClick={handleDeselectAll}
            disabled={selectedIds.size === 0}
            title="Limpiar selección actual"
          >
            Deseleccionar todas
          </button>
          <button
            type="button"
            className="secondary small"
            onClick={handleOmitUnavailable}
            title="Desmarcar videos privados o eliminados de la selección"
          >
            Omitir no disponibles
          </button>
        </div>
      </div>

      <div
        className="virtual-grid-container"
        ref={parentRef}
        tabIndex={0}
        role="grid"
        aria-label="Lista de canciones de la playlist"
        aria-rowcount={totalCount}
        aria-busy={isLoading}
        onKeyDown={handleKeyDown}
      >
        <div className="grid-header" role="row">
          <div className="grid-cell select-cell" role="columnheader" aria-label="Selección">
            <input
              type="checkbox"
              aria-label="Seleccionar o deseleccionar todas las canciones filtradas"
              checked={isAllFilteredSelected}
              onChange={(e) => {
                if (e.target.checked) {
                  handleSelectAllFiltered();
                } else {
                  handleDeselectAll();
                }
              }}
            />
          </div>
          <div className="grid-cell pos-cell" role="columnheader">#</div>
          <div className="grid-cell title-cell" role="columnheader">Título</div>
          <div className="grid-cell artist-cell" role="columnheader">Artista</div>
          <div className="grid-cell duration-cell" role="columnheader">Duración</div>
          <div className="grid-cell status-cell" role="columnheader">Estado</div>
        </div>

        <div
          className="virtual-grid-body"
          style={{
            height: `${rowVirtualizer.getTotalSize()}px`,
            width: "100%",
            position: "relative",
          }}
        >
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const item = filteredItems[virtualRow.index];
            if (!item) return null;

            const isSelected = selectedIds.has(item.id);
            const isFocused = virtualRow.index === focusedIndex;
            const isUnavailable = item.availability !== "available";

            let availabilityLabel = "Disponible";
            let availabilityClass = "avail-ok";
            if (item.availability === "private") {
              availabilityLabel = "Privado";
              availabilityClass = "avail-private";
            } else if (item.availability === "deleted") {
              availabilityLabel = "Eliminado";
              availabilityClass = "avail-deleted";
            } else if (item.availability === "geo_blocked") {
              availabilityLabel = "Bloqueado";
              availabilityClass = "avail-geoblocked";
            }

            return (
              <div
                key={item.id}
                role="row"
                aria-rowindex={item.position}
                aria-selected={isSelected}
                className={`grid-row ${isSelected ? "selected" : ""} ${
                  isFocused ? "focused" : ""
                } ${isUnavailable ? "unavailable" : ""}`}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${virtualRow.size}px`,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
                onClick={() => {
                  setFocusedIndex(virtualRow.index);
                  handleToggleSelect(item.id);
                }}
              >
                <div
                  className="grid-cell select-cell"
                  role="gridcell"
                  onClick={(e) => e.stopPropagation()}
                >
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => handleToggleSelect(item.id)}
                    aria-label={`Seleccionar ${item.title}`}
                  />
                </div>
                <div className="grid-cell pos-cell" role="gridcell">
                  {item.position}
                </div>
                <div className="grid-cell title-cell" role="gridcell" title={item.title}>
                  <span className="cell-truncate">{item.title}</span>
                </div>
                <div className="grid-cell artist-cell" role="gridcell" title={item.artist}>
                  <span className="cell-truncate">{item.artist}</span>
                </div>
                <div className="grid-cell duration-cell" role="gridcell">
                  {formatDuration(item.durationSeconds)}
                </div>
                <div className="grid-cell status-cell" role="gridcell">
                  {item.downloadStatus ? (
                    <span className="status-indicator">
                      {item.downloadStatus}
                      {item.progressPercent !== undefined &&
                        item.progressPercent !== null &&
                        ` (${Math.round(item.progressPercent)}%)`}
                    </span>
                  ) : (
                    <span className={`availability-pill ${availabilityClass}`}>
                      {availabilityLabel}
                    </span>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        {filteredItems.length === 0 && !isLoading && (
          <div className="empty-virtual-grid">
            <p>No se encontraron canciones que coincidan con la búsqueda.</p>
          </div>
        )}
      </div>
    </div>
  );
}
