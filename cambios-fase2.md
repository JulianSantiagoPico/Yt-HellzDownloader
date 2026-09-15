# Cambios necesarios para completar la Fase 2 — Dominio y Persistencia

## Resumen ejecutivo

La capa de dominio y persistencia está sólida. El gap está en la **superficie Tauri** (comandos que exponen las capacidades a la UI) y en la **eliminación del spike de la Fase 1** que bypassa el modelo de dominio.

**Alcance de la Fase 2:** Solo persiste la estructura de jobs/items y expone comandos de gestión. La descarga efectiva de archivos se implementa en Fase 4 (scheduler + pipeline). Por ahora, los jobs quedan en estado `Queued` esperando ser procesados.

---

## 0. Preparar UI para el cambio (NO dejarla rota)

**Archivo:** `src/App.tsx`

**Por qué:** Eliminar `download_single_track` deja la app sin funcionalidad. Mientras se construye la UI completa de Fase 2, se debe al menos consumir `create_job` + `extract_and_enqueue_items` para mantener la app funcional.

**Acciones:**

- [ ] Crear interfaz mínima que:
  1. Acepta URL de playlist/video
  2. Llama a `create_job` → obtiene `job_id`
  3. Llama a `extract_and_enqueue_items` → muestra progreso de descubrimiento
  4. Muestra resultado: "X tracks listos para descarga"
- [ ] Eliminar referencias a `download_single_track`, `cancel_download`, `CollisionPolicy` (se reemplaza por `existing_file_policy` string)
- [ ] Mantener la lógica de diagnóstico de errores (`ClassifiedError`) pero adaptarla al nuevo `CommandError` DTO
- [ ] Escuchar eventos `job-state-changed` y `item-progress-changed` para actualizar la UI en tiempo real

---

## 1. Definir `CommandError` y estrategia de errores

**Archivo:** `src-tauri/src/commands/mod.rs` (nuevo tipo) o `src-tauri/src/error.rs`

**Por qué:** Todos los comandos retornan `Result<_, CommandError>` pero el tipo no existe. Necesitamos un error tipado que se serialice a JSON para React.

**Especificación:**

```rust
#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandError {
    #[error("Validación: {message}")]
    #[serde(rename = "validation")]
    Validation { message: String },

    #[error("No encontrado: {message}")]
    #[serde(rename = "notFound")]
    NotFound { message: String },

    #[error("Conflicto: {message}")]
    #[serde(rename = "conflict")]
    Conflict { message: String },

    #[error("Estado inválido: {message}")]
    #[serde(rename = "invalidState")]
    InvalidState { message: String },

    #[error("Interno: {message}")]
    #[serde(rename = "internal")]
    Internal { message: String },
}

impl From<sqlx::Error> for CommandError {
    fn from(e: sqlx::Error) -> Self {
        CommandError::Internal { message: e.to_string() }
    }
}

impl From<youtube::YoutubeError> for CommandError {
    fn from(e: youtube::YoutubeError) -> Self {
        match e {
            youtube::YoutubeError::InvalidUrl(msg) => CommandError::Validation { message: msg },
            youtube::YoutubeError::Private | youtube::YoutubeError::Removed => {
                CommandError::NotFound { message: e.to_string() }
            }
            _ => CommandError::Internal { message: e.to_string() },
        }
    }
}
```

**Mapeo a DTO para React:**

```rust
#[derive(serde::Serialize)]
pub struct ErrorDto {
    pub kind: String,       // "validation" | "notFound" | "conflict" | "invalidState" | "internal"
    pub message: String,
    pub user_message: String, // Mensaje amigable para mostrar
}
```

**Nota:** La UI actual parsea `ClassifiedError` (JSON con `userMessage`, `category`, `rawStderr`). Mantener compatibilidad hacia atrás o migrar completamente al nuevo formato.

---

## 2. Definir payloads de eventos Tauri

**Archivo:** `src-tauri/src/events.rs` (nuevo) o en `commands/mod.rs`

**Por qué:** Los comandos dicen "publicar `job-state-changed`" pero no definen su estructura. React necesita saber qué escuchar y qué datos recibir.

**Especificación:**

```rust
#[derive(Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AppEvent {
    JobStateChanged {
        job_id: String,
        previous_status: String,
        new_status: String,
    },
    ItemProgressChanged {
        job_id: String,
        item_id: String,
        status: String,
        progress: Option<f32>,
    },
    QueueChanged {
        reason: String, // "jobCreated" | "priorityChanged" | "jobCompleted"
    },
    ExtractionProgress {
        job_id: String,
        processed: u32,
        total: u32,
    },
}

impl AppEvent {
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::JobStateChanged { .. } => "job-state-changed",
            AppEvent::ItemProgressChanged { .. } => "item-progress-changed",
            AppEvent::QueueChanged { .. } => "queue-changed",
            AppEvent::ExtractionProgress { .. } => "extraction-progress",
        }
    }
}
```

