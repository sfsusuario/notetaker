//! Instalación del motor local: binario oficial precompilado de whisper.cpp
//! (`whisper-bin-x64.zip`, CPU) y modelos GGML desde Hugging Face. Todo va a
//! `<app_local_data>/whisper/{bin,models}`; `scripts/setup-whisper.ps1`
//! produce el mismo layout.
//!
//! Aceleración GPU (opcional): `whisper/gpu/ggml-vulkan.dll`, compilado del
//! mismo tag por `scripts/build-whisper-vulkan.ps1` (whisper.cpp no publica
//! binarios Vulkan para Windows). Vive fuera de `bin/` a propósito: ggml carga
//! cualquier `ggml-*.dll` que encuentre junto al exe, y solo `server.rs` sabe
//! activarlo con los ajustes correctos (`GGML_BACKEND_PATH`).
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

/// (id, etiqueta, tamaño aprox. MB). Las variantes cuantizadas (q5/q8) dan el
/// mismo texto con menos memoria y son más rápidas en CPU: en un Core Ultra 7
/// 258V, small-q5_1 va a 4.2x tiempo real frente a 3.0x de small.
pub const MODELS: &[(&str, &str, u32)] = &[
    ("tiny", "Tiny — muy rápido, calidad baja", 75),
    ("base", "Base — rápido en cualquier CPU", 142),
    ("small-q5_1", "Small Q5 — buena calidad, recomendado en vivo sin GPU", 182),
    ("small", "Small — igual que Small Q5 pero más pesado", 466),
    ("medium-q5_0", "Medium Q5 — alta calidad, lento sin GPU", 515),
    ("medium", "Medium — alta calidad, lento sin GPU", 1500),
    ("large-v3-turbo-q5_0", "Large v3 Turbo Q5 — casi máxima calidad, más ligero", 548),
    ("large-v3-turbo-q8_0", "Large v3 Turbo Q8 — máxima calidad, recomendado con GPU", 834),
    ("large-v3-turbo", "Large v3 Turbo — máxima calidad, el más pesado", 1600),
];

/// Silero VAD para whisper.cpp (~0.9 MB). Con él, el ruido (teclas, golpes,
/// música) no llega al modelo: en una llamada de prueba, 23 de 41 frases del
/// micrófono eran alucinaciones sobre ruido ("Thank you.", "Спасибо.") y el
/// servidor trabajó 3.3 veces menos con el VAD activo.
pub const VAD_MODEL: &str = "ggml-silero-v5.1.2.bin";
const VAD_URL: &str = "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin";

pub fn vad_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join(VAD_MODEL))
}

/// Ruta del modelo VAD si está descargado.
pub fn vad_model(app: &AppHandle) -> Option<PathBuf> {
    let p = vad_model_path(app).ok()?;
    std::fs::metadata(&p).map(|m| m.len() > 500_000).unwrap_or(false).then_some(p)
}

/// Descarga el modelo VAD si falta (se llama al arrancar y al instalar).
pub async fn ensure_vad_model(app: &AppHandle, client: &reqwest::Client) -> Result<PathBuf, String> {
    if let Some(p) = vad_model(app) {
        return Ok(p);
    }
    let dest = vad_model_path(app)?;
    download_with_progress(app, client, VAD_URL, &dest, "vad").await?;
    Ok(dest)
}

/// Backend Vulkan opcional (ver cabecera del módulo).
pub const GPU_DLL: &str = "ggml-vulkan.dll";
/// Tag de whisper.cpp con el que se compiló el DLL: si no coincide con
/// `RELEASE_TAG`, su ABI puede no casar con `ggml-base.dll` y no se usa.
pub const GPU_TAG_FILE: &str = "ggml-vulkan.tag";

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

pub fn gpu_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(paths::whisper_dir(app)?.join("gpu"))
}

/// none | stale (compilado para otro tag) | ready
pub fn gpu_backend_state(app: &AppHandle) -> &'static str {
    let Ok(dir) = gpu_dir(app) else { return "none" };
    if !dir.join(GPU_DLL).exists() {
        return "none";
    }
    let tag = std::fs::read_to_string(dir.join(GPU_TAG_FILE)).unwrap_or_default();
    if tag.trim() == RELEASE_TAG {
        "ready"
    } else {
        "stale"
    }
}

/// Ruta del DLL Vulkan si es utilizable con el servidor instalado.
pub fn gpu_backend_path(app: &AppHandle) -> Option<PathBuf> {
    if gpu_backend_state(app) != "ready" {
        return None;
    }
    gpu_dir(app).ok().map(|d| d.join(GPU_DLL))
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
    /// "CPU" o el nombre del dispositivo GPU que usa el servidor.
    pub backend: String,
    pub threads: u32,
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
    /// none | stale | ready (backend Vulkan opcional)
    pub gpu_backend: String,
    pub gpu_dir: String,
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
        gpu_backend: gpu_backend_state(app).into(),
        gpu_dir: gpu_dir(app).unwrap_or_default().to_string_lossy().to_string(),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadProgress {
    pub item: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub percent: f32,
    /// downloading | extracting | done | error
    pub phase: &'static str,
    pub message: Option<String>,
}

pub(crate) fn emit_dl(app: &AppHandle, p: DownloadProgress) {
    let _ = app.emit("whisper://download-progress", p);
}

pub(crate) async fn download_with_progress(
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
    if let Err(e) = ensure_vad_model(&app, &client).await {
        eprintln!("[whisper] no se pudo descargar el modelo VAD: {e}");
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
                    backend: s.backend_label(),
                    threads: s.threads,
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
