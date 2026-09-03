//! Decodificación de archivos de audio (wav/mp3/m4a/ogg/flac) a PCM 16 kHz
//! mono i16 con symphonia.
use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::downmix::f32_to_mono_i16;
use super::resample::Resampler;
use super::TARGET_RATE;

/// `on_progress(percent)` se llama con 0–100 (estimado por frames si el
/// contenedor informa la duración; si no, se queda en 0 hasta el final).
pub fn decode_to_pcm16k(path: &Path, mut on_progress: impl FnMut(f32)) -> Result<Vec<i16>, String> {
    let file = File::open(path).map_err(|e| format!("No se pudo abrir {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Formato de audio no reconocido: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("El archivo no tiene pista de audio")?;
    let track_id = track.id;
    let total_frames = track.codec_params.n_frames;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Códec no soportado: {e}"))?;

    let mut resampler: Option<Resampler> = None;
    let mut out: Vec<i16> = Vec::new();
    let mut decoded_frames: u64 = 0;
    let mut last_pct = -1.0f32;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("Error leyendo el audio: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("Error decodificando: {e}")),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count() as u16;
        let frames = decoded.frames() as u64;
        let mut sbuf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        sbuf.copy_interleaved_ref(decoded);
        let mono = f32_to_mono_i16(sbuf.samples(), channels);
        let rs = resampler.get_or_insert_with(|| Resampler::new(spec.rate, TARGET_RATE));
        out.extend(rs.process(&mono));
        decoded_frames += frames;
        if let Some(total) = total_frames {
            let pct = ((decoded_frames as f32 / total.max(1) as f32) * 100.0).min(100.0);
            if pct - last_pct >= 1.0 {
                last_pct = pct;
                on_progress(pct);
            }
        }
    }
    on_progress(100.0);
    if out.is_empty() {
        return Err("El archivo no contiene audio decodificable".into());
    }
    Ok(out)
}
