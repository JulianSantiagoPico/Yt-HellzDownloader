use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExistingFilePolicy {
    #[default]
    Ask,
    Reuse,
    Overwrite,
    Rename,
    #[serde(alias = "fail_if_exists")]
    FailIfExists,
}

impl ExistingFilePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Reuse => "reuse",
            Self::Overwrite => "overwrite",
            Self::Rename => "rename",
            Self::FailIfExists => "fail_if_exists",
        }
    }
}

/// Sanitiza un nombre de archivo para Windows, considerando caracteres prohibidos,
/// nombres reservados de dispositivo (incluso con extensiones) y longitud máxima disponible.
pub fn sanitize_filename(name: &str, max_len: usize) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => {
                sanitized.push('_');
            }
            c if (c as u32) < 32 => {
                sanitized.push('_');
            }
            c => sanitized.push(c),
        }
    }

    let mut trimmed = sanitized.trim().trim_end_matches('.').trim().to_string();

    if trimmed.is_empty() {
        trimmed = "unnamed".to_string();
    }

    // Comprobar nombres de dispositivo reservados (incluso con extensiones como CON.mp3 o AUX.txt)
    if let Some((stem, ext)) = trimmed.split_once('.') {
        let uppercase_stem = stem.trim().to_ascii_uppercase();
        for reserved in RESERVED_NAMES {
            if uppercase_stem == *reserved {
                trimmed = format!("{}_file.{}", stem, ext);
                break;
            }
        }
    } else {
        let uppercase_stem = trimmed.trim().to_ascii_uppercase();
        for reserved in RESERVED_NAMES {
            if uppercase_stem == *reserved {
                trimmed = format!("{}_file", trimmed);
                break;
            }
        }
    }

    // Limitar longitud máxima dinámicamente
    let effective_max = max_len.clamp(30, 220);
    if trimmed.chars().count() > effective_max {
        trimmed = trimmed.chars().take(effective_max).collect();
        trimmed = trimmed.trim_end().trim_end_matches('.').to_string();
    }

    trimmed
}

/// Formatea el nombre de archivo para una pista según su posición o título individual.
pub fn format_track_filename(position: Option<u32>, title: &str, max_name_len: usize) -> String {
    let clean_title = sanitize_filename(title, max_name_len.saturating_sub(15));
    match position {
        Some(pos) if pos >= 1000 => format!("{:04} - {}.mp3", pos, clean_title),
        Some(pos) => format!("{:03} - {}.mp3", pos, clean_title),
        None => format!("{}.mp3", clean_title),
    }
}

/// Resuelve la ruta definitiva del archivo aplicando la política de colisión.
pub fn resolve_destination_path(
    destination_dir: &Path,
    title: &str,
    position: Option<u32>,
    policy: ExistingFilePolicy,
) -> Result<PathBuf, String> {
    let dest_dir_len = destination_dir.to_string_lossy().chars().count();
    let max_name_len = (240_usize).saturating_sub(dest_dir_len).clamp(30, 200);

    let base_filename = format_track_filename(position, title, max_name_len);
    let target_path = destination_dir.join(&base_filename);

    if !target_path.exists() {
        return Ok(target_path);
    }

    match policy {
        ExistingFilePolicy::FailIfExists | ExistingFilePolicy::Ask => Err(format!(
            "El archivo '{}' ya existe en el destino y la política impide sobrescribirlo.",
            base_filename
        )),
        ExistingFilePolicy::Reuse | ExistingFilePolicy::Overwrite => Ok(target_path),
        ExistingFilePolicy::Rename => {
            let stem = target_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("track");
            let ext = target_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp3");

            for idx in 1..=9999 {
                let candidate_name = format!("{} ({}).{}", stem, idx, ext);
                let candidate_path = destination_dir.join(&candidate_name);
                if !candidate_path.exists() {
                    return Ok(candidate_path);
                }
            }
            Err("No se pudo encontrar un nombre disponible tras 9999 intentos".into())
        }
    }
}

