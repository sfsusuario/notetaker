//! Whisper en vivo (pseudo tiempo real): corta frases por silencio, las envía
//! a whisper-server y emite `stt://final` 1–4 s después. Mientras hay frases
//! en cola emite un `stt://partial` vacío ("escribiendo…").
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::{mpsc, watch};

use super::{client, server};
use crate::audio::utterance::{CutterConfig, Utterance, UtteranceCutter};
use crate::audio::wav::encode_wav_in_memory;
use crate::audio::TARGET_RATE;
use crate::state::AppState;
use crate::stt::{
    emit_error, emit_segment, emit_status, now_ms, segment_id, AudioChunk, EngineId,
    SegmentPayload, StatusPayload, SttSpec,
};

fn status(spec: &SttSpec, status: &'static str, message: Option<String>) -> StatusPayload {
    StatusPayload {
        session_id: spec.session_id.clone(),
        source: spec.source,
        engine: EngineId::Whisper,
        status,
        latency_ms: 0,
        retry_count: 0,
        message,
    }
}

/// Parcial "vacío": la UI muestra la burbuja de escritura mientras `pending`.
fn emit_pending(app: &AppHandle, spec: &SttSpec, pending: bool) {
    emit_segment(
        app,
        SegmentPayload {
            id: format!("{}-pending", spec.source.as_str()),
            session_id: spec.session_id.clone(),
            source: spec.source,
            speaker: None,
            text: if pending { "…".into() } else { String::new() },
            start_ms: 0,
            end_ms: 0,
            received_at: now_ms(),
            is_final: false,
            language: None,
        },
    );
}

pub fn inference_timeout(duration_ms: u64) -> Duration {
    Duration::from_secs(60 + (duration_ms / 1000) * 3)
}

pub async fn run(
    app: AppHandle,
    state: Arc<AppState>,
    model: String,
    spec: SttSpec,
    mut rx: mpsc::Receiver<AudioChunk>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let Some(port) = server::current_port(&state).await else {
        emit_error(&app, "El servidor whisper no está en marcha.");
        emit_status(&app, status(&spec, "disconnected", None));
        return;
    };
    let _ = model;
    emit_status(&app, status(&spec, "connected", None));

    let (utt_tx, mut utt_rx) = mpsc::channel::<Utterance>(4);

    // Consumidor: inferencia secuencial (el server atiende una petición a la vez)
    let worker = {
        let app = app.clone();
        let state = state.clone();
        let spec = spec.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(u) = utt_rx.recv().await {
                emit_pending(&app, &spec, true);
                let dur = u.duration_ms();
                let wav = encode_wav_in_memory(&u.samples, TARGET_RATE);
                let result = {
                    let _guard = state.whisper_infer.lock().await;
                    client::inference(&state.http, port, wav, &spec.language, inference_timeout(dur)).await
                };
                match result {
                    Ok(res) => {
                        let lang = res.language.clone();
                        let mut i = 0u64;
                        for seg in client::clean_segments(&res) {
                            let start_ms = u.start_ms + seg.start_ms;
                            let end_ms = if seg.end_ms > seg.start_ms {
                                u.start_ms + seg.end_ms
                            } else {
                                u.start_ms + dur
                            };
                            emit_segment(
                                &app,
                                SegmentPayload {
                                    id: segment_id(spec.source, start_ms, end_ms),
                                    session_id: spec.session_id.clone(),
                                    source: spec.source,
                                    speaker: None,
                                    text: seg.text,
                                    start_ms,
                                    end_ms,
                                    received_at: now_ms() + i,
                                    is_final: true,
                                    language: lang.clone(),
                                },
                            );
                            i += 1;
                        }
                        emit_status(&app, status(&spec, "connected", None));
                    }
                    Err(e) => {
                        eprintln!("[whisper] inferencia fallida: {e}");
                        emit_status(&app, status(&spec, "degraded", Some(e)));
                    }
                }
                emit_pending(&app, &spec, false);
            }
        })
    };

    let mut cutter = UtteranceCutter::new(CutterConfig::LIVE);
    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
            chunk = rx.recv() => {
                let Some(c) = chunk else { break };
                if let Some(u) = cutter.push(&c.samples, Some(c.position_ms)) {
                    match utt_tx.try_send(u) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            emit_status(&app, status(&spec, "degraded", Some("El motor local va retrasado; se omitió una frase".into())));
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => break,
                    }
                }
            }
        }
    }
    if let Some(u) = cutter.flush() {
        let _ = utt_tx.send(u).await;
    }
    drop(utt_tx);
    let _ = tokio::time::timeout(Duration::from_secs(45), worker).await;
    emit_pending(&app, &spec, false);
    emit_status(&app, status(&spec, "disconnected", None));
}
