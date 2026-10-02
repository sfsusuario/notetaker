import { diarizeAudio, recordingPaths } from "../services/ipc/native";
import * as db from "../services/storage/db";
import { useEnginesStore } from "../stores/useEnginesStore";
import { useHistoryStore } from "../stores/useHistoryStore";
import { useSettingsStore } from "../stores/useSettingsStore";
import { useUiStore } from "../stores/useUiStore";
import type { Segment, SpeakerTurn } from "../types";

/**
 * Pistas que se analizan: el audio de los demás (en Teams/Zoom llegan todos
 * mezclados por el sistema) y los archivos. El micrófono sigue siendo "Yo".
 */
const TRACKS = ["system", "file"] as const;

/** Un segmento sin solapamiento se asigna al turno más cercano si está a ≤ 1.5 s. */
const NEAR_MS = 1500;

/** Hablante de cada segmento: el turno con más solapamiento en el tiempo. */
export function assignSpeakers(
  segments: Segment[],
  turns: SpeakerTurn[],
): Array<{ id: string; speaker: string | null }> {
  return segments.map((seg) => {
    const end = Math.max(seg.endMs, seg.startMs + 1);
    const overlap = new Map<number, number>();
    let nearest: { gap: number; speaker: number } | null = null;
    for (const t of turns) {
      const o = Math.min(end, t.endMs) - Math.max(seg.startMs, t.startMs);
      if (o > 0) {
        overlap.set(t.speaker, (overlap.get(t.speaker) ?? 0) + o);
      } else {
        const gap = t.endMs <= seg.startMs ? seg.startMs - t.endMs : t.startMs - end;
        if (!nearest || gap < nearest.gap) nearest = { gap, speaker: t.speaker };
      }
    }
    let best: number | null = null;
    let bestOverlap = 0;
    for (const [speaker, o] of overlap) {
      if (o > bestOverlap) {
        best = speaker;
        bestOverlap = o;
      }
    }
    if (best == null && nearest && nearest.gap <= NEAR_MS) best = nearest.speaker;
    return { id: seg.id, speaker: best == null ? null : String(best) };
  });
}

/**
 * Renumera los hablantes del análisis completo para que coincidan con los que
 * ya se veían (etiquetas en vivo o un análisis anterior): "Hablante 2" sigue
 * siendo la misma persona y los nombres puestos a mano se conservan. Los que
 * no casan reciben números nuevos, nunca uno ya usado.
 */
export function alignToCurrent(
  updates: Array<{ id: string; speaker: string | null }>,
  segments: Segment[],
): Array<{ id: string; speaker: string | null }> {
  const byId = new Map(segments.map((s) => [s.id, s]));
  const weight = new Map<string, number>();
  for (const u of updates) {
    const seg = byId.get(u.id);
    if (u.speaker == null || seg?.speaker == null) continue;
    const key = `${u.speaker}|${seg.speaker}`;
    weight.set(key, (weight.get(key) ?? 0) + Math.max(1, seg.endMs - seg.startMs));
  }
  const map = new Map<string, string>();
  const used = new Set<string>();
  for (const [key] of [...weight].sort((a, b) => b[1] - a[1])) {
    const [final, current] = key.split("|");
    if (map.has(final) || used.has(current)) continue;
    map.set(final, current);
    used.add(current);
  }
  const existing = segments.map((s) => Number(s.speaker ?? NaN)).filter((n) => Number.isFinite(n));
  let next = Math.max(-1, ...existing, ...[...used].map(Number)) + 1;
  const finals = [...new Set(updates.map((u) => u.speaker).filter((s): s is string => s != null))];
  for (const f of finals.sort((a, b) => Number(a) - Number(b))) {
    if (!map.has(f)) map.set(f, String(next++));
  }
  return updates.map((u) => ({ id: u.id, speaker: u.speaker == null ? null : map.get(u.speaker) ?? u.speaker }));
}

/** Tras transcribir con whisper, si está activado en Ajustes e instalado. */
export function maybeDiarize(sessionId: string): void {
  if (!useSettingsStore.getState().diarizeAuto) return;
  if (!useEnginesStore.getState().diarize?.installed) return;
  void diarizeSession(sessionId, { auto: true });
}

/**
 * Identifica los hablantes de las pistas "Otros" y de archivo y los guarda en
 * los segmentos ("Hablante 1, 2…"), con la numeración alineada a la que ya
 * había (`alignToCurrent`) para conservar los nombres puestos a mano.
 */
export async function diarizeSession(sessionId: string, opts: { auto?: boolean } = {}): Promise<void> {
  const toast = useUiStore.getState().toast;
  if (!useEnginesStore.getState().diarize?.installed) {
    if (!opts.auto) toast("Instala la identificación de hablantes en Ajustes.", "error");
    return;
  }
  const history = useHistoryStore.getState();
  if (history.diarizing[sessionId]) return;
  history.setDiarizing(sessionId, true);
  try {
    const paths = await recordingPaths(sessionId);
    const segments = await db.getSegments(sessionId);
    let analysed = 0;
    let found = 0;
    for (const source of TRACKS) {
      const path = paths[source];
      const segs = segments.filter((s) => s.source === source);
      if (!path || segs.length === 0) continue;
      const turns = await diarizeAudio(path);
      analysed++;
      const speakers = new Set(turns.map((t) => t.speaker)).size;
      found = Math.max(found, speakers);
      // Con una sola voz se mantiene "Otros"/"Transcripción" en vez de "Hablante 1".
      const updates =
        speakers > 1
          ? alignToCurrent(assignSpeakers(segs, turns), segs)
          : segs.map((s) => ({ id: s.id, speaker: null }));
      await db.setSegmentSpeakers(sessionId, updates);
    }
    if (useHistoryStore.getState().current?.session.id === sessionId) {
      await useHistoryStore.getState().refreshCurrent();
    }
    if (analysed === 0) {
      if (!opts.auto) toast("No hay audio de otros participantes que analizar.", "info");
    } else if (found > 1) {
      toast(`Identificados ${found} hablantes`, "ok");
    } else if (!opts.auto) {
      toast("Solo se detectó una voz.", "info");
    }
  } catch (e) {
    toast(`No se pudieron identificar los hablantes: ${e instanceof Error ? e.message : String(e)}`, "error");
  } finally {
    useHistoryStore.getState().setDiarizing(sessionId, false);
  }
}
