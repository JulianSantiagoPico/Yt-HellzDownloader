use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandError {
    #[error("Validación: {message}")]
    Validation { message: String },

    #[error("No encontrado: {message}")]
    NotFound { message: String },

    #[error("Conflicto: {message}")]
    Conflict { message: String },

    #[error("Estado inválido: {message}")]
    InvalidState { message: String },

    #[error("Interno: {message}")]
    Internal { message: String },
}

impl From<sqlx::Error> for CommandError {
    fn from(e: sqlx::Error) -> Self {
        CommandError::Internal {
            message: e.to_string(),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for CommandError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        CommandError::Internal {
            message: e.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    pub kind: String,
    pub message: String,
    pub user_message: String,
}

impl From<&CommandError> for ErrorDto {
    fn from(err: &CommandError) -> Self {
        let kind = match err {
            CommandError::Validation { .. } => "validation",
            CommandError::NotFound { .. } => "notFound",
            CommandError::Conflict { .. } => "conflict",
            CommandError::InvalidState { .. } => "invalidState",
            CommandError::Internal { .. } => "internal",
        };

        let user_message = match err {
            CommandError::Validation { message } => message.clone(),
            CommandError::NotFound { message } => message.clone(),
            CommandError::Conflict { message } => message.clone(),
            CommandError::InvalidState { message } => message.clone(),
            CommandError::Internal { message } => {
                format!("Error interno del sistema: {}", message)
            }
        };

        ErrorDto {
            kind: kind.to_string(),
            message: err.to_string(),
            user_message,
        }
    }
}
