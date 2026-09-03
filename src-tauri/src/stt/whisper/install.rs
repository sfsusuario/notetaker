//! Instalación del motor local: binario oficial precompilado de whisper.cpp
//! (`whisper-bin-x64.zip`, CPU) y modelos GGML desde Hugging Face. Todo va a
//! `<app_local_data>/whisper/{bin,models}`; `scripts/setup-whisper.ps1`
//! produce el mismo layout.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::paths;
use crate::state::AppState;

pub const RELEASE_TAG: &str = "b4938";

/// (id, etiqueta, tamaño aprox. MB)
pub const MODELS: &[(&str, &str, u32)] = &[
    ("tiny", "Tiny — muy rápido, calidad baja", 75),
    ("base", "Base — recomendado en vivo (CPU)", 142),
    ("small", "Small — mejor calidad, más lento", 466),
    ("medium", "Medium — alta calidad, lento en CPU", 1500),
    ("large-v3-turbo", "Large v3 Turbo — máxima calidad, muy lento en CPU", 1600),
];

pub fn zip_url() -> String {
    format!("https://github.com/ggml-org/whisper.cpp/releases/download/{RELEASE_TAG}/whisper-bin-x64.zip")
}

pub fn model_url(id: &str) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{id}.bin")
}

pub fn bin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(paths::whisper_dir(app)?.join("bin"))
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(paths::whisper_dir(app)?.join("models"))
}

pub fn server_exe(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(bin_dir(app)?.join("whisper-server.exe"))
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(format!("ggml-{id}.bin")))
}

fn is_valid_model(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.len() > 1_000_000).unwrap_or(false)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelStatus {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub size_mb: u32,
    pub path: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunningInfo {
    pub model: String,
    pub port: u16,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WhisperStatus {
    pub server_installed: bool,
    pub server_path: String,
    pub models_dir: String,
    pub release_tag: String,
    pub models: Vec<WhisperModelStatus>,
    pub running: Option<RunningInfo>,
}

/// Estado en disco (sin consultar el proceso).
pub fn status(app: &AppHandle) -> WhisperStatus {
    let exe = server_exe(app).unwrap_or_default();
    let mdir = models_dir(app).unwrap_or_default();
    let models = MODELS
        .iter()
        .map(|(id, label, size)| {
            let p = mdir.join(format!("ggml-{id}.bin"));
            WhisperModelStatus {
                id: id.to_string(),
                label: label.to_string(),
                installed: is_valid_model(&p),
                size_mb: *size,
                path: p.to_string_lossy().to_string(),
            }
        })
        .collect();
    WhisperStatus {
        server_installed: exe.exists(),
        server_path: exe.to_string_lossy().to_string(),
        models_dir: mdir.to_string_lossy().to_string(),
        release_tag: RELEASE_TAG.into(),
        models,
        running: None,
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    item: String,
    downloaded: u64,
    total: Option<u64>,
    percent: f32,
    /// downloading | extracting | done | error
    phase: &'static str,
    message: Option<String>,
}

fn emit_dl(app: &AppHandle, p: DownloadProgress) {
    let _ = app.emit("whisper://download-progress", p);
}

async fn download_with_progress(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    item: &str,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        paths::ensure_dir(parent)?;
    }
    let part = dest.with_extension("part");
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Descarga de {item}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Descarga de {item}: HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| format!("No se pudo crear {}: {e}", part.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Descarga de {item}: {e}"))? {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Escritura de {item}: {e}"))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed().as_millis() >= 200 {
            last_emit = Instant::now();
            emit_dl(
                app,
                DownloadProgress {
                    item: item.into(),
                    downloaded,
                    total,
                    percent: total
                        .map(|t| (downloaded as f32 / t as f32) * 100.0)
                        .unwrap_or(0.0),
                    phase: "downloading",
                    message: None,
                },
            );
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| format!("No se pudo mover {}: {e}", dest.display()))?;
    emit_dl(
        app,
        DownloadProgress {
            item: item.into(),
            downloaded,
            total,
            percent: 100.0,
            phase: "downloading",
            message: None,
        },
    );
    Ok(())
}

/// Extrae el zip aplanando `Release/*` → `bin/*` (el exe necesita las DLLs al lado).
fn extract_release(zip_path: &Path, bin: &Path) -> Result<(), String> {
    paths::ensure_dir(bin)?;
    let file = std::fs::File::open(zip_path).map_err(|e| format!("No se pudo abrir el zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Zip inválido: {e}"))?;
    let mut extracted = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("Zip: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let file_name = match name.rsplit('/').next() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        // Solo binarios/DLLs; se omiten tests y ejemplos pesados que no usamos.
        let lower = file_name.to_lowercase();
        let keep = lower.ends_with(".dll")
            || lower == "whisper-server.exe"
            || lower == "whisper-cli.exe"
            || lower == "whisper-stream.exe";
        if !keep {
            continue;
        }
        let dest = bin.join(&file_name);
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("Zip ({file_name}): {e}"))?;
        std::fs::write(&dest, buf).map_err(|e| format!("No se pudo escribir {}: {e}", dest.display()))?;
        extracted += 1;
    }
    if extracted == 0 {
        return Err("El zip no contenía binarios reconocibles".into());
    }
    Ok(())
}

