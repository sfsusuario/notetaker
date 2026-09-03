use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tauri::async_runtime::JoinHandle;

use crate::audio::capture::CaptureHandle;
use crate::meeting::MeetingInfo;
use crate::stt::whisper::server::WhisperServer;
use crate::stt::{EngineId, Source};

/// Handles de una fuente de audio en vivo (mic o sistema).
pub struct SourceHandles {
    pub stop_tx: watch::Sender<bool>,
    pub capture: CaptureHandle,
    pub paused: Arc<AtomicBool>,
    /// Tarea del pipeline (captura → WAV → chunks). Termina al parar.
    pub pipeline: Option<JoinHandle<()>>,
    /// Tarea del motor STT. Termina tras drenar los últimos resultados.
    pub engine: Option<JoinHandle<()>>,
    pub wav_path: PathBuf,
}

pub struct LiveSession {
    pub session_id: String,
    /// None = solo grabación (sin transcripción en vivo).
    pub engine: Option<EngineId>,
    pub sources: Vec<(Source, SourceHandles)>,
    pub audio_dir: PathBuf,
    pub started_at: u64,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingApps {
    pub teams: bool,
    pub zoom: bool,
    pub meet: bool,
}

impl Default for MeetingApps {
    fn default() -> Self {
        Self {
            teams: true,
            zoom: true,
            meet: true,
        }
    }
}

#[derive(Default)]
pub struct MeetingState {
    pub enabled: bool,
    pub apps: MeetingApps,
    /// Reuniones ignoradas por el usuario (clave estable por reunión).
    pub snoozed: HashSet<String>,
    pub current: Option<MeetingInfo>,
}

pub struct AppState {
    pub live: Mutex<Option<LiveSession>>,
    /// Cancelación del trabajo de transcripción de archivo en curso.
    pub file_cancel: Mutex<Option<watch::Sender<bool>>>,
    /// Proceso whisper-server (uno por app; se reutiliza entre sesiones).
    pub whisper: tokio::sync::Mutex<Option<WhisperServer>>,
    /// Serializa las peticiones /inference (el server las atiende de una en una).
    pub whisper_infer: tokio::sync::Mutex<()>,
    pub http: reqwest::Client,
    pub meeting: Mutex<MeetingState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            live: Mutex::new(None),
            file_cancel: Mutex::new(None),
            whisper: tokio::sync::Mutex::new(None),
            whisper_infer: tokio::sync::Mutex::new(()),
            http: reqwest::Client::builder()
                .user_agent("notetaker/0.1")
                .build()
                .expect("reqwest client"),
            meeting: Mutex::new(MeetingState::default()),
        }
    }
}

impl AppState {
    pub fn is_live(&self) -> bool {
        self.live.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}
