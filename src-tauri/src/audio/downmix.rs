/// Convierte buffers intercalados de N canales a mono i16.
pub fn f32_to_mono_i16(data: &[f32], channels: u16) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    data.chunks_exact(ch)
        .map(|frame| {
            let sum: f32 = frame.iter().sum();
            let avg = (sum / ch as f32).clamp(-1.0, 1.0);
            (avg * i16::MAX as f32) as i16
        })
        .collect()
}

pub fn i16_to_mono_i16(data: &[i16], channels: u16) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    data.chunks_exact(ch)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / ch as i32) as i16
        })
        .collect()
}

pub fn u16_to_mono_i16(data: &[u16], channels: u16) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    data.chunks_exact(ch)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32 - 32768).sum();
            (sum / ch as i32) as i16
        })
        .collect()
}

/// RMS y pico normalizados (0.0–1.0) sobre un buffer mono i16.
pub fn rms_peak(samples: &[i16]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum_sq = 0.0f64;
    let mut peak = 0i32;
    for &s in samples {
        let v = s as f64 / i16::MAX as f64;
        sum_sq += v * v;
        peak = peak.max((s as i32).abs());
    }
    let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
    (rms, peak as f32 / i16::MAX as f32)
}
