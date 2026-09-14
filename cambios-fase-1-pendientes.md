# Cambios pendientes — Fase 1: núcleo de descarga

Este documento convierte la revisión de la implementación actual en tareas concretas. No modifica el alcance del MVP ni sustituye [plan-desarrollo.md](plan-desarrollo.md) o [plan-desarrollo-ajustado.md](plan-desarrollo-ajustado.md).

## Bloqueantes antes de distribuir la Fase 1

### 1. Impedir sobrescritura o borrado silencioso de archivos existentes

**Problema:** `move_file_safely` puede borrar el destino existente al usar su fallback de copia. El flujo de descarga no ha decidido una política de colisión antes de llegar a ese punto.

**Cambios:**

- Crear una política explícita para la Fase 1: por defecto `fail_if_exists` o `rename`; nunca `overwrite` implícito.
- Resolver el destino antes de convertir y comprobar si ya existe.
- Si existe, devolver un error tipado y accionable, o generar un nombre alternativo determinista, por ejemplo `Título (1).mp3`.
- Separar el movimiento sin reemplazo del futuro movimiento con reemplazo explícitamente autorizado.
- El fallback entre volúmenes no debe borrar el destino hasta que exista una política `overwrite` confirmada y la copia temporal se haya validado.

**Pruebas de aceptación:**

- Descargar dos vídeos cuyo título saneado sea igual no borra el primer MP3.
- Si existe un archivo ajeno con el mismo nombre, su contenido y fecha de modificación no cambian.
- Un fallo después de copiar el temporal no deja el destino original borrado.

## Seguridad y aislamiento

### 2. Generar y validar identificadores de ejecución en Rust

**Problema:** el frontend controla `run_id`, que forma parte de una ruta temporal y de la clave de cancelación.

**Cambios:**

- El comando `download_single_track` genera un UUID v4 en Rust; el frontend recibe el ID como resultado inicial o en el primer evento de progreso.
- Alternativamente, si el contrato requiere recibirlo, analizarlo como UUID y rechazar cualquier otro texto.
- Rechazar un ID ya activo; no reemplazar silenciosamente un token de cancelación existente.
- `create_item_temp_dir` debe recibir un tipo de ID validado, nunca una cadena de ruta arbitraria.

**Pruebas de aceptación:**

- IDs como `..`, `../otra-carpeta`, rutas absolutas y cadenas vacías son rechazados antes de crear directorios.
- Dos solicitudes simultáneas no pueden compartir registro de cancelación ni directorio temporal.

### 3. Eliminar la API genérica de ejecución de sidecars

**Problema:** `run_tool` y `cancel_tool` siguen expuestos a Tauri y permiten que React elija argumentos libres para herramientas privilegiadas.

**Cambios:**

- Retirar `run_tool`, `cancel_tool` y `RunToolRequest` de la superficie invocable de Tauri antes de crear un instalador de Fase 1.
- Mantener las utilidades de procesos como API interna de Rust.
- Exponer únicamente comandos de dominio: `download_single_track`, `cancel_download` y los posteriores definidos por el plan.
- Construir argumentos de yt-dlp, FFmpeg y ffprobe exclusivamente desde tipos internos validados.

**Pruebas de aceptación:**

- El frontend no puede invocar ningún comando que acepte `tool` y `args`.
- Una auditoría del `invoke_handler` contiene solo comandos de dominio autorizados.

### 4. Hacer obligatorio el Job Object de Windows

**Problema:** los errores de `assign_process` se ignoran, de modo que una cancelación podría dejar procesos hijos vivos.

**Cambios:**

- Propagar el error de creación, asignación o terminación del Job Object; no continuar si no puede garantizarse la asociación.
- Centralizar el lanzamiento de procesos para que ningún adaptador pueda omitir la asociación.
- Añadir timeout por etapa y un error tipado para proceso bloqueado.

**Pruebas de aceptación:**

- Un ejecutable de prueba que crea un hijo es terminado junto con su padre al cancelar.
- El test falla si queda un hijo vivo después de cancelar o tras un cierre del proceso principal.

## Descarga y metadatos

### 5. Usar validación y normalización de URL robustas

**Cambios:**

- Añadir el crate `url` y analizar la URL con `Url::parse`.
- Exigir `https`, host exacto de la allowlist y ausencia de usuario, contraseña o host ambiguo.
- Normalizar la URL antes de entregarla a yt-dlp.
- Mantener la validación del frontend solo como ayuda visual; el backend es la fuente de verdad.
- Crear un contrato de error breve que no refleje una URL completa no confiable.

**Pruebas de aceptación:**

