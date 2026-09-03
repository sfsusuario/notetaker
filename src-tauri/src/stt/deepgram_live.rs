//! Cliente Deepgram en streaming (WebSocket). Adaptado de custom-iv:
//! KeepAlive bajo silencio, backoff [1,2,5,10], ids deterministas. Cambios:
//! diarización (`diarize=true&diarize_model=v1`), audio continuo (sin gating
//! VAD) y `conn_offset_ms` para alinear los timestamps de cada conexión con la
//! línea de tiempo del WAV guardado.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tauri::AppHandle;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use super::{
    emit_error, emit_segment, emit_status, now_ms, segment_id, AudioChunk, EngineId,
    SegmentPayload, StatusPayload, SttSpec,
};

#[derive(Deserialize)]
struct DgResponse {
    #[serde(rename = "type")]
    kind: Option<String>,
    channel: Option<DgChannel>,
    is_final: Option<bool>,
    start: Option<f64>,
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct DgChannel {
    alternatives: Vec<DgAlternative>,
}

#[derive(Deserialize)]
struct DgAlternative {
    transcript: String,
    words: Option<Vec<DgWord>>,
}

#[derive(Deserialize)]
struct DgWord {
    language: Option<String>,
    speaker: Option<u32>,
}

fn dominant<'a>(items: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for i in items {
        *counts.entry(i).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(l, _)| l.to_string())
}

fn dominant_language(alt: &DgAlternative) -> Option<String> {
    let words = alt.words.as_ref()?;
    dominant(words.iter().filter_map(|w| w.language.as_deref()))
}

fn dominant_speaker(alt: &DgAlternative) -> Option<String> {
    let words = alt.words.as_ref()?;
    let owned: Vec<String> = words
        .iter()
        .filter_map(|w| w.speaker.map(|s| s.to_string()))
        .collect();
    dominant(owned.iter().map(|s| s.as_str()))
}

struct Ctx {
    spec: SttSpec,
    /// Posición (ms) en la línea de tiempo del primer chunk enviado en la
    /// conexión actual: Deepgram reinicia sus timestamps a 0 por conexión.
    conn_offset_ms: u64,
}

fn status(ctx: &Ctx, status: &'static str, latency_ms: u64, retry: u32, message: Option<String>) -> StatusPayload {
    StatusPayload {
        session_id: ctx.spec.session_id.clone(),
        source: ctx.spec.source,
        engine: EngineId::Deepgram,
        status,
        latency_ms,
        retry_count: retry,
        message,
    }
}

fn handle_text_message(app: &AppHandle, ctx: &Ctx, raw: &str) {
    let Ok(resp) = serde_json::from_str::<DgResponse>(raw) else {
        eprintln!("[deepgram] mensaje no-JSON: {}", &raw[..raw.len().min(200)]);
        return;
    };
    if resp.kind.as_deref() != Some("Results") {
        eprintln!(
            "[deepgram] {} :: {}",
            resp.kind.as_deref().unwrap_or("?"),
            &raw[..raw.len().min(200)]
        );
        return;
    }
    let Some(channel) = resp.channel else { return };
    let Some(alt) = channel.alternatives.first() else {
        return;
    };
    if alt.transcript.trim().is_empty() {
        return;
    }
    let start_ms = ctx.conn_offset_ms + (resp.start.unwrap_or(0.0) * 1000.0) as u64;
    let end_ms = start_ms + (resp.duration.unwrap_or(0.0) * 1000.0) as u64;
    let is_final = resp.is_final.unwrap_or(false);
    emit_segment(
        app,
        SegmentPayload {
            id: segment_id(ctx.spec.source, start_ms, end_ms),
            session_id: ctx.spec.session_id.clone(),
            source: ctx.spec.source,
            speaker: dominant_speaker(alt),
            text: alt.transcript.trim().to_string(),
            start_ms,
            end_ms,
            received_at: now_ms(),
            is_final,
            language: dominant_language(alt),
        },
    );
}

fn build_url(language: &str, with_diarize_model: bool) -> String {
    let language = if language == "auto" { "multi" } else { language };
    let mut url = format!(
        "wss://api.deepgram.com/v1/listen?model=nova-3&encoding=linear16&sample_rate={}&channels=1&interim_results=true&punctuate=true&smart_format=true&diarize=true&language={}",
        crate::audio::TARGET_RATE, language
    );
    if with_diarize_model {
        url.push_str("&diarize_model=v1");
    }
    url
}

const BACKOFF_SECS: [u64; 4] = [1, 2, 5, 10];
const DEGRADED_LATENCY_MS: u64 = 1500;

async fn drain(app: &AppHandle, ctx: &Ctx, stream: &mut (impl StreamExt<Item = Result<Message, WsError>> + Unpin)) {
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(t) => handle_text_message(app, ctx, &t),
                Message::Close(_) => break,
                _ => {}
            }
        }
    })
    .await;
}

