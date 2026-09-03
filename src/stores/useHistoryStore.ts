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

interface HistoryState {
  sessions: Session[];
  query: string;
  loading: boolean;
  current: OpenSession | null;
  /** progreso de transcripción de archivo por sesión */
  progress: Record<string, ProgressEvent>;
  setQuery: (q: string) => void;
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
  current: null,
  progress: {},

  setQuery: (query) => {
    set({ query });
    void get().load();
  },

  load: async () => {
    set({ loading: true });
    try {
      const sessions = await db.listSessions({ query: get().query });
      set({ sessions });
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
