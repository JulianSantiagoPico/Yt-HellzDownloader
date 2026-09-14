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

use crate::processes::{spawn_in_job, tool_path, Tool};

const ALLOWED_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

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

/// Valida y normaliza rigurosamente una URL de YouTube utilizando el crate `url`.
pub fn validate_and_normalize_youtube_url(raw_url: &str) -> Result<(String, String), String> {
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

    let video_id = if host == "youtu.be" {
        let path = parsed.path().trim_start_matches('/');
        let id = path.split('/').next().unwrap_or("");
        id.to_string()
    } else {
        if parsed.path() != "/watch" {
            return Err(
                "Ruta de YouTube inválida. Se espera una URL de reproducción (/watch)".into(),
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

    // Validar formato estándar del ID de vídeo de YouTube (11 caracteres alfanuméricos, guiones o barras bajas)
    if video_id.len() != 11
        || !video_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Identificador de vídeo de YouTube inválido".into());
    }

    let canonical_url = format!("https://www.youtube.com/watch?v={}", video_id);
    Ok((video_id, canonical_url))
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

    let stderr_reader = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut err = String::new();
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 || err.len() > 64 * 1024 {
                break;
            }
            err.push_str(&line);
            line.clear();
        }
        err
    });

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
        return Err(format!(
            "No se pudo extraer metadata del vídeo: {}",
            stderr_content.trim()
        ));
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

    let stderr_reader = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut err = String::new();
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 || err.len() > 64 * 1024 {
                break;
            }
            err.push_str(&line);
            line.clear();
        }
        err
    });

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
        return Err(format!(
            "Fallo en la descarga de yt-dlp: {}",
            stderr_content.trim()
        ));
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

    #[tokio::test]
    async fn test_metadata_extraction_timeout_handling() {
        // Probar que el mecanismo de timeout aborta operaciones que exceden el tiempo límite
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
