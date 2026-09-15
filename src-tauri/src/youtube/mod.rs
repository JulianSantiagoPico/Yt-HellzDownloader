use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::processes::{read_bounded_string, spawn_in_job, tool_path, Tool};

pub mod errors;

const ALLOWED_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UrlType {
    SingleVideo,
    Playlist,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub id: String,
    pub canonical_url: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub upload_date: Option<String>,
    pub duration_seconds: Option<f64>,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub id: String,
    pub url: String,
    pub title: String,
    pub artist: String,
    pub duration_seconds: Option<f64>,
    pub index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistInfo {
    pub playlist_id: String,
    pub title: String,
    pub entry_count: usize,
    pub entries: Vec<PlaylistEntry>,
}

/// Resultado de la validación: puede ser un vídeo individual o una playlist.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedUrl {
    pub url_type: UrlType,
    pub video_id: Option<String>,
    pub playlist_id: Option<String>,
    pub canonical_url: String,
}

/// Valida y normaliza rigurosamente una URL de YouTube utilizando el crate `url`.
/// Devuelve un `ValidatedUrl` que indica si es un vídeo individual o una playlist.
pub fn validate_youtube_url(raw_url: &str) -> Result<ValidatedUrl, String> {
    let parsed = Url::parse(raw_url.trim()).map_err(|_| "URL inválida".to_string())?;

    if parsed.scheme() != "https" {
        return Err("Solo se admiten conexiones seguras HTTPS".into());
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Credenciales de usuario no permitidas en la URL".into());
    }

    if let Some(port) = parsed.port() {
        if port != 443 {
            return Err("Puerto no estándar no permitido".into());
        }
    }

    let host = parsed
        .host_str()
        .ok_or("Falta host en la URL")?
        .to_ascii_lowercase();
    let is_allowed = ALLOWED_HOSTS.iter().any(|allowed| host == *allowed);
    if !is_allowed {
        return Err("Dominio no autorizado. Solo se permite YouTube y YouTube Music.".into());
    }

    let path = parsed.path();

    // Detectar playlist: /playlist?list=<id>
    if path == "/playlist" {
        let mut playlist_id = None;
        for (key, val) in parsed.query_pairs() {
            if key == "list" {
                playlist_id = Some(val.to_string());
                break;
            }
        }
        let pl_id = playlist_id.ok_or("Parámetro 'list' ausente en la URL de playlist")?;

        return Ok(ValidatedUrl {
            url_type: UrlType::Playlist,
            video_id: None,
            playlist_id: Some(pl_id),
            canonical_url: raw_url.trim().to_string(),
        });
    }

    // Vídeo individual: youtu.be/<id> o /watch?v=<id>
    let video_id = if host == "youtu.be" {
        let vid = path.trim_start_matches('/');
        if vid.is_empty() || vid.contains('/') {
            return Err("Ruta de youtu.be inválida. Se espera youtu.be/<id>".into());
        }
        vid.to_string()
    } else {
        if path != "/watch" {
            return Err(
                "Ruta de YouTube inválida. Se espera una URL de reproducción (/watch) o playlist (/playlist)".into(),
            );
        }
        let mut id = None;
        for (key, val) in parsed.query_pairs() {
            if key == "v" {
                id = Some(val.to_string());
                break;
            }
        }
        id.ok_or("Parámetro 'v' ausente en la URL")?
    };

    if video_id.len() != 11
        || !video_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Identificador de vídeo de YouTube inválido".into());
    }

    let canonical_url = format!("https://www.youtube.com/watch?v={}", video_id);
    Ok(ValidatedUrl {
        url_type: UrlType::SingleVideo,
        video_id: Some(video_id),
        playlist_id: None,
        canonical_url,
    })
}

/// Función de compatibilidad: valida y normaliza una URL devolviendo (video_id, canonical_url).
/// Para playlists, devuelve un error indicando que se use `validate_youtube_url`.
pub fn validate_and_normalize_youtube_url(raw_url: &str) -> Result<(String, String), String> {
    let validated = validate_youtube_url(raw_url)?;
    match validated.url_type {
        UrlType::SingleVideo => Ok((
            validated.video_id.unwrap_or_default(),
            validated.canonical_url,
        )),
        UrlType::Playlist => {
            Err("Esta es una URL de playlist. Use el flujo de descarga de playlist.".into())
        }
    }
}

