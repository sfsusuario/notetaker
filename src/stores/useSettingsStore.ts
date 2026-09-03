import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  AudioSource,
  EngineId,
  MeetingApps,
  ProviderConfig,
} from "../types";

export type Theme = "dark" | "light" | "system";
export type ExportFormat = "md" | "txt" | "json";

export interface SettingsValues {
  llm: ProviderConfig;
  defaultEngine: EngineId;
  whisperModel: string;
  whisperThreads: number;
  defaultSources: AudioSource[];
  micDeviceId: string | null;
  systemDeviceId: string | null;
  language: string;
  meetingDetection: boolean;
  meetingApps: MeetingApps;
  startMinimized: boolean;
  theme: Theme;
  exportFormat: ExportFormat;
  autoTitle: boolean;
  /** panel de chat abierto en la vista en vivo */
  liveChatOpen: boolean;
  /** panel de chat abierto en el detalle de sesión */
  sessionChatOpen: boolean;
  /** último tamaño/posición de la ventana (píxeles físicos) */
  windowBounds: { x: number; y: number; w: number; h: number } | null;
  windowMaximized: boolean;
}

interface SettingsState extends SettingsValues {
  set: (partial: Partial<SettingsValues>) => void;
  setLlm: (partial: Partial<ProviderConfig>) => void;
}

const DEFAULTS: SettingsValues = {
  llm: { provider: "gemini", model: "gemini-2.5-flash" },
  defaultEngine: "deepgram",
  whisperModel: "base",
  whisperThreads: Math.max(2, Math.min(8, (navigator.hardwareConcurrency || 4) - 1)),
  defaultSources: ["mic", "system"],
  micDeviceId: null,
  systemDeviceId: null,
  language: "auto",
  meetingDetection: true,
  meetingApps: { teams: true, zoom: true, meet: true },
  startMinimized: false,
  theme: "dark",
  exportFormat: "md",
  autoTitle: true,
  liveChatOpen: false,
  sessionChatOpen: false,
  windowBounds: null,
  windowMaximized: false,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULTS,
      set: (partial) => set(partial),
      setLlm: (partial) => set((s) => ({ llm: { ...s.llm, ...partial } })),
    }),
    {
      name: "notetaker-settings",
      partialize: (s) => {
        const { set: _s, setLlm: _l, ...rest } = s;
        void _s;
        void _l;
        return rest;
      },
      // Rellena claves nuevas que un estado persistido antiguo no tuviera.
      merge: (persisted, current) => {
        const p = (persisted ?? {}) as Partial<SettingsValues>;
        const llm = { ...current.llm, ...(p.llm ?? {}) };
        if (llm.provider === "gemini" && /^gemini-3-/.test(llm.model)) {
          llm.model = "gemini-2.5-flash";
        }
        return {
          ...current,
          ...p,
          llm,
          meetingApps: { ...current.meetingApps, ...(p.meetingApps ?? {}) },
        };
      },
    },
  ),
);

export function applyTheme(theme: Theme) {
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
}
