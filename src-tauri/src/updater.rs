use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::processes::{tool_path, Tool};

/// Información de una versión de yt-dlp en el manifiesto.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseEntry {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// Manifiesto local con versiones conocidas de yt-dlp.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub current_version: String,
    pub releases: Vec<ReleaseEntry>,
}

/// Estado de la versión actual.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YtdlpVersionInfo {
    pub version: String,
    pub binary_path: String,
}

/// Resultado de una operación de actualización.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub previous_version: String,
    pub new_version: String,
    pub success: bool,
    pub message: String,
}

/// Obtiene la versión actual de yt-dlp ejecutando `yt-dlp --version`.
pub async fn get_current_version(resource_dir: &Path) -> Result<YtdlpVersionInfo, String> {
    let path = tool_path(resource_dir, Tool::YtDlp)?;

    let output = tokio::process::Command::new(&path)
        .arg("--version")
        .output()
        .await
        .map_err(|e| format!("Error al ejecutar yt-dlp --version: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.trim().to_string();

    Ok(YtdlpVersionInfo {
        version,
        binary_path: path.display().to_string(),
    })
}

/// Calcula el hash SHA-256 de un archivo.
fn compute_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Error al leer archivo para hash: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let result = hasher.finalize();
    Ok(hex::encode(result))
}

/// Carga el manifiesto de actualizaciones desde el directorio de recursos.
fn load_manifest(resource_dir: &Path) -> Result<UpdateManifest, String> {
    let manifest_path = resource_dir
        .join("yt-dlp-manifest.json")
        .to_path_buf();

    if !manifest_path.exists() {
        return Ok(UpdateManifest {
            current_version: String::new(),
            releases: Vec::new(),
        });
    }

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Error al leer manifiesto: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Error al parsear manifiesto: {}", e))
}

/// Guarda el manifiesto de actualizaciones.
fn save_manifest(resource_dir: &Path, manifest: &UpdateManifest) -> Result<(), String> {
    let manifest_path = resource_dir.join("yt-dlp-manifest.json");
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Error al serializar manifiesto: {}", e))?;
    fs::write(&manifest_path, content)
        .map_err(|e| format!("Error al guardar manifiesto: {}", e))?;
    Ok(())
}

/// Descarga el manifiesto desde una URL remota y lo fusiona con el local.
pub async fn check_for_updates(
    resource_dir: &Path,
    manifest_url: &str,
    token: &CancellationToken,
) -> Result<Vec<ReleaseEntry>, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Error al crear cliente HTTP: {}", e))?;

    let response = tokio::select! {
        res = client.get(manifest_url).send() => res,
        _ = token.cancelled() => {
            return Err("Consulta de actualizaciones cancelada".into());
        }
    };

    let response = response.map_err(|e| format!("Error al descargar manifiesto: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "El servidor devolvió código {}",
            response.status().as_u16()
        ));
    }

    let remote_manifest: UpdateManifest = response
        .json()
        .await
        .map_err(|e| format!("Error al parsear manifiesto remoto: {}", e))?;

    let mut local_manifest = load_manifest(resource_dir)?;

    // Fusionar: agregar versiones que no existen localmente
    let existing_versions: std::collections::HashSet<String> =
        local_manifest.releases.iter().map(|r| r.version.clone()).collect();

    for release in &remote_manifest.releases {
        if !existing_versions.contains(&release.version) {
            local_manifest.releases.push(release.clone());
        }
    }

    save_manifest(resource_dir, &local_manifest)?;

    Ok(remote_manifest.releases)
}

/// Descarga y reemplaza el binario de yt-dlp con verificación de hash y rollback.
pub async fn update_binary(
    resource_dir: &Path,
    release: &ReleaseEntry,
    token: &CancellationToken,
) -> Result<UpdateResult, String> {
    let current_info = get_current_version(resource_dir).await?;
    let binary_path = PathBuf::from(&current_info.binary_path);

    // Crear backup del binario actual
    let backup_path = binary_path.with_extension("exe.bak");
    fs::copy(&binary_path, &backup_path)
        .map_err(|e| format!("Error al crear backup: {}", e))?;

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Error al crear cliente HTTP: {}", e))?;

    // Descargar el nuevo binario a un archivo temporal
    let temp_path = binary_path.with_extension("exe.tmp");

    let response = tokio::select! {
        res = client.get(&release.url).send() => res,
        _ = token.cancelled() => {
            let _ = fs::remove_file(&temp_path);
            return Err("Descarga de actualización cancelada".into());
        }
    };

    let response = response.map_err(|e| format!("Error al descargar actualización: {}", e))?;

    if !response.status().is_success() {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "El servidor devolvió código {} al descargar",
            response.status().as_u16()
        ));
    }

    let mut file = fs::File::create(&temp_path)
        .map_err(|e| format!("Error al crear archivo temporal: {}", e))?;

    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|e| format!("Error al recibir datos: {}", e))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("Error al escribir datos: {}", e))?;
    }
    drop(file);

    // Verificar hash SHA-256
    let downloaded_hash = compute_sha256(&temp_path)?;
    if downloaded_hash.to_lowercase() != release.sha256.to_lowercase() {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_file(&backup_path);
        return Err(format!(
            "Verificación de integridad fallida. Se esperaba {}, se obtuvo {}",
            release.sha256, downloaded_hash
        ));
    }

    // Reemplazar el binario original
    fs::rename(&temp_path, &binary_path)
        .map_err(|e| format!("Error al reemplazar binario: {}", e))?;

    // Verificar que el nuevo binario funciona
    let version_check = get_current_version(resource_dir).await;
    match version_check {
        Ok(info) => {
            let _ = fs::remove_file(&backup_path);
            Ok(UpdateResult {
                previous_version: current_info.version,
                new_version: info.version,
                success: true,
                message: "Actualización completada correctamente.".to_string(),
            })
        }
        Err(_) => {
            // Rollback: restaurar el binario anterior
            fs::rename(&backup_path, &binary_path)
                .map_err(|_| "Fallo tanto la actualización como el rollback".to_string())?;
            Err("El binario actualizado no funciona. Se restauró la versión anterior.".into())
        }
    }
}

/// Guarda una entrada en el manifiesto local para una versión específica.
pub fn register_version(
    resource_dir: &Path,
    version: &str,
    sha256: &str,
    url: &str,
) -> Result<(), String> {
    let mut manifest = load_manifest(resource_dir)?;

    // Actualizar current_version
    manifest.current_version = version.to_string();

    // Agregar entrada si no existe
    if !manifest.releases.iter().any(|r| r.version == version) {
        manifest.releases.push(ReleaseEntry {
            version: version.to_string(),
            url: url.to_string(),
            sha256: sha256.to_string(),
            size_bytes: 0,
        });
    }

    save_manifest(resource_dir, &manifest)
}
