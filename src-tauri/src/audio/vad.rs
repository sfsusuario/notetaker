/// VAD simple por energía con hangover: mantiene la puerta abierta unos
/// chunks tras la última voz para no cortar finales de frase.
pub struct Vad {
    threshold: f32,
    hangover_chunks: u32,
    remaining: u32,
}

impl Vad {
    pub fn new(threshold: f32, hangover_chunks: u32) -> Self {
        Self {
            threshold,
            hangover_chunks,
            remaining: 0,
        }
    }

    pub fn is_voice(&mut self, rms: f32) -> bool {
        if rms >= self.threshold {
            self.remaining = self.hangover_chunks;
            true
        } else if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            false
        }
    }
}

impl Default for Vad {
    fn default() -> Self {
        // Umbral bajo (~0.3% de fondo de escala) para no cortar voz de
        // micrófono con poca ganancia; ~10 chunks de 100 ms = 1 s de hangover.
        Self::new(0.003, 12)
    }
}
