use std::{fs, path::Path, process::Stdio, time::Duration};

use id3::{
    frame::{ExtendedText, Picture, PictureType},
    Tag, TagLike, Version,
};
use serde::{Deserialize, Serialize};
use tokio::{process::Command, time::timeout};
use tokio_util::sync::CancellationToken;

use crate::{
    processes::{spawn_in_job, tool_path, Tool},
    youtube::VideoMetadata,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub codec: String,
    pub channels: u32,
    pub duration_seconds: f64,
    pub bit_rate_kbps: Option<i64>,
}

/// Convierte el archivo de audio fuente a MP3 de 192 kbps estéreo constante (-ac 2) usando FFmpeg.
pub async fn convert_to_mp3(
    resource_dir: &Path,
    input_path: &Path,
    output_mp3: &Path,
    token: &CancellationToken,
) -> Result<(), String> {
    let ffmpeg_path = tool_path(resource_dir, Tool::Ffmpeg)?;

    let mut cmd = Command::new(ffmpeg_path);
    cmd.args([
        "-y",
        "-i",
        input_path.to_str().ok_or("Ruta de entrada inválida")?,
        "-vn",
        "-c:a",
        "libmp3lame",
        "-b:a",
        "192k",
        "-ac",
        "2",
        "-ar",
        "44100",
        output_mp3.to_str().ok_or("Ruta de salida inválida")?,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let (mut child, job) = spawn_in_job(cmd)?;

    let stderr = child
        .stderr
        .take()
        .ok_or("No se pudo capturar stderr de FFmpeg")?;
    let stderr_reader = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut err = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut err).await;
        err
    });

    let execution = async {
        tokio::select! {
            res = child.wait() => res.map_err(|e| e.to_string()),
            _ = token.cancelled() => {
                let _ = job.terminate(1);
                let _ = child.kill().await;
                Err("Conversión cancelada por el usuario".into())
            }
        }
    };

    let exit_status = match timeout(Duration::from_secs(300), execution).await {
        Ok(res) => res?,
        Err(_) => {
            let _ = job.terminate(1);
            let _ = child.kill().await;
            return Err("Tiempo de espera agotado durante la conversión a MP3".into());
        }
    };

    let stderr_output = stderr_reader.await.unwrap_or_default();
    if !exit_status.success() {
        return Err(format!(
            "FFmpeg falló al convertir a MP3: {}",
            stderr_output.trim()
        ));
    }

    if !output_mp3.exists() {
        return Err("El archivo MP3 resultante no fue generado".into());
    }

    Ok(())
}

/// Escribe etiquetas ID3v2.3 y verifica inmediatamente que los frames críticos son legibles.
pub fn tag_mp3(
    mp3_path: &Path,
    metadata: &VideoMetadata,
    cover_path: Option<&Path>,
    album_name: Option<&str>,
) -> Result<(), String> {
    let mut tag = Tag::read_from_path(mp3_path).unwrap_or_default();

    tag.set_title(&metadata.title);
    tag.set_artist(&metadata.artist);
    tag.set_album(album_name.unwrap_or(if metadata.artist.is_empty() {
        "YouTube"
    } else {
        &metadata.artist
    }));

    // Frames TXXX para identificación y trazabilidad inequívoca
    tag.add_frame(ExtendedText {
        description: "YOUTUBE_VIDEO_ID".to_string(),
        value: metadata.id.clone(),
    });
    tag.add_frame(ExtendedText {
        description: "YOUTUBE_URL".to_string(),
        value: metadata.canonical_url.clone(),
    });

    // Fecha si está disponible
    if let Some(date_str) = &metadata.upload_date {
        if date_str.len() >= 4 {
            if let Ok(year) = date_str[..4].parse::<i32>() {
                tag.set_year(year);
            }
        }
    }

    // Portada si existe
    if let Some(path) = cover_path {
        if path.is_file() {
            if let Ok(data) = fs::read(path) {
                let mime_type = if path.extension().and_then(|e| e.to_str()) == Some("png") {
                    "image/png".to_string()
                } else {
                    "image/jpeg".to_string()
                };

                tag.add_frame(Picture {
                    mime_type,
                    picture_type: PictureType::CoverFront,
                    description: "Portada".to_string(),
                    data,
                });
            }
        }
    }

    // Guardar con versión ID3v2.3
    tag.write_to_path(mp3_path, Version::Id3v23)
        .map_err(|e| format!("Error al guardar etiquetas ID3: {}", e))?;

    // Verificación posterior inmediata: comprobar que los metadatos críticos se leen correctamente
    let read_back = Tag::read_from_path(mp3_path)
        .map_err(|e| format!("Error de verificación al releer etiquetas ID3: {}", e))?;

    if read_back.title().is_none() || read_back.artist().is_none() {
        return Err(
            "Verificación fallida: no se pudieron releer el título o artista en el archivo MP3"
                .into(),
        );
    }

    let has_yt_id = read_back
        .extended_texts()
        .any(|et| et.description == "YOUTUBE_VIDEO_ID" && et.value == metadata.id);

    if !has_yt_id {
        return Err(
            "Verificación fallida: el frame YOUTUBE_VIDEO_ID no se escribió correctamente".into(),
        );
    }

    Ok(())
}

