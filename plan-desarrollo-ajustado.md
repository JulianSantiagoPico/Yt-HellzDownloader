# Ajustes vinculantes al plan de desarrollo

Este documento complementa [plan-desarrollo.md](plan-desarrollo.md) sin reemplazarlo ni reducir el alcance de su MVP. Cuando haya una contradicción, prevalece este documento. Su fin es convertir decisiones implícitas en contratos implementables y comprobables.

## 1. Alcance y orden de entrega

Se conserva íntegramente el MVP definido en la sección 23 del plan original, incluidas las actualizaciones de `yt-dlp`, recuperación, sincronización, edición portable y soporte de playlists de hasta 5.000 entradas.

Las fases se mantienen, pero se trabajará mediante cortes verticales. Una fase solo se considerará terminada si su criterio de salida pasa en una aplicación empaquetada, no solo en desarrollo. Las funciones de fases posteriores pueden diseñarse antes si son necesarias para que el modelo de datos o la seguridad sean correctos, pero no se presentarán como terminadas antes de sus pruebas de aceptación.

La Fase 0 deberá producir además una decisión escrita sobre:

- Qué componente es dueño de cada paso de postproceso de audio.
- Cómo se termina de forma fiable todo el árbol de procesos en Windows.
- Qué garantías atómicas ofrece cada volumen de destino soportado.
- Cómo se autentica una actualización de herramientas.

## 2. Identidades, datos y deduplicación

### 2.1 Identidad de vídeo, entrada y archivo

`tracks` representa el vídeo global y mantiene `youtube_video_id` único. Una entrada de una playlist es otra entidad: `playlist_tracks` representa una aparición concreta de ese vídeo en una playlist.

Se añaden o aclaran estos campos y restricciones:

| Entidad | Regla |
|---|---|
| `playlist_tracks` | Debe tener `id` UUID propio, `playlist_id`, `track_id`, `position` y, si yt-dlp lo proporciona, `source_entry_id`. Debe conservarse aunque el mismo vídeo aparezca varias veces. |
| `playlist_tracks` | Índice único para `(playlist_id, position)` en el snapshot actual; el modelo de migración debe permitir conservar las posiciones históricas removidas. |
| `job_items` | Debe tener `playlist_track_id` nullable durante extracción, además de `track_id`. Tras resolver la entrada, ambos IDs se guardan antes de encolarlo. |
| `local_files` | Debe referenciar `playlist_track_id` además de `track_id`, o bien una nueva entidad `artifacts` que los relacione. Así se conserva qué MP3 corresponde a qué playlist, posición, álbum y nombre. |
| `local_files` | Debe almacenar el identificador de vídeo escrito en la etiqueta, el perfil, la ruta normalizada, el tamaño, fecha de modificación y resultado de validación. |

El sistema no inferirá que dos archivos son equivalentes solamente por título o ruta. Un MP3 será reutilizable automáticamente solo si su registro y/o etiqueta verificable demuestra la identidad esperada y `ffprobe` confirma el perfil.

### 2.2 Exclusión mutua entre jobs

Se añade una reserva persistente por `(youtube_video_id, format_profile)` para impedir que dos items descarguen y conviertan simultáneamente el mismo vídeo. La reserva incluye `owner_job_item_id`, `lease_expires_at` y `updated_at`.

Al encontrar una reserva activa, un item pasa a `waiting_for_duplicate` y espera su resultado. Si el primer item completa un artefacto válido y compatible, el segundo vuelve a resolver su destino y aplica su política de archivos; si falla, se libera la reserva y el siguiente item puede ejecutarse. Las reservas vencidas se recuperan en el arranque. No se compartirán temporales entre destinos distintos en el MVP: solo se reutilizan artefactos finales ya validados.

## 3. Estados, recuperación e idempotencia

`interrupted` y `waiting_for_duplicate` pasan a ser estados persistidos de `job_items`. Los estados de job incluyen al menos `created`, `extracting`, `queued`, `running`, `paused`, `cancelling`, `cancelled`, `completed`, `completed_with_errors` y `failed`.

Todo cambio de estado debe efectuarse mediante una transición validada por el dominio y una transacción corta que persista, como mínimo, el estado anterior, el nuevo, la marca temporal y el evento de actividad. Una transición repetida por recuperación debe ser inocua.

Antes de lanzar un proceso, el item ya debe tener guardados su estado, ruta temporal, intento y reserva. Al completarlo, la validación del archivo se realiza antes de marcar `completed`. Nunca se deduce que un item terminó solo porque exista una ruta prevista.

Al arrancar, se recuperan como `interrupted` los items con un lease de ejecución no válido, se liberan las reservas vencidas y se inspeccionan únicamente las rutas temporales pertenecientes a esos items. La decisión del usuario de continuar, pausar o cancelar sigue siendo obligatoria; el sistema no reanuda por sí solo.