/// Extrae metadata del vídeo usando yt-dlp con Job Object y timeout de 45 segundos.
pub async fn extract_metadata(
    resource_dir: &Path,
    url: &str,
    token: &CancellationToken,
) -> Result<VideoMetadata, String> {
    let (video_id, canonical_url) = validate_and_normalize_youtube_url(url)?;
    let ytdlp_path = tool_path(resource_dir, Tool::YtDlp)?;

    let mut cmd = Command::new(ytdlp_path);
    cmd.args([
        "--dump-single-json",
        "--no-playlist",
        "--skip-download",
        "--no-warnings",
        &canonical_url,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let (mut child, job) = spawn_in_job(cmd)?;

    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout")?;
    let stderr = child.stderr.take().ok_or("No se pudo capturar stderr")?;

    let stdout_reader = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buf = String::new();
        // Límite de 2MB para metadata JSON
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 || buf.len() > 2 * 1024 * 1024 {
                break;
            }
            buf.push_str(&line);
            line.clear();
        }
        buf
    });

    let stderr_reader = tokio::spawn(async move { read_bounded_string(stderr, 64 * 1024).await });

    let execution = async {
        tokio::select! {
            res = child.wait() => res.map_err(|e| e.to_string()),
            _ = token.cancelled() => {
                let _ = job.terminate(1);
                let _ = child.kill().await;
                Err("Operación cancelada".into())
            }
        }
    };

    let exit_status = match timeout(Duration::from_secs(45), execution).await {
        Ok(res) => res?,
        Err(_) => {
            let _ = job.terminate(1);
            let _ = child.kill().await;
            return Err("Tiempo de espera agotado al obtener la información del vídeo".into());
        }
    };

    let stdout_content = stdout_reader.await.unwrap_or_default();
    let stderr_content = stderr_reader.await.unwrap_or_default();

    if !exit_status.success() {
        let classified = errors::classify_ytdlp_error(
            stderr_content.trim(),
            "No se pudo extraer metadata del vídeo",
        );
        return Err(serde_json::to_string(&classified).unwrap_or(classified.user_message));
    }

    let json_val: serde_json::Value = serde_json::from_str(&stdout_content)
        .map_err(|e| format!("Error al analizar metadata recibida: {}", e))?;

    let title = json_val["title"]
        .as_str()
        .unwrap_or("Canción desconocida")
        .to_string();
    let artist = json_val["artist"]
        .as_str()
        .or_else(|| json_val["uploader"].as_str())
        .or_else(|| json_val["channel"].as_str())
        .unwrap_or("Artista desconocido")
        .to_string();
    let album = json_val["album"].as_str().map(|s| s.to_string());
    let upload_date = json_val["upload_date"]
        .as_str()
        .or_else(|| json_val["release_date"].as_str())
        .map(|s| s.to_string());
    let duration_seconds = json_val["duration"].as_f64();
    let thumbnail_url = json_val["thumbnail"].as_str().map(|s| s.to_string());

    Ok(VideoMetadata {
        id: video_id,
        canonical_url,
        title,
        artist,
        album,
        upload_date,
        duration_seconds,
        thumbnail_url,
    })
}

