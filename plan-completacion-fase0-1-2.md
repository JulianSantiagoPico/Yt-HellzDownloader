# Plan de completación: Fases 0, 1 y 2

Este documento detalla las tareas necesarias para cerrar los gaps abiertos en las fases 0, 1 y 2 del plan de desarrollo, contrastando la implementación actual con `plan-desarrollo.md` y `plan-desarrollo-ajustado.md`.

---

## Fase 0 — Cerrar gaps abiertos

### Tarea 0.1: Firma ed25519 del manifiesto de actualizaciones

**Archivo afectado:** `src-tauri/src/updater.rs`
**Dependencia nueva:** `ed25519-dalek` en `Cargo.toml`

El manifiesto remoto actual se parsea como JSON plano sin ninguna verificación criptográfica. El plan ajustado §6 exige firma ed25519 con clave pública embebida.

**Cambios:**

1. Añadir `ed25519-dalek` y `hex` a `[dependencies]` en `Cargo.toml`.
2. Embedir clave pública ed25519 como constante:
   ```rust
   const PUB_KEY: [u8; 32] = /* clave generada y hardcodeada */;
   ```
3. Cambiar la estructura del JSON remoto para envolver el payload:
   ```json
   {
     "payload": "{\"current_version\":\"...\",\"releases\":[...]}",
     "signature": "hex_encoded_ed25519_signature"
   }
   ```
4. Añadir función de verificación:
   ```rust
   fn verify_manifest_signature(payload: &str, signature_hex: &str) -> Result<(), String> {
       let sig_bytes = hex::decode(signature_hex)
           .map_err(|_| "Firma con hex inválido".into())?;
       let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());
       let key = ed25519_dalek::VerifyingKey::from_bytes(&PUB_KEY).unwrap();
       key.verify(payload.as_bytes(), &sig)
           .map_err(|_| "Firma del manifiesto inválida".into())
   }
   ```
5. Invocar en `check_for_updates()` antes de fusionar con el local:
   ```rust
   let remote_text = response.text().await?;
   let wrapper: ManifestWrapper = serde_json::from_str(&remote_text)?;
   verify_manifest_signature(&wrapper.payload, &wrapper.signature)?;
   let remote_manifest: UpdateManifest = serde_json::from_str(&wrapper.payload)?;
   ```
6. Si la firma es inválida, rechazar el manifiesto completo sin fusionar ni guardar.

**Tests:**
- Firma válida → manifiesto aceptado.
- Firma inválida → error y manifiesto rechazado.
- Payload alterado → error.
- Hex inválido → error.

---

### Tarea 0.2: Estrategia por tipo de volumen en `move_file_safely`

**Archivo afectado:** `src-tauri/src/filesystem/mod.rs`
**Dependencia nueva:** `windows-sys` (ya presente en `Cargo.toml`)

`move_file_safely` actual solo intenta `fs::rename` y luego fallback a copy+delete sin distinguir volumen. El plan ajustado §4 y `decisiones-fase-0.md` §3 exigen estrategias distintas por tipo de sistema de archivos.

**Cambios:**

1. Añadir detección de volumen:
   ```rust
   #[derive(Debug, PartialEq, Eq)]
   enum VolumeKind {
       Ntfs,
       Fat,
       Unknown,
   }

   fn detect_volume_kind(path: &Path) -> VolumeKind {
       // Usar GetVolumeInformationW de windows-sys
       // Si el volumen es NTFS → VolumeKind::Ntfs
       // Si FAT32/exFAT → VolumeKind::Fat
       // Si falla o es red → VolumeKind::Unknown
   }
   ```

