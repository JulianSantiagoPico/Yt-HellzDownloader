# Contribuir

## Commits

Usamos [Conventional Commits](https://www.conventionalcommits.org/). El formato es:

```
tipo(ámbito opcional): descripción breve en imperativo
```

Ejemplos: `feat(download): añade cola de descargas`, `fix: evita cerrar procesos ajenos`, `docs: actualiza guía de instalación`. Los tipos habituales son `feat`, `fix`, `docs`, `refactor`, `test`, `build`, `ci` y `chore`.

Un cambio incompatible debe usar `!`, por ejemplo `feat!: cambia el formato de configuración`, o incluir `BREAKING CHANGE:` en el cuerpo. El hook local y los pull requests validan este formato.

## Versionado y publicación

Se aplica Semantic Versioning:

- `PATCH` (`0.1.1`): correcciones compatibles.
- `MINOR` (`0.2.0`): funcionalidades compatibles.
- `MAJOR` (`1.0.0`): cambios incompatibles.

La versión debe ser idéntica en `package.json`, `src-tauri/tauri.conf.json` y `src-tauri/Cargo.toml`. Nunca se edita cada archivo por separado:

```powershell
npm run version:set -- 0.2.0
npm run versions:check
```

Para publicar, fusiona los cambios aprobados en `main`, actualiza la versión y crea un commit de release. Tras subirlo, crea y sube el tag:

```powershell
git commit -am "chore(release): v0.2.0"
git tag -a v0.2.0 -m "v0.2.0"
git push origin main --follow-tags
```

El workflow `Release` valida la versión, descarga y verifica los sidecars, genera el instalador NSIS y el ZIP portable, y publica ambos en GitHub Releases.

## Sidecars reproducibles

`src-tauri/binaries/sidecars.lock.json` fija las versiones, URL inmutables y SHA-256 de `yt-dlp`, `ffmpeg` y `ffprobe`. CI y los releases solo consumen ese lock; no consultan versiones `latest`. Para actualizar un sidecar, usa el comando explícito con las versiones y hashes publicados, revisa el cambio y ejecuta:

```powershell
npm run sidecars:lock:update -- -YtDlpVersion 2026.08.19 -YtDlpSha256 <sha256> -FfmpegVersion 9.0.1 -FfmpegSha256 <sha256> -FfprobeSha256 <sha256>
```

```powershell
npm run sidecars:lock:check
npm run sidecars:download
npm run sidecars:verify
```