/// Instala el servidor (si falta o `include_server`) y/o un modelo.
#[tauri::command]
pub async fn whisper_install(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    model: Option<String>,
    include_server: Option<bool>,
) -> Result<WhisperStatus, String> {
    let client = state.http.clone();
    let exe = server_exe(&app)?;
    if include_server.unwrap_or(false) || !exe.exists() {
        let wdir = paths::whisper_dir(&app)?;
        paths::ensure_dir(&wdir)?;
        let zip_path = wdir.join("whisper-bin-x64.zip");
        if let Err(e) = download_with_progress(&app, &client, &zip_url(), &zip_path, "server").await {
            emit_dl(&app, DownloadProgress { item: "server".into(), downloaded: 0, total: None, percent: 0.0, phase: "error", message: Some(e.clone()) });
            return Err(e);
        }
        emit_dl(&app, DownloadProgress { item: "server".into(), downloaded: 0, total: None, percent: 100.0, phase: "extracting", message: None });
        // Si había un server corriendo, pararlo antes de sobrescribir DLLs
        super::server::stop(&state).await;
        let bin = bin_dir(&app)?;
        let zp = zip_path.clone();
        tokio::task::spawn_blocking(move || extract_release(&zp, &bin))
            .await
            .map_err(|e| e.to_string())??;
        let _ = std::fs::remove_file(&zip_path);
        emit_dl(&app, DownloadProgress { item: "server".into(), downloaded: 0, total: None, percent: 100.0, phase: "done", message: None });
    }

    if let Some(id) = model {
        if !MODELS.iter().any(|(m, _, _)| *m == id) {
            return Err(format!("Modelo desconocido: {id}"));
        }
        let dest = model_path(&app, &id)?;
        if !is_valid_model(&dest) {
            if let Err(e) = download_with_progress(&app, &client, &model_url(&id), &dest, &id).await {
                emit_dl(&app, DownloadProgress { item: id.clone(), downloaded: 0, total: None, percent: 0.0, phase: "error", message: Some(e.clone()) });
                return Err(e);
            }
            emit_dl(&app, DownloadProgress { item: id.clone(), downloaded: 0, total: None, percent: 100.0, phase: "done", message: None });
        }
    }
    whisper_status(app, state).await
}

#[tauri::command]
pub async fn whisper_status(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<WhisperStatus, String> {
    let mut st = status(&app);
    if let Ok(mut guard) = state.whisper.try_lock() {
        if let Some(s) = guard.as_mut() {
            if s.alive() {
                st.running = Some(RunningInfo {
                    model: s.model.clone(),
                    port: s.port,
                });
            }
        }
    }
    Ok(st)
}

#[tauri::command]
pub async fn whisper_delete_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    model: String,
) -> Result<WhisperStatus, String> {
    let p = model_path(&app, &model)?;
    {
        let mut guard = state.whisper.lock().await;
        if let Some(s) = guard.as_mut() {
            if s.model == model {
                s.kill();
                *guard = None;
            }
        }
    }
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| format!("No se pudo borrar {}: {e}", p.display()))?;
    }
    whisper_status(app, state).await
}

#[tauri::command]
pub async fn whisper_stop_server(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    super::server::stop(&state).await;
    Ok(())
}
