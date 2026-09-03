//! Cliente HTTP de whisper-server: POST /inference con el WAV en memoria.
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

#[derive(Deserialize, Debug, Default)]
pub struct WhisperResult {
    pub text: Option<String>,
    pub language: Option<String>,
    pub segments: Option<Vec<WhisperSeg>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WhisperSeg {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub no_speech_prob: Option<f64>,
    pub avg_logprob: Option<f64>,
}

/// Segmento limpio: (start_ms, end_ms, texto) relativo al inicio del WAV enviado.
pub struct CleanSeg {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub async fn inference(
    client: &reqwest::Client,
    port: u16,
    wav: Vec<u8>,
    language: &str,
    timeout: Duration,
) -> Result<WhisperResult, String> {
    let part = Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = Form::new()
        .part("file", part)
        .text("response_format", "verbose_json")
        .text("language", language.to_string())
        .text("temperature", "0.0")
        .text("temperature_inc", "0.2")
        .text("no_timestamps", "false");
    let resp = client
        .post(format!("http://127.0.0.1:{port}/inference"))
        .multipart(form)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("whisper-server: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("whisper-server: {e}"))?;
    if !status.is_success() {
        return Err(format!("whisper-server {}: {}", status.as_u16(), &body[..body.len().min(300)]));
    }
    serde_json::from_str::<WhisperResult>(&body)
        .map_err(|e| format!("Respuesta de whisper-server no válida: {e} — {}", &body[..body.len().min(200)]))
}

/// Frases típicas que whisper "alucina" sobre silencio o música.
const BLACKLIST: &[&str] = &[
    "[blank_audio]",
    "[música]",
    "[music]",
    "[silencio]",
    "(música)",
    "(music)",
    "subtítulos realizados por",
    "subtitulos realizados por",
    "subtítulos por",
    "gracias por ver",
    "thanks for watching",
    "thank you for watching",
    "amara.org",
    "www.",
];

pub fn is_hallucination(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return true;
    }
    if t.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    BLACKLIST.iter().any(|b| t.contains(b))
}

/// Filtra segmentos vacíos/alucinados y convierte tiempos a ms.
pub fn clean_segments(result: &WhisperResult) -> Vec<CleanSeg> {
    let mut out = Vec::new();
    let Some(segs) = &result.segments else {
        if let Some(t) = &result.text {
            if !is_hallucination(t) {
                out.push(CleanSeg {
                    start_ms: 0,
                    end_ms: 0,
                    text: t.trim().to_string(),
                });
            }
        }
        return out;
    };
    for s in segs {
        if s.no_speech_prob.unwrap_or(0.0) > 0.6 && s.avg_logprob.unwrap_or(0.0) < -1.0 {
            continue;
        }
        if is_hallucination(&s.text) {
            continue;
        }
        out.push(CleanSeg {
            start_ms: (s.start.max(0.0) * 1000.0) as u64,
            end_ms: (s.end.max(0.0) * 1000.0) as u64,
            text: s.text.trim().to_string(),
        });
    }
    out
}
