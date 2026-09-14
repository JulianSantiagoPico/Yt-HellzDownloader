# Pasos para el cierre de la Fase 1 — Núcleo de descarga

## Estado actual

La implementación funcional y las pruebas de robustez están completas. La tubería Descarga → Conversión → Etiquetado → Validación → Movimiento funciona. Los Job Objects, validación estricta de URLs y nombres están operativos y verificados.

---

## Pasos completados

### 1. Calidad estática

- [x] Ejecutar `cargo clippy --all-targets -- -D warnings` y corregir avisos (0 errores, 0 warnings).
- [x] Ejecutar `cargo fmt --check` para verificar formato (verificación limpia).
- [x] Ejecutar `npm run build` y verificar empaquetado Tauri (completado con éxito).

### 2. Tests unitarios

- [x] Test de `move_file_safely` con validación de tamaño e integridad antes de reemplazar (`test_move_file_safely_no_overwrite_without_permission`).
- [x] Test de `resolve_destination_path` con política `FailIfExists` y `Rename` (`test_resolve_destination_path_rename_policy`).
- [x] Test de cancelación durante conversión FFmpeg (`test_convert_to_mp3_cancellation`).
- [x] Test de timeout en extracción de metadata (`test_metadata_extraction_timeout_handling`).
- [x] Test de salida excesiva de procesos acotada a 64KB (`test_bounded_process_output_reading`).

### 3. Seguridad de archivos

- [x] Verificado que `move_file_safely` nunca borra el destino sin autorización `Overwrite` explícita.
- [x] Verificado que el fallback entre volúmenes valida tamaño antes de reemplazar.
- [x] Probado con ruta Unicode larga (250+ caracteres) y caracteres especiales (`test_long_unicode_path_resolution_and_file_creation`).

### 4. Documentación técnica

- [x] Documentada decisión de Fase 0: quién posee cada paso de postproceso (`docs/decisiones-fase-0.md`).
- [x] Documentadas garantías atómicas por tipo de volumen: NTFS, FAT, red (`docs/decisiones-fase-0.md`).
- [x] Documentada estrategia de terminación de árbol de procesos en Windows mediante Job Objects (`docs/decisiones-fase-0.md`).

### 5. Pruebas automatizadas y de tubería

- [x] Flujo completo verificado: generación de audio estéreo, recodificación a MP3 192 kbps (`-ac 2`), incrustación de ID3v2.3 con portada, URL canónica e ID de YouTube, y validación estricta con `ffprobe` (`test_audio_pipeline_convert_tag_validate`).
- [x] Cancelación verificada en árbol de procesos sin dejar procesos huérfanos (`test_job_object_terminates_child_processes`).
- [x] Prevención de colisiones determinista y política `ExistingFilePolicy` configurada en frontend y backend.
- [x] Manejo de carpetas y permisos con fallbacks seguros.

### 6. Empaquetado

- [x] Verificado que el instalador y paquete portable incluyen sidecars en `binaries/` (`yt-dlp.exe`, `ffmpeg.exe`, `ffprobe.exe`).
- [x] Artefacto portable generado en `artifacts/YT-Playlist-Downloader-portable.zip` (95 MB).
- [x] La aplicación ejecuta de forma autónoma sin necesidad de Python ni herramientas en el `PATH`.

---

## Criterio de salida verificado

| Criterio | Estado |
|---|---|
| `cargo test` pasa | Verificado (16 tests ok en 0.14s) |
| `cargo clippy` sin warnings | Verificado (0 warnings con `-D warnings`) |
| `cargo fmt --check` pasa | Verificado |
| `npm run build` + empaquetado Tauri | Verificado (`artifacts/YT-Playlist-Downloader-portable.zip`) |
| MP3 estéreo validado y etiquetado sin sobrescribir | Verificado |
| Cancelar no deja procesos hijos | Verificado (Job Objects) |

---

## Notas

- La migración SQLite actual (`0001_phase_zero.sql`) es temporal; las tablas de jobs/tracks se implementan en Fase 2.
- No hay API genérica de sidecars expuesta al frontend.
- Los UUIDs se generan y validan exclusivamente en Rust.
