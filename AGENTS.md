# Guía para agentes

## Propósito y alcance

YT Playlist Downloader es una aplicación local para Windows 10/11 x64. Descarga y organiza contenido de YouTube/YouTube Music mediante sidecars verificados (`yt-dlp`, `ffmpeg` y `ffprobe`). Está construida con Tauri 2, React 19, TypeScript, Rust, SQLx y SQLite.

No se deben añadir cookies, credenciales, telemetría ni llamadas a servicios externos que no sean necesarias para descargar los sidecars fijados o procesar una URL introducida por la persona usuaria.

## Estructura

- `src/`: interfaz React/TypeScript. `App.tsx` orquesta la vista principal; `components/` contiene componentes de UI reutilizables y virtualizados.
- `src-tauri/src/`: backend Rust.
  - `commands/`: comandos Tauri y DTOs que forman el contrato con el frontend.
  - `domain/`: entidades y máquinas de estado del dominio.
  - `persistence/`: conexión, recuperación, migraciones y repositorios SQLx.
  - `scheduler/`, `youtube/`, `audio/`, `processes/`, `filesystem/`: descarga, conversión, procesos secundarios y manejo seguro de archivos.
  - `events.rs`: eventos emitidos hacia el frontend.
- `src-tauri/migrations/`: migraciones SQLite, aplicadas en orden por SQLx.
- `src-tauri/capabilities/default.json`: permisos mínimos de Tauri de la ventana principal.
- `src-tauri/binaries/sidecars.lock.json`: versiones, URL inmutables y hashes SHA-256 de los sidecars.
- `scripts/`: validación de versiones, descarga/verificación de sidecars y empaquetado Windows.
- `.github/workflows/`: CI y publicación.

## Principios de implementación

- Mantén el frontend y el backend sincronizados: todo comando Tauri nuevo o modificado requiere actualizar sus argumentos/resultados en TypeScript y los consumidores de eventos correspondientes.
- Conserva los nombres de payload en `camelCase` en el límite Rust/TypeScript; no expongas entidades de persistencia sin revisar su contrato público.
- Las transiciones de `JobStatus` y `JobItemStatus` deben pasar por los helpers/repositorios de transición. No actualices estados directamente salvo que sea una operación de recuperación o migración cuidadosamente justificada.
- Toda escritura de base de datos debe ser asíncrona mediante SQLx y respetar las migraciones. Agrega una migración nueva; no reescribas migraciones ya aplicadas.
- Las operaciones prolongadas deben respetar cancelación, registrar/limpiar procesos y emitir progreso. No bloquees el hilo de la UI ni uses procesos externos arbitrarios.
- Valida las URLs de YouTube en el backend aunque también exista validación en la UI.
- Mantén el principio de mínimo privilegio: si se requiere un permiso Tauri adicional, limita su alcance y actualiza la capability correspondiente.

## Sidecars y seguridad

- Solo se permite ejecutar los sidecars previstos. No construyas comandos de shell a partir de texto de usuario.
- No edites `sidecars.lock.json` manualmente para cambiar versiones. Usa `npm run sidecars:lock:update -- ...` con versiones y SHA-256 publicados, revisa el diff y ejecuta las validaciones de lock y verificación.
- No desactives verificaciones SHA-256 ni sustituyas URLs inmutables por `latest`.
- Trata rutas de salida, nombres de archivos y metadatos remotos como datos no confiables; usa las utilidades de filesystem existentes y evita traversal/sobrescrituras accidentales.

## Comandos de desarrollo y validación

Ejecuta desde la raíz del repositorio:

```powershell
npm install
npm run sidecars:download
npm run sidecars:verify
npm run tauri dev
```

Antes de entregar cambios relevantes, ejecuta lo que aplique:

```powershell
npm run versions:check
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run sidecars:lock:check
```

Para validar empaquetado Windows cuando el cambio lo afecta:

```powershell
npm run tauri build
npm run package:portable
```

La descarga de sidecars usa red y puede requerir `GITHUB_TOKEN` por límites de GitHub. No incluyas binarios descargados ni artefactos de `dist/`, `target/` o `artifacts/` en cambios de código.

## Estilo y cambios

- Escribe código y mensajes de interfaz en el idioma consistente con el área modificada; la UI y la documentación actual están principalmente en español.
- Conserva las convenciones existentes de TypeScript, Rust y CSS; evita reformateos masivos no relacionados.
- Añade o adapta pruebas Rust junto al módulo afectado (`#[cfg(test)]`, tests de fixtures o de integración) cuando cambies lógica de dominio, persistencia, parsing o scheduler.
- Comprueba manualmente el flujo Tauri cuando alteres comandos, eventos, diálogos, progreso o cancelación.
- No modifiques cambios no relacionados ya presentes en el árbol de trabajo.

## Versiones, commits y CI

- Las versiones de `package.json`, `src-tauri/Cargo.toml` y `src-tauri/tauri.conf.json` deben coincidir. Actualízalas únicamente con `npm run version:set -- <semver>` y confirma con `npm run versions:check`.
- Usa Conventional Commits: `feat(scope): descripción`, `fix: descripción`, `docs: descripción`, etc.
- CI ejecuta el build de frontend en Ubuntu y `cargo check`/`cargo test` con sidecars verificados en Windows. Los cambios deben conservar ambos recorridos funcionales.