### Semántica de pausa y cancelación

- **Pausa gradual:** no programa trabajos nuevos; los procesos activos terminan su etapa actual y los items quedan `paused` antes de la siguiente etapa.
- **Pausa inmediata:** marca la intención, termina el árbol de procesos y deja el item en `paused` o `interrupted` según se haya confirmado la terminación. Puede requerir reiniciar la descarga.
- **Cancelación:** es terminal para el item salvo reintento explícito. Solo borra temporales registrados de ese item tras comprobar que continúan bajo el directorio temporal autorizado.

## 4. Pipeline de audio y archivos

La Fase 0 elegirá y documentará un solo dueño por operación. La opción preferida es: yt-dlp descarga el flujo de audio a un temporal controlado; FFmpeg convierte a MP3; un adaptador de etiquetas escribe metadata y portada; ffprobe valida el resultado. No se habilitarán postprocesadores duplicados de yt-dlp para conversión o etiquetado en paralelo.

El perfil `mp3_192` se define como una recodificación de compatibilidad a 192 kbps; no implica mejorar la calidad del audio fuente. El build distribuido de FFmpeg debe incluir el codificador necesario, y la Fase 0 debe verificarlo en el binario empaquetado.

Cada item tiene un directorio temporal propio, generado bajo un directorio administrado en el mismo volumen del destino cuando sea posible. Los nombres temporales no proceden del título remoto. El movimiento final debe verificar que la ruta de origen y destino siguen perteneciendo a los directorios autorizados justo antes de operar.

Para FAT/exFAT, recursos de red, rutas sincronizadas o situaciones en que no exista reemplazo atómico, la aplicación usará copia a temporal del destino, validación y reemplazo con recuperación. La UI puede avisar de que la operación es menos resistente a cortes, pero no debe asumir atomicidad inexistente.

La resolución de nombres debe probar y manejar: Unicode, normalización, rutas extendidas, colisiones insensibles a mayúsculas, nombres reservados, puntos y espacios finales, y caracteres de salto de línea. Las rutas M3U8 se escribirán relativas, en UTF-8, con separadores consistentes; una entrada que empiece por `#` o contenga salto de línea se normaliza para que no sea interpretada como directiva.

## 5. Sidecars, procesos y superficie Tauri

La UI nunca puede enviar una lista libre de argumentos a `yt-dlp`, FFmpeg o ffprobe. La API pública de Tauri se limita a casos de uso de dominio, por ejemplo `create_job`, `resume_job`, `cancel_job`, `retry_failed_items`, `sync_playlist` y `export_diagnostic_logs`. El backend construye argumentos desde tipos validados y plantillas internas.

El código de la Fase 0 que acepte `tool` y `args` directamente desde React es exclusivamente un spike y debe eliminarse o hacerse inaccesible antes de cualquier distribución. Una allowlist de ejecutables no basta si los argumentos siguen siendo arbitrarios.

Cada proceso debe lanzarse sin shell, con stdin cerrado y stdout/stderr drenados de forma continua. Los logs y eventos se limitan en tamaño y frecuencia. En Windows, la implementación debe asociar procesos y descendientes a un mecanismo equivalente a un Job Object; la cancelación solo se considera satisfactoria cuando el test confirma que no sobreviven hijos.

## 6. Actualizaciones y confianza

Además de validar SHA-256, las actualizaciones de `yt-dlp` se autorizan mediante un manifiesto de canal firmado. La aplicación incorpora la clave pública de confianza; el manifiesto identifica versión, URL exacta del asset, hash, fecha y canal. Un manifiesto inválido, caducado o no verificable no permite descargar ni activar binarios.

La candidata se prueba usando únicamente argumentos internos inocuos. La activación conserva un binario anterior y el fallback incluido. Ningún trabajo activo cambia de versión. Las migraciones de base de datos y los cambios de formato de los datos locales deben declarar compatibilidad hacia atrás o impedir explícitamente un downgrade con un mensaje y una copia de seguridad.

La política de distribución debe incluir los avisos, licencias y obligaciones del build concreto de FFmpeg y de las demás herramientas redistribuidas. Antes de distribución pública se firma instalador, ejecutable y manifiestos de actualización.

## 7. Operación, privacidad y experiencia

Antes y durante un job se comprobarán accesibilidad de la carpeta destino y espacio libre. Las estimaciones de tamaño se presentan como aproximadas, con una reserva configurable; no bloquean una descarga por una estimación ausente o poco fiable. Errores de antivirus, archivos bloqueados, disco lleno o cambios de permiso se clasifican y ofrecen una acción recuperable.

