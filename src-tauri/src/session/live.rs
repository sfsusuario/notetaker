//! Comandos de la sesión en vivo: start / stop / pause / resume / status.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use super::pipeline::{spawn_source_stream, StartedSource};
use crate::audio::wav::{mix_to_file, wav_duration_ms};
use crate::paths;
use crate::state::{AppState, LiveSession};
use crate::stt::{now_ms, Engine, EngineId, Source, SttSpec};

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LiveConfig {
    pub session_id: String,
    /// None = solo grabar (se guarda el WAV y se transcribe más tarde).
    pub engine: Option<EngineId>,
    pub model: Option<String>,
    pub sources: Vec<Source>,
    pub mic_device_id: Option<String>,
    pub system_device_id: Option<String>,
    pub language: Option<String>,
    pub whisper_threads: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub session_id: String,
    pub audio_dir: String,
    pub sources: Vec<StartedSource>,
    pub started_at: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    pub session_id: String,
    pub duration_ms: u64,
    pub mix: Option<String>,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub session_id: String,
    pub engine: Option<EngineId>,
    pub sources: Vec<Source>,
    pub paused: bool,
    pub started_at: u64,
}

#[tauri::command]
pub async fn session_start_live(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    cfg: LiveConfig,
) -> Result<StartResult, String> {
    if state.is_live() {
        stop_live(&app, &state, "replaced").await;
    }
    if cfg.sources.is_empty() {
        return Err("Elige al menos una fuente de audio (micrófono o sistema).".into());
    }
    if cfg.sources.contains(&Source::File) {
        return Err("La fuente 'archivo' no es válida en una sesión en vivo.".into());
    }
    let threads = cfg.whisper_threads.unwrap_or(4);
    let engine = match cfg.engine {
        Some(id) => Some(Engine::build(&app, &state, id, cfg.model.clone(), threads).await?),
        None => None,
    };
    let audio_dir = paths::session_audio_dir(&app, &cfg.session_id)?;
    paths::ensure_dir(&audio_dir)?;
    let language = cfg.language.clone().unwrap_or_else(|| "auto".into());

    let mut handles = Vec::new();
    let mut started = Vec::new();
    let mut labels: Vec<(Source, String)> = Vec::new();
    for source in cfg.sources.iter().copied() {
        let device_id = match source {
            Source::Mic => cfg.mic_device_id.clone(),
            Source::System => cfg.system_device_id.clone(),
            Source::File => None,
        };
        let wav_path = audio_dir.join(format!("{}.wav", source.as_str()));
        let spec = SttSpec {
            session_id: cfg.session_id.clone(),
            source,
            language: language.clone(),
        };
        match spawn_source_stream(
            app.clone(),
            state.inner().clone(),
            engine.clone(),
            spec,
            device_id,
            wav_path,
            Arc::new(AtomicBool::new(false)),
        ) {
            Ok((h, s)) => {
                labels.push((source, s.device_label.clone()));
                handles.push((source, h));
                started.push(s);
            }
            Err(e) => {
                for (_, h) in &handles {
                    let _ = h.stop_tx.send(true);
                    h.capture.stop();
                }
                return Err(e);
            }
        }
    }

    let started_at = now_ms();
    *state.live.lock().unwrap() = Some(LiveSession {
        session_id: cfg.session_id.clone(),
        engine: cfg.engine,
        sources: handles,
        audio_dir: audio_dir.clone(),
        started_at,
        device_labels: labels,
    });
    Ok(StartResult {
        session_id: cfg.session_id,
        audio_dir: audio_dir.to_string_lossy().to_string(),
        sources: started,
        started_at,
    })
}

