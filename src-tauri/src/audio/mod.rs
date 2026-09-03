pub mod capture;
pub mod decode;
pub mod downmix;
pub mod resample;
pub mod utterance;
pub mod vad;
pub mod wav;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

/// Frecuencia de trabajo de todo el pipeline (whisper la exige; Deepgram la
/// acepta). Los WAV guardados también van a 16 kHz mono i16.
pub const TARGET_RATE: u32 = 16_000;
/// Tamaño de chunk: 100 ms a 16 kHz.
pub const CHUNK_SAMPLES: usize = (TARGET_RATE / 10) as usize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub label: String,
    pub kind: String, // "input" | "output"
    pub is_default: bool,
}

#[tauri::command]
pub fn audio_list_devices() -> Result<Vec<DeviceInfo>, String> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    let default_out = host.default_output_device().and_then(|d| d.name().ok());

    if let Ok(devices) = host.input_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                out.push(DeviceInfo {
                    is_default: default_in.as_deref() == Some(name.as_str()),
                    id: name.clone(),
                    label: name,
                    kind: "input".into(),
                });
            }
        }
    }
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                out.push(DeviceInfo {
                    is_default: default_out.as_deref() == Some(name.as_str()),
                    id: name.clone(),
                    label: name,
                    kind: "output".into(),
                });
            }
        }
    }
    Ok(out)
}
