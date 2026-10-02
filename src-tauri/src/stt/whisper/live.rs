//! Whisper en vivo (pseudo tiempo real): corta frases por silencio, las envía
//! a whisper-server y emite `stt://final` al terminar cada una. Mientras la
//! persona sigue hablando, en "Otros" se transcribe la frase en curso cada
//! ~2 s (si el servidor está libre) y se muestra como texto provisional.
//!
//! Medido reproduciendo una llamada real de 8 min (mic + sistema) contra el
//! servidor en GPU: el primer texto de cada frase de "Otros" pasó de 10.4 s a
//! 4.2 s de mediana con el texto provisional; el VAD y fijar el idioma por
//! pista bajaron el trabajo del servidor de 276 s a 82 s.
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::sync::{mpsc, watch};

use super::{client, server};
use crate::audio::utterance::{CutterConfig, Utterance, UtteranceCutter};
use crate::audio::wav::encode_wav_in_memory;
use crate::audio::TARGET_RATE;
use crate::state::AppState;
use crate::stt::{
    emit_error, emit_segment, emit_status, now_ms, segment_id, AudioChunk, EngineId,
    SegmentPayload, Source, StatusPayload, SttSpec,
};

/// Frecuencia del texto provisional (1.5 s apenas mejoraba: 4.0 s frente a 4.2 s
/// de mediana, con un 10 % más de carga).
const PARTIAL_EVERY: Duration = Duration::from_millis(2_000);
/// Audio mínimo de la frase en curso para pedir texto provisional.
const PARTIAL_MIN_SAMPLES: usize = TARGET_RATE as usize * 2;
/// Frases más cortas no sirven para detectar el idioma con fiabilidad.
const LANG_MIN_MS: u64 = 3_000;
/// Con el idioma fijado, 1 de cada N frases largas vuelve a detectarlo por si
/// cambia (reuniones bilingües).
const LANG_PROBE_EVERY: u32 = 4;
/// Parecido mínimo (coseno) para asignar un segmento a un hablante conocido.
/// Calibrado con 3 lectores reales en trozos de ~3 s: 0.55 encontró los 3 y
/// acertó el 91–92.5 % (el techo con esa granularidad era 92–93.5 %); 0.60
/// ya partía a un lector en 5–6 grupos.
const SPEAKER_THRESHOLD: f32 = 0.55;
/// Segmentos cortos (< 2 s) solo se asignan a hablantes existentes.
const SPEAKER_SHORT_THRESHOLD: f32 = 0.40;
/// Tramos más cortos no tienen huella fiable: heredan el hablante vecino.
const SPEAKER_MIN_SAMPLES: usize = TARGET_RATE as usize / 2;

/// Idioma que se pide a whisper para una pista. Con "auto", detectar en cada
/// frase cuesta una pasada extra del encoder (+54 % de trabajo del servidor);
/// se fija cuando dos frases con voz real coinciden. Es por pista, no por
/// sesión: en la llamada de prueba la reunión era en inglés y la voz del
/// micrófono en español, y forzar un idioma sobre el otro inventa traducciones.
#[derive(Debug, Default)]
pub struct LangState {
    /// Elegido por el usuario (no "auto"): se usa siempre.
    fixed: Option<String>,
    locked: Option<String>,
    last_detected: Option<String>,
    since_probe: u32,
}

impl LangState {
    pub fn new(setting: &str) -> Self {
        Self {
            fixed: (setting != "auto" && !setting.is_empty()).then(|| setting.to_string()),
            ..Default::default()
        }
    }

    /// Idioma para una frase final de `dur_ms`: (idioma, es una detección).
    pub fn for_final(&mut self, dur_ms: u64) -> (String, bool) {
        if let Some(f) = &self.fixed {
            return (f.clone(), false);
        }
        match &self.locked {
            None => ("auto".into(), true),
            Some(l) if dur_ms >= LANG_MIN_MS => {
                self.since_probe += 1;
                if self.since_probe >= LANG_PROBE_EVERY {
                    self.since_probe = 0;
                    ("auto".into(), true)
                } else {
                    (l.clone(), false)
                }
            }
            Some(l) => (l.clone(), false),
        }
    }

    /// Idioma para texto provisional (nunca detecta: es descartable).
    pub fn current(&self) -> String {
        self.fixed.clone().or_else(|| self.locked.clone()).unwrap_or_else(|| "auto".into())
    }

    /// Registra lo detectado en una frase pedida con "auto".
    pub fn observe(&mut self, dur_ms: u64, detected: Option<&str>, has_text: bool) {
        let Some(d) = detected.filter(|d| !d.is_empty()) else { return };
        if dur_ms < LANG_MIN_MS || !has_text {
            return;
        }
        if self.last_detected.as_deref() == Some(d) && self.locked.as_deref() != Some(d) {
            eprintln!("[whisper] idioma fijado: {d}");
            self.locked = Some(d.to_string());
        }
        self.last_detected = Some(d.to_string());
    }
}

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

