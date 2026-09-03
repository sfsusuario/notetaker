import { secretGet } from "../../ipc/native";
import { streamSse } from "./sse";
import type { GenerateArgs, GenerateResult, LlmProvider } from "./types";
import { DEPTH_MAX_TOKENS } from "./types";

const ENDPOINT = "https://api.openai.com/v1/chat/completions";

async function key(): Promise<string> {
  const k = await secretGet("openai");
  if (!k) throw new Error("Falta la API key de OpenAI.");
  return k;
}

export const openaiProvider: LlmProvider = {
  id: "openai",
  secretName: "openai",
  models: ["gpt-5.1", "gpt-5", "gpt-5-mini", "gpt-4o"],

  async test(model) {
    const res = await fetch(ENDPOINT, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${await key()}`,
      },
      body: JSON.stringify({
        model,
        // max_completion_tokens: los modelos gpt-5/o-series rechazan el antiguo
        // max_tokens (por eso "o4-mini" fallaba); gpt-4o acepta ambos.
        max_completion_tokens: 1,
        messages: [{ role: "user", content: "ping" }],
      }),
    });
    if (!res.ok) throw new Error(`OpenAI ${res.status}: ${await res.text()}`);
    return model;
  },

  async listModels() {
    const res = await fetch("https://api.openai.com/v1/models", {
      headers: { Authorization: `Bearer ${await key()}` },
    });
    if (!res.ok) throw new Error(`OpenAI ${res.status}: ${await res.text()}`);
    const json = await res.json();
    return (json.data ?? [])
      .map((m: { id: string }) => m.id)
      .filter((id: string) => /^(gpt|o\d|chatgpt)/.test(id))
      .sort();
  },

  async generate(args: GenerateArgs): Promise<GenerateResult> {
    const systemText = args.system.map((b) => b.text).join("\n\n");
    const userContent: unknown[] = [{ type: "text", text: args.turn.text }];
    if (args.turn.imageBase64) {
      userContent.push({
        type: "image_url",
        image_url: { url: `data:image/png;base64,${args.turn.imageBase64}` },
      });
    }

    const messages = [
      { role: "system", content: systemText },
      ...args.history.map((h) => ({ role: h.role, content: h.text })),
      { role: "user", content: userContent },
    ];

    const res = await fetch(ENDPOINT, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${await key()}`,
      },
      body: JSON.stringify({
        model: args.model,
        max_completion_tokens: DEPTH_MAX_TOKENS[args.depth],
        stream: true,
        stream_options: { include_usage: true },
        messages,
      }),
      signal: args.signal,
    });
    if (!res.ok || !res.body) {
      throw new Error(`OpenAI ${res.status}: ${await res.text()}`);
    }

    let text = "";
    let usage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0 };

    for await (const data of streamSse(res.body)) {
      if (data === "[DONE]") break;
      const json = JSON.parse(data);
      const delta: string | undefined = json.choices?.[0]?.delta?.content;
      if (delta) {
        text += delta;
        args.onDelta(delta);
      }
      if (json.usage) {
        usage = {
          inputTokens: json.usage.prompt_tokens ?? 0,
          outputTokens: json.usage.completion_tokens ?? 0,
          cacheReadTokens:
            json.usage.prompt_tokens_details?.cached_tokens ?? 0,
        };
      }
    }

    return { text, usage, refused: false };
  },
};
