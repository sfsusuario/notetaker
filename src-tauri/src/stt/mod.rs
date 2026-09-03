//! Capa común de motores de transcripción: identificadores, capacidades,
//! payloads de eventos hacia el WebView y el enum `Engine` que despacha a
//! Deepgram o whisper.cpp.
pub mod deepgram_file;
pub mod deepgram_live;
pub mod whisper;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, watch};

use crate::state::AppState;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "lowercase")]
pub enum EngineId {
    Deepgram,
    Whisper,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Mic,
    System,
    File,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Mic => "mic",
            Source::System => "system",
            Source::File => "file",
        }
    }
}

/// 100 ms de audio a 16 kHz con su posición absoluta en la línea de tiempo
/// de la fuente (= posición en el WAV guardado).
pub struct AudioChunk {
    pub samples: Vec<i16>,
    pub position_ms: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SegmentPayload {
    pub id: String,
    pub session_id: String,
    pub source: Source,
    pub speaker: Option<String>,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub received_at: u64,
    pub is_final: bool,
    pub language: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub session_id: String,
    pub source: Source,
    pub engine: EngineId,
    /// connected | degraded | reconnecting | disconnected | loading
    pub status: &'static str,
    pub latency_ms: u64,
    pub retry_count: u32,
    pub message: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub session_id: String,
    /// decoding | uploading | transcribing | done | error | cancelled
    pub phase: &'static str,
    pub percent: f32,
    pub message: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EngineCapabilities {
    pub id: EngineId,
    pub label: String,
    pub description: String,
    pub diarization: bool,
    pub partials: bool,
    pub offline: bool,
    pub needs_api_key: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub size_mb: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    #[serde(flatten)]
    pub caps: EngineCapabilities,
    pub ready: bool,
    pub readiness: String,
    pub models: Vec<ModelInfo>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Id determinista por posición: si un motor reemite el mismo segmento el id
/// coincide y la UI/DB lo deduplican.
pub fn segment_id(source: Source, start_ms: u64, end_ms: u64) -> String {
    format!("{}-{start_ms}-{end_ms}", source.as_str())
}

pub fn emit_segment(app: &AppHandle, payload: SegmentPayload) {
    let event = if payload.is_final {
        "stt://final"
    } else {
        "stt://partial"
    };
    let _ = app.emit(event, payload);
}

pub fn emit_status(app: &AppHandle, payload: StatusPayload) {
    let _ = app.emit("stt://status", payload);
}

pub fn emit_progress(app: &AppHandle, payload: ProgressPayload) {
    let _ = app.emit("stt://progress", payload);
}

/// Error visible para el usuario (banner).
pub fn emit_error(app: &AppHandle, message: impl Into<String>) {
    let _ = app.emit(
        "audio://error",
        serde_json::json!({ "message": message.into() }),
    );
}

pub fn capabilities(id: EngineId) -> EngineCapabilities {
    match id {
        EngineId::Deepgram => EngineCapabilities {
            id,
            label: "Deepgram (nube)".into(),
            description: "Nova-3 en streaming. Máxima calidad y latencia mínima; requiere API key."
                .into(),
            diarization: true,
            partials: true,
            offline: false,
            needs_api_key: true,
        },
        EngineId::Whisper => EngineCapabilities {
            id,
            label: "Whisper local".into(),
            description:
                "whisper.cpp en tu equipo, sin conexión. Transcribe por frases con 1–4 s de retraso."
                    .into(),
            diarization: false,
            partials: false,
            offline: true,
            needs_api_key: false,
        },
    }
}

#[tauri::command]
pub fn engines_list(app: AppHandle) -> Vec<EngineInfo> {
    let deepgram_ready = crate::secrets::has_secret("deepgram");
    let ws = whisper::install::status(&app);
    let whisper_models: Vec<ModelInfo> = ws
        .models
        .iter()
        .map(|m| ModelInfo {
            id: m.id.clone(),
            label: m.label.clone(),
            installed: m.installed,
            size_mb: m.size_mb,
        })
        .collect();
    let installed_models: Vec<&ModelInfo> = whisper_models.iter().filter(|m| m.installed).collect();
    let whisper_ready = ws.server_installed && !installed_models.is_empty();
    let whisper_readiness = if !ws.server_installed {
        "Servidor no instalado".to_string()
    } else if installed_models.is_empty() {
        "Sin modelos descargados".to_string()
    } else {
        format!(
            "Modelos: {}",
            installed_models
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    vec![
        EngineInfo {
            caps: capabilities(EngineId::Deepgram),
            ready: deepgram_ready,
            readiness: if deepgram_ready {
                "API key configurada".into()
            } else {
                "Falta la API key (Ajustes)".into()
            },
            models: vec![ModelInfo {
                id: "nova-3".into(),
                label: "Nova-3".into(),
                installed: true,
                size_mb: 0,
            }],
        },
        EngineInfo {
            caps: capabilities(EngineId::Whisper),
            ready: whisper_ready,
            readiness: whisper_readiness,
            models: whisper_models,
        },
    ]
}

/// Parámetros comunes de una transcripción (vivo o archivo).
#[derive(Clone, Debug)]
pub struct SttSpec {
    pub session_id: String,
    pub source: Source,
    /// "auto" o código BCP-47 corto ("es", "en"…)
    pub language: String,
}

/// Motor instanciado con sus credenciales/modelo.
#[derive(Clone)]
pub enum Engine {
    Deepgram { api_key: String },
    Whisper { model: String },
}

impl Engine {
    /// Construye el motor validando requisitos (API key / modelo instalado).
    pub async fn build(
        app: &AppHandle,
        state: &Arc<AppState>,
        id: EngineId,
        model: Option<String>,
        threads: u32,
    ) -> Result<Engine, String> {
        match id {
            EngineId::Deepgram => Ok(Engine::Deepgram {
                api_key: crate::secrets::read_deepgram_key()?,
            }),
            EngineId::Whisper => {
                let model = model.unwrap_or_else(|| "base".into());
                whisper::server::ensure_running(app, state, &model, threads).await?;
                Ok(Engine::Whisper { model })
            }
        }
    }

    /// Consume chunks de audio hasta que `rx` se cierre o `stop_rx` sea true.
    pub async fn run_live(
        self,
        app: AppHandle,
        state: Arc<AppState>,
        spec: SttSpec,
        rx: mpsc::Receiver<AudioChunk>,
        stop_rx: watch::Receiver<bool>,
    ) {
        match self {
            Engine::Deepgram { api_key } => {
                deepgram_live::run(app, api_key, spec, rx, stop_rx).await;
            }
            Engine::Whisper { model } => {
                whisper::live::run(app, state, model, spec, rx, stop_rx).await;
            }
        }
    }

    /// Transcribe PCM 16 kHz mono completo emitiendo `stt://final` por segmento.
    pub async fn transcribe_pcm(
        &self,
        app: AppHandle,
        state: Arc<AppState>,
        spec: SttSpec,
        pcm: Vec<i16>,
        cancel: watch::Receiver<bool>,
    ) -> Result<(), String> {
        match self {
            Engine::Deepgram { api_key } => {
                deepgram_file::transcribe(app, state, api_key.clone(), spec, pcm, cancel).await
            }
            Engine::Whisper { model } => {
                whisper::file::transcribe(app, state, model.clone(), spec, pcm, cancel).await
            }
        }
    }
}
