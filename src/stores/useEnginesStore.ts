import { create } from "zustand";
import { diarizeStatus, enginesList, whisperStatus } from "../services/ipc/native";
import type {
  DiarizeStatus,
  DownloadProgress,
  EngineInfo,
  WhisperServerEvent,
  WhisperStatus,
} from "../types";

interface EnginesState {
  engines: EngineInfo[];
  whisper: WhisperStatus | null;
  /** Identificación de hablantes local (sherpa-onnx) */
  diarize: DiarizeStatus | null;
  downloads: Record<string, DownloadProgress>;
  server: WhisperServerEvent | null;
  loaded: boolean;
  refresh: () => Promise<void>;
  setDownload: (p: DownloadProgress) => void;
  setServer: (e: WhisperServerEvent) => void;
}

export const useEnginesStore = create<EnginesState>((set) => ({
  engines: [],
  whisper: null,
  diarize: null,
  downloads: {},
  server: null,
  loaded: false,
  refresh: async () => {
    try {
      const [engines, whisper, diarize] = await Promise.all([enginesList(), whisperStatus(), diarizeStatus()]);
      set({ engines, whisper, diarize, loaded: true });
    } catch (e) {
      console.error("engines refresh", e);
    }
  },
  setDownload: (p) =>
    set((s) => {
      const downloads = { ...s.downloads, [p.item]: p };
      if (p.phase === "done") {
        // deja el 100 % visible un instante y luego lo retira
        setTimeout(() => {
          set((s2) => {
            const d = { ...s2.downloads };
            if (d[p.item]?.phase === "done") delete d[p.item];
            return { downloads: d };
          });
        }, 1500);
      }
      return { downloads };
    }),
  setServer: (server) => set({ server }),
}));