/// Prepara un directorio temporal aislado para el ítem, intentando ubicarlo en el mismo volumen de destino.
pub fn prepare_item_temp_dir(
    destination_dir: &Path,
    fallback_temp_base: &Path,
    item_id: &Uuid,
) -> Result<(PathBuf, bool), String> {
    let dest_temp_base = destination_dir.join(".yt-downloader-temp");
    let dest_item_dir = dest_temp_base.join(item_id.to_string());

    // Intentar crear temporal en la carpeta destino para permitir renombrado atómico en el mismo volumen
    if fs::create_dir_all(&dest_item_dir).is_ok() {
        // Prueba de escritura
        let probe = dest_item_dir.join(".probe");
        if fs::write(&probe, b"1").is_ok() {
            let _ = fs::remove_file(&probe);
            return Ok((dest_item_dir, true));
        }
        let _ = fs::remove_dir_all(&dest_item_dir);
    }

    // Fallback: usar el directorio temporal administrado de la aplicación
    let fallback_item_dir = fallback_temp_base.join(item_id.to_string());
    fs::create_dir_all(&fallback_item_dir)
        .map_err(|e| format!("No se pudo crear directorio temporal de fallback: {}", e))?;

    Ok((fallback_item_dir, false))
}

/// Limpia el directorio temporal de un ítem garantizando que es seguro eliminarlo.
pub fn clean_item_temp_dir(item_dir: &Path) -> Result<(), std::io::Error> {
    if item_dir.exists() {
        fs::remove_dir_all(item_dir)?;
    }
    // Si el padre es .yt-downloader-temp y quedó vacío, eliminarlo
    if let Some(parent) = item_dir.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some(".yt-downloader-temp") {
            let is_empty = fs::read_dir(parent)?.next().is_none();
            if is_empty {
                let _ = fs::remove_dir(parent);
            }
        }
    }
    Ok(())
}