/// Valida el MP3 final con ffprobe para verificar códec, estéreo (2 canales), duración y bitrate.
pub async fn validate_mp3(
    resource_dir: &Path,
    mp3_path: &Path,
    token: &CancellationToken,
) -> Result<ValidationResult, String> {
    let ffprobe_path = tool_path(resource_dir, Tool::Ffprobe)?;

    let mut cmd = Command::new(ffprobe_path);
    cmd.args([
        "-v",
        "error",
        "-select_streams",
        "a:0",
        "-show_entries",
        "stream=codec_name,channels,bit_rate,duration",
        "-show_entries",
        "format=format_name,duration,bit_rate",
        "-of",
        "json",
        mp3_path.to_str().ok_or("Ruta de MP3 inválida")?,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

    let (mut child, job) = spawn_in_job(cmd)?;

    let stdout = child
        .stdout
        .take()
        .ok_or("No se pudo capturar stdout de ffprobe")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("No se pudo capturar stderr de ffprobe")?;

    let stdout_reader = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut out = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut out).await;
        out
    });

    let stderr_reader = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut err = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut err).await;
        err
    });

    let execution = async {
        tokio::select! {
            res = child.wait() => res.map_err(|e| e.to_string()),
            _ = token.cancelled() => {
                let _ = job.terminate(1);
                let _ = child.kill().await;
                Err("Validación cancelada por el usuario".into())
            }
        }
    };

    let exit_status = match timeout(Duration::from_secs(30), execution).await {
        Ok(res) => res?,
        Err(_) => {
            let _ = job.terminate(1);
            let _ = child.kill().await;
            return Err("Tiempo de espera agotado al validar con ffprobe".into());
        }
    };

    let stdout_content = stdout_reader.await.unwrap_or_default();
    let stderr_content = stderr_reader.await.unwrap_or_default();

    if !exit_status.success() {
        return Err(format!(
            "ffprobe rechazó el archivo MP3: {}",
            stderr_content.trim()
        ));
    }

    let parsed: serde_json::Value = serde_json::from_str(&stdout_content)
        .map_err(|e| format!("Salida no legible de ffprobe: {}", e))?;

    let stream = parsed["streams"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or("ffprobe no detectó ningún flujo de audio en el archivo")?;

    let codec = stream["codec_name"].as_str().unwrap_or("").to_string();
    if codec != "mp3" {
        return Err(format!(
            "El códec detectado no es MP3 (se obtuvo: {})",
            codec
        ));
    }

    let channels = stream["channels"].as_u64().unwrap_or(0) as u32;
    if channels != 2 {
        return Err(format!(
            "El perfil exige audio estéreo (2 canales). Se detectaron {} canales.",
            channels
        ));
    }

    let duration_seconds = stream["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            parsed["format"]["duration"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    if duration_seconds <= 0.0 {
        return Err("El archivo MP3 tiene una duración inválida o nula".into());
    }

    let bit_rate_kbps = stream["bit_rate"]
        .as_str()
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| {
            parsed["format"]["bit_rate"]
                .as_str()
                .and_then(|s| s.parse::<i64>().ok())
        })
        .map(|bps| bps / 1000);

    Ok(ValidationResult {
        valid: true,
        codec,
        channels,
        duration_seconds,
        bit_rate_kbps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audio_pipeline_convert_tag_validate() {
        let bin_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let ffmpeg = bin_dir.join("ffmpeg.exe");
        let ffprobe = bin_dir.join("ffprobe.exe");

        assert!(
            ffmpeg.exists(),
            "Falta ffmpeg.exe en binaries para ejecutar las pruebas"
        );
        assert!(
            ffprobe.exists(),
            "Falta ffprobe.exe en binaries para ejecutar las pruebas"
        );

        let temp_dir =
            std::env::temp_dir().join(format!("yt_test_audio_pipeline_{}", uuid::Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);

        let synth_source = temp_dir.join("source.wav");
        let output_mp3 = temp_dir.join("test_output.mp3");

        // Generar 1 segundo de audio sintético estéreo con FFmpeg
        let gen_status = std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
                synth_source.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to generate test audio");
        assert!(gen_status.status.success());

        // 1. Convertir a MP3 a 192 kbps estéreo
        let token = CancellationToken::new();
        let convert_res = convert_to_mp3(
            temp_dir.parent().unwrap(),
            &synth_source,
            &output_mp3,
            &token,
        )
        .await;
        assert!(
            convert_res.is_ok(),
            "Falló la conversión a MP3: {:?}",
            convert_res
        );

        // 2. Etiquetar con ID3v2.3
        let metadata = VideoMetadata {
            id: "dQw4w9WgXcQ".to_string(),
            canonical_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            title: "Never Gonna Give You Up".to_string(),
            artist: "Rick Astley".to_string(),
            album: Some("Whenever You Need Somebody".to_string()),
            upload_date: Some("19871112".to_string()),
            duration_seconds: Some(1.0),
            thumbnail_url: None,
        };
        let tag_res = tag_mp3(&output_mp3, &metadata, None, None);
        assert!(tag_res.is_ok(), "Falló el etiquetado ID3: {:?}", tag_res);

        // 3. Verificar que las etiquetas ID3 se escribieron correctamente
        let read_tag = Tag::read_from_path(&output_mp3).expect("Failed to read back ID3 tag");
        assert_eq!(read_tag.title(), Some("Never Gonna Give You Up"));
        assert_eq!(read_tag.artist(), Some("Rick Astley"));
        assert_eq!(read_tag.year(), Some(1987));

        let ytid_frame = read_tag
            .extended_texts()
            .find(|et| et.description == "YOUTUBE_VIDEO_ID");
        assert!(ytid_frame.is_some());
        assert_eq!(ytid_frame.unwrap().value, "dQw4w9WgXcQ");

        // 4. Validar integridad y perfil con ffprobe (2 canales estéreo requeridos)
        let val_res = validate_mp3(temp_dir.parent().unwrap(), &output_mp3, &token).await;
        assert!(val_res.is_ok(), "ffprobe no validó el MP3: {:?}", val_res);
        let val = val_res.unwrap();
        assert_eq!(val.codec, "mp3");
        assert_eq!(val.channels, 2);
        assert!(val.duration_seconds > 0.5 && val.duration_seconds < 1.5);

        // Limpieza
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_convert_to_mp3_cancellation() {
        let bin_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let ffmpeg = bin_dir.join("ffmpeg.exe");
        if !ffmpeg.exists() {
            return;
        }

        let temp_dir =
            std::env::temp_dir().join(format!("yt_cancel_test_{}", uuid::Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);

        let synth_source = temp_dir.join("source_long.wav");
        let output_mp3 = temp_dir.join("test_output.mp3");

        // Generar 10 segundos de audio para tener tiempo suficiente de cancelar
        let gen_status = std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "10",
                synth_source.to_str().unwrap(),
            ])
            .output()
            .expect("Failed to generate test audio");
        assert!(gen_status.status.success());

        let token = CancellationToken::new();
        let token_clone = token.clone();

        // Cancelar inmediatamente después de lanzar
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token_clone.cancel();
        });

        let convert_res = convert_to_mp3(
            temp_dir.parent().unwrap(),
            &synth_source,
            &output_mp3,
            &token,
        )
        .await;
        assert!(convert_res.is_err());
        let err_msg = convert_res.unwrap_err();
        assert!(err_msg.contains("cancelada") || err_msg.contains("Cancelada"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
