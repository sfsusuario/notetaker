import type { LlmProviderId } from "../../../types";
import { anthropicProvider } from "./anthropic";
import { deepseekProvider } from "./deepseek";
import { geminiProvider } from "./gemini";
import { kimiProvider } from "./kimi";
import { ollamaProvider } from "./ollama";
import { openaiProvider } from "./openai";
import type { LlmProvider } from "./types";

export const PROVIDERS: Record<LlmProviderId, LlmProvider> = {
  anthropic: anthropicProvider,
  openai: openaiProvider,
  gemini: geminiProvider,
  deepseek: deepseekProvider,
  kimi: kimiProvider,
  ollama: ollamaProvider,
};

export const PROVIDER_LABELS: Record<LlmProviderId, string> = {
  gemini: "Gemini (Google)",
  openai: "OpenAI",
  anthropic: "Anthropic (Claude)",
  deepseek: "DeepSeek",
  kimi: "Kimi (Moonshot)",
  ollama: "Ollama (local)",
};

export function getProvider(id: LlmProviderId): LlmProvider {
  return PROVIDERS[id];
}

export type { LlmProvider } from "./types";