/// Parcial de la pista: "…" (escribiendo), texto provisional, o vacío (quitar).
fn emit_partial(app: &AppHandle, spec: &SttSpec, text: &str, start_ms: u64) {
    emit_segment(
        app,
        SegmentPayload {
            id: format!("{}-pending", spec.source.as_str()),
            session_id: spec.session_id.clone(),
            source: spec.source,
            speaker: None,
            text: text.to_string(),
            start_ms,
            end_ms: start_ms,
            received_at: now_ms(),
            is_final: false,
            language: None,
        },
    );
}

pub fn inference_timeout(duration_ms: u64) -> Duration {
    Duration::from_secs(60 + (duration_ms / 1000) * 3)
}

/// Estado compartido entre el bucle de audio, el trabajador de finales y las
/// peticiones de texto provisional de una pista.
struct Shared {
    /// Frases cerradas pendientes de su texto final (en cola o en curso).
    pending: AtomicUsize,
    /// Sube con cada frase cerrada: invalida textos provisionales viejos.
    generation: AtomicU64,
    /// Hay texto provisional en pantalla (no se pisa con "…").
    partial_shown: AtomicBool,
    partial_in_flight: AtomicBool,
    lang: Mutex<LangState>,
    /// Hablantes provisionales de la pista (None = sin etiquetas en vivo).
    speakers: Option<(crate::speaker::SharedEmbedder, Mutex<crate::speaker::OnlineSpeakers>)>,
}

pub async fn run(
    app: AppHandle,
    state: Arc<AppState>,
    model: String,
    partials: bool,
    speakers: bool,
    spec: SttSpec,
    mut rx: mpsc::Receiver<AudioChunk>,
    mut stop_rx: watch::Receiver<bool>,
) {
    let Some((port, vad)) = server::current(&state).await else {
        emit_error(&app, "El servidor whisper no está en marcha.");
        emit_status(&app, status(&spec, "disconnected", None));
        return;
    };
    let _ = model;
    emit_status(&app, status(&spec, "connected", None));

    let shared = Arc::new(Shared {
        pending: AtomicUsize::new(0),
        generation: AtomicU64::new(0),
        partial_shown: AtomicBool::new(false),
        partial_in_flight: AtomicBool::new(false),
        lang: Mutex::new(LangState::new(&spec.language)),
        speakers: if speakers && spec.source == Source::System {
            let (app2, state2) = (app.clone(), state.clone());
            tokio::task::spawn_blocking(move || crate::speaker::shared_embedder(&app2, &state2))
                .await
                .ok()
                .flatten()
                .map(|e| (e, Mutex::new(crate::speaker::OnlineSpeakers::new(SPEAKER_THRESHOLD, SPEAKER_SHORT_THRESHOLD))))
        } else {
            None
        },
    });
    // Sin límite: antes, con 4 frases en cola se descartaban las siguientes.
    let (utt_tx, mut utt_rx) = mpsc::unbounded_channel::<Utterance>();

    // Consumidor: inferencia secuencial (el server atiende una petición a la vez)
    let worker = {
        let (app, state, spec, shared) = (app.clone(), state.clone(), spec.clone(), shared.clone());
        tauri::async_runtime::spawn(async move {
            while let Some(u) = utt_rx.recv().await {
                if !shared.partial_shown.load(Ordering::SeqCst) {
                    emit_partial(&app, &spec, "…", u.start_ms);
                }
                let dur = u.duration_ms();
                let (language, detecting) = shared.lang.lock().unwrap().for_final(dur);
                let wav = encode_wav_in_memory(&u.samples, TARGET_RATE);
                let result = {
                    let _guard = state.whisper_infer.lock().await;
                    client::inference(&state.http, port, wav, &language, vad, inference_timeout(dur)).await
                };
                match result {
                    Ok(res) => {
                        let lang = res.language.clone();
                        let segs = client::clean_segments(&res);
                        if detecting {
                            shared.lang.lock().unwrap().observe(dur, lang.as_deref(), !segs.is_empty());
                        }
                        let labels = label_speakers(&shared, &u, &segs).await;
                        for (i, (seg, speaker)) in merge_runs(segs, labels).into_iter().enumerate() {
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
                                    speaker: speaker.map(|s| s.to_string()),
                                    text: seg.text,
                                    start_ms,
                                    end_ms,
                                    received_at: now_ms() + i as u64,
                                    is_final: true,
                                    language: lang.clone(),
                                },
                            );
                        }
                        emit_status(&app, status(&spec, "connected", None));
                    }
                    Err(e) => {
                        eprintln!("[whisper] inferencia fallida: {e}");
                        emit_status(&app, status(&spec, "degraded", Some(e)));
                    }
                }
                if shared.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                    shared.partial_shown.store(false, Ordering::SeqCst);
                    emit_partial(&app, &spec, "", 0);
                }
            }
        })
    };

    // Texto provisional solo para "Otros": la voz propia no hace falta leerla
    // al instante, y con eco duplicaría el trabajo del servidor.
    let partials = partials && spec.source == Source::System;
    let mut last_partial = Instant::now();
    let mut cutter = UtteranceCutter::new(CutterConfig::LIVE);
    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
            chunk = rx.recv() => {
                let Some(c) = chunk else { break };
                if let Some(u) = cutter.push(&c.samples, Some(c.position_ms)) {
                    shared.generation.fetch_add(1, Ordering::SeqCst);
                    let backlog = shared.pending.fetch_add(1, Ordering::SeqCst) + 1;
                    if backlog >= 4 {
                        emit_status(&app, status(&spec, "degraded", Some(format!("El motor local va {backlog} frases por detrás"))));
                    }
                    if utt_tx.send(u).is_err() { break; }
                } else if partials
                    && last_partial.elapsed() >= PARTIAL_EVERY
                    && shared.pending.load(Ordering::SeqCst) == 0
                    && !shared.partial_in_flight.load(Ordering::SeqCst)
                {
                    if let Some((start, buf)) = cutter.pending() {
                        if buf.len() >= PARTIAL_MIN_SAMPLES {
                            last_partial = Instant::now();
                            spawn_partial(&app, &state, &spec, &shared, port, vad, start, buf.to_vec());
                        }
                    }
                }
            }
        }
    }
    if let Some(u) = cutter.flush() {
        shared.generation.fetch_add(1, Ordering::SeqCst);
        shared.pending.fetch_add(1, Ordering::SeqCst);
        let _ = utt_tx.send(u);
    }
    drop(utt_tx);
    let _ = tokio::time::timeout(Duration::from_secs(45), worker).await;
    emit_partial(&app, &spec, "", 0);
    emit_status(&app, status(&spec, "disconnected", None));
}