La pantalla de importación explica que URLs con contenido privado, regional, con verificación de edad o que requiera iniciar sesión pueden fallar, porque el MVP no admite cookies ni autenticación. La documentación de privacidad especifica que YouTube y el origen de actualizaciones observan la conexión del usuario; la aplicación no añade telemetría ni identificadores propios.

La UI debe poder usarse con teclado: foco restaurado al cerrar diálogos, botones con estados claros, contraste suficiente, etiquetas accesibles y anuncios no intrusivos de los cambios de progreso. Los diálogos de conflicto y recuperación deben poder aplicar una decisión en lote sin perder el foco ni bloquear indefinidamente la cola.

Los diagnósticos incluyen versión de esquema, versión de app/herramientas y categorías de errores, pero la exportación revisa rutas, URLs y texto de procesos antes de incluirlos. Se define retención para logs, eventos y copias de seguridad, con rotación y límites de tamaño.

## 8. Pruebas y criterios añadidos por fase

Además de las pruebas originales, son obligatorias las siguientes:

| Fase | Pruebas de aceptación adicionales |
|---|---|
| 0 | Instalador en una VM limpia sin privilegios; WebView2 ausente; ejecución de los sidecars empaquetados; codificador MP3 disponible; proceso hijo que sobrevive al padre y cuya cancelación se verifica; ruta Unicode/larga. |
| 1 | El MP3 final contiene la identidad prevista, portada y metadata; fallo entre cada etapa no crea un falso `completed`; carpeta no escribible y disco lleno se muestran como error recuperable. |
| 2 | Transiciones repetidas e interrupción recuperada son idempotentes; expiración de leases y reservas; copia de seguridad anterior a migración no compatible. |
| 3 | Fixture de 5.000 entradas con vídeos duplicados y repetidos dentro de una misma playlist; navegación por teclado y tabla virtualizada. |
| 4 | Dos jobs del mismo vídeo no lanzan dos descargas; fairness bajo carga; límites separados de descarga y conversión; no hay eventos de progreso sin límite. |
| 5 | Pausa gradual e inmediata cumplen sus semánticas; cancelación no deja hijos; limpieza no atraviesa junctions, enlaces ni directorios no administrados. |
| 6 | NTFS, FAT/exFAT cuando esté disponible, ruta sincronizada simulada, colisiones por Unicode/mayúsculas y M3U8 con títulos hostiles. |
| 7 | Misma canción en dos playlists conserva sus artefactos y metadata de entrada; sincronización no descarga dos veces un vídeo reservado ni renombra sin confirmación. |
| 8 | Manifiesto firmado válido, hash inválido, firma inválida, manifiesto caducado, candidata fallida y rollback sin afectar jobs pendientes. |
| 9 | Instalador y ZIP portable en al menos dos equipos o VMs Windows 10/11; datos no se eliminan sin confirmación; documentación de SmartScreen, WebView2 y restricciones de contenido. |

## 9. Riesgos añadidos

| Riesgo | Mitigación |
|---|---|
| Dos jobs descargan el mismo vídeo | Reserva persistente por vídeo y perfil, con lease y espera explícita. |
| El frontend convierte la allowlist en ejecución libre | Comandos de dominio; argumentos construidos solo por Rust. |
| Actualización con hash publicado por un origen comprometido | Manifiesto firmado con clave pública embebida y URL exacta. |
| Un estado persistido no refleja el archivo real | Validación previa al estado final y recuperación idempotente. |
| Volumen no ofrece renombrado atómico | Estrategia de copia, validación y recuperación específica por volumen. |
| Diferencias de metadata para el mismo vídeo | Artefacto vinculado a la entrada de playlist, no únicamente al vídeo global. |
| Expectativas de calidad erróneas | Etiquetar 192 kbps como recodificación de compatibilidad. |

## 10. Definition of Done del MVP

El MVP se declara finalizado únicamente cuando satisface todos los criterios de la sección 23 del plan original y las pruebas de aceptación de este documento. En particular, debe existir evidencia repetible de que:

1. El instalador y el ZIP funcionan sin dependencias manuales en Windows soportado.
2. La UI no puede ejecutar herramientas con argumentos libres.
3. Un cierre, una pausa o una cancelación no corrompen datos ni dejan procesos hijos o archivos desconocidos eliminados.
4. La sincronización, deduplicación y reutilización distinguen correctamente vídeo, entrada de playlist y artefacto local.
5. Una actualización de herramientas solo se activa si proviene de un manifiesto confiable, pasa validación y conserva rollback.
6. Los escenarios de 5.000 entradas, rutas hostiles, falta de espacio y conflictos masivos siguen siendo operables y comprensibles para el usuario.
