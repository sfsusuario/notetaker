import type { Segment, SpeakerOverride } from "../types";

/** Clave estable de hablante para etiquetas/colores. */
export function speakerKey(seg: Pick<Segment, "source" | "speaker">): string {
  if (seg.source === "mic") return "me";
  if (seg.speaker != null && seg.speaker !== "") return `${seg.source}:${seg.speaker}`;
  return seg.source === "file" ? "file" : "others";
}

export function speakerLabel(
  seg: Pick<Segment, "source" | "speaker">,
  overrides: SpeakerOverride[] | Record<string, string> = [],
): string {
  const key = speakerKey(seg);
  const map: Record<string, string> = Array.isArray(overrides)
    ? Object.fromEntries(overrides.map((o) => [o.speakerKey, o.label]))
    : overrides;
  if (map[key]) return map[key];
  if (key === "me") return "Yo";
  if (key === "others") return "Otros";
  if (key === "file") return "Transcripción";
  const n = Number(seg.speaker);
  return Number.isFinite(n) ? `Hablante ${n + 1}` : `Hablante ${seg.speaker}`;
}

export function isMe(seg: Pick<Segment, "source">): boolean {
  return seg.source === "mic";
}

const PALETTE = [
  "bg-sky-500",
  "bg-emerald-500",
  "bg-amber-500",
  "bg-rose-500",
  "bg-violet-500",
  "bg-teal-500",
  "bg-orange-500",
  "bg-fuchsia-500",
];

export function speakerColor(key: string): string {
  if (key === "me") return "bg-indigo-500";
  if (key === "others" || key === "file") return "bg-zinc-500";
  let h = 0;
  for (const c of key) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const n = Number(key.split(":")[1]);
  const idx = Number.isFinite(n) ? n : h;
  return PALETTE[idx % PALETTE.length];
}

export function initials(label: string): string {
  const parts = label.trim().split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return label.slice(0, 2).toUpperCase();
}
