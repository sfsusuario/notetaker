/**
 * Acciones de alto nivel (única puerta de entrada desde la UI, el popup y la
 * bandeja). Coordina comandos nativos, DB y stores.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  meetingDetectionSet,
  recordingPaths,
  sessionPause,
  sessionResume,
  sessionStartLive,
  sessionStatus,
  sessionStop,
  transcribeCancel,
  transcribeFile,
  deleteRecording,
} from "../services/ipc/native";
import { generateTitle } from "../services/llm/chatClient";
import * as db from "../services/storage/db";
import { useEnginesStore } from "../stores/useEnginesStore";
import { useHistoryStore } from "../stores/useHistoryStore";
import { useSessionStore } from "../stores/useSessionStore";
import { applyTheme, useSettingsStore } from "../stores/useSettingsStore";
import { useUiStore } from "../stores/useUiStore";
import type {
  AudioSource,
  EngineId,
  LiveConfig,
  SegmentSource,
  StopResult,
} from "../types";
import { defaultTitle, newId } from "./format";

const toast = (text: string, kind: "info" | "ok" | "error" = "info") =>
  useUiStore.getState().toast(text, kind);

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

// ── Arranque ────────────────────────────────────────────────────────────────

export async function bootstrap(): Promise<void> {
  const settings = useSettingsStore.getState();
  applyTheme(settings.theme);
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => applyTheme(useSettingsStore.getState().theme));

  await syncDetectionToBackend();
  void useEnginesStore.getState().refresh();
  void useHistoryStore.getState().load();

  // Si el backend tiene una sesión huérfana (recarga del WebView), se cierra.
  try {
    const st = await sessionStatus();
    if (st && !useSessionStore.getState().sessionId) {
      await sessionStop();
    }
  } catch {
    /* ignore */
  }

  if (!settings.startMinimized) {
    const w = getCurrentWindow();
    await w.show();
    await w.setFocus();
  }
}

export async function syncDetectionToBackend(): Promise<void> {
  const s = useSettingsStore.getState();
  try {
    await meetingDetectionSet({ enabled: s.meetingDetection, apps: s.meetingApps });
  } catch (e) {
    console.error("meetingDetectionSet", e);
  }
}

// ── Sesión en vivo ──────────────────────────────────────────────────────────

export interface StartLiveOptions {
  engine?: EngineId;
  model?: string | null;
  sources?: AudioSource[];
  language?: string;
  micDeviceId?: string | null;
  systemDeviceId?: string | null;
}

export async function startLive(opts: StartLiveOptions = {}): Promise<boolean> {
  const s = useSettingsStore.getState();
  const engine = opts.engine ?? s.defaultEngine;
  const model =
    opts.model !== undefined ? opts.model : engine === "whisper" ? s.whisperModel : "nova-3";
  const sources = opts.sources ?? s.defaultSources;
  const cfg: LiveConfig = {
    sessionId: newId(),
    engine,
    model,
    sources,
    micDeviceId: opts.micDeviceId !== undefined ? opts.micDeviceId : s.micDeviceId,
    systemDeviceId:
      opts.systemDeviceId !== undefined ? opts.systemDeviceId : s.systemDeviceId,
    language: opts.language ?? s.language,
    whisperThreads: s.whisperThreads,
  };
  if (sources.length === 0) {
    toast("Elige al menos una fuente de audio.", "error");
    return false;
  }
  if (useSessionStore.getState().sessionId) {
    await stopLive();
  }
  try {
    await db.insertSession({
      id: cfg.sessionId,
      mode: "live",
      engine,
      engineModel: model,
      sources,
      language: cfg.language ?? null,
      status: "recording",
    });
    const res = await sessionStartLive(cfg);
    await db.updateSession(cfg.sessionId, { audioDir: res.audioDir });
    useSessionStore.getState().start(cfg.sessionId, cfg, res.startedAt);
    useUiStore.getState().navigate("live");
    void useHistoryStore.getState().load();
    return true;
  } catch (e) {
    await db.deleteSession(cfg.sessionId).catch(() => {});
    toast(errText(e), "error");
    return false;
  }
}

/** Cierra la sesión en DB y navega al detalle. Lo llama stopLive o el evento session://stopped. */
export async function finalizeLiveSession(result: StopResult | null, sessionId: string) {
  const live = useSessionStore.getState();
  const segments = live.sessionId === sessionId ? live.segments : [];
  await db.updateSession(sessionId, {
    endedAt: Date.now(),
    durationMs: result?.durationMs ?? null,
    status: "done",
  });
  live.reset();
  await useHistoryStore.getState().load();
  await openSession(sessionId);
  if (segments.length > 0) void maybeGenerateTitle(sessionId);
}

export async function stopLive(): Promise<void> {
  const live = useSessionStore.getState();
  const sessionId = live.sessionId;
  if (!sessionId || live.stopping) return;
  live.setStopping(true);
  try {
    const result = await sessionStop();
    await finalizeLiveSession(result, sessionId);
  } catch (e) {
    toast(errText(e), "error");
    useSessionStore.getState().setStopping(false);
  }
}

export async function pauseLive(): Promise<void> {
  try {
    await sessionPause();
    useSessionStore.getState().setPaused(true);
  } catch (e) {
    toast(errText(e), "error");
  }
}

export async function resumeLive(): Promise<void> {
  try {
    await sessionResume();
    useSessionStore.getState().setPaused(false);
  } catch (e) {
    toast(errText(e), "error");
  }
}

