import { streamNdjson } from "./sse";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { DEPTH_MAX_TOKENS } from "./types";

const DEFAULT_BASE = "http://localhost:11434";

function base(url?: string): string {
  return (url ?? DEFAULT_BASE).replace(/\/$/, "");
}

/** Ollama es local: no requiere API key (secretName omitido). */
export const ollamaProvider: LlmProvider = {
  id: "ollama",
  models: ["llama3.2", "qwen2.5-coder", "deepseek-coder-v2"],

  async test(model, baseUrl) {
    const res = await fetch(`${base(baseUrl)}/api/tags`);
    if (!res.ok) throw new Error(`Ollama ${res.status}: ¿está corriendo?`);
    const json = await res.json();
    const names: string[] = (json.models ?? []).map(
      (m: { name: string }) => m.name,
    );
    const has = names.some((n) => n === model || n.startsWith(`${model}:`));
    return has ? `${model} (local)` : `Ollama OK — modelos: ${names.join(", ")}`;
  },

  async listModels(baseUrl) {
    const res = await fetch(`${base(baseUrl)}/api/tags`);
    if (!res.ok) throw new Error(`Ollama ${res.status}: ¿está corriendo?`);
    const json = await res.json();
    return (json.models ?? []).map((m: { name: string }) => m.name);
  },

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    const systemText = args.system.map((b) => b.text).join("\n\n");

    interface OllamaMsg {
      role: string;
      content: string;
      images?: string[];
    }
    const messages: OllamaMsg[] = [
      { role: "system", content: systemText },
      ...args.history.map((h) => ({ role: h.role, content: h.text })),
      {
        role: "user",
        content: args.turn.text,
        ...(args.turn.imageBase64 ? { images: [args.turn.imageBase64] } : {}),
      },
    ];

    const res = await fetch(`${base(args.baseUrl)}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: args.model,
        stream: true,
        messages,
        options: { num_predict: DEPTH_MAX_TOKENS[args.depth] },
      }),
      signal: args.signal,
    });
    if (!res.ok || !res.body) {
      throw new Error(`Ollama ${res.status}: ${await res.text()}`);
    }

    let text = "";
    let usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };

    for await (const obj of streamNdjson(res.body)) {
      const json = obj as {
        message?: { content?: string };
        prompt_eval_count?: number;
        eval_count?: number;
        done?: boolean;
      };
      const delta = json.message?.content;
      if (delta) {
        text += delta;
        args.onDelta(delta);
      }
      if (json.done) {
        usage = {
          inputTokens: json.prompt_eval_count ?? 0,
          outputTokens: json.eval_count ?? 0,
          cacheReadTokens: 0,
        };
      }
    }

    return { text, usage, refused: false };
  },
};
