import { fmtMs } from "../../app/format";
import { speakerLabel } from "../../app/speakers";
import type { Segment, SpeakerOverride } from "../../types";

/** Límite aproximado de caracteres de transcripción enviados al modelo. */
export const TRANSCRIPT_CHAR_LIMIT = 60_000;

/** "[mm:ss] Yo: …" una línea por segmento, fusionando consecutivos del mismo hablante. */
export function formatTranscript(
  segments: Segment[],
  speakers: SpeakerOverride[] = [],
): string {
  const lines: string[] = [];
  let lastLabel: string | null = null;
  let buf: string[] = [];
  let bufStart = 0;
  const flush = () => {
    if (lastLabel !== null && buf.length) {
      lines.push(`[${fmtMs(bufStart)}] ${lastLabel}: ${buf.join(" ")}`);
    }
    buf = [];
  };
  for (const seg of segments) {
    const label = speakerLabel(seg, speakers);
    if (label !== lastLabel) {
      flush();
      lastLabel = label;
      bufStart = seg.startMs;
    }
    buf.push(seg.text.trim());
  }
  flush();
  return lines.join("\n");
}

/** Recorta al final si excede el límite (lo más reciente es lo más útil). */
export function clampTranscript(text: string): { text: string; truncated: boolean } {
  if (text.length <= TRANSCRIPT_CHAR_LIMIT) return { text, truncated: false };
  const cut = text.slice(text.length - TRANSCRIPT_CHAR_LIMIT);
  const nl = cut.indexOf("\n");
  return { text: nl > 0 ? cut.slice(nl + 1) : cut, truncated: true };
}

export function systemQa(transcript: string, truncated: boolean, meta: {
  title?: string | null;
  date?: string;
  live?: boolean;
}): string {
  return [
    "Eres un asistente que ayuda a entender y explotar la transcripción de una reunión.",
    "Responde SIEMPRE en el idioma en que te pregunta el usuario, de forma clara y concreta.",
    "Basa tus respuestas únicamente en la transcripción. Si algo no aparece, dilo explícitamente.",
    "Cuando cites algo, indica el hablante y el minuto entre corchetes, p. ej. [12:34].",
    "Usa Markdown ligero: listas para acciones/decisiones, negrita para lo importante. Sin preámbulos.",
    "Los hablantes se identifican así: \"Yo\" es la persona que usa esta app (su micrófono); \"Otros\"/\"Hablante N\" son el resto de participantes.",
    meta.live
      ? "La reunión sigue en curso: la transcripción está incompleta y puede contener errores de reconocimiento."
      : "",
    truncated
      ? "NOTA: por longitud, solo se incluye la parte final de la transcripción."
      : "",
    "",
    `Título: ${meta.title ?? "(sin título)"}${meta.date ? ` · Fecha: ${meta.date}` : ""}`,
    "",
    "=== TRANSCRIPCIÓN ===",
    transcript || "(vacía)",
    "=== FIN ===",
  ]
    .filter((l) => l !== "")
    .join("\n");
}

export const TITLE_SYSTEM =
  "Generas títulos breves para reuniones a partir de su transcripción. Responde SOLO con el título: máximo 8 palabras, sin comillas, sin punto final, sin explicaciones, en el idioma predominante de la transcripción. Si no hay contenido suficiente, responde: Reunión sin tema claro";

export function titleTurn(transcript: string): string {
  return `Transcripción (fragmento):\n\n${transcript.slice(0, 6000)}\n\nTítulo:`;
}

export const SUMMARY_PROMPT =
  "Haz un resumen ejecutivo de la reunión con estas secciones en Markdown: **Resumen** (3–5 frases), **Decisiones**, **Acciones** (con responsable si se menciona), **Temas pendientes**. Sé fiel a la transcripción.";
