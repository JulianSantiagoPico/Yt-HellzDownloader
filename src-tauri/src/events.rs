use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AppEvent {
    JobStateChanged {
        job_id: String,
        previous_status: String,
        new_status: String,
    },
    ItemProgressChanged {
        job_id: String,
        item_id: String,
        status: String,
        progress: Option<f32>,
    },
    QueueChanged {
        reason: String,
    },
    ExtractionProgress {
        job_id: String,
        processed: u32,
        total: u32,
    },
}

impl AppEvent {
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::JobStateChanged { .. } => "job-state-changed",
            AppEvent::ItemProgressChanged { .. } => "item-progress-changed",
            AppEvent::QueueChanged { .. } => "queue-changed",
            AppEvent::ExtractionProgress { .. } => "extraction-progress",
        }
    }
}

/// Helper para emitir un evento Tauri con manejo de error simplificado.
pub fn emit_event(app: &AppHandle, event: &AppEvent) -> Result<(), String> {
    app.emit(event.name(), event.clone())
        .map_err(|e| format!("Error al emitir evento {}: {}", event.name(), e))
}
