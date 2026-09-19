use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;

use super::downmix;

/// Trozo de audio mono i16 tal como sale del callback de cpal.
pub type AudioFrame = Vec<i16>;

pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    pub sample_rate: u32,
    pub device_label: String,
    pub dropped_frames: Arc<AtomicU64>,
    /// Aviso si el dispositivo guardado no existía y se usó el predeterminado.
    pub fallback_note: Option<String>,
}

impl CaptureHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Dispositivo elegido y, si el guardado en ajustes ya no existe, el aviso
/// que explica por qué se usó otro.
fn find_device(
    host: &cpal::Host,
    source: &str,
    device_id: &Option<String>,
) -> Result<(cpal::Device, Option<String>), String> {
    fn by_name(
        devices: impl Iterator<Item = cpal::Device>,
        name: &str,
    ) -> Option<cpal::Device> {
        let mut devices = devices;
        devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))
    }
    // Un dispositivo guardado puede desaparecer (auricular Bluetooth que
    // cambia de perfil o se desconecta). Antes eso impedía grabar; ahora se
    // recurre al predeterminado y se avisa.
    let fallback = |dev: Option<cpal::Device>, missing: &str, what: &str| {
        dev.map(|d| {
            let label = d.name().unwrap_or_else(|_| "predeterminado".into());
            (
                d,
                Some(format!(
                    "{what} «{missing}» no está disponible; se usó «{label}» en su lugar."
                )),
            )
        })
    };
    match (source, device_id) {
        // Loopback WASAPI: stream de entrada sobre un dispositivo de SALIDA
        ("system", Some(id)) => host
            .output_devices()
            .ok()
            .and_then(|d| by_name(d, id))
            .map(|d| (d, None))
            .or_else(|| fallback(host.default_output_device(), id, "La salida"))
            .ok_or_else(|| "Sin dispositivo de salida por defecto".to_string()),
        ("system", None) => host
            .default_output_device()
            .map(|d| (d, None))
            .ok_or_else(|| "Sin dispositivo de salida por defecto".to_string()),
        ("mic", Some(id)) => host
            .input_devices()
            .ok()
            .and_then(|d| by_name(d, id))
            .map(|d| (d, None))
            .or_else(|| fallback(host.default_input_device(), id, "El micrófono"))
            .ok_or_else(|| "Sin micrófono por defecto".to_string()),
        ("mic", None) => host
            .default_input_device()
            .map(|d| (d, None))
            .ok_or_else(|| "Sin micrófono por defecto".to_string()),
        (other, _) => Err(format!("Fuente desconocida: {other}")),
    }
}

/// Arranca la captura en un hilo dedicado (cpal::Stream es !Send).
/// Devuelve el handle una vez el stream está construido y sonando.
pub fn start(
    source: String,
    device_id: Option<String>,
    frame_tx: mpsc::Sender<AudioFrame>,
    error_tx: mpsc::Sender<String>,
) -> Result<CaptureHandle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicU64::new(0));
    #[allow(clippy::type_complexity)]
    let (ready_tx, ready_rx) =
        std::sync::mpsc::channel::<Result<(u32, String, Option<String>), String>>();

    let stop_thread = stop.clone();
    let dropped_thread = dropped.clone();

    std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            let host = cpal::default_host();
            let (device, fallback_note) = match find_device(&host, &source, &device_id) {
                Ok(d) => d,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let label = device.name().unwrap_or_else(|_| "desconocido".into());

            let supported = if source == "system" {
                device.default_output_config()
            } else {
                device.default_input_config()
            };
            let supported = match supported {
                Ok(c) => c,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("Sin configuración de audio: {e}")));
                    return;
                }
            };

            let sample_rate = supported.sample_rate().0;
            let channels = supported.channels();
            let sample_format = supported.sample_format();
            eprintln!(
                "[audio] fuente={source} dispositivo='{label}' rate={sample_rate} ch={channels} fmt={sample_format:?}"
            );
            let config: cpal::StreamConfig = supported.into();

            let err_tx = error_tx.clone();
            let err_fn = move |e: cpal::StreamError| {
                let _ = err_tx.try_send(format!("Error del stream de audio: {e}"));
            };

            let make_push = |dropped: Arc<AtomicU64>, tx: mpsc::Sender<AudioFrame>| {
                move |mono: AudioFrame| {
                    if tx.try_send(mono).is_err() {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            };

            let stream = match sample_format {
                cpal::SampleFormat::F32 => {
                    let push = make_push(dropped_thread.clone(), frame_tx.clone());
                    device.build_input_stream(
                        &config,
                        move |data: &[f32], _| push(downmix::f32_to_mono_i16(data, channels)),
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::I16 => {
                    let push = make_push(dropped_thread.clone(), frame_tx.clone());
                    device.build_input_stream(
                        &config,
                        move |data: &[i16], _| push(downmix::i16_to_mono_i16(data, channels)),
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let push = make_push(dropped_thread.clone(), frame_tx.clone());
                    device.build_input_stream(
                        &config,
                        move |data: &[u16], _| push(downmix::u16_to_mono_i16(data, channels)),
                        err_fn,
                        None,
                    )
                }
                other => {
                    let _ = ready_tx.send(Err(format!("Formato no soportado: {other:?}")));
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("No se pudo abrir el stream: {e}")));
                    return;
                }
            };
            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(format!("No se pudo iniciar el stream: {e}")));
                return;
            }
            let _ = ready_tx.send(Ok((sample_rate, label, fallback_note)));

            while !stop_thread.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            drop(stream);
        })
        .map_err(|e| format!("No se pudo crear el hilo de audio: {e}"))?;

    let (sample_rate, device_label, fallback_note) = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Timeout inicializando la captura de audio".to_string())??;

    Ok(CaptureHandle {
        stop,
        sample_rate,
        device_label,
        dropped_frames: dropped,
        fallback_note,
    })
}