- Aceptar URLs válidas de `youtube.com`, `www.youtube.com`, `music.youtube.com` y `youtu.be` dentro del alcance definido.
- Rechazar usuario/contraseña, hosts con sufijos maliciosos, puertos inválidos, HTTP y URLs malformadas.

### 6. Completar y verificar el perfil MP3 192 kbps estéreo

**Cambios:**

- Añadir `-ac 2` al comando FFmpeg si el producto promete salida estéreo.
- Conservar `-c:a libmp3lame`, `-b:a 192k` y documentar que es recodificación de compatibilidad.
- Validar con ffprobe códec, duración, canales y bitrate esperado; definir una tolerancia para valores reportados por contenedor/encoder.
- Convertir los errores de validación en categorías accionables.

**Pruebas de aceptación:**

- Una fuente mono genera un MP3 final de dos canales.
- Un MP3 que no es MP3, tiene duración nula o se desvía del perfil es rechazado antes de moverlo al destino.

### 7. Completar etiquetas y portada

**Cambios:**

- Extraer y guardar URL canónica del vídeo y fecha de publicación si yt-dlp la proporciona.
- Escribir `TXXX:YOUTUBE_VIDEO_ID`, `TXXX:YOUTUBE_URL`, título, artista, álbum, portada y fecha disponible en ID3.
- Para la descarga individual, definir un álbum consistente, por ejemplo el canal o `YouTube`; una playlist posterior reemplazará ese valor por el título de playlist.
- Comprobar después de etiquetar que los frames críticos se pueden leer de nuevo.

**Pruebas de aceptación:**

- El MP3 contiene portada válida y los frames de ID, URL, título y artista.
- La falta de portada o fecha no aborta una descarga de audio válida; se registra como metadata parcial.

## Archivos, temporales y errores

### 8. Situar temporales en el volumen correcto y endurecer el fallback

**Cambios:**

- Intentar crear un directorio temporal administrado dentro de la carpeta destino, por ejemplo `.yt-downloader-temp/<uuid>`.
- Si no es posible, usar el directorio local de la app y marcar que el movimiento será entre volúmenes.
- En el fallback, copiar a un temporal único del destino, validar tamaño y reproducibilidad mediante ffprobe, y solo entonces publicar según la política de colisión.
- Limpiar exclusivamente rutas creadas y registradas para ese item.

**Pruebas de aceptación:**

- En NTFS con temporal y destino en el mismo volumen, el movimiento final no hace una copia.
- Ante fallo de copia o corte simulado, se conserva el archivo previo y se puede recuperar o limpiar el temporal registrado.

### 9. Limitar, clasificar y sanear salida de procesos

**Cambios:**

- Limitar bytes de stdout, stderr y JSON de metadata por proceso.
- Guardar diagnóstico completo solo en logs locales rotativos y no devolverlo íntegro a React.
- Usar una plantilla estructurada de progreso de yt-dlp y un parser aislado; no depender del formato humano de `[download]`.
- Emitir progreso con frecuencia limitada y conservar un último evento por etapa.

**Pruebas de aceptación:**

- Un sidecar que produzca salida excesiva no agota memoria ni bloquea el proceso.
- El parser tolera líneas desconocidas y un cambio de formato sin marcar una descarga como completada.

### 10. Endurecer nombres de Windows

**Cambios:**

- Considerar reservados los nombres de dispositivo incluso con extensión (`CON.txt`, `AUX.mp3`, etc.).
- Calcular el límite del nombre a partir de la ruta final, no usar siempre 200 caracteres.
- Añadir pruebas de Unicode, caracteres de control, puntos/espacios finales y colisiones insensibles a mayúsculas.

## Calidad y cierre de fase

### 11. Completar pruebas y calidad estática

**Cambios:**

- Corregir los dos avisos de Clippy `needless_borrow` en las pruebas de audio.
- Hacer fallar las pruebas de integración que requieren sidecars si el job de CI declara que los binarios deberían estar presentes; no deben pasar silenciosamente por ausencia de binarios.
- Añadir pruebas con adaptadores simulados para timeout, cancelación, salida parcial, error de FFmpeg y destino no escribible.
- Ejecutar manualmente una descarga de contenido autorizado, un fallo de red y una cancelación real.

**Criterio de salida de Fase 1:**

1. `cargo test` y `cargo clippy --all-targets -- -D warnings` pasan.
2. `npm run build` y el empaquetado de Tauri pasan.
3. El instalador de prueba funciona en un Windows x64 limpio sin dependencias manuales.
4. Un vídeo público autorizado genera un MP3 estéreo, validado y etiquetado sin sobrescribir archivos ajenos.
5. Cancelar no deja procesos hijos ni temporales fuera de los directorios administrados.
