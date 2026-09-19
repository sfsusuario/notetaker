import { create } from "zustand";
import type {
  AudioMetrics,
  AutoStopPending,
  ConnectionState,
  LiveConfig,
  Segment,
  SegmentSource,
  StatusEvent,
} from "../types";

/** Estado de la sesión EN VIVO (solo una a la vez). */
interface SessionState {
  sessionId: string | null;
  config: LiveConfig | null;
  startedAt: number | null;
  segments: Segment[];
  partials: Partial<Record<SegmentSource, Segment | null>>;
  status: Partial<Record<SegmentSource, ConnectionState>>;
  metrics: Partial<Record<SegmentSource, AudioMetrics>>;
  paused: boolean;
  stopping: boolean;
  error: string | null;
  titleRequested: boolean;
  /** Propuesta de parada automática con cuenta atrás (null = ninguna) */
  autoStop: AutoStopPending | null;
  start: (sessionId: string, config: LiveConfig, startedAt: number) => void;
  addFinal: (seg: Segment) => void;
  setPartial: (seg: Segment) => void;
  setStatus: (ev: StatusEvent) => void;
  setMetrics: (m: AudioMetrics) => void;
  setPaused: (paused: boolean) => void;
  setStopping: (stopping: boolean) => void;
  setError: (error: string | null) => void;
  markTitleRequested: () => void;
  setAutoStop: (p: AutoStopPending | null) => void;
  reset: () => void;
}

const EMPTY = {
  sessionId: null,
  config: null,
  startedAt: null,
  segments: [] as Segment[],
  partials: {},
  status: {},
  metrics: {},
  paused: false,
  stopping: false,
  error: null,
  titleRequested: false,
  autoStop: null,
};

/** Inserta/reemplaza por id y mantiene el orden por receivedAt (luego startMs). */
export function upsertSorted(list: Segment[], seg: Segment): Segment[] {
  const idx = list.findIndex((s) => s.id === seg.id);
  const next = idx >= 0 ? [...list.slice(0, idx), seg, ...list.slice(idx + 1)] : [...list, seg];
  next.sort((a, b) => a.receivedAt - b.receivedAt || a.startMs - b.startMs);
  return next;
}

export const useSessionStore = create<SessionState>((set) => ({
  ...EMPTY,
  start: (sessionId, config, startedAt) =>
    set({ ...EMPTY, sessionId, config, startedAt }),
  addFinal: (seg) =>
    set((s) => {
      const partials = { ...s.partials };
      const p = partials[seg.source];
      // Un final de Deepgram reemplaza al parcial de la misma fuente; el
      // "pending" de whisper se limpia con su propio evento vacío.
      if (p && !p.id.endsWith("-pending")) partials[seg.source] = null;
      return { segments: upsertSorted(s.segments, seg), partials };
    }),
  setPartial: (seg) =>
    set((s) => ({
      partials: { ...s.partials, [seg.source]: seg.text ? seg : null },
    })),
  setStatus: (ev) =>
    set((s) => ({
      status: {
        ...s.status,
        [ev.source]: {
          status: ev.status,
          latencyMs: ev.latencyMs,
          retryCount: ev.retryCount,
          message: ev.message,
        },
      },
    })),
  setMetrics: (m) => set((s) => ({ metrics: { ...s.metrics, [m.source]: m } })),
  setPaused: (paused) => set({ paused }),
  setStopping: (stopping) => set({ stopping }),
  setError: (error) => set({ error }),
  markTitleRequested: () => set({ titleRequested: true }),
  setAutoStop: (autoStop) => set({ autoStop }),
  reset: () => set({ ...EMPTY }),
}));

/** Tiempo transcurrido de la sesión = audio escrito (máximo entre fuentes). */
export function livePositionMs(metrics: SessionState["metrics"]): number {
  return Math.max(0, ...Object.values(metrics).map((m) => m?.positionMs ?? 0));
}
