use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct ProcessRegistry(Arc<Mutex<HashMap<String, CancellationToken>>>);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunToolRequest {
    pub run_id: String,
    pub tool: Tool,
    pub args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum Tool {
    #[serde(rename = "yt-dlp")]
    YtDlp,
    #[serde(rename = "ffmpeg")]
    Ffmpeg,
    #[serde(rename = "ffprobe")]
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolProgress<'a> {
    run_id: &'a str,
    stream: &'a str,
    line: &'a str,
}

fn tool_path(resource_dir: &Path, tool: Tool) -> Result<PathBuf, String> {
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

pub async fn run(
    app: AppHandle,
    registry: ProcessRegistry,
    request: RunToolRequest,
) -> Result<i32, String> {
    if request.args.iter().any(|arg| arg.contains('\0')) {
        return Err("Argumento de proceso inválido".into());
    }
    let path = tool_path(
        &app.path()
            .resource_dir()
            .map_err(|error| error.to_string())?,
        request.tool,
    )?;
    let token = CancellationToken::new();
    registry
        .0
        .lock()
        .await
        .insert(request.run_id.clone(), token.clone());

    let mut child = Command::new(path)
        .args(&request.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().ok_or("No se pudo capturar stdout")?;
    let stderr = child.stderr.take().ok_or("No se pudo capturar stderr")?;
    let stdout_task = emit_lines(app.clone(), request.run_id.clone(), "stdout", stdout);
    let stderr_task = emit_lines(app.clone(), request.run_id.clone(), "stderr", stderr);

    let result = tokio::select! {
        status = child.wait() => status.map_err(|error| error.to_string())?.code().unwrap_or(-1),
        _ = token.cancelled() => { child.kill().await.map_err(|error| error.to_string())?; -2 }
    };
    let _ = tokio::join!(stdout_task, stderr_task);
    registry.0.lock().await.remove(&request.run_id);
    Ok(result)
}

async fn emit_lines<R: tokio::io::AsyncRead + Unpin>(
    app: AppHandle,
    run_id: String,
    stream: &'static str,
    reader: R,
) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let bounded: String = line.chars().take(4_096).collect();
        let _ = app.emit(
            "tool-progress",
            ToolProgress {
                run_id: &run_id,
                stream,
                line: &bounded,
            },
        );
    }
}

pub async fn cancel(registry: &ProcessRegistry, run_id: &str) -> bool {
    if let Some(token) = registry.0.lock().await.get(run_id) {
        token.cancel();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Tool;
    #[test]
    fn tool_names_are_fixed() {
        assert_eq!(Tool::YtDlp.filename(), "yt-dlp.exe");
    }
}
