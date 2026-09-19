use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
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
    /// Nombre real del dispositivo abierto por fuente (para los avisos).
    pub device_labels: Vec<(Source, String)>,
    /// Reunión asociada a esta grabación. Puede adjuntarse después de empezar
    /// (caso habitual: primero grabo y luego entro a la llamada).
    pub meeting_key: Option<String>,
    /// "teams" | "zoom" | "meet" de la reunión adjunta (el debounce es por app).
    pub meeting_app: Option<String>,
    /// Epoch ms en que se adjuntó la reunión.
    pub meeting_since: u64,
    /// Epoch ms de la última vez que ALGUNA fuente superó el umbral de voz.
    pub last_voice_at: Arc<AtomicU64>,
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

/// Ajustes de parada automática. El IPC va en SEGUNDOS (la UI convierte
/// minutos/horas): así la función se puede probar sin esperar 10 minutos.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoStopSettings {
    pub on_meeting_end: bool,
    pub on_silence: bool,
    /// 0 = desactivado
    pub silence_sec: u32,
    /// 0 = desactivado
    pub max_sec: u64,
    pub grace_sec: u32,
}

impl Default for AutoStopSettings {
    /// Duplicados de los valores del frontend: el vigilante arranca en
    /// `setup()`, antes de que `bootstrap()` empuje los ajustes reales.
    fn default() -> Self {
        Self {
            on_meeting_end: true,
            on_silence: true,
            silence_sec: 600,
            max_sec: 14_400,
            grace_sec: 60,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    MeetingEnd,
    Silence,
    MaxDuration,
}

impl StopReason {
    /// Valor que viaja en `StopResult.reason` (el frontend distingue por el
    /// prefijo "auto-" para saber si mostrar el aviso de parada automática).
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::MeetingEnd => "auto-meeting-end",
            StopReason::Silence => "auto-silence",
            StopReason::MaxDuration => "auto-max-duration",
        }
    }
}

/// Propuesta de parada en curso (cuenta atrás).
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingStop {
    pub session_id: String,
    pub reason: StopReason,
    /// Instante absoluto en epoch ms: la UI pinta `deadline - now` y no
    /// acumula deriva aunque la ventana esté minimizada.
    pub deadline_ms: u64,
    pub grace_sec: u32,
    /// Texto ya localizado para el popup y el banner.
    pub detail: String,
}

#[derive(Default)]
pub struct AutoStopState {
    pub settings: AutoStopSettings,
    pub pending: Option<PendingStop>,
    /// `stop_live` en vuelo: puede tardar ~50 s por el timeout del motor.
    pub firing: bool,
    /// Sondeos consecutivos sin ver la reunión adjunta.
    pub missing_ticks: u32,
    /// No proponer nada antes de este instante (tras «Seguir grabando»).
    pub suppress_until_ms: u64,
    /// Prórroga del tope duro.
    pub max_extra_ms: u64,
    /// Epoch ms del sondeo anterior, para detectar suspensión del equipo.
    pub last_tick_ms: u64,
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
    pub autostop: Mutex<AutoStopState>,
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
            autostop: Mutex::new(AutoStopState::default()),
        }
    }
}

impl AppState {
    pub fn is_live(&self) -> bool {
        self.live.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}
