//! Perfil del equipo para el motor local: CPU (núcleos P/E), RAM, GPU,
//! alimentación y la configuración de whisper recomendada para ese hardware.
use serde::Serialize;
use tauri::AppHandle;

use crate::stt::whisper::install;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub dedicated_mb: u64,
    pub shared_mb: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    /// gpu | cpu
    pub accel: &'static str,
    pub threads: u32,
    pub model: &'static str,
    pub notes: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemProfile {
    pub cpu: String,
    pub physical_cores: u32,
    pub logical_cores: u32,
    /// Solo en CPUs híbridas (Intel 12.ª gen. en adelante).
    pub performance_cores: Option<u32>,
    pub efficiency_cores: Option<u32>,
    pub ram_total_gb: f64,
    pub ram_available_gb: f64,
    pub gpus: Vec<GpuInfo>,
    /// None si no hay batería o no se pudo leer.
    pub on_battery: Option<bool>,
    /// none | stale | ready
    pub gpu_backend: String,
    pub recommended: Recommendation,
}

#[tauri::command]
pub async fn system_profile(app: AppHandle) -> SystemProfile {
    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );
    let logical = crate::stt::whisper::server::max_threads();
    let physical = System::physical_core_count().map(|n| n as u32).unwrap_or(logical);
    let (performance_cores, efficiency_cores) = hybrid_cores();
    let gpus = gpus();
    let on_battery = on_battery();
    let gpu_backend = install::gpu_backend_state(&app);
    let recommended = recommend(physical, gpus.first().map(|g| g.name.as_str()), gpu_backend, on_battery);
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    SystemProfile {
        cpu: sys.cpus().first().map(|c| clean_name(c.brand())).unwrap_or_default(),
        physical_cores: physical,
        logical_cores: logical,
        performance_cores,
        efficiency_cores,
        ram_total_gb: sys.total_memory() as f64 / GB,
        ram_available_gb: sys.available_memory() as f64 / GB,
        gpus,
        on_battery,
        gpu_backend: gpu_backend.into(),
        recommended,
    }
}

/// Configuración recomendada, a partir de mediciones reales en un Core Ultra 7
/// 258V + Arc 140V sobre 120 s de voz en español:
/// - GPU (Vulkan) + large-v3-turbo-q8_0: 4.3x tiempo real, mismo texto que
///   turbo completo en CPU (que iba a 1.0x). Los hilos apenas importan.
/// - Solo CPU: small-q5_1 a 4.2x con 8 hilos (8 hilos > 4 en ese chip).
pub fn recommend(
    physical_cores: u32,
    gpu_name: Option<&str>,
    gpu_backend: &str,
    on_battery: Option<bool>,
) -> Recommendation {
    let mut notes = Vec::new();
    let rec = if gpu_backend == "ready" && gpu_name.is_some() {
        notes.push("La GPU transcribe con el modelo de máxima calidad a ~4–5x tiempo real.".to_string());
        Recommendation {
            accel: "gpu",
            threads: physical_cores.clamp(1, 4),
            model: "large-v3-turbo-q8_0",
            notes: Vec::new(),
        }
    } else {
        if let Some(name) = gpu_name {
            notes.push(if gpu_backend == "stale" {
                format!("El backend GPU instalado es de otra versión del servidor: vuelve a ejecutar scripts\\build-whisper-vulkan.ps1 para usar tu {name}.")
            } else {
                format!("Tu {name} puede acelerar whisper ~4x: ejecuta scripts\\build-whisper-vulkan.ps1 (una vez).")
            });
        }
        Recommendation {
            accel: "cpu",
            threads: physical_cores.clamp(1, 8),
            model: if physical_cores >= 6 { "small-q5_1" } else { "base" },
            notes: Vec::new(),
        }
    };
    if on_battery == Some(true) {
        notes.push("Estás en batería: Windows limita la potencia y la transcripción va más lenta que con el cargador.".into());
    }
    Recommendation { notes, ..rec }
}

/// "Intel(R) Core(TM) Ultra 7 258V" → "Intel Core Ultra 7 258V"
fn clean_name(raw: &str) -> String {
    raw.replace("(R)", "")
        .replace("(TM)", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn hybrid_cores() -> (Option<u32>, Option<u32>) {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };
    let mut len: u32 = 0;
    unsafe {
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
    }
    if len == 0 {
        return (None, None);
    }
    // u64 para que el buffer quede alineado para la estructura
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    let ok = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut len,
        )
    };
    if ok.is_err() {
        return (None, None);
    }
    let base = buf.as_ptr() as *const u8;
    let mut classes: Vec<u8> = Vec::new();
    let mut offset = 0usize;
    while offset < len as usize {
        // SAFETY: Windows rellena entradas de tamaño variable (`Size`) dentro de `len`.
        let entry = unsafe { &*(base.add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX) };
        if entry.Size == 0 {
            break;
        }
        if entry.Relationship == RelationProcessorCore {
            classes.push(unsafe { entry.Anonymous.Processor.EfficiencyClass });
        }
        offset += entry.Size as usize;
    }
    let max = classes.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return (None, None); // CPU no híbrida
    }
    let p = classes.iter().filter(|&&c| c == max).count() as u32;
    (Some(p), Some(classes.len() as u32 - p))
}

#[cfg(not(windows))]
fn hybrid_cores() -> (Option<u32>, Option<u32>) {
    (None, None)
}

#[cfg(windows)]
fn gpus() -> Vec<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let mut out = Vec::new();
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let name = clean_name(&String::from_utf16_lossy(&desc.Description[..end]));
        if name.contains("Basic Render") || out.iter().any(|g: &GpuInfo| g.name == name) {
            continue;
        }
        out.push(GpuInfo {
            name,
            dedicated_mb: desc.DedicatedVideoMemory as u64 / (1024 * 1024),
            shared_mb: desc.SharedSystemMemory as u64 / (1024 * 1024),
        });
    }
    out
}

#[cfg(not(windows))]
fn gpus() -> Vec<GpuInfo> {
    Vec::new()
}

#[cfg(windows)]
fn on_battery() -> Option<bool> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    // BatteryFlag 128 = sin batería; ACLineStatus 0 = desenchufado, 255 = desconocido
    if s.BatteryFlag == 128 || s.ACLineStatus == 255 {
        return None;
    }
    Some(s.ACLineStatus == 0)
}

#[cfg(not(windows))]
fn on_battery() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_ready_prefers_turbo_on_gpu() {
        let r = recommend(8, Some("Intel Arc 140V GPU (16GB)"), "ready", Some(false));
        assert_eq!((r.accel, r.model, r.threads), ("gpu", "large-v3-turbo-q8_0", 4));
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn cpu_only_scales_model_with_cores() {
        let big = recommend(8, None, "none", None);
        assert_eq!((big.accel, big.model, big.threads), ("cpu", "small-q5_1", 8));
        let small = recommend(4, None, "none", None);
        assert_eq!((small.model, small.threads), ("base", 4));
    }

    #[test]
    fn suggests_installing_gpu_backend_and_warns_on_battery() {
        let r = recommend(8, Some("Intel Arc 140V GPU (16GB)"), "none", Some(true));
        assert_eq!(r.accel, "cpu");
        assert!(r.notes[0].contains("build-whisper-vulkan.ps1"));
        assert!(r.notes[1].contains("batería"));
    }
}