2. Refactorizar `move_file_safely` con tres ramas:
   ```rust
   pub fn move_file_safely(src: &Path, dst: &Path, allow_overwrite: bool) -> Result<(), String> {
       // 1. Si dst no existe y mismo volumen → rename atómico
       if !dst.exists() {
           if fs::rename(src, dst).is_ok() {
               return Ok(());
           }
       }

       let volume = detect_volume_kind(dst);

       match volume {
           VolumeKind::Ntfs => {
               // Rename con reemplazo atómico
               if dst.exists() && allow_overwrite {
                   fs::rename(src, dst).map_err(|e| /* ... */)?;
                   return Ok(());
               }
           }
           VolumeKind::Fat | VolumeKind::Unknown => {
               // Copia a .tmp.part, validar tamaño, reemplazar
               let temp_part = dst.with_extension("mp3.tmp.part");
               fs::copy(src, &temp_part).map_err(|e| /* ... */)?;
               // Validar tamaño
               let src_len = fs::metadata(src)?.len();
               let part_len = fs::metadata(&temp_part)?.len();
               if src_len != part_len || src_len == 0 {
                   fs::remove_file(&temp_part)?;
                   return Err("Integridad fallida".into());
               }
               // Reemplazar
               if dst.exists() && allow_overwrite {
                   fs::remove_file(dst)?;
               }
               fs::rename(&temp_part, dst)?;
               fs::remove_file(src)?;
               return Ok(());
           }
       }

       // 2. Fallback: copy + delete con reintentos para sharing violation
       let temp_dst = dst.with_extension(format!("tmp.{}", Uuid::new_v4()));
       // ... implementación actual con reintentos de 100/200/400ms
   }
   ```

3. Añadir reintentos para `ERROR_SHARING_VIOLATION` (código 32):
   ```rust
   fn retry_on_sharing_violation<F, T>(mut f: F, max_retries: u32) -> Result<T, String>
   where F: FnMut() -> Result<T, std::io::Error> {
       for attempt in 0..=max_retries {
           match f() {
               Ok(val) => return Ok(val),
               Err(e) if attempt < max_retries && e.raw_os_error() == Some(32) => {
                   std::thread::sleep(Duration::from_millis(100 * 2u64.pow(attempt)));
               }
               Err(e) => return Err(e.to_string()),
           }
       }
       unreachable!()
   }
   ```

**Tests:**
- Mock de volumen FAT: verificar que `.tmp.part` se crea.
- Verificar que `fs::rename` se usa en ruta NTFS cuando dst no existe.
- Verificar reintento en sharing violation.

---

### Tarea 0.3: Corregir CHECK constraint `fail_if_exists`

**Archivo nuevo:** `src-tauri/migrations/0003_fix_fail_if_exists_and_indexes.sql`

El schema actual en `0002_domain_schema.sql:69` define:
```sql
CHECK (existing_file_policy IN ('ask', 'reuse', 'overwrite', 'rename'))
```

Pero `filesystem/mod.rs:23` define `FailIfExists` y `commands/mod.rs:110` lo maneja. Esto causará error al intentar insertar `'fail_if_exists'` en `jobs`.

**Solución:** Recrear la tabla `jobs` con el CHECK actualizado. SQLite no soporta `ALTER TABLE ... ALTER COLUMN ... ADD CHECK`.

```sql
-- 0003_fix_fail_if_exists_and_indexes.sql

-- Recrear tabla jobs con CHECK actualizado
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

-- Recrear índices
CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_created_at ON jobs(created_at);
CREATE INDEX idx_jobs_playlist_id ON jobs(playlist_id);
```

**Tests:**
- Aplicar migración sobre BD con datos existentes → sin pérdida.
- Insertar job con `existing_file_policy = 'fail_if_exists'` → sin error de CHECK.
- Insertar job con `existing_file_policy = 'ask'` → sigue funcionando.

---

### Tarea 0.4: Documentar smoke test en decisiones-fase-0

**Archivo afectado:** `docs/decisiones-fase-0.md`

El smoke test ya está implementado en `updater.rs:224-241` (`get_current_version` después de reemplazar + rollback en `Err`). Falta documentarlo formalmente.

