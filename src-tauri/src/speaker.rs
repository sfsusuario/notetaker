//! Hablantes en vivo: huella de voz (CAM++) de cada frase con la librería en C
//! de sherpa-onnx (`sherpa-onnx-c-api.dll`, misma versión que el ejecutable de
//! `diarize.rs`) y agrupación al vuelo. Es provisional: al terminar la sesión,
//! el análisis completo de `diarize.rs` corrige las etiquetas conservando la
//! numeración que ya se vio.
use std::ffi::{c_char, c_void, CString};
use std::path::Path;

use libloading::Library;

#[repr(C)]
struct ExtractorConfig {
    model: *const c_char,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
}

type CreateFn = unsafe extern "C" fn(*const ExtractorConfig) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type DimFn = unsafe extern "C" fn(*const c_void) -> i32;
type CreateStreamFn = unsafe extern "C" fn(*const c_void) -> *const c_void;
type AcceptWaveformFn = unsafe extern "C" fn(*const c_void, i32, *const f32, i32);
type InputFinishedFn = unsafe extern "C" fn(*const c_void);
type IsReadyFn = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;
type ComputeFn = unsafe extern "C" fn(*const c_void, *const c_void) -> *const f32;
type DestroyEmbeddingFn = unsafe extern "C" fn(*const f32);
type DestroyStreamFn = unsafe extern "C" fn(*const c_void);

/// Extractor de huellas de voz. Se usa detrás de un Mutex (una frase a la vez).
pub struct Embedder {
    extractor: *const c_void,
    dim: usize,
    destroy: DestroyFn,
    create_stream: CreateStreamFn,
    accept: AcceptWaveformFn,
    input_finished: InputFinishedFn,
    is_ready: IsReadyFn,
    compute: ComputeFn,
    destroy_embedding: DestroyEmbeddingFn,
    destroy_stream: DestroyStreamFn,
    // Al final: se descarga después de destruir el extractor (orden de Drop).
    _lib: Library,
}

// El extractor no tiene afinidad de hilo; el acceso se serializa con un Mutex.
unsafe impl Send for Embedder {}
unsafe impl Sync for Embedder {}

impl Embedder {
    pub fn load(dll: &Path, model: &Path, threads: i32) -> Result<Self, String> {
        // LOAD_WITH_ALTERED_SEARCH_PATH: onnxruntime.dll se busca junto al DLL.
        let lib = unsafe { libloading::os::windows::Library::load_with_flags(dll, 0x0000_0008) }
            .map_err(|e| format!("No se pudo cargar {}: {e}", dll.display()))?;
        let lib: Library = lib.into();
        unsafe {
            macro_rules! sym {
                ($name:literal, $t:ty) => {
                    *lib.get::<$t>($name).map_err(|e| format!("sherpa-onnx: falta {}: {e}", String::from_utf8_lossy($name)))?
                };
            }
            let create: CreateFn = sym!(b"SherpaOnnxCreateSpeakerEmbeddingExtractor\0", CreateFn);
            let dim: DimFn = sym!(b"SherpaOnnxSpeakerEmbeddingExtractorDim\0", DimFn);
            let model_c = CString::new(model.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
            let provider = CString::new("cpu").unwrap();
            let config = ExtractorConfig {
                model: model_c.as_ptr(),
                num_threads: threads,
                debug: 0,
                provider: provider.as_ptr(),
            };
            let extractor = create(&config);
            if extractor.is_null() {
                return Err(format!("sherpa-onnx no pudo cargar el modelo {}", model.display()));
            }
            Ok(Self {
                extractor,
                dim: dim(extractor).max(0) as usize,
                destroy: sym!(b"SherpaOnnxDestroySpeakerEmbeddingExtractor\0", DestroyFn),
                create_stream: sym!(b"SherpaOnnxSpeakerEmbeddingExtractorCreateStream\0", CreateStreamFn),
                accept: sym!(b"SherpaOnnxOnlineStreamAcceptWaveform\0", AcceptWaveformFn),
                input_finished: sym!(b"SherpaOnnxOnlineStreamInputFinished\0", InputFinishedFn),
                is_ready: sym!(b"SherpaOnnxSpeakerEmbeddingExtractorIsReady\0", IsReadyFn),
                compute: sym!(b"SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding\0", ComputeFn),
                destroy_embedding: sym!(b"SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding\0", DestroyEmbeddingFn),
                destroy_stream: sym!(b"SherpaOnnxDestroyOnlineStream\0", DestroyStreamFn),
                _lib: lib,
            })
        }
    }

    /// Huella normalizada (norma 1) de un audio 16 kHz; None si es muy corto.
    pub fn embed(&self, samples: &[i16]) -> Option<Vec<f32>> {
        if samples.is_empty() || self.dim == 0 {
            return None;
        }
        let wave: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        unsafe {
            let stream = (self.create_stream)(self.extractor);
            if stream.is_null() {
                return None;
            }
            (self.accept)(stream, 16_000, wave.as_ptr(), wave.len() as i32);
            (self.input_finished)(stream);
            let mut out = None;
            if (self.is_ready)(self.extractor, stream) != 0 {
                let v = (self.compute)(self.extractor, stream);
                if !v.is_null() {
                    out = Some(normalize(std::slice::from_raw_parts(v, self.dim).to_vec()));
                    (self.destroy_embedding)(v);
                }
            }
            (self.destroy_stream)(stream);
            out
        }
    }
}

impl Drop for Embedder {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.extractor) };
    }
}

