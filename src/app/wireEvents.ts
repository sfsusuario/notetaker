/**
 * Conecta los eventos nativos (Rust → WebView) con los stores y la DB.
 * Solo la ventana principal escribe en la DB (disciplina de un solo escritor).
 */
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  onAudioError,
  onAudioMetrics,
  onMeetingDetected,
  onMeetingDetectionChanged,
  onMeetingStartRequest,
  onSessionStopped,
  onSttFinal,
  onSttPartial,
  onSttProgress,
  onSttStatus,
  onTrayNewSession,
  onWhisperDownloadProgress,
  onWhisperServer,
} from "../services/ipc/native";
import * as db from "../services/storage/db";
import { useEnginesStore } from "../stores/useEnginesStore";
import { useHistoryStore } from "../stores/useHistoryStore";
import { livePositionMs, useSessionStore } from "../stores/useSessionStore";
import { useSettingsStore } from "../stores/useSettingsStore";
import { useUiStore } from "../stores/useUiStore";
import type { QuickStartConfig } from "../types";
import { finalizeLiveSession, maybeGenerateTitle, startLive } from "./actions";

const TITLE_AFTER_MS = 90_000;
const TITLE_AFTER_CHARS = 900;

export async function wireNativeEvents(): Promise<UnlistenFn[]> {
  const u: UnlistenFn[] = [];

  u.push(
    await onSttPartial((seg) => {
      const live = useSessionStore.getState();
      if (live.sessionId === seg.sessionId) live.setPartial(seg);
    }),
  );

  u.push(
    await onSttFinal((seg) => {
      const live = useSessionStore.getState();
      if (live.sessionId === seg.sessionId) {
        live.addFinal(seg);
        const s = useSessionStore.getState();
        if (!s.titleRequested && useSettingsStore.getState().autoTitle) {
          const chars = s.segments.reduce((n, x) => n + x.text.length, 0);
          if (livePositionMs(s.metrics) >= TITLE_AFTER_MS || chars >= TITLE_AFTER_CHARS) {
            s.markTitleRequested();
            void maybeGenerateTitle(seg.sessionId);
          }
        }
      } else {
        useHistoryStore.getState().appendSegment(seg);
      }
      void db.insertSegment(seg);
    }),
  );

  u.push(
    await onSttStatus((ev) => {
      const live = useSessionStore.getState();
      if (live.sessionId === ev.sessionId) live.setStatus(ev);
    }),
  );

  u.push(await onSttProgress((p) => useHistoryStore.getState().setProgress(p)));

  u.push(
    await onAudioMetrics((m) => {
      const live = useSessionStore.getState();
      if (live.sessionId === m.sessionId) live.setMetrics(m);
    }),
  );

  u.push(
    await onAudioError((e) => {
      useSessionStore.getState().setError(e.message);
      useUiStore.getState().toast(e.message, "error");
    }),
  );

  u.push(
    await onSessionStopped((r) => {
      const live = useSessionStore.getState();
      // stopLive() ya gestiona el cierre normal; aquí solo los cierres
      // iniciados por el backend (Salir de la bandeja, sesión reemplazada).
      if (live.sessionId === r.sessionId && !live.stopping) {
        void finalizeLiveSession(r, r.sessionId);
      }
    }),
  );

  u.push(
    await onMeetingStartRequest(({ config, meeting }) => {
      const c = (config ?? {}) as Partial<QuickStartConfig>;
      useUiStore
        .getState()
        .toast(`Iniciando transcripción de ${meeting?.appLabel ?? "la reunión"}…`);
      void startLive({
        engine: c.engine,
        model: c.model,
        sources: c.sources,
        language: c.language,
      });
    }),
  );

  u.push(
    await onMeetingDetected((m) => {
      if (!useSessionStore.getState().sessionId) {
        useUiStore.getState().toast(`Reunión detectada en ${m.appLabel}`);
      }
    }),
  );

  u.push(
    await onMeetingDetectionChanged((enabled) => {
      useSettingsStore.getState().set({ meetingDetection: enabled });
    }),
  );

  u.push(await onTrayNewSession(() => useUiStore.getState().navigate("new")));

  u.push(
    await onWhisperDownloadProgress((p) => {
      const engines = useEnginesStore.getState();
      engines.setDownload(p);
      if (p.phase === "done" || p.phase === "error") void engines.refresh();
    }),
  );

  u.push(
    await onWhisperServer((e) => {
      useEnginesStore.getState().setServer(e);
      if (e.status === "error" && e.message) {
        useUiStore.getState().toast(e.message, "error");
      }
    }),
  );

  return u;
}