**Añadir sección §5:**
```markdown
## 5. Smoke Test de Actualizaciones

Tras reemplazar el binario de yt-dlp, se ejecuta `yt-dlp --version` con:
- Timeout de 10 segundos.
-stdout drenado.
- Si falla o devuelve una versión no parseable, se restaura el backup inmediatamente.
- Si la versión devuelta coincide con la anterior (no se actualizó), se considera fallo.

Esta verificación está implementada en `updater.rs::update_binary()` y cubierta por
el test `test_update_binary_rollback_on_failure`.
```

---

## Fase 1 — Orquestador de descarga completo

### Tarea 1.1: Módulo scheduler — orquestador de ítems

**Archivo nuevo:** `src-tauri/src/scheduler/mod.rs`

Responsabilidades:
- Seleccionar el próximo `job_item` en estado `Queued` respetando prioridad del job + `priority_offset` + `playlist_position`.
- Aplicar concurrencia configurable con semáforos `tokio::sync::Semaphore`.
- Ejecutar el pipeline secuencial por ítem.

**Estructura:**

```rust
use std::path::PathBuf;
use std::sync::Arc;
use sqlx::SqlitePool;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::domain::states::JobItemStatus;

pub struct DownloadScheduler {
    pool: SqlitePool,
    resource_dir: PathBuf,
    download_semaphore: Arc<Semaphore>,
    conversion_semaphore: Arc<Semaphore>,
    shutdown_token: CancellationToken,
    notify: Notify,
}

impl DownloadScheduler {
    pub fn new(
        pool: SqlitePool,
        resource_dir: PathBuf,
        max_downloads: usize,
        max_conversions: usize,
    ) -> Self { /* ... */ }

    /// Notifica al scheduler que hay nuevos items disponibles.
    pub fn notify_new_items(&self) {
        self.notify.notify_one();
    }

    /// Señala al scheduler que pare de forma ordenada.
    pub fn shutdown(&self) {
        self.shutdown_token.cancel();
    }

    /// Bucle principal del scheduler.
    pub async fn run(&self) {
        loop {
            tokio::select! {
                _ = self.shutdown_token.cancelled() => break,
                _ = self.notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(500)) => {},
            }

            // Seleccionar items listos para ejecutar
            let ready_items = self.select_ready_items().await;
            for item in ready_items {
                let pool = self.pool.clone();
                let resource_dir = self.resource_dir.clone();
                let dl_sem = self.download_semaphore.clone();
                let cv_sem = self.conversion_semaphore.clone();
                let token = self.shutdown_token.clone();

                tokio::spawn(async move {
                    process_item(pool, resource_dir, item, dl_sem, cv_sem, token).await;
                });
            }
        }
    }

    /// Selecciona items encolados respetando prioridad.
    async fn select_ready_items(&self) -> Vec<JobItemRow> {
        // 1. Contar slots disponibles en semáforos
        // 2. Query: SELECT ji.*, j.priority, j.status
        //    FROM job_items ji JOIN jobs j ON ji.job_id = j.id
        //    WHERE ji.status = 'queued'
        //      AND j.status IN ('queued', 'running')
        //    ORDER BY j.priority DESC, ji.created_at ASC
        //    LIMIT ?
        // 3. Filtrar: el job no puede estar pausado o cancelling
    }
}
```

**Pipeline por ítem (`process_item`):**