/// Mueve un archivo validado a su destino final de forma atómica o segura con verificación y reemplazo.
pub fn move_file_safely(src: &Path, dst: &Path, allow_overwrite: bool) -> Result<(), String> {
    if !src.exists() {
        return Err(format!("Archivo fuente no existe: {}", src.display()));
    }

    if dst.exists() && !allow_overwrite {
        return Err(format!(
            "El archivo destino '{}' ya existe y no se autorizó sobrescritura.",
            dst.display()
        ));
    }

    if let Some(parent) = dst.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // 1. Intentar renombrado atómico (funciona en el mismo volumen NTFS)
    if !dst.exists() && fs::rename(src, dst).is_ok() {
        return Ok(());
    }

    // 2. Si dst existe y allow_overwrite está habilitado, o si están en volúmenes distintos:
    let temp_dst = dst.with_extension(format!("tmp.{}", Uuid::new_v4()));
    if let Err(e) = fs::copy(src, &temp_dst) {
        let _ = fs::remove_file(&temp_dst);
        return Err(format!("Fallo al copiar archivo al destino: {}", e));
    }

    // Validar tamaño idéntico antes de cualquier sustitución
    let src_len = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    let temp_len = fs::metadata(&temp_dst).map(|m| m.len()).unwrap_or(1);
    if src_len != temp_len || src_len == 0 {
        let _ = fs::remove_file(&temp_dst);
        return Err("Fallo de integridad: tamaño de archivo copiado no coincide".into());
    }

    // Sustituir de forma segura
    if dst.exists() {
        if !allow_overwrite {
            let _ = fs::remove_file(&temp_dst);
            return Err("Sobrescritura no autorizada en destino".into());
        }
        if let Err(e) = fs::remove_file(dst) {
            let _ = fs::remove_file(&temp_dst);
            return Err(format!("No se pudo reemplazar archivo existente: {}", e));
        }
    }

    if let Err(e) = fs::rename(&temp_dst, dst) {
        let _ = fs::remove_file(&temp_dst);
        return Err(format!("No se pudo finalizar renombrado de archivo: {}", e));
    }

    let _ = fs::remove_file(src);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitization_rules_and_reserved_names() {
        assert_eq!(sanitize_filename("Normal Song", 100), "Normal Song");
        assert_eq!(
            sanitize_filename("Song: Part 1? *Special*", 100),
            "Song_ Part 1_ _Special_"
        );
        assert_eq!(sanitize_filename("Track...   ", 100), "Track");
        assert_eq!(sanitize_filename("CON", 100), "CON_file");
        assert_eq!(sanitize_filename("aux.mp3", 100), "aux_file.mp3");
        assert_eq!(sanitize_filename("NUL.txt", 100), "NUL_file.txt");
        assert_eq!(
            sanitize_filename("日本語の曲 - 100% ✨", 100),
            "日本語の曲 - 100% ✨"
        );
        assert_eq!(sanitize_filename("   ", 100), "unnamed");
    }

    #[test]
    fn test_resolve_destination_path_rename_policy() {
        let temp_dir = std::env::temp_dir().join(format!("yt_dest_test_{}", Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);

        let initial = temp_dir.join("Canción.mp3");
        fs::write(&initial, b"initial").unwrap();

        // Con política Rename, debe generar 'Canción (1).mp3'
        let candidate1 =
            resolve_destination_path(&temp_dir, "Canción", None, ExistingFilePolicy::Rename)
                .unwrap();
        assert_eq!(candidate1.file_name().unwrap(), "Canción (1).mp3");

        // Creamos 'Canción (1).mp3' para forzar 'Canción (2).mp3'
        fs::write(&candidate1, b"c1").unwrap();
        let candidate2 =
            resolve_destination_path(&temp_dir, "Canción", None, ExistingFilePolicy::Rename)
                .unwrap();
        assert_eq!(candidate2.file_name().unwrap(), "Canción (2).mp3");

        // Con política FailIfExists, debe fallar
        let fail_res =
            resolve_destination_path(&temp_dir, "Canción", None, ExistingFilePolicy::FailIfExists);
        assert!(fail_res.is_err());

        // Comprobar que el archivo original no fue alterado
        let original_content = fs::read(&initial).unwrap();
        assert_eq!(original_content, b"initial");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_move_file_safely_no_overwrite_without_permission() {
        let temp_dir = std::env::temp_dir().join(format!("yt_move_test_{}", Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);

        let src = temp_dir.join("source.mp3");
        let dst = temp_dir.join("destination.mp3");
        fs::write(&src, b"source content").unwrap();
        fs::write(&dst, b"destination original").unwrap();

        let res = move_file_safely(&src, &dst, false);
        assert!(res.is_err(), "No debe sobrescribir sin permiso");
        assert_eq!(fs::read(&dst).unwrap(), b"destination original");
        assert!(src.exists());

        // Con permiso de sobrescritura sí debe proceder
        let ok_res = move_file_safely(&src, &dst, true);
        assert!(ok_res.is_ok());
        assert_eq!(fs::read(&dst).unwrap(), b"source content");
        assert!(!src.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_long_unicode_path_resolution_and_file_creation() {
        let temp_dir = std::env::temp_dir().join(format!("yt_long_test_{}", Uuid::new_v4()));
        let _ = fs::create_dir_all(&temp_dir);

        // Título Unicode extremadamente largo (250+ caracteres)
        let long_title = "Canción con Título Súper Largo y Caracteres Especiales: 日本語の曲名と絵文字 ✨🎵🎉 y símbolos prohibidos <|>/?*\" y más texto para exceder doscientos caracteres con creces y probar el recorte seguro en Windows";
        let resolved =
            resolve_destination_path(&temp_dir, long_title, None, ExistingFilePolicy::Rename)
                .unwrap();

        let filename = resolved.file_name().unwrap().to_str().unwrap();
        // Verificar que termina en .mp3 y no contiene caracteres prohibidos
        assert!(filename.ends_with(".mp3"));
        assert!(!filename.contains('<') && !filename.contains('>') && !filename.contains(':'));
        assert!(!filename.contains('"') && !filename.contains('/') && !filename.contains('\\'));
        assert!(!filename.contains('|') && !filename.contains('?') && !filename.contains('*'));

        // Verificar que se puede crear y escribir el archivo físicamente en el sistema de archivos de Windows
        fs::write(&resolved, b"audio test").expect("Failed to write long Unicode file");
        assert!(resolved.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