/// Hablante provisional de cada segmento, con la huella de su propio tramo de
/// audio (no la de la frase entera: una frase de hasta 15 s puede mezclar
/// voces, y así el acierto bajaba del 91 % al 74 %).
async fn label_speakers(shared: &Arc<Shared>, u: &Utterance, segs: &[client::CleanSeg]) -> Vec<Option<u32>> {
    if shared.speakers.is_none() || segs.is_empty() {
        return vec![None; segs.len()];
    }
    let spans: Vec<(Vec<i16>, u64)> = segs
        .iter()
        .map(|s| {
            let end_ms = if s.end_ms > s.start_ms { s.end_ms } else { u.duration_ms() };
            let a = ((s.start_ms * TARGET_RATE as u64 / 1000) as usize).min(u.samples.len());
            let b = ((end_ms * TARGET_RATE as u64 / 1000) as usize).clamp(a, u.samples.len());
            (u.samples[a..b].to_vec(), end_ms.saturating_sub(s.start_ms))
        })
        .collect();
    let shared = shared.clone();
    let mut labels = tokio::task::spawn_blocking(move || {
        let (embedder, speakers) = shared.speakers.as_ref().unwrap();
        let embedder = embedder.lock().unwrap();
        let mut speakers = speakers.lock().unwrap();
        spans
            .iter()
            .map(|(pcm, ms)| {
                if pcm.len() < SPEAKER_MIN_SAMPLES {
                    return None;
                }
                embedder.embed(pcm).and_then(|e| speakers.assign(&e, *ms))
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    labels.resize(segs.len(), None);
    // Sin etiqueta propia: la del vecino (anterior o siguiente) de la misma frase
    for i in 0..labels.len() {
        if labels[i].is_none() {
            labels[i] = labels[..i].iter().rev().flatten().next().or_else(|| labels[i + 1..].iter().flatten().next()).copied();
        }
    }
    labels
}

/// Une segmentos seguidos del mismo hablante dentro de una frase: whisper
/// devuelve trozos de 1–3 palabras ("MySQL,", "back up.") y cada uno era una
/// burbuja. El hablante se calcula antes, con cada trozo por separado.
fn merge_runs(segs: Vec<client::CleanSeg>, labels: Vec<Option<u32>>) -> Vec<(client::CleanSeg, Option<u32>)> {
    let mut out: Vec<(client::CleanSeg, Option<u32>)> = Vec::new();
    for (seg, label) in segs.into_iter().zip(labels) {
        if let Some((last, last_label)) = out.last_mut() {
            if *last_label == label && seg.start_ms.saturating_sub(last.end_ms) <= 1_000 {
                last.text.push(' ');
                last.text.push_str(&seg.text);
                last.end_ms = last.end_ms.max(seg.end_ms);
                continue;
            }
        }
        out.push((seg, label));
    }
    out
}

/// Transcribe la frase en curso y la muestra como texto provisional. Cede ante
/// los finales: si el servidor está ocupado no espera, y si la frase se cerró
/// mientras tanto descarta el resultado.
#[allow(clippy::too_many_arguments)]
fn spawn_partial(
    app: &AppHandle,
    state: &Arc<AppState>,
    spec: &SttSpec,
    shared: &Arc<Shared>,
    port: u16,
    vad: bool,
    start_ms: u64,
    samples: Vec<i16>,
) {
    let (app, state, spec, shared) = (app.clone(), state.clone(), spec.clone(), shared.clone());
    let generation = shared.generation.load(Ordering::SeqCst);
    shared.partial_in_flight.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        if let Ok(_guard) = state.whisper_infer.try_lock() {
            let language = shared.lang.lock().unwrap().current();
            let dur = (samples.len() as u64 * 1000) / TARGET_RATE as u64;
            let wav = encode_wav_in_memory(&samples, TARGET_RATE);
            if let Ok(res) = client::inference(&state.http, port, wav, &language, vad, inference_timeout(dur)).await {
                let text: Vec<String> = client::clean_segments(&res).into_iter().map(|s| s.text).collect();
                let still_open = shared.generation.load(Ordering::SeqCst) == generation
                    && shared.pending.load(Ordering::SeqCst) == 0;
                if still_open && !text.is_empty() {
                    shared.partial_shown.store(true, Ordering::SeqCst);
                    emit_partial(&app, &spec, &text.join(" "), start_ms);
                }
            }
        }
        shared.partial_in_flight.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn seg(s: u64, e: u64, t: &str) -> client::CleanSeg {
        client::CleanSeg { start_ms: s, end_ms: e, text: t.into() }
    }

    #[test]
    fn joins_short_pieces_of_the_same_speaker() {
        let segs = vec![seg(0, 1_500, "there's only 68 installations."), seg(1_600, 2_400, "MySQL,"), seg(2_500, 3_000, "I don't know."), seg(3_100, 5_000, "Yeah, we could")];
        let out = merge_runs(segs, vec![Some(2), Some(2), Some(2), Some(0)]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0.text, "there's only 68 installations. MySQL, I don't know.");
        assert_eq!((out[0].0.start_ms, out[0].0.end_ms, out[0].1), (0, 3_000, Some(2)));
        assert_eq!(out[1].1, Some(0));
    }

    #[test]
    fn a_long_pause_keeps_separate_bubbles() {
        let out = merge_runs(vec![seg(0, 1_000, "uno"), seg(4_000, 5_000, "dos")], vec![None, None]);
        assert_eq!(out.len(), 2);
    }
}

#[cfg(test)]
mod lang_tests {
    use super::*;

    #[test]
    fn user_choice_is_always_used() {
        let mut l = LangState::new("es");
        assert_eq!(l.for_final(10_000), ("es".into(), false));
        assert_eq!(l.current(), "es");
    }

    #[test]
    fn locks_after_two_agreeing_real_detections() {
        let mut l = LangState::new("auto");
        assert_eq!(l.for_final(5_000), ("auto".into(), true));
        l.observe(1_500, Some("english"), true); // demasiado corta: no cuenta
        l.observe(5_000, Some("russian"), false); // sin texto (ruido): no cuenta
        l.observe(5_000, Some("english"), true);
        assert_eq!(l.current(), "auto");
        l.observe(6_000, Some("english"), true);
        assert_eq!(l.current(), "english");
        assert_eq!(l.for_final(5_000), ("english".into(), false));
    }

    #[test]
    fn probes_periodically_and_follows_a_language_change() {
        let mut l = LangState::new("auto");
        l.observe(5_000, Some("english"), true);
        l.observe(5_000, Some("english"), true);
        let asked: Vec<bool> = (0..8).map(|_| l.for_final(5_000).1).collect();
        assert_eq!(asked.iter().filter(|&&d| d).count(), 2, "1 de cada 4 frases largas detecta");
        l.observe(5_000, Some("spanish"), true);
        assert_eq!(l.current(), "english", "un sondeo aislado no cambia el idioma");
        l.observe(5_000, Some("spanish"), true);
        assert_eq!(l.current(), "spanish");
    }
}
