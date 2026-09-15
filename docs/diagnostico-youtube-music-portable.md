# Diagnóstico: enlace de YouTube Music y ventanas de consola en el portable

**Fecha de comprobación:** 14 de septiembre de 2026  
**Artefacto probado:** `artifacts/portable`, con `yt-dlp.exe` 2026.08.19.  
**Última actualización:** 14 de septiembre de 2026 (refleja el estado actual del código)

## Resultado

El enlace probado es válido para la aplicación:

```text
https://music.youtube.com/watch?v=UnnwqBV5YWk&list=RDAMVMr46x3JsGhLc
```

La validación acepta el host `music.youtube.com`, la ruta `/watch` y el ID
`UnnwqBV5YWk`. Los parámetros extra, como `list`, no impiden la operación. La
aplicación lo normaliza deliberadamente a:

```text
https://www.youtube.com/watch?v=UnnwqBV5YWk
```

Al ejecutar el mismo `yt-dlp.exe` que incluye el portable con los argumentos de
extracción de metadata, el proceso devuelve:

```text
ERROR: [youtube] UnnwqBV5YWk: Sign in to confirm you're not a bot.
Use --cookies-from-browser or --cookies for the authentication.
```

Por tanto, el fallo ocurre antes de leer o convertir metadata: YouTube está
aplicando una comprobación anti-automatización a esa petición. No se debe a que
la URL sea de YouTube Music ni a `&list=...`.

## Estado de las prioridades originales

Las prioridades 1 y 2 de este diagnóstico **ya están implementadas** en el
código. Este documento las mantiene como referencia de lo que se corrigió y
documenta las mejoras adicionales identificadas durante la revisión.

### Prioridad 1 — Ejecutar sidecars sin ventana visible ✅ IMPLEMENTADA

`src-tauri/src/processes/mod.rs:84-85` define `CREATE_NO_WINDOW` (0x08000000)
y lo aplica en `spawn_in_job` mediante
`std::os::windows::process::CommandExt::creation_flags`. Todos los procesos
secundarios (yt-dlp, ffmpeg, ffprobe) pasan por esta función.

### Prioridad 2 — Traducir los bloqueos conocidos de yt-dlp a mensajes útiles ✅ IMPLEMENTADA

`src-tauri/src/youtube/errors.rs` clasifica stderr en: AntiBotBlock,
PrivateVideo, DeletedVideo, RegionalRestriction, NetworkTimeout y Generic. El
frontend (`src/App.tsx:344-375`) muestra categoría, mensaje legible y stderr
expandible.

## Hallazgos de la revisión de código

### 1. Lectura de stderr duplicada

Hay implementaciones casi idénticas de lectura de stderr en `extract_metadata`,
`get_playlist_metadata`, `download_audio_stream` y `convert_to_mp3`. La función
`read_bounded_string` en `processes/mod.rs:105-120` existe pero no se usa.

**Corregido:** consolidada toda la lectura de stderr con `read_bounded_string`.

### 2. FFmpeg no clasifica stderr

`audio/mod.rs` devuelve el stderr crudo de FFmpeg sin procesar, a diferencia de
los errores de yt-dlp que se clasifican.

**Corregido:** añadida clasificación básica de errores de FFmpeg.

### 3. La prueba de normalización no cubre el caso exacto del diagnóstico

Las pruebas existentes usan `music.youtube.com/watch?v=dQw4w9WgXcQ` pero no con
parámetros adicionales como `&list=RDAMVMr46x3JsGhLc`.

**Corregido:** añadida prueba con la URL exacta del diagnóstico.

### 4. No hay prueba de cancelación con verificación de terminación

La propuesta del diagnóstico de verificar que no quedan procesos huérfanos no
tenía implementación automatizada.

**Corregido:** prueba de integración que cancela y verifica terminación.

### 5. El frontend no distingue visualmente las categorías de error

La categoría existe en el backend pero se muestra como texto plano sin estilo
diferenciado.

**Corregido:** mapeo de categorías a colores/íconos en ErrorDisplay.

### 6. La validación de youtu.be es permisiva

`youtube/mod.rs:128-131` acepta rutas con segmentos adicionales (`/extra/path`)
sin error.

**Corregido:** validación estricta del formato `/<id>`.

### 7. No hay logging persistente

El diagnóstico sugirió conservar stderr en registro de diagnóstico. Solo existe
la sección expandible en UI.

**Corregido:** opción "Exportar diagnóstico" con URL, versión, stderr y hora.

### 8. Timeouts hardcodeados

Los timeouts están fijos en 45s, 120s, 300s, 600s sin posibilidad de
configuración sin recompilar.

**Pendiente:** decisión de producto. No se cambia sin consulta.

## Correcciones aplicadas

1. Consolidación de stderr con `read_bounded_string` en los 4 sitios
2. Clasificación de errores de FFmpeg
3. Prueba de normalización con URL exacta del diagnóstico
4. Prueba de cancelación con verificación de terminación
5. Estilos visuales por categoría de error en frontend
6. Validación estricta de `youtu.be/<id>`
7. Botón "Exportar diagnóstico" en la UI

## Pruebas de regresión

1. Prueba automatizada de normalización para el enlace exacto de este informe:
   debe devolver el ID `UnnwqBV5YWk` y la URL canónica de YouTube. ✅
2. Prueba de clasificación de `stderr` para el mensaje anti-bot reproducido. ✅
3. Prueba manual del `.zip` portable desde una carpeta temporal sin consola
   visible durante todas las etapas.
4. Pruebas de error separadas para: vídeo privado, vídeo eliminado, red caída,
   timeout y bloqueo anti-bot. ✅
5. Prueba de cancelación después de ocultar la consola, verificando que no
   quedan procesos `yt-dlp`, `ffmpeg` ni `ffprobe` en ejecución. ✅

## Decisión para este incidente

No modificar la URL ni eliminar el parámetro `list` solucionará este caso. La
corrección inmediata de producto (ocultar ventanas y explicar anti-bot) está
implementada. Las mejoras adicionales de calidad de código y experiencia de
usuario también están implementadas.
