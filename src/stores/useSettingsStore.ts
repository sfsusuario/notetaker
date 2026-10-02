import { create } from "zustand";
import { persist } from "zustand/middleware";
import type {
  AudioSource,
  EngineId,
  MeetingApps,
  ProviderConfig,
  WhisperAccel,
} from "../types";

/** Hilos lógicos del equipo: más hilos que esto ralentiza whisper. */
const MAX_THREADS = navigator.hardwareConcurrency || 32;

export type Theme = "dark" | "light" | "system";
export type ExportFormat = "md" | "txt" | "json";

export interface SettingsValues {
  llm: ProviderConfig;
  defaultEngine: EngineId;
  whisperModel: string;
  whisperThreads: number;
  whisperAccel: WhisperAccel;
  /** Texto provisional de "Otros" en vivo con whisper (más fluido, más GPU) */
  whisperPartials: boolean;
  /** Identificar hablantes ("Otros", archivos) al terminar una transcripción con whisper */
  diarizeAuto: boolean;
  /** Hablantes provisionales en "Otros" durante la sesión en vivo */
  diarizeLive: boolean;
  defaultSources: AudioSource[];
  micDeviceId: string | null;
  systemDeviceId: string | null;
  language: string;
  meetingDetection: boolean;
  meetingApps: MeetingApps;
  /** Detener al terminar la reunión detectada */
  autoStopOnMeetingEnd: boolean;
  /** Detener tras un rato sin oír nada */
  autoStopOnSilence: boolean;
  autoStopSilenceMin: number;
  /** Tope duro en horas; 0 = desactivado */
  autoStopMaxHours: number;
  /** Segundos de cortesía antes de detener (cuenta atrás cancelable) */
  autoStopGraceSec: number;
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
  // Local y sin API key; "auto" usa el mejor modelo instalado en Ollama.
  llm: { provider: "ollama", model: "auto" },
  defaultEngine: "deepgram",
  whisperModel: "base",
  whisperThreads: Math.max(2, Math.min(8, MAX_THREADS)),
  whisperAccel: "auto",
  whisperPartials: true,
  diarizeAuto: true,
  diarizeLive: true,
  defaultSources: ["mic", "system"],
  micDeviceId: null,
  systemDeviceId: null,
  language: "auto",
  meetingDetection: true,
  meetingApps: { teams: true, zoom: true, meet: true },
  autoStopOnMeetingEnd: true,
  autoStopOnSilence: true,
  autoStopSilenceMin: 10,
  // Más que cualquier reunión real, pero corta la grabación olvidada toda la
  // noche, que es la que llena el disco (~115 MB/h por pista).
  autoStopMaxHours: 4,
  // 60 s: el caso es «me levanté de la mesa»; con 15–30 s no da tiempo a
  // volver y con 2 min se desperdicia grabación de silencio.
  autoStopGraceSec: 60,
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
      version: 1,
      migrate: (persisted, version) => {
        const p = (persisted ?? {}) as Partial<SettingsValues>;
        // v1: Ollama pasa a "auto" (el mejor modelo instalado en cada momento).
        if (version < 1 && p.llm?.provider === "ollama") {
          p.llm = { ...p.llm, model: "auto" };
        }
        return p as SettingsState;
      },
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
          whisperThreads: Math.min(p.whisperThreads ?? current.whisperThreads, MAX_THREADS),
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
