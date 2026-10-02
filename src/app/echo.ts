/**
 * Eco: con altavoces (sin auriculares) el micrófono también capta a los demás,
 * y "Yo" repetía lo que ya decía "Otros". En una llamada de prueba, 11 de 41
 * frases del micrófono eran eco. Se compara el texto, no el audio: las pistas
 * no comparten línea de tiempo (la del sistema solo avanza cuando suena algo).
 */

/** Ventana (por hora de llegada) en la que se buscan coincidencias. */
export const ECHO_WINDOW_MS = 30_000;
/** Parte de las palabras de "Yo" que deben aparecer en "Otros". */
const ECHO_MIN = 0.6;

function words(text: string): string[] {
  return text
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .split(/[^a-z0-9ñ]+/)
    .filter((w) => w.length >= 3);
}

/** ¿El texto del micrófono repite lo dicho en `others`? Frases de < 3 palabras no se juzgan. */
export function isEcho(micText: string, others: string[]): boolean {
  const mic = words(micText);
  if (mic.length < 3) return false;
  const pool = new Set(others.flatMap(words));
  return mic.filter((w) => pool.has(w)).length / mic.length >= ECHO_MIN;
}