**Publicación desde comandos:**

```rust
// Dentro de un comando con acceso a tauri::AppHandle
app_handle.emit(AppEvent::name(), &event_payload)?;
```

**Eventos que la UI debe escuchar:**

| Evento | Payload | Cuándo |
|--------|---------|--------|
| `job-state-changed` | `{ jobId, previousStatus, newStatus }` | Transición de estado del job |
| `item-progress-changed` | `{ jobId, itemId, status, progress? }` | Cambio en un item |
| `queue-changed` | `{ reason }` | La cola de jobs se modifica |
| `extraction-progress` | `{ jobId, processed, total }` | Durante extracción de playlist |

---

## 3. Crear comando `create_job`

**Archivo:** `src-tauri/src/commands/mod.rs` (nueva función)
**Archivo:** `src-tauri/src/persistence/repositories/jobs.rs` (extender si necesario)

**Por qué:** Es el comando fundamental que conecta la UI con el modelo de dominio. Sin él no se puede crear ningún trabajo.

**Especificación:**

```rust
#[tauri::command]
async fn create_job(
    state: State<'_, AppState>,
    source_url: String,
    output_directory: Option<String>,
    organization_mode: Option<String>,
    format_profile: Option<String>,
    existing_file_policy: Option<String>,
) -> Result<Job, CommandError>
```

**Flujo interno:**

1. Validar URL (reutilizar `youtube::validate_url`)
2. Normalizar URL y extraer `youtube_playlist_id`
3. Buscar si ya existe una playlist con ese ID
4. Crear registro en `playlists` si no existe
5. Crear registro en `jobs` con:
   - `kind`: `Import` (o `Sync` si la playlist ya existe)
   - `status`: `Created`
   - `priority`: valor por defecto (0)
   - `source_url`: URL normalizada
   - `output_directory`: la elegida o la global (desde `settings`)
   - `organization_mode`, `format_profile`, `existing_file_policy`
6. Publicar evento `queue-changed`
7. Retornar el job creado

**Nota:** La extracción de metadata y creación de items se hará en un paso separado (comando `extract_and_enqueue_items`). Por ahora, `create_job` solo crea el registro base.

---

## 4. Crear comando `extract_and_enqueue_items`

**Archivo:** `src-tauri/src/commands/mod.rs` (nueva función)

**Por qué:** Separar la creación del job de la extracción de metadata permite que la UI muestre progreso de descubrimiento (plan §8.6).

**Especificación:**

```rust
#[tauri::command]
async fn extract_and_enqueue_items(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<ExtractionResult, CommandError>
```

**Flujo interno:**

1. Cargar el job por ID; verificar que esté en estado `Created`
2. Transicionar job a `Extracting`
3. Publicar `job-state-changed`
4. Ejecutar `youtube::extract_playlist` con la `source_url` del job
5. **Procesamiento incremental** (para playlists de 5000):
   - Procesar en lotes de 50-100
   - Publicar `extraction-progress` cada N entradas
6. Por cada entrada extraída:
   a. Upsert en `tracks` (reutilizar `playlists::upsert_track`)
   b. Crear `playlist_track` (reutilizar `playlists::add_playlist_track`)
   c. Crear `job_item` con `track_id` y `playlist_track_id` (reutilizar `job_items::create_job_item`)
7. **Idempotencia:** Si el job ya tiene items (re-extracción), usar `ON CONFLICT(job_id, track_id) DO UPDATE` o rechazar si no está en `Created`
8. Asociar `playlist_id` al job (update)
9. Transicionar job a `Queued` (todos los items están pendientes)
10. Publicar `job-state-changed`
11. Retornar `ExtractionResult { total, available, unavailable, playlist_id }`

**Consideraciones:**

- Los videos privados/eliminados se registran como items con status `Skipped` o `Failed` con error_code apropiado
- Si la extracción falla a mitad, el job queda en `Extracting` (recuperación al arranque lo detecta)
- Publicar eventos `job-state-changed` y `extraction-progress` durante la extracción

---

## 5. Crear comandos de ciclo de vida del job

**Archivo:** `src-tauri/src/commands/mod.rs`

### 5.1 `pause_job`

```rust
#[tauri::command]
async fn pause_job(
    state: State<'_, AppState>,
    job_id: String,
    immediate: bool,
) -> Result<Job, CommandError>
```