/// Detiene la sesión: para la captura (el pipeline cierra el WAV y el motor
/// drena sus últimos resultados), mezcla los WAV y emite `session://stopped`.
pub async fn stop_live(app: &AppHandle, state: &Arc<AppState>, reason: &str) -> Option<StopResult> {
    let session = state.live.lock().unwrap().take()?;
    let session_id = session.session_id.clone();
    let audio_dir = session.audio_dir.clone();

    // 1) Parar la captura: frame_tx se cierra → pipeline termina → chunk_tx se cierra → motor drena
    for (_, h) in &session.sources {
        h.capture.stop();
    }
    let mut wavs = Vec::new();
    let mut sources: Vec<(Source, std::path::PathBuf)> = Vec::new();
    let device_labels = session.device_labels.clone();
    for (src, mut h) in session.sources {
        if let Some(p) = h.pipeline.take() {
            if tokio::time::timeout(Duration::from_secs(5), p).await.is_err() {
                let _ = h.stop_tx.send(true);
            }
        }
        if let Some(e) = h.engine.take() {
            if tokio::time::timeout(Duration::from_secs(45), e).await.is_err() {
                let _ = h.stop_tx.send(true);
            }
        }
        sources.push((src, h.wav_path.clone()));
        wavs.push(h.wav_path.clone());
    }

    // Una fuente puede acabar sin muestras por motivos normales: WASAPI no
    // entrega nada cuando el dispositivo de salida está en reposo (no sonó
    // nada) y, con auriculares Bluetooth, usar su micrófono los pasa a modo
    // manos libres y desactiva la captura del audio del equipo. Se avisa como
    // información (no como error) y solo si la sesión duró lo suficiente para
    // que la ausencia de audio sea significativa.
    let longest_ms = sources
        .iter()
        .filter_map(|(_, p)| wav_duration_ms(p))
        .max()
        .unwrap_or(0);
    for (src, path) in &sources {
        let dur = wav_duration_ms(path).unwrap_or(0);
        if dur >= 300 {
            continue;
        }
        // Se descarta el WAV vacío: si no, aparece como pista de la sesión y
        // luego falla al intentar transcribirlo.
        let _ = std::fs::remove_file(path);
        if longest_ms < 5_000 {
            continue;
        }
        let device = device_labels
            .iter()
            .find(|(s, _)| s == src)
            .map(|(_, d)| d.as_str())
            .unwrap_or("el dispositivo elegido");
        let message = match src {
            Source::System => format!(
                "No se guardó audio del sistema: durante la sesión no sonó nada en «{device}». Con auriculares Bluetooth, además, usar su micrófono los pone en modo manos libres y desactiva la captura del audio del equipo."
            ),
            Source::Mic => format!(
                "No se guardó audio del micrófono «{device}»: no llegó ninguna muestra. Revisa que no esté silenciado y que la app tenga permiso para usarlo."
            ),
            Source::File => continue,
        };
        let _ = app.emit("audio://warning", serde_json::json!({ "message": message }));
    }

    // 2) Mezcla para reproducción
    let mix_path = audio_dir.join("mix.wav");
    let mp = mix_path.clone();
    let duration_ms = tokio::task::spawn_blocking(move || mix_to_file(&wavs, &mp))
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(0);

    let result = StopResult {
        session_id,
        duration_ms,
        mix: if mix_path.exists() {
            Some(mix_path.to_string_lossy().to_string())
        } else {
            None
        },
        reason: reason.to_string(),
    };
    let _ = app.emit("session://stopped", result.clone());
    Some(result)
}

#[tauri::command]
pub async fn session_stop(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<Option<StopResult>, String> {
    Ok(stop_live(&app, &state, "user").await)
}

fn set_paused(state: &AppState, paused: bool) -> Result<(), String> {
    let guard = state.live.lock().unwrap();
    let session = guard.as_ref().ok_or("No hay una sesión en curso")?;
    for (_, h) in &session.sources {
        h.paused.store(paused, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn session_pause(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    set_paused(&state, true)
}

#[tauri::command]
pub fn session_resume(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    set_paused(&state, false)
}

#[tauri::command]
pub fn session_status(state: State<'_, Arc<AppState>>) -> Option<LiveStatus> {
    let guard = state.live.lock().unwrap();
    let s = guard.as_ref()?;
    Some(LiveStatus {
        session_id: s.session_id.clone(),
        engine: s.engine,
        sources: s.sources.iter().map(|(src, _)| *src).collect(),
        paused: s
            .sources
            .first()
            .map(|(_, h)| h.paused.load(Ordering::Relaxed))
            .unwrap_or(false),
        started_at: s.started_at,
    })
}
