//! Comandos de archivos: exportación, apertura en el explorador, rutas de
//! grabaciones y borrado.
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::paths;

/// Escritura del archivo de exportación en la ruta elegida por el usuario
/// (el diálogo de guardado ya validó la ruta con interacción explícita).
#[tauri::command]
pub fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("No se pudo escribir {path}: {e}"))
}

#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("No se pudo leer {path}: {e}"))
}

#[tauri::command]
pub fn file_size(path: String) -> Result<u64, String> {
    std::fs::metadata(&path)
        .map(|m| m.len())
        .map_err(|e| format!("No se pudo leer {path}: {e}"))
}

/// Abre una ruta (archivo o carpeta) con la app del sistema.
#[tauri::command]
pub fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| format!("No se pudo abrir {path}: {e}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingPaths {
    pub dir: String,
    pub mix: Option<String>,
    pub mic: Option<String>,
    pub system: Option<String>,
    pub file: Option<String>,
    pub duration_ms: Option<u64>,
}

fn existing(p: std::path::PathBuf) -> Option<String> {
    if p.exists() {
        Some(p.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Rutas de audio de una sesión (las que existan).
#[tauri::command]
pub fn recording_paths(app: AppHandle, session_id: String) -> Result<RecordingPaths, String> {
    let dir = paths::session_audio_dir(&app, &session_id)?;
    let mix = dir.join("mix.wav");
    let mic = dir.join("mic.wav");
    let system = dir.join("system.wav");
    let file = dir.join("file.wav");
    let duration_ms = [&mix, &file, &mic, &system]
        .iter()
        .find(|p| p.exists())
        .and_then(|p| crate::audio::wav::wav_duration_ms(p));
    Ok(RecordingPaths {
        dir: dir.to_string_lossy().to_string(),
        mix: existing(mix),
        mic: existing(mic),
        system: existing(system),
        file: existing(file),
        duration_ms,
    })
}

/// Borra la carpeta de audio de una sesión (al eliminarla del historial).
#[tauri::command]
pub fn delete_recording(app: AppHandle, session_id: String) -> Result<(), String> {
    let dir = paths::session_audio_dir(&app, &session_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("No se pudo borrar {}: {e}", dir.display()))?;
    }
    Ok(())
}

#[tauri::command]
pub fn data_dir(app: AppHandle) -> Result<String, String> {
    Ok(paths::app_local_data(&app)?.to_string_lossy().to_string())
}