- Si `immediate == false`: pausa gradual — no programa nuevos items, los activos terminan su etapa
- Si `immediate == true`: pausa inmediata — **llamar a `processes::kill_job_processes(job_id)`**, items → `Interrupted` o `Paused`
- Transicionar job a `Paused`
- Publicar `job-state-changed`

### 5.2 `resume_job`

```rust
#[tauri::command]
async fn resume_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Job, CommandError>
```

- Verificar que el job esté en `Paused`
- Transicionar a `Queued`
- Items `Paused` o `Interrupted` → `Queued` (para que el scheduler los programe)
- Publicar `job-state-changed`

### 5.3 `cancel_job`

```rust
#[tauri::command]
async fn cancel_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Job, CommandError>
```

- Transicionar job a `Cancelling`
- **Matar procesos:** `processes::kill_job_processes(job_id)`
- **Limpiar temporales:** Eliminar archivos `.part`, `.ytdl`, `.webm` parciales del disco
- Marcar items activos: `Downloading`/`Converting`/etc → `Cancelled`
- Liberar reservas del job
- Transicionar job to `Cancelled`
- Publicar `job-state-changed`

### 5.4 `retry_failed_items`

```rust
#[tauri::command]
async fn retry_failed_items(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<u32, CommandError>
```

- Buscar items del job en estado `Failed`
- **Filtrar por error reintentable:**
  - Reintentables: `network_error`, `timeout`, `rate_limited`, `temporary_failure`
  - No reintentables: `video_not_found`, `private_video`, `age_restricted`, `copyright_claim`
- Para items reintentables: resetear `attempts` a 0, limpiar `error_code`/`error_message`, transicionar a `Queued`
- Para items no reintentables: marcar como `PermanentlyFailed` (o mantener `Failed` con flag)
- Retornar cantidad de items encolados

---

## 6. Crear comando `set_job_priority`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn set_job_priority(
    state: State<'_, AppState>,
    job_id: String,
    priority: i32,
) -> Result<Job, CommandError>
```

- Actualizar `jobs.priority`
- Publicar `queue-changed`

---

## 7. Crear comando `get_job_progress`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn get_job_progress(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobProgress, CommandError>
```

**Respuesta:**

```rust
#[derive(Serialize)]
struct JobProgress {
    job_id: String,
    status: String,
    total_items: u32,
    completed_items: u32,
    failed_items: u32,
    pending_items: u32,
    current_item: Option<CurrentItem>,
    percent_complete: f32,
}

#[derive(Serialize)]
struct CurrentItem {
    item_id: String,
    track_title: String,
    stage: String, // "downloading" | "converting" | "tagging"
    progress: f32,
}
```

**Por qué:** La UI necesita mostrar el progreso de un job activo. Sin este comando, no hay forma de saber cuántos items van ni cuál se está procesando.

---

## 8. Crear comando `get_queue_status`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn get_queue_status(
    state: State<'_, AppState>,
) -> Result<Vec<QueueEntry>, CommandError>
```

**Respuesta:**

```rust
#[derive(Serialize)]
struct QueueEntry {
    job_id: String,
    kind: String,
    status: String,
    priority: i32,
    total_items: u32,
    completed_items: u32,
    created_at: DateTime<Utc>,
}
```

**Por qué:** La UI necesita mostrar la cola de jobs pendientes, ordenados por prioridad.

---

## 9. Crear comando `list_playlists`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn list_playlists(
    state: State<'_, AppState>,
) -> Result<Vec<PlaylistSummary>, CommandError>
```

- Retornar lista de playlists con conteo de tracks, último sync, y estado de jobs asociados
- Reutilizar `playlists::list_playlists` + join con jobs

---

## 10. Crear comando `get_playlist_details`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn get_playlist_details(
    state: State<'_, AppState>,
    playlist_id: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<PlaylistDetails, CommandError>
```

- Retornar playlist + tracks asociados (con paginación para 5000 elementos)
- Incluir estado de descarga de cada track si hay jobs asociados
- Parámetros `offset` y `limit` para paginación (default: offset=0, limit=100)

---

## 11. Crear comando `get_local_files`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn get_local_files(
    state: State<'_, AppState>,
    track_id: Option<String>,
    playlist_id: Option<String>,
) -> Result<Vec<LocalFile>, CommandError>
```

- Consultar archivos locales registrados
- Filtrar por track o playlist si se especifica

---

## 12. Crear comando `resolve_file_conflicts`

**Archivo:** `src-tauri/src/commands/mod.rs`

