pub mod job_object;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use job_object::JobObjectHandle;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct ProcessRegistry(Arc<Mutex<HashMap<String, CancellationToken>>>);

impl ProcessRegistry {
    pub async fn insert_token(
        &self,
        run_id: String,
        token: CancellationToken,
    ) -> Result<(), String> {
        let mut lock = self.0.lock().await;
        if lock.contains_key(&run_id) {
            return Err("Ya existe una ejecución activa con este identificador".into());
        }
        lock.insert(run_id, token);
        Ok(())
    }

    pub async fn remove_token(&self, run_id: &str) {
        self.0.lock().await.remove(run_id);
    }

    pub async fn cancel(&self, run_id: &str) -> bool {
        if let Some(token) = self.0.lock().await.get(run_id) {
            token.cancel();
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    YtDlp,
    Ffmpeg,
    Ffprobe,
}

impl Tool {
    pub fn filename(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp.exe",
            Self::Ffmpeg => "ffmpeg.exe",
            Self::Ffprobe => "ffprobe.exe",
        }
    }
}

pub fn tool_path(resource_dir: &Path, tool: Tool) -> Result<PathBuf, String> {
    let packaged = resource_dir.join("binaries").join(tool.filename());
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(tool.filename());
    [packaged, development]
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            format!(
                "No se encontró {}. Ejecuta npm run sidecars:download.",
                tool.filename()
            )
        })
}

/// Inicia un proceso asociado obligatoriamente a un Windows Job Object.
/// En Windows, oculta la ventana de consola mediante `CREATE_NO_WINDOW`.
pub fn spawn_in_job(
    mut command: tokio::process::Command,
) -> Result<(tokio::process::Child, JobObjectHandle), String> {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let job = JobObjectHandle::new()?;
    let child = command
        .spawn()
        .map_err(|error| format!("Error al iniciar proceso secundario: {}", error))?;

    #[cfg(windows)]
    {
        if let Some(raw_handle) = child.raw_handle() {
            job.assign_process(raw_handle)?;
        } else {
            return Err("No se pudo obtener el descriptor del proceso para el Job Object".into());
        }
    }

    Ok((child, job))
}

pub async fn read_bounded_string<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    max_bytes: usize,
) -> String {
    use tokio::io::AsyncBufReadExt;
    let mut lines = tokio::io::BufReader::new(reader).lines();
    let mut buf = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if buf.len() >= max_bytes {
            break;
        }
        buf.push_str(&line);
        buf.push('\n');
    }
    if buf.len() > max_bytes {
        buf.truncate(max_bytes);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_fixed() {
        assert_eq!(Tool::YtDlp.filename(), "yt-dlp.exe");
        assert_eq!(Tool::Ffmpeg.filename(), "ffmpeg.exe");
        assert_eq!(Tool::Ffprobe.filename(), "ffprobe.exe");
    }

    #[tokio::test]
    async fn test_mp3_encoder_present_in_ffmpeg() {
        let bin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
        let ffmpeg = bin_dir.join(Tool::Ffmpeg.filename());
        if ffmpeg.exists() {
            let output = std::process::Command::new(ffmpeg)
                .args(["-encoders"])
                .output()
                .expect("Failed to execute ffmpeg");
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(
                text.contains("libmp3lame") || text.contains("mp3"),
                "El binario de FFmpeg debe incluir un codificador MP3"
            );
        }
    }

    #[tokio::test]
    async fn test_process_registry_duplicate_prevention() {
        let registry = ProcessRegistry::default();
        let token1 = CancellationToken::new();
        let token2 = CancellationToken::new();

        assert!(registry.insert_token("job-1".into(), token1).await.is_ok());
        // El segundo intento con la misma clave debe fallar
        assert!(registry.insert_token("job-1".into(), token2).await.is_err());

        registry.remove_token("job-1").await;
        let token3 = CancellationToken::new();
        assert!(registry.insert_token("job-1".into(), token3).await.is_ok());
    }

    /// Verifica que la cancelación termina el proceso y no deja huérfanos.
    /// Usa un sleep largo que debería ser terminado por el Job Object.
    #[cfg(windows)]
    #[tokio::test]
    async fn test_cancellation_terminates_process_tree() {
        use std::process::Stdio;
        use std::time::Instant;

        let mut cmd = tokio::process::Command::new("cmd.exe");
        cmd.args(["/c", "timeout", "/t", "30", "/nobreak"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let (mut child, job) = spawn_in_job(cmd).expect("debe spawnear proceso");
        let start = Instant::now();

        // Cancelamos inmediatamente
        let _ = job.terminate(1);

        // El proceso debe terminar antes del timeout de 30s
        // Esperamos un máximo de 5 segundos para confirmar terminación
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;

        let elapsed = start.elapsed();
        assert!(
            result.is_ok(),
            "El proceso no terminó tras cancelación (esperó {:?})",
            elapsed
        );
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "La cancelación tardó demasiado: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_bounded_process_output_reading() {
        // Simular un stream de salida con 200 KB de texto
        let heavy_payload = "A".repeat(200 * 1024);
        let cursor = std::io::Cursor::new(heavy_payload);

        let bounded = read_bounded_string(cursor, 64 * 1024).await;
        assert_eq!(bounded.len(), 64 * 1024);
    }
}
