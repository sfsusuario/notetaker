//! Identificación de hablantes local (quién habla y cuándo) con sherpa-onnx
//! como proceso auxiliar, igual que whisper-server: binario oficial
//! precompilado (CPU) + segmentación pyannote 3.0 + huella de voz CAM++
//! multilingüe. Se ejecuta al terminar una transcripción, sobre el WAV de una
//! pista; el frontend reparte los turnos entre los segmentos de whisper.
//!
//! Medido en un Core Ultra 7 258V con 3 lectores reales mezclados en turnos de
//! 1.5–20 s: 27x tiempo real, ~350 MB de RAM, número de hablantes acertado sin
//! indicarlo y ~85 % del tiempo de voz bien atribuido. Los modelos de huella
//! entrenados solo en inglés (wespeaker, eres2net) confundían voces en español.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::paths;
use crate::state::AppState;
use crate::stt::whisper::install::{download_with_progress, emit_dl, DownloadProgress};

pub const SHERPA_VERSION: &str = "v1.13.8";
const EXE: &str = "sherpa-onnx-offline-speaker-diarization.exe";
/// Lo único que necesita el ejecutable de la distribución oficial.
const BIN_FILES: &[&str] = &[EXE, "onnxruntime.dll", "onnxruntime_providers_shared.dll"];
/// Librería en C del mismo paquete (carpeta `lib/`): huellas de voz por frase
/// para etiquetar hablantes en vivo (`speaker.rs`).
pub const CAPI_DLL: &str = "sherpa-onnx-c-api.dll";

pub fn capi_dll(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(bin_dir(app)?.join(CAPI_DLL))
}

pub fn embedding_model(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(models_dir(app)?.join("embedding.onnx"))
}
const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";

/// Desplazamiento de la ventana de segmentación: 0.25 en vez de 0.1 es 2.5x
/// más rápido y acertó lo mismo (82.8 % frente a 80.4 % en 30 min).
const WINDOW_SHIFT: &str = "0.25";
/// Distancia para agrupar huellas: entre 0.7 y 1.0 encontró los 3 hablantes
/// reales; el valor de serie (0.5) creaba entre 6 y 15 grupos.
const CLUSTER_THRESHOLD: &str = "0.9";
/// Grupos con todos sus tramos por debajo de esto y poco tiempo total son
/// fragmentos espurios (en 30 min salieron dos: 15.8 s en tramos de ≤ 2.8 s y
/// 0.3 s); los hablantes reales siempre tienen algún turno más largo.
const SPURIOUS_MAX_TURN_MS: u64 = 3_000;
const SPURIOUS_SHARE: f64 = 0.03;

fn archive_name() -> String {
    format!("sherpa-onnx-{SHERPA_VERSION}-win-x64-shared-MT-Release-no-tts")
}

fn bin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(paths::diarize_dir(app)?.join("bin"))
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(paths::diarize_dir(app)?.join("models"))
}