```rust
#[tauri::command]
async fn resolve_file_conflicts(
    state: State<'_, AppState>,
    job_id: String,
    resolution: String, // "reuse" | "overwrite" | "rename"
    apply_to_all: bool,
) -> Result<u32, CommandError>
```

- Aplicar resolución de conflicto a items en estado que requiere decisión
- Si `apply_to_all == true`, aplicar a todos los conflictos pendientes del job
- Retornar cantidad de items resueltos

---

## 13. Crear comando `export_diagnostic_logs`

**Archivo:** `src-tauri/src/commands/mod.rs`

**Dependencia:** Agregar al `Cargo.toml`:
```toml
zip = "2.2"  # o latest
```

```rust
#[tauri::command]
async fn export_diagnostic_logs(
    state: State<'_, AppState>,
    output_path: String,
) -> Result<ExportResult, CommandError>
```

**Contenido del ZIP (según plan §18 y ajuste §7):**

- Versión de app y herramientas
- Últimos N registros de `activity_events` (sanitizados)
- Resumen de jobs e items (sin URLs completas)
- Settings actuales (sin rutas personales completas si es posible)
- Logs rotativos del directorio de logs

**Sanitización previa a inclusión:**

- Redactar rutas personales completas
- Redactar URLs de video (conservar solo IDs)
- Limitar tamaño de payload de eventos

---

## 14. Integrar `tool_versions` con el updater

**Archivos:** `src-tauri/src/updater.rs`, `src-tauri/src/persistence/repositories/` (nuevo o extender `settings`)

**Por qué:** La tabla `tool_versions` existe pero no se usa. Debe ser la fuente de verdad de qué versión de yt-dlp está activa.

**Acciones:**

- [ ] Al verificar versión actual, insertar/actualizar en `tool_versions`
- [ ] Al activar candidata, mover versión anterior a `previous` en BD
- [ ] Al hacer rollback, restaurar desde BD
- [ ] La tabla `tool_versions` debe tener registro para `yt-dlp`, `ffmpeg`, `ffprobe`
- [ ] Crear función `get_active_tool_version(pool, tool_name) -> Option<ToolVersion>`
- [ ] Crear función `register_tool_version(pool, tool_version) -> Result<()>`

---

## 15. Inventariar migraciones existentes y planificar 0003

**Archivos:** `src-tauri/migrations/0001_phase_zero.sql`, `src-tauri/migrations/0002_domain_schema.sql`

**Acciones previas (inventario):**

- [ ] Listar todas las tablas creadas en 0001 y 0002
- [ ] Listar todos los índices existentes
- [ ] Verificar constraints y foreign keys

**Posibles ajustes para 0003 (confirmar durante implementación):**

- [ ] Verificar que `jobs.kind` incluya `single_download` si se va a conservar (o eliminarlo si el spike se borra)
- [ ] Verificar que `job_items` tiene suficientes índices para las queries del scheduler
- [ ] Añadir índice en `activity_events(entity_type, entity_id)` si se va a consultar por entidad
- [ ] Considerar añadir `updated_at` a `item_reservations` si no existe
- [ ] Añadir `error_category` a `job_items` para distinguir errores reintentables vs permanentes

---

## 16. Eliminar el spike `download_single_track` y `cancel_download`

**Archivo:** `src-tauri/src/commands/mod.rs`

**Por qué:** El plan §5 y el ajuste §5 dictan que el código que acepta ejecución directa de procesos desde React es un spike que debe eliminarse. Ambos comandos bypassan el modelo Job/Item y no persisten estado.

**Acciones:**

- [ ] Eliminar la función `download_single_track` del módulo `commands`
- [ ] Eliminar la función `cancel_download` del módulo `commands`
- [ ] Eliminar sus imports y referencias en `lib.rs` (el bloque `tauri::generate_handler![]`)
- [ ] Eliminar la función `get_default_output_directory` si solo era soporte del spike
- [ ] Verificar que no queden referencias a `ProcessRegistry` o `spawn_in_job` desde comandos expuestos

**Nota:** Conservar el módulo `processes/`, `audio/` y `youtube/` — serán reutilizados por el pipeline real. Solo se eliminan los comandos que exponen ejecución directa.

**Nota de orden:** Este paso se hace **después** de que la UI consuma los nuevos comandos, para no dejar la app rota.

---

## 17. Registrar todos los nuevos comandos en `lib.rs`

**Archivo:** `src-tauri/src/lib.rs`

Añadir al bloque `tauri::generate_handler![]`:

```rust
create_job,
extract_and_enqueue_items,
pause_job,
resume_job,
cancel_job,
retry_failed_items,
set_job_priority,
get_job_progress,
get_queue_status,
list_playlists,
get_playlist_details,
get_local_files,
resolve_file_conflicts,
export_diagnostic_logs,
```

