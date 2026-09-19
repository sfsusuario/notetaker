//! Pipeline de una fuente en vivo: captura (cpal) → remuestreo a 16 kHz →
//! WAV en disco → chunks de 100 ms al motor + métricas a 10 Hz.
//! Sin motor (modo "solo grabar") solo se escribe el WAV; la sesión se puede
//! transcribir después desde el historial.
//! La línea de tiempo (`position_ms`) avanza solo con audio escrito al WAV,
//! así los timestamps de los segmentos coinciden con la grabación.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, watch};

use crate::audio::capture::{self, AudioFrame};
use crate::audio::downmix::rms_peak;
use crate::audio::resample::Resampler;
use crate::audio::wav::WavSink;
use crate::audio::{CHUNK_SAMPLES, TARGET_RATE};
use crate::state::{AppState, SourceHandles};
use crate::stt::{emit_error, now_ms, AudioChunk, Engine, Source, SttSpec};

/// Umbral de "hay alguien hablando" para la parada automática (≈ −40 dBFS).
/// Deliberadamente más alto que el del VAD (0.003): aquel está elegido para no
/// cortar voz, y el ruido de sala vive en 0.001–0.005, así que con ese umbral
/// el silencio no saltaría nunca. La voz normal está en 0.02–0.15.
const AUTOSTOP_VOICE_RMS: f32 = 0.010;
/// Chunks de 100 ms seguidos por encima del umbral para contar como voz: un
/// teclazo o un clic no deben reiniciar la cuenta de silencio.
const VOICE_RUN_CHUNKS: u32 = 3;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartedSource {
    pub source: Source,
    pub device_label: String,
    pub sample_rate: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MetricsPayload {
    session_id: String,
    source: Source,
    rms: f32,
    peak: f32,
    dropped_frames: u64,
    position_ms: u64,
    device_label: String,
}

pub fn spawn_source_stream(
    app: AppHandle,
    state: Arc<AppState>,
    engine: Option<Engine>,
    spec: SttSpec,
    device_id: Option<String>,
    wav_path: PathBuf,
    paused: Arc<AtomicBool>,
    // `last_voice` lo comparten todas las fuentes de la sesión: basta con que
    // UNA oiga voz para que la sesión no se considere en silencio.
    last_voice: Arc<AtomicU64>,
) -> Result<(SourceHandles, StartedSource), String> {
    let (frame_tx, mut frame_rx) = mpsc::channel::<AudioFrame>(64);
    let (err_tx, mut err_rx) = mpsc::channel::<String>(8);
    let (stop_tx, stop_rx) = watch::channel(false);
    let (chunk_tx, chunk_rx) = mpsc::channel::<AudioChunk>(64);

    let capture_source = match spec.source {
        Source::Mic => "mic",
        Source::System => "system",
        Source::File => return Err("La fuente 'file' no se captura en vivo".into()),
    };
    let capture_handle = capture::start(capture_source.into(), device_id, frame_tx, err_tx)?;
    let sample_rate = capture_handle.sample_rate;
    let device_label = capture_handle.device_label.clone();
    let dropped = capture_handle.dropped_frames.clone();

    // El dispositivo guardado había desaparecido: avisar sin bloquear la grabación.
    if let Some(note) = &capture_handle.fallback_note {
        let _ = app.emit("audio://warning", serde_json::json!({ "message": note }));
    }

    let wav = WavSink::open(&wav_path)?;

    // Errores de audio → banner
    {
        let app = app.clone();
        let label = match spec.source {
            Source::Mic => "Micrófono",
            _ => "Audio del sistema",
        };
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = err_rx.recv().await {
                emit_error(&app, format!("{label}: {msg}"));
            }
        });
    }

    // Pipeline
    let pipeline = {
        let app = app.clone();
        let spec = spec.clone();
        let device_label = device_label.clone();
        let paused = paused.clone();
        let mut stop_rx = stop_rx.clone();
        tauri::async_runtime::spawn(async move {
            let mut rs = Resampler::new(sample_rate, TARGET_RATE);
            let mut buf: Vec<i16> = Vec::with_capacity(CHUNK_SAMPLES * 4);
            let mut position_ms: u64 = 0;
            let mut voice_run: u32 = 0;
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() { break; }
                    }
                    frame = frame_rx.recv() => {
                        let Some(frame) = frame else { break };
                        if paused.load(Ordering::Relaxed) {
                            buf.clear();
                            continue;
                        }
                        buf.extend(rs.process(&frame));
                        while buf.len() >= CHUNK_SAMPLES {
                            let chunk: Vec<i16> = buf.drain(..CHUNK_SAMPLES).collect();
                            wav.write(&chunk);
                            let (rms, peak) = rms_peak(&chunk);
                            // Marca de actividad para la parada por silencio.
                            if rms >= AUTOSTOP_VOICE_RMS {
                                voice_run += 1;
                            } else {
                                voice_run = 0;
                            }
                            if voice_run >= VOICE_RUN_CHUNKS {
                                last_voice.store(now_ms(), Ordering::Relaxed);
                            }
                            let _ = app.emit(
                                "audio://metrics",
                                MetricsPayload {
                                    session_id: spec.session_id.clone(),
                                    source: spec.source,
                                    rms,
                                    peak,
                                    dropped_frames: dropped.load(Ordering::Relaxed),
                                    position_ms,
                                    device_label: device_label.clone(),
                                },
                            );
                            // try_send: si el motor va por detrás, soltar audio antes que bloquear
                            let _ = chunk_tx.try_send(AudioChunk { samples: chunk, position_ms });
                            position_ms += 100;
                        }
                    }
                }
            }
            wav.finalize();
            // Al salir se dropea chunk_tx → el motor drena y cierra
        })
    };

    // Motor (ausente en modo "solo grabar")
    let engine_task = match engine {
        Some(engine) => {
            let app = app.clone();
            let spec = spec.clone();
            let stop_rx = stop_rx.clone();
            Some(tauri::async_runtime::spawn(async move {
                engine.run_live(app, state, spec, chunk_rx, stop_rx).await;
            }))
        }
        None => {
            // Nadie consume los chunks: try_send falla y se descartan.
            drop(chunk_rx);
            drop(state);
            None
        }
    };

    Ok((
        SourceHandles {
            stop_tx,
            capture: capture_handle,
            paused,
            pipeline: Some(pipeline),
            engine: engine_task,
            wav_path,
        },
        StartedSource {
            source: spec.source,
            device_label,
            sample_rate,
        },
    ))
}
