//! Transcripción desde archivo (o retranscripción de los WAV de una sesión).
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tokio::sync::watch;

use crate::audio::decode::decode_to_pcm16k;
use crate::audio::wav::encode_wav_in_memory;
use crate::audio::TARGET_RATE;
use crate::paths;
use crate::state::AppState;
use crate::stt::{emit_progress, Engine, EngineId, ProgressPayload, Source, SttSpec};

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileConfig {
    pub session_id: String,
    pub engine: EngineId,
    pub model: Option<String>,
    pub path: String,
    pub language: Option<String>,
    /// Etiqueta de origen de los segmentos (File por defecto; Mic/System al retranscribir).
    pub source: Option<Source>,
    /// Copiar el audio decodificado a recordings/<id>/file.wav para reproducirlo.
    pub copy_audio: Option<bool>,
    pub whisper_threads: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    pub session_id: String,
    pub duration_ms: u64,
    pub audio_dir: Option<String>,
    pub audio_path: Option<String>,
}

fn progress(session_id: &str, phase: &'static str, percent: f32, message: Option<String>) -> ProgressPayload {
    ProgressPayload {
        session_id: session_id.to_string(),
        phase,
        percent,
        message,
    }
}

#[tauri::command]
pub async fn transcribe_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    cfg: FileConfig,
) -> Result<FileResult, String> {
    let (cancel_tx, cancel_rx) = watch::channel(false);
    {
        let mut guard = state.file_cancel.lock().unwrap();
        if let Some(prev) = guard.take() {
            let _ = prev.send(true);
        }
        *guard = Some(cancel_tx);
    }
    let result = run(app.clone(), state.inner().clone(), cfg.clone(), cancel_rx).await;
    *state.file_cancel.lock().unwrap() = None;
    match &result {
        Ok(_) => emit_progress(&app, progress(&cfg.session_id, "done", 100.0, None)),
        Err(e) if e == "Cancelado" => {
            emit_progress(&app, progress(&cfg.session_id, "cancelled", 0.0, None))
        }
        Err(e) => emit_progress(&app, progress(&cfg.session_id, "error", 0.0, Some(e.clone()))),
    }
    result
}

async fn run(
    app: AppHandle,
    state: Arc<AppState>,
    cfg: FileConfig,
    cancel: watch::Receiver<bool>,
) -> Result<FileResult, String> {
    let path = PathBuf::from(&cfg.path);
    if !path.exists() {
        return Err(format!("No existe el archivo {}", path.display()));
    }
    let session_id = cfg.session_id.clone();
    emit_progress(&app, progress(&session_id, "decoding", 0.0, Some("Decodificando audio…".into())));

    // Decodificación en un hilo bloqueante con progreso limitado a ~5/s
    let pcm = {
        let app2 = app.clone();
        let sid = session_id.clone();
        let p = path.clone();
        tokio::task::spawn_blocking(move || {
            let mut last = Instant::now();
            decode_to_pcm16k(&p, |pct| {
                if last.elapsed().as_millis() >= 200 || pct >= 100.0 {
                    last = Instant::now();
                    emit_progress(&app2, progress(&sid, "decoding", pct, None));
                }
            })
        })
        .await
        .map_err(|e| e.to_string())??
    };
    if *cancel.borrow() {
        return Err("Cancelado".into());
    }
    let duration_ms = (pcm.len() as u64 * 1000) / TARGET_RATE as u64;

    // Copia del audio para reproducirlo desde el historial
    let mut audio_dir = None;
    let mut audio_path = None;
    if cfg.copy_audio.unwrap_or(false) {
        let dir = paths::session_audio_dir(&app, &session_id)?;
        paths::ensure_dir(&dir)?;
        let dest = dir.join("file.wav");
        let bytes = encode_wav_in_memory(&pcm, TARGET_RATE);
        tokio::fs::write(&dest, bytes)
            .await
            .map_err(|e| format!("No se pudo guardar {}: {e}", dest.display()))?;
        audio_dir = Some(dir.to_string_lossy().to_string());
        audio_path = Some(dest.to_string_lossy().to_string());
    }

    let threads = cfg.whisper_threads.unwrap_or(4);
    let engine = Engine::build(&app, &state, cfg.engine, cfg.model.clone(), threads).await?;
    let spec = SttSpec {
        session_id: session_id.clone(),
        source: cfg.source.unwrap_or(Source::File),
        language: cfg.language.clone().unwrap_or_else(|| "auto".into()),
    };
    emit_progress(&app, progress(&session_id, "transcribing", 0.0, None));
    engine
        .transcribe_pcm(app.clone(), state.clone(), spec, pcm, cancel)
        .await?;

    Ok(FileResult {
        session_id,
        duration_ms,
        audio_dir,
        audio_path,
    })
}

#[tauri::command]
pub fn transcribe_cancel(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if let Some(tx) = state.file_cancel.lock().unwrap().as_ref() {
        let _ = tx.send(true);
    }
    Ok(())
}
