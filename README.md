# YT Playlist Downloader

Aplicación local para Windows 10/11 x64 construida con Tauri 2, React,
TypeScript, Rust y SQLite. El desarrollo sigue `plan-desarrollo.md` por fases.

## Estado

Fase 1 implementada. El núcleo de descarga incluye:

- formulario de URL con validación de enlaces de YouTube;
- selección de carpeta de destino con diálogo nativo;
- política de colisiones configurable (renombrar / fallar / sobrescribir);
- pipeline completo: descarga con `yt-dlp`, conversión a MP3 192 kbps estéreo
  con `ffmpeg`, incrustación de carátula y escritura de metadatos ID3;
- progreso por etapas emitido al frontend (`download-progress`);
- cancelación de descargas asociando procesos a Windows Job Objects.

La Fase 0 (spike inicial) mantiene las capacidades heredadas:

- arranque de Tauri 2 y frontend React;
- SQLite embebido con SQLx, WAL y migraciones;
- ejecución restringida a `yt-dlp`, `ffmpeg` y `ffprobe`;
- descarga con verificación SHA-256 de sidecars;
- configuración NSIS y script de ZIP portable.

## Desarrollo

```powershell
npm install
npm run sidecars:download
npm run sidecars:verify
npm run tauri dev
```

## Validación

```powershell
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
npm run package:portable
```

Los binarios y artefactos generados no se versionan. No se usan cookies,
credenciales ni telemetría.
