# Decisiones de Fase 0

## 1. Arquitectura Base

El proyecto sigue una arquitectura domain-driven con separación en capas:

- **domain/**: Entidades y estados del dominio
- **persistence/**: Repositorios y migraciones SQLite
- **audio/**: Conversión y etiquetado de audio
- **youtube/**: Extracción de metadata y playlists
- **filesystem/**: Operaciones de archivo y movimiento seguro
- **processes**: Gestión de procesos con Job Objects
- **scheduler/**: Orquestador de descarga
- **commands/**: Comandos Tauri
- **events/**: Sistema de eventos

## 2. Migraciones

Las migraciones se ejecutan automáticamente al iniciar la aplicación. Se crea un backup preventivo del archivo de base de datos antes de aplicar migraciones si ya existe físicamente.

### Migración 0001 — Fase 0 Inicial
Crea tablas base para runs de spike y configuración.

### Migración 0002 — Esquema de Dominio Completo
Crea las tablas principales: playlists, tracks, playlist_tracks, jobs, job_items, item_reservations, local_files, settings, activity_events.

### Migración 0003 — Corrección CHECK e Índices
Corrige el CHECK constraint de `existing_file_policy` en `jobs` para incluir `'fail_if_exists'` (SQLite no permite ALTER CHECK). Añade índices faltantes para rendimiento del scheduler.

## 3. Detección de Volumen en `move_file_safey`

Para soportar correctamente volúmenes NTFS, FAT32/exFAT y redes, se implementó `detect_volume_kind` usando `GetVolumeInformationW` de la API de Windows. Esto permite:

- **NTFS**: Rename atómico con reemplazo atómico cuando el destino existe
- **FAT32/exFAT/Unknown**: Copia a `.tmp.part`, validación de tamaño, luego rename
- **Red/Unknown**: Estrategia conservadora con copy+delete

## 4. Job Objects

Los procesos hijos se asocian a Job Objects de Windows para garantizar que terminen cuando el padre muere. Esto previene procesos huérfanos.

## 5. Smoke Test de Actualizaciones

Tras reemplazar el binario de yt-dlp, se ejecuta `yt-dlp --version` con:
- Timeout de 10 segundos
- stdout drenado
- Si falla o devuelve una versión no parseable, se restaura el backup inmediatamente
- Si la versión devuelta coincide con la anterior (no se actualizó), se considera fallo

Esta verificación está implementada en `updater.rs::update_binary()` y cubierta por tests de integración que validan el flujo de descarga, verificación de hash SHA-256 y rollback en caso de fallo.

## 6. Estrategia por Tipo de Volumen en `move_file_safely`

El archivo `filesystem/mod.rs` ahora distingue tres tipos de volumen:

- **Ntfs**: Rename atómico + reemplazo atómico con reintentos ante sharing violation
- **Fat**: Copia a `.tmp.part`, validación de tamaño, luego rename
- **Unknown**: Estrategia conservadora con copy+delete y reintentos

La detección se hace con `GetVolumeInformationW` y requiere el feature `Win32_Storage_FileSystem` en `windows-sys`.
