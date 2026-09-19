//! Detección de reuniones en segundo plano + popup de inicio rápido.
pub mod detector;
pub mod win;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub use detector::MeetingInfo;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

use crate::state::{AppState, MeetingApps, PendingStop, StopReason};
use crate::stt::now_ms;

const POLL_SECS: u64 = 3;

/// Sondeos sin ver la reunión antes de dar por terminada una llamada.
/// Teams solo cuenta como reunión si tiene el micrófono abierto (ver
/// `detector.rs`), así que silenciarse hace desaparecer la detección: 60 s
/// cubre a quien se silencia para escuchar. Zoom (ventana o micro) y Meet
/// (título de pestaña) no dependen del micrófono y toleran menos espera.
fn debounce_ticks(app: &str) -> u32 {
    match app {
        "teams" => 20, // 60 s
        _ => 5,        // 15 s
    }
}

/// Si entre dos sondeos pasa más de esto, el equipo estuvo suspendido.
const SUSPEND_GAP_MS: u64 = 30_000;
/// Margen tras adjuntar una reunión antes de poder darla por terminada.
const MEETING_SETTLE_MS: u64 = 60_000;

pub(crate) fn show_popup(app: &AppHandle) {
    let Some(w) = app.get_webview_window("popup") else {
        return;
    };
    if let Ok(Some(mon)) = w.primary_monitor() {
        let scale = mon.scale_factor();
        let size = mon.size();
        let pos = mon.position();
        let ww = w.outer_size().map(|s| s.width).unwrap_or(440);
        let x = pos.x + ((size.width as i32 - ww as i32) / 2).max(0);
        let y = pos.y + (24.0 * scale) as i32;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.show();
    let _ = w.set_always_on_top(true);
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.hide();
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Hilo de sondeo (cada 3 s). Las llamadas Win32/registro son síncronas y
/// baratas; un hilo propio evita bloquear el runtime async. Además del popup
/// de reunión detectada, aquí vive el vigilante de parada automática, que
/// debe correr incluso con la detección de reuniones apagada.
pub fn spawn_poller(app: AppHandle, state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("meeting-poller".into())
        .spawn(move || {
            let mut sys = sysinfo::System::new();
            loop {
                std::thread::sleep(Duration::from_secs(POLL_SECS));
                let (enabled, apps) = {
                    let m = state.meeting.lock().unwrap();
                    (m.enabled, m.apps)
                };
                let found = if enabled {
                    detector::detect(&mut sys, &apps)
                } else {
                    None
                };
                if enabled {
                    handle_transitions(&app, &state, found.clone());
                }
                // Siempre: el silencio y el tope duro no dependen del detector.
                autostop_tick(&app, &state, enabled, found.as_ref());
            }
        })
        .expect("meeting poller thread");
}

/// Popup de "reunión detectada" según las transiciones del detector.
fn handle_transitions(app: &AppHandle, state: &Arc<AppState>, found: Option<MeetingInfo>) {
    let mut m = state.meeting.lock().unwrap();
    let prev = m.current.clone();
    match (prev, found) {
        (None, Some(f)) => {
            let snoozed = m.snoozed.contains(&f.key);
            m.current = Some(f.clone());
            drop(m);
            if !snoozed && !state.is_live() {
                let _ = app.emit("meeting://detected", &f);
                show_popup(app);
            }
        }
        (Some(p), Some(f)) if p.key != f.key => {
            m.snoozed.remove(&p.key);
            let snoozed = m.snoozed.contains(&f.key);
            m.current = Some(f.clone());
            drop(m);
            let _ = app.emit("meeting://ended", &p);
            if !snoozed && !state.is_live() {
                let _ = app.emit("meeting://detected", &f);
                show_popup(app);
            } else {
                hide_popup(app);
            }
        }
        (Some(p), None) => {
            m.current = None;
            m.snoozed.remove(&p.key);
            drop(m);
            let _ = app.emit("meeting://ended", &p);
            hide_popup(app);
        }
        _ => {}
    }
}

/// Instantánea de la sesión viva: se toma con el lock corto y se suelta antes
/// de evaluar nada (nunca dos mutex a la vez, nunca `stop_live` con un lock).
struct LiveSnap {
    session_id: String,
    started_at: u64,
    paused: bool,
    meeting_key: Option<String>,
    meeting_app: Option<String>,
    meeting_since: u64,
    last_voice: Arc<std::sync::atomic::AtomicU64>,
}

fn snapshot(state: &AppState) -> Option<LiveSnap> {
    let guard = state.live.lock().ok()?;
    let s = guard.as_ref()?;
    Some(LiveSnap {
        session_id: s.session_id.clone(),
        started_at: s.started_at,
        paused: s
            .sources
            .first()
            .map(|(_, h)| h.paused.load(Ordering::Relaxed))
            .unwrap_or(false),
        meeting_key: s.meeting_key.clone(),
        meeting_app: s.meeting_app.clone(),
        meeting_since: s.meeting_since,
        last_voice: s.last_voice_at.clone(),
    })
}

fn attach_meeting(state: &AppState, info: &MeetingInfo, now: u64) {
    if let Ok(mut guard) = state.live.lock() {
        if let Some(s) = guard.as_mut() {
            s.meeting_key = Some(info.key.clone());
            s.meeting_app = Some(info.app.clone());
            s.meeting_since = now;
        }
    }
}

/// Reescribe solo la clave: misma app, pid nuevo (Teams reiniciado) es una
/// continuación de la misma reunión, no un final.
fn update_meeting_key(state: &AppState, key: &str) {
    if let Ok(mut guard) = state.live.lock() {
        if let Some(s) = guard.as_mut() {
            s.meeting_key = Some(key.to_string());
        }
    }
}

fn detail_for(reason: StopReason, snap: &LiveSnap, silence_sec: u32, max_sec: u64) -> String {
    match reason {
        StopReason::MeetingEnd => {
            let app_label = match snap.meeting_app.as_deref() {
                Some("teams") => "Microsoft Teams",
                Some("zoom") => "Zoom",
                Some("meet") => "Google Meet",
                _ => "la reunión",
            };
            format!("La reunión de {app_label} terminó.")
        }
        StopReason::Silence => {
            let min = (silence_sec / 60).max(1);
            if silence_sec < 60 {
                format!("No se oye nada desde hace {silence_sec} s.")
            } else {
                format!("No se oye nada desde hace {min} min.")
            }
        }
        StopReason::MaxDuration => {
            if max_sec >= 3600 {
                format!("La grabación lleva más de {} h.", max_sec / 3600)
            } else if max_sec >= 60 {
                format!("La grabación alcanzó el límite de {} min.", max_sec / 60)
            } else {
                format!("La grabación alcanzó el límite de {max_sec} s.")
            }
        }
    }
}

/// Entrada de la decisión, sin dependencias de Tauri ni de locks: así la
/// lógica de los disparadores se puede probar sola (ver los tests al final).
#[derive(Clone, Copy)]
pub(crate) struct DecideInput<'a> {
    pub now: u64,
    pub settings: crate::state::AutoStopSettings,
    pub started_at: u64,
    pub max_extra_ms: u64,
    pub last_voice: u64,
    pub detection_on: bool,
    pub meeting_attached: bool,
    pub meeting_since: u64,
    pub meeting_app: Option<&'a str>,
    /// La reunión adjunta se está viendo ahora mismo.
    pub meeting_present: bool,
    pub missing_ticks: u32,
}

/// Decide si toca proponer una parada. Devuelve el nuevo contador de
/// ausencias y el motivo (si lo hay).
pub(crate) fn decide(i: &DecideInput) -> (u32, Option<StopReason>) {
    // 1. Tope duro: tiempo de pared, pausas incluidas.
    if i.settings.max_sec > 0
        && i.now.saturating_sub(i.started_at) >= i.settings.max_sec * 1000 + i.max_extra_ms
    {
        return (0, Some(StopReason::MaxDuration));
    }

    // 2. Fin de reunión, con margen de asentamiento y debounce por app.
    let mut missing = i.missing_ticks;
    let meeting_rule = i.settings.on_meeting_end
        && i.detection_on
        && i.meeting_attached
        && i.now.saturating_sub(i.meeting_since) >= MEETING_SETTLE_MS;
    if meeting_rule {
        if i.meeting_present {
            missing = 0;
        } else {
            missing += 1;
        }
        if missing >= debounce_ticks(i.meeting_app.unwrap_or("")) {
            return (0, Some(StopReason::MeetingEnd));
        }
    } else {
        missing = 0;
    }

    // 3. Silencio, solo fuera de una reunión en curso: si sigues en la llamada
    // y solo escuchas, callarte es legítimo.
    if i.settings.on_silence
        && i.settings.silence_sec > 0
        && !i.meeting_present
        && i.now.saturating_sub(i.last_voice) >= i.settings.silence_sec as u64 * 1000
    {
        return (missing, Some(StopReason::Silence));
    }

    (missing, None)
}

/// Un sondeo del vigilante de parada automática.
fn autostop_tick(
    app: &AppHandle,
    state: &Arc<AppState>,
    detection_on: bool,
    found: Option<&MeetingInfo>,
) {
    let now = now_ms();
    let (settings, gap) = {
        let mut a = state.autostop.lock().unwrap();
        let gap = if a.last_tick_ms == 0 {
            0
        } else {
            now.saturating_sub(a.last_tick_ms)
        };
        a.last_tick_ms = now;
        (a.settings, gap)
    };

    let Some(snap) = snapshot(state) else {
        // Sin sesión: no debe quedar ninguna propuesta viva.
        let had = state.autostop.lock().unwrap().pending.is_some();
        if had {
            crate::autostop::clear_pending(app, state, "no-session");
        }
        return;
    };

    // El equipo estuvo suspendido: sin esto, al despertar `now - last_voice`
    // vale horas y se cortaría la grabación de inmediato.
    if gap > SUSPEND_GAP_MS {
        snap.last_voice.store(now, Ordering::Relaxed);
        {
            let mut a = state.autostop.lock().unwrap();
            a.missing_ticks = 0;
            a.max_extra_ms += gap;
            a.suppress_until_ms = now + 10_000;
        }
        crate::autostop::clear_pending(app, state, "resume");
        return;
    }

    // Vincular/desvincular la reunión de la sesión.
    if !detection_on {
        if snap.meeting_key.is_some() {
            crate::autostop::detach_meeting(state);
        }
    } else if let Some(info) = found {
        match (&snap.meeting_key, &snap.meeting_app) {
            (None, _) => attach_meeting(state, info, now),
            (Some(k), Some(a)) if a == &info.app && k != &info.key => {
                update_meeting_key(state, &info.key);
            }
            _ => {}
        }
    }

    // En pausa el contador de silencio se congela: pausar es deliberado.
    if snap.paused {
        snap.last_voice.store(now, Ordering::Relaxed);
    }

    // ¿Hay una propuesta en marcha? Comprobar vencimiento y coherencia.
    {
        let a = state.autostop.lock().unwrap();
        if let Some(p) = a.pending.clone() {
            let firing = a.firing;
            drop(a);
            if p.session_id != snap.session_id {
                crate::autostop::clear_pending(app, state, "session-changed");
                return;
            }
            if !firing && now >= p.deadline_ms {
                state.autostop.lock().unwrap().firing = true;
                let app2 = app.clone();
                let state2 = state.clone();
                let reason = p.reason;
                tauri::async_runtime::spawn(async move {
                    crate::session::live::stop_live(&app2, &state2, reason.as_str()).await;
                });
            }
            return;
        }
    }

    // Sin propuesta: evaluar disparadores.
    {
        let a = state.autostop.lock().unwrap();
        if a.firing || now < a.suppress_until_ms {
            return;
        }
    }

    let meeting_present = detection_on
        && found.is_some()
        && snap.meeting_key.is_some()
        && found.map(|f| Some(&f.app) == snap.meeting_app.as_ref()).unwrap_or(false);

    let (max_extra, missing_ticks) = {
        let a = state.autostop.lock().unwrap();
        (a.max_extra_ms, a.missing_ticks)
    };

    let (missing, reason) = decide(&DecideInput {
        now,
        settings,
        started_at: snap.started_at,
        max_extra_ms: max_extra,
        last_voice: snap.last_voice.load(Ordering::Relaxed),
        detection_on,
        meeting_attached: snap.meeting_key.is_some(),
        meeting_since: snap.meeting_since,
        meeting_app: snap.meeting_app.as_deref(),
        meeting_present,
        missing_ticks,
    });
    state.autostop.lock().unwrap().missing_ticks = missing;

    let Some(reason) = reason else { return };

    let pending = PendingStop {
        session_id: snap.session_id.clone(),
        reason,
        deadline_ms: now + settings.grace_sec as u64 * 1000,
        grace_sec: settings.grace_sec,
        detail: detail_for(reason, &snap, settings.silence_sec, settings.max_sec),
    };
    // Si alguien detuvo la sesión mientras evaluábamos, no proponer nada.
    // (Comprobado antes de tomar el lock de `autostop`: nunca dos a la vez.)
    if !state.is_live() {
        return;
    }
    {
        let mut a = state.autostop.lock().unwrap();
        a.pending = Some(pending.clone());
        a.missing_ticks = 0;
    }
    eprintln!("[autostop] propuesta: {:?} — {}", reason, pending.detail);
    let _ = app.emit("autostop://proposed", &pending);
    // Sin `show_main`: no robar el foco mientras se comparte pantalla.
    show_popup(app);
}

/// Activa o desactiva la detección de reuniones. Lo usan el comando de
/// Ajustes y el ítem de la bandeja: al apagarla hay que desvincular la reunión
/// de la sesión viva, o el vigilante propondría parar por "fin de reunión".
pub fn set_detection_enabled(app: &AppHandle, state: &Arc<AppState>, enabled: bool) {
    {
        let mut m = state.meeting.lock().unwrap();
        m.enabled = enabled;
        if !enabled {
            m.current = None;
        }
    }
    if !enabled {
        crate::autostop::detach_meeting(state);
        crate::autostop::clear_pending(app, state, "detection-off");
        hide_popup(app);
    }
    let _ = app.emit("meeting://detection-changed", enabled);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionSettings {
    pub enabled: bool,
    pub apps: Option<MeetingApps>,
}

#[tauri::command]
pub fn meeting_detection_set(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: DetectionSettings,
) -> Result<(), String> {
    if let Some(a) = settings.apps {
        state.meeting.lock().unwrap().apps = a;
    }
    set_detection_enabled(&app, state.inner(), settings.enabled);
    crate::tray::sync_detection_item(&app, settings.enabled);
    Ok(())
}

#[tauri::command]
pub fn meeting_current(state: State<'_, Arc<AppState>>) -> Option<MeetingInfo> {
    state.meeting.lock().unwrap().current.clone()
}

#[tauri::command]
pub fn meeting_snooze(app: AppHandle, state: State<'_, Arc<AppState>>, key: Option<String>) -> Result<(), String> {
    let mut m = state.meeting.lock().unwrap();
    let k = key.or_else(|| m.current.as_ref().map(|c| c.key.clone()));
    if let Some(k) = k {
        m.snoozed.insert(k);
    }
    drop(m);
    hide_popup(&app);
    Ok(())
}

/// Acción del popup: "start" (config opaca que el frontend de main entiende),
/// "ignore" (silenciar esta reunión) o "hide".
#[tauri::command]
pub fn popup_action(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    action: String,
    config: Option<serde_json::Value>,
) -> Result<(), String> {
    match action.as_str() {
        "start" => {
            let meeting = state.meeting.lock().unwrap().current.clone();
            hide_popup(&app);
            show_main(&app);
            let _ = app.emit_to(
                "main",
                "meeting://start-request",
                serde_json::json!({ "config": config, "meeting": meeting }),
            );
            Ok(())
        }
        "ignore" => {
            let mut m = state.meeting.lock().unwrap();
            if let Some(c) = &m.current {
                let k = c.key.clone();
                m.snoozed.insert(k);
            }
            drop(m);
            hide_popup(&app);
            Ok(())
        }
        "hide" => {
            hide_popup(&app);
            Ok(())
        }
        other => Err(format!("Acción desconocida: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AutoStopSettings;

    const NOW: u64 = 1_000_000_000;

    fn base() -> DecideInput<'static> {
        DecideInput {
            now: NOW,
            settings: AutoStopSettings {
                on_meeting_end: true,
                on_silence: true,
                silence_sec: 600,
                max_sec: 14_400,
                grace_sec: 60,
            },
            started_at: NOW - 60_000,
            max_extra_ms: 0,
            last_voice: NOW - 1_000,
            detection_on: true,
            meeting_attached: false,
            meeting_since: 0,
            meeting_app: None,
            meeting_present: false,
            missing_ticks: 0,
        }
    }

    #[test]
    fn no_propone_nada_en_una_sesion_normal() {
        assert_eq!(decide(&base()).1, None);
    }

    #[test]
    fn silencio_dispara_al_cumplirse_el_plazo() {
        let mut i = base();
        i.last_voice = NOW - 599_000;
        assert_eq!(decide(&i).1, None, "aún no se cumple el plazo");
        i.last_voice = NOW - 600_000;
        assert_eq!(decide(&i).1, Some(StopReason::Silence));
    }

    #[test]
    fn silencio_no_dispara_dentro_de_una_reunion_en_curso() {
        // Caso real: sesión de solo micro mientras escuchas la llamada.
        let mut i = base();
        i.last_voice = NOW - 3_600_000;
        i.meeting_attached = true;
        i.meeting_app = Some("teams");
        i.meeting_since = NOW - 600_000;
        i.meeting_present = true;
        assert_eq!(decide(&i).1, None);
    }

    #[test]
    fn silencio_desactivado_no_dispara() {
        let mut i = base();
        i.last_voice = NOW - 3_600_000;
        i.settings.on_silence = false;
        assert_eq!(decide(&i).1, None);
    }

    #[test]
    fn tope_duro_dispara_aunque_haya_voz_y_reunion() {
        let mut i = base();
        i.started_at = NOW - 14_400_000;
        i.meeting_attached = true;
        i.meeting_present = true;
        i.meeting_app = Some("zoom");
        i.meeting_since = NOW - 600_000;
        assert_eq!(decide(&i).1, Some(StopReason::MaxDuration));
    }

    #[test]
    fn la_prorroga_del_tope_duro_se_respeta() {
        let mut i = base();
        i.started_at = NOW - 14_400_000;
        i.max_extra_ms = 30 * 60_000;
        assert_eq!(decide(&i).1, None);
    }

    #[test]
    fn fin_de_reunion_necesita_el_debounce_completo() {
        let mut i = base();
        i.meeting_attached = true;
        i.meeting_app = Some("teams");
        i.meeting_since = NOW - 600_000;
        i.meeting_present = false;
        // Teams: 20 sondeos (silenciarse el micro lo hace desaparecer).
        let mut missing = 0;
        for tick in 1..20 {
            let (m, r) = decide(&DecideInput { missing_ticks: missing, ..i });
            assert_eq!(r, None, "no debe disparar en el sondeo {tick}");
            missing = m;
        }
        let (_, r) = decide(&DecideInput { missing_ticks: missing, ..i });
        assert_eq!(r, Some(StopReason::MeetingEnd));
    }

    #[test]
    fn meet_y_zoom_toleran_menos_espera_que_teams() {
        let mut i = base();
        i.meeting_attached = true;
        i.meeting_app = Some("meet");
        i.meeting_since = NOW - 600_000;
        i.meeting_present = false;
        i.missing_ticks = 4;
        assert_eq!(decide(&i).1, Some(StopReason::MeetingEnd));
    }

    #[test]
    fn ver_la_reunion_reinicia_el_contador_de_ausencias() {
        let mut i = base();
        i.meeting_attached = true;
        i.meeting_app = Some("teams");
        i.meeting_since = NOW - 600_000;
        i.meeting_present = true;
        i.missing_ticks = 19;
        let (missing, reason) = decide(&i);
        assert_eq!(reason, None);
        assert_eq!(missing, 0);
    }

    #[test]
    fn no_hay_fin_de_reunion_antes_del_margen_de_asentamiento() {
        let mut i = base();
        i.meeting_attached = true;
        i.meeting_app = Some("meet");
        i.meeting_since = NOW - 10_000; // recién adjuntada
        i.meeting_present = false;
        i.missing_ticks = 50;
        assert_eq!(decide(&i).1, None);
    }

    #[test]
    fn apagar_la_deteccion_desactiva_la_regla_de_reunion() {
        let mut i = base();
        i.detection_on = false;
        i.meeting_attached = true;
        i.meeting_app = Some("teams");
        i.meeting_since = NOW - 600_000;
        i.missing_ticks = 50;
        assert_eq!(decide(&i).1, None);
    }

    #[test]
    fn todo_desactivado_no_propone_nunca() {
        let mut i = base();
        i.settings.on_meeting_end = false;
        i.settings.on_silence = false;
        i.settings.max_sec = 0;
        i.started_at = NOW - 86_400_000;
        i.last_voice = NOW - 86_400_000;
        assert_eq!(decide(&i).1, None);
    }
}
