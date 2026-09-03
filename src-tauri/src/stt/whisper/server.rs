//! Ciclo de vida de `whisper-server.exe`: un proceso por app, arrancado en el
//! primer uso y reutilizado (cargar el modelo tarda segundos). Se mata al
//! salir de la app y al cambiar de modelo.
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::install;
use crate::state::AppState;

pub struct WhisperServer {
    child: Child,
    pub port: u16,
    pub model: String,
    pub threads: u32,
}

impl WhisperServer {
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        self.kill();
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ServerEvent {
    /// starting | ready | stopped | error
    status: &'static str,
    model: String,
    port: Option<u16>,
    message: Option<String>,
}

fn emit(app: &AppHandle, status: &'static str, model: &str, port: Option<u16>, message: Option<String>) {
    let _ = app.emit(
        "whisper://server",
        ServerEvent {
            status,
            model: model.to_string(),
            port,
            message,
        },
    );
}

/// Asocia el proceso hijo a un "job object" con KILL_ON_JOB_CLOSE: si la app
/// muere de forma abrupta (cierre forzado, reinicio del servidor de
/// desarrollo), Windows mata también a whisper-server en vez de dejarlo
/// corriendo con el modelo cargado en memoria.
#[cfg(windows)]
fn attach_kill_on_close(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // El HANDLE se guarda como usize porque HANDLE no es Sync.
    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        match CreateJobObjectW(None, windows::core::PCWSTR::null()) {
            Ok(h) => {
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let _ = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                h.0 as usize
            }
            Err(e) => {
                eprintln!("[whisper] no se pudo crear el job object: {e}");
                0
            }
        }
    });
    if job == 0 {
        return;
    }
    unsafe {
        if let Err(e) = AssignProcessToJobObject(
            HANDLE(job as *mut std::ffi::c_void),
            HANDLE(child.as_raw_handle() as *mut std::ffi::c_void),
        ) {
            eprintln!("[whisper] no se pudo asociar el proceso al job: {e}");
        }
    }
}

#[cfg(not(windows))]
fn attach_kill_on_close(_child: &Child) {}

/// Mata servidores whisper de arranques anteriores que quedaran huérfanos.
/// Solo toca binarios dentro de la carpeta de datos de la app.
pub fn kill_stale(app: &AppHandle) {
    let Ok(bin) = install::bin_dir(app) else { return };
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let me = std::process::id();
    for (pid, proc_) in sys.processes() {
        if pid.as_u32() == me {
            continue;
        }
        let name = proc_.name().to_string_lossy().to_lowercase();
        if name != "whisper-server.exe" && name != "whisper-server" {
            continue;
        }
        if proc_.exe().map(|p| p.starts_with(&bin)).unwrap_or(false) {
            eprintln!("[whisper] matando servidor huérfano pid={}", pid.as_u32());
            proc_.kill();
        }
    }
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(8178)
}

/// Puerto del server en marcha (si lo hay).
pub async fn current_port(state: &AppState) -> Option<u16> {
    let mut guard = state.whisper.lock().await;
    let s = guard.as_mut()?;
    if s.alive() {
        Some(s.port)
    } else {
        *guard = None;
        None
    }
}

/// Garantiza un server corriendo con `model`; devuelve el puerto.
pub async fn ensure_running(
    app: &AppHandle,
    state: &Arc<AppState>,
    model: &str,
    threads: u32,
) -> Result<u16, String> {
    let mut guard = state.whisper.lock().await;
    if let Some(s) = guard.as_mut() {
        if s.model == model && s.threads == threads && s.alive() {
            return Ok(s.port);
        }
        s.kill();
        *guard = None;
    }

    let exe = install::server_exe(app)?;
    if !exe.exists() {
        return Err("El motor local no está instalado. Ve a Ajustes → Whisper local e instala el servidor.".into());
    }
    let model_path = install::model_path(app, model)?;
    if !model_path.exists() {
        return Err(format!(
            "El modelo '{model}' no está descargado. Descárgalo en Ajustes → Whisper local."
        ));
    }
    let bin = install::bin_dir(app)?;
    let port = free_port();
    let threads = threads.clamp(1, 32);

    emit(app, "starting", model, Some(port), None);

    let mut cmd = Command::new(&exe);
    cmd.args([
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "-m",
        &model_path.to_string_lossy(),
        "-t",
        &threads.to_string(),
    ])
    .current_dir(&bin)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("No se pudo lanzar whisper-server: {e}"))?;
    attach_kill_on_close(&child);

    // Drenar stderr a la consola (si no, el pipe se llena y el proceso se bloquea)
    if let Some(stderr) = child.stderr.take() {
        std::thread::Builder::new()
            .name("whisper-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    eprintln!("[whisper-server] {line}");
                }
            })
            .ok();
    }

    // Espera a que escuche (carga del modelo: tiny/base ~2 s, medium >10 s)
    let started = Instant::now();
    let deadline = Duration::from_secs(180);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let msg = format!("whisper-server terminó al arrancar (código {status}). Revisa que el modelo no esté corrupto.");
            emit(app, "error", model, None, Some(msg.clone()));
            return Err(msg);
        }
        if TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_millis(200)).is_ok() {
            break;
        }
        if started.elapsed() > deadline {
            let _ = child.kill();
            let msg = "whisper-server no respondió a tiempo".to_string();
            emit(app, "error", model, None, Some(msg.clone()));
            return Err(msg);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    emit(app, "ready", model, Some(port), None);
    *guard = Some(WhisperServer {
        child,
        port,
        model: model.to_string(),
        threads,
    });
    Ok(port)
}

pub async fn stop(state: &AppState) {
    let mut guard = state.whisper.lock().await;
    if let Some(mut s) = guard.take() {
        s.kill();
    }
}

/// Para el server desde un contexto síncrono (salida de la app).
pub fn kill_sync(state: &AppState) {
    match state.whisper.try_lock() {
        Ok(mut guard) => {
            if let Some(mut s) = guard.take() {
                s.kill();
            }
        }
        Err(_) => {
            // Lock ocupado por una inferencia en curso: el Drop del estado no
            // llegará a tiempo, así que se fuerza por nombre de proceso.
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/F", "/IM", "whisper-server.exe"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
}