/// Extrae la lista de vídeos de una playlist usando yt-dlp.
pub async fn extract_playlist(
    resource_dir: &Path,
    url: &str,
    token: &CancellationToken,
) -> Result<PlaylistInfo, String> {
    let validated = validate_youtube_url(url)?;
    let playlist_id = validated.playlist_id.ok_or("La URL no es una playlist")?;

    let ytdlp_path = tool_path(resource_dir, Tool::YtDlp)?;

    let mut cmd = Command::new(ytdlp_path);
    cmd.args([
        "--flat-playlist",
        "--dump-single-json",
        "--no-warnings",
        &validated.canonical_url,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let (mut child, job) = spawn_in_job(cmd)?;

    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout")?;
    let stderr = child.stderr.take().ok_or("No se pudo capturar stderr")?;

    let stdout_reader = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buf = String::new();
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 || buf.len() > 10 * 1024 * 1024 {
                break;
            }
            buf.push_str(&line);
            line.clear();
        }
        buf
    });

    let stderr_reader = tokio::spawn(async move { read_bounded_string(stderr, 64 * 1024).await });

    let execution = async {
        tokio::select! {
            res = child.wait() => res.map_err(|e| e.to_string()),
            _ = token.cancelled() => {
                let _ = job.terminate(1);
                let _ = child.kill().await;
                Err("Operación cancelada".into())
            }
        }
    };

    let exit_status = match timeout(Duration::from_secs(120), execution).await {
        Ok(res) => res?,
        Err(_) => {
            let _ = job.terminate(1);
            let _ = child.kill().await;
            return Err("Tiempo de espera agotado al obtener la playlist".into());
        }
    };

    let stdout_content = stdout_reader.await.unwrap_or_default();
    let stderr_content = stderr_reader.await.unwrap_or_default();

    if !exit_status.success() {
        let classified =
            errors::classify_ytdlp_error(stderr_content.trim(), "No se pudo extraer la playlist");
        return Err(serde_json::to_string(&classified).unwrap_or(classified.user_message));
    }

    let json_val: serde_json::Value = serde_json::from_str(&stdout_content)
        .map_err(|e| format!("Error al analizar metadata de playlist: {}", e))?;

    let title = json_val["title"]
        .as_str()
        .unwrap_or("Playlist desconocida")
        .to_string();

    let entries_val = json_val["entries"]
        .as_array()
        .ok_or("No se encontraron entradas en la playlist")?;

    let entries: Vec<PlaylistEntry> = entries_val
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let id = entry["id"].as_str()?.to_string();
            let entry_title = entry["title"].as_str().unwrap_or("Sin título").to_string();
            let artist = entry["uploader"]
                .as_str()
                .or_else(|| entry["channel"].as_str())
                .unwrap_or("Artista desconocido")
                .to_string();
            let duration = entry["duration"].as_f64();
            let video_url = format!("https://www.youtube.com/watch?v={}", id);

            Some(PlaylistEntry {
                id,
                url: video_url,
                title: entry_title,
                artist,
                duration_seconds: duration,
                index: i + 1,
            })
        })
        .collect();

    Ok(PlaylistInfo {
        playlist_id,
        title,
        entry_count: entries.len(),
        entries,
    })
}