```rust
async fn process_item(
    pool: SqlitePool,
    resource_dir: PathBuf,
    item: JobItemRow,
    dl_sem: Arc<Semaphore>,
    cv_sem: Arc<Semaphore>,
    token: CancellationToken,
) {
    let _dl_permit = dl_sem.acquire().await.unwrap();
    let item_id = item.id.clone();
    let job_id = item.job_id.clone();

    // 0. Preparar directorio temporal
    let dest_dir = Path::new(&item.output_directory);
    let (temp_dir, same_volume) = filesystem::prepare_item_temp_dir(
        dest_dir,
        &PathBuf::from(&item.temp_base),
        &Uuid::parse_str(&item_id).unwrap(),
    ).await.unwrap();

    // 1. Adquirir reserva exclusiva
    let video_id = &item.youtube_video_id;
    let reservation = reservations::acquire_or_wait_reservation(
        &pool, video_id, &item.format_profile, &item_id, 300
    ).await.unwrap();

    match reservation {
        AcquireReservationResult::WaitingForDuplicate { .. } => {
            // Esperar a que el primer item termine y reevaluar
            return;
        }
        AcquireReservationResult::Acquired => {}
    }

    // 2. Transicionar a Downloading
    transition_job_item_status(&pool, &item_id, JobItemStatus::Downloading, None, None).await;

    // 3. Descargar audio
    let download_result = youtube::download_audio_stream(
        &resource_dir, &item.source_url, &temp_dir, &token,
        |pct, _| { /* emitir progreso */ }
    ).await;

    let source_path = match download_result {
        Ok(path) => path,
        Err(e) => {
            handle_item_error(&pool, &item_id, &job_id, "download", &e).await;
            return;
        }
    };

    // 4. Transicionar a Converting
    let _dl_permit = dl_permit; // liberar slot de descarga
    let _cv_permit = cv_sem.acquire().await.unwrap();
    transition_job_item_status(&pool, &item_id, JobItemStatus::Converting, None, None).await;

    let mp3_path = temp_dir.join("output.mp3");
    if let Err(e) = audio::convert_to_mp3(&resource_dir, &source_path, &mp3_path, &token).await {
        handle_item_error(&pool, &item_id, &job_id, "convert", &e).await;
        return;
    }

    // 5. Transicionar a Tagging
    transition_job_item_status(&pool, &item_id, JobItemStatus::Tagging, None, None).await;

    let metadata = /* obtener del track */;
    if let Err(e) = audio::tag_mp3(&mp3_path, &metadata, None, Some(&item.playlist_title)).await {
        handle_item_error(&pool, &item_id, &job_id, "tag", &e).await;
        return;
    }

    // 6. Transicionar a Validating
    transition_job_item_status(&pool, &item_id, JobItemStatus::Validating, None, None).await;

    let validation = match audio::validate_mp3(&resource_dir, &mp3_path, &token).await {
        Ok(v) => v,
        Err(e) => {
            handle_item_error(&pool, &item_id, &job_id, "validate", &e).await;
            return;
        }
    };

    // 7. Mover a destino final
    let final_path = /* resolver con resolve_destination_path */;
    if let Err(e) = filesystem::move_file_safely(&mp3_path, &final_path, true).await {
        handle_item_error(&pool, &item_id, &job_id, "move", &e).await;
        return;
    }

    // 8. Registrar en local_files
    let local_file = LocalFile { /* campos */ };
    local_files::register_local_file(&pool, &local_file).await;

    // 9. Limpiar temporales
    let _ = filesystem::clean_item_temp_dir(&temp_dir);

    // 10. Liberar reserva
    let _ = reservations::release_reservation(&pool, video_id, &item.format_profile, &item_id).await;

    // 11. Transicionar a Completed
    transition_job_item_status(&pool, &item_id, JobItemStatus::Completed, None, None).await;

    // 12. Verificar si el job completo terminó
    check_job_completion(&pool, &job_id).await;
}
```

**Manejo de errores:**

```rust
async fn handle_item_error(
    pool: &SqlitePool,
    item_id: &str,
    job_id: &str,
    stage: &str,
    error: &str,
) {
    let classified = classify_error(error);
    let error_code = classified.code.to_string();

    // Transicionar a Failed
    transition_job_item_status(
        pool, item_id, JobItemStatus::Failed,
        None, Some(&error_code),
    ).await;

    // Liberar reserva
    let _ = reservations::release_reservation(pool, &video_id, &profile, item_id).await;

    // Limpiar temporales
    if let Some(temp_path) = get_item_temp_path(pool, item_id).await {
        let _ = filesystem::clean_item_temp_dir(Path::new(&temp_path));
    }

    // Si es reintentable, programar reintento
    if classified.retryable {
        let delay = compute_backoff(attempt);
        schedule_retry(pool, item_id, delay).await;
    }
}

fn classify_error(error: &str) -> ClassifiedError {
    if error.contains("timeout") || error.contains("network") {
        ClassifiedError { code: "network_error", retryable: true }
    } else if error.contains("private") || error.contains("deleted") {
        ClassifiedError { code: "video_unavailable", retryable: false }
    } else if error.contains("rate") {
        ClassifiedError { code: "rate_limited", retryable: true }
    } else {
        ClassifiedError { code: "unknown", retryable: false }
    }
}

fn compute_backoff(attempt: u32) -> Duration {
    let base = Duration::from_secs(2);
    let max = Duration::from_secs(300);
    let jitter = Duration::from_millis(rand::random::<u64>() % 1000);
    let delay = base.saturating_mul(2u32.saturating_pow(attempt));
    delay.min(max) + jitter
}
```

