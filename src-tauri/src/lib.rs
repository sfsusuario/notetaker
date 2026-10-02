mod audio;
mod autostop;
mod diarize;
mod files;
mod meeting;
mod ollama;
mod paths;
mod secrets;
mod session;
mod speaker;
mod state;
mod stt;
mod sysprofile;
mod tray;

use std::sync::Arc;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_sql::{Migration, MigrationKind};

fn migrations() -> Vec<Migration> {
    vec![Migration {
        version: 1,
        description: "create sessions, segments, chat_messages and speakers",
        sql: "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                title_auto INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                ended_at INTEGER,
                mode TEXT NOT NULL,
                engine TEXT NOT NULL,
                engine_model TEXT,
                sources TEXT NOT NULL,
                language TEXT,
                source_file_path TEXT,
                audio_dir TEXT,
                duration_ms INTEGER,
                status TEXT NOT NULL DEFAULT 'recording',
                notes TEXT,
                tags TEXT
            );
            CREATE TABLE IF NOT EXISTS segments (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                source TEXT NOT NULL,
                speaker TEXT,
                text TEXT NOT NULL,
                start_ms INTEGER NOT NULL,
                end_ms INTEGER NOT NULL,
                received_at INTEGER NOT NULL,
                language TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_segments_session ON segments(session_id, received_at);
            CREATE TABLE IF NOT EXISTS chat_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_session ON chat_messages(session_id, created_at);
            CREATE TABLE IF NOT EXISTS speakers (
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                speaker_key TEXT NOT NULL,
                label TEXT NOT NULL,
                PRIMARY KEY (session_id, speaker_key)
            );
        ",
        kind: MigrationKind::Up,
    }]
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install Rustls ring crypto provider");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:notetaker.db", migrations())
                .build(),
        )
        .manage(Arc::new(state::AppState::default()))
        .setup(|app| {
            tray::setup_tray(app.handle())?;
            // Servidores whisper que sobrevivieran a un cierre abrupto anterior.
            stt::whisper::server::kill_stale(app.handle());
            let state = app.state::<Arc<state::AppState>>().inner().clone();
            // Instalaciones anteriores al VAD: el modelo (0.9 MB) se baja en
            // segundo plano y se usa en cuanto se arranque el servidor.
            if stt::whisper::install::server_exe(app.handle()).map(|p| p.exists()).unwrap_or(false) {
                let (handle, http) = (app.handle().clone(), state.http.clone());
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = stt::whisper::install::ensure_vad_model(&handle, &http).await {
                        eprintln!("[whisper] modelo VAD no disponible: {e}");
                    }
                });
            }
            // Identificación de hablantes instalada antes de las etiquetas en
            // vivo: completa la librería en C (~7 MB) en segundo plano.
            {
                let (handle, state) = (app.handle().clone(), state.clone());
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = diarize::complete_install(&handle, &state).await {
                        eprintln!("[diarize] no se pudo completar la instalación: {e}");
                    }
                });
            }
            meeting::spawn_poller(app.handle().clone(), state);
            Ok(())
        })
        // Cerrar la ventana principal la oculta a la bandeja; "Salir" en la
        // bandeja termina la app. El popup también se oculta en vez de morir.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" || window.label() == "popup" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            audio::audio_list_devices,
            stt::engines_list,
            session::live::session_start_live,
            session::live::session_stop,
            session::live::session_pause,
            session::live::session_resume,
            session::live::session_status,
            session::file::transcribe_file,
            session::file::transcribe_cancel,
            stt::whisper::install::whisper_install,
            stt::whisper::install::whisper_status,
            stt::whisper::install::whisper_delete_model,
            stt::whisper::install::whisper_stop_server,
            sysprofile::system_profile,
            ollama::ollama_get,
            ollama::ollama_post,
            ollama::ollama_chat,
            ollama::ollama_cancel,
            diarize::diarize_status,
            diarize::diarize_install,
            diarize::diarize_audio,
            autostop::autostop_set,
            autostop::autostop_pending,
            autostop::autostop_cancel,
            autostop::autostop_stop_now,
            meeting::meeting_detection_set,
            meeting::meeting_current,
            meeting::meeting_snooze,
            meeting::popup_action,
            secrets::secret_set,
            secrets::secret_get,
            secrets::secret_delete,
            files::save_text_file,
            files::read_text_file,
            files::file_size,
            files::open_path,
            files::recording_paths,
            files::delete_recording,
            files::data_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app.try_state::<Arc<state::AppState>>() {
                stt::whisper::server::kill_sync(&state);
            }
        }
    });
}
