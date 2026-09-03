//! Cortador de "utterances" (frases) sobre chunks de 100 ms a 16 kHz. Lo usa
//! el motor local (whisper) tanto en vivo como sobre archivos: acumula audio
//! mientras hay voz y devuelve el bloque al detectar `silence_ms` de silencio
//! o al alcanzar `max_ms`.
use std::collections::VecDeque;

use super::downmix::rms_peak;
use super::vad::Vad;
use super::{CHUNK_SAMPLES, TARGET_RATE};

const CHUNK_MS: u64 = 100;

#[derive(Clone, Copy)]
pub struct CutterConfig {
    /// Silencio que cierra la frase.
    pub silence_ms: u32,
    /// Longitud máxima antes de cortar aunque siga hablando.
    pub max_ms: u32,
    /// Bloques con menos voz que esto se descartan (ruido, clics).
    pub min_voice_ms: u32,
    /// Audio previo a la primera voz que se conserva (evita cortar el inicio).
    pub pre_roll_ms: u32,
    /// Umbral RMS (0–1) para considerar voz.
    pub threshold: f32,
}

impl CutterConfig {
    pub const LIVE: CutterConfig = CutterConfig {
        silence_ms: 700,
        max_ms: 15_000,
        min_voice_ms: 300,
        pre_roll_ms: 200,
        threshold: 0.004,
    };
    pub const FILE: CutterConfig = CutterConfig {
        silence_ms: 600,
        max_ms: 28_000,
        min_voice_ms: 300,
        pre_roll_ms: 200,
        threshold: 0.003,
    };
}

pub struct Utterance {
    /// Posición (ms) del primer sample en la línea de tiempo de la fuente.
    pub start_ms: u64,
    pub samples: Vec<i16>,
}

impl Utterance {
    pub fn duration_ms(&self) -> u64 {
        (self.samples.len() as u64 * 1000) / TARGET_RATE as u64
    }
}

pub struct UtteranceCutter {
    cfg: CutterConfig,
    vad: Vad,
    pre_roll: VecDeque<Vec<i16>>,
    buf: Vec<i16>,
    /// ms de la línea de tiempo en los que empezó `buf` (incluye pre-roll).
    buf_start_ms: u64,
    voice_ms: u32,
    in_speech: bool,
    /// Posición actual (ms) = audio total entregado a `push`.
    cursor_ms: u64,
}

impl UtteranceCutter {
    pub fn new(cfg: CutterConfig) -> Self {
        let hangover = (cfg.silence_ms / CHUNK_MS as u32).max(1);
        Self {
            cfg,
            vad: Vad::new(cfg.threshold, hangover),
            pre_roll: VecDeque::new(),
            buf: Vec::new(),
            buf_start_ms: 0,
            voice_ms: 0,
            in_speech: false,
            cursor_ms: 0,
        }
    }

    /// Alimenta un chunk (idealmente de 100 ms). `position_ms` es la posición
    /// del chunk en la línea de tiempo de la fuente; si se pasa `None` se usa
    /// el cursor interno (útil al trocear archivos).
    pub fn push(&mut self, chunk: &[i16], position_ms: Option<u64>) -> Option<Utterance> {
        let pos = position_ms.unwrap_or(self.cursor_ms);
        let chunk_ms = (chunk.len() as u64 * 1000) / TARGET_RATE as u64;
        self.cursor_ms = pos + chunk_ms;

        let (rms, _) = rms_peak(chunk);
        let loud = rms >= self.cfg.threshold;
        let voice = self.vad.is_voice(rms);

        if !self.in_speech {
            if !voice {
                // Sin voz: mantener pre-roll
                self.pre_roll.push_back(chunk.to_vec());
                let max_chunks = (self.cfg.pre_roll_ms as usize / CHUNK_MS as usize).max(1);
                while self.pre_roll.len() > max_chunks {
                    self.pre_roll.pop_front();
                }
                return None;
            }
            // Arranca una frase: pre-roll + chunk
            self.in_speech = true;
            self.voice_ms = 0;
            let pre_ms: u64 = self
                .pre_roll
                .iter()
                .map(|c| (c.len() as u64 * 1000) / TARGET_RATE as u64)
                .sum();
            self.buf_start_ms = pos.saturating_sub(pre_ms);
            self.buf.clear();
            for c in self.pre_roll.drain(..) {
                self.buf.extend_from_slice(&c);
            }
        }

        self.buf.extend_from_slice(chunk);
        if loud {
            self.voice_ms += chunk_ms as u32;
        }

        let buf_ms = (self.buf.len() as u64 * 1000) / TARGET_RATE as u64;
        if !voice || buf_ms >= self.cfg.max_ms as u64 {
            return self.finish();
        }
        None
    }

    /// Cierra la frase en curso (fin de stream).
    pub fn flush(&mut self) -> Option<Utterance> {
        if !self.in_speech {
            return None;
        }
        self.finish()
    }

    fn finish(&mut self) -> Option<Utterance> {
        self.in_speech = false;
        self.vad = Vad::new(
            self.cfg.threshold,
            (self.cfg.silence_ms / CHUNK_MS as u32).max(1),
        );
        let samples = std::mem::take(&mut self.buf);
        let voice_ms = self.voice_ms;
        self.voice_ms = 0;
        self.pre_roll.clear();
        if voice_ms < self.cfg.min_voice_ms {
            return None;
        }
        // Recorta el silencio final sobrante (deja ~300 ms de cola)
        let keep_tail = (TARGET_RATE as usize * 300) / 1000;
        let trimmed = trim_trailing_silence(samples, self.cfg.threshold, keep_tail);
        if trimmed.len() < CHUNK_SAMPLES {
            return None;
        }
        Some(Utterance {
            start_ms: self.buf_start_ms,
            samples: trimmed,
        })
    }
}

fn trim_trailing_silence(mut samples: Vec<i16>, threshold: f32, keep_tail: usize) -> Vec<i16> {
    let mut end = samples.len();
    while end >= CHUNK_SAMPLES {
        let (rms, _) = rms_peak(&samples[end - CHUNK_SAMPLES..end]);
        if rms >= threshold {
            break;
        }
        end -= CHUNK_SAMPLES;
    }
    let cut = (end + keep_tail).min(samples.len());
    samples.truncate(cut);
    samples
}
