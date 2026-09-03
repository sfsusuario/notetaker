import { create } from "zustand";
import * as db from "../services/storage/db";
import type { ChatMessage } from "../types";

interface ChatState {
  bySession: Record<string, ChatMessage[]>;
  /** respuesta en streaming (una a la vez) */
  streaming: { sessionId: string; text: string } | null;
  error: string | null;
  load: (sessionId: string) => Promise<void>;
  append: (m: ChatMessage) => void;
  setStreaming: (sessionId: string, text: string) => void;
  endStreaming: () => void;
  setError: (e: string | null) => void;
  clear: (sessionId: string) => Promise<void>;
}

export const useChatStore = create<ChatState>((set) => ({
  bySession: {},
  streaming: null,
  error: null,
  load: async (sessionId) => {
    const list = await db.listChatMessages(sessionId);
    set((s) => ({ bySession: { ...s.bySession, [sessionId]: list } }));
  },
  append: (m) =>
    set((s) => ({
      bySession: {
        ...s.bySession,
        [m.sessionId]: [...(s.bySession[m.sessionId] ?? []), m],
      },
    })),
  setStreaming: (sessionId, text) => set({ streaming: { sessionId, text } }),
  endStreaming: () => set({ streaming: null }),
  setError: (error) => set({ error }),
  clear: async (sessionId) => {
    await db.clearChat(sessionId);
    set((s) => ({ bySession: { ...s.bySession, [sessionId]: [] } }));
  },
}));
