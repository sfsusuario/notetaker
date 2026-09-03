//! Transcripción de audio pregrabado con Deepgram (REST /v1/listen).
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tauri::AppHandle;
use tokio::sync::watch;

use super::{emit_progress, emit_segment, now_ms, segment_id, ProgressPayload, SegmentPayload, SttSpec};
use crate::audio::wav::encode_wav_in_memory;
use crate::audio::TARGET_RATE;
use crate::state::AppState;

#[derive(Deserialize)]
struct DgFileResponse {
    results: Option<DgResults>,
}

#[derive(Deserialize)]
struct DgResults {
    channels: Option<Vec<DgChannel>>,
    utterances: Option<Vec<DgUtterance>>,
}

#[derive(Deserialize)]
struct DgChannel {
    detected_language: Option<String>,
    alternatives: Option<Vec<DgAlt>>,
}

#[derive(Deserialize)]
struct DgAlt {
    transcript: Option<String>,
}

#[derive(Deserialize)]
struct DgUtterance {
    start: f64,
    end: f64,
    transcript: String,
    speaker: Option<u32>,
}

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
    api_key: String,
    spec: SttSpec,
    pcm: Vec<i16>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), String> {
    let wav = encode_wav_in_memory(&pcm, TARGET_RATE);
    let size_mb = wav.len() as f32 / 1_048_576.0;
    emit_progress(
        &app,
        progress(&spec, "uploading", 0.0, Some(format!("Subiendo {size_mb:.1} MB a Deepgram…"))),
    );

    let lang = if spec.language == "auto" {
        "detect_language=true".to_string()
    } else {
        format!("language={}", spec.language)
    };
    let url = format!(
        "https://api.deepgram.com/v1/listen?model=nova-3&smart_format=true&punctuate=true&diarize=true&utterances=true&{lang}"
    );

    let req = state
        .http
        .post(url)
        .header("Authorization", format!("Token {api_key}"))
        .header("Content-Type", "audio/wav")
        .timeout(Duration::from_secs(900))
        .body(wav)
        .send();

    let resp = tokio::select! {
        r = req => r.map_err(|e| format!("Deepgram: {e}"))?,
        _ = cancel.changed() => return Err("Cancelado".into()),
    };

    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("Deepgram: {e}"))?;
    if !status.is_success() {
        return Err(format!("Deepgram {}: {}", status.as_u16(), &text[..text.len().min(300)]));
    }
    emit_progress(&app, progress(&spec, "transcribing", 90.0, None));

    let parsed: DgFileResponse = serde_json::from_str(&text).map_err(|e| format!("Respuesta de Deepgram no válida: {e}"))?;
    let results = parsed.results.ok_or("Deepgram no devolvió resultados")?;
    let language = results
        .channels
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.detected_language.clone());

    let utterances = results.utterances.unwrap_or_default();
    if utterances.is_empty() {
        // Sin utterances (audio vacío o sin voz): un único segmento si hay texto
        let transcript = results
            .channels
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.alternatives.as_ref())
            .and_then(|a| a.first())
            .and_then(|a| a.transcript.clone())
            .unwrap_or_default();
        if !transcript.trim().is_empty() {
            let end_ms = (pcm.len() as u64 * 1000) / TARGET_RATE as u64;
            emit_segment(
                &app,
                SegmentPayload {
                    id: segment_id(spec.source, 0, end_ms),
                    session_id: spec.session_id.clone(),
                    source: spec.source,
                    speaker: None,
                    text: transcript.trim().to_string(),
                    start_ms: 0,
                    end_ms,
                    received_at: now_ms(),
                    is_final: true,
                    language: language.clone(),
                },
            );
        }
        return Ok(());
    }

    let base = now_ms();
    for (i, u) in utterances.iter().enumerate() {
        if u.transcript.trim().is_empty() {
            continue;
        }
        let start_ms = (u.start * 1000.0) as u64;
        let end_ms = (u.end * 1000.0) as u64;
        emit_segment(
            &app,
            SegmentPayload {
                id: segment_id(spec.source, start_ms, end_ms),
                session_id: spec.session_id.clone(),
                source: spec.source,
                speaker: u.speaker.map(|s| s.to_string()),
                text: u.transcript.trim().to_string(),
                start_ms,
                end_ms,
                // received_at creciente para conservar el orden en la UI/DB
                received_at: base + i as u64,
                is_final: true,
                language: language.clone(),
            },
        );
    }
    Ok(())
}
