# Plan de completación: Fases 0, 1 y 2 — AJUSTADO Y EJECUTADO

## Resumen de ejecución

Este plan fue contrastado contra el codebase real y ejecutado con las siguientes desviaciones:

- **Tarea 0.1 (ed25519)** — Postergada. No hay servidor de actualizaciones que firme el manifiesto, por lo que implementar la verificación criptográfica ahora no aporta seguridad real. Se retomará cuando exista backend.
- **Tarea 0.3 + 2.1** — Fusionadas en una sola: la migración 0003 crea el CHECK constraint corregido y los 3 índices necesarios.
- **Tarea 1.1** — El scheduler se implementó usando las funciones reales existentes: `youtube::download_audio_stream`, `audio::convert_to_mp3`, `audio::tag_mp3`, `audio::validate_mp3` (no las asumidas en el plan original).
- **Tarea 2.2** — Se añadieron 4 tests para migración 0003 (los tests de reservas y recuperación ya existían en `persistence/mod.rs`).

## Orden de ejecución real

```
1. ✅ Migración 0003 (CHECK fail_if_exists + 3 índices)
2. ✅ Feature Win32_Storage_FileSystem en Cargo.toml
3. ✅ Detección de volumen + refactor move_file_safely (Tarea 0.2)
4. ✅ Scheduler completo (Tarea 1.1) con tests
5. ✅ Integración en AppState/lib.rs (Tareas 1.2)
6. ✅ Wiring con commands (Tareas 1.3, 1.4)
7. ✅ Tests de migración 0003 (Tarea 2.2)
8. ✅ Documentación smoke test (Tarea 0.4)
9. ⏳ Tarea 0.1 (ed25519) — pendiente de servidor de actualizaciones
```

## Tareas completadas

### Fase 0

- ✅ **0.2**: Detección de volumen en `move_file_safely` con `GetVolumeInformationW`
- ✅ **0.3/2.1**: Migración 0003 con CHECK constraint corregido y 3 índices nuevos
- ✅ **0.4**: Documentación de smoke test en `docs/decisiones-fase-0.md`

### Fase 1

- ✅ **1.1**: Scheduler completo con pipeline end-to-end (download → convert → tag → validate → move → register)
- ✅ **1.2**: Integración en AppState y arranque en background
- ✅ **1.3**: Wiring con extract_and_enqueue_items, pause_job, resume_job, cancel_job, retry_failed_items
- ✅ **1.4**: Cancelación por job mediante CancellationToken por job

### Farea 2

- ✅ **2.1**: Migración 0003 aplicada y testeada
- ✅ **2.2**: Tests de migración añadidos
- ✅ **2.3**: Consistencia schema-código verificada

## Archivos creados/modificados

| Archivo | Acción |
|---|---|
| `src-tauri/migrations/0003_fix_policy_and_add_indexes.sql` | **Nuevo** — CHECK + índices |
| `src-tauri/Cargo.toml` | +`Win32_Storage_FileSystem`, +`rand` |
| `src-tauri/src/filesystem/mod.rs` | Refactor con detección de volumen |
| `src-tauri/src/scheduler/mod.rs` | **Nuevo** — Orquestador completo |
| `src-tauri/src/scheduler/tests.rs` | Tests del scheduler (en mod.rs) |
| `src-tauri/src/lib.rs` | Integración scheduler en AppState |
| `src-tauri/src/commands/mod.rs` | Wiring scheduler ↔ commands |
| `src-tauri/src/persistence/mod.rs` | Tests de migración 0003 |
| `docs/decisiones-fase-0.md` | **Nuevo** — Documentación smoke test |

## Tests ejecutados

```
48 tests passed, 0 failed
- filesystem: 8 tests (incluyendo detección de volumen, FAT strategy, retry)
- persistence: 9 tests (incluyendo migración 0003)
- scheduler: 6 tests (incluyendo classify_error, backoff)
- processes: 8 tests
- audio: 2 tests
- youtube: 15 tests
```

## Criterios de salida cumplidos

| Fase | Criterio | Estado |
|---|---|---|
| 0 | `move_file_safely` maneja NTFS/FAT/unknown con estrategias distintas | ✅ |
| 0 | `fail_if_exists` inserta sin error de CHECK | ✅ |
| 0 | Smoke test documentado | ✅ |
| 1 | Scheduler ejecuta pipeline end-to-end con concurrencia | ✅ |
| 1 | Pausa/resume/cancel funcionan por job | ✅ |
| 1 | Backoff exponencial funciona en errores reintentables | ✅ |
| 2 | Migración 0003 aplica limpiamente sobre BD vacía | ✅ |
| 2 | Índices verificados | ✅ |
| 2 | Tests de persistencia pasan | ✅ |
