//! Remuestreador mono i16 con estado entre llamadas. Ratio entero (48k→16k)
//! por promedio de bloques (filtro anti-alias barato); otros ratios por
//! interpolación lineal. Suficiente para voz.
pub struct Resampler {
    src: u32,
    dst: u32,
    /// Ratio entero (src / dst) cuando aplica.
    factor: Option<usize>,
    /// Muestras sobrantes que no completaron un bloque (modo entero).
    carry: Vec<i16>,
    /// Posición fraccional relativa a `prev` (modo lineal).
    pos: f64,
    prev: i16,
}

impl Resampler {
    pub fn new(src: u32, dst: u32) -> Self {
        let factor = if src != dst && src % dst == 0 {
            Some((src / dst) as usize)
        } else {
            None
        };
        Self {
            src,
            dst,
            factor,
            carry: Vec::new(),
            pos: 1.0,
            prev: 0,
        }
    }

    pub fn process(&mut self, input: &[i16]) -> Vec<i16> {
        if self.src == self.dst {
            return input.to_vec();
        }
        if let Some(f) = self.factor {
            let mut data = std::mem::take(&mut self.carry);
            data.extend_from_slice(input);
            let full = data.len() / f * f;
            let out: Vec<i16> = data[..full]
                .chunks_exact(f)
                .map(|b| (b.iter().map(|&s| s as i32).sum::<i32>() / f as i32) as i16)
                .collect();
            self.carry = data[full..].to_vec();
            return out;
        }
        // Lineal: samples[0] = prev, samples[1..] = input
        let step = self.src as f64 / self.dst as f64;
        let mut samples = Vec::with_capacity(input.len() + 1);
        samples.push(self.prev);
        samples.extend_from_slice(input);
        let mut out = Vec::with_capacity((input.len() as f64 / step) as usize + 2);
        let mut pos = self.pos;
        while pos + 1.0 < samples.len() as f64 {
            let i = pos as usize;
            let frac = pos - i as f64;
            let a = samples[i] as f64;
            let b = samples[i + 1] as f64;
            out.push((a + (b - a) * frac).round() as i16);
            pos += step;
        }
        let consumed = samples.len() - 1;
        self.pos = pos - consumed as f64;
        self.prev = *samples.last().unwrap_or(&0);
        out
    }
}
