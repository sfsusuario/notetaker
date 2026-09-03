// ── Motores y fuentes ────────────────────────────────────────────────────────

export type EngineId = "deepgram" | "whisper";

/** Fuentes capturables en vivo. */
export type AudioSource = "mic" | "system";

/** Origen de un segmento (en vivo: mic/system; desde archivo: file). */
export type SegmentSource = "mic" | "system" | "file";

/** live = transcribe mientras graba · record = solo graba · file = archivo ya existente */
export type SessionMode = "live" | "record" | "file";

/** recorded = audio guardado y pendiente de transcribir */
export type SessionStatus = "recording" | "processing" | "done" | "error" | "recorded";

/** Motor con el que se transcribió; "none" = grabada sin transcribir todavía. */
export type SessionEngine = EngineId | "none";

export interface Segment {
  id: string;
  sessionId: string;
  source: SegmentSource;
  /** índice de hablante del motor ("0", "1"…) o null si no diariza */
  speaker: string | null;
  text: string;
  /** ms desde el inicio de la fuente = posición en el WAV guardado */
  startMs: number;
  endMs: number;
  /** epoch ms de llegada: clave de orden cronológico entre fuentes */
  receivedAt: number;
  isFinal: boolean;
  language: string | null;
}

export interface Session {
  id: string;
  title: string | null;
  /** true si el título lo generó la IA (se puede regenerar); false si lo editó el usuario */
  titleAuto: boolean;
  createdAt: number;
  endedAt: number | null;
  mode: SessionMode;
  engine: SessionEngine;
  engineModel: string | null;
  sources: SegmentSource[];
  language: string | null;
  sourceFilePath: string | null;
  audioDir: string | null;
  durationMs: number | null;
  status: SessionStatus;
  notes: string | null;
  tags: string[];
  /** solo en listados */
  segmentCount?: number;
}

export interface ChatMessage {
  id: string;
  sessionId: string;
  role: "user" | "assistant";
  content: string;
  createdAt: number;
}

export interface SpeakerOverride {
  speakerKey: string;
  label: string;
}

// ── Eventos nativos ──────────────────────────────────────────────────────────

export type ConnectionStatus =
  | "connected"
  | "degraded"
  | "reconnecting"
  | "disconnected"
  | "loading";

export interface ConnectionState {
  status: ConnectionStatus;
  latencyMs: number;
  retryCount: number;
  message?: string | null;
}

export interface StatusEvent extends ConnectionState {
  sessionId: string;
  source: SegmentSource;
  engine: EngineId;
}

export interface AudioMetrics {
  sessionId: string;
  source: SegmentSource;
  rms: number;
  peak: number;
  droppedFrames: number;
  positionMs: number;
  deviceLabel: string;
}

export interface ProgressEvent {
  sessionId: string;
  phase:
    | "decoding"
    | "uploading"
    | "transcribing"
    | "done"
    | "error"
    | "cancelled";
  percent: number;
  message: string | null;
}

export interface DeviceInfo {
  id: string;
  label: string;
  kind: "input" | "output";
  isDefault: boolean;
}

export interface ModelInfo {
  id: string;
  label: string;
  installed: boolean;
  sizeMb: number;
}

export interface EngineInfo {
  id: EngineId;
  label: string;
  description: string;
  diarization: boolean;
  partials: boolean;
  offline: boolean;
  needsApiKey: boolean;
  ready: boolean;
  readiness: string;
  models: ModelInfo[];
}

export interface WhisperModelStatus extends ModelInfo {
  path: string;
}

export interface WhisperStatus {
  serverInstalled: boolean;
  serverPath: string;
  modelsDir: string;
  releaseTag: string;
  models: WhisperModelStatus[];
  running: { model: string; port: number } | null;
}

export interface DownloadProgress {
  item: string;
  downloaded: number;
  total: number | null;
  percent: number;
  phase: "downloading" | "extracting" | "done" | "error";
  message: string | null;
}

export interface WhisperServerEvent {
  status: "starting" | "ready" | "stopped" | "error";
  model: string;
  port: number | null;
  message: string | null;
}

export interface MeetingInfo {
  key: string;
  app: "teams" | "zoom" | "meet";
  appLabel: string;
  title: string;
}

export interface MeetingApps {
  teams: boolean;
  zoom: boolean;
  meet: boolean;
}

// ── Configuraciones de arranque ─────────────────────────────────────────────

export interface LiveConfig {
  sessionId: string;
  /** null = solo grabar (se transcribe después desde el historial) */
  engine: EngineId | null;
  model?: string | null;
  sources: AudioSource[];
  micDeviceId?: string | null;
  systemDeviceId?: string | null;
  language?: string;
  whisperThreads?: number;
}

export interface FileConfig {
  sessionId: string;
  engine: EngineId;
  model?: string | null;
  path: string;
  language?: string;
  source?: SegmentSource;
  copyAudio?: boolean;
  whisperThreads?: number;
}

/** Ajustes rápidos que el popup envía a la ventana principal. */
export interface QuickStartConfig {
  /** null = solo grabar */
  engine: EngineId | null;
  model: string | null;
  sources: AudioSource[];
  language: string;
}

export interface StartResult {
  sessionId: string;
  audioDir: string;
  sources: Array<{ source: SegmentSource; deviceLabel: string; sampleRate: number }>;
  startedAt: number;
}

export interface StopResult {
  sessionId: string;
  durationMs: number;
  mix: string | null;
  reason: string;
}

export interface RecordingPaths {
  dir: string;
  mix: string | null;
  mic: string | null;
  system: string | null;
  file: string | null;
  durationMs: number | null;
}

// ── LLM ─────────────────────────────────────────────────────────────────────

export type Depth = "minimal" | "concise" | "standard" | "detailed";

export type LlmProviderId =
  | "anthropic"
  | "openai"
  | "gemini"
  | "deepseek"
  | "kimi"
  | "ollama";

export interface ProviderConfig {
  provider: LlmProviderId;
  model: string;
  /** Solo Ollama: URL base local (p. ej. http://localhost:11434) */
  baseUrl?: string;
}

export const LANGUAGES: ReadonlyArray<readonly [string, string]> = [
  ["auto", "Automático"],
  ["es", "Español"],
  ["en", "Inglés"],
  ["pt", "Portugués"],
  ["fr", "Francés"],
  ["de", "Alemán"],
  ["it", "Italiano"],
  ["ca", "Catalán"],
  ["ja", "Japonés"],
  ["zh", "Chino"],
] as const;
