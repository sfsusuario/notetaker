//! Rutas de datos de la app (todo bajo app_local_data_dir, fuera de Roaming:
//! los modelos de whisper pesan cientos de MB).
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub fn app_local_data(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| format!("No se pudo resolver la carpeta de datos: {e}"))
}

pub fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_local_data(app)?.join("recordings"))
}

pub fn session_audio_dir(app: &AppHandle, session_id: &str) -> Result<PathBuf, String> {
    // El id lo genera el frontend (uuid); se sanea igualmente por si acaso.
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err("Id de sesión inválido".into());
    }
    Ok(recordings_dir(app)?.join(safe))
}

pub fn whisper_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_local_data(app)?.join("whisper"))
}

pub fn ensure_dir(p: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("No se pudo crear {}: {e}", p.display()))
}