---

### Tarea 1.2: Integración con AppState

**Archivo afectado:** `src-tauri/src/lib.rs`

```rust
use std::sync::Arc;
use scheduler::DownloadScheduler;

pub struct AppState {
    pub pool: SqlitePool,
    pub data_directory: String,
    pub processes: ProcessRegistry,
    pub initial_recovery_report: Mutex<persistence::RecoveryReport>,
    pub scheduler: Arc<DownloadScheduler>,  // NUEVO
}
```

En `setup`:
```rust
let max_downloads = settings::get_settings(&pool).await?
    .max_concurrent_downloads as usize;
let max_conversions = settings::get_settings(&pool).await?
    .max_concurrent_conversions as usize;

let scheduler = Arc::new(DownloadScheduler::new(
    pool.clone(),
    resource_dir.clone(),
    max_downloads,
    max_conversions,
));

// Arrancar el loop del scheduler
let scheduler_clone = scheduler.clone();
tauri::async_runtime::spawn(async move {
    scheduler_clone.run().await;
});

app.manage(AppState {
    pool,
    data_directory: data_dir.display().to_string(),
    processes: ProcessRegistry::default(),
    initial_recovery_report: Mutex::new(recovery_report),
    scheduler,
});
```

---

### Tarea 1.3: Wiring con comandos existentes

**Archivo afectado:** `src-tauri/src/commands/mod.rs`

En `extract_and_enqueue_items`, al final (después de transicionar a `Queued`):
```rust
// Notificar al scheduler que hay items nuevos
state.scheduler.notify_new_items();
```

En `pause_job`:
```rust
if immediate {
    // El scheduler debe cancelar las tareas activas de este job
    state.scheduler.pause_job(&job_id).await;
}
```

En `resume_job`:
```rust
// Reactivar items pausados e interrumpidos
state.scheduler.resume_job(&job_id).await;
state.scheduler.notify_new_items();
```

En `cancel_job`:
```rust
// Terminar procesos activos y limpiar
state.scheduler.cancel_job(&job_id).await;
```

En `retry_failed_items`:
```rust
// Después de resetear items a Queued
state.scheduler.notify_new_items();
```

---

### Tarea 1.4: Cancelación por job en el scheduler

**Archivo:** `src-tauri/src/scheduler/mod.rs`

```rust
impl DownloadScheduler {
    pub async fn pause_job(&self, job_id: &str) {
        // Cancelar todas las tareas activas de este job
        if let Some(token) = self.job_tokens.lock().await.remove(job_id) {
            token.cancel();
        }
        // Esperar a que terminen (graceful)
        // Los items quedarán en Interrupted o Paused según la semántica
    }

    pub async fn cancel_job(&self, job_id: &str) {
        if let Some(token) = self.job_tokens.lock().await.remove(job_id) {
            token.cancel();
        }
        // Los items en ejecución se marcarán como Cancelled
    }

    pub async fn resume_job(&self, job_id: &str) {
        // Crear nuevo token para el job
        let token = CancellationToken::new();
        self.job_tokens.lock().await.insert(job_id.to_string(), token);
        self.notify.notify_one();
    }
}
```

---

### Tarea 1.5: Tests del scheduler

**Archivo nuevo:** `src-tauri/src/scheduler/tests.rs`

