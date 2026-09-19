//! Parada automática de la grabación: ajustes, propuesta con cuenta atrás y
//! cancelación. La evaluación de los disparadores vive en el hilo del
//! vigilante (`meeting::autostop_tick`); aquí están el estado compartido y los
//! comandos que usan la ventana principal y el popup.
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::state::{AppState, AutoStopSettings, PendingStop, StopReason};
use crate::stt::now_ms;

/// Tras «Seguir grabando» no se vuelve a proponer nada durante este tiempo.
const SUPPRESS_MS: u64 = 60_000;
/// Prórroga del tope duro cuando el usuario decide seguir.
const MAX_EXTRA_MS: u64 = 30 * 60_000;

/// Retira la propuesta en curso (si la hay) y avisa a las ventanas.
/// Idempotente: se llama desde varios sitios y puede repetirse.
pub fn clear_pending(app: &AppHandle, state: &AppState, cause: &str) {
    let had = {
        let mut a = state.autostop.lock().unwrap();
        a.missing_ticks = 0;
        a.firing = false;
        a.pending.take().is_some()
    };
    if had {
        let _ = app.emit("autostop://cancelled", serde_json::json!({ "cause": cause }));
        crate::meeting::hide_popup(app);
    }
}

/// Desvincula la reunión de la sesión viva (si la hay). Se usa al apagar la
/// detección y al cancelar una propuesta por fin de reunión: sin esto se
/// volvería a proponer en el sondeo siguiente, indefinidamente.
pub fn detach_meeting(state: &AppState) {
    if let Ok(mut guard) = state.live.lock() {
        if let Some(s) = guard.as_mut() {
            s.meeting_key = None;
            s.meeting_app = None;
        }
    }
}

#[tauri::command]
pub fn autostop_set(
    state: State<'_, Arc<AppState>>,
    settings: AutoStopSettings,
) -> Result<(), String> {
    eprintln!("[autostop] ajustes: {settings:?}");
    let mut a = state.autostop.lock().unwrap();
    a.settings = settings;
    // Cambiar los umbrales no retira una propuesta ya en marcha; solo reinicia
    // el contador de ausencias para que el debounce nuevo cuente desde cero.
    a.missing_ticks = 0;
    Ok(())
}

#[tauri::command]
pub fn autostop_pending(state: State<'_, Arc<AppState>>) -> Option<PendingStop> {
    state.autostop.lock().unwrap().pending.clone()
}

/// «Seguir grabando»: retira la propuesta y evita que vuelva enseguida.
#[tauri::command]
pub fn autostop_cancel(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let now = now_ms();
    let reason = {
        let mut a = state.autostop.lock().unwrap();
        let reason = a.pending.as_ref().map(|p| p.reason);
        a.pending = None;
        a.firing = false;
        a.missing_ticks = 0;
        a.suppress_until_ms = now + SUPPRESS_MS;
        if reason == Some(StopReason::MaxDuration) {
            a.max_extra_ms += MAX_EXTRA_MS;
        }
        reason
    };
    match reason {
        // El fin de reunión ya ocurrió: si no se desvincula, el vigilante
        // propondría de nuevo en cuanto pase la supresión.
        Some(StopReason::MeetingEnd) => detach_meeting(&state),
        Some(StopReason::Silence) => {
            if let Ok(guard) = state.live.lock() {
                if let Some(s) = guard.as_ref() {
                    s.last_voice_at.store(now, Ordering::Relaxed);
                }
            }
        }
        _ => {}
    }
    let _ = app.emit("autostop://cancelled", serde_json::json!({ "cause": "user" }));
    crate::meeting::hide_popup(&app);
    Ok(())
}

/// «Detener ahora»: lo decide el usuario, así que el motivo es "user" y no
/// lleva el aviso de parada automática.
#[tauri::command]
pub async fn autostop_stop_now(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    clear_pending(&app, &state, "user");
    crate::session::live::stop_live(&app, state.inner(), "user").await;
    Ok(())
}