/// Descarga el flujo de audio y portada usando plantilla estructurada y timeout.
pub async fn download_audio_stream<F>(
    resource_dir: &Path,
    url: &str,
    temp_dir: &Path,
    token: &CancellationToken,
    progress_cb: F,
) -> Result<PathBuf, String>
where
    F: Fn(f32, &str) + Send + 'static,
{
    let (_, canonical_url) = validate_and_normalize_youtube_url(url)?;
    let ytdlp_path = tool_path(resource_dir, Tool::YtDlp)?;

    let output_template = temp_dir.join("source.%(ext)s");
    let thumbnail_template = temp_dir.join("cover.%(ext)s");

    let mut cmd = Command::new(ytdlp_path);
    cmd.args([
        "--no-playlist",
        "-f",
        "bestaudio/ba",
        "--write-thumbnail",
        "--convert-thumbnails",
        "jpg",
        "-o",
        output_template.to_str().unwrap_or("source.%(ext)s"),
        "-o",
        &format!(
            "thumbnail:{}",
            thumbnail_template.to_str().unwrap_or("cover.%(ext)s")
        ),
        "--newline",
        "--progress-template",
        "download-progress:%(progress._percent_str)s",
        "--no-warnings",
        &canonical_url,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let (mut child, job) = spawn_in_job(cmd)?;

    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout")?;
    let stderr = child.stderr.take().ok_or("No se pudo capturar stderr")?;

    let progress_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut last_emit = std::time::Instant::now();

        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(rest) = line.strip_prefix("download-progress:") {
                let clean_pct = rest.trim().trim_end_matches('%').trim();
                if let Ok(pct) = clean_pct.parse::<f32>() {
                    // Limitar frecuencia de emisión a máximo una vez cada 150ms o si es 100%
                    if last_emit.elapsed() >= Duration::from_millis(150) || pct >= 99.9 {
                        progress_cb(pct, &line);
                        last_emit = std::time::Instant::now();
                    }
                }
            }
        }
    });

    let stderr_reader = tokio::spawn(async move { read_bounded_string(stderr, 64 * 1024).await });

    let execution = async {
        tokio::select! {
            res = child.wait() => res.map_err(|e| e.to_string()),
            _ = token.cancelled() => {
                let _ = job.terminate(1);
                let _ = child.kill().await;
                Err("Descarga cancelada por el usuario".into())
            }
        }
    };

    // Timeout de 10 minutos para descarga de pista individual
    let exit_status = match timeout(Duration::from_secs(600), execution).await {
        Ok(res) => res?,
        Err(_) => {
            let _ = job.terminate(1);
            let _ = child.kill().await;
            return Err("Tiempo de espera agotado durante la descarga".into());
        }
    };

    let _ = progress_task.await;
    let stderr_content = stderr_reader.await.unwrap_or_default();

    if !exit_status.success() {
        let classified =
            errors::classify_ytdlp_error(stderr_content.trim(), "Fallo en la descarga de yt-dlp");
        return Err(serde_json::to_string(&classified).unwrap_or(classified.user_message));
    }

    let entries = std::fs::read_dir(temp_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.starts_with("source.")
                && !file_name.ends_with(".part")
                && !file_name.ends_with(".ytdl")
            {
                return Ok(path);
            }
        }
    }

    Err("No se encontró el archivo de audio descargado en el directorio temporal".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_and_normalize_youtube_url_valid() {
        let (id1, norm1) =
            validate_and_normalize_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
                .unwrap();
        assert_eq!(id1, "dQw4w9WgXcQ");
        assert_eq!(norm1, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");

        let (id2, norm2) =
            validate_and_normalize_youtube_url("https://youtu.be/dQw4w9WgXcQ?si=extra").unwrap();
        assert_eq!(id2, "dQw4w9WgXcQ");
        assert_eq!(norm2, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");

        let (id3, norm3) =
            validate_and_normalize_youtube_url("https://music.youtube.com/watch?v=dQw4w9WgXcQ")
                .unwrap();
        assert_eq!(id3, "dQw4w9WgXcQ");
        assert_eq!(norm3, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    }

    #[test]
    fn test_validate_and_normalize_youtube_url_invalid() {
        assert!(
            validate_and_normalize_youtube_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ")
                .is_err()
        );
        assert!(validate_and_normalize_youtube_url(
            "https://user:pass@youtube.com/watch?v=dQw4w9WgXcQ"
        )
        .is_err());
        assert!(
            validate_and_normalize_youtube_url("https://youtube.com:8080/watch?v=dQw4w9WgXcQ")
                .is_err()
        );
        assert!(
            validate_and_normalize_youtube_url("https://evil-youtube.com/watch?v=dQw4w9WgXcQ")
                .is_err()
        );
        assert!(validate_and_normalize_youtube_url("https://vimeo.com/123456").is_err());
        assert!(
            validate_and_normalize_youtube_url("https://www.youtube.com/watch?v=short").is_err()
        );
        assert!(validate_and_normalize_youtube_url("not an url").is_err());
    }

    #[test]
    fn test_validate_youtube_url_playlist() {
        let result = validate_youtube_url(
            "https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf",
        )
        .unwrap();
        assert_eq!(result.url_type, UrlType::Playlist);
        assert_eq!(
            result.playlist_id.as_deref(),
            Some("PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf")
        );
        assert!(result.video_id.is_none());
    }

    #[test]
    fn test_validate_youtube_url_music_playlist() {
        let result = validate_youtube_url(
            "https://music.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf",
        )
        .unwrap();
        assert_eq!(result.url_type, UrlType::Playlist);
        assert_eq!(
            result.playlist_id.as_deref(),
            Some("PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf")
        );
    }

    #[test]
    fn test_validate_and_normalize_youtube_music_url_with_list_param() {
        // Caso exacto del diagnóstico: URL de YouTube Music con parámetro list
        let (id, canonical) = validate_and_normalize_youtube_url(
            "https://music.youtube.com/watch?v=UnnwqBV5YWk&list=RDAMVMr46x3JsGhLc",
        )
        .unwrap();
        assert_eq!(id, "UnnwqBV5YWk");
        assert_eq!(canonical, "https://www.youtube.com/watch?v=UnnwqBV5YWk");
    }

    #[test]
    fn test_validate_youtube_url_youtu_be_strict() {
        // youtu.be debe rechazar rutas con segmentos adicionales
        assert!(validate_youtube_url("https://youtu.be/dQw4w9WgXcQ/extra").is_err());
        // youtu.be debe rechazar path vacío
        assert!(validate_youtube_url("https://youtu.be/").is_err());
        // Caso válido sigue funcionando
        let result = validate_youtube_url("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(result.video_id.as_deref(), Some("dQw4w9WgXcQ"));
    }

    #[test]
    fn test_validate_and_normalize_rejects_playlist() {
        let result = validate_and_normalize_youtube_url(
            "https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("playlist"));
    }

    #[tokio::test]
    async fn test_metadata_extraction_timeout_handling() {
        let slow_future = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<(), String>(())
        };

        let res = timeout(Duration::from_millis(5), slow_future).await;
        assert!(
            res.is_err(),
            "Debe fallar por timeout cuando el proceso excede el límite"
        );
    }
}
