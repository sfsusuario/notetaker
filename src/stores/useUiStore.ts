import { create } from "zustand";

export type View = "new" | "live" | "history" | "session" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "ok" | "error";
  text: string;
}

interface UiState {
  view: View;
  openSessionId: string | null;
  toasts: Toast[];
  navigate: (view: View, sessionId?: string | null) => void;
  toast: (text: string, kind?: Toast["kind"]) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 1;

export const useUiStore = create<UiState>((set) => ({
  view: "new",
  openSessionId: null,
  toasts: [],
  navigate: (view, sessionId = null) =>
    set((s) => ({
      view,
      openSessionId: view === "session" ? (sessionId ?? s.openSessionId) : s.openSessionId,
    })),
  toast: (text, kind = "info") => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    setTimeout(() => {
      set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
    }, kind === "error" ? 8000 : 4000);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
