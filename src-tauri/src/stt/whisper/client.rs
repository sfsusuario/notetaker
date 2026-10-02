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
    vad: bool,
    timeout: Duration,
) -> Result<WhisperResult, String> {
    let part = Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let mut form = Form::new()
        .part("file", part)
        .text("response_format", "verbose_json")
        .text("language", language.to_string())
        .text("temperature", "0.0")
        .text("temperature_inc", "0.2")
        .text("no_timestamps", "false")
        // Sin esto el servidor añade `language_probabilities` a verbose_json
        // con OTRA pasada completa del encoder por petición (4–5 s por ventana
        // de 27 s en la Arc 140V). La app no usa ese dato.
        .text("no_language_probabilities", "true");
    if vad {
        // Silero VAD dentro de whisper-server: el ruido (teclas, golpes,
        // música) no llega al modelo, que sobre ruido inventa frases como
        // "Thank you." o "Спасибо." (23 de 41 frases del micrófono en una
        // llamada de prueba).
        form = form.text("vad", "true");
    }
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
    // Vistas en una llamada de prueba sobre ruido del micrófono
    "продолжение следует",
    "субтитры",
    "untertitel",
    "sous-titres",
];

pub fn is_hallucination(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.is_empty() {
        return true;
    }
    if t.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    // Anotaciones de sonido, no habla: "*sad music*", "[Música]", "(risas)", "♪ … ♪"
    let wrapped = |open: char, close: char| t.starts_with(open) && t.ends_with(close);
    if wrapped('*', '*') || wrapped('[', ']') || wrapped('(', ')') || t.starts_with('♪') {
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
    // whisper corta segmentos a mitad de palabra ("caball" + "ero"): el trozo
    // que continúa una palabra llega sin espacio inicial y se une al anterior
    // (o se descarta con él), para no partir burbujas ni el texto del chat.
    let mut prev_kept = false;
    for (i, s) in segs.iter().enumerate() {
        let continues_word = i > 0 && !s.text.is_empty() && !s.text.starts_with(char::is_whitespace);
        if continues_word {
            if prev_kept {
                if let Some(last) = out.last_mut() {
                    last.text.push_str(s.text.trim_end());
                    last.end_ms = last.end_ms.max((s.end.max(0.0) * 1000.0) as u64);
                }
            }
            continue;
        }
        prev_kept = false;
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
        prev_kept = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, text: &str) -> WhisperSeg {
        WhisperSeg { start, end, text: text.into(), no_speech_prob: None, avg_logprob: None }
    }

    #[test]
    fn joins_segments_split_mid_word() {
        // Salida real de large-v3-turbo sobre el capítulo de prueba
        let r = WhisperResult {
            segments: Some(vec![
                seg(14.24, 24.62, " ii la degollación de los inocentes tomás grangrith caball"),
                seg(24.62, 24.62, "ero"),
                seg(24.62, 28.57, " fue el hombre de las realidades"),
            ]),
            ..Default::default()
        };
        let out = clean_segments(&r);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "ii la degollación de los inocentes tomás grangrith caballero");
        assert_eq!((out[0].start_ms, out[0].end_ms), (14_240, 24_620));
        assert_eq!(out[1].text, "fue el hombre de las realidades");
    }

    #[test]
    fn drops_sound_annotations_and_foreign_noise_phrases() {
        for t in ["*sad music*", "*thud*", "[Música]", "(risas)", "♪ la la ♪", "Продолжение следует..."] {
            assert!(is_hallucination(t), "{t}");
        }
        for t in ["1, 2, 3, escuchando y probando sonido.", "(Bueno), sigamos", "Gracias, Edwin"] {
            assert!(!is_hallucination(t), "{t}");
        }
    }

    #[test]
    fn continuation_of_a_dropped_segment_is_dropped() {
        let r = WhisperResult {
            segments: Some(vec![seg(0.0, 2.0, " Gracias por ver el vid"), seg(2.0, 3.0, "eo"), seg(3.0, 5.0, " hola")]),
            ..Default::default()
        };
        let out = clean_segments(&r);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hola");
    }
}
