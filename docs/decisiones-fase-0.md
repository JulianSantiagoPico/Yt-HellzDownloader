# Decisiones de Arquitectura — Fase 0

Este documento formaliza las decisiones técnicas vinculantes requeridas por el plan de desarrollo ajustado para cerrar la Fase 0 y guiar las fases posteriores.

---

## 1. Propiedad del Pipeline de Audio y Archivos

Para evitar postprocesadores duplicados, conflictos de concurrencia y resultados inconsistentes, cada etapa del procesamiento tiene un único componente responsable:

| Etapa | Componente Responsable | Justificación |
|---|---|---|
| **Extracción de metadata** | `yt-dlp` (modo `--dump-single-json` / `--flat-playlist`) | Obtiene identificadores, duración, título, canal y URL de portada sin descargar medios. |
| **Descarga del flujo de audio** | `yt-dlp` (modo `-f bestaudio/ba` sin postprocesador) | Descarga exclusivamente el stream de audio original a un directorio temporal aislado por item (`temp/<item_id>/source.*`). |
| **Descarga de portada** | Cliente HTTP interno de Rust o extracción de thumbnail por `yt-dlp` | Descarga la imagen en temporal controlado (`temp/<item_id>/cover.jpg`). |
| **Recodificación a MP3** | `ffmpeg` ejecutado directamente | Convierte el archivo temporal fuente a MP3 a 192 kbps estéreo constante (`-c:a libmp3lame -b:a 192k`). Es una recodificación de compatibilidad, no un aumento de calidad. |
| **Etiquetado y carátula** | Adaptador de ID3 nativo en Rust | Escribe etiquetas ID3v2.3/ID3v2.4 limpias (Título, Artista, Álbum/Playlist, Portada APIC, URL fuente, y el frame `TXXX:YOUTUBE_VIDEO_ID`). Evita problemas de escape en CLI de FFmpeg. |
| **Validación de integridad** | `ffprobe` (modo JSON estructurado) | Valida que el archivo final sea un MP3 válido, con flujo de audio legible, duración coherente y tasa de bits esperada antes de considerarlo completado. |
| **Movimiento y catálogo** | Subsistema `filesystem` de Rust | Mueve de forma atómica el archivo validado a la carpeta destino elegida, registra en SQLite (`local_files`), y limpia los temporales. |

No se permiten postprocesadores paralelos de `yt-dlp` (como `--extract-audio` o `--embed-thumbnail`) para garantizar trazabilidad y control de fallos en cada etapa.

---

## 2. Terminación Fiable del Árbol de Procesos en Windows

En Windows, llamar a `TerminateProcess` sobre el proceso principal (`child.kill()`) no finaliza los subprocesos hijos que éste haya creado (por ejemplo, si `yt-dlp` invoca procesos auxiliares).

### Decisión técnica:
- Todos los procesos lanzados por la aplicación se asocian inmediatamente tras su creación a un **Windows Job Object**.
- El Job Object se configura con la bandera `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
- Al cancelar una tarea o cerrarse el descriptor del Job Object, el kernel de Windows termina forzosamente y de forma atómica todo el árbol de procesos descendientes.
- La verificación de cancelación comprueba que no queden procesos huérfanos antes de considerar cancelada la operación.

---

## 3. Garantías Atómicas por Volumen y Sistema de Archivos

Diferentes sistemas de archivos en Windows ofrecen distintas garantías para la sustitución de archivos:

### NTFS (Volumen local estándar)
- Soporta renombramiento y reemplazo atómico mediante `MoveFileExW` con `MOVEFILE_REPLACE_EXISTING` cuando origen y destino residen en el mismo volumen.
- Los archivos temporales se ubicarán en una subcarpeta `.temp` dentro de la carpeta destino o del mismo volumen para garantizar que el movimiento final sea un renombramiento atómico instantáneo en el sistema de archivos.

### FAT32 / exFAT / Unidades USB
- No garantizan atomicidad completa en renombramientos que sobrescriben archivos abiertos o ante caídas repentinas de energía.
- Estrategia: copia al directorio destino con sufijo temporal (`.tmp.part`), validación de integridad (`size` y lectura mínima), y posterior reemplazo con control de errores y recuperación en caso de fallo.

### Recursos de Red (SMB / UNC) y Carpetas Sincronizadas (OneDrive / Dropbox / Google Drive)
- Los archivos pueden ser bloqueados temporalmente por el cliente de sincronización o por bloqueos de red oportunistas.
- Estrategia: no asumir atomicidad instantánea. Realizar la operación en temporal local o temporal en destino, reintentar con backoff breve ante errores de archivo bloqueado (`ERROR_SHARING_VIOLATION`), y clasificar el error como transitorio/recuperable si persiste.

---

## 4. Autenticación y Confianza en Actualizaciones de Herramientas

Las actualizaciones independientes de `yt-dlp` deben mitigar el riesgo de descargas maliciosas o enlaces comprometidos:

1. **Manifiesto de canal firmado**: La aplicación incorporará la clave pública ed25519 de confianza del proyecto. Las actualizaciones solo se descargarán si van acompañadas de un manifiesto firmado que especifique versión, URL exacta del asset, SHA-256 y fecha.
2. **Validación criptográfica doble**:
   - Se valida la firma digital del manifiesto con la clave pública embebida.
   - Tras descargar el binario, se verifica que su hash SHA-256 coincida exactamente con el declarado en el manifiesto.
3. **Smoke test aislado**:
   - Antes de activar el binario, se ejecuta `--version` con stdout drenado y timeout estricto.
4. **Conservación de versión anterior (Rollback)**:
   - La versión activa previa se mueve a `previous/`.
   - La versión empaquetada de fábrica en el instalador (`bundled`) nunca se modifica ni elimina y actúa como fallback inmutable de emergencia.
   - Si un trabajo real falla repetidamente tras una actualización, la UI permite restaurar con un solo clic la versión anterior.