// ── Desde archivo / retranscripción ─────────────────────────────────────────

export async function startFromFile(
  path: string,
  opts: { engine: EngineId; model: string | null; language: string },
): Promise<void> {
  const s = useSettingsStore.getState();
  const sessionId = newId();
  const name = path.split(/[\\/]/).pop() ?? path;
  await db.insertSession({
    id: sessionId,
    mode: "file",
    engine: opts.engine,
    engineModel: opts.model,
    sources: ["file"],
    language: opts.language,
    sourceFilePath: path,
    status: "processing",
    title: name.replace(/\.[^.]+$/, ""),
  });
  await useHistoryStore.getState().load();
  await openSession(sessionId);
  try {
    const res = await transcribeFile({
      sessionId,
      engine: opts.engine,
      model: opts.model,
      path,
      language: opts.language,
      source: "file",
      copyAudio: true,
      whisperThreads: s.whisperThreads,
    });
    await db.updateSession(sessionId, {
      status: "done",
      endedAt: Date.now(),
      durationMs: res.durationMs,
      audioDir: res.audioDir,
    });
    await useHistoryStore.getState().load();
    await useHistoryStore.getState().refreshCurrent();
    toast("Transcripción completada", "ok");
    void maybeGenerateTitle(sessionId);
  } catch (e) {
    const msg = errText(e);
    await db.updateSession(sessionId, { status: msg === "Cancelado" ? "done" : "error" });
    await useHistoryStore.getState().refreshCurrent();
    toast(msg, msg === "Cancelado" ? "info" : "error");
  }
}

export async function retranscribe(
  sessionId: string,
  opts: { engine: EngineId; model: string | null; language: string },
): Promise<void> {
  const ok = await confirm(
    "Se reemplazará la transcripción actual de esta sesión. ¿Continuar?",
    { title: "Retranscribir", kind: "warning" },
  );
  if (!ok) return;
  const paths = await recordingPaths(sessionId);
  const jobs: Array<{ path: string; source: SegmentSource }> = [];
  if (paths.mic) jobs.push({ path: paths.mic, source: "mic" });
  if (paths.system) jobs.push({ path: paths.system, source: "system" });
  if (jobs.length === 0 && paths.file) jobs.push({ path: paths.file, source: "file" });
  if (jobs.length === 0) {
    toast("Esta sesión no tiene audio guardado.", "error");
    return;
  }
  const s = useSettingsStore.getState();
  await db.deleteSegments(sessionId);
  await db.updateSession(sessionId, {
    status: "processing",
    engine: opts.engine,
    engineModel: opts.model,
    language: opts.language,
  });
  useHistoryStore.getState().replaceSegments(sessionId, []);
  await useHistoryStore.getState().refreshCurrent();
  try {
    for (const j of jobs) {
      await transcribeFile({
        sessionId,
        engine: opts.engine,
        model: opts.model,
        path: j.path,
        language: opts.language,
        source: j.source,
        copyAudio: false,
        whisperThreads: s.whisperThreads,
      });
    }
    await db.updateSession(sessionId, { status: "done" });
    toast("Retranscripción completada", "ok");
  } catch (e) {
    const msg = errText(e);
    await db.updateSession(sessionId, { status: msg === "Cancelado" ? "done" : "error" });
    toast(msg, msg === "Cancelado" ? "info" : "error");
  }
  await useHistoryStore.getState().load();
  await useHistoryStore.getState().refreshCurrent();
}

export async function cancelFileJob(): Promise<void> {
  await transcribeCancel().catch(() => {});
}

// ── Historial ───────────────────────────────────────────────────────────────

export async function openSession(id: string): Promise<void> {
  const cur = await useHistoryStore.getState().open(id);
  if (cur) useUiStore.getState().navigate("session", id);
}

export async function deleteSessionFully(id: string): Promise<boolean> {
  const ok = await confirm("Se eliminará la sesión, su transcripción, el chat y el audio guardado.", {
    title: "Eliminar sesión",
    kind: "warning",
  });
  if (!ok) return false;
  await useHistoryStore.getState().remove(id);
  await deleteRecording(id).catch(() => {});
  if (useUiStore.getState().openSessionId === id) {
    useUiStore.getState().navigate("history");
  }
  return true;
}

// ── Título automático ───────────────────────────────────────────────────────

const titleInFlight = new Set<string>();

export async function maybeGenerateTitle(sessionId: string, force = false): Promise<void> {
  const settings = useSettingsStore.getState();
  if (!force && !settings.autoTitle) return;
  if (titleInFlight.has(sessionId)) return;
  titleInFlight.add(sessionId);
  try {
    const session = await db.getSession(sessionId);
    if (!session) return;
    if (!force && !session.titleAuto) return;
    const live = useSessionStore.getState();
    const segments =
      live.sessionId === sessionId && live.segments.length > 0
        ? live.segments
        : await db.getSegments(sessionId);
    const speakers = await db.listSpeakers(sessionId);
    const title = await generateTitle(segments, speakers);
    if (!title) return;
    await db.setTitle(sessionId, title, true);
    useHistoryStore.getState().patchSession(sessionId, { title, titleAuto: true });
  } catch (e) {
    console.warn("title", e);
    if (force) toast(`No se pudo generar el título: ${errText(e)}`, "error");
  } finally {
    titleInFlight.delete(sessionId);
  }
}

export function ensureTitle(sessionId: string, createdAt: number, current: string | null) {
  if (current) return;
  void db.setTitle(sessionId, defaultTitle(createdAt), true);
}