```rust
#[cfg(test)]
mod tests {
    // Test: selección de items por prioridad
    // - Crear 3 jobs con prioridades 0, 5, -1
    // - Verificar que el scheduler selecciona primero el de prioridad 5

    // Test: límite de concurrencia
    // - Configurar max_downloads = 1
    // - Lanzar 3 items
    // - Verificar que solo 1 se ejecuta a la vez

    // Test: error reintentable genera reintento
    // - Simular error de red
    // - Verificar que el item vuelve a Queued con next_attempt_at

    // Test: error definitivo no reintenta
    // - Simular video privado
    // - Verificar que el item queda en Failed sin reintento

    // Test: cancelación de job
    // - Iniciar job con 3 items
    // - Cancelar job
    // - Verificar que los items se marcan como Cancelled

    // Test: pausa gradual
    // - Iniciar job
    // - Pausar (no inmediato)
    // - Verificar que los items activos terminan y los nuevos no inician

    // Test: prioridad con anti-starvation
    // - Job baja prioridad con 100 items
    // - Job alta prioridad con 10 items
    // - Verificar que el job de baja prioridad eventualmente avanza
}
```

---

## Fase 2 — Persistencia y dominio: cerrar gaps

### Tarea 2.1: Migración 0003 (combinada)

**Archivo nuevo:** `src-tauri/migrations/0003_fix_policy_and_add_indexes.sql`

```sql
-- 1. Corregir CHECK constraint de existing_file_policy en jobs
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
```

**Tests de migración:**
```rust
#[tokio::test]
async fn test_migration_0003_applies_cleanly() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    // Insertar job con fail_if_exists
    sqlx::query("INSERT INTO jobs (id, kind, status, source_url, output_directory, existing_file_policy) VALUES (?, 'import', 'created', 'url', 'dir', 'fail_if_exists')")
        .bind(Uuid::new_v4().to_string())
        .execute(&pool).await.unwrap();

    // Verificar que no falla
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE existing_file_policy = 'fail_if_exists'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_migration_0003_preserves_existing_data() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    // Insertar datos previos
    let job_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO jobs (id, kind, status, source_url, output_directory) VALUES (?, 'import', 'created', 'url', 'dir')")
        .bind(&job_id).execute(&pool).await.unwrap();

    // La migración 0003 se aplicaría después
    // Verificar que el dato persiste
    let retrieved: String = sqlx::query_scalar("SELECT id FROM jobs WHERE id = ?")
        .bind(&job_id).fetch_one(&pool).await.unwrap();
    assert_eq!(retrieved, job_id);
}
```

---

### Tarea 2.2: Tests de persistencia ampliados

**Archivo afectado:** `src-tauri/src/persistence/mod.rs`

```rust
#[tokio::test]
async fn test_two_items_same_video_gets_reservation_conflict() {
    // Crear 2 jobs con items para el mismo video
    // Item 1 adquiere reserva
    // Item 2 intenta adquirir → WaitingForDuplicate
    // Item 1 completa → Item 2 reevalúa
}

#[tokio::test]
async fn test_recovery_detects_temporary_files() {
    // Crear item con temporary_path apuntando a archivo existente
    // Ejecutar recover_on_startup
    // Verificar has_temporary_files = true

    // Crear item con temporary_path apuntando a ruta inexistente
    // Ejecutar recover_on_startup
    // Verificar has_temporary_files = false
}

#[tokio::test]
async fn test_insert_job_with_fail_if_exists_policy() {
    // Insertar job con existing_file_policy = 'fail_if_exists'
    // Verificar que no falla el CHECK constraint
}

#[tokio::test]
async fn test_local_files_indexed_by_format_profile() {
    // Insertar local_files con distintos format_profile
    // Ejecutar query con filtro por format_profile
    // Verificar que usa el índice (EXPLAIN QUERY PLAN)
}

#[tokio::test]
async fn test_job_items_indexed_by_job_id_and_status() {
    // Insertar job_items con distintos statuses
    // Ejecutar query: SELECT ... WHERE job_id = ? AND status = 'queued'
    // Verificar uso de índice idx_job_items_job_status
}
```

