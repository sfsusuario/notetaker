//! Icono en la bandeja: Abrir, Nueva sesión, Detección de reuniones (check),
//! Salir. Cerrar la ventana principal la oculta; la bandeja es la vía para
//! recuperarla o salir del todo.
use std::sync::{Arc, Mutex};

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Wry,
};

use crate::meeting::show_main;
use crate::state::AppState;

pub struct TrayState {
    pub detection: Mutex<Option<CheckMenuItem<Wry>>>,
}

pub fn sync_detection_item(app: &AppHandle, enabled: bool) {
    if let Some(t) = app.try_state::<TrayState>() {
        if let Some(item) = t.detection.lock().unwrap().as_ref() {
            let _ = item.set_checked(enabled);
        }
    }
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Abrir NoteTaker", true, None::<&str>)?;
    let new_session = MenuItem::with_id(app, "new", "Nueva sesión", true, None::<&str>)?;
    let detection = CheckMenuItem::with_id(app, "detection", "Detección de reuniones", true, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &new_session, &detection, &sep, &quit])?;

    app.manage(TrayState {
        detection: Mutex::new(Some(detection.clone())),
    });

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("NoteTaker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "new" => {
                show_main(app);
                let _ = app.emit_to("main", "tray://new-session", ());
            }
            "detection" => {
                let enabled = app
                    .try_state::<TrayState>()
                    .and_then(|t| t.detection.lock().ok().and_then(|g| g.as_ref().and_then(|i| i.is_checked().ok())))
                    .unwrap_or(true);
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    // Misma ruta que el comando de Ajustes: además de apagar la
                    // detección, desvincula la reunión de la sesión viva.
                    crate::meeting::set_detection_enabled(app, state.inner(), enabled);
                }
            }
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(state) = app.try_state::<Arc<AppState>>() {
                        let state = state.inner().clone();
                        crate::session::live::stop_live(&app, &state, "quit").await;
                    }
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