pub async fn run(
    app: AppHandle,
    api_key: String,
    spec: SttSpec,
    mut audio_rx: mpsc::Receiver<AudioChunk>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut ctx = Ctx {
        spec,
        conn_offset_ms: 0,
    };
    let mut retry: u32 = 0;
    let mut with_diarize_model = true;

    'outer: loop {
        if *stop_rx.borrow() {
            break;
        }
        emit_status(&app, status(&ctx, "reconnecting", 0, retry, None));

        let url = build_url(&ctx.spec.language, with_diarize_model);
        let request = match url.clone().into_client_request() {
            Ok(mut req) => match format!("Token {}", api_key).parse() {
                Ok(value) => {
                    req.headers_mut().insert("Authorization", value);
                    req
                }
                Err(_) => {
                    emit_error(&app, "La API key de Deepgram no es válida (revisa que no tenga espacios ni saltos de línea).");
                    emit_status(&app, status(&ctx, "disconnected", 0, retry, None));
                    break;
                }
            },
            Err(_) => {
                emit_error(&app, "No se pudo construir la petición al servicio de transcripción.");
                emit_status(&app, status(&ctx, "disconnected", 0, retry, None));
                break;
            }
        };

        eprintln!("[deepgram] conectando ({}): {url}", ctx.spec.source.as_str());
        match connect_async(request).await {
            Ok((ws, _)) => {
                retry = 0;
                eprintln!("[deepgram] conectado ({})", ctx.spec.source.as_str());
                emit_status(&app, status(&ctx, "connected", 0, 0, None));
                let (mut sink, mut stream) = ws.split();
                let mut last_audio = Instant::now();
                let mut last_send = Instant::now();
                let mut latency_ms: u64 = 0;
                let mut first_chunk = true;
                let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
                heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        biased;

                        _ = stop_rx.changed() => {
                            if *stop_rx.borrow() {
                                let _ = sink.send(Message::Text("{\"type\":\"CloseStream\"}".into())).await;
                                drain(&app, &ctx, &mut stream).await;
                                emit_status(&app, status(&ctx, "disconnected", latency_ms, 0, None));
                                break 'outer;
                            }
                        }

                        chunk = audio_rx.recv() => {
                            match chunk {
                                Some(c) => {
                                    if first_chunk {
                                        ctx.conn_offset_ms = c.position_ms;
                                        first_chunk = false;
                                    }
                                    last_audio = Instant::now();
                                    last_send = Instant::now();
                                    let mut bytes = Vec::with_capacity(c.samples.len() * 2);
                                    for s in &c.samples {
                                        bytes.extend_from_slice(&s.to_le_bytes());
                                    }
                                    if sink.send(Message::Binary(bytes)).await.is_err() {
                                        break; // reconectar
                                    }
                                }
                                None => {
                                    // fin de audio: cerrar limpio y drenar finales
                                    let _ = sink.send(Message::Text("{\"type\":\"CloseStream\"}".into())).await;
                                    drain(&app, &ctx, &mut stream).await;
                                    emit_status(&app, status(&ctx, "disconnected", latency_ms, 0, None));
                                    break 'outer;
                                }
                            }
                        }

                        _ = heartbeat.tick() => {
                            // Deepgram cierra sockets ociosos ~10 s (pausa, silencio)
                            if last_audio.elapsed() > Duration::from_secs(5) {
                                if sink.send(Message::Text("{\"type\":\"KeepAlive\"}".into())).await.is_err() {
                                    break;
                                }
                            }
                            let st = if latency_ms > DEGRADED_LATENCY_MS { "degraded" } else { "connected" };
                            emit_status(&app, status(&ctx, st, latency_ms, 0, None));
                        }

                        msg = stream.next() => {
                            match msg {
                                Some(Ok(Message::Text(t))) => {
                                    latency_ms = last_send.elapsed().as_millis() as u64;
                                    handle_text_message(&app, &ctx, &t);
                                }
                                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_))) => {}
                                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break, // reconectar
                            }
                        }
                    }
                }
            }
            Err(WsError::Http(resp)) => {
                let code = resp.status().as_u16();
                let body = resp
                    .body()
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();
                eprintln!("[deepgram] HTTP {code}: {body}");
                if code == 400 && with_diarize_model {
                    // El parámetro diarize_model no está disponible: reintentar sin él.
                    with_diarize_model = false;
                    continue 'outer;
                }
                if code == 401 || code == 403 {
                    emit_error(&app, "Deepgram rechazó la API key (401). Revísala en Ajustes.");
                    emit_status(&app, status(&ctx, "disconnected", 0, retry, Some("API key inválida".into())));
                    break 'outer;
                }
            }
            Err(e) => {
                eprintln!("[deepgram] conexión fallida: {e}");
            }
        }

        if *stop_rx.borrow() {
            break;
        }
        let delay = BACKOFF_SECS[(retry as usize).min(BACKOFF_SECS.len() - 1)];
        retry += 1;
        emit_status(&app, status(&ctx, "reconnecting", 0, retry, None));
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
        }
    }
}
