import { create } from "zustand";
import * as db from "../services/storage/db";
import { recordingPaths } from "../services/ipc/native";
import type {
  ProgressEvent,
  RecordingPaths,
  Segment,
  Session,
  SpeakerOverride,
} from "../types";
import { upsertSorted } from "./useSessionStore";

export interface OpenSession {
  session: Session;
  segments: Segment[];
  speakers: SpeakerOverride[];
  paths: RecordingPaths | null;
}

export const PAGE_SIZE = 12;

interface HistoryState {
  sessions: Session[];
  query: string;
  loading: boolean;
  /** página actual (0-based) y total de sesiones que cumplen el filtro */
  page: number;
  total: number;
  current: OpenSession | null;
  /** progreso de transcripción de archivo por sesión */
  progress: Record<string, ProgressEvent>;
  setQuery: (q: string) => void;
  setPage: (p: number) => void;
  load: () => Promise<void>;
  open: (id: string) => Promise<OpenSession | null>;
  close: () => void;
  refreshCurrent: () => Promise<void>;
  rename: (id: string, title: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  appendSegment: (seg: Segment) => void;
  replaceSegments: (sessionId: string, segments: Segment[]) => void;
  setProgress: (p: ProgressEvent) => void;
  setSpeakerLabel: (sessionId: string, key: string, label: string) => Promise<void>;
  patchSession: (id: string, patch: Partial<Session>) => void;
}

export const useHistoryStore = create<HistoryState>((set, get) => ({
  sessions: [],
  query: "",
  loading: false,
  page: 0,
  total: 0,
  current: null,
  progress: {},

  setQuery: (query) => {
    // Al cambiar el filtro se vuelve a la primera página.
    set({ query, page: 0 });
    void get().load();
  },

  setPage: (page) => {
    set({ page: Math.max(0, page) });
    void get().load();
  },

  load: async () => {
    set({ loading: true });
    try {
      const { query, page } = get();
      const [total, sessions] = await Promise.all([
        db.countSessions(query),
        db.listSessions({ query, limit: PAGE_SIZE, offset: page * PAGE_SIZE }),
      ]);
      // Si al borrar se vacía la última página, retrocede una.
      if (sessions.length === 0 && page > 0 && total > 0) {
        set({ page: page - 1, total });
        set({ loading: false });
        await get().load();
        return;
      }
      set({ sessions, total });
    } finally {
      set({ loading: false });
    }
  },

  open: async (id) => {
    const session = await db.getSession(id);
    if (!session) {
      set({ current: null });
      return null;
    }
    const [segments, speakers, paths] = await Promise.all([
      db.getSegments(id),
      db.listSpeakers(id),
      recordingPaths(id).catch(() => null),
    ]);
    const cur = { session, segments, speakers, paths };
    set({ current: cur });
    return cur;
  },

  close: () => set({ current: null }),

  refreshCurrent: async () => {
    const id = get().current?.session.id;
    if (id) await get().open(id);
  },

  rename: async (id, title) => {
    const t = title.trim();
    if (!t) return;
    await db.setTitle(id, t, false);
    get().patchSession(id, { title: t, titleAuto: false });
  },

  remove: async (id) => {
    await db.deleteSession(id);
    set((s) => ({
      sessions: s.sessions.filter((x) => x.id !== id),
      current: s.current?.session.id === id ? null : s.current,
    }));
  },

  appendSegment: (seg) =>
    set((s) => {
      if (!s.current || s.current.session.id !== seg.sessionId) return {};
      return {
        current: { ...s.current, segments: upsertSorted(s.current.segments, seg) },
      };
    }),

  replaceSegments: (sessionId, segments) =>
    set((s) =>
      s.current && s.current.session.id === sessionId
        ? { current: { ...s.current, segments } }
        : {},
    ),

  setProgress: (p) =>
    set((s) => ({ progress: { ...s.progress, [p.sessionId]: p } })),

  setSpeakerLabel: async (sessionId, key, label) => {
    await db.upsertSpeaker(sessionId, key, label.trim());
    set((s) => {
      if (!s.current || s.current.session.id !== sessionId) return {};
      const speakers = s.current.speakers.filter((x) => x.speakerKey !== key);
      speakers.push({ speakerKey: key, label: label.trim() });
      return { current: { ...s.current, speakers } };
    });
  },

  patchSession: (id, patch) =>
    set((s) => ({
      sessions: s.sessions.map((x) => (x.id === id ? { ...x, ...patch } : x)),
      current:
        s.current && s.current.session.id === id
          ? { ...s.current, session: { ...s.current.session, ...patch } }
          : s.current,
    })),
}));
