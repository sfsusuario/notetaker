//! Ciclo de vida de `whisper-server.exe`: un proceso por app, arrancado en el
//! primer uso y reutilizado (cargar el modelo tarda segundos). Se mata al
//! salir de la app y al cambiar de modelo, hilos o aceleración.
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::install;
use crate::state::AppState;

/// Dónde corre la inferencia. `Auto` = GPU si el backend Vulkan está instalado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accel {
    #[default]
    Auto,
    Gpu,
    Cpu,
}

pub struct WhisperServer {
    child: Child,
    pub port: u16,
    pub model: String,
    pub threads: u32,
    pub accel: Accel,
    /// Arrancado con el modelo VAD (`-vm`): las peticiones pueden pedir `vad`.
    pub vad: bool,
    /// Dispositivo que reporta el propio servidor al cargar (lo rellena el
    /// hilo que drena stderr).
    backend: Arc<Mutex<BackendInfo>>,
}

#[derive(Default)]
struct BackendInfo {
    gpu_name: Option<String>,
    using_gpu: bool,
}

impl WhisperServer {
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn backend_label(&self) -> String {
        let b = self.backend.lock().unwrap();
        match (&b.gpu_name, b.using_gpu) {
            (Some(name), true) => format!("GPU · {name}"),
            (None, true) => "GPU".into(),
            _ => "CPU".into(),
        }
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
    backend: Option<String>,
}

fn emit(app: &AppHandle, status: &'static str, model: &str, port: Option<u16>, message: Option<String>) {
    emit_full(app, status, model, port, message, None);
}

fn emit_full(
    app: &AppHandle,
    status: &'static str,
    model: &str,
    port: Option<u16>,
    message: Option<String>,
    backend: Option<String>,
) {
    let _ = app.emit(
        "whisper://server",
        ServerEvent {
            status,
            model: model.to_string(),
            port,
            message,
            backend,
        },
    );
}

/// Hilos lógicos del equipo: pedir más a ggml solo añade esperas entre hilos
/// (con 20 hilos en 8 núcleos la inferencia es más lenta, no más rápida).
pub fn max_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(8)
}

/// "ggml_vulkan: 0 = Intel(R) Arc(TM) 140V GPU (16GB) (Intel Corporation) | uma: 1 | …"
/// → "Intel Arc 140V GPU (16GB)".
fn parse_vulkan_device(line: &str) -> Option<String> {
    let rest = line.split("ggml_vulkan: 0 = ").nth(1)?;
    let mut name = rest.split(" | ").next()?.trim().to_string();
    // Quita el fabricante final "(Intel Corporation)"
    if name.ends_with(')') {
        if let Some(i) = name.rfind(" (") {
            name.truncate(i);
        }
    }
    Some(name.replace("(R)", "").replace("(TM)", "").split_whitespace().collect::<Vec<_>>().join(" "))
}

fn track_backend(line: &str, info: &Mutex<BackendInfo>) {
    if let Some(name) = parse_vulkan_device(line) {
        info.lock().unwrap().gpu_name = Some(name);
    } else if line.contains("whisper_backend_init_gpu: using") {
        info.lock().unwrap().using_gpu = true;
    }
}