pub type SharedEmbedder = std::sync::Arc<std::sync::Mutex<Embedder>>;

/// Extractor de la app (se carga una vez, ~28 MB de modelo). None si la
/// identificación de hablantes no está instalada con la librería en C.
pub fn shared_embedder(app: &tauri::AppHandle, state: &crate::state::AppState) -> Option<SharedEmbedder> {
    let mut slot = state.speaker_embedder.lock().unwrap();
    if let Some(e) = slot.as_ref() {
        return Some(e.clone());
    }
    if !crate::diarize::status(app).live {
        return None;
    }
    let (dll, model) = (crate::diarize::capi_dll(app).ok()?, crate::diarize::embedding_model(app).ok()?);
    match Embedder::load(&dll, &model, 2) {
        Ok(e) => {
            let e = std::sync::Arc::new(std::sync::Mutex::new(e));
            *slot = Some(e.clone());
            Some(e)
        }
        Err(err) => {
            eprintln!("[speaker] no se pudo cargar el extractor: {err}");
            None
        }
    }
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Agrupación al vuelo: cada huella va al hablante más parecido si supera el
/// umbral; si no, abre un hablante nuevo. Las frases cortas solo se asignan a
/// hablantes existentes (su huella es poco fiable).
pub struct OnlineSpeakers {
    /// Suma de huellas normalizadas por hablante (la media es el centroide).
    centroids: Vec<Vec<f32>>,
    threshold: f32,
    short_threshold: f32,
}

/// Frases más cortas que esto no crean hablantes nuevos.
pub const MIN_NEW_SPEAKER_MS: u64 = 2_000;

impl OnlineSpeakers {
    pub fn new(threshold: f32, short_threshold: f32) -> Self {
        Self { centroids: Vec::new(), threshold, short_threshold }
    }

    /// Hablante (0, 1…) de una frase de `dur_ms` con huella `emb`.
    pub fn assign(&mut self, emb: &[f32], dur_ms: u64) -> Option<u32> {
        let best = self
            .centroids
            .iter()
            .enumerate()
            .map(|(i, c)| (i, cosine(emb, &normalize(c.clone()))))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let short = dur_ms < MIN_NEW_SPEAKER_MS;
        match best {
            Some((i, sim)) if sim >= self.threshold || (short && sim >= self.short_threshold) => {
                self.centroids[i].iter_mut().zip(emb).for_each(|(c, e)| *c += e);
                Some(i as u32)
            }
            _ if short => None,
            _ => {
                self.centroids.push(emb.to_vec());
                Some((self.centroids.len() - 1) as u32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_new_speakers_and_reuses_known_ones() {
        let mut s = OnlineSpeakers::new(0.6, 0.4);
        let a = normalize(vec![1.0, 0.1, 0.0]);
        let b = normalize(vec![0.0, 1.0, 0.1]);
        assert_eq!(s.assign(&a, 5_000), Some(0));
        assert_eq!(s.assign(&b, 5_000), Some(1));
        assert_eq!(s.assign(&normalize(vec![0.9, 0.2, 0.0]), 5_000), Some(0));
        // corta y distinta: no crea hablante
        assert_eq!(s.assign(&normalize(vec![0.0, 0.0, 1.0]), 1_000), None);
    }

    /// Calibración con las mezclas de 3 lectores (ver diarize.rs):
    /// SPK_DIR con mix6.wav/mix6.csv y SPK_BIN con sherpa-onnx-c-api.dll + embedding.onnx.
    #[test]
    #[ignore]
    fn calibrate_online_clustering() {
        use crate::audio::utterance::{CutterConfig, UtteranceCutter};
        let dir = std::path::PathBuf::from(std::env::var("SPK_DIR").unwrap());
        let bin = std::path::PathBuf::from(std::env::var("SPK_BIN").unwrap());
        let emb = Embedder::load(&bin.join("sherpa-onnx-c-api.dll"), &bin.join("embedding.onnx"), 2).unwrap();
        for mix in ["mix6", "mix30"] {
            let pcm: Vec<i16> = hound::WavReader::open(dir.join(format!("{mix}.wav"))).unwrap().samples::<i16>().map(|s| s.unwrap()).collect();
            let truth: Vec<(f64, f64, String)> = std::fs::read_to_string(dir.join(format!("{mix}.csv"))).unwrap().lines().skip(1)
                .map(|l| { let p: Vec<&str> = l.split(',').collect(); (p[0].parse().unwrap(), p[1].parse().unwrap(), p[2].to_string()) }).collect();
            let mut cutter = UtteranceCutter::new(CutterConfig::LIVE);
            let mut whole = Vec::new();
            for (i, chunk) in pcm.chunks(1600).enumerate() {
                if let Some(u) = cutter.push(chunk, Some(i as u64 * 100)) { whole.push(u); }
            }
            // SPK_SPLIT_MS > 0: etiquetar trozos de ~N ms dentro de cada frase
            let split: usize = std::env::var("SPK_SPLIT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut utts = Vec::new();
            for u in whole {
                if split == 0 { utts.push(u); continue; }
                let n = split * 16;
                let mut parts: Vec<&[i16]> = u.samples.chunks(n).collect();
                if parts.len() > 1 && parts.last().unwrap().len() < n / 2 {
                    let last = parts.pop().unwrap().len();
                    let prev = parts.pop().unwrap();
                    let start = prev.as_ptr() as usize - u.samples.as_ptr() as usize;
                    parts.push(&u.samples[start / 2..start / 2 + prev.len() + last]);
                }
                let mut off = 0usize;
                for p in parts {
                    utts.push(crate::audio::utterance::Utterance { start_ms: u.start_ms + (off / 16) as u64, samples: p.to_vec() });
                    off += p.len();
                }
            }
            // Techo: cada unidad con su lector mayoritario
            {
                let (mut tot, mut best) = (0.0, 0.0);
                for u in &utts {
                    let (a, b) = (u.start_ms as f64 / 1000.0, (u.start_ms + u.duration_ms()) as f64 / 1000.0);
                    let mut per: std::collections::HashMap<&str, f64> = Default::default();
                    for (ts, te, who) in &truth { let o = (b.min(*te) - a.max(*ts)).max(0.0); if o > 0.0 { *per.entry(who).or_default() += o; tot += o; } }
                    best += per.values().cloned().fold(0.0, f64::max);
                }
                println!("SPK {mix} techo con esta granularidad: {:.1} % ({} unidades)", 100.0 * best / tot, utts.len());
            }
            let t0 = std::time::Instant::now();
            let embs: Vec<Option<Vec<f32>>> = utts.iter().map(|u| emb.embed(&u.samples)).collect();
            let per = t0.elapsed().as_secs_f64() * 1000.0 / utts.len() as f64;
            for th in [0.45f32, 0.5, 0.55, 0.6, 0.65] {
                let mut s = OnlineSpeakers::new(th, th - 0.15);
                let mut co: std::collections::HashMap<(u32, String), f64> = Default::default();
                let (mut total, mut unl) = (0.0, 0.0);
                let mut n_spk = 0;
                for (u, e) in utts.iter().zip(&embs) {
                    let (a, b) = (u.start_ms as f64 / 1000.0, (u.start_ms + u.duration_ms()) as f64 / 1000.0);
                    let label = e.as_ref().and_then(|e| s.assign(e, u.duration_ms()));
                    for (ts, te, who) in &truth {
                        let o = (b.min(*te) - a.max(*ts)).max(0.0);
                        if o <= 0.0 { continue; }
                        total += o;
                        match label { Some(l) => { n_spk = n_spk.max(l + 1); *co.entry((l, who.clone())).or_default() += o; } None => unl += o }
                    }
                }
                // mapeo voraz grupo -> lector con más tiempo
                let mut by_label: std::collections::HashMap<u32, f64> = Default::default();
                for ((l, _), v) in &co { let e = by_label.entry(*l).or_default(); *e = e.max(*v); }
                let ok: f64 = by_label.values().sum();
                println!("SPK {mix} umbral {th:.2}: {n_spk} hablantes · acierto {:.1} % · sin etiqueta {:.1} % · {per:.0} ms/frase ({} frases)", 100.0 * ok / total, 100.0 * unl / total, utts.len());
            }
        }
    }
}
