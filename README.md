# YT Playlist Downloader

Aplicación local para Windows 10/11 x64 construida con Tauri 2, React,
TypeScript, Rust y SQLite. El desarrollo sigue `plan-desarrollo.md` por fases.

## Estado

Fase 0 en validación externa. El spike implementa:

- arranque de Tauri 2 y frontend React;
- SQLite embebido con SQLx, WAL y migraciones;
- ejecución restringida a `yt-dlp`, `ffmpeg` y `ffprobe`;
- captura incremental de stdout/stderr y cancelación;
- descarga con verificación SHA-256 de sidecars;
- configuración NSIS y script de ZIP portable.

La compilación local, las pruebas Rust, la verificación de sidecars, el
instalador NSIS y el ZIP portable están validados. La fase no se considera
cerrada hasta:

- probar instalador y portable en un segundo equipo Windows x64 limpio;
- confirmar la licencia efectiva mediante `ffmpeg -L` e incorporar los textos
  íntegros exigidos por las versiones distribuidas.

No se inicia la Fase 1 hasta registrar esas dos validaciones.

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
