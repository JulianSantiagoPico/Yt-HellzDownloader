use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifiedError {
    pub user_message: String,
    pub category: ErrorCategory,
    pub raw_stderr: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    AntiBotBlock,
    PrivateVideo,
    DeletedVideo,
    RegionalRestriction,
    NetworkTimeout,
    Generic,
}

impl ErrorCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AntiBotBlock => "Bloqueo anti-automatización",
            Self::PrivateVideo => "Vídeo privado",
            Self::DeletedVideo => "Vídeo eliminado",
            Self::RegionalRestriction => "Restricción regional",
            Self::NetworkTimeout => "Error de red",
            Self::Generic => "Error de yt-dlp",
        }
    }
}

pub fn classify_ytdlp_error(stderr: &str, prefix: &str) -> ClassifiedError {
    let lower = stderr.to_ascii_lowercase();

    let (category, user_message) = if lower.contains("sign in to confirm you're not a bot")
        || lower.contains("confirm you're not a bot")
        || lower.contains("youtube is being monitored")
        || lower.contains("automated access")
        || lower.contains("bot")
    {
        (
            ErrorCategory::AntiBotBlock,
            "YouTube bloqueó temporalmente la extracción automatizada para este vídeo.\n\n\
             El enlace es válido. Opciones:\n\
             · Inténtalo de nuevo más tarde.\n\
             · Si hay una actualización de yt-dlp disponible, actualízala desde la aplicación."
                .to_string(),
        )
    } else if lower.contains("private video") || lower.contains("video is private") {
        (
            ErrorCategory::PrivateVideo,
            "Este vídeo es privado y no se puede acceder a él.".to_string(),
        )
    } else if lower.contains("video unavailable")
        || lower.contains("has been removed")
        || lower.contains("video not available")
    {
        (
            ErrorCategory::DeletedVideo,
            "Este vídeo ha sido eliminado o no está disponible.".to_string(),
        )
    } else if lower.contains("not available in your country")
        || lower.contains("region")
        || lower.contains("geo")
    {
        (
            ErrorCategory::RegionalRestriction,
            "Este vídeo no está disponible en tu región.".to_string(),
        )
    } else if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection refused")
        || lower.contains("network")
    {
        (
            ErrorCategory::NetworkTimeout,
            "Error de conexión o tiempo de espera agotado. Verifica tu conexión a internet."
                .to_string(),
        )
    } else {
        (
            ErrorCategory::Generic,
            format!("{} Ver los detalles a continuación.", prefix),
        )
    };

    ClassifiedError {
        user_message,
        category,
        raw_stderr: stderr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_anti_bot_block() {
        let err = classify_ytdlp_error(
            "ERROR: [youtube] UnnwqBV5YWk: Sign in to confirm you're not a bot.",
            "Error de yt-dlp",
        );
        assert_eq!(err.category, ErrorCategory::AntiBotBlock);
        assert!(err.user_message.contains("bloqueó"));
        assert!(err.raw_stderr.contains("Sign in"));
    }

    #[test]
    fn classify_private_video() {
        let err = classify_ytdlp_error(
            "ERROR: [youtube] abc12345678: Private video",
            "Error de yt-dlp",
        );
        assert_eq!(err.category, ErrorCategory::PrivateVideo);
        assert!(err.user_message.contains("privado"));
    }

    #[test]
    fn classify_deleted_video() {
        let err = classify_ytdlp_error(
            "ERROR: [youtube] abc12345678: Video unavailable",
            "Error de yt-dlp",
        );
        assert_eq!(err.category, ErrorCategory::DeletedVideo);
        assert!(err.user_message.contains("eliminado"));
    }

    #[test]
    fn classify_regional_restriction() {
        let err = classify_ytdlp_error(
            "ERROR: [youtube] abc12345678: not available in your country",
            "Error de yt-dlp",
        );
        assert_eq!(err.category, ErrorCategory::RegionalRestriction);
        assert!(err.user_message.contains("región"));
    }

    #[test]
    fn classify_network_timeout() {
        let err = classify_ytdlp_error(
            "ERROR: [youtube] abc12345678: Connection timed out",
            "Error de yt-dlp",
        );
        assert_eq!(err.category, ErrorCategory::NetworkTimeout);
        assert!(err.user_message.contains("conexión"));
    }

    #[test]
    fn classify_generic_fallback() {
        let err = classify_ytdlp_error(
            "ERROR: Some unknown error occurred",
            "No se pudo extraer metadata",
        );
        assert_eq!(err.category, ErrorCategory::Generic);
        assert!(err.user_message.contains("No se pudo extraer metadata"));
    }
}
