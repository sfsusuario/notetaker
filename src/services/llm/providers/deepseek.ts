import { secretGet } from "../../ipc/native";
import { streamSse } from "./sse";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { DEPTH_MAX_TOKENS } from "./types";

// DeepSeek expone una API compatible con OpenAI.
const BASE = "https://api.deepseek.com/v1";

async function key(): Promise<string> {
  const k = await secretGet("deepseek");
  if (!k) throw new Error("Falta la API key de DeepSeek.");
  return k;
}

export const deepseekProvider: LlmProvider = {
  id: "deepseek",
  secretName: "deepseek",
  models: ["deepseek-chat", "deepseek-reasoner"],

  async test(model) {
    const res = await fetch(`${BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${await key()}`,
      },
      body: JSON.stringify({
        model,
        max_tokens: 1,
        messages: [{ role: "user", content: "ping" }],
      }),
    });
    if (!res.ok) throw new Error(`DeepSeek ${res.status}: ${await res.text()}`);
    return model;
  },

  async listModels() {
    const res = await fetch(`${BASE}/models`, {
      headers: { Authorization: `Bearer ${await key()}` },
    });
    if (!res.ok) throw new Error(`DeepSeek ${res.status}: ${await res.text()}`);
    const json = await res.json();
    return (json.data ?? []).map((m: { id: string }) => m.id).sort();
  },

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    // DeepSeek chat es solo texto: la captura de pantalla se ignora.
    const systemText = args.system.map((b) => b.text).join("\n\n");
    const messages = [
      { role: "system", content: systemText },
      ...args.history.map((h) => ({ role: h.role, content: h.text })),
      { role: "user", content: args.turn.text },
    ];

    const res = await fetch(`${BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${await key()}`,
      },
      body: JSON.stringify({
        model: args.model,
        max_tokens: DEPTH_MAX_TOKENS[args.depth],
        stream: true,
        messages,
      }),
      signal: args.signal,
    });
    if (!res.ok || !res.body) {
      throw new Error(`DeepSeek ${res.status}: ${await res.text()}`);
    }

    let text = "";
    let usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };

    for await (const data of streamSse(res.body)) {
      if (data === "[DONE]") break;
      const json = JSON.parse(data);
      // deepseek-reasoner emite 'reasoning_content' aparte; solo mostramos la
      // respuesta final ('content').
      const delta: string | undefined = json.choices?.[0]?.delta?.content;
      if (delta) {
        text += delta;
        args.onDelta(delta);
      }
      if (json.usage) {
        usage = {
          inputTokens: json.usage.prompt_tokens ?? 0,
          outputTokens: json.usage.completion_tokens ?? 0,
          cacheReadTokens: json.usage.prompt_cache_hit_tokens ?? 0,
        };
      }
    }

    return { text, usage, refused: false };
  },
};
