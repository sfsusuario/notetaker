import { save } from "@tauri-apps/plugin-dialog";
import { fmtDate, fmtDuration, fmtMs } from "../../app/format";
import { speakerLabel } from "../../app/speakers";
import { saveTextFile } from "../ipc/native";
import type { ChatMessage, Segment, Session, SpeakerOverride } from "../../types";
import type { ExportFormat } from "../../stores/useSettingsStore";

export interface ExportInput {
  session: Session;
  segments: Segment[];
  speakers: SpeakerOverride[];
  chat?: ChatMessage[];
}

function title(s: Session): string {
  return s.title ?? `Sesión ${fmtDate(s.createdAt)}`;
}

export function buildMarkdown({ session, segments, speakers, chat }: ExportInput): string {
  const head = [
    `# ${title(session)}`,
    "",
    `- Fecha: ${fmtDate(session.createdAt)}`,
    `- Duración: ${fmtDuration(session.durationMs)}`,
    `- Motor: ${session.engine}${session.engineModel ? ` (${session.engineModel})` : ""}`,
    `- Fuentes: ${session.sources.join(", ") || "—"}`,
    session.notes ? `\n## Notas\n\n${session.notes}` : "",
    "",
    "## Transcripción",
    "",
  ];
  const body = segments.map(
    (s) => `**[${fmtMs(s.startMs)}] ${speakerLabel(s, speakers)}:** ${s.text}`,
  );
  const chatPart =
    chat && chat.length
      ? [
          "",
          "## Chat",
          "",
          ...chat.map((m) => `**${m.role === "user" ? "Tú" : "Asistente"}:** ${m.content}`),
        ]
      : [];
  return [...head, ...body, ...chatPart].join("\n") + "\n";
}

export function buildTxt({ session, segments, speakers }: ExportInput): string {
  const lines = [
    title(session),
    `${fmtDate(session.createdAt)} · ${fmtDuration(session.durationMs)}`,
    "",
    ...segments.map((s) => `[${fmtMs(s.startMs)}] ${speakerLabel(s, speakers)}: ${s.text}`),
  ];
  return lines.join("\n") + "\n";
}

export function buildJson(input: ExportInput): string {
  return JSON.stringify(
    {
      session: input.session,
      speakers: input.speakers,
      segments: input.segments,
      chat: input.chat ?? [],
    },
    null,
    2,
  );
}

function safeName(s: string): string {
  return s.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 60);
}

export async function exportSession(
  input: ExportInput,
  format: ExportFormat,
): Promise<string | null> {
  const filters = {
    md: { name: "Markdown", extensions: ["md"] },
    txt: { name: "Texto", extensions: ["txt"] },
    json: { name: "JSON", extensions: ["json"] },
  }[format];
  const path = await save({
    defaultPath: `${safeName(title(input.session)) || "sesion"}.${format}`,
    filters: [filters],
  });
  if (!path) return null;
  const content =
    format === "md"
      ? buildMarkdown(input)
      : format === "txt"
        ? buildTxt(input)
        : buildJson(input);
  await saveTextFile(path, content);
  return path;
}

/**
 * Copia la transcripción en Markdown al portapapeles. Más ágil que exportar a
 * un archivo cuando solo quieres pegarla en otra herramienta.
 */
export async function copyTranscript(input: ExportInput): Promise<boolean> {
  const text = buildMarkdown(input);
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Reserva por si el portapapeles asíncrono no está disponible en el WebView.
    try {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.cssText = "position:fixed;opacity:0";
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand("copy");
      ta.remove();
      return ok;
    } catch {
      return false;
    }
  }
}
