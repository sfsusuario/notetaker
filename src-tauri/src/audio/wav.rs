//! Escritura/lectura de WAV 16 kHz mono i16 (hound) y utilidades en memoria.
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

use super::TARGET_RATE;

fn spec() -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

enum WavMsg {
    Samples(Vec<i16>),
    Finalize,
}

/// Escritor en un hilo propio: la tarea del pipeline nunca bloquea en disco.
/// La cabecera se actualiza cada segundo (`flush`) para que el archivo sea
/// legible aunque la app muera sin `finalize`.
pub struct WavSink {
    tx: Option<mpsc::Sender<WavMsg>>,
    thread: Option<JoinHandle<()>>,
}

impl WavSink {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("No se pudo crear {}: {e}", parent.display()))?;
        }
        let file = File::create(path).map_err(|e| format!("No se pudo crear {}: {e}", path.display()))?;
        let mut writer = WavWriter::new(BufWriter::new(file), spec())
            .map_err(|e| format!("No se pudo iniciar el WAV: {e}"))?;
        let (tx, rx) = mpsc::channel::<WavMsg>();
        let p = path.to_path_buf();
        let thread = std::thread::Builder::new()
            .name(format!("wav-{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("out")))
            .spawn(move || {
                let mut last_flush = Instant::now();
                loop {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(WavMsg::Samples(s)) => {
                            for v in s {
                                let _ = writer.write_sample(v);
                            }
                        }
                        Ok(WavMsg::Finalize) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if last_flush.elapsed() >= Duration::from_secs(1) {
                        let _ = writer.flush();
                        last_flush = Instant::now();
                    }
                }
                if let Err(e) = writer.finalize() {
                    eprintln!("[wav] finalize {}: {e}", p.display());
                }
            })
            .map_err(|e| format!("No se pudo crear el hilo WAV: {e}"))?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    pub fn write(&self, samples: &[i16]) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(WavMsg::Samples(samples.to_vec()));
        }
    }

    /// Cierra el archivo y espera a que la cabecera quede escrita.
    pub fn finalize(mut self) {
        self.finish();
    }

    fn finish(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(WavMsg::Finalize);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WavSink {
    fn drop(&mut self) {
        self.finish();
    }
}

/// WAV PCM 16-bit mono completo en memoria (cabecera de 44 bytes + datos).
pub fn encode_wav_in_memory(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Lee un WAV escrito por esta app (16 kHz mono i16). Para otros formatos
/// usar `decode::decode_to_pcm16k`.
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<i16>, String> {
    let mut reader = WavReader::open(path).map_err(|e| format!("No se pudo abrir {}: {e}", path.display()))?;
    let s = reader.spec();
    if s.channels != 1 || s.sample_rate != TARGET_RATE || s.bits_per_sample != 16 {
        return Err(format!(
            "Formato inesperado en {} ({} ch, {} Hz, {} bits)",
            path.display(),
            s.channels,
            s.sample_rate,
            s.bits_per_sample
        ));
    }
    reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Error leyendo {}: {e}", path.display()))
}

/// Mezcla N WAV 16k mono en uno (suma con saturación; longitud = la mayor).
pub fn mix_to_file(inputs: &[PathBuf], out: &Path) -> Result<u64, String> {
    let mut tracks: Vec<Vec<i16>> = Vec::new();
    for p in inputs {
        if p.exists() {
            match read_wav_16k_mono(p) {
                Ok(t) => tracks.push(t),
                Err(e) => eprintln!("[wav] mix: {e}"),
            }
        }
    }
    if tracks.is_empty() {
        return Err("No hay audio que mezclar".into());
    }
    let len = tracks.iter().map(|t| t.len()).max().unwrap_or(0);
    let mut writer = WavWriter::create(out, spec()).map_err(|e| format!("No se pudo crear {}: {e}", out.display()))?;
    for i in 0..len {
        let mut acc: i32 = 0;
        for t in &tracks {
            if let Some(v) = t.get(i) {
                acc += *v as i32;
            }
        }
        let _ = writer.write_sample(acc.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    writer.finalize().map_err(|e| format!("No se pudo cerrar {}: {e}", out.display()))?;
    Ok((len as u64 * 1000) / TARGET_RATE as u64)
}

pub fn wav_duration_ms(path: &Path) -> Option<u64> {
    let reader = WavReader::open(path).ok()?;
    let s = reader.spec();
    let frames = reader.duration() as u64;
    Some(frames * 1000 / s.sample_rate as u64)
}