Eliminar del handler:

```rust
download_single_track,
cancel_download,
get_default_output_directory,
```

---

## 18. Añadir estructura de respuesta para la UI

**Archivo:** `src-tauri/src/commands/types.rs` (nuevo archivo)

Definir tipos de respuesta que la UI consumirá:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct ExtractionResult {
    pub total: u32,
    pub available: u32,
    pub unavailable: u32,
    pub playlist_id: String,
}

#[derive(Serialize)]
pub struct PlaylistSummary {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub track_count: u32,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub active_job_count: u32,
}

#[derive(Serialize)]
pub struct PlaylistDetails {
    pub playlist: Playlist,
    pub tracks: Vec<PlaylistTrackWithStatus>,
    pub total: u32,
}

#[derive(Serialize)]
pub struct PlaylistTrackWithStatus {
    pub track: Track,
    pub position: u32,
    pub download_status: Option<String>,
}

#[derive(Serialize)]
pub struct ExportResult {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Serialize)]
pub struct JobProgress {
    pub job_id: String,
    pub status: String,
    pub total_items: u32,
    pub completed_items: u32,
    pub failed_items: u32,
    pub pending_items: u32,
    pub current_item: Option<CurrentItem>,
    pub percent_complete: f32,
}

#[derive(Serialize)]
pub struct CurrentItem {
    pub item_id: String,
    pub track_title: String,
    pub stage: String,
    pub progress: f32,
}

#[derive(Serialize)]
pub struct QueueEntry {
    pub job_id: String,
    pub kind: String,
    pub status: String,
    pub priority: i32,
    pub total_items: u32,
    pub completed_items: u32,
    pub created_at: DateTime<Utc>,
}
```

---

## Orden de implementación recomendado

| Paso | Tarea | Dependencias |
|------|-------|--------------|
| 0 | Preparar UI mínima (`App.tsx`) | Ninguna |
| 1 | Definir `CommandError` y `ErrorDto` | Ninguna |
| 2 | Definir payloads de eventos (`AppEvent`) | Ninguna |
| 3 | Crear tipos de respuesta (`commands/types.rs`) | Ninguna |
| 4 | Crear `create_job` | Pasos 1, 3 |
| 5 | Crear `extract_and_enqueue_items` | Pasos 2, 4 |
| 6 | Crear comandos de ciclo de vida (`pause_job`, `resume_job`, `cancel_job`, `retry_failed_items`) | Pasos 1, 4 |
| 7 | Crear `set_job_priority` | Paso 4 |
| 8 | Crear `get_job_progress` y `get_queue_status` | Paso 4 |
| 9 | Crear `list_playlists` y `get_playlist_details` | Ninguna |
| 10 | Crear `get_local_files` | Ninguna |
| 11 | Crear `resolve_file_conflicts` | Paso 6 |
| 12 | Crear `export_diagnostic_logs` | Ninguna |
| 13 | Integrar `tool_versions` con updater | Ninguna |
| 14 | Inventariar migraciones y crear 0003 | Pasos 4-6 |
| 15 | Eliminar spike (`download_single_track`, `cancel_download`) | Todos los anteriores |
| 16 | Registrar todos los comandos en `lib.rs` | Todos los anteriores |
| 17 | Tests unitarios para nuevos comandos | Cada paso |

---

## Criterios de aceptación (ajuste §8, Fase 2)

Al finalizar estos cambios, debe poder demostrarse que:

1. [ ] Cerrar y abrir la app conserva todos los datos (jobs, items, playlists, settings)
2. [ ] Se puede crear un job desde la UI (vía comando `create_job`)
3. [ ] La extracción de playlist persiste tracks y crea items en BD
4. [ ] Pausar, continuar y cancelar un job funcionan y persisten el estado
5. [ ] Reintentar items fallidos los devuelve a `Queued` (solo errores reintentables)
6. [ ] La recuperación al arranque marca items activos como `interrupted`
7. [ ] Las transiciones son idempotentes (repetir una transición no corrompe datos)
8. [ ] No existe ningún comando que exponga ejecución directa de yt-dlp/FFmpeg con argumentos libres
9. [ ] `tool_versions` se usa como fuente de verdad para versiones de herramientas
10. [ ] `activity_events` se puede exportar para diagnóstico
11. [ ] La UI muestra progreso de extracción y estado de jobs en tiempo real
12. [ ] Los eventos Tauri están documentados y la UI los consume correctamente
13. [ ] Los errores se clasifican y muestran mensajes amigables al usuario
