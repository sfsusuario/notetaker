import type { Depth, LlmProviderId } from "../../../types";

export interface SystemBlock {
  text: string;
  /** marca de breakpoint de caché (solo lo usa Anthropic) */
  cache?: boolean;
}

export interface TurnContent {
  text: string;
  /** captura de pantalla en base64 PNG, adjunta como bloque de visión */
  imageBase64?: string | null;
}

export interface HistoryTurn {
  role: "user" | "assistant";
  text: string;
}

export interface GenerateArgs {
  model: string;
  depth: Depth;
  system: SystemBlock[];
  history: HistoryTurn[];
  turn: TurnContent;
  onDelta: (text: string) => void;
  signal?: AbortSignal;
  /** solo Ollama: URL base local */
  baseUrl?: string;
}

export interface GenerateResult {
  text: string;
  usage: {
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
  };
  refused: boolean;
}

/**
 * Interfaz común a todos los proveedores. Cada proveedor recibe la misma
 * estructura de contexto (system + history + turn) y hace streaming vía onDelta.
 */
export interface LlmProvider {
  id: LlmProviderId;
  /** clave del secreto en el keyring (undefined = no requiere key, p. ej. Ollama) */
  secretName?: "anthropic" | "openai" | "gemini" | "deepseek" | "kimi";
  /** modelos sugeridos para el selector de la UI */
  models: string[];
  generate(args: GenerateArgs): Promise<GenerateResult>;
  /** valida credenciales/disponibilidad; devuelve un nombre legible */
  test(model: string, baseUrl?: string): Promise<string>;
  /** lista los modelos disponibles del proveedor (requiere clave/servicio) */
  listModels(baseUrl?: string): Promise<string[]>;
}

export const EFFORT: Record<Depth, "low" | "medium" | "high"> = {
  minimal: "low",
  concise: "low",
  standard: "medium",
  detailed: "high",
};

/** Sugerencia de longitud por verbosidad, para proveedores sin `effort`. */
export const DEPTH_MAX_TOKENS: Record<Depth, number> = {
  minimal: 200,
  concise: 512,
  standard: 2048,
  detailed: 8192,
};