fn is_file_over(p: &Path, bytes: u64) -> bool {
    std::fs::metadata(p).map(|m| m.len() > bytes).unwrap_or(false)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiarizeStatus {
    pub installed: bool,
    /// Además puede etiquetar en vivo (librería en C presente).
    pub live: bool,
    pub dir: String,
    pub version: String,
}

pub fn status(app: &AppHandle) -> DiarizeStatus {
    let dir = paths::diarize_dir(app).unwrap_or_default();
    let installed = BIN_FILES.iter().all(|f| dir.join("bin").join(f).exists())
        && is_file_over(&dir.join("models").join("segmentation.onnx"), 1_000_000)
        && is_file_over(&dir.join("models").join("embedding.onnx"), 1_000_000);
    DiarizeStatus {
        installed,
        live: installed && dir.join("bin").join(CAPI_DLL).exists(),
        dir: dir.to_string_lossy().to_string(),
        version: SHERPA_VERSION.into(),
    }
}

/// Instalaciones anteriores a las etiquetas en vivo: completa la librería en C.
pub async fn complete_install(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let st = status(app);
    if !st.installed || st.live {
        return Ok(());
    }
    install(app, state).await
}

#[tauri::command]
pub fn diarize_status(app: AppHandle) -> DiarizeStatus {
    status(&app)
}

/// Extrae un .tar.bz2 con el tar de Windows (bsdtar, incluido desde Windows 10 1803).
fn untar(archive: &Path, dest: &Path) -> Result<(), String> {
    let mut cmd = Command::new("tar");
    cmd.arg("-xjf").arg(archive).arg("-C").arg(dest).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("No se pudo ejecutar tar: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar no pudo extraer {}: {}",
            archive.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn dl_event(item: &str, phase: &'static str, message: Option<String>) -> DownloadProgress {
    DownloadProgress {
        item: item.into(),
        downloaded: 0,
        total: None,
        percent: if phase == "error" { 0.0 } else { 100.0 },
        phase,
        message,
    }
}

/// Descarga el ejecutable (~18 MB) y los dos modelos (~34 MB).
#[tauri::command]
pub async fn diarize_install(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<DiarizeStatus, String> {
    let result = install(&app, &state).await;
    match &result {
        Ok(()) => emit_dl(&app, dl_event("diarize", "done", None)),
        Err(e) => emit_dl(&app, dl_event("diarize", "error", Some(e.clone()))),
    }
    result.map(|_| status(&app))
}

async fn install(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let root = paths::diarize_dir(app)?;
    let (bin, models, tmp) = (bin_dir(app)?, models_dir(app)?, root.join("tmp"));
    for d in [&bin, &models, &tmp] {
        paths::ensure_dir(d)?;
    }
    let client = state.http.clone();

    if !BIN_FILES.iter().all(|f| bin.join(f).exists()) || !bin.join(CAPI_DLL).exists() {
        let url = format!(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/{SHERPA_VERSION}/{}.tar.bz2",
            archive_name()
        );
        let archive = tmp.join("sherpa.tar.bz2");
        download_with_progress(app, &client, &url, &archive, "diarize").await?;
        emit_dl(app, dl_event("diarize", "extracting", None));
        let (t, b) = (tmp.clone(), bin.clone());
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            untar(&archive, &t)?;
            let root = t.join(archive_name());
            // Solo lo que falte: onnxruntime.dll puede estar cargado en la app
            // (huellas en vivo) y Windows no deja sobrescribirlo.
            let files = BIN_FILES.iter().map(|f| ("bin", *f)).chain([("lib", CAPI_DLL)]);
            for (from, f) in files {
                if !b.join(f).exists() {
                    std::fs::copy(root.join(from).join(f), b.join(f)).map_err(|e| format!("No se pudo copiar {f}: {e}"))?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    let seg = models.join("segmentation.onnx");
    if !is_file_over(&seg, 1_000_000) {
        let archive = tmp.join("segmentation.tar.bz2");
        download_with_progress(app, &client, SEGMENTATION_URL, &archive, "diarize").await?;
        let t = tmp.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            untar(&archive, &t)?;
            std::fs::copy(t.join("sherpa-onnx-pyannote-segmentation-3-0").join("model.onnx"), &seg)
                .map_err(|e| format!("No se pudo copiar el modelo de segmentación: {e}"))?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())??;
    }

    let emb = models.join("embedding.onnx");
    if !is_file_over(&emb, 1_000_000) {
        download_with_progress(app, &client, EMBEDDING_URL, &emb, "diarize").await?;
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Turno de un hablante en la línea de tiempo del WAV analizado.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 0, 1, 2… por orden de aparición.
    pub speaker: u32,
}

/// Analiza un WAV 16 kHz mono y devuelve los turnos de cada hablante.
#[tauri::command]
pub async fn diarize_audio(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<Vec<Turn>, String> {
    if !status(&app).installed {
        return Err("La identificación de hablantes no está instalada (Ajustes → Identificar hablantes).".into());
    }
    let wav = PathBuf::from(&path);
    if !wav.exists() {
        return Err(format!("No existe {}", wav.display()));
    }
    // Una a la vez: cada análisis ocupa 4 núcleos.
    let _guard = state.diarize.lock().await;
    let (bin, models) = (bin_dir(&app)?, models_dir(&app)?);
    // Más de 4 hilos fue más lento (60 s frente a 32 s) por los núcleos E.
    let threads = crate::stt::whisper::server::max_threads().clamp(1, 4);
    let started = std::time::Instant::now();
    let stdout = tokio::task::spawn_blocking(move || run(&bin, &models, &wav, threads))
        .await
        .map_err(|e| e.to_string())??;
    let turns = clean_turns(parse_output(&stdout));
    let speakers = turns.iter().map(|t| t.speaker).max().map_or(0, |m| m + 1);
    eprintln!(
        "[diarize] {} turnos, {speakers} hablantes en {:.1} s",
        turns.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(turns)
}

fn run(bin: &Path, models: &Path, wav: &Path, threads: u32) -> Result<String, String> {
    let mut cmd = Command::new(bin.join(EXE));
    cmd.arg(format!("--segmentation.pyannote-model={}", models.join("segmentation.onnx").display()))
        .arg(format!("--embedding.model={}", models.join("embedding.onnx").display()))
        .arg(format!("--segmentation.num-threads={threads}"))
        .arg(format!("--embedding.num-threads={threads}"))
        .arg(format!("--segmentation.pyannote-window-shift-ratio={WINDOW_SHIFT}"))
        .arg(format!("--clustering.cluster-threshold={CLUSTER_THRESHOLD}"))
        .arg("--print-args=false")
        .arg(wav)
        .current_dir(bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Prioridad baja: no debe competir con la UI ni con una sesión en vivo.
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    let child = cmd.spawn().map_err(|e| format!("No se pudo lanzar sherpa-onnx: {e}"))?;
    crate::stt::whisper::server::attach_kill_on_close(&child);
    let out = child.wait_with_output().map_err(|e| format!("sherpa-onnx: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(3).collect();
        return Err(format!("sherpa-onnx terminó con {}: {}", out.status, tail.join(" | ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Líneas "0.318 -- 6.865 speaker_00" → turnos (speaker = número de grupo).
fn parse_output(stdout: &str) -> Vec<Turn> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let start: f64 = it.next()?.parse().ok()?;
            if it.next()? != "--" {
                return None;
            }
            let end: f64 = it.next()?.parse().ok()?;
            let speaker: u32 = it.next()?.strip_prefix("speaker_")?.parse().ok()?;
            Some(Turn {
                start_ms: (start * 1000.0).round() as u64,
                end_ms: (end * 1000.0).round() as u64,
                speaker,
            })
        })
        .collect()
}

/// Reasigna los grupos espurios al hablante vecino, une turnos contiguos del
/// mismo hablante y numera los hablantes por orden de aparición.
fn clean_turns(mut turns: Vec<Turn>) -> Vec<Turn> {
    turns.sort_by_key(|t| t.start_ms);
    let total: u64 = turns.iter().map(|t| t.end_ms - t.start_ms).sum();
    let mut per: HashMap<u32, (u64, u64)> = HashMap::new(); // (total, tramo máx.)
    for t in &turns {
        let e = per.entry(t.speaker).or_default();
        e.0 += t.end_ms - t.start_ms;
        e.1 = e.1.max(t.end_ms - t.start_ms);
    }
    let spurious = |s: u32| {
        per.len() > 1
            && per.get(&s).is_some_and(|&(sum, longest)| {
                longest < SPURIOUS_MAX_TURN_MS && (sum as f64) < SPURIOUS_SHARE * total as f64
            })
    };
    let keep: Vec<usize> = (0..turns.len()).filter(|&i| !spurious(turns[i].speaker)).collect();
    if keep.is_empty() {
        return Vec::new();
    }
    for i in 0..turns.len() {
        if spurious(turns[i].speaker) {
            // El turno válido más cercano en el tiempo
            let nearest = keep
                .iter()
                .min_by_key(|&&k| {
                    let (a, b) = (&turns[i], &turns[k]);
                    if b.end_ms <= a.start_ms {
                        a.start_ms - b.end_ms
                    } else {
                        b.start_ms.saturating_sub(a.end_ms)
                    }
                })
                .copied()
                .unwrap_or(keep[0]);
            turns[i].speaker = turns[nearest].speaker;
        }
    }
    let mut merged: Vec<Turn> = Vec::with_capacity(turns.len());
    for t in turns {
        match merged.last_mut() {
            Some(last) if last.speaker == t.speaker && t.start_ms <= last.end_ms + 500 => {
                last.end_ms = last.end_ms.max(t.end_ms);
            }
            _ => merged.push(t),
        }
    }
    let mut order: HashMap<u32, u32> = HashMap::new();
    for t in &mut merged {
        let next = order.len() as u32;
        t.speaker = *order.entry(t.speaker).or_insert(next);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(s: u64, e: u64, spk: u32) -> Turn {
        Turn { start_ms: s, end_ms: e, speaker: spk }
    }

    #[test]
    fn parses_sherpa_output() {
        let out = "OfflineSpeakerDiarizationConfig(...)\nStarted\n0.318 -- 6.865 speaker_00\n7.017 -- 10.747 speaker_01\nDuration : 56.861 s\n";
        assert_eq!(parse_output(out), vec![turn(318, 6865, 0), turn(7017, 10747, 1)]);
    }

    #[test]
    fn renumbers_by_first_appearance() {
        let t = clean_turns(vec![turn(0, 5_000, 3), turn(6_000, 12_000, 0), turn(13_000, 20_000, 3)]);
        assert_eq!(t.iter().map(|x| x.speaker).collect::<Vec<_>>(), vec![0, 1, 0]);
    }

    #[test]
    fn spurious_fragments_join_the_nearest_speaker() {
        // Grupo 7: un tramo de 1 s entre dos turnos largos del grupo 0
        let mut v = vec![turn(0, 20_000, 0), turn(20_000, 21_000, 7), turn(21_200, 40_000, 0)];
        v.push(turn(40_000, 70_000, 1));
        let t = clean_turns(v);
        assert_eq!(t, vec![turn(0, 40_000, 0), turn(40_000, 70_000, 1)]);
    }

    #[test]
    fn brief_real_speaker_with_a_full_sentence_is_kept() {
        // 4 s en total pero con un turno de 4 s: alguien que habló una vez
        let t = clean_turns(vec![turn(0, 300_000, 0), turn(300_000, 304_000, 1), turn(304_000, 600_000, 0)]);
        assert_eq!(t.iter().map(|x| x.speaker).max(), Some(1));
    }
}
