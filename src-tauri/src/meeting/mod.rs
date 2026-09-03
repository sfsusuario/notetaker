//! Detección de reuniones en segundo plano + popup de inicio rápido.
pub mod detector;
pub mod win;

use std::sync::Arc;
use std::time::Duration;

pub use detector::MeetingInfo;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

use crate::state::{AppState, MeetingApps};

const POLL_SECS: u64 = 3;

fn show_popup(app: &AppHandle) {
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
/// baratas; un hilo propio evita bloquear el runtime async.
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
                if !enabled {
                    continue;
                }
                let found = detector::detect(&mut sys, &apps);
                let mut m = state.meeting.lock().unwrap();
                let prev = m.current.clone();
                match (prev, found) {
                    (None, Some(f)) => {
                        let snoozed = m.snoozed.contains(&f.key);
                        m.current = Some(f.clone());
                        drop(m);
                        if !snoozed && !state.is_live() {
                            let _ = app.emit("meeting://detected", &f);
                            show_popup(&app);
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
                            show_popup(&app);
                        } else {
                            hide_popup(&app);
                        }
                    }
                    (Some(p), None) => {
                        m.current = None;
                        m.snoozed.remove(&p.key);
                        drop(m);
                        let _ = app.emit("meeting://ended", &p);
                        hide_popup(&app);
                    }
                    _ => {}
                }
            }
        })
        .expect("meeting poller thread");
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
    {
        let mut m = state.meeting.lock().unwrap();
        m.enabled = settings.enabled;
        if let Some(a) = settings.apps {
            m.apps = a;
        }
        if !settings.enabled {
            m.current = None;
        }
    }
    if !settings.enabled {
        hide_popup(&app);
    }
    crate::tray::sync_detection_item(&app, settings.enabled);
    let _ = app.emit("meeting://detection-changed", settings.enabled);
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
