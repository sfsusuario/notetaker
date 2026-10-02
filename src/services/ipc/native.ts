import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AudioMetrics,
  AutoStopPending,
  AutoStopSettingsIpc,
  DeviceInfo,
  DiarizeStatus,
  DownloadProgress,
  EngineInfo,
  FileConfig,
  LiveConfig,
  MeetingApps,
  MeetingInfo,
  ProgressEvent,
  RecordingPaths,
  Segment,
  StartResult,
  StatusEvent,
  SpeakerTurn,
  StopResult,
  SystemProfile,
  WhisperServerEvent,
  WhisperStatus,
} from "../../types";

// ── Comandos ────────────────────────────────────────────────────────────────

export const audioListDevices = () => invoke<DeviceInfo[]>("audio_list_devices");

export const enginesList = () => invoke<EngineInfo[]>("engines_list");

export const sessionStartLive = (cfg: LiveConfig) =>
  invoke<StartResult>("session_start_live", { cfg });
export const sessionStop = () => invoke<StopResult | null>("session_stop");
export const sessionPause = () => invoke<void>("session_pause");
export const sessionResume = () => invoke<void>("session_resume");
export const sessionStatus = () =>
  invoke<{
    sessionId: string;
    engine: string;
    sources: string[];
    paused: boolean;
    startedAt: number;
  } | null>("session_status");

export const transcribeFile = (cfg: FileConfig) =>
  invoke<{
    sessionId: string;
    durationMs: number;
    audioDir: string | null;
    audioPath: string | null;
  }>("transcribe_file", { cfg });
export const transcribeCancel = () => invoke<void>("transcribe_cancel");

export const whisperInstall = (args: { model?: string | null; includeServer?: boolean }) =>
  invoke<WhisperStatus>("whisper_install", {
    model: args.model ?? null,
    includeServer: args.includeServer ?? false,
  });
export const whisperStatus = () => invoke<WhisperStatus>("whisper_status");
export const systemProfile = () => invoke<SystemProfile>("system_profile");
export const diarizeStatus = () => invoke<DiarizeStatus>("diarize_status");
export const diarizeInstall = () => invoke<DiarizeStatus>("diarize_install");
export const diarizeAudio = (path: string) => invoke<SpeakerTurn[]>("diarize_audio", { path });

// Ollama va por Rust: el origen del WebView compilado (http://tauri.localhost)
// recibe 403 de Ollama si se llama con fetch.
export const ollamaGet = <T>(baseUrl: string | undefined, path: "/api/tags" | "/api/ps" | "/api/version") =>
  invoke<T>("ollama_get", { baseUrl: baseUrl ?? null, path });
export const ollamaPost = <T>(baseUrl: string | undefined, path: "/api/show" | "/api/generate", body: unknown) =>
  invoke<T>("ollama_post", { baseUrl: baseUrl ?? null, path, body });

/** POST /api/chat en streaming; resuelve cuando llega el último fragmento. */
export async function ollamaChat(
  baseUrl: string | undefined,
  body: unknown,
  onChunk: (chunk: Record<string, unknown>) => void,
  signal?: AbortSignal,
): Promise<void> {
  const requestId = crypto.randomUUID();
  let ended!: () => void;
  const done = new Promise<void>((resolve) => (ended = resolve));
  const channel = new Channel<Record<string, unknown>>();
  channel.onmessage = (m) => {
    if (m.__end) ended();
    else onChunk(m);
  };
  const abort = () => void invoke("ollama_cancel", { requestId });
  signal?.addEventListener("abort", abort, { once: true });
  try {
    await invoke("ollama_chat", { baseUrl: baseUrl ?? null, requestId, body, onEvent: channel });
    await done;
  } finally {
    signal?.removeEventListener("abort", abort);
  }
}
export const whisperDeleteModel = (model: string) =>
  invoke<WhisperStatus>("whisper_delete_model", { model });
export const whisperStopServer = () => invoke<void>("whisper_stop_server");

export const meetingDetectionSet = (settings: { enabled: boolean; apps?: MeetingApps }) =>
  invoke<void>("meeting_detection_set", { settings });
export const meetingCurrent = () => invoke<MeetingInfo | null>("meeting_current");
export const meetingSnooze = (key?: string) =>
  invoke<void>("meeting_snooze", { key: key ?? null });
export const popupAction = (action: "start" | "ignore" | "hide", config?: unknown) =>
  invoke<void>("popup_action", { action, config: config ?? null });

export const autostopSet = (settings: AutoStopSettingsIpc) =>
  invoke<void>("autostop_set", { settings });
export const autostopPending = () => invoke<AutoStopPending | null>("autostop_pending");
/** «Seguir grabando»: retira la propuesta y evita que vuelva enseguida. */
export const autostopCancel = () => invoke<void>("autostop_cancel");
/** «Detener ahora»: para la sesión como parada manual. */
export const autostopStopNow = () => invoke<void>("autostop_stop_now");

export type SecretName =
  | "deepgram"
  | "anthropic"
  | "openai"
  | "gemini"
  | "deepseek"
  | "kimi";
export const secretSet = (name: SecretName, value: string) =>
  invoke<void>("secret_set", { name, value });
export const secretGet = (name: SecretName) =>
  invoke<string | null>("secret_get", { name });
export const secretDelete = (name: SecretName) =>
  invoke<void>("secret_delete", { name });

export const saveTextFile = (path: string, content: string) =>
  invoke<void>("save_text_file", { path, content });
export const readTextFile = (path: string) =>
  invoke<string>("read_text_file", { path });
export const fileSize = (path: string) => invoke<number>("file_size", { path });
export const openPath = (path: string) => invoke<void>("open_path", { path });
export const recordingPaths = (sessionId: string) =>
  invoke<RecordingPaths>("recording_paths", { sessionId });
export const deleteRecording = (sessionId: string) =>
  invoke<void>("delete_recording", { sessionId });
export const dataDir = () => invoke<string>("data_dir");

// ── Eventos Rust → WebView ──────────────────────────────────────────────────

type Handler<T> = (payload: T) => void;
const on =
  <T>(event: string) =>
  (h: Handler<T>): Promise<UnlistenFn> =>
    listen<T>(event, (e) => h(e.payload));

export const onSttPartial = on<Segment>("stt://partial");
export const onSttFinal = on<Segment>("stt://final");
export const onSttStatus = on<StatusEvent>("stt://status");
export const onSttProgress = on<ProgressEvent>("stt://progress");
export const onAudioMetrics = on<AudioMetrics>("audio://metrics");
export const onAudioError = on<{ message: string }>("audio://error");
/** Aviso informativo (p. ej. una fuente que no capturó audio): no es un fallo. */
export const onAudioWarning = on<{ message: string }>("audio://warning");
export const onSessionStopped = on<StopResult>("session://stopped");
export const onMeetingDetected = on<MeetingInfo>("meeting://detected");
export const onMeetingEnded = on<MeetingInfo>("meeting://ended");
export const onMeetingDetectionChanged = on<boolean>("meeting://detection-changed");
export const onMeetingStartRequest = on<{
  config: unknown;
  meeting: MeetingInfo | null;
}>("meeting://start-request");
export const onTrayNewSession = on<void>("tray://new-session");
export const onAutostopProposed = on<AutoStopPending>("autostop://proposed");
export const onAutostopCancelled = on<{ cause: string }>("autostop://cancelled");
export const onWhisperDownloadProgress = on<DownloadProgress>(
  "whisper://download-progress",
);
export const onWhisperServer = on<WhisperServerEvent>("whisper://server");