---

### Tarea 2.3: Verificar consistencia schema-código

**Checklist de verificación:**

| Campo/Relación | Schema | Código Rust | Estado |
|---|---|---|---|
| `local_files.playlist_track_id` | `0002:124` ✅ | `entities.rs` ✅ | OK |
| `local_files.video_id_tag` | `0002:131` ✅ | `commands/mod.rs:846` ✅ | OK |
| `job_items.playlist_track_id` | `0002:84` ✅ | `entities.rs` ✅ | OK |
| `job_items.waiting_for_duplicate` | `0002:86` ✅ | `states.rs` ✅ | OK |
| `item_reservations.lease_expires_at` | `0002:113` ✅ | `reservations.rs` ✅ | OK |
| `jobs.existing_file_policy` CHECK | `0002:69` ❌ falta `fail_if_exists` | `filesystem/mod.rs:23` ✅ | **PENDIENTE (Tarea 2.1)** |
| `jobs.playlist_id` index | `0002` ❌ no existe | `commands/mod.rs:752` filtra en Rust | **PENDIENTE (Tarea 2.1)** |
| `job_items.(job_id, status)` index | `0002` ❌ no existe | Scheduler necesita query rápido | **PENDIENTE (Tarea 2.1)** |
| `local_files.format_profile` index | `0002` ❌ no existe | Queries por perfil sin índice | **PENDIENTE (Tarea 2.1)** |

---

## Orden de ejecución

```
Tarea 0.3 (migración fail_if_exists + índices)
  │
  ├──→ Tarea 2.1 (migración 0003 aplicada y testeada)
  │
Tarea 0.1 (firma ed25519) ──── puede paralelizarse con 0.3
  │
Tarea 0.2 (volumen detection)
  │
Tarea 1.1 (orquestador scheduler) ← bloqueante para Fase 1
  │
Tarea 1.2 (integración con AppState)
  │
Tarea 1.3 (wiring con commands)
  │
Tarea 1.4 (cancelación por job)
  │
Tarea 0.4 (documentar smoke test)
  │
Tarea 2.2 (tests de persistencia)
  │
Tarea 2.3 (verificar consistencia)
  │
Tarea 1.5 (tests del scheduler)
```

## Criterios de salida por fase

| Fase | Criterio de salida |
|---|---|
| **0 completada** | Manifiesto firmado rechazado si firma inválida. `move_file_safely` maneja NTFS/FAT/unknown con estrategias distintas. `fail_if_exists` inserta sin error de CHECK. Smoke test documentado en `decisiones-fase-0.md`. |
| **1 completada** | Un job con 3+ items se procesa end-to-end: download → convert → tag → validate → move → register en `local_files`. Pausa detiene nuevos items. Cancelación termina procesos hijos (Job Object). Prioridad afecta orden de selección. Backoff exponencial funciona en errores reintentables. |
| **2 completada** | Migración 0003 aplica limpiamente sobre BD vacía y con datos. Índices verificados con `EXPLAIN QUERY PLAN`. Tests de persistencia pasan: reservas, recuperación, idempotencia, `fail_if_exists`, índices activos. |

## Archivos a crear/modificar

| Archivo | Acción |
|---|---|
| `Cargo.toml` | Añadir `ed25519-dalek` |
| `src-tauri/src/updater.rs` | Firma ed25519 del manifiesto |
| `src-tauri/src/filesystem/mod.rs` | Detección de volumen + estrategias por tipo |
| `src-tauri/migrations/0003_fix_policy_and_add_indexes.sql` | **Nuevo.** CHECK + índices |
| `src-tauri/src/scheduler/mod.rs` | **Nuevo.** Orquestador completo |
| `src-tauri/src/lib.rs` | Integrar scheduler en AppState |
| `src-tauri/src/commands/mod.rs` | Wiring scheduler ↔ commands |
| `src-tauri/src/persistence/mod.rs` | Tests adicionales |
| `docs/decisiones-fase-0.md` | Documentar smoke test |
