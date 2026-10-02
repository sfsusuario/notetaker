//! Whisper sobre PCM completo (modo archivo / retranscripción): se detectan
//! las frases por silencio y se agrupan en ventanas contiguas de hasta ~28 s,
//! con inferencia secuencial y progreso por ventana.
use std::sync::Arc;
use std::time::Instant;

use tauri::AppHandle;
use tokio::sync::watch;

use super::{client, live::inference_timeout, server};
use crate::audio::utterance::{CutterConfig, UtteranceCutter};
use crate::audio::wav::encode_wav_in_memory;
use crate::audio::{CHUNK_SAMPLES, TARGET_RATE};
use crate::state::AppState;
use crate::stt::{emit_progress, emit_segment, now_ms, segment_id, ProgressPayload, SegmentPayload, SttSpec};

/// Ventana máxima por petición: whisper procesa bloques de 30 s.
const WINDOW_MAX_MS: u64 = 28_000;
/// Un silencio más largo que esto abre ventana nueva: el silencio largo es
/// donde whisper inventa texto, y no aporta contexto.
const WINDOW_MAX_GAP_MS: u64 = 3_000;

fn progress(spec: &SttSpec, phase: &'static str, percent: f32, message: Option<String>) -> ProgressPayload {
    ProgressPayload {
        session_id: spec.session_id.clone(),
        phase,
        percent,
        message,
    }
}

/// Agrupa frases `(inicio, fin)` en ms en ventanas contiguas `(inicio, fin)`.
///
/// El encoder de whisper procesa SIEMPRE 30 s por petición (rellena con
/// silencio), así que una frase de 3 s cuesta lo mismo que una ventana de
/// 28 s. Cortando en cada pausa de 600 ms, una conversación normal genera
/// 4–6 veces más peticiones de las necesarias.
pub fn pack_windows(utterances: &[(u64, u64)], max_ms: u64, max_gap_ms: u64) -> Vec<(u64, u64)> {
    let mut out: Vec<(u64, u64)> = Vec::new();
    for &(start, end) in utterances {
        match out.last_mut() {
            Some((w_start, w_end))
                if start.saturating_sub(*w_end) <= max_gap_ms && end.saturating_sub(*w_start) <= max_ms =>
            {
                *w_end = (*w_end).max(end);
            }
            _ => out.push((start, end)),
        }
    }
    out
}

pub async fn transcribe(
    app: AppHandle,
    state: Arc<AppState>,
    _model: String,
    spec: SttSpec,
    pcm: Vec<i16>,
    cancel: watch::Receiver<bool>,
) -> Result<(), String> {
    let (port, vad) = server::current(&state)
        .await
        .ok_or("El servidor whisper no está en marcha")?;
    let total_ms = (pcm.len() as u64 * 1000) / TARGET_RATE as u64;

    // Trocear todo el PCM en frases primero (rápido) para conocer el total
    let mut cutter = UtteranceCutter::new(CutterConfig::FILE);
    let mut utterances: Vec<(u64, u64)> = Vec::new();
    for chunk in pcm.chunks(CHUNK_SAMPLES) {
        if let Some(u) = cutter.push(chunk, None) {
            utterances.push((u.start_ms, u.start_ms + u.duration_ms()));
        }
    }
    if let Some(u) = cutter.flush() {
        utterances.push((u.start_ms, u.start_ms + u.duration_ms()));
    }
    if utterances.is_empty() {
        emit_progress(&app, progress(&spec, "transcribing", 100.0, Some("No se detectó voz".into())));
        return Ok(());
    }
    let windows = pack_windows(&utterances, WINDOW_MAX_MS, WINDOW_MAX_GAP_MS);
    eprintln!(
        "[whisper] {} frases → {} ventanas ({} s de audio)",
        utterances.len(),
        windows.len(),
        total_ms / 1000
    );

    let samples_per_ms = TARGET_RATE as u64 / 1000;
    // Con "auto", whisper detecta el idioma en cada petición con una pasada
    // extra del encoder. Se detecta en la primera ventana (≥ unos segundos de
    // voz, fiable) y se reutiliza en el resto del archivo.
    let mut language = spec.language.clone();
    let started = Instant::now();
    let mut audio_done_ms: u64 = 0;
    let mut order: u64 = 0;
    for (idx, &(w_start, w_end)) in windows.iter().enumerate() {
        if *cancel.borrow() {
            return Err("Cancelado".into());
        }
        let pct = (w_start as f32 / total_ms.max(1) as f32) * 100.0;
        let speed = if audio_done_ms > 0 {
            let x = audio_done_ms as f64 / started.elapsed().as_millis().max(1) as f64;
            format!(" · {x:.1}x tiempo real")
        } else {
            String::new()
        };
        emit_progress(
            &app,
            progress(
                &spec,
                "transcribing",
                pct,
                Some(format!("Ventana {}/{} · {}{speed}", idx + 1, windows.len(), fmt_ms(w_start))),
            ),
        );
        let from = ((w_start * samples_per_ms) as usize).min(pcm.len());
        let to = ((w_end * samples_per_ms) as usize).clamp(from, pcm.len());
        let dur = w_end - w_start;
        let wav = encode_wav_in_memory(&pcm[from..to], TARGET_RATE);
        let result = {
            let _guard = state.whisper_infer.lock().await;
            client::inference(&state.http, port, wav, &language, vad, inference_timeout(dur)).await
        }?;
        audio_done_ms += dur;
        let lang = result.language.clone();
        if language == "auto" {
            if let Some(detected) = lang.as_deref().filter(|l| !l.is_empty()) {
                // whisper-server devuelve el nombre completo ("spanish") y lo
                // acepta igual que el código corto.
                language = detected.to_string();
            }
        }
        for seg in client::clean_segments(&result) {
            let start_ms = w_start + seg.start_ms;
            let end_ms = if seg.end_ms > seg.start_ms {
                (w_start + seg.end_ms).min(w_end)
            } else {
                w_end
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
    let secs = started.elapsed().as_secs_f64();
    eprintln!(
        "[whisper] {} s de voz en {secs:.1} s ({:.1}x tiempo real)",
        audio_done_ms / 1000,
        audio_done_ms as f64 / 1000.0 / secs.max(0.001)
    );
    Ok(())
}

fn fmt_ms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_close_utterances_into_one_window() {
        let utts = [(0, 3_000), (3_700, 8_000), (8_900, 12_000)];
        assert_eq!(pack_windows(&utts, 28_000, 3_000), vec![(0, 12_000)]);
    }

    #[test]
    fn splits_at_max_window_length() {
        let utts = [(0, 10_000), (10_500, 20_000), (20_500, 29_000)];
        assert_eq!(pack_windows(&utts, 28_000, 3_000), vec![(0, 20_000), (20_500, 29_000)]);
    }

    #[test]
    fn long_silence_starts_new_window() {
        let utts = [(0, 4_000), (9_000, 12_000)];
        assert_eq!(pack_windows(&utts, 28_000, 3_000), vec![(0, 4_000), (9_000, 12_000)]);
    }

    #[test]
    fn keeps_timeline_positions() {
        let utts = [(61_000, 64_000), (64_500, 70_000)];
        assert_eq!(pack_windows(&utts, 28_000, 3_000), vec![(61_000, 70_000)]);
        assert!(pack_windows(&[], 28_000, 3_000).is_empty());
    }
}
