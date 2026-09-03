//! Whisper sobre PCM completo (modo archivo / retranscripción): ventanas por
//! silencio de hasta ~28 s, inferencia secuencial y progreso por ventana.
use std::sync::Arc;

use tauri::AppHandle;
use tokio::sync::watch;

use super::{client, live::inference_timeout, server};
use crate::audio::utterance::{CutterConfig, Utterance, UtteranceCutter};
use crate::audio::wav::encode_wav_in_memory;
use crate::audio::{CHUNK_SAMPLES, TARGET_RATE};
use crate::state::AppState;
use crate::stt::{emit_progress, emit_segment, now_ms, segment_id, ProgressPayload, SegmentPayload, SttSpec};

fn progress(spec: &SttSpec, phase: &'static str, percent: f32, message: Option<String>) -> ProgressPayload {
    ProgressPayload {
        session_id: spec.session_id.clone(),
        phase,
        percent,
        message,
    }
}

pub async fn transcribe(
    app: AppHandle,
    state: Arc<AppState>,
    _model: String,
    spec: SttSpec,
    pcm: Vec<i16>,
    cancel: watch::Receiver<bool>,
) -> Result<(), String> {
    let port = server::current_port(&state)
        .await
        .ok_or("El servidor whisper no está en marcha")?;
    let total_ms = (pcm.len() as u64 * 1000) / TARGET_RATE as u64;

    // Trocear todo el PCM en frases primero (rápido) para conocer el total
    let mut cutter = UtteranceCutter::new(CutterConfig::FILE);
    let mut utterances: Vec<Utterance> = Vec::new();
    for chunk in pcm.chunks(CHUNK_SAMPLES) {
        if let Some(u) = cutter.push(chunk, None) {
            utterances.push(u);
        }
    }
    if let Some(u) = cutter.flush() {
        utterances.push(u);
    }
    if utterances.is_empty() {
        emit_progress(&app, progress(&spec, "transcribing", 100.0, Some("No se detectó voz".into())));
        return Ok(());
    }

    let mut order: u64 = 0;
    for (idx, u) in utterances.iter().enumerate() {
        if *cancel.borrow() {
            return Err("Cancelado".into());
        }
        let pct = (u.start_ms as f32 / total_ms.max(1) as f32) * 100.0;
        emit_progress(
            &app,
            progress(
                &spec,
                "transcribing",
                pct,
                Some(format!("Frase {}/{} · {}", idx + 1, utterances.len(), fmt_ms(u.start_ms))),
            ),
        );
        let dur = u.duration_ms();
        let wav = encode_wav_in_memory(&u.samples, TARGET_RATE);
        let result = {
            let _guard = state.whisper_infer.lock().await;
            client::inference(&state.http, port, wav, &spec.language, inference_timeout(dur)).await
        }?;
        let lang = result.language.clone();
        for seg in client::clean_segments(&result) {
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
                    received_at: now_ms() + order,
                    is_final: true,
                    language: lang.clone(),
                },
            );
            order += 1;
        }
    }
    Ok(())
}

fn fmt_ms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}