/// Asocia el proceso hijo a un "job object" con KILL_ON_JOB_CLOSE: si la app
/// muere de forma abrupta (cierre forzado, reinicio del servidor de
/// desarrollo), Windows mata también a whisper-server en vez de dejarlo
/// corriendo con el modelo cargado en memoria.
#[cfg(windows)]
pub(crate) fn attach_kill_on_close(child: &Child) {
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
pub(crate) fn attach_kill_on_close(_child: &Child) {}

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

/// Puerto del server en marcha (si lo hay) y si tiene VAD.
pub async fn current(state: &AppState) -> Option<(u16, bool)> {
    let mut guard = state.whisper.lock().await;
    let s = guard.as_mut()?;
    if s.alive() {
        Some((s.port, s.vad))
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
    accel: Accel,
) -> Result<u16, String> {
    let threads = threads.clamp(1, max_threads());
    // Si el modelo VAD llega después (se descarga en segundo plano), el
    // servidor se reinicia una vez para usarlo.
    let vad = install::vad_model(app);
    let mut guard = state.whisper.lock().await;
    if let Some(s) = guard.as_mut() {
        if s.model == model && s.threads == threads && s.accel == accel && s.vad == vad.is_some() && s.alive() {
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
    let gpu = match accel {
        Accel::Cpu => None,
        Accel::Auto | Accel::Gpu => install::gpu_backend_path(app),
    };
    let mut notice = None;
    if accel == Accel::Gpu && gpu.is_none() {
        notice = Some("La aceleración GPU no está instalada para este servidor; se usa la CPU.".to_string());
    }

    let launched = match launch(app, &exe, &bin, &model_path, model, threads, gpu.as_deref(), vad.as_deref()).await {
        Ok(l) => Ok(l),
        // Un driver de GPU que falla al inicializar no debe dejar al usuario
        // sin transcripción: se reintenta en CPU.
        Err(e) if gpu.is_some() => {
            eprintln!("[whisper] el arranque con GPU falló ({e}); reintentando en CPU");
            notice = Some("La GPU falló al arrancar el motor; se usa la CPU.".into());
            launch(app, &exe, &bin, &model_path, model, threads, None, vad.as_deref()).await
        }
        Err(e) => Err(e),
    };
    let (child, port, backend) = match launched {
        Ok(l) => l,
        Err(msg) => {
            emit(app, "error", model, None, Some(msg.clone()));
            return Err(msg);
        }
    };

    let server = WhisperServer {
        child,
        port,
        model: model.to_string(),
        threads,
        accel,
        vad: vad.is_some(),
        backend,
    };
    let backend_label = server.backend_label();
    *guard = Some(server);
    // Tras soltar el lock: la UI pide whisper_status al recibir "ready".
    drop(guard);
    emit_full(app, "ready", model, Some(port), notice, Some(backend_label));
    Ok(port)
}

type Launched = (Child, u16, Arc<Mutex<BackendInfo>>);

/// Lanza el proceso y espera a que acepte conexiones.
async fn launch(
    app: &AppHandle,
    exe: &Path,
    bin: &Path,
    model_path: &Path,
    model: &str,
    threads: u32,
    gpu_dll: Option<&Path>,
    vad_model: Option<&Path>,
) -> Result<Launched, String> {
    let port = free_port();
    emit(app, "starting", model, Some(port), None);

    let mut cmd = Command::new(exe);
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
    .current_dir(bin)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    if let Some(vad) = vad_model {
        cmd.arg("-vm").arg(vad);
    }
    match gpu_dll {
        Some(dll) => {
            cmd.env("GGML_BACKEND_PATH", dll)
                // Las matrices cooperativas (coopmat) del driver Intel Arc dan
                // resultados corruptos: con ellas small/turbo devolvieron
                // texto inventado (WER 47–91 %, distinto en cada pasada); sin
                // ellas, el mismo texto que en CPU (turbo 1.3 %) a 4x la
                // velocidad. Medido con el driver 32.0.101.6326 (Arc 140V).
                .env("GGML_VK_DISABLE_COOPMAT", "1");
        }
        None => {
            cmd.env_remove("GGML_BACKEND_PATH").arg("-ng");
        }
    }
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

    // Drenar stderr a la consola (si no, el pipe se llena y el proceso se
    // bloquea) y anotar qué dispositivo usa.
    let backend = Arc::new(Mutex::new(BackendInfo::default()));
    if let Some(stderr) = child.stderr.take() {
        let backend = backend.clone();
        std::thread::Builder::new()
            .name("whisper-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    track_backend(&line, &backend);
                    eprintln!("[whisper-server] {line}");
                }
            })
            .ok();
    }

    // Espera a que escuche (carga del modelo: tiny/base ~2 s, medium >10 s;
    // con GPU la primera vez se suman unos segundos de compilar shaders)
    let started = Instant::now();
    let deadline = Duration::from_secs(180);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "whisper-server terminó al arrancar (código {status}). Revisa que el modelo no esté corrupto."
            ));
        }
        if TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_millis(200)).is_ok() {
            break;
        }
        if started.elapsed() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("whisper-server no respondió a tiempo".to_string());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok((child, port, backend))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vulkan_device_name() {
        let line = "ggml_vulkan: 0 = Intel(R) Arc(TM) 140V GPU (16GB) (Intel Corporation) | uma: 1 | fp16: 1 | bf16: 0";
        assert_eq!(parse_vulkan_device(line).as_deref(), Some("Intel Arc 140V GPU (16GB)"));
        assert_eq!(parse_vulkan_device("load_backend: loaded CPU backend"), None);
    }

    #[test]
    fn backend_label_requires_gpu_in_use() {
        let info = Mutex::new(BackendInfo::default());
        track_backend("ggml_vulkan: 0 = Intel(R) Arc(TM) 140V GPU (16GB) (Intel Corporation) | uma: 1", &info);
        assert!(!info.lock().unwrap().using_gpu, "detectar la GPU no implica usarla (-ng)");
        track_backend("whisper_backend_init_gpu: using Vulkan0 backend", &info);
        assert!(info.lock().unwrap().using_gpu);
    }
}
